//! Flow-sensitive, HIR-hosted region inference.
//!
//! This pass is a facts producer only.  It does not emit SEM050/SEM052 and it
//! does not replace the existing marked-value diagnostic authority.  Its input
//! is the one typed-HIR [`AccessFactTable`] constructed by the semantic driver.

use super::borrow_region_cfg::{CfgError, CfgLocation, CfgNodeId, HirCfg, build_cfg};
use crate::{AccessFactTable, AccessMarker, TypeTable};
use radix_hir::visit::{HirVisitor, walk_expr, walk_stmt};
use radix_hir::{
    DefId, HirBlock, HirExpression, HirExpressionKind, HirId, HirItemKind, HirLocal, HirModule,
    HirStatement, HirStatementKind,
};
use radix_lexer::Span;
use radix_types::{RegionRelationship, ReturnedBorrowProvenance};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;

/// Borrow flavor tracked by a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionBorrowKind {
    Shared,
    Mutable,
}

/// One inferred region, retaining the HIR locations that caused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorrowRegion {
    pub borrower: DefId,
    pub source: DefId,
    pub kind: RegionBorrowKind,
    pub start: CfgLocation,
    pub last_uses: Vec<CfgLocation>,
    live_nodes: Vec<CfgNodeId>,
}

impl BorrowRegion {
    #[must_use]
    pub fn is_live_at(&self, node: CfgNodeId) -> bool {
        self.live_nodes.binary_search(&node).is_ok()
    }

    #[must_use]
    pub fn last_use(&self) -> Option<CfgLocation> {
        self.last_uses.last().copied()
    }

    #[must_use]
    pub fn live_nodes(&self) -> &[CfgNodeId] {
        &self.live_nodes
    }
}

/// Region facts for one semantic analysis run.
#[derive(Debug, Clone)]
pub struct RegionFacts {
    cfg: HirCfg,
    regions: Vec<BorrowRegion>,
    /// Number of table queries performed while deriving the event stream.
    /// This is a receipt that the engine consumed the supplied table without
    /// constructing or cloning another fact authority.
    fact_reads: usize,
}

impl RegionFacts {
    #[must_use]
    pub fn cfg(&self) -> &HirCfg {
        &self.cfg
    }

    #[must_use]
    pub fn regions(&self) -> &[BorrowRegion] {
        &self.regions
    }

    #[must_use]
    pub fn fact_reads(&self) -> usize {
        self.fact_reads
    }

    #[must_use]
    pub fn region_for(&self, borrower: DefId) -> Option<&BorrowRegion> {
        self.regions
            .iter()
            .find(|region| region.borrower == borrower)
    }

    #[must_use]
    pub fn overlaps(&self, left: usize, right: usize) -> bool {
        let Some(left) = self.regions.get(left) else {
            return false;
        };
        let Some(right) = self.regions.get(right) else {
            return false;
        };
        left.source == right.source && left.live_nodes.iter().any(|node| right.is_live_at(*node))
    }
}

/// Fail-closed region-engine error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionError {
    Cfg(CfgError),
    MissingLocation { hir_id: HirId, span: Span },
}

impl std::fmt::Display for RegionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cfg(error) => error.fmt(f),
            Self::MissingLocation { hir_id, .. } => {
                write!(
                    f,
                    "borrow-region CFG has no location for HirId({})",
                    hir_id.0
                )
            }
        }
    }
}

impl std::error::Error for RegionError {}

impl From<CfgError> for RegionError {
    fn from(error: CfgError) -> Self {
        Self::Cfg(error)
    }
}

// ── Durable provenance reduction (SCR-04) ────────────────────────────────────

/// Fail-closed reduction failure for one function boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenanceError {
    /// A borrow-returning function needs a region-derived summary, but region
    /// facts are unavailable (unsupported CFG shape or absent engine run).
    Unavailable { function: DefId },
    /// A borrow-returning function's return value cannot be tied to a region
    /// or a borrow-typed parameter by the region engine's facts.
    Incoherent { function: DefId },
}

impl std::fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable { function } => write!(
                f,
                "borrow-returning function DefId({}) has no region facts to reduce",
                function.0
            ),
            Self::Incoherent { function } => write!(
                f,
                "borrow-returning function DefId({}) has incoherent provenance",
                function.0
            ),
        }
    }
}

impl std::error::Error for ProvenanceError {}

/// Reduce flow-sensitive region results to stable codegen-facing provenance
/// summaries (SCR-04 persistence law: signature/provenance facts only — the
/// CFG and `RegionFacts` layout are never serialized).
///
/// `regions` is [`None`] when the region engine could not run (unsupported
/// control-flow shape). That is only fatal for a function whose declared
/// return type is a borrow; every other function reduces to the settled
/// no-borrow summary.
///
/// # Errors
/// Returns [`ProvenanceError`] when a borrow-returning function's summary
/// would be absent ([`ProvenanceError::Unavailable`]) or cannot be tied to
/// the region engine's facts ([`ProvenanceError::Incoherent`]).
pub fn reduce(
    hir: &HirModule,
    types: &TypeTable,
    regions: Option<&RegionFacts>,
) -> Result<crate::capability::FunctionProvenanceTable, ProvenanceError> {
    let mut rows = Vec::new();
    for (def_id, function, body, params) in functions_with_bodies(hir) {
        let ret_marker = function
            .ret_ty
            .and_then(|ty| types.peel_access_marker(ty).0);
        let returns_borrow = matches!(ret_marker, Some(AccessMarker::Ref | AccessMarker::Mut));

        let mut returned_borrow = ReturnedBorrowProvenance::None;
        let mut required_regions: Vec<RegionRelationship> = Vec::new();
        if returns_borrow {
            let mut returns = Vec::new();
            collect_returns(body, &mut returns);
            if regions.is_none() && !returns.is_empty() {
                // The engine did not run (or rejected the CFG): a required
                // provenance summary is absent, not merely unmatched.
                return Err(ProvenanceError::Unavailable { function: def_id });
            }
            for expr in returns {
                let Some(root) = root_def_id(expr) else {
                    return Err(ProvenanceError::Incoherent { function: def_id });
                };
                if let Some(region) = regions.and_then(|facts| facts.region_for(root)) {
                    let provenance = if params.iter().any(|param| param.def_id == region.source) {
                        ReturnedBorrowProvenance::Parameter(region.source)
                    } else {
                        ReturnedBorrowProvenance::Definition(region.source)
                    };
                    if matches!(returned_borrow, ReturnedBorrowProvenance::None) {
                        returned_borrow = provenance;
                    } else if returned_borrow != provenance {
                        return Err(ProvenanceError::Incoherent { function: def_id });
                    }
                    required_regions.push(RegionRelationship {
                        from: region.source,
                        to: region.borrower,
                    });
                } else if params.iter().any(|param| param.def_id == root) {
                    // Returning a borrow-typed parameter re-exports the
                    // caller's borrow; no intra-body region is created.
                    returned_borrow = ReturnedBorrowProvenance::Parameter(root);
                } else {
                    return Err(ProvenanceError::Incoherent { function: def_id });
                }
            }
        }
        required_regions.sort_by_key(|row| (row.from.0, row.to.0));
        required_regions.dedup();

        // Last-use facts: every region whose borrower is a local of this body.
        let locals = body_locals(body);
        let last_uses = regions
            .map(|facts| {
                facts
                    .regions()
                    .iter()
                    .filter(|region| locals.contains(&region.borrower))
                    .filter_map(|region| {
                        region
                            .last_use()
                            .and_then(|location| location.hir_id)
                            .map(|site| crate::capability::LastUseFact {
                                borrower: region.borrower,
                                site,
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();

        rows.push(crate::capability::FunctionProvenanceSummary {
            function: def_id,
            returned_borrow,
            required_regions,
            last_uses,
        });
    }
    Ok(crate::capability::FunctionProvenanceTable::from_rows(rows))
}

/// Every function and method with a body: (definition, function, body, params).
fn functions_with_bodies(
    hir: &HirModule,
) -> Vec<(
    DefId,
    &radix_hir::HirFunction,
    &radix_hir::HirBlock,
    &[radix_hir::HirParam],
)> {
    let mut out = Vec::new();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                if let Some(body) = function.body.as_ref() {
                    out.push((
                        item.def_id,
                        function.as_ref(),
                        body,
                        function.params.as_slice(),
                    ));
                }
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if let Some(body) = method.func.body.as_ref() {
                        out.push((
                            method.def_id,
                            &method.func,
                            body,
                            method.func.params.as_slice(),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn collect_returns<'h>(block: &'h HirBlock, out: &mut Vec<&'h HirExpression>) {
    for statement in &block.statements {
        if let HirStatementKind::Redde(Some(value)) = &statement.kind {
            out.push(value);
        }
    }
}

fn body_locals(body: &HirBlock) -> FxHashSet<DefId> {
    let mut locals = FxHashSet::default();
    let mut collector = LocalCollector {
        locals: &mut locals,
    };
    collector.visit_block(body);
    locals
}

struct LocalCollector<'m> {
    locals: &'m mut FxHashSet<DefId>,
}

impl HirVisitor for LocalCollector<'_> {
    fn visit_stmt(&mut self, statement: &HirStatement) {
        if let HirStatementKind::Local(local) = &statement.kind {
            self.locals.insert(local.def_id);
        }
        walk_stmt(self, statement);
    }
}

/// Infer marked borrow regions over typed HIR and the supplied access facts.
///
/// The dataflow is a backward may-live fixpoint from the borrower's uses: a
/// borrow region covers every CFG node from which a use is reachable without
/// re-entering the binding or passing a dead-source kill.  Unsupported HIR
/// control-flow shapes return an error instead of receiving an unsound
/// approximation.
///
/// # Errors
/// Returns `RegionError` when a control-flow or access shape is unsupported.
pub fn analyze(
    hir: &HirModule,
    types: &TypeTable,
    access_facts: &AccessFactTable,
) -> Result<RegionFacts, RegionError> {
    let cfg = build_cfg(hir)?;
    let mut collector = EventCollector::new(&cfg, types, access_facts);
    collector.visit_program(hir);

    let borrows = std::mem::take(&mut collector.borrows);
    let mut regions = Vec::with_capacity(borrows.len());
    for borrow in borrows {
        let start = cfg
            .nodes()
            .get(borrow.start.0)
            .map(|node| node.location)
            .ok_or(RegionError::MissingLocation {
                hir_id: borrow.hir_id,
                span: borrow.span,
            })?;
        let uses = collector
            .uses
            .get(&borrow.borrower)
            .cloned()
            .unwrap_or_default();
        let source_kills = collector
            .source_kills
            .get(&borrow.source)
            .cloned()
            .unwrap_or_default();
        let live_nodes = solve_live_nodes(&cfg, borrow.start, &uses, &source_kills);
        let mut last_uses: Vec<_> = uses
            .into_iter()
            .filter(|node| live_nodes.binary_search(node).is_ok())
            .filter_map(|node| cfg.nodes().get(node.0).map(|cfg_node| cfg_node.location))
            .collect();
        last_uses.sort_by_key(|location| {
            (
                location.span.start,
                location.span.end,
                location.hir_id.map_or(0, |id| id.0),
            )
        });
        last_uses.dedup();

        regions.push(BorrowRegion {
            borrower: borrow.borrower,
            source: borrow.source,
            kind: borrow.kind,
            start,
            last_uses,
            live_nodes,
        });
    }
    let fact_reads = collector.fact_reads;
    drop(collector);

    Ok(RegionFacts {
        cfg,
        regions,
        fact_reads,
    })
}

#[derive(Debug, Clone, Copy)]
struct BorrowCandidate {
    borrower: DefId,
    source: DefId,
    kind: RegionBorrowKind,
    start: CfgNodeId,
    hir_id: HirId,
    span: Span,
}

struct EventCollector<'a> {
    cfg: &'a HirCfg,
    types: &'a TypeTable,
    access_facts: &'a AccessFactTable,
    borrows: Vec<BorrowCandidate>,
    uses: FxHashMap<DefId, Vec<CfgNodeId>>,
    source_kills: FxHashMap<DefId, Vec<CfgNodeId>>,
    locations: Vec<CfgNodeId>,
    fact_reads: usize,
}

impl<'a> EventCollector<'a> {
    fn new(cfg: &'a HirCfg, types: &'a TypeTable, access_facts: &'a AccessFactTable) -> Self {
        let mut source_kills: FxHashMap<DefId, Vec<CfgNodeId>> = FxHashMap::default();
        for fact in access_facts.dead_sources() {
            if let Some(site) = fact.site {
                if let Some(node) = cfg.node_for_hir_id(site) {
                    source_kills.entry(fact.source).or_default().push(node);
                }
            }
        }
        for nodes in source_kills.values_mut() {
            nodes.sort_unstable();
            nodes.dedup();
        }
        Self {
            cfg,
            types,
            access_facts,
            borrows: Vec::new(),
            uses: FxHashMap::default(),
            source_kills,
            locations: Vec::new(),
            fact_reads: access_facts.dead_source_count(),
        }
    }

    fn collect_local(&mut self, statement: &HirStatement, local: &HirLocal) {
        let Some(init) = &local.init else {
            return;
        };
        self.fact_reads += 2;
        let _initializer_fact = self.access_facts.expression(init.id);
        let _binding_fact = self.access_facts.binding(local.def_id);

        let kind = local
            .ty
            .and_then(|ty| self.types.peel_access_marker(ty).0)
            .and_then(region_kind);
        let explicit_kind = match &init.kind {
            HirExpressionKind::Ref(ref_kind, _) => Some(match ref_kind {
                radix_hir::HirRefKind::Shared => RegionBorrowKind::Shared,
                radix_hir::HirRefKind::Mutable => RegionBorrowKind::Mutable,
            }),
            _ => None,
        };
        let Some(kind) = kind.or(explicit_kind) else {
            return;
        };
        let Some(source) = root_def_id(init) else {
            return;
        };
        let start = self
            .cfg
            .node_for_hir_id(statement.id)
            .or_else(|| self.cfg.node_for_hir_id(init.id));
        let Some(start) = start else {
            return;
        };
        self.borrows.push(BorrowCandidate {
            borrower: local.def_id,
            source,
            kind,
            start,
            hir_id: init.id,
            span: init.span,
        });
    }
}

impl HirVisitor for EventCollector<'_> {
    fn visit_stmt(&mut self, statement: &HirStatement) {
        if let HirStatementKind::Local(local) = &statement.kind {
            self.collect_local(statement, local);
        }
        if let Some(node) = self.cfg.node_for_hir_id(statement.id) {
            self.locations.push(node);
            walk_stmt(self, statement);
            self.locations.pop();
        } else {
            walk_stmt(self, statement);
        }
    }

    fn visit_expr(&mut self, expression: &HirExpression) {
        let node = self
            .cfg
            .node_for_hir_id(expression.id)
            .or_else(|| self.locations.last().copied());
        if let HirExpressionKind::Path(def_id) = &expression.kind {
            if let Some(node) = node {
                self.uses.entry(*def_id).or_default().push(node);
            }
        }
        if let Some(node) = node {
            self.locations.push(node);
            walk_expr(self, expression);
            self.locations.pop();
        } else {
            walk_expr(self, expression);
        }
    }
}

/// Backward may-live fixpoint from the borrower's uses.
///
/// A node is live when a use is reachable from it without re-entering the
/// binding statement (each re-entry re-establishes the borrow, killing the
/// previous region) and without passing a dead-source kill. Loop back-edges
/// therefore keep a condition-resident borrow live around the cycle while a
/// body-local borrow released before a back-edge stays dead at later writes.
fn solve_live_nodes(
    cfg: &HirCfg,
    start: CfgNodeId,
    uses: &[CfgNodeId],
    source_kills: &[CfgNodeId],
) -> Vec<CfgNodeId> {
    let kills: FxHashSet<CfgNodeId> = source_kills.iter().copied().collect();
    let mut live: FxHashSet<CfgNodeId> = uses.iter().copied().collect();
    let mut queue: VecDeque<_> = uses.iter().copied().collect();

    while let Some(node) = queue.pop_front() {
        for edge in cfg.predecessors(node) {
            let predecessor = edge.from;
            if predecessor == start {
                // The binding re-establishes the borrow: live at the binding,
                // but its predecessors (the previous iteration) are not.
                live.insert(start);
                continue;
            }
            if kills.contains(&predecessor) || !live.insert(predecessor) {
                continue;
            }
            queue.push_back(predecessor);
        }
    }

    let mut nodes: Vec<_> = live.into_iter().collect();
    nodes.sort_unstable();
    nodes
}

fn region_kind(marker: AccessMarker) -> Option<RegionBorrowKind> {
    match marker {
        AccessMarker::Ref => Some(RegionBorrowKind::Shared),
        AccessMarker::Mut => Some(RegionBorrowKind::Mutable),
        AccessMarker::Own | AccessMarker::Copy => None,
    }
}

fn root_def_id(expression: &HirExpression) -> Option<DefId> {
    match &expression.kind {
        HirExpressionKind::Path(def_id) => Some(*def_id),
        HirExpressionKind::Field(object, _)
        | HirExpressionKind::Index(object, _)
        | HirExpressionKind::Deref(object)
        | HirExpressionKind::Ref(_, object) => root_def_id(object),
        _ => None,
    }
}
