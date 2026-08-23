//! SFR-3 (summa-filum-reduction): `summa ex` admission laws.
//!
//! Replaces the SFR-2 fail-closed gate (`summa_expression_unsupported`) with
//! the real admission rules: coordinate count = iterable rank (the shared apud
//! rule from `expr`), term type = numeric scalar (`fractus`/`numerus`),
//! exactly-one-`redde` pure body (the fragment-compatible effect subset),
//! `filum` only inside `@ nucleum` kernels, and the v1 tail gate
//! (`K % W ≠ 0` fails closed at admission).
//!
//! The expression type is the term type: the sum-only combine reduces terms
//! of the iterable's element type to a result of that same type. The term body
//! is checked under a term return contract so every `redde` in the body is
//! unified against the term type rather than the enclosing function's return.

use super::expr::{apud_arity, ApudArity, ISSUE_ITERA_APUD_ARITY_MISMATCH};
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use crate::passes::fragment_definition::{
    is_allowed_device_method, is_device_value_type, struct_def_from_type,
};
use radix_hir::HirCallArg;
use radix_lexer::Span;

const ISSUE_SUMMA_REQUIRES_SHAPED_ITERABLE: &str = "summa_requires_shaped_iterable";
const ISSUE_SUMMA_TERM_TYPE_NON_SCALAR: &str = "summa_term_type_non_scalar";
const ISSUE_SUMMA_FILUM_REQUIRES_NUCLEUM_KERNEL: &str = "summa_filum_requires_nucleum_kernel";
const ISSUE_SUMMA_TAIL_NOT_ALIGNED: &str = "summa_tail_not_aligned";
const ISSUE_SUMMA_BODY_REQUIRES_SINGLE_REDDE: &str = "summa_body_requires_single_redde";
const ISSUE_SUMMA_BODY_FORBIDS_RUMPE: &str = "summa_body_forbids_rumpe";
const ISSUE_SUMMA_BODY_FORBIDS_CEDE: &str = "summa_body_forbids_cede";
const ISSUE_SUMMA_BODY_FORBIDS_CAPE: &str = "summa_body_forbids_cape";
const ISSUE_SUMMA_BODY_FORBIDS_IACE: &str = "summa_body_forbids_iace";
const ISSUE_SUMMA_BODY_EFFECT: &str = "summa_body_effect";
const ISSUE_SUMMA_BODY_CONTEXT: &str = "summa_body_context";

/// v1 cyclic lane width for the `filum` schedule (Metal simdgroup width,
/// CUDA warp size). Tails (`K % W ≠ 0`) fail closed at admission.
const SUMMA_LANE_WIDTH: u64 = 32;

/// Check one `summa ex` expression and produce its term type.
pub(super) fn check_summa_expr(
    checker: &mut TypeChecker<'_>,
    source: &mut HirExpression,
    coordinates: &[(DefId, Symbol)],
    filum: Option<&(DefId, Symbol)>,
    binding: &(DefId, Symbol),
    body: &mut HirBlock,
    span: Span,
) -> TypeId {
    let source_ty = checker.check_expr(source);
    let dispatch_ty = checker
        .types
        .peel_access_marker(checker.resolve_inferred_type(source_ty))
        .1;

    // v1 iterable surface (settled contract): tensor and matrix only; lista
    // and bounded buffers stay with the `.summa()` method / `reducta`.
    let elem_ty = match checker.types.get(dispatch_ty) {
        Type::Tensor(elem, _) | Type::Matrix(elem, _) => checker.resolve_inferred_type(*elem),
        _ => {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SUMMA_REQUIRES_SHAPED_ITERABLE,
                span,
            );
            check_summa_body(
                checker,
                coordinates,
                filum,
                binding,
                body,
                checker.error_type,
            );
            return checker.error_type;
        }
    };

    // Shared apud arity rule: coordinate count must equal the iterable rank.
    match apud_arity(
        checker.types,
        dispatch_ty,
        !coordinates.is_empty(),
        coordinates.len(),
    ) {
        ApudArity::Admitted => {}
        ApudArity::RequiresShaped => checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            super::expr::ISSUE_ITERA_APUD_REQUIRES_SHAPED_ITERABLE,
            span,
        ),
        ApudArity::Mismatch { expected, actual } => checker.errors.push(
            SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                .with_arg("issue", ISSUE_ITERA_APUD_ARITY_MISMATCH)
                .with_arg("expected", expected.to_string())
                .with_arg("actual", actual.to_string()),
        ),
    }

    // Term-type law: the term is the iterable element and must be a numeric
    // scalar (`fractus`/`numerus` family), matching the sum-only combine.
    if !is_numeric_scalar(checker.types, elem_ty) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_SUMMA_TERM_TYPE_NON_SCALAR,
            source.span,
        );
    }

    // Placement law: the distributed `filum` form admits only inside an
    // `@ nucleum` device-entry kernel. The sequential form is an ordinary
    // host/device expression.
    if filum.is_some() && !checker.current_nucleum_entry {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_SUMMA_FILUM_REQUIRES_NUCLEUM_KERNEL,
            span,
        );
    }

    // Tail gate: the cyclic schedule is `k ≡ f (mod W)` with W = 32; a
    // statically known element count K with `K % W ≠ 0` fails closed at
    // admission. Only the distributed form has lanes.
    if filum.is_some() {
        if let Some(dims) = summa_source_extent(checker, dispatch_ty) {
            let k: u64 = dims
                .iter()
                .map(|d| u64::try_from(*d).unwrap_or(u64::MAX))
                .product();
            if k % SUMMA_LANE_WIDTH != 0 {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_SUMMA_TAIL_NOT_ALIGNED,
                    span,
                );
            }
        }
    }

    check_summa_body(checker, coordinates, filum, binding, body, elem_ty);
    elem_ty
}

/// Literal element-count `K` of a shaped summa source, when every dimension
/// is a compile-time natural literal. Symbolic shapes admit (no proven tail).
fn summa_source_extent(checker: &TypeChecker<'_>, dispatch_ty: TypeId) -> Option<Vec<i64>> {
    let shape = match checker.types.get(dispatch_ty) {
        Type::Tensor(_, shape) | Type::Matrix(_, shape) => *shape,
        _ => return None,
    };
    crate::i64s_from_literal_index(shape, checker.types)
}

/// Term-type law helper: `fractus`/`numerus` family scalars only.
fn is_numeric_scalar(types: &TypeTable, ty: TypeId) -> bool {
    matches!(
        types.get(ty),
        Type::Primitive(Primitive::Numerus | Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
    )
}

/// Bind the term body scope and check it under the term return contract.
///
/// The body is typed first (so expression types are available to the body-law
/// walker's device-receiver classification), then the structural body laws
/// run on the typed tree.
fn check_summa_body(
    checker: &mut TypeChecker<'_>,
    coordinates: &[(DefId, Symbol)],
    filum: Option<&(DefId, Symbol)>,
    binding: &(DefId, Symbol),
    body: &mut HirBlock,
    term_ty: TypeId,
) {
    checker.push_scope();
    let numerus = checker.numerus_type();
    for (def_id, _) in coordinates {
        checker.insert_binding(*def_id, numerus);
    }
    if let Some((def_id, _)) = filum {
        checker.insert_binding(*def_id, numerus);
    }
    checker.insert_binding(binding.0, term_ty);

    let prev_return = checker.current_return;
    let prev_allow_inferred = checker.allow_inferred_return;
    checker.current_return = Some(term_ty);
    checker.allow_inferred_return = false;
    checker.check_block(body, None);
    checker.current_return = prev_return;
    checker.allow_inferred_return = prev_allow_inferred;

    check_summa_body_laws(checker, coordinates, filum, binding, body);
    checker.pop_scope();
}

/// One structural pass over the term body enforcing the SFR-3 body laws:
/// exactly one valued `redde` and the fragment-compatible pure device-safe
/// subset (fragment calls and device intrinsics allowed; no
/// `rumpe`/`cede`/`cape`/`iace`; no host allocation, I/O, alternate exit,
/// dynamic function, or address taking).
struct SummaBodyState {
    /// Bindings the body owns: coordinates, term binder, filum binder, and
    /// body-declared locals. Writes to any other binding are captured-state
    /// mutation (impure) — `captured_mut_host` in the fragment vocabulary.
    own_ids: FxHashSet<DefId>,
    /// Total `redde` statements in the body tree.
    redde_count: usize,
    /// `redde` statements carrying a value.
    valued_redde: usize,
}

fn check_summa_body_laws(
    checker: &mut TypeChecker<'_>,
    coordinates: &[(DefId, Symbol)],
    filum: Option<&(DefId, Symbol)>,
    binding: &(DefId, Symbol),
    body: &HirBlock,
) {
    let mut state = SummaBodyState {
        own_ids: FxHashSet::from_iter(
            coordinates
                .iter()
                .map(|(def_id, _)| *def_id)
                .chain(filum.map(|(def_id, _)| *def_id))
                .chain(std::iter::once(binding.0)),
        ),
        redde_count: 0,
        valued_redde: 0,
    };
    state.walk_block(checker, body);
    if state.redde_count != 1 || state.valued_redde != 1 {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_SUMMA_BODY_REQUIRES_SINGLE_REDDE,
            body.span,
        );
    }
}

impl SummaBodyState {
    fn walk_block(&mut self, checker: &mut TypeChecker<'_>, block: &HirBlock) {
        for stmt in &block.statements {
            self.walk_stmt(checker, stmt);
        }
        if let Some(expr) = &block.expr {
            self.walk_expr(checker, expr);
        }
    }

    fn walk_stmt(&mut self, checker: &mut TypeChecker<'_>, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Local(local) => {
                self.own_ids.insert(local.def_id);
                if let Some(init) = &local.init {
                    self.walk_expr(checker, init);
                }
            }
            HirStatementKind::Redde(value) => {
                self.redde_count += 1;
                if let Some(value) = value {
                    self.valued_redde += 1;
                    self.walk_expr(checker, value);
                }
            }
            HirStatementKind::Expr(expr) => self.walk_expr(checker, expr),
            HirStatementKind::Rumpe => checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SUMMA_BODY_FORBIDS_RUMPE,
                stmt.span,
            ),
            HirStatementKind::Perge | HirStatementKind::Tacet => {}
            HirStatementKind::IncDec(inc_dec) => self.walk_place(checker, &inc_dec.target),
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    self.walk_expr(checker, &clause.cond);
                    self.walk_block(checker, &clause.body);
                }
            }
        }
    }

    fn walk_expr(&mut self, checker: &mut TypeChecker<'_>, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(_)
            | HirExpressionKind::Literal(_)
            | HirExpressionKind::Vacua
            | HirExpressionKind::Error => {}
            HirExpressionKind::Binary(_, lhs, rhs) => {
                self.walk_expr(checker, lhs);
                self.walk_expr(checker, rhs);
            }
            HirExpressionKind::Unary(_, operand) => self.walk_expr(checker, operand),
            HirExpressionKind::Call(callee, _, args) => {
                self.walk_call(checker, callee, args, expr.span)
            }
            // EOT-3M (exhaustive-match completion): a transfer invokes its
            // sink; keep the same unapproved-effect rejection as `∇`.
            HirExpressionKind::Transfer { sink, payloads } => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, sink);
                for payload in payloads {
                    self.walk_expr(checker, payload);
                }
            }
            HirExpressionKind::Gradient {
                call,
                selected_arguments,
            } => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, call);
                for selector in selected_arguments {
                    self.walk_expr(checker, selector);
                }
            }
            HirExpressionKind::MethodCall(receiver, method, _, args) => {
                self.walk_method_call(checker, receiver, *method, args, expr.span);
            }
            HirExpressionKind::Field(object, _) => self.walk_expr(checker, object),
            HirExpressionKind::Index(object, index) => {
                self.walk_expr(checker, object);
                self.walk_expr(checker, index);
            }
            HirExpressionKind::OptionalChain(object, _) | HirExpressionKind::NonNull(object, _) => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, object);
            }
            HirExpressionKind::Block(block) => self.walk_block(checker, block),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                self.walk_expr(checker, cond);
                if let Some(catch) = then_catch {
                    self.walk_cape(checker, catch);
                }
                self.walk_block(checker, then_block);
                if let Some(else_block) = else_block {
                    self.walk_block(checker, else_block);
                }
            }
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for scrutinee in scrutinees {
                    self.walk_expr(checker, scrutinee);
                }
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.walk_expr(checker, guard);
                    }
                    self.walk_expr(checker, &arm.body);
                }
            }
            HirExpressionKind::Loop(block) => self.walk_block(checker, block),
            HirExpressionKind::Dum(cond, block) => {
                self.walk_expr(checker, cond);
                self.walk_block(checker, block);
            }
            HirExpressionKind::Itera(_, _, _, _, source, block) => {
                self.walk_expr(checker, source);
                self.walk_block(checker, block);
            }
            // Nested summa nodes are admitted/declined by their own admission
            // check under the ordinary expression path; the fragment subset
            // treats the nested node as an unapproved effect, so the outer
            // body walk does not descend into it.
            HirExpressionKind::Summa { .. } => effect(checker, expr.span, "unapproved_effect"),
            // Same fragment-subset posture for the dedicated first-match
            // head: unadmitted until AFM-3/AFM-4, so never descended into.
            HirExpressionKind::FirstMatch { .. } => {
                effect(checker, expr.span, "unapproved_effect")
            }
            HirExpressionKind::Intervallum {
                start, end, step, ..
            } => {
                self.walk_expr(checker, start);
                self.walk_expr(checker, end);
                if let Some(step) = step {
                    self.walk_expr(checker, step);
                }
            }
            HirExpressionKind::Assign(target, value) => {
                self.walk_place(checker, target);
                self.walk_expr(checker, value);
            }
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
            } => {
                if recovery.is_some() {
                    effect(checker, expr.span, "alternate_exit");
                }
                self.walk_place(checker, target);
                self.walk_expr(checker, source);
                if let Some(recovery) = recovery {
                    self.walk_expr(checker, recovery);
                }
            }
            HirExpressionKind::Array(elements) => {
                effect(checker, expr.span, "host_allocation");
                for element in elements {
                    match element {
                        HirArrayElement::Expr(inner) | HirArrayElement::Spread(inner) => {
                            self.walk_expr(checker, inner);
                        }
                    }
                }
            }
            HirExpressionKind::Struct(_, fields) => {
                for (_, value) in fields {
                    self.walk_expr(checker, value);
                }
            }
            HirExpressionKind::Tuple(elements, _) => {
                for element in elements {
                    self.walk_expr(checker, element);
                }
            }
            HirExpressionKind::Scribe(_, elements) | HirExpressionKind::Scriptum(_, elements) => {
                effect(checker, expr.span, "host_io");
                for element in elements {
                    self.walk_expr(checker, element);
                }
            }
            HirExpressionKind::ReadLine(_) => effect(checker, expr.span, "host_io"),
            HirExpressionKind::Praefixum(inner) => self.walk_expr(checker, inner),
            HirExpressionKind::Adfirma(cond, message) => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, cond);
                if let Some(message) = message {
                    self.walk_expr(checker, message);
                }
            }
            HirExpressionKind::Panic(message) => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, message);
            }
            HirExpressionKind::Throw(value) => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_SUMMA_BODY_FORBIDS_IACE,
                    expr.span,
                );
                self.walk_expr(checker, value);
            }
            HirExpressionKind::Handled { body, catch } => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_SUMMA_BODY_FORBIDS_CAPE,
                    expr.span,
                );
                self.walk_block(checker, body);
                self.walk_cape(checker, catch);
            }
            HirExpressionKind::Clausura(params, _, _, body, _) => {
                effect(checker, expr.span, "dynamic_function");
                for param in params {
                    if let Some(default) = &param.default {
                        self.walk_expr(checker, default);
                    }
                }
                self.walk_expr(checker, body);
            }
            HirExpressionKind::Cede(value) | HirExpressionKind::Yield(value) => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_SUMMA_BODY_FORBIDS_CEDE,
                    expr.span,
                );
                self.walk_expr(checker, value);
            }
            HirExpressionKind::Reddet(value) | HirExpressionKind::Tacebit(value) => {
                effect(checker, expr.span, "async_posture");
                self.walk_expr(checker, value);
            }
            HirExpressionKind::Verte {
                source, entries, ..
            } => {
                self.walk_expr(checker, source);
                if let Some(entries) = entries {
                    for field in entries {
                        self.walk_object_field(checker, field);
                    }
                }
            }
            HirExpressionKind::Conversio {
                source,
                target,
                recovery,
                ..
            } => {
                if recovery.is_some() {
                    effect(checker, expr.span, "alternate_exit");
                }
                self.walk_expr(checker, source);
                if let HirConversioTarget::Intervallum(operand) = target {
                    self.walk_expr(checker, operand);
                }
                if let Some(recovery) = recovery {
                    self.walk_expr(checker, recovery);
                }
            }
            HirExpressionKind::Ad { opener, .. } => {
                effect(checker, expr.span, "host_runtime");
                if let Some(opener) = opener {
                    self.walk_expr(checker, opener);
                }
            }
            HirExpressionKind::Ref(_, operand) | HirExpressionKind::Deref(operand) => {
                effect(checker, expr.span, "address_taking");
                self.walk_expr(checker, operand);
            }
            HirExpressionKind::TypeCheck { expr: inner, .. } => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, inner);
            }
        }
    }

    fn walk_place(&mut self, checker: &mut TypeChecker<'_>, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                if !self.own_ids.contains(def_id) {
                    effect(checker, expr.span, "captured_mut_host");
                }
            }
            HirExpressionKind::Field(object, _) => self.walk_place(checker, object),
            HirExpressionKind::Index(object, index) => {
                self.walk_place(checker, object);
                self.walk_expr(checker, index);
            }
            _ => self.walk_expr(checker, expr),
        }
    }

    fn walk_cape(&mut self, checker: &mut TypeChecker<'_>, cape: &HirCape) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_SUMMA_BODY_FORBIDS_CAPE,
            cape.span,
        );
        self.walk_block(checker, &cape.body);
    }

    fn walk_object_field(&mut self, checker: &mut TypeChecker<'_>, field: &HirObjectField) {
        match &field.key {
            HirObjectKey::Ident(_) | HirObjectKey::String(_) => {}
            HirObjectKey::Computed(expr) | HirObjectKey::Spread(expr) => {
                self.walk_expr(checker, expr);
            }
        }
        if let Some(value) = &field.value {
            self.walk_expr(checker, value);
        }
    }

    fn walk_call(
        &mut self,
        checker: &mut TypeChecker<'_>,
        callee: &HirExpression,
        args: &[HirCallArg],
        span: Span,
    ) {
        match &callee.kind {
            HirExpressionKind::Path(def_id) => self.classify_callee(checker, *def_id, span),
            other => {
                if !matches!(other, HirExpressionKind::Error) {
                    effect(checker, span, "dynamic_function");
                }
                self.walk_expr(checker, callee);
            }
        }
        for arg in args {
            self.walk_expr(checker, &arg.expr);
        }
    }

    fn walk_method_call(
        &mut self,
        checker: &mut TypeChecker<'_>,
        receiver: &HirExpression,
        method: Symbol,
        args: &[HirCallArg],
        span: Span,
    ) {
        if let Some(role) = resolve_method_role(checker, receiver, method) {
            match role {
                NucleumRole::Fragment => {}
                NucleumRole::Entry => context(checker, span, "entry_callee"),
            }
        } else if is_allowed_device_method(checker.interner.map_or("", |i| i.resolve(method)))
            && is_device_value_type(receiver.ty.unwrap_or(checker.error_type), checker.types)
        {
            // Approved device intrinsic method — admitted (fragment-compatible).
        } else {
            context(checker, span, "host_callable");
        }
        self.walk_expr(checker, receiver);
        for arg in args {
            self.walk_expr(checker, &arg.expr);
        }
    }

    /// Callee classification shared with the fragment definition rules:
    /// device intrinsics and `@ nucleum fragment` calls are admitted; entry
    /// kernels, graphics stages, and ordinary host callables are not.
    fn classify_callee(&mut self, checker: &mut TypeChecker<'_>, def_id: DefId, span: Span) {
        if checker.resolver.gpu_builtin(def_id).is_some() {
            return;
        }
        match checker.nucleum_roles.get(&def_id).copied() {
            Some(NucleumRole::Fragment) => {}
            Some(NucleumRole::Entry) => context(checker, span, "entry_callee"),
            None => context(checker, span, "host_callable"),
        }
    }
}

/// Fragment-compatible effect rejection (`summa_body_effect`).
fn effect(checker: &mut TypeChecker<'_>, span: Span, reason: &'static str) {
    checker.errors.push(
        SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
            .with_arg("issue", ISSUE_SUMMA_BODY_EFFECT)
            .with_arg("reason", reason),
    );
}

/// Fragment-compatible call-context rejection (`summa_body_context`).
fn context(checker: &mut TypeChecker<'_>, span: Span, reason: &'static str) {
    checker.errors.push(
        SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
            .with_arg("issue", ISSUE_SUMMA_BODY_CONTEXT)
            .with_arg("reason", reason),
    );
}

/// Resolve a method call's nucleum role via the method role table. Methods
/// without a nucleum role fall through to the device-intrinsic and
/// host-callable checks in [`SummaBodyState::walk_method_call`].
fn resolve_method_role(
    checker: &TypeChecker<'_>,
    receiver: &HirExpression,
    method: Symbol,
) -> Option<NucleumRole> {
    let receiver_ty = receiver.ty?;
    let struct_def = struct_def_from_type(receiver_ty, checker.types)?;
    checker
        .method_nucleum_roles
        .get(&(struct_def, method))
        .copied()
}
