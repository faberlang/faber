//! Parameter-mode and Rust borrow analysis for typed HIR.
//!
//! Two entry points implement the HIR v1 G2 split:
//!
//! - [`analyze_modes`] — **target-neutral** Faber `de`/`in`/`ex` policy:
//!   use-after-consume, illegal mutation through `de`, and incompatible mode
//!   passing. Runs for every application-lane target.
//! - [`analyze`] — **Rust qualification**: the mode rules plus shared/mutable
//!   borrow conflict approximation used by Rust emit.
//!
//! Precise lifetime reasoning remains Rust's job. Language mode validity is
//! decided before target selection so TS/Go cannot silently accept programs
//! that violate Faber ownership modes.
//!
//! ERROR STRATEGY
//! ==============
//! - Ownership violations are emitted as semantic errors.
//! - Parameter-mode suggestions are emitted as semantic warnings.
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
    FuncSig, Primitive, Resolver, SemanticError, SemanticErrorKind, SemanticParamMode, Type,
    TypeId, TypeTable, WarningKind,
};
use radix_hir::visit::{walk_expr, HirVisitor};
use radix_hir::{
    DefId, HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirFunction, HirItem,
    HirItemKind, HirModule, HirParamMode, HirStatement, HirStatementKind,
};
use rustc_hash::FxHashMap;

const ISSUE_ASSIGN_TO_DE_PARAMETER: &str = "assign_to_de_parameter";
const ISSUE_BORROW_OF_MOVED: &str = "borrow_of_moved";
const ISSUE_CANNOT_MOVE_WHILE_BORROWED: &str = "cannot_move_while_borrowed";
const ISSUE_DE_TO_IN_OR_EX_POSITION: &str = "de_to_in_or_ex_position";
const ISSUE_MUTABLE_BORROW_CONFLICT: &str = "mutable_borrow_conflict";
const ISSUE_SHARED_BORROW_CONFLICT: &str = "shared_borrow_conflict";
const ISSUE_UNUSED_EX_PARAMETER: &str = "unused_ex_parameter";
const ISSUE_UNUSED_IN_PARAMETER: &str = "unused_in_parameter";
const ISSUE_USE_AFTER_MOVE: &str = "use_after_move";
const ISSUE_USE_WHILE_MUTABLY_BORROWED: &str = "use_while_mutably_borrowed";
const ISSUE_WRITE_WHILE_BORROWED: &str = "write_while_borrowed";

/// Target-neutral Faber parameter-mode analysis (`de`/`in`/`ex`).
///
/// Does not apply Rust lifetime/borrow conflict approximation. Use this during
/// base analysis so all HIR targets share mode validity.
///
/// # Errors
///
/// Returns mode violations as a list of [`SemanticError`]s.
pub fn analyze_modes(
    hir: &HirModule,
    _resolver: &Resolver,
    types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    run(hir, types, false)
}

/// Full Rust check: mode rules plus borrow/lifetime conflict approximation.
///
/// Used by legacy `PassConfig::for_target(Rust)` single-shot analysis.
///
/// # Errors
///
/// Returns borrow/lifetime conflicts as a list of [`SemanticError`]s.
pub fn analyze(
    hir: &HirModule,
    _resolver: &Resolver,
    types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    run(hir, types, true)
}

/// Rust-only lifetime/borrow conflicts after modes already ran.
///
/// Filters diagnostics to issues that are not pure language-mode policy so
/// driver base + qualification does not double-report use-after-`ex` / `de`.
///
/// # Errors
///
/// Returns Rust-specific borrow/lifetime conflicts as a list of
/// [`SemanticError`]s.
pub fn analyze_rust_lifetime(
    hir: &HirModule,
    _resolver: &Resolver,
    types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    let mut checker = BorrowChecker::new(types, true);
    checker.check_program(hir);
    checker.errors.retain(|err| {
        err.args.iter().any(|arg| {
            arg.name == "issue"
                && matches!(
                    arg.value.as_str(),
                    ISSUE_BORROW_OF_MOVED
                        | ISSUE_CANNOT_MOVE_WHILE_BORROWED
                        | ISSUE_MUTABLE_BORROW_CONFLICT
                        | ISSUE_SHARED_BORROW_CONFLICT
                        | ISSUE_USE_WHILE_MUTABLY_BORROWED
                        | ISSUE_WRITE_WHILE_BORROWED
                )
        })
    });
    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

fn run(hir: &HirModule, types: &TypeTable, rust_lifetime: bool) -> Result<(), Vec<SemanticError>> {
    let mut checker = BorrowChecker::new(types, rust_lifetime);
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
    /// When true, emit Rust shared/mutable borrow conflict diagnostics.
    rust_lifetime: bool,
    states: FxHashMap<DefId, BorrowState>,
    scopes: Vec<BorrowScope>,
    param_usage: FxHashMap<DefId, ParamUsage>,
    errors: Vec<SemanticError>,
}

/// Usage summary for one function parameter after traversing its body.
///
/// This is policy state rather than ownership state: it decides whether the
/// declared Faber mode is stronger than the function actually needs.
#[derive(Clone, Copy)]
struct ParamUsage {
    mode: HirParamMode,
    span: radix_lexer::Span,
    mutated: bool,
    passed_in_or_ex: bool,
    passed_ex: bool,
    returned: bool,
    moved: bool,
}

impl<'a> BorrowChecker<'a> {
    fn new(types: &'a TypeTable, rust_lifetime: bool) -> Self {
        Self {
            types,
            rust_lifetime,
            states: FxHashMap::default(),
            scopes: Vec::new(),
            param_usage: FxHashMap::default(),
            errors: Vec::new(),
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
                self.check_expr(&const_item.value);
            }
            _ => {}
        }
    }

    fn check_function(&mut self, func: &HirFunction) {
        self.reset();
        for param in &func.params {
            self.ensure_state(param.def_id);
            self.param_usage.insert(
                param.def_id,
                ParamUsage {
                    mode: param.mode,
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
                if let Some(init) = &local.init {
                    self.check_expr(init);
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

    fn check_call_args(&mut self, callee: &HirExpression, args: &[HirCallArg]) {
        let Some(callee_ty) = callee.ty else {
            for arg in args {
                self.check_expr(&arg.expr);
            }
            return;
        };

        let Some(sig) = Self::callee_signature(self.types, callee_ty) else {
            for arg in args {
                self.check_expr(&arg.expr);
            }
            return;
        };

        for (arg, param) in args.iter().zip(sig.params.iter()) {
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
                SemanticParamMode::Move => self.move_from_expr(&arg.expr),
                SemanticParamMode::Owned => self.check_expr(&arg.expr),
            }
        }
    }

    fn check_lvalue(&mut self, target: &HirExpression) {
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

    fn read_use(&mut self, def_id: DefId, span: radix_lexer::Span) {
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            self.error(SemanticErrorKind::UseAfterMove, ISSUE_USE_AFTER_MOVE, span);
            return;
        }
        if self.rust_lifetime && state.mutable {
            self.error(
                SemanticErrorKind::MutableBorrowConflict,
                ISSUE_USE_WHILE_MUTABLY_BORROWED,
                span,
            );
        }
    }

    fn write_use(&mut self, def_id: DefId, span: radix_lexer::Span) {
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
        if self.rust_lifetime && (state.mutable || state.shared > 0) {
            self.error(
                SemanticErrorKind::MutableBorrowConflict,
                ISSUE_WRITE_WHILE_BORROWED,
                span,
            );
        }
    }

    fn move_use(&mut self, def_id: DefId, span: radix_lexer::Span) {
        if let Some(usage) = self.param_usage.get_mut(&def_id) {
            usage.moved = true;
        }
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            self.error(SemanticErrorKind::UseAfterMove, ISSUE_USE_AFTER_MOVE, span);
            return;
        }
        if self.rust_lifetime && (state.mutable || state.shared > 0) {
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
        if !self.rust_lifetime {
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
        if !self.rust_lifetime {
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

    fn emit_mode_lints(&mut self) {
        let mut warnings = Vec::new();
        for usage in self.param_usage.values() {
            match usage.mode {
                HirParamMode::In => {
                    if !usage.mutated && !usage.passed_in_or_ex {
                        warnings.push(
                            SemanticError::structured(
                                SemanticErrorKind::Warning(WarningKind::UnusedMutRefParam),
                                usage.span,
                            )
                            .with_arg("issue", ISSUE_UNUSED_IN_PARAMETER),
                        );
                    }
                }
                HirParamMode::Ex => {
                    if !usage.passed_ex && !usage.returned && !usage.moved {
                        warnings.push(
                            SemanticError::structured(
                                SemanticErrorKind::Warning(WarningKind::UnusedMoveParam),
                                usage.span,
                            )
                            .with_arg("issue", ISSUE_UNUSED_EX_PARAMETER),
                        );
                    }
                }
                HirParamMode::Owned | HirParamMode::De => {}
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
                self.visit_expr(value);
            }
            // `↤` writes the target place (ownership mode / borrow rules for
            // `de`/`in` params apply) before visiting the conversion source
            // and its `⇥` recovery — no ordinary-assignment bypass.
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
            HirExpressionKind::Clausura(params, _, _, body) => {
                self.push_scope();
                for param in params {
                    self.ensure_state(param.def_id);
                }
                self.visit_expr(body);
                self.pop_scope();
            }
            HirExpressionKind::Ref(kind, inner) => match Self::root_def_id(inner) {
                Some(def_id) => {
                    self.check_projection_inputs(inner);
                    match kind {
                        radix_hir::HirRefKind::Shared => self.borrow_shared(def_id, expr.span),
                        radix_hir::HirRefKind::Mutable => self.borrow_mut(def_id, expr.span),
                    }
                }
                None => self.visit_expr(inner),
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
