//! Definite-assignment analysis for typed HIR.
//!
//! This pass owns two related assignment-discipline invariants:
//!
//! - **Use-before-init**: non-`sponte` bindings may be read only after they are
//!   definitely initialized on every control-flow path reaching the read.
//! - **Write-once**: `fixum` locals and owned/de/ex parameters may be assigned at
//!   most once. A declaration without an initializer (`fixum numerus x`) is the
//!   deferred-init form: it is `Unassigned` until its single later assignment.
//!   `varia` locals and `in` parameters are freely reassignable.
//!
//! Typecheck no longer adjudicates immutability of local/parameter assignments;
//! it only rejects constants and invalid lvalues. All assignment discipline
//! lives here so the [`InitState`] lattice is the single source of truth.
//!
//! The analyzer merges path states at control-flow joins (`si`/`sin`/`secus`
//! branches and `discerne` arms) via [`InitState::join_paths`], so conditional
//! initialization is rejected as unsound. It remains intra-function and
//! flow-insensitive (no full CFG, no loop fixpoint), so a future 3b CFG pass may
//! still refine precision without changing diagnostics.
//!
//! PASS ORDER
//! ==========
//! Runs after typecheck and borrow analysis. When borrow already diagnoses a
//! moved-then-read use, definite-assignment does not add a second read error for
//! the same binding because moved values were initialized before the move.
//!
//! INVARIANTS
//! ==========
//! - State is keyed by root `DefId`, matching borrow's projection collapse.
//! - `sponte` parameters are exempt from uninitialized-read diagnostics.
//! - Write-once enforcement applies to `fixum` locals and non-`in` parameters;
//!   `varia` locals and `in` parameters are exempt.

use super::method_receiver_mutation::{self, MutatingMethods};
use crate::init_state::InitState;
use crate::{Resolver, SemanticError, SemanticErrorKind, TypeTable};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{
    DefId, HirBlock, HirCape, HirCasuArm, HirExpression, HirExpressionKind, HirFunction, HirItem,
    HirItemKind, HirLocal, HirModule, HirParam, HirParamMode, HirPattern, HirStatement,
    HirStatementKind,
};
use radix_lexer::{Interner, Symbol};

const ISSUE_IMMUTABLE_SECOND_ASSIGNMENT: &str = "immutable_second_assignment";
/// R2: the left side of `⇇` is a `fixum` binding or a non-`mut` parameter.
const ISSUE_COPY_INTO_TARGET_IMMUTABLE: &str = "copy_into_target_immutable";
const ISSUE_MUTATING_METHOD_ON_IMMUTABLE: &str =
    "imperativus_operator_method_requires_varia_receiver";
const ISSUE_POSSIBLY_UNINITIALIZED: &str = "possibly_uninitialized";

/// Validate definite-assignment invariants across the lowered program.
///
/// # Errors
///
/// Returns definite-assignment violations as a list of [`SemanticError`]s.
pub fn analyze(
    hir: &HirModule,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
) -> Result<(), Vec<SemanticError>> {
    let mutating = method_receiver_mutation::collect(hir, types, interner);
    let mut checker = DefiniteAssignmentChecker::new(types, resolver, interner, mutating);
    checker.check_program(hir);

    if !checker.errors.is_empty() {
        return Err(checker.errors);
    }
    Ok(())
}

struct DefiniteAssignmentChecker<'a> {
    states: FxHashMap<DefId, InitState>,
    sponte_slots: FxHashSet<DefId>,
    /// Write-once bindings: `fixum` locals and (immutable) parameters. A second
    /// assignment to any of these is rejected; the first initializes them.
    write_once: FxHashSet<DefId>,
    /// Bare and `ref` parameters: the name may be re-pointed inside the callee.
    repointable: FxHashSet<DefId>,
    errors: Vec<SemanticError>,
    /// Type table for tensor-index-mutation detection (bracket `ponde` sugar).
    types: &'a TypeTable,
    resolver: &'a Resolver,
    interner: &'a Interner,
    /// Class methods whose body changes the receiver in place.
    mutating_methods: MutatingMethods,
}

impl<'a> DefiniteAssignmentChecker<'a> {
    fn new(
        types: &'a TypeTable,
        resolver: &'a Resolver,
        interner: &'a Interner,
        mutating_methods: MutatingMethods,
    ) -> Self {
        Self {
            resolver,
            interner,
            mutating_methods,
            states: FxHashMap::default(),
            sponte_slots: FxHashSet::default(),
            write_once: FxHashSet::default(),
            repointable: FxHashSet::default(),
            errors: Vec::new(),
            types,
        }
    }

    fn check_program(&mut self, hir: &HirModule) {
        for item in &hir.items {
            self.check_item(item);
        }

        if let Some(entry) = &hir.entry {
            self.reset();
            self.check_block(entry);
        }
    }

    fn check_item(&mut self, item: &HirItem) {
        match &item.kind {
            HirItemKind::Function(func) => self.check_function(func),
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    self.check_function(&method.func);
                }
            }
            HirItemKind::Constant(const_item) => {
                self.reset();
                self.visit_expr(&const_item.value);
            }
            _ => {}
        }
    }

    fn check_function(&mut self, func: &HirFunction) {
        self.reset();
        for param in &func.params {
            self.register_param(param);
        }
        if let Some(cli_args) = &func.cli_args {
            self.register_param(cli_args);
            self.mark_assigned(cli_args.def_id);
        }
        if let Some(body) = &func.body {
            self.check_block(body);
        }
    }

    fn reset(&mut self) {
        self.states.clear();
        self.sponte_slots.clear();
        self.write_once.clear();
        self.repointable.clear();
    }

    fn register_param(&mut self, param: &HirParam) {
        self.track_binding(param.def_id);
        // WHY: `mut` (`in`) parameters write the caller's value and `own`/`copy`
        // parameters write the callee's own value, so all three are writable
        // (they lower to Rust `mut` bindings). Unmarked/`ref` parameters are
        // write-once: they are already initialized by the caller, so any
        // assignment is a reassignment.
        // A `ref` parameter is not write-once at all: re-pointing it is legal
        // and a write through it is the borrow pass's single `assign_through_ref`.
        // A bare parameter is `ref` by default: re-pointing the name is legal,
        // a write through it stays this pass's rejection.
        if !matches!(param.mode, HirParamMode::In) && !self.types.is_exclusive_value(param.ty) {
            if matches!(param.mode, HirParamMode::De) {
                self.repointable.insert(param.def_id);
            } else {
                self.write_once.insert(param.def_id);
                self.repointable.insert(param.def_id);
            }
        }
        if param.sponte {
            self.sponte_slots.insert(param.def_id);
        } else {
            self.mark_assigned(param.def_id);
        }
    }

    fn check_block(&mut self, block: &HirBlock) {
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
        if let Some(expr) = &block.expr {
            self.visit_expr(expr);
        }
    }

    fn check_stmt(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Local(local) => self.check_local(local),
            HirStatementKind::Expr(expr) => self.visit_expr(expr),
            HirStatementKind::Redde(value) | HirStatementKind::ReddeFilum(value) => {
                if let Some(expr) = value {
                    self.visit_expr(expr);
                }
            }
            HirStatementKind::Rumpe | HirStatementKind::Perge | HirStatementKind::Tacet => {}
            HirStatementKind::IncDec(inc_dec) => self.op_assign(&inc_dec.target),
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    self.visit_expr(&clause.cond);
                    // PRP-057: `take` swaps the current state out in O(1) instead
                    // of cloning the full map at each branch point.
                    let incoming = std::mem::take(&mut self.states);
                    self.states = self.run_branches(2, incoming, |checker, index| {
                        if index == 0 {
                            checker.check_block(&clause.body);
                        }
                    });
                }
            }
        }
    }

    fn check_local(&mut self, local: &HirLocal) {
        self.track_binding(local.def_id);
        // WHY: `fixum` (!mutable) locals are write-once — they may be declared
        // without an initializer and assigned exactly once later. `varia`
        // (mutable) locals are freely reassignable.
        if !local.mutable {
            self.write_once.insert(local.def_id);
        }
        if local.runtime_provided {
            self.mark_assigned(local.def_id);
            return;
        }
        if let Some(init) = &local.init {
            self.visit_expr(init);
            self.mark_assigned(local.def_id);
        }
    }

    fn track_binding(&mut self, def_id: DefId) {
        self.states.entry(def_id).or_default();
    }

    fn mark_assigned(&mut self, def_id: DefId) {
        if !self.states.contains_key(&def_id) {
            return;
        }
        self.states.insert(def_id, InitState::Assigned);
    }

    /// Reject a second assignment to a write-once (`fixum`/param) binding.
    /// The first assignment transitions `Unassigned → Assigned`; any later
    /// assignment (when the state is `Assigned` or `MaybeAssigned`) is illegal.
    fn enforce_write_once(&mut self, def_id: DefId, span: radix_lexer::Span, repoint: bool) {
        if !self.write_once.contains(&def_id) {
            return;
        }
        // Re-pointing a parameter name is legal (callee-local); only a write
        // through it is refused.
        if self.repointable.contains(&def_id) {
            if !repoint {
                self.error(
                    SemanticErrorKind::AssignToImmutableBorrow,
                    crate::types::ISSUE_ASSIGN_TO_DE_PARAMETER,
                    span,
                );
            }
            return;
        }
        if matches!(
            self.states.get(&def_id).copied(),
            Some(InitState::Unassigned) | None
        ) {
            return;
        }
        self.error(
            SemanticErrorKind::ImmutableAssignment,
            ISSUE_IMMUTABLE_SECOND_ASSIGNMENT,
            span,
        );
    }

    /// A call to a method that changes its receiver in place is a write to the
    /// receiver binding, so it obeys the same const/var rule as a field write:
    /// an immutable binding (`fixum` local or non-`in` parameter, `copy`
    /// parameters included) rejects it.
    fn check_mutating_method_call(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        span: radix_lexer::Span,
    ) {
        let HirExpressionKind::Path(def_id) =
            &method_receiver_mutation::unwrap_non_null(receiver).kind
        else {
            return;
        };
        if !self.write_once.contains(def_id) {
            return;
        }
        let Some(def) = receiver
            .ty
            .and_then(|ty| method_receiver_mutation::class_def(self.types, ty))
        else {
            return;
        };
        if !self.mutating_methods.contains(&(def, method)) {
            return;
        }
        let binding = self
            .resolver
            .get_symbol(*def_id)
            .map_or("value", |symbol| self.interner.resolve(symbol.name))
            .to_owned();
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::ImmutableAssignment, span)
                .with_arg("issue", ISSUE_MUTATING_METHOD_ON_IMMUTABLE)
                .with_arg("binding", binding)
                .with_arg("method", self.interner.resolve(method)),
        );
    }

    fn read_binding(&mut self, def_id: DefId, span: radix_lexer::Span) {
        if self.sponte_slots.contains(&def_id) {
            return;
        }
        let Some(state) = self.states.get(&def_id).copied() else {
            return;
        };
        if !state.is_definitely_initialized() {
            self.error(
                SemanticErrorKind::UseBeforeInit,
                ISSUE_POSSIBLY_UNINITIALIZED,
                span,
            );
        }
    }

    fn assign_binding(&mut self, target: &HirExpression) {
        if let Some(def_id) = Self::root_def_id(target) {
            self.check_projection_inputs(target);
            // WHY: tensor bracket assignment (`output[id] ← v`) is sugar for
            // `ponde`, an in-place mutation through a shared heap reference —
            // not a rebinding of the tensor local. It must obey `ponde`
            // mutability (allowed on `fixum`), not the write-once rule for
            // scalar/struct bindings. The HIR object carries its typecheck
            // type, so a tensor-typed index base is a mutation, not an assign.
            if !self.is_tensor_index_mutation(target) {
                let repoint = matches!(target.kind, HirExpressionKind::Path(_));
                self.enforce_write_once(def_id, target.span, repoint);
                self.mark_assigned(def_id);
            }
        } else {
            self.visit_expr(target);
        }
    }

    /// Whether `target` is an `Index` projection whose base is a tensor type.
    /// Such targets are in-place mutations (`ponde` sugar), not rebindings, and
    /// are exempt from write-once enforcement.
    fn is_tensor_index_mutation(&self, target: &HirExpression) -> bool {
        let HirExpressionKind::Index(object, _) = &target.kind else {
            return false;
        };
        object
            .ty
            .is_some_and(|ty| crate::tensor_bracket::is_coordinate_bracket_receiver(self.types, ty))
    }

    /// Read-modify-write of an lvalue: visit projection inputs once, read the
    /// root binding, then mark it assigned. Used by compound assignments.
    fn op_assign(&mut self, target: &HirExpression) {
        if let Some(def_id) = Self::root_def_id(target) {
            self.check_projection_inputs(target);
            self.read_binding(def_id, target.span);
            let repoint = matches!(target.kind, HirExpressionKind::Path(_));
            self.enforce_write_once(def_id, target.span, repoint);
            self.mark_assigned(def_id);
        } else {
            self.visit_expr(target);
        }
    }

    fn assign_pattern_bindings(&mut self, patterns: &[HirPattern]) {
        for pattern in patterns {
            self.assign_pattern_binding(pattern);
        }
    }

    fn assign_pattern_binding(&mut self, pattern: &HirPattern) {
        match pattern {
            HirPattern::Binding(def_id, _) => {
                self.track_binding(*def_id);
                self.mark_assigned(*def_id);
            }
            HirPattern::Alias(def_id, _, inner) => {
                self.track_binding(*def_id);
                self.mark_assigned(*def_id);
                self.assign_pattern_binding(inner);
            }
            HirPattern::Variant(_, patterns) | HirPattern::Or(patterns) => {
                for pattern in patterns {
                    self.assign_pattern_binding(pattern);
                }
            }
            HirPattern::TypeTest { binding, .. } => {
                if let Some((def_id, _)) = binding {
                    self.track_binding(*def_id);
                    self.mark_assigned(*def_id);
                }
            }
            HirPattern::Wildcard | HirPattern::Literal(_) | HirPattern::NegatedLiteral(_) => {}
        }
    }

    fn check_cape(&mut self, cape: &HirCape) {
        self.track_binding(cape.binding_def_id);
        self.mark_assigned(cape.binding_def_id);
        self.check_block(&cape.body);
    }

    fn check_casu_arm(&mut self, arm: &HirCasuArm) {
        self.assign_pattern_bindings(&arm.patterns);
        self.visit_expr(&arm.body);
    }

    fn root_def_id(expr: &HirExpression) -> Option<DefId> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => Some(*def_id),
            HirExpressionKind::Field(object, _) | HirExpressionKind::Index(object, _) => {
                Self::root_def_id(object)
            }
            HirExpressionKind::Deref(inner) => Self::root_def_id(inner),
            _ => None,
        }
    }

    fn check_projection_inputs(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Field(object, _) | HirExpressionKind::Deref(object) => {
                self.check_projection_inputs(object);
            }
            HirExpressionKind::Index(object, index) => {
                self.check_projection_inputs(object);
                self.visit_expr(index);
            }
            _ => {}
        }
    }

    fn error(&mut self, kind: SemanticErrorKind, issue: &str, span: radix_lexer::Span) {
        self.errors
            .push(SemanticError::structured(kind, span).with_arg("issue", issue));
    }

    /// Run `branch_count` independent branches, each starting from `incoming`,
    /// and return the join of their exit states. A zero-branch call (e.g. an
    /// empty `discerne`) returns `incoming` unchanged.
    ///
    /// The call site must use `std::mem::take` to avoid cloning the full state
    /// map at the save point. Each branch still clones `incoming` (no structural
    /// sharing), but that cost is bounded by branch count × function scope size
    /// and is below noise for typical programs.
    fn run_branches(
        &mut self,
        branch_count: usize,
        incoming: FxHashMap<DefId, InitState>,
        mut body: impl FnMut(&mut Self, usize),
    ) -> FxHashMap<DefId, InitState> {
        let mut merged: Option<FxHashMap<DefId, InitState>> = None;
        for index in 0..branch_count {
            // PERF: each branch clones `incoming` so the body can mutate
            // `self.states` independently. A future persistent-map or delta-log
            // approach could eliminate this, but the cost is bounded and low for
            // realistic function scopes.
            self.states.clone_from(&incoming);
            body(self, index);
            let branch_state = std::mem::take(&mut self.states);
            merged = Some(match merged {
                None => branch_state,
                Some(acc) => join_states(acc, branch_state),
            });
        }
        merged.unwrap_or(incoming)
    }
}

/// Join two branch exit states.
///
/// Keys present in both maps are joined via [`InitState::join_paths`].
/// Keys present only in `b` are inserted as-is (they were absent from
/// earlier arms).  Keys present only in `a` are degraded by joining with
/// [`InitState::Unassigned`] because they were assigned on some paths
/// only. This covers arm-local pattern bindings from earlier branches
/// and one-sided assignments through nested control flow.
///
/// Arm-local bindings degrade to `MaybeAssigned` after the merge, but
/// they are out of scope afterwards and unreadable, so this is harmless
/// for correctness while keeping the lattice honest.
fn join_states(
    mut a: FxHashMap<DefId, InitState>,
    b: FxHashMap<DefId, InitState>,
) -> FxHashMap<DefId, InitState> {
    let mut seen: FxHashSet<DefId> = FxHashSet::default();
    for (key, rhs) in b {
        seen.insert(key);
        a.entry(key)
            .and_modify(|lhs| *lhs = lhs.join_paths(rhs))
            .or_insert(rhs);
    }
    // Keys only in `a`: join with Unassigned (absent from `b`).
    for (key, val) in &mut a {
        if !seen.contains(key) {
            *val = val.join_paths(InitState::Unassigned);
        }
    }
    a
}

impl HirVisitor for DefiniteAssignmentChecker<'_> {
    fn visit_block(&mut self, block: &HirBlock) {
        self.check_block(block);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.read_binding(*def_id, expr.span),
            HirExpressionKind::Assign(target, value) => {
                self.visit_expr(value);
                self.assign_binding(target);
            }
            // `target ⇇ value` writes the storage `target` already names: the
            // value is read, the target is read (it must be initialized), and
            // the target's root must be a writable binding. Unlike `←` it never
            // rebinds, so it is legal only on `varia` bindings and `mut`
            // parameters, however often it runs.
            HirExpressionKind::CopyInto { target, value } => {
                self.visit_expr(value);
                self.visit_expr(target);
                if let Some(def_id) = Self::root_def_id(target)
                    && self.write_once.contains(&def_id)
                {
                    self.error(
                        SemanticErrorKind::ImmutableAssignment,
                        ISSUE_COPY_INTO_TARGET_IMMUTABLE,
                        target.span,
                    );
                }
            }
            // `↤` stores exactly like ordinary `←`: the source (and its `⊥`
            // recovery) are reads, then the target binding is assigned.
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                ..
            } => {
                self.visit_expr(source);
                if let Some(recovery) = recovery {
                    self.visit_expr(recovery);
                }
                self.assign_binding(target);
            }

            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for scrutinee in scrutinees {
                    self.visit_expr(scrutinee);
                }
                // PRP-057: `take` avoids O(N) clone at discerne join point.
                let incoming = std::mem::take(&mut self.states);
                self.states = self.run_branches(arms.len(), incoming, |checker, index| {
                    checker.check_casu_arm(&arms[index]);
                });
            }
            HirExpressionKind::Itera {
                binding,
                iter,
                body: block,
                mutable,
                ..
            } => {
                self.track_binding(*binding);
                // WHY: a `let` (`sit`) loop binder is a fixed name written once
                // by the loop head, so any store through it is a second
                // assignment. `mut` (`varia`) binders stay writable.
                if !*mutable {
                    self.write_once.insert(*binding);
                }
                self.visit_expr(iter);
                self.mark_assigned(*binding);
                self.visit_block(block);
            }
            HirExpressionKind::IteraLockstep {
                sources,
                bindings,
                body,
                mutable,
                ..
            } => {
                for source in sources {
                    self.visit_expr(source);
                }
                for (binding, _) in bindings {
                    self.track_binding(*binding);
                    if !*mutable {
                        self.write_once.insert(*binding);
                    }
                    self.mark_assigned(*binding);
                }
                self.visit_block(body);
            }
            HirExpressionKind::IteraRange {
                ranges,
                bindings,
                body,
                mutable,
                ..
            } => {
                for range in ranges {
                    self.visit_expr(range);
                }
                for (binding, _) in bindings {
                    self.track_binding(*binding);
                    if !*mutable {
                        self.write_once.insert(*binding);
                    }
                    self.mark_assigned(*binding);
                }
                self.visit_block(body);
            }
            HirExpressionKind::Reduction {
                source,
                group,
                binding,
                body,
                identity,
                mutable,
                ..
            } => {
                self.visit_expr(source);
                if let Some(group) = group {
                    self.visit_expr(group);
                }
                if let Some(identity) = identity {
                    self.visit_expr(identity);
                }
                if let Some((def_id, _)) = binding {
                    self.track_binding(*def_id);
                    if !*mutable {
                        self.write_once.insert(*def_id);
                    }
                    self.mark_assigned(*def_id);
                }
                if let Some(body) = body {
                    self.visit_block(body);
                }
            }
            HirExpressionKind::Scan {
                source,
                group,
                binding,
                body,
                ..
            } => {
                self.visit_expr(source);
                if let Some(group) = group {
                    self.visit_expr(group);
                }
                self.track_binding(binding.0);
                self.write_once.insert(binding.0);
                self.mark_assigned(binding.0);
                self.visit_block(body);
            }
            HirExpressionKind::Sort {
                source,
                binding,
                body,
                mutable,
                ..
            } => {
                self.visit_expr(source);
                if let Some((def_id, _)) = binding {
                    self.track_binding(*def_id);
                    if !*mutable {
                        self.write_once.insert(*def_id);
                    }
                    self.mark_assigned(*def_id);
                }
                if let Some(body) = body {
                    self.visit_block(body);
                }
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
                // PRP-057: `take` avoids O(N) clone of the full state map when
                // entering a new closure scope. The outer state is swapped out
                // (O(1)) and restored after the body is analysed.
                let saved_states = std::mem::take(&mut self.states);
                let saved_sponte = std::mem::take(&mut self.sponte_slots);
                for param in params {
                    self.register_param(param);
                }
                self.visit_expr(body);
                self.states = saved_states;
                self.sponte_slots = saved_sponte;
            }
            HirExpressionKind::Handled { body, catch } => {
                self.visit_block(body);
                self.check_cape(catch);
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                self.visit_expr(cond);
                // TARGET: merge path states so conditional initialization is sound.
                // GRAMMAR: `si` has an optional `secus`/`sin` else branch; an
                // else-less `si` is modeled as a no-op skip branch carrying the
                // incoming state, so partial assignment degrades to MaybeAssigned.
                // PRP-057: `take` avoids O(N) clone at si join point.
                let incoming = std::mem::take(&mut self.states);
                self.states = self.run_branches(2, incoming, |checker, index| {
                    if index == 0 {
                        checker.visit_block(then_block);
                        if let Some(catch) = then_catch {
                            checker.check_cape(catch);
                        }
                    } else if let Some(else_block) = else_block {
                        checker.visit_block(else_block);
                    }
                });
            }
            HirExpressionKind::Ref(_, inner) => self.visit_expr(inner),
            HirExpressionKind::MethodCall(receiver, method, ..) => {
                self.check_mutating_method_call(receiver, *method, expr.span);
                walk_expr(self, expr);
            }
            _ => walk_expr(self, expr),
        }
    }

    fn visit_cape(&mut self, cape: &HirCape) {
        self.check_cape(cape);
    }
}
