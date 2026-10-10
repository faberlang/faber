//! Target-neutral derived access facts for binding initializers and dead sources.
//!
//! This provider records only facts that can be established without changing
//! semantic policy. Aggregate literals and constructors allocate fresh
//! aggregate storage; aliases, calls, and every other shape remain
//! conservatively unknown. The dead-source side of the table is deliberately
//! event based: a source can be dead on one control-flow path and live on
//! another, so callers must query the recorded transfer or scope-end site
//! rather than treating a definition as globally dead.

use crate::{AccessMarker, Type, TypeTable};
use radix_hir::visit::{HirVisitor, walk_expr, walk_local, walk_stmt};
use radix_hir::{
    DefId, HirArrayElement, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirId,
    HirItemKind, HirLocal, HirModule, HirStatement, HirStatementKind,
};
use radix_lexer::Span;
use rustc_hash::{FxHashMap, FxHashSet};

/// Conservative storage-origin classification for one expression or binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessFact {
    /// The expression constructs aggregate storage at its evaluation site.
    Fresh,
    /// The provider cannot prove a fresh aggregate storage origin.
    Unknown,
}

/// Why a source binding is known to be dead at a recorded site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeadSourceReason {
    /// An exclusive source was transferred into an `own` destination.
    OwnTransfer,
    /// An exclusive source's lexical scope ended without an observed use.
    ScopeEnd,
}

/// One event-local dead-source fact.
///
/// `source` and `source_span` identify the source binding. `site` is the HIR
/// identity of an own-transfer expression when one exists; scope-end facts use
/// `None` because a structural block end is a presentation anchor rather than
/// an expression node. `site_span` remains available for both query and future
/// diagnostics without making this provider emit any diagnostics today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadSourceFact {
    pub source: DefId,
    pub source_span: Span,
    pub site: Option<HirId>,
    pub site_span: Span,
    pub reason: DeadSourceReason,
}

/// Derived access facts indexed by expression and binding identity.
///
/// A binding initializer is recorded under its expression [`HirId`] and its
/// local [`DefId`]. Queries default to [`AccessFact::Unknown`] when an identity
/// has no row, so downstream consumers do not need to re-walk HIR to stay
/// conservative. Dead-source facts are kept as a separate, ordered event list
/// because branch-local death must not be widened into global death.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessFactTable {
    by_expression: FxHashMap<HirId, AccessFact>,
    by_binding: FxHashMap<DefId, AccessFact>,
    dead_sources: Vec<DeadSourceFact>,
}

impl AccessFactTable {
    /// Return the fact for an expression, conservatively defaulting to unknown.
    #[must_use]
    pub fn expression(&self, id: HirId) -> AccessFact {
        self.by_expression
            .get(&id)
            .copied()
            .unwrap_or(AccessFact::Unknown)
    }

    /// Return the fact for a local binding, conservatively defaulting to unknown.
    #[must_use]
    pub fn binding(&self, def_id: DefId) -> AccessFact {
        self.by_binding
            .get(&def_id)
            .copied()
            .unwrap_or(AccessFact::Unknown)
    }

    /// Iterate over expression-site facts recorded from binding initializers.
    pub fn expressions(&self) -> impl Iterator<Item = (HirId, AccessFact)> + '_ {
        let mut rows: Vec<_> = self
            .by_expression
            .iter()
            .map(|(id, fact)| (*id, *fact))
            .collect();
        rows.sort_by_key(|(id, _)| id.0);
        rows.into_iter()
    }

    /// Iterate over binding-site facts recorded from local declarations.
    pub fn bindings(&self) -> impl Iterator<Item = (DefId, AccessFact)> + '_ {
        let mut rows: Vec<_> = self
            .by_binding
            .iter()
            .map(|(def_id, fact)| (*def_id, *fact))
            .collect();
        rows.sort_by_key(|(def_id, _)| def_id.0);
        rows.into_iter()
    }

    /// Return dead-source events in deterministic source order.
    #[must_use]
    pub fn dead_sources(&self) -> &[DeadSourceFact] {
        &self.dead_sources
    }

    /// Return dead-source events for one source identity.
    pub fn dead_sources_of(&self, source: DefId) -> impl Iterator<Item = &DeadSourceFact> {
        self.dead_sources
            .iter()
            .filter(move |fact| fact.source == source)
    }

    /// Return the transfer fact recorded at an expression site, if any.
    #[must_use]
    pub fn dead_source_at(&self, source: DefId, site: HirId) -> Option<&DeadSourceFact> {
        self.dead_sources
            .iter()
            .find(|fact| fact.source == source && fact.site == Some(site))
    }

    /// Number of expression-site rows.
    #[must_use]
    pub fn expression_count(&self) -> usize {
        self.by_expression.len()
    }

    /// Number of binding-site rows.
    #[must_use]
    pub fn binding_count(&self) -> usize {
        self.by_binding.len()
    }

    /// Number of dead-source events.
    #[must_use]
    pub fn dead_source_count(&self) -> usize {
        self.dead_sources.len()
    }

    fn record(&mut self, local: &HirLocal) {
        let Some(init) = &local.init else {
            return;
        };
        let fact = classify_initializer(init);
        self.by_expression.insert(init.id, fact);
        self.by_binding.insert(local.def_id, fact);
    }

    fn record_dead_source(&mut self, fact: DeadSourceFact) {
        if !self.dead_sources.contains(&fact) {
            self.dead_sources.push(fact);
            self.dead_sources.sort_by_key(|candidate| {
                (
                    candidate.source.0,
                    candidate.site_span.start,
                    candidate.site_span.end,
                    candidate.site.map_or(0, |site| site.0),
                    match candidate.reason {
                        DeadSourceReason::OwnTransfer => 0_u8,
                        DeadSourceReason::ScopeEnd => 1_u8,
                    },
                )
            });
        }
    }
}

/// Derive target-neutral access facts from a HIR module without a type table.
///
/// This compatibility entry point preserves the original MU-8a shape-only
/// query. The analyzed-application carrier uses
/// [`analyze_access_facts_with_types`] so dead-source facts can consume the
/// already-landed type ownership facts read-only.
#[must_use]
pub fn analyze_access_facts(hir: &HirModule) -> AccessFactTable {
    analyze_access_facts_impl(hir, None)
}

/// Derive access facts from typed HIR and ownership facts.
///
/// No semantic diagnostic is emitted here. The dead-source analysis is
/// conservative at joins: transfer and scope-end facts are suppressed inside
/// unknown branches and loops, and a source is only recorded when its existing
/// exclusive-value fact is live on the analyzed path.
#[must_use]
pub fn analyze_access_facts_with_types(hir: &HirModule, types: &TypeTable) -> AccessFactTable {
    analyze_access_facts_impl(hir, Some(types))
}

fn analyze_access_facts_impl(hir: &HirModule, types: Option<&TypeTable>) -> AccessFactTable {
    let mut facts = AccessFactTable::default();
    let mut collector = AccessFactCollector { facts: &mut facts };
    collector.visit_program(hir);

    let Some(types) = types else {
        return facts;
    };

    let mut analyzer = DeadSourceAnalyzer::new(types, &mut facts);
    analyzer.analyze_module(hir);
    facts
}

struct AccessFactCollector<'a> {
    facts: &'a mut AccessFactTable,
}

impl HirVisitor for AccessFactCollector<'_> {
    fn visit_local(&mut self, local: &HirLocal) {
        self.facts.record(local);
        walk_local(self, local);
    }
}

fn classify_initializer(expr: &HirExpression) -> AccessFact {
    match &expr.kind {
        HirExpressionKind::Array(elements)
            if elements
                .iter()
                .all(|element| matches!(element, HirArrayElement::Expr(_))) =>
        {
            AccessFact::Fresh
        }
        HirExpressionKind::Struct(_, _) | HirExpressionKind::Tuple(_, _) => AccessFact::Fresh,
        HirExpressionKind::Verte {
            entries: Some(_), ..
        } => AccessFact::Fresh,
        _ => AccessFact::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Liveness {
    Live,
    Dead,
    MaybeDead,
}

#[derive(Debug, Clone, Copy)]
struct BindingSite {
    span: Span,
    exclusive: bool,
    fresh: bool,
}

#[derive(Debug, Clone, Copy)]
struct PendingDeadSource {
    fact: DeadSourceFact,
    valid: bool,
}

struct DeadSourceAnalyzer<'a> {
    types: &'a TypeTable,
    facts: &'a mut AccessFactTable,
    states: FxHashMap<DefId, Liveness>,
    bindings: FxHashMap<DefId, BindingSite>,
    used: FxHashSet<DefId>,
    pending_dead_sources: Vec<PendingDeadSource>,
    scopes: Vec<Vec<DefId>>,
    function_params: FxHashMap<DefId, Vec<radix_types::ParamType>>,
    branch_depth: usize,
}

impl<'a> DeadSourceAnalyzer<'a> {
    fn new(types: &'a TypeTable, facts: &'a mut AccessFactTable) -> Self {
        Self {
            types,
            facts,
            states: FxHashMap::default(),
            bindings: FxHashMap::default(),
            used: FxHashSet::default(),
            pending_dead_sources: Vec::new(),
            scopes: Vec::new(),
            function_params: FxHashMap::default(),
            branch_depth: 0,
        }
    }

    fn analyze_module(&mut self, hir: &HirModule) {
        for item in &hir.items {
            match &item.kind {
                HirItemKind::Function(function) => {
                    self.function_params
                        .insert(item.def_id, function_params(function));
                }
                HirItemKind::Struct(strukt) => {
                    for method in &strukt.methods {
                        self.function_params
                            .insert(method.def_id, function_params(&method.func));
                    }
                }
                _ => {}
            }
        }
        for item in &hir.items {
            match &item.kind {
                HirItemKind::Function(function) => self.analyze_function(function),
                HirItemKind::Struct(strukt) => {
                    for method in &strukt.methods {
                        self.analyze_function(&method.func);
                    }
                }
                _ => {}
            }
        }
        if let Some(entry) = &hir.entry {
            self.reset_flow();
            self.visit_block(entry);
            self.finalize_pending_dead_sources();
        }
    }

    fn analyze_function(&mut self, function: &HirFunction) {
        self.reset_flow();
        let mut params = Vec::with_capacity(function.params.len());
        for param in &function.params {
            self.declare(
                param.def_id,
                param.span,
                self.types.is_exclusive_value(param.ty),
                false,
            );
            params.push(param.def_id);
        }
        if let Some(body) = &function.body {
            self.visit_block(body);
        }
        self.finalize_pending_dead_sources();
        // Parameters are function-scope roots. Their body block owns the
        // lexical locals, so no synthetic scope-end fact is needed here.
        for param in params {
            self.states.remove(&param);
            self.bindings.remove(&param);
        }
    }

    fn reset_flow(&mut self) {
        self.states.clear();
        self.bindings.clear();
        self.used.clear();
        self.pending_dead_sources.clear();
        self.scopes.clear();
        self.branch_depth = 0;
    }

    fn declare(&mut self, def_id: DefId, span: Span, exclusive: bool, fresh: bool) {
        self.states.insert(def_id, Liveness::Live);
        self.bindings.insert(
            def_id,
            BindingSite {
                span,
                exclusive,
                fresh,
            },
        );
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(def_id);
        }
    }

    fn source_path(expr: &HirExpression) -> Option<DefId> {
        match expr.kind {
            HirExpressionKind::Path(def_id) => Some(def_id),
            _ => None,
        }
    }

    fn source_is_live_transferable(&self, expr: &HirExpression) -> Option<(DefId, bool)> {
        let source = Self::source_path(expr)?;
        let binding = self.bindings.get(&source);
        let exclusive = binding.is_some_and(|binding| binding.exclusive)
            || expr.ty.is_some_and(|ty| self.types.is_exclusive_value(ty));
        let fresh = binding.is_some_and(|binding| binding.fresh);
        let live = matches!(self.states.get(&source), Some(Liveness::Live));
        (live && (exclusive || fresh)).then_some((source, exclusive))
    }

    fn source_span(&self, source: DefId, fallback: Span) -> Span {
        self.bindings
            .get(&source)
            .map_or(fallback, |binding| binding.span)
    }

    fn record_transfer(&mut self, source_expr: &HirExpression, site: &HirExpression) {
        let Some((source, exclusive)) = self.source_is_live_transferable(source_expr) else {
            return;
        };
        // A prior path use may have retained an alias. Stay conservative and
        // do not turn a later own transfer into a storage-death fact.
        if self.used.contains(&source) {
            return;
        }
        self.states.insert(source, Liveness::Dead);
        if self.branch_depth != 0 {
            return;
        }
        let fact = DeadSourceFact {
            source,
            source_span: self.source_span(source, source_expr.span),
            site: Some(site.id),
            site_span: site.span,
            reason: DeadSourceReason::OwnTransfer,
        };
        if exclusive {
            self.facts.record_dead_source(fact);
        } else {
            // MU-3 permits an unmarked source only when its later use proves
            // absent. Hold that candidate until the whole function/entry has
            // been walked so an invalidating later path cannot receive a
            // falsely optimistic fact.
            self.pending_dead_sources
                .push(PendingDeadSource { fact, valid: true });
        }
    }

    fn finalize_pending_dead_sources(&mut self) {
        for pending in std::mem::take(&mut self.pending_dead_sources) {
            if pending.valid {
                self.facts.record_dead_source(pending.fact);
            }
        }
    }

    fn record_scope_end(&mut self, block: &HirBlock, scope: &[DefId]) {
        let Some(end) = &block.end else {
            return;
        };
        if self.branch_depth != 0 {
            return;
        }
        for &source in scope {
            let Some(binding) = self.bindings.get(&source).copied() else {
                continue;
            };
            if !(binding.exclusive || binding.fresh)
                || self.used.contains(&source)
                || !matches!(self.states.get(&source), Some(Liveness::Live))
            {
                continue;
            }
            self.facts.record_dead_source(DeadSourceFact {
                source,
                source_span: binding.span,
                site: None,
                site_span: end.span,
                reason: DeadSourceReason::ScopeEnd,
            });
            self.states.insert(source, Liveness::Dead);
        }
    }

    fn mark_path_used(&mut self, def_id: DefId) {
        self.used.insert(def_id);
        for pending in &mut self.pending_dead_sources {
            if pending.fact.source == def_id {
                pending.valid = false;
            }
        }
    }

    fn call_params(&self, callee: &HirExpression) -> Option<Vec<radix_types::ParamType>> {
        if let Some(ty) = callee.ty {
            return match self.types.get(ty) {
                Type::Func(sig) => Some(sig.params.clone()),
                Type::Alias(_, resolved) => match self.types.get(*resolved) {
                    Type::Func(sig) => Some(sig.params.clone()),
                    _ => None,
                },
                _ => None,
            };
        }
        match callee.kind {
            HirExpressionKind::Path(def_id) => self.function_params.get(&def_id).cloned(),
            _ => None,
        }
    }

    fn destination_is_copy(&self, ty: Option<radix_types::TypeId>) -> bool {
        ty.and_then(|id| self.types.peel_access_marker(id).0) == Some(AccessMarker::Copy)
    }

    fn destination_is_own(&self, ty: Option<radix_types::TypeId>) -> bool {
        ty.is_some_and(|id| self.types.is_own_view(id))
    }

    fn join_states(
        baseline: &FxHashMap<DefId, Liveness>,
        left: &FxHashMap<DefId, Liveness>,
        right: &FxHashMap<DefId, Liveness>,
    ) -> FxHashMap<DefId, Liveness> {
        let mut joined = baseline.clone();
        for (&def_id, &before) in baseline {
            let left = left.get(&def_id).copied().unwrap_or(before);
            let right = right.get(&def_id).copied().unwrap_or(before);
            let state = match (left, right) {
                (Liveness::Dead, Liveness::Dead) => Liveness::Dead,
                (Liveness::Live, Liveness::Live) => Liveness::Live,
                _ => Liveness::MaybeDead,
            };
            joined.insert(def_id, state);
        }
        joined
    }

    fn with_unknown_branch(
        &mut self,
        then_body: impl FnOnce(&mut Self),
        else_body: impl FnOnce(&mut Self),
    ) {
        let baseline_states = self.states.clone();
        let baseline_bindings = self.bindings.clone();
        let baseline_used = self.used.clone();
        self.branch_depth += 1;
        then_body(self);
        let then_states = self.states.clone();
        let then_used = self.used.clone();
        self.states.clone_from(&baseline_states);
        self.bindings = baseline_bindings.clone();
        self.used = baseline_used.clone();
        else_body(self);
        let else_states = self.states.clone();
        let else_used = self.used.clone();
        self.branch_depth = self.branch_depth.saturating_sub(1);
        self.states = Self::join_states(&baseline_states, &then_states, &else_states);
        self.bindings = baseline_bindings;
        self.used = baseline_used;
        self.used.extend(then_used);
        self.used.extend(else_used);
    }

    fn known_bool(expr: &HirExpression) -> Option<bool> {
        match expr.kind {
            HirExpressionKind::Literal(radix_hir::HirLiteral::Bool(value)) => Some(value),
            _ => None,
        }
    }

    fn visit_if(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        then_catch: Option<&radix_hir::HirCape>,
        else_block: Option<&HirBlock>,
    ) {
        self.visit_expr(cond);
        match Self::known_bool(cond) {
            Some(true) => self.visit_block(then_block),
            Some(false) => {
                if let Some(else_block) = else_block {
                    self.visit_block(else_block);
                }
            }
            None => self.with_unknown_branch(
                |this| {
                    this.visit_block(then_block);
                    if let Some(catch) = then_catch {
                        this.visit_block(&catch.body);
                    }
                },
                |this| {
                    if let Some(else_block) = else_block {
                        this.visit_block(else_block);
                    }
                },
            ),
        }
    }
}

fn function_params(function: &HirFunction) -> Vec<radix_types::ParamType> {
    function
        .params
        .iter()
        .map(|param| radix_types::ParamType {
            ty: param.ty,
            mode: match param.mode {
                radix_hir::HirParamMode::Owned | radix_hir::HirParamMode::Axis => {
                    radix_types::SemanticParamMode::Owned
                }
                radix_hir::HirParamMode::De => radix_types::SemanticParamMode::Ref,
                radix_hir::HirParamMode::In => radix_types::SemanticParamMode::MutRef,
                radix_hir::HirParamMode::Ex => radix_types::SemanticParamMode::Move,
            },
            optional: param.optional,
        })
        .collect()
}

impl HirVisitor for DeadSourceAnalyzer<'_> {
    fn visit_block(&mut self, block: &HirBlock) {
        self.scopes.push(Vec::new());
        for statement in &block.statements {
            self.visit_stmt(statement);
        }
        if let Some(expr) = &block.expr {
            self.visit_expr(expr);
        }
        let scope = self.scopes.pop().unwrap_or_default();
        self.record_scope_end(block, &scope);
    }

    fn visit_stmt(&mut self, statement: &HirStatement) {
        match &statement.kind {
            HirStatementKind::Local(local) => {
                let binding_span = local.init.as_ref().map_or(statement.span, |init| init.span);
                self.declare(
                    local.def_id,
                    binding_span,
                    local.ty.is_some_and(|ty| self.types.is_exclusive_value(ty)),
                    local.init.is_some() && self.facts.binding(local.def_id) == AccessFact::Fresh,
                );
                if let Some(init) = &local.init {
                    if self.destination_is_own(local.ty) && !self.destination_is_copy(local.ty) {
                        if self.source_is_live_transferable(init).is_some() {
                            self.record_transfer(init, init);
                        } else {
                            self.visit_expr(init);
                        }
                    } else {
                        self.visit_expr(init);
                    }
                }
            }
            HirStatementKind::Redde(value) => {
                if let Some(value) = value {
                    self.visit_expr(value);
                }
            }
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    self.visit_expr(&clause.cond);
                    self.branch_depth += 1;
                    self.visit_block(&clause.body);
                    self.branch_depth = self.branch_depth.saturating_sub(1);
                }
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.mark_path_used(*def_id),
            HirExpressionKind::Call(callee, _, args, _) => {
                self.visit_expr(callee);
                let params = self.call_params(callee);
                for (index, arg) in args.iter().enumerate() {
                    let own_dest = params
                        .as_ref()
                        .and_then(|params| params.get(index))
                        .is_some_and(|param| {
                            self.destination_is_own(Some(param.ty))
                                && !self.destination_is_copy(Some(param.ty))
                        });
                    if own_dest && self.source_is_live_transferable(&arg.expr).is_some() {
                        self.record_transfer(&arg.expr, &arg.expr);
                    } else {
                        self.visit_expr(&arg.expr);
                    }
                }
            }
            HirExpressionKind::Assign(target, value) => {
                let target_root = Self::source_path(target);
                let own_target = self.destination_is_own(target.ty)
                    || target_root.is_some_and(|def_id| {
                        self.bindings
                            .get(&def_id)
                            .is_some_and(|binding| binding.exclusive)
                    });
                if own_target && !self.destination_is_copy(target.ty) {
                    if self.source_is_live_transferable(value).is_some() {
                        self.record_transfer(value, value);
                    } else {
                        self.visit_expr(value);
                    }
                    if let Some(target_root) = target_root {
                        self.states.insert(target_root, Liveness::Live);
                    }
                } else {
                    self.visit_expr(target);
                    self.visit_expr(value);
                }
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => self.visit_if(cond, then_block, then_catch.as_deref(), else_block.as_ref()),
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for scrutinee in scrutinees {
                    self.visit_expr(scrutinee);
                }
                let baseline = self.states.clone();
                let bindings = self.bindings.clone();
                let used = self.used.clone();
                let mut branch_states = Vec::new();
                let mut branch_used = Vec::new();
                for arm in arms {
                    self.states = baseline.clone();
                    self.bindings = bindings.clone();
                    self.used = used.clone();
                    self.branch_depth += 1;
                    self.visit_expr(&arm.body);
                    self.branch_depth = self.branch_depth.saturating_sub(1);
                    branch_states.push(self.states.clone());
                    branch_used.push(self.used.clone());
                }
                if let Some(first) = branch_states.first() {
                    let mut joined = baseline.clone();
                    for (&def_id, &before) in &baseline {
                        let mut state = first.get(&def_id).copied().unwrap_or(before);
                        for states in branch_states.iter().skip(1) {
                            let next = states.get(&def_id).copied().unwrap_or(before);
                            state = match (state, next) {
                                (Liveness::Dead, Liveness::Dead) => Liveness::Dead,
                                (Liveness::Live, Liveness::Live) => Liveness::Live,
                                _ => Liveness::MaybeDead,
                            };
                        }
                        joined.insert(def_id, state);
                    }
                    self.states = joined;
                } else {
                    self.states = baseline;
                }
                self.bindings = bindings;
                self.used = used;
                for branch in branch_used {
                    self.used.extend(branch);
                }
            }
            HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => {
                if matches!(expr.kind, HirExpressionKind::Loop(_)) {
                    self.with_unknown_branch(|this| this.visit_block(block), |_| {});
                } else {
                    self.visit_block(block);
                }
            }
            HirExpressionKind::Dum(cond, block) => {
                self.visit_expr(cond);
                self.with_unknown_branch(|this| this.visit_block(block), |_| {});
            }
            HirExpressionKind::Itera {
                iter, body: block, ..
            } => {
                self.visit_expr(iter);
                self.with_unknown_branch(|this| this.visit_block(block), |_| {});
            }
            HirExpressionKind::Handled { body, catch } => {
                self.with_unknown_branch(
                    |this| this.visit_block(body),
                    |this| this.visit_block(&catch.body),
                );
            }
            HirExpressionKind::Clausura(_, _, _, body) => {
                let baseline_states = self.states.clone();
                let baseline_bindings = self.bindings.clone();
                let baseline_used = self.used.clone();
                self.branch_depth += 1;
                self.visit_expr(body);
                self.branch_depth = self.branch_depth.saturating_sub(1);
                let closure_used = self.used.clone();
                self.states = baseline_states;
                self.bindings = baseline_bindings;
                // Captured paths are uses even though closure execution is
                // deferred; keep scope-end facts conservative.
                self.used = baseline_used;
                self.used.extend(closure_used);
            }
            _ => walk_expr(self, expr),
        }
    }
}
