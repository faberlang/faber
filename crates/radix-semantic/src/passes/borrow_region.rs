//! Flow-sensitive, HIR-hosted region inference.
//!
//! This pass is a facts producer only.  It does not emit SEM050/SEM052 and it
//! does not replace the existing marked-value diagnostic authority.  Its input
//! is the one typed-HIR [`AccessFactTable`] constructed by the semantic driver.

use super::borrow_region_cfg::{CfgError, CfgLocation, CfgNodeId, HirCfg, build_cfg};
use crate::{AccessFactTable, AccessMarker, TypeTable};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr, walk_stmt};
use radix_hir::{
    DefId, HirBlock, HirExpression, HirExpressionKind, HirId, HirItemKind, HirLocal, HirModule,
    HirStatement, HirStatementKind,
};
use radix_intrinsics::lookup_method_for_type_id;
use radix_lexer::Interner;
use radix_lexer::Span;
use radix_types::{RegionRelationship, ReturnedBorrowProvenance};
use std::collections::VecDeque;

/// SEM008 `region_facts_not_built`: the region engine rejected a shape whose
/// borrow decisions it owns, so the pipeline fails closed with a named
/// diagnostic instead of silently keeping the block-scope approximation.
pub const ISSUE_REGION_FACTS_NOT_BUILT: &str = "region_facts_not_built";

/// The named fail-closed diagnostic for one engine gap.
///
/// `construct` names the HIR shape the engine could not cover (the
/// [`CfgError`] variant's own construct, or the missing-location site).
#[must_use]
pub fn region_facts_unavailable_error(construct: &str, span: Span) -> crate::SemanticError {
    crate::SemanticError::structured(radix_diagnostics::SemanticErrorKind::LoweringError, span)
        .with_arg("issue", ISSUE_REGION_FACTS_NOT_BUILT)
        .with_arg("construct", construct)
        .with_arg("context", "borrow-region inference")
}

/// Map one engine failure to its fail-closed outcome: the named diagnostic
/// when the engine owns the shape, or `None` when the resolver's own named
/// rejection (SEM030/SEM031, a transfer outside its scope) already covers
/// the construct and a second diagnostic would only duplicate it.
#[must_use]
pub fn fail_closed_diagnostic(error: &RegionError) -> Option<crate::SemanticError> {
    match error {
        RegionError::Cfg(CfgError::UnsupportedShape { construct, span }) => {
            Some(region_facts_unavailable_error(construct, *span))
        }
        RegionError::MissingLocation { span, .. } => Some(region_facts_unavailable_error(
            "borrow-region start location",
            *span,
        )),
        RegionError::Cfg(CfgError::TransferOutsideLoop { .. }) => None,
    }
}

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

    pub fn regions_for(&self, borrower: DefId) -> impl Iterator<Item = &BorrowRegion> {
        self.regions
            .iter()
            .filter(move |region| region.borrower == borrower)
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
            let returns = collect_returns(body);
            if regions.is_none() && !returns.is_empty() {
                // The engine did not run (or rejected the CFG): a required
                // provenance summary is absent, not merely unmatched.
                return Err(ProvenanceError::Unavailable { function: def_id });
            }
            for root in returns {
                let Some(root) = root else {
                    return Err(ProvenanceError::Incoherent { function: def_id });
                };
                let mut region_found = false;
                for region in regions
                    .into_iter()
                    .flat_map(|facts| facts.regions_for(root))
                {
                    region_found = true;
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
                }
                if !region_found {
                    if params.iter().any(|param| param.def_id == root) {
                        // Returning a borrow-typed parameter re-exports the
                        // caller's borrow; no intra-body region is created.
                        returned_borrow = ReturnedBorrowProvenance::Parameter(root);
                    } else {
                        return Err(ProvenanceError::Incoherent { function: def_id });
                    }
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

/// Root definition of every `redde value` in the function body, including
/// those nested in `si`, `custodi`, loops, and handlers. `None` marks a
/// returned value with no definition root.
///
/// WHY: a closure's `redde` returns from the closure, not the function, so
/// closure bodies are not entered.
fn collect_returns(block: &HirBlock) -> Vec<Option<DefId>> {
    let mut collector = ReturnCollector { roots: Vec::new() };
    collector.visit_block(block);
    collector.roots
}

struct ReturnCollector {
    roots: Vec<Option<DefId>>,
}

impl HirVisitor for ReturnCollector {
    fn visit_stmt(&mut self, statement: &HirStatement) {
        if let HirStatementKind::Redde(Some(value)) = &statement.kind {
            self.roots.push(root_def_id(value));
        }
        walk_stmt(self, statement);
    }

    fn visit_expr(&mut self, expression: &HirExpression) {
        if matches!(expression.kind, HirExpressionKind::Clausura(..)) {
            return;
        }
        walk_expr(self, expression);
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
    analyze_with_interner(hir, types, access_facts, &Interner::new())
}

/// Interner-aware region inference. Tensor view methods are identified by
/// their canonical intrinsic rows and receive the same last-use treatment as
/// explicitly marked borrows.
///
/// # Errors
/// Returns `RegionError` when a control-flow shape has no conservative CFG
/// lowering, or when a borrow's start node has no source location.
pub fn analyze_with_interner(
    hir: &HirModule,
    types: &TypeTable,
    access_facts: &AccessFactTable,
    interner: &Interner,
) -> Result<RegionFacts, RegionError> {
    let cfg = build_cfg(hir)?;
    let mut collector = EventCollector::new(&cfg, types, access_facts, interner);
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

fn is_tensor_view_method(name: &str) -> bool {
    matches!(
        name,
        "sectio"
            | "disloco"
            | "transpone"
            | "__faber_glyph_transpose"
            | "forma"
            | "expande"
            | "limes"
    )
}

fn has_tensor_value_type(expression: &HirExpression, types: &TypeTable) -> bool {
    expression.ty.is_some_and(|ty| {
        let (_, inner) = types.peel_access_marker(ty);
        matches!(
            types.get(inner),
            crate::Type::Tensor(_, _) | crate::Type::Matrix(_, _)
        )
    })
}

fn is_tensor_view_expr(
    expression: &HirExpression,
    types: &TypeTable,
    interner: &Interner,
    aliases: &FxHashMap<DefId, FxHashSet<DefId>>,
) -> bool {
    match &expression.kind {
        HirExpressionKind::Path(def_id) => aliases.contains_key(def_id),
        HirExpressionKind::MethodCall(receiver, method, _, args, _) => receiver
            .ty
            .zip(interner.strings().get(method.0 as usize))
            .and_then(|(ty, name)| lookup_method_for_type_id(ty, types, name, args.len()))
            .is_some_and(|entry| is_tensor_view_method(entry.faber_name)),
        HirExpressionKind::NonNull(object, radix_hir::HirNonNullKind::Value)
            if has_tensor_value_type(expression, types) =>
        {
            is_tensor_view_expr(object, types, interner, aliases)
        }
        HirExpressionKind::Block(block) if has_tensor_value_type(expression, types) => block
            .expr
            .as_deref()
            .is_some_and(|expr| is_tensor_view_expr(expr, types, interner, aliases)),
        HirExpressionKind::Binary(radix_hir::HirBinOp::Coalesce, left, right)
            if has_tensor_value_type(expression, types) =>
        {
            is_tensor_view_expr(left, types, interner, aliases)
                || is_tensor_view_expr(right, types, interner, aliases)
        }
        HirExpressionKind::Si {
            then_block,
            else_block,
            ..
        } if has_tensor_value_type(expression, types) => {
            then_block
                .expr
                .as_deref()
                .is_some_and(|expr| is_tensor_view_expr(expr, types, interner, aliases))
                || else_block.as_ref().is_some_and(|block| {
                    block
                        .expr
                        .as_deref()
                        .is_some_and(|expr| is_tensor_view_expr(expr, types, interner, aliases))
                })
        }
        HirExpressionKind::CallDefault { body, default, .. }
            if has_tensor_value_type(expression, types) =>
        {
            body.expr
                .as_deref()
                .is_some_and(|expr| is_tensor_view_expr(expr, types, interner, aliases))
                || is_tensor_view_expr(default, types, interner, aliases)
        }
        HirExpressionKind::Verte { source, .. } | HirExpressionKind::Conversio { source, .. }
            if has_tensor_value_type(expression, types) =>
        {
            is_tensor_view_expr(source, types, interner, aliases)
        }
        _ => false,
    }
}

fn tensor_view_roots(
    expression: &HirExpression,
    types: &TypeTable,
    interner: &Interner,
    aliases: &FxHashMap<DefId, FxHashSet<DefId>>,
) -> FxHashSet<DefId> {
    match &expression.kind {
        HirExpressionKind::Path(def_id) => aliases.get(def_id).cloned().unwrap_or_default(),
        HirExpressionKind::MethodCall(receiver, _, _, _, _) => {
            if !is_tensor_view_expr(expression, types, interner, aliases) {
                return FxHashSet::default();
            }
            let mut roots = tensor_view_roots(receiver, types, interner, aliases);
            if roots.is_empty()
                && let Some(root) = root_def_id(receiver)
            {
                roots.insert(root);
            }
            roots
        }
        HirExpressionKind::NonNull(object, radix_hir::HirNonNullKind::Value)
        | HirExpressionKind::Block(HirBlock {
            expr: Some(object), ..
        }) => tensor_view_roots(object, types, interner, aliases),
        HirExpressionKind::Field(object, _)
        | HirExpressionKind::Index(object, _)
        | HirExpressionKind::Deref(object)
        | HirExpressionKind::NonNull(object, _) => {
            tensor_view_roots(object, types, interner, aliases)
        }
        HirExpressionKind::Binary(radix_hir::HirBinOp::Coalesce, left, right) => {
            let mut roots = tensor_view_roots(left, types, interner, aliases);
            roots.extend(tensor_view_roots(right, types, interner, aliases));
            roots
        }
        HirExpressionKind::Si {
            then_block,
            else_block,
            ..
        } => {
            let mut roots = then_block
                .expr
                .as_deref()
                .map(|expr| tensor_view_roots(expr, types, interner, aliases))
                .unwrap_or_default();
            if let Some(expr) = else_block.as_ref().and_then(|block| block.expr.as_deref()) {
                roots.extend(tensor_view_roots(expr, types, interner, aliases));
            }
            roots
        }
        HirExpressionKind::CallDefault { body, default, .. } => {
            let mut roots = body
                .expr
                .as_deref()
                .map(|expr| tensor_view_roots(expr, types, interner, aliases))
                .unwrap_or_default();
            roots.extend(tensor_view_roots(default, types, interner, aliases));
            roots
        }
        HirExpressionKind::Verte { source, .. } | HirExpressionKind::Conversio { source, .. } => {
            tensor_view_roots(source, types, interner, aliases)
        }
        _ => FxHashSet::default(),
    }
}

/// The body of an expression that runs as its own CFG component, plus the
/// operand expressions that still evaluate in the enclosing flow.
///
/// WHY: a closure body runs on call and a reduction/scan/sort lane body runs
/// per element; neither belongs to the flow around its definition, but the
/// names they read must still count as used at the definition site.
struct SeparateComponent<'a> {
    operands: Vec<&'a HirExpression>,
    body: ComponentBody<'a>,
}

enum ComponentBody<'a> {
    Expr(&'a HirExpression),
    Block(&'a HirBlock),
}

fn separate_component(kind: &HirExpressionKind) -> Option<SeparateComponent<'_>> {
    match kind {
        HirExpressionKind::Clausura(_, _, _, body) => Some(SeparateComponent {
            operands: Vec::new(),
            body: ComponentBody::Expr(body),
        }),
        // A `praefixum` block evaluates at build time: like a closure body,
        // it runs outside the flow around its definition, so the names it
        // reads count as used at the definition site.
        HirExpressionKind::Praefixum(body) => Some(SeparateComponent {
            operands: Vec::new(),
            body: ComponentBody::Expr(body),
        }),
        HirExpressionKind::Reduction {
            source,
            group,
            body: Some(body),
            identity,
            ..
        } => {
            let mut operands = vec![&**source];
            if let Some(group) = group {
                operands.push(group);
            }
            if let Some(identity) = identity {
                operands.push(identity);
            }
            Some(SeparateComponent {
                operands,
                body: ComponentBody::Block(body),
            })
        }
        HirExpressionKind::Scan {
            source,
            group,
            body,
            ..
        } => {
            let mut operands = vec![&**source];
            if let Some(group) = group {
                operands.push(group);
            }
            Some(SeparateComponent {
                operands,
                body: ComponentBody::Block(body),
            })
        }
        HirExpressionKind::Sort {
            source,
            body: Some(body),
            ..
        } => Some(SeparateComponent {
            operands: vec![&**source],
            body: ComponentBody::Block(body),
        }),
        _ => None,
    }
}

struct EventCollector<'a> {
    cfg: &'a HirCfg,
    types: &'a TypeTable,
    access_facts: &'a AccessFactTable,
    interner: &'a Interner,
    tensor_view_roots: FxHashMap<DefId, FxHashSet<DefId>>,
    borrows: Vec<BorrowCandidate>,
    uses: FxHashMap<DefId, Vec<CfgNodeId>>,
    source_kills: FxHashMap<DefId, Vec<CfgNodeId>>,
    locations: Vec<CfgNodeId>,
    fact_reads: usize,
}

impl<'a> EventCollector<'a> {
    fn new(
        cfg: &'a HirCfg,
        types: &'a TypeTable,
        access_facts: &'a AccessFactTable,
        interner: &'a Interner,
    ) -> Self {
        let mut source_kills: FxHashMap<DefId, Vec<CfgNodeId>> = FxHashMap::default();
        for fact in access_facts.dead_sources() {
            if let Some(site) = fact.site
                && let Some(node) = cfg.node_for_hir_id(site)
            {
                source_kills.entry(fact.source).or_default().push(node);
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
            interner,
            tensor_view_roots: FxHashMap::default(),
            borrows: Vec::new(),
            uses: FxHashMap::default(),
            source_kills,
            locations: Vec::new(),
            fact_reads: access_facts.dead_source_count(),
        }
    }

    /// Walk a separate-component body (a closure or lane body), then anchor
    /// every name it used at the component's definition site.
    ///
    /// WHY: the component's uses live in a disconnected part of the CFG, so a
    /// borrow used only inside it would look dead in the enclosing flow even
    /// between its bind and this site.  Anchoring one use at the site where
    /// the value escapes keeps the region live up to the creation point —
    /// exactly the reach the block-scope approximation decided for captures.
    fn visit_separate_component(
        &mut self,
        component: SeparateComponent<'_>,
        site: Option<CfgNodeId>,
    ) {
        for operand in component.operands {
            self.visit_expr(operand);
        }
        let before: Vec<(DefId, usize)> = self
            .uses
            .iter()
            .map(|(def_id, nodes)| (*def_id, nodes.len()))
            .collect();
        match component.body {
            ComponentBody::Expr(body) => self.visit_expr(body),
            ComponentBody::Block(body) => self.visit_block(body),
        }
        let Some(site) = site else {
            return;
        };
        for (def_id, count) in before {
            if self
                .uses
                .get(&def_id)
                .is_some_and(|nodes| nodes.len() > count)
            {
                self.uses.entry(def_id).or_default().push(site);
            }
        }
    }

    fn collect_local(&mut self, statement: &HirStatement, local: &HirLocal) {
        let Some(init) = &local.init else {
            return;
        };
        self.fact_reads += 2;
        let _initializer_fact = self.access_facts.expression(init.id);
        let _binding_fact = self.access_facts.binding(local.def_id);

        let view_roots =
            if is_tensor_view_expr(init, self.types, self.interner, &self.tensor_view_roots) {
                tensor_view_roots(init, self.types, self.interner, &self.tensor_view_roots)
            } else {
                FxHashSet::default()
            };
        if !view_roots.is_empty() {
            self.tensor_view_roots
                .insert(local.def_id, view_roots.clone());
        }
        let kind = (!view_roots.is_empty())
            .then_some(RegionBorrowKind::Shared)
            .or_else(|| {
                local
                    .ty
                    .and_then(|ty| self.types.peel_access_marker(ty).0)
                    .and_then(region_kind)
            });
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
        let sources: Vec<DefId> = if view_roots.is_empty() {
            let Some(source) = root_def_id(init) else {
                return;
            };
            vec![source]
        } else {
            view_roots.into_iter().collect()
        };
        let start = self
            .cfg
            .node_for_hir_id(statement.id)
            .or_else(|| self.cfg.node_for_hir_id(init.id));
        let Some(start) = start else {
            return;
        };
        for source in sources {
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
        if let Some(component) = separate_component(&expression.kind) {
            self.visit_separate_component(component, node);
            return;
        }
        if let HirExpressionKind::Path(def_id) = &expression.kind
            && let Some(node) = node
        {
            self.uses.entry(*def_id).or_default().push(node);
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
