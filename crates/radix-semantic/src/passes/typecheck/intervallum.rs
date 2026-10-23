//! Type rules for `intervallum<T>` construction and range-membership
//! (`∈` / `∉` and the `intra` keyword) consumption.
//!
//! COMPILER PHASE: Semantic typecheck
//! INPUT: HIR `Intervallum` expressions (start, end, optional step) and range-membership
//! operands with resolved bound and step types
//! OUTPUT: Checked `intervallum<T>` / `intervallum<T, S>` expressions with
//! validated numeric bound types; a stored stepped range carries its literal
//! step in the type (`intervallum<T, S>`), while a run-time step stays
//! inline-only; diagnostics for non-numeric steps, unsupported bound types,
//! range operand mismatches, and `longitudo` bound restrictions

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use crate::{Primitive, Type, is_intervallum_bound_type};
const ISSUE_RANGE_STEP_MUST_BE_NUMERIC: &str = "range_step_must_be_numeric";
const ISSUE_RANGE_STEP_NOT_POSITIVE: &str = "range_step_not_positive";
const ISSUE_INTERVALLUM_VALUE_STEP_UNSUPPORTED: &str = "intervallum_value_step_unsupported";
const ISSUE_INTERVALLUM_BOUND_REQUIRED: &str = "intervallum_bound_required";
const ISSUE_INTERVALLUM_TARGET_NOT_RANGE: &str = "intervallum_target_not_range";
const ISSUE_INTERVALLUM_LONGITUDO_BOUND: &str = "intervallum_longitudo_bound";
const ISSUE_INTRA_REQUIRES_INTERVALLUM_RANGE: &str = "intra_requires_intervallum_range";
const ISSUE_RANGE_OPERAND_TYPE_MISMATCH: &str = "range_operand_type_mismatch";
const ISSUE_RANGE_OPERAND_MUST_BE_NUMERIC: &str = "range_operand_must_be_numeric";

impl TypeChecker<'_> {
    pub fn check_expr_allowing_direct_intervallum_step(
        &mut self,
        expr: &mut HirExpression,
    ) -> TypeId {
        if !matches!(expr.kind, HirExpressionKind::Intervallum { .. }) {
            return self.check_expr(expr);
        }

        let prev = self.allow_intervallum_step;
        self.allow_intervallum_step = true;
        let ty = self.check_expr(expr);
        self.allow_intervallum_step = prev;
        ty
    }

    pub fn check_intervallum_expr(
        &mut self,
        start: &mut HirExpression,
        end: &mut HirExpression,
        step: &mut Option<Box<HirExpression>>,
        span: radix_lexer::Span,
    ) -> TypeId {
        let start_ty = self.check_expr(start);
        let end_ty = self.check_expr(end);
        if let Some(step_expr) = step {
            let step_ty = self.check_expr(step_expr);
            if self.is_infer(self.resolve_inferred_type(step_ty)) {
                let numerus = self.numerus_type();
                self.unify_assign_issue(
                    step_ty,
                    numerus,
                    step_expr.span,
                    ISSUE_RANGE_STEP_MUST_BE_NUMERIC,
                );
            }
            if !self.is_numeric(step_ty) {
                self.invalid_operand_types(ISSUE_RANGE_STEP_MUST_BE_NUMERIC, step_expr.span);
            }
            if constant_step_is_not_positive(step_expr) {
                self.error_issue(
                    SemanticErrorKind::OperandTypesInvalid,
                    ISSUE_RANGE_STEP_NOT_POSITIVE,
                    step_expr.span,
                );
            }
        }

        self.require_intervallum_bound(start_ty, start.span);
        self.require_intervallum_bound(end_ty, end.span);
        let joined = self.join_intervallum_bounds((start, start_ty), (end, end_ty), span);
        let bound = if let Some(joined) = joined {
            joined
        } else {
            self.exact_equality_types(start_ty, end_ty, span);
            self.resolve_inferred_type(start_ty)
        };
        if bound == self.error_type {
            return self.error_type;
        }
        if let Some(step) = self.stored_intervallum_step(step.as_deref(), bound) {
            return self.types.intervallum_with_step(bound, step);
        }
        self.types.intervallum(bound)
    }

    /// The type of a *stored* stepped range carries its step: in a value
    /// position (`let`/`sit` initializer, call argument, return value) nothing
    /// consumes the step, so the range type itself records it.
    ///
    /// The step is a compile-time constant (operator ruling): only a positive
    /// integer literal interns a stepped type. A run-time step (`0‥10 per k`)
    /// or a step over a non-integer bound stays inline-only and is refused
    /// here rather than silently dropped. Positions that consume the step on
    /// the spot (`for range` headers, bracket windows) are unchanged.
    fn stored_intervallum_step(
        &mut self,
        step: Option<&HirExpression>,
        bound: TypeId,
    ) -> Option<radix_types::IndexId> {
        use radix_hir::HirLiteral;
        if self.allow_intervallum_step {
            return None;
        }
        let step = step?;
        let resolved_bound = self.resolve_inferred_type(bound);
        let stride = match &step.kind {
            HirExpressionKind::Literal(HirLiteral::Int(value))
                if *value > 0
                    && crate::passes::resolve::is_integer_intervallum_bound(
                        self.types,
                        resolved_bound,
                    ) =>
            {
                Some(*value)
            }
            _ => None,
        };
        if let Some(stride) = stride {
            return Some(
                self.types
                    .intern_index(radix_types::IndexExpr::Literal(stride)),
            );
        }
        self.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_INTERVALLUM_VALUE_STEP_UNSUPPORTED,
            step.span,
        );
        None
    }

    pub fn require_intervallum_bound(&mut self, ty: TypeId, span: radix_lexer::Span) {
        let resolved = self.resolve_inferred_type(ty);
        if resolved == self.error_type {
            return;
        }
        if self.is_infer(resolved) {
            let numerus = self.numerus_type();
            self.unify_assign_issue(ty, numerus, span, ISSUE_INTERVALLUM_BOUND_REQUIRED);
            return;
        }
        if !is_intervallum_bound_type(self.types, resolved) {
            self.invalid_operand_types(ISSUE_INTERVALLUM_BOUND_REQUIRED, span);
        }
    }

    pub fn intervallum_bound_type(&self, ty: TypeId) -> Option<TypeId> {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Intervallum(bound, _) => Some(*bound),
            _ => None,
        }
    }

    pub fn intervallum_type_id(&self, ty: TypeId) -> Option<TypeId> {
        let resolved = self.resolve_inferred_type(ty);
        matches!(self.types.get(resolved), Type::Intervallum(..)).then_some(resolved)
    }

    /// Whether `ty` is a range whose compile-time step is exactly one. A
    /// non-range type is not a step-one range, so it answers `false`.
    pub fn intervallum_step_is_one(&self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved) {
            Type::Intervallum(_, step) => matches!(
                self.types.get_index(*step),
                radix_types::IndexExpr::Literal(1)
            ),
            _ => false,
        }
    }

    pub fn resolve_intervallum_type(
        &mut self,
        ty: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if let Some(id) = self.intervallum_type_id(ty) {
            return Some(id);
        }
        if matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Infer(_) | Type::Error
        ) {
            return None;
        }
        self.invalid_conversion(ISSUE_INTERVALLUM_TARGET_NOT_RANGE, span);
        None
    }

    /// Helper-backed interval operations currently lower to `Intervallum<i64>`.
    pub fn is_intervallum_runtime_numerus_bound(&mut self, bound: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(bound)),
            Type::Primitive(Primitive::Numerus)
        )
    }

    /// v1 `longitudo` is bare-numerus-only; sized numerics and `instans` wait on runtime helpers.
    pub fn check_intervallum_longitudo_bound(
        &mut self,
        bound: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        if self.is_intervallum_runtime_numerus_bound(bound) {
            return true;
        }
        self.invalid_operand_types(ISSUE_INTERVALLUM_LONGITUDO_BOUND, span);
        false
    }

    /// The range rule of membership: the right operand is an `intervallum` and
    /// the left is a numeric value of its bound type. Reached by the `intra`
    /// keyword and by `check_membership` once it has resolved a range.
    pub fn check_intra_operand(
        &mut self,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        lhs_span: radix_lexer::Span,
        rhs_span: radix_lexer::Span,
    ) {
        let Some(bound) = self.intervallum_bound_type(rhs_ty) else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_INTRA_REQUIRES_INTERVALLUM_RANGE,
                rhs_span,
            );
            return;
        };

        let resolved_lhs = self.resolve_inferred_type(lhs_ty);
        if !self.is_infer(resolved_lhs) && !is_intervallum_bound_type(self.types, resolved_lhs) {
            self.invalid_operand_types(ISSUE_RANGE_OPERAND_MUST_BE_NUMERIC, lhs_span);
        } else {
            self.unify_assign_issue(lhs_ty, bound, lhs_span, ISSUE_RANGE_OPERAND_TYPE_MISMATCH);
        }
    }
}

/// A range step is a positive stride (direction comes from the bounds), so a
/// literal zero or negative step is a compile error. A non-literal step is
/// checked at run time by the range lowering.
fn constant_step_is_not_positive(step: &HirExpression) -> bool {
    use radix_hir::{HirLiteral, HirUnOp};
    match &step.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => *value == 0,
        HirExpressionKind::Literal(HirLiteral::Float(value)) => *value <= 0.0,
        HirExpressionKind::Literal(HirLiteral::FloatText { value, .. }) => *value <= 0.0,
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => matches!(
            &inner.kind,
            HirExpressionKind::Literal(
                HirLiteral::Int(_) | HirLiteral::Float(_) | HirLiteral::FloatText { .. }
            )
        ),
        _ => false,
    }
}
