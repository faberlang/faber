//! Type rules for `intervallum<T>` construction and `intra` consumption.
//!
//! COMPILER PHASE: Semantic typecheck
//! INPUT: HIR `Intervallum` expressions (start, end, optional step) and `intra`
//! operands with resolved bound and step types
//! OUTPUT: Checked `intervallum<T>` expressions with validated numeric bound
//! types; diagnostics for non-numeric steps, unsupported bound types, range
//! operand mismatches, and `longitudo` bound restrictions

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use crate::{is_intervallum_bound_type, Primitive, Type};
const ISSUE_RANGE_STEP_MUST_BE_NUMERIC: &str = "range_step_must_be_numeric";
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
            if !self.allow_intervallum_step {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_INTERVALLUM_VALUE_STEP_UNSUPPORTED,
                    step_expr.span,
                );
            }
        }

        self.require_intervallum_bound(start_ty, start.span);
        self.require_intervallum_bound(end_ty, end.span);
        self.exact_equality_types(start_ty, end_ty, span);

        let bound = self.resolve_inferred_type(start_ty);
        if bound == self.error_type {
            return self.error_type;
        }
        self.types.intervallum(bound)
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
            Type::Intervallum(bound) => Some(*bound),
            _ => None,
        }
    }

    pub fn intervallum_type_id(&self, ty: TypeId) -> Option<TypeId> {
        let resolved = self.resolve_inferred_type(ty);
        matches!(self.types.get(resolved), Type::Intervallum(_)).then_some(resolved)
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
        if self.is_infer(resolved_lhs) {
            self.unify_assign_issue(lhs_ty, bound, lhs_span, ISSUE_RANGE_OPERAND_TYPE_MISMATCH);
        } else if !is_intervallum_bound_type(self.types, resolved_lhs) {
            self.invalid_operand_types(ISSUE_RANGE_OPERAND_MUST_BE_NUMERIC, lhs_span);
        } else {
            self.unify_assign_issue(lhs_ty, bound, lhs_span, ISSUE_RANGE_OPERAND_TYPE_MISMATCH);
        }
    }
}
