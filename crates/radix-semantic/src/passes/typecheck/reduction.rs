//! Admission and pure term laws for the closed reduction family.
//! Arithmetic retains its exact intermediate; only a consuming store narrows.
use super::expr::{ApudArity, ISSUE_ITERA_APUD_ARITY_MISMATCH, apud_arity};
#[allow(clippy::wildcard_imports)]
use super::*;
use crate::passes::fragment_definition::{
    is_allowed_device_method, is_device_value_type, struct_def_from_type,
};
use radix_hir::{HirCallArg, HirReductionOp};
use radix_lexer::Span;

const ISSUE_REDUCTA_REQUIRES_SHAPED_ITERABLE: &str = "reducta_requires_shaped_iterable";
const ISSUE_REDUCTA_TERM_TYPE_NON_SCALAR: &str = "reducta_term_type_non_scalar";
const ISSUE_REDUCTA_FILUM_REQUIRES_NUCLEUM_KERNEL: &str = "reducta_filum_requires_nucleum_kernel";
const ISSUE_REDUCTA_TAIL_NOT_ALIGNED: &str = "reducta_tail_not_aligned";
const ISSUE_REDUCTA_BODY_REQUIRES_SINGLE_REDDE: &str = "reducta_body_requires_single_redde";
const ISSUE_REDUCTA_BODY_FORBIDS_RUMPE: &str = "reducta_body_forbids_rumpe";
const ISSUE_REDUCTA_BODY_FORBIDS_CEDE: &str = "reducta_body_forbids_cede";
const ISSUE_REDUCTA_BODY_FORBIDS_CAPE: &str = "reducta_body_forbids_cape";
const ISSUE_REDUCTA_BODY_FORBIDS_IACE: &str = "reducta_body_forbids_iace";
const ISSUE_REDUCTA_BODY_EFFECT: &str = "reducta_body_effect";
const ISSUE_REDUCTA_BODY_CONTEXT: &str = "reducta_body_context";

/// Type one reduction from its source, body term, and expected bucket extent.
pub(super) fn check_reduction_expr(
    checker: &mut TypeChecker<'_>,
    op: HirReductionOp,
    source: &mut HirExpression,
    coordinates: &[(DefId, Symbol)],
    group: Option<&mut Box<HirExpression>>,
    filum: Option<(DefId, Symbol)>,
    binding: Option<(DefId, Symbol)>,
    body: Option<&mut HirBlock>,
    identity: Option<&mut Box<HirExpression>>,
    expected: Option<TypeId>,
    span: Span,
) -> TypeId {
    let source_ty = checker.check_expr(source);
    let dispatch = checker
        .types
        .peel_access_marker(checker.resolve_inferred_type(source_ty))
        .1;
    let (element, shape) = match checker.types.get(dispatch) {
        Type::Tensor(element, shape) | Type::Matrix(element, shape) => (*element, *shape),
        _ => {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_REDUCTA_REQUIRES_SHAPED_ITERABLE,
                span,
            );
            return checker.error_type;
        }
    };
    let element = checker.resolve_inferred_type(element);
    let dimensions = crate::i64s_from_literal_index(shape, checker.types);
    let rank = super::expr::tensor_shape_rank(checker.types, shape)
        .map_or(coordinates.len().max(1), |n| n as usize);
    match apud_arity(
        checker.types,
        dispatch,
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
    if filum.is_some() {
        if !checker.current_nucleum_entry {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_REDUCTA_FILUM_REQUIRES_NUCLEUM_KERNEL,
                span,
            );
        }
        if body.is_none() {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_filum_requires_body",
                span,
            );
        }
        if let Some(dimensions) = &dimensions {
            let extent = dimensions
                .iter()
                .try_fold(1u64, |n, d| n.checked_mul(u64::try_from(*d).ok()?));
            if extent.is_some_and(|k| !k.is_multiple_of(32)) {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_TAIL_NOT_ALIGNED,
                    span,
                );
            }
        }
    }
    checker.push_scope();
    let integer = checker.numerus_type();
    for (id, _) in coordinates {
        checker.insert_binding(*id, integer);
    }
    if let Some((id, _)) = filum {
        checker.insert_binding(id, integer);
    }
    let expected_group = expected.and_then(|ty| {
        let ty = checker.resolve_inferred_type(ty);
        match checker.types.get(ty) {
            Type::Tensor(element, shape) => Some((*element, *shape)),
            _ => None,
        }
    });
    let grouped = group.is_some();
    if let Some(group) = group {
        if coordinates.is_empty() {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_group_requires_apud",
                span,
            );
        }
        let key_ty = checker.check_expr(group);
        let key_ty = checker.resolve_inferred_type(key_ty);
        if !matches!(
            checker.types.get(key_ty),
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _)
        ) {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_group_key_not_integer",
                group.span,
            );
        }
        if expected_group.is_none_or(|(_, shape)| {
            crate::i64s_from_literal_index(shape, checker.types)
                .is_none_or(|d| d.len() != 1 || d[0] < 0)
        }) {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_group_extent_unknown",
                span,
            );
        }
        if matches!(op, HirReductionOp::ArgMax | HirReductionOp::ArgMin)
            || (matches!(op, HirReductionOp::Max | HirReductionOp::Min) && identity.is_none())
        {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_group_op_unsupported",
                span,
            );
        }
    }
    let term_ty = if let (Some(binding), Some(body)) = (binding, body) {
        checker.insert_binding(binding.0, element);
        let prev_return = checker.current_return.take();
        let prev_inferred = checker.inferred_return.take();
        let prev_allow = checker.allow_inferred_return;
        checker.allow_inferred_return = true;
        checker.check_block(body, None);
        let term = checker.inferred_return.unwrap_or(checker.error_type);
        checker.current_return = prev_return;
        checker.inferred_return = prev_inferred;
        checker.allow_inferred_return = prev_allow;
        check_reducta_body_laws(checker, coordinates, filum, binding, body);
        checker.resolve_inferred_type(term)
    } else {
        element
    };
    checker.pop_scope();
    let boolean = checker.types.primitive(Primitive::Bivalens);
    let bool_op = matches!(
        op,
        HirReductionOp::All | HirReductionOp::Any | HirReductionOp::Count
    );
    if bool_op {
        if term_ty != boolean {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_term_type_not_bool",
                span,
            );
        }
    } else if !is_numeric_scalar(checker.types, term_ty) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_REDUCTA_TERM_TYPE_NON_SCALAR,
            span,
        );
    }
    let result = match op {
        HirReductionOp::Count => integer,
        HirReductionOp::All | HirReductionOp::Any => boolean,
        HirReductionOp::ArgMax | HirReductionOp::ArgMin if rank > 1 => {
            checker.types.tuple(vec![integer; rank])
        }
        HirReductionOp::ArgMax | HirReductionOp::ArgMin => integer,
        HirReductionOp::Sum => checker.summa_result_type(term_ty),
        _ => term_ty,
    };
    let extremum = matches!(
        op,
        HirReductionOp::Max | HirReductionOp::Min | HirReductionOp::ArgMax | HirReductionOp::ArgMin
    );
    if let Some(identity) = identity {
        if !extremum {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_identity_not_applicable",
                identity.span,
            );
        }
        let identity_ty = checker.check_expr_with_expected(identity, Some(result));
        checker.unify_assign_issue(
            identity_ty,
            result,
            identity.span,
            "reducta_identity_type_mismatch",
        );
    } else if extremum && !grouped && dimensions.is_none_or(|d| d.is_empty() || d.contains(&0)) {
        let nullable = checker.types.option(result);
        if let Some(expected) = expected {
            let expected = checker.resolve_inferred_type(expected);
            if !matches!(
                checker.types.get(expected),
                Type::Option(_) | Type::Union(_)
            ) {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    "reducta_nullable_result_requires_coalesce",
                    span,
                );
            }
        }
        return nullable;
    }
    if grouped {
        if let Some((_, shape)) = expected_group {
            return checker.types.tensor_with_shape(result, shape);
        }
        return checker.error_type;
    }
    result
}

fn is_numeric_scalar(types: &TypeTable, ty: TypeId) -> bool {
    matches!(
        types.get(ty),
        Type::Primitive(Primitive::Numerus | Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
    )
}

struct ReductionBodyState {
    /// Bindings the body owns: coordinates, term binder, filum binder, and
    /// body-declared locals. Writes to any other binding are captured-state
    /// mutation (impure) — `captured_mut_host` in the fragment vocabulary.
    own_ids: FxHashSet<DefId>,
    /// Total `redde` statements in the body tree.
    redde_count: usize,
    /// `redde` statements carrying a value.
    valued_redde: usize,
}

fn check_reducta_body_laws(
    checker: &mut TypeChecker<'_>,
    coordinates: &[(DefId, Symbol)],
    filum: Option<(DefId, Symbol)>,
    binding: (DefId, Symbol),
    body: &HirBlock,
) {
    let mut state = ReductionBodyState {
        own_ids: coordinates
            .iter()
            .map(|(def_id, _)| *def_id)
            .chain(filum.map(|(def_id, _)| def_id))
            .chain(std::iter::once(binding.0))
            .collect(),
        redde_count: 0,
        valued_redde: 0,
    };
    state.walk_block(checker, body);
    if state.redde_count != 1 || state.valued_redde != 1 {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_REDUCTA_BODY_REQUIRES_SINGLE_REDDE,
            body.span,
        );
    }
}

impl ReductionBodyState {
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
                ISSUE_REDUCTA_BODY_FORBIDS_RUMPE,
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
            | HirExpressionKind::Insere(_)
            | HirExpressionKind::Vacua
            | HirExpressionKind::Error => {}
            HirExpressionKind::Widen { operand, .. } => self.walk_expr(checker, operand),
            HirExpressionKind::Binary(_, lhs, rhs) => {
                self.walk_expr(checker, lhs);
                self.walk_expr(checker, rhs);
            }
            HirExpressionKind::Unary(_, operand) => self.walk_expr(checker, operand),
            HirExpressionKind::Call(callee, _, args, _) => {
                self.walk_call(checker, callee, args, expr.span)
            }
            // `target ⇇ value` writes existing tensor storage, an effect a term
            // body does not admit; keep the same rejection as `∇`.
            HirExpressionKind::CopyInto { target, value } => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, target);
                self.walk_expr(checker, value);
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
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
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
            HirExpressionKind::Marked { expr: inner, .. } => self.walk_expr(checker, inner),
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
                    self.walk_expr(checker, &arm.body);
                }
            }
            HirExpressionKind::Loop(block) => self.walk_block(checker, block),
            HirExpressionKind::Dum(cond, block) => {
                self.walk_expr(checker, cond);
                self.walk_block(checker, block);
            }
            HirExpressionKind::Itera {
                iter: source,
                body: block,
                ..
            } => {
                self.walk_expr(checker, source);
                self.walk_block(checker, block);
            }
            HirExpressionKind::IteraLockstep { sources, body, .. } => {
                for source in sources {
                    self.walk_expr(checker, source);
                }
                self.walk_block(checker, body);
            }
            HirExpressionKind::IteraRange { ranges, body, .. } => {
                for range in ranges {
                    self.walk_expr(checker, range);
                }
                self.walk_block(checker, body);
            }
            // Nested reductions are admitted/declined by their own admission
            // check under the ordinary expression path; the fragment subset
            // treats the nested node as an unapproved effect, so the outer
            // body walk does not descend into it.
            HirExpressionKind::Reduction { .. } => effect(checker, expr.span, "unapproved_effect"),
            HirExpressionKind::Sort { .. } => effect(checker, expr.span, "unapproved_effect"),
            // Same fragment-subset posture for the dedicated first-match
            // head: unadmitted until AFM-3/AFM-4, so never descended into.
            HirExpressionKind::FirstMatch { .. } => effect(checker, expr.span, "unapproved_effect"),
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
                        HirArrayElement::Infer(_) => {}
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
            // `¶` builds text, like `scriptum`.
            HirExpressionKind::Format { value, .. } => {
                effect(checker, expr.span, "host_io");
                self.walk_expr(checker, value);
            }
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
                    ISSUE_REDUCTA_BODY_FORBIDS_IACE,
                    expr.span,
                );
                self.walk_expr(checker, value);
            }
            HirExpressionKind::Handled { body, catch } => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_BODY_FORBIDS_CAPE,
                    expr.span,
                );
                self.walk_block(checker, body);
                self.walk_cape(checker, catch);
            }
            // UT: a trap inside a `summa` term body is an alternate-exit
            // dataflow the fold cannot host — same rejection class as cape.
            HirExpressionKind::Trap(body) => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_BODY_FORBIDS_CAPE,
                    expr.span,
                );
                self.walk_block(checker, body);
            }
            HirExpressionKind::CallDefault { body, default, .. } => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_BODY_FORBIDS_CAPE,
                    expr.span,
                );
                self.walk_block(checker, body);
                self.walk_expr(checker, default);
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
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
                    ISSUE_REDUCTA_BODY_FORBIDS_CEDE,
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
            ISSUE_REDUCTA_BODY_FORBIDS_CAPE,
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
                NucleumRole::Entry if is_inlinable_method(checker, receiver, method) => {}
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
    /// device intrinsics, `@ nucleum fragment` calls and inlinable kernels
    /// (public or private) are admitted; async or fallible kernels, graphics
    /// stages, and ordinary host callables are not.
    fn classify_callee(&mut self, checker: &mut TypeChecker<'_>, def_id: DefId, span: Span) {
        if checker.resolver.gpu_builtin(def_id).is_some() {
            return;
        }
        match checker.nucleum_roles.get(&def_id).copied() {
            Some(NucleumRole::Fragment) => {}
            // Rule 12.7 inlines a kernel (public or private) into its caller,
            // so it is a term function here; its body is proved write-free
            // after typecheck (`filum_independence`).
            Some(NucleumRole::Entry) if checker.inlinable_kernels.contains(&def_id) => {}
            Some(NucleumRole::Entry) => context(checker, span, "entry_callee"),
            None => context(checker, span, "host_callable"),
        }
    }
}

/// Fragment-compatible effect rejection (`reducta_body_effect`).
fn effect(checker: &mut TypeChecker<'_>, span: Span, reason: &'static str) {
    checker.errors.push(
        SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
            .with_arg("issue", ISSUE_REDUCTA_BODY_EFFECT)
            .with_arg("reason", reason),
    );
}

/// Fragment-compatible call-context rejection (`reducta_body_context`).
fn context(checker: &mut TypeChecker<'_>, span: Span, reason: &'static str) {
    checker.errors.push(
        SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
            .with_arg("issue", ISSUE_REDUCTA_BODY_CONTEXT)
            .with_arg("reason", reason),
    );
}

/// Resolve a method call's nucleum role via the method role table. Methods
/// without a nucleum role fall through to the device-intrinsic and
/// host-callable checks in [`ReductionBodyState::walk_method_call`].
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

/// A method kernel the filum pass inlines into its caller (rule 12.7): the
/// receiver type names the class statically, so the method definition is the
/// one the type checker resolves the call to.
fn is_inlinable_method(
    checker: &TypeChecker<'_>,
    receiver: &HirExpression,
    method: Symbol,
) -> bool {
    receiver
        .ty
        .and_then(|ty| struct_def_from_type(ty, checker.types))
        .and_then(|class| checker.method_defs.get(&(class, method)))
        .is_some_and(|def| checker.inlinable_kernels.contains(def))
}
