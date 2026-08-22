//! Parameter-mode and Rust borrow analysis for typed HIR.
//!
//! Two entry points implement the HIR v1 G2 split:
//!
//! - [`analyze_modes`] — **target-neutral** access-fact policy: use-after-
//!   consume, illegal mutation through `ref`, and incompatible access passing.
//!   Runs for every application-lane target.
//! - [`analyze`] — Rust uses the same declared-fact rules; it does not add a
//!   simulated lifetime policy.
//!
//! Precise lifetime reasoning remains Rust's job. Declared access validity is
//! decided before target selection so TS/Go cannot silently accept programs
//! that violate Faber ownership facts.
//!
//! ERROR STRATEGY
//! ==============
//! - Ownership violations are emitted as semantic errors.
//! - Parameter-mode suggestions are emitted as semantic warnings.
//! - Declared `ref` type facts (MU-2) reject projection assignment and
//!   passing into `mut`/`own` positions. Mutating-receiver bans live with
//!   catalog lookup in typecheck; this pass does not key them on void return.
//! - Declared `own` type facts (MU-3) transfer at bind sites and own-param
//!   calls. SEM050 (`use_after_move`) is a language rule, mode-scoped to
//!   exclusive (`own`/`copy`) values. Unmarked Move-mode args still move only
//!   under the Rust-lifetime qualification.
//! - Declared `mut` type facts (MU-4) make the mutable-borrow-conflict family
//!   (SEM052) a language rule, mode-scoped to exclusive-borrow operations:
//!   mut-local binds, `mut`/`ref` dest calls, and explicit refs of `mut` roots.
//!   Unmarked overlapping `&`/`&mut` still fire only under Rust-lifetime
//!   qualification. `mut` locals parse and warn (OQ-3).
//! - The pass returns all findings together so the semantic driver can apply
//!   the repository-wide warning/error policy through `SemanticErrorKind`.
//!
//! INVARIANTS
//! ==========
//! - Borrow state is keyed by root `DefId`; field, index, and deref
//!   projections borrow or move the base value.
//! - Borrow lifetimes are scope approximations: explicit borrow expressions are
//!   registered in the current `BorrowScope` and released when that HIR block
//!   exits.
//! - Mutating method calls are receiver writes, not retained borrows; this
//!   keeps statement-position collection intrinsics such as `xs.appende(v)`
//!   reusable within one block.
//! - The `TypeTable` is consulted only for callee signatures, because argument
//!   modes determine whether a call reads, mutably borrows, or moves an input.
//! - Warnings produced here remain non-fatal unless the semantic driver or
//!   diagnostic consumer treats warning-kind diagnostics as errors.

use crate::{
    AccessMarker, FuncSig, ParamType, Primitive, Resolver, SemanticError, SemanticErrorKind,
    SemanticParamMode, Type, TypeId, TypeTable, WarningKind, ISSUE_ASSIGN_THROUGH_REF,
    ISSUE_MUT_INTO_OWN, ISSUE_MUT_LOCAL, ISSUE_REF_TO_MUT_OR_OWN_POSITION, ISSUE_UNMARKED_INTO_OWN,
    SUGGESTION_COPY,
};
use radix_hir::visit::{walk_expr, HirVisitor};
use radix_hir::{
    DefId, HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirFunction, HirItem,
    HirItemKind, HirModule, HirParamMode, HirStatement, HirStatementKind,
};
use rustc_hash::{FxHashMap, FxHashSet};

const ISSUE_ASSIGN_TO_DE_PARAMETER: &str = "assign_to_de_parameter";
const ISSUE_BORROW_OF_MOVED: &str = "borrow_of_moved";
const ISSUE_CANNOT_MOVE_WHILE_BORROWED: &str = "cannot_move_while_borrowed";
const ISSUE_DE_TO_IN_OR_EX_POSITION: &str = "de_to_in_or_ex_position";
const ISSUE_MUTABLE_BORROW_CONFLICT: &str = "mutable_borrow_conflict";
const ISSUE_SHARED_BORROW_CONFLICT: &str = "shared_borrow_conflict";
const ISSUE_UNUSED_OWN_PARAMETER: &str = "unused_own_parameter";
const ISSUE_UNUSED_MUT_PARAMETER: &str = "unused_mut_parameter";
const ISSUE_USE_AFTER_MOVE: &str = "use_after_move";
const ISSUE_USE_WHILE_MUTABLY_BORROWED: &str = "use_while_mutably_borrowed";
const ISSUE_WRITE_WHILE_BORROWED: &str = "write_while_borrowed";

/// Target-neutral Faber access-fact analysis.
///
/// Language `ref`/`mut`/`own`/`copy` facts run here. Rust lifetime/borrow
/// conflict approximation for unmarked values does not. Use this during base
/// analysis so all HIR targets share access validity.
///
/// # Errors
///
/// Returns access violations as a list of [`SemanticError`]s.
pub fn analyze_modes(
    hir: &HirModule,
    _resolver: &Resolver,
    types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    run(hir, types, false, true)
}

/// Legacy Rust-target analysis: declared access facts plus the Rust lifetime
/// approximation used by [`PassConfig::for_target`](crate::PassConfig::for_target).
/// Target-neutral callers use [`analyze_modes`], while the explicit qualification
/// seam uses [`analyze_rust_lifetime`].
///
/// # Errors
///
/// Returns borrow/lifetime violations as a list of [`SemanticError`]s.
pub fn analyze(
    hir: &HirModule,
    _resolver: &Resolver,
    types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    run(hir, types, true, true)
}

/// Retired Rust lifetime simulation hook.
///
/// The driver still calls this target-qualification seam, but semantic
/// lifetime simulation is no longer policy. Generated Rust and its compiler
/// own precise borrow checking; declared `ref`/`mut`/`own`/`copy` facts are
/// enforced by [`analyze_modes`] instead.
///
/// # Errors
///
/// Always returns `Ok(())`; the result type remains for the qualification API.
pub fn analyze_rust_lifetime(
    _hir: &HirModule,
    _resolver: &Resolver,
    _types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    Ok(())
}

fn run(
    hir: &HirModule,
    types: &TypeTable,
    rust_lifetime: bool,
    language_conflicts: bool,
) -> Result<(), Vec<SemanticError>> {
    let mut checker = BorrowChecker::new(types, rust_lifetime, language_conflicts);
    checker.check_program(hir);

    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

/// Per-definition ownership state tracked at the root value level.
///
/// The pass intentionally does not model partial moves or field-granular
/// borrowing. Projections collapse through `root_def_id`, which keeps the
/// pre-codegen check conservative and aligned with the current HIR surface.
#[derive(Clone, Copy, Default)]
struct BorrowState {
    moved: bool,
    shared: u32,
    mutable: bool,
}

#[derive(Clone, Copy)]
enum BorrowKind {
    Shared,
    Mutable,
}

struct BorrowScope {
    borrows: Vec<(DefId, BorrowKind)>,
}

struct BorrowChecker<'a> {
    /// Semantic type table produced before analysis; call signatures determine
    /// whether each argument is read, borrowed, or moved.
    types: &'a TypeTable,
    /// Retained only for the retired qualification implementation path;
    /// normal analysis never enables simulated Rust lifetime diagnostics.
    rust_lifetime: bool,
    /// When true, emit the mutable-borrow-conflict family as language rules
    /// (mut-local binds, `mut`/`ref` dests, explicit refs of `mut` roots).
    language_conflicts: bool,
    states: FxHashMap<DefId, BorrowState>,
    scopes: Vec<BorrowScope>,
    param_usage: FxHashMap<DefId, ParamUsage>,
    /// Bindings whose declared type carries a `ref` access fact.
    ref_roots: FxHashSet<DefId>,
    /// Bindings whose declared type carries a `mut` access fact.
    mut_roots: FxHashSet<DefId>,
    /// Bindings whose declared type is an exclusive `own`/`copy` value.
    own_roots: FxHashSet<DefId>,
    /// Bindings whose declared type acquires a duplicate-then-own `copy` value.
    copy_roots: FxHashSet<DefId>,
    /// Unmarked sources used as an own-bind RHS; a later use proves them live.
    unmarked_own_binds: FxHashMap<DefId, radix_lexer::Span>,
    /// Function param contracts keyed by callee `DefId` (named calls leave
    /// `callee.ty` unset; this index supplies own-param dest facts).
    func_params: FxHashMap<DefId, Vec<ParamType>>,
    errors: Vec<SemanticError>,
}

/// Usage summary for one function parameter after traversing its body.
///
/// This is policy state rather than ownership state: it decides whether the
/// declared access fact is stronger than the function actually needs.
#[derive(Clone, Copy)]
struct ParamUsage {
    mode: HirParamMode,
    marker: Option<AccessMarker>,
    span: radix_lexer::Span,
    mutated: bool,
    passed_in_or_ex: bool,
    passed_ex: bool,
    returned: bool,
    moved: bool,
}

impl<'a> BorrowChecker<'a> {
    fn new(types: &'a TypeTable, rust_lifetime: bool, language_conflicts: bool) -> Self {
        Self {
            types,
            rust_lifetime,
            language_conflicts,
            states: FxHashMap::default(),
            scopes: Vec::new(),
            param_usage: FxHashMap::default(),
            ref_roots: FxHashSet::default(),
            mut_roots: FxHashSet::default(),
            own_roots: FxHashSet::default(),
            copy_roots: FxHashSet::default(),
            unmarked_own_binds: FxHashMap::default(),
            func_params: FxHashMap::default(),
            errors: Vec::new(),
        }
    }

    fn check_program(&mut self, hir: &HirModule) {
        for item in &hir.items {
            self.index_item(item);
        }
        for item in &hir.items {
            self.check_item(item);
        }

        if let Some(entry) = &hir.entry {
            self.reset();
            self.check_block(entry);
        }
    }

    fn index_item(&mut self, item: &HirItem) {
        match &item.kind {
            HirItemKind::Function(func) => self.index_function(item.def_id, func),
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    self.index_function(method.def_id, &method.func);
                }
            }
            _ => {}
        }
    }

    fn index_function(&mut self, def_id: DefId, func: &HirFunction) {
        let params = func
            .params
            .iter()
            .map(|param| ParamType {
                ty: param.ty,
                mode: match param.mode {
                    HirParamMode::Owned => SemanticParamMode::Owned,
                    HirParamMode::De => SemanticParamMode::Ref,
                    HirParamMode::In => SemanticParamMode::MutRef,
                    HirParamMode::Ex => SemanticParamMode::Move,
                },
                optional: param.optional,
            })
            .collect();
        self.func_params.insert(def_id, params);
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
                self.check_expr(&const_item.value);
            }
            _ => {}
        }
    }

    fn check_function(&mut self, func: &HirFunction) {
        self.reset();
        for param in &func.params {
            self.ensure_state(param.def_id);
            self.note_ref_root(param.def_id, param.ty);
            self.note_mut_root(param.def_id, param.ty);
            self.note_copy_root(param.def_id, param.ty);
            self.note_own_root(param.def_id, param.ty);
            self.param_usage.insert(
                param.def_id,
                ParamUsage {
                    mode: param.mode,
                    marker: self.types.peel_access_marker(param.ty).0,
                    span: param.span,
                    mutated: false,
                    passed_in_or_ex: false,
                    passed_ex: false,
                    returned: false,
                    moved: false,
                },
            );
        }
        if let Some(body) = &func.body {
            self.check_block(body);
            self.emit_mode_lints();
        }
    }

    fn reset(&mut self) {
        self.states.clear();
        self.scopes.clear();
        self.param_usage.clear();
        self.ref_roots.clear();
        self.mut_roots.clear();
        self.own_roots.clear();
        self.copy_roots.clear();
        self.unmarked_own_binds.clear();
    }

    fn check_block(&mut self, block: &HirBlock) {
        self.push_scope();
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
        if let Some(expr) = &block.expr {
            self.check_expr(expr);
        }
        self.pop_scope();
    }

    fn check_stmt(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Local(local) => {
                self.ensure_state(local.def_id);
                if let Some(ty) = local.ty {
                    self.note_ref_root(local.def_id, ty);
                    self.note_mut_root(local.def_id, ty);
                    self.note_copy_root(local.def_id, ty);
                    self.note_own_root(local.def_id, ty);
                    if self.language_conflicts && self.types.is_mut_view(ty) {
                        self.warn_mut_local(stmt.span);
                    }
                }
                if let Some(init) = &local.init {
                    if local.ty.is_some_and(|ty| self.types.is_copy_view(ty)) {
                        self.bind_copy_from(init);
                    } else if local.ty.is_some_and(|ty| self.types.is_own_view(ty)) {
                        self.bind_own_from(init);
                    } else if self.language_conflicts
                        && local.ty.is_some_and(|ty| self.types.is_mut_view(ty))
                    {
                        self.bind_mut_from(init);
                    } else if self.language_conflicts
                        && local.ty.is_some_and(|ty| self.types.is_ref_view(ty))
                    {
                        self.bind_ref_from(init);
                    } else {
                        self.check_expr(init);
                    }
                }
            }
            HirStatementKind::Expr(expr) => self.check_expr(expr),
            HirStatementKind::Redde(value) => {
                if let Some(expr) = value {
                    if let Some(def_id) = Self::root_def_id(expr) {
                        if let Some(usage) = self.param_usage.get_mut(&def_id) {
                            usage.returned = true;
                        }
                    }
                    self.check_expr(expr);
                }
            }
            HirStatementKind::Rumpe | HirStatementKind::Perge | HirStatementKind::Tacet => {}
            HirStatementKind::IncDec(inc_dec) => self.check_lvalue(&inc_dec.target),
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    self.check_expr(&clause.cond);
                    self.check_block(&clause.body);
                }
            }
        }
    }

    fn check_expr(&mut self, expr: &HirExpression) {
        self.visit_expr(expr);
    }

    fn callee_signature(types: &TypeTable, ty: TypeId) -> Option<&FuncSig> {
        match types.get(ty) {
            Type::Func(sig) => Some(sig),
            Type::Alias(_, resolved) => match types.get(*resolved) {
                Type::Func(sig) => Some(sig),
                _ => None,
            },
            _ => None,
        }
    }

    fn call_params(&self, callee: &HirExpression) -> Option<&[ParamType]> {
        if let Some(ty) = callee.ty {
            return Self::callee_signature(self.types, ty).map(|sig| sig.params.as_slice());
        }
        match &callee.kind {
            HirExpressionKind::Path(def_id) => self.func_params.get(def_id).map(Vec::as_slice),
            _ => None,
        }
    }

    fn check_call_args(&mut self, callee: &HirExpression, args: &[HirCallArg]) {
        let Some(params) = self.call_params(callee) else {
            for arg in args {
                self.check_expr(&arg.expr);
            }
            return;
        };
        let params = params.to_vec();
        // Named function Paths leave `callee.ty` unset. Applying the retired
        // lifetime simulation to those calls would reject D-1 unmarked
        // `const` → `mut` param use. Language `mut` dest borrows are
        // call-scoped so sequential use after a single `mut` param call stays
        // legal. Own-param transfers still run below.
        let rust_sim = callee.ty.is_some();
        let language_call_borrows = self.language_conflicts && !(rust_sim && self.rust_lifetime);
        // Named function Paths leave `callee.ty` unset. Their mut/ref dest-call
        // borrows are call-scoped in every pass: real Rust releases an
        // argument's `&mut`/`&` reborrow when the call ends, so sequential use
        // after a single `mut` param call stays legal (D-1). Push a call scope
        // so even the rust-lifetime pass releases these at call end instead of
        // holding the borrow to end-of-block.
        let call_scoped = !rust_sim;
        if language_call_borrows || call_scoped {
            self.push_scope();
        }

        for (arg, param) in args.iter().zip(params.iter()) {
            if let Some(arg_def_id) = Self::root_def_id(&arg.expr) {
                if let Some(arg_usage) = self.param_usage.get_mut(&arg_def_id) {
                    if matches!(
                        param.mode,
                        SemanticParamMode::MutRef | SemanticParamMode::Move
                    ) {
                        arg_usage.passed_in_or_ex = true;
                    }
                    if matches!(param.mode, SemanticParamMode::Move) {
                        arg_usage.passed_ex = true;
                    }
                    if matches!(arg_usage.mode, HirParamMode::De)
                        && matches!(
                            param.mode,
                            SemanticParamMode::MutRef | SemanticParamMode::Move
                        )
                    {
                        self.error(
                            SemanticErrorKind::ModeMismatch,
                            ISSUE_DE_TO_IN_OR_EX_POSITION,
                            arg.expr.span,
                        );
                    }
                }
            }
            if self.arg_is_ref_view(&arg.expr) && self.dest_is_mut_or_own(param) {
                self.error(
                    SemanticErrorKind::ModeMismatch,
                    ISSUE_REF_TO_MUT_OR_OWN_POSITION,
                    arg.expr.span,
                );
            }
            let dest_copy = self.types.is_copy_view(param.ty);
            let dest_own = self.types.is_own_view(param.ty);
            if self.arg_is_mut_view(&arg.expr) && dest_own {
                self.error(
                    SemanticErrorKind::ModeMismatch,
                    ISSUE_MUT_INTO_OWN,
                    arg.expr.span,
                );
            }
            if dest_copy {
                self.bind_copy_from(&arg.expr);
                continue;
            }
            if dest_own && self.arg_is_exclusive(&arg.expr) {
                self.move_from_expr(&arg.expr);
                continue;
            }
            if language_call_borrows {
                if self.dest_is_mut(param) {
                    self.borrow_from_expr(&arg.expr, BorrowKind::Mutable);
                } else if self.dest_is_ref(param) {
                    self.borrow_from_expr(&arg.expr, BorrowKind::Shared);
                } else {
                    self.check_expr(&arg.expr);
                }
                continue;
            }
            match param.mode {
                SemanticParamMode::Ref => {
                    if self.rust_lifetime {
                        self.borrow_from_expr(&arg.expr, BorrowKind::Shared);
                    } else {
                        // Mode-only: treat as a read of the root value.
                        if let Some(def_id) = Self::root_def_id(&arg.expr) {
                            self.check_projection_inputs(&arg.expr);
                            self.read_use(def_id, arg.expr.span);
                        } else {
                            self.check_expr(&arg.expr);
                        }
                    }
                }
                SemanticParamMode::MutRef => {
                    if self.rust_lifetime {
                        self.borrow_from_expr(&arg.expr, BorrowKind::Mutable);
                    } else if let Some(def_id) = Self::root_def_id(&arg.expr) {
                        self.check_projection_inputs(&arg.expr);
                        // Mutate demand without registering a Rust borrow scope.
                        self.read_use(def_id, arg.expr.span);
                    } else {
                        self.check_expr(&arg.expr);
                    }
                }
                SemanticParamMode::Move => {
                    // Language: only exclusive values move. Rust lifetime still
                    // consumes every Move-mode argument (existing simulation).
                    if self.rust_lifetime || self.arg_is_exclusive(&arg.expr) {
                        self.move_from_expr(&arg.expr);
                    } else {
                        self.check_expr(&arg.expr);
                    }
                }
                SemanticParamMode::Owned if dest_own => {
                    if self.arg_is_exclusive(&arg.expr) {
                        self.move_from_expr(&arg.expr);
                    } else {
                        self.check_expr(&arg.expr);
                    }
                }
                SemanticParamMode::Owned => self.check_expr(&arg.expr),
            }
        }
        if language_call_borrows || call_scoped {
            self.pop_scope();
        }
    }

    fn check_lvalue(&mut self, target: &HirExpression) {
        if Self::is_projection(target) {
            if let Some(def_id) = Self::root_def_id(target) {
                if self.ref_roots.contains(&def_id) {
                    self.error(
                        SemanticErrorKind::AssignToImmutableBorrow,
                        ISSUE_ASSIGN_THROUGH_REF,
                        target.span,
                    );
                }
            }
        }
        if let Some(def_id) = Self::root_def_id(target) {
            self.check_projection_inputs(target);
            self.write_use(def_id, target.span);
        } else {
            self.check_expr(target);
        }
    }

    fn borrow_from_expr(&mut self, expr: &HirExpression, kind: BorrowKind) {
        if let Some(def_id) = Self::root_def_id(expr) {
            self.check_projection_inputs(expr);
            match kind {
                BorrowKind::Shared => self.borrow_shared(def_id, expr.span),
                BorrowKind::Mutable => self.borrow_mut(def_id, expr.span),
            }
        } else {
            self.check_expr(expr);
        }
    }

    fn move_from_expr(&mut self, expr: &HirExpression) {
        if let Some(def_id) = Self::root_def_id(expr) {
            self.check_projection_inputs(expr);
            self.move_use(def_id, expr.span);
        } else {
            self.check_expr(expr);
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn root_def_id(expr: &HirExpression) -> Option<DefId> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => Some(*def_id),
            HirExpressionKind::Field(object, _) => Self::root_def_id(object),
            HirExpressionKind::Index(object, _) => Self::root_def_id(object),
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
                self.check_expr(index);
            }
            _ => {}
        }
    }

    fn ensure_state(&mut self, def_id: DefId) {
        self.states.entry(def_id).or_default();
    }

    fn note_ref_root(&mut self, def_id: DefId, ty: TypeId) {
        if self.types.is_ref_view(ty) {
            self.ref_roots.insert(def_id);
        }
    }

    fn note_mut_root(&mut self, def_id: DefId, ty: TypeId) {
        if self.types.is_mut_view(ty) {
            self.mut_roots.insert(def_id);
        }
    }

    fn note_copy_root(&mut self, def_id: DefId, ty: TypeId) {
        if self.types.is_copy_view(ty) {
            self.copy_roots.insert(def_id);
        }
    }

    fn note_own_root(&mut self, def_id: DefId, ty: TypeId) {
        if self.types.is_exclusive_value(ty) {
            self.own_roots.insert(def_id);
        }
    }

    /// Explicit `&`/`&mut` on unmarked values stay Rust-lifetime only.
    /// `mut` roots are language (MU-4). Combined analysis tracks both.
    fn tracks_explicit_ref(&self, def_id: DefId) -> bool {
        if self.rust_lifetime {
            self.language_conflicts || !self.mut_roots.contains(&def_id)
        } else {
            self.language_conflicts && self.mut_roots.contains(&def_id)
        }
    }

    /// Exclusive-borrow bind of a `mut` local from a path (or projection root).
    fn bind_mut_from(&mut self, rhs: &HirExpression) {
        self.borrow_from_expr(rhs, BorrowKind::Mutable);
    }

    /// Shared-borrow bind of a `ref` local from a path (or projection root).
    fn bind_ref_from(&mut self, rhs: &HirExpression) {
        self.borrow_from_expr(rhs, BorrowKind::Shared);
    }

    /// Bind-site duplicate acquisition for a `copy` destination.
    ///
    /// The source is read, never moved. MU-6 materializes the deep duplicate;
    /// this pass records the semantic event by preserving source liveness while
    /// the destination remains an exclusive value.
    fn bind_copy_from(&mut self, rhs: &HirExpression) {
        self.check_expr(rhs);
    }

    /// Bind-site move tracking for an `own` destination.
    ///
    /// Exclusive sources transfer (SEM050 on later use). Unmarked sources are
    /// tentatively accepted as provably-dead; a later use rejects the bind
    /// with a `copy` suggestion. Fresh RHS is a no-move read of the tree.
    fn bind_own_from(&mut self, rhs: &HirExpression) {
        match &rhs.kind {
            HirExpressionKind::Path(_) if self.arg_is_exclusive(rhs) => {
                self.move_from_expr(rhs);
            }
            HirExpressionKind::Path(def_id) => {
                self.check_expr(rhs);
                self.unmarked_own_binds.insert(*def_id, rhs.span);
            }
            _ => self.check_expr(rhs),
        }
    }

    fn arg_is_exclusive(&self, expr: &HirExpression) -> bool {
        if let Some(ty) = expr.ty {
            if self.types.is_exclusive_value(ty) {
                return true;
            }
        }
        match Self::root_def_id(expr) {
            Some(def_id) => self.own_roots.contains(&def_id),
            None => false,
        }
    }

    fn note_live_unmarked_own_bind(&mut self, def_id: DefId) {
        if let Some(span) = self.unmarked_own_binds.remove(&def_id) {
            self.error_own_bind(ISSUE_UNMARKED_INTO_OWN, span);
        }
    }

    fn dest_is_mut_or_own(&self, param: &ParamType) -> bool {
        matches!(
            param.mode,
            SemanticParamMode::MutRef | SemanticParamMode::Move
        ) || self.types.is_mut_or_own_view(param.ty)
    }

    fn dest_is_mut(&self, param: &ParamType) -> bool {
        matches!(param.mode, SemanticParamMode::MutRef) || self.types.is_mut_view(param.ty)
    }

    fn dest_is_ref(&self, param: &ParamType) -> bool {
        matches!(param.mode, SemanticParamMode::Ref) || self.types.is_ref_view(param.ty)
    }

    fn arg_is_ref_view(&self, expr: &HirExpression) -> bool {
        if let Some(ty) = expr.ty {
            if self.types.is_ref_view(ty) {
                return true;
            }
        }
        if let Some(def_id) = Self::root_def_id(expr) {
            if self.ref_roots.contains(&def_id) {
                return true;
            }
        }
        match &expr.kind {
            HirExpressionKind::Field(object, _)
            | HirExpressionKind::Index(object, _)
            | HirExpressionKind::Deref(object) => self.arg_is_ref_view(object),
            _ => false,
        }
    }

    fn arg_is_mut_view(&self, expr: &HirExpression) -> bool {
        if let Some(ty) = expr.ty {
            if self.types.is_mut_view(ty) {
                return true;
            }
        }
        if let Some(def_id) = Self::root_def_id(expr) {
            if self.mut_roots.contains(&def_id) {
                return true;
            }
        }
        match &expr.kind {
            HirExpressionKind::Field(object, _)
            | HirExpressionKind::Index(object, _)
            | HirExpressionKind::Deref(object) => self.arg_is_mut_view(object),
            _ => false,
        }
    }

    fn is_projection(expr: &HirExpression) -> bool {
        matches!(
            expr.kind,
            HirExpressionKind::Field(_, _)
                | HirExpressionKind::Index(_, _)
                | HirExpressionKind::Deref(_)
        )
    }

    fn read_use(&mut self, def_id: DefId, span: radix_lexer::Span) {
        self.note_live_unmarked_own_bind(def_id);
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            self.error(SemanticErrorKind::UseAfterMove, ISSUE_USE_AFTER_MOVE, span);
            return;
        }
        if state.mutable {
            self.error(
                SemanticErrorKind::MutableBorrowConflict,
                ISSUE_USE_WHILE_MUTABLY_BORROWED,
                span,
            );
        }
    }

    fn write_use(&mut self, def_id: DefId, span: radix_lexer::Span) {
        self.note_live_unmarked_own_bind(def_id);
        if let Some(usage) = self.param_usage.get_mut(&def_id) {
            if matches!(usage.mode, HirParamMode::De) {
                self.error(
                    SemanticErrorKind::AssignToImmutableBorrow,
                    ISSUE_ASSIGN_TO_DE_PARAMETER,
                    span,
                );
            } else if matches!(usage.mode, HirParamMode::In) {
                usage.mutated = true;
            }
        }
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            self.error(SemanticErrorKind::UseAfterMove, ISSUE_USE_AFTER_MOVE, span);
            return;
        }
        if state.mutable || state.shared > 0 {
            self.error(
                SemanticErrorKind::MutableBorrowConflict,
                ISSUE_WRITE_WHILE_BORROWED,
                span,
            );
        }
    }

    fn move_use(&mut self, def_id: DefId, span: radix_lexer::Span) {
        self.note_live_unmarked_own_bind(def_id);
        if let Some(usage) = self.param_usage.get_mut(&def_id) {
            usage.moved = true;
        }
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            self.error(SemanticErrorKind::UseAfterMove, ISSUE_USE_AFTER_MOVE, span);
            return;
        }
        if state.mutable || state.shared > 0 {
            self.error(
                SemanticErrorKind::CannotMoveOut,
                ISSUE_CANNOT_MOVE_WHILE_BORROWED,
                span,
            );
            return;
        }
        state.moved = true;
    }

    fn borrow_shared(&mut self, def_id: DefId, span: radix_lexer::Span) {
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            // Use-after-consume is language policy even without Rust lifetime.
            self.error(
                if self.rust_lifetime {
                    SemanticErrorKind::BorrowOfMoved
                } else {
                    SemanticErrorKind::UseAfterMove
                },
                if self.rust_lifetime {
                    ISSUE_BORROW_OF_MOVED
                } else {
                    ISSUE_USE_AFTER_MOVE
                },
                span,
            );
            return;
        }
        if state.mutable {
            self.error(
                SemanticErrorKind::MutableBorrowConflict,
                ISSUE_SHARED_BORROW_CONFLICT,
                span,
            );
            return;
        }
        state.shared = state.shared.saturating_add(1);
        if let Some(scope) = self.scopes.last_mut() {
            scope.borrows.push((def_id, BorrowKind::Shared));
        }
    }

    fn borrow_mut(&mut self, def_id: DefId, span: radix_lexer::Span) {
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            self.error(
                if self.rust_lifetime {
                    SemanticErrorKind::BorrowOfMoved
                } else {
                    SemanticErrorKind::UseAfterMove
                },
                if self.rust_lifetime {
                    ISSUE_BORROW_OF_MOVED
                } else {
                    ISSUE_USE_AFTER_MOVE
                },
                span,
            );
            return;
        }
        if state.mutable || state.shared > 0 {
            self.error(
                SemanticErrorKind::MutableBorrowConflict,
                ISSUE_MUTABLE_BORROW_CONFLICT,
                span,
            );
            return;
        }
        state.mutable = true;
        if let Some(scope) = self.scopes.last_mut() {
            scope.borrows.push((def_id, BorrowKind::Mutable));
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(BorrowScope {
            borrows: Vec::new(),
        });
    }

    fn pop_scope(&mut self) {
        let Some(scope) = self.scopes.pop() else {
            return;
        };
        for (def_id, kind) in scope.borrows {
            if let Some(state) = self.states.get_mut(&def_id) {
                match kind {
                    BorrowKind::Shared => {
                        state.shared = state.shared.saturating_sub(1);
                    }
                    BorrowKind::Mutable => {
                        state.mutable = false;
                    }
                }
            }
        }
    }

    fn error(&mut self, kind: SemanticErrorKind, issue: &str, span: radix_lexer::Span) {
        self.errors
            .push(SemanticError::structured(kind, span).with_arg("issue", issue));
    }

    fn warn_mut_local(&mut self, span: radix_lexer::Span) {
        self.errors.push(
            SemanticError::structured(
                SemanticErrorKind::Warning(WarningKind::DiscouragedMutLocal),
                span,
            )
            .with_arg("issue", ISSUE_MUT_LOCAL),
        );
    }

    fn error_own_bind(&mut self, issue: &str, span: radix_lexer::Span) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::ModeMismatch, span)
                .with_arg("issue", issue)
                .with_arg("suggestion", SUGGESTION_COPY),
        );
    }

    fn emit_mode_lints(&mut self) {
        let mut warnings = Vec::new();
        for usage in self.param_usage.values() {
            match usage.marker {
                Some(AccessMarker::Mut) => {
                    if !usage.mutated && !usage.passed_in_or_ex {
                        warnings.push(
                            SemanticError::structured(
                                SemanticErrorKind::Warning(WarningKind::UnusedMutRefParam),
                                usage.span,
                            )
                            .with_arg("issue", ISSUE_UNUSED_MUT_PARAMETER),
                        );
                    }
                }
                Some(AccessMarker::Own) => {
                    if !usage.passed_ex && !usage.returned && !usage.moved {
                        warnings.push(
                            SemanticError::structured(
                                SemanticErrorKind::Warning(WarningKind::UnusedMoveParam),
                                usage.span,
                            )
                            .with_arg("issue", ISSUE_UNUSED_OWN_PARAMETER),
                        );
                    }
                }
                Some(AccessMarker::Ref | AccessMarker::Copy) | None => {}
            }
        }
        self.errors.extend(warnings);
    }
}

impl HirVisitor for BorrowChecker<'_> {
    fn visit_block(&mut self, block: &HirBlock) {
        self.check_block(block);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.read_use(*def_id, expr.span),
            HirExpressionKind::Call(callee, _, args) => {
                self.visit_expr(callee);
                self.check_call_args(callee, args);
            }
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for scrutinee in scrutinees {
                    self.visit_expr(scrutinee);
                }
                for arm in arms {
                    self.push_scope();
                    if let Some(guard) = &arm.guard {
                        self.visit_expr(guard);
                    }
                    self.visit_expr(&arm.body);
                    self.pop_scope();
                }
            }
            HirExpressionKind::Itera(_, binding, _, iter, block) => {
                self.visit_expr(iter);
                self.ensure_state(*binding);
                self.check_block(block);
            }
            HirExpressionKind::Assign(target, value) => {
                self.check_lvalue(target);
                if target.ty.is_some_and(|ty| self.types.is_copy_view(ty))
                    || Self::root_def_id(target).is_some_and(|id| self.copy_roots.contains(&id))
                {
                    self.bind_copy_from(value);
                } else if target.ty.is_some_and(|ty| self.types.is_own_view(ty))
                    || Self::root_def_id(target).is_some_and(|id| self.own_roots.contains(&id))
                {
                    self.bind_own_from(value);
                } else if self.language_conflicts
                    && (target.ty.is_some_and(|ty| self.types.is_mut_view(ty))
                        || Self::root_def_id(target).is_some_and(|id| self.mut_roots.contains(&id)))
                {
                    self.bind_mut_from(value);
                } else {
                    self.visit_expr(value);
                }
            }
            // `↤` writes the target place (declared ownership/access rules
            // apply) before visiting the conversion source and its `⇥`
            // recovery — no ordinary-assignment bypass.
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
            } => {
                self.check_lvalue(target);
                self.visit_expr(source);
                if let Some(recovery) = recovery {
                    self.visit_expr(recovery);
                }
            }
            HirExpressionKind::Clausura(params, _, _, body, _) => {
                self.push_scope();
                for param in params {
                    self.ensure_state(param.def_id);
                }
                self.visit_expr(body);
                self.pop_scope();
            }
            HirExpressionKind::Ref(kind, inner) => match Self::root_def_id(inner) {
                Some(def_id) if self.tracks_explicit_ref(def_id) => {
                    self.check_projection_inputs(inner);
                    match kind {
                        radix_hir::HirRefKind::Shared => self.borrow_shared(def_id, expr.span),
                        radix_hir::HirRefKind::Mutable => self.borrow_mut(def_id, expr.span),
                    }
                }
                _ => self.visit_expr(inner),
            },
            HirExpressionKind::MethodCall(receiver, _method, _type_args, args) => {
                // Determine receiver semantics from the method call's return type.
                // Methods returning Vacuum mutate the receiver in place; treat as
                // a receiver write. The call may conflict with an existing borrow,
                // but it does not keep a mutable borrow alive after the statement.
                let is_mutating = expr.ty.is_some_and(|ty| {
                    matches!(self.types.get(ty), Type::Primitive(Primitive::Vacuum))
                });
                if is_mutating {
                    if let Some(def_id) = Self::root_def_id(receiver) {
                        self.check_projection_inputs(receiver);
                        self.write_use(def_id, expr.span);
                    } else {
                        self.visit_expr(receiver);
                    }
                } else {
                    self.visit_expr(receiver);
                }
                for arg in args {
                    self.check_expr(&arg.expr);
                }
            }
            _ => walk_expr(self, expr),
        }
    }
}
