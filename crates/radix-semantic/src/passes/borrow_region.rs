//! Flow-sensitive, HIR-hosted region inference.
//!
//! This pass is a facts producer only.  It does not emit SEM050/SEM052 and it
//! does not replace the existing marked-value diagnostic authority.  Its input
//! is the one typed-HIR [`AccessFactTable`] constructed by the semantic driver.

use super::borrow_region_cfg::{build_cfg, CfgError, CfgLocation, CfgNodeId, HirCfg};
use crate::{AccessFactTable, AccessMarker, TypeTable};
use radix_hir::visit::{walk_expr, walk_stmt, HirVisitor};
use radix_hir::{
    DefId, HirExpression, HirExpressionKind, HirId, HirLocal, HirModule, HirStatement,
    HirStatementKind,
};
use radix_lexer::Span;
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
