//! Named rejections for the unbounded integer `inf` until its semantics land.
//!
//! inf track U2 admits `inf` as a *value* type: declared, stored, passed,
//! returned, held in fields and lists, compared. Its arithmetic, exact
//! constant folding and conversions belong to U3/U4 (inf-delivery rulings
//! 34–40). Until then every operation that would *compute a new `inf` value*,
//! or convert into or out of one, is a named compile error rather than a
//! silent fall-through: with `integer_range() == None` the range rule, the
//! store checks and the backends all read `inf` as "not an integer" and would
//! otherwise run the operation on a 64-bit carrier and wrap or saturate.

use super::TypeChecker;
use radix_hir::{HirBinOp, HirExpression};
use radix_lexer::Span;
use radix_types::{NumericWidth, TypeId};

/// Arithmetic, bitwise or shift operator on an `inf` operand or result.
pub(super) const ISSUE_INF_ARITHMETIC_UNSUPPORTED: &str = "inf_arithmetic_unsupported";

/// Conversion out of `inf`, or into it from anything but a bounded integer.
pub(super) const ISSUE_INF_CONVERSIO_UNSUPPORTED: &str = "inf_conversio_unsupported";

/// Binary operators that compute a new integer value. Comparisons, equality,
/// logic and coalescing only read `inf` values and stay admitted.
pub(super) fn is_value_computing_op(op: HirBinOp) -> bool {
    matches!(
        op,
        HirBinOp::Add
            | HirBinOp::Sub
            | HirBinOp::Mul
            | HirBinOp::Div
            | HirBinOp::TrueDiv
            | HirBinOp::Mod
            | HirBinOp::BitAnd
            | HirBinOp::BitOr
            | HirBinOp::BitXor
            | HirBinOp::Shl
            | HirBinOp::Shr
    )
}

impl TypeChecker<'_> {
    /// Whether `ty` is the unbounded integer (through aliases, access markers
    /// and solved inference holes).
    pub(super) fn is_inf_type(&self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        let (_, resolved) = self.types.peel_access_marker(resolved);
        radix_types::numeric_width::is_unbounded_integer_type(&self.types, resolved)
    }

    fn is_inf_expr(&self, expr: &HirExpression) -> bool {
        expr.ty.is_some_and(|ty| self.is_inf_type(ty))
    }

    /// Reject a value-computing operation whose operand or result is `inf`.
    /// Returns `true` when it reported.
    pub(super) fn reject_inf_arithmetic(
        &mut self,
        operands: &[&HirExpression],
        result: TypeId,
        span: Span,
    ) -> bool {
        if !self.is_inf_type(result) && !operands.iter().any(|operand| self.is_inf_expr(operand)) {
            return false;
        }
        self.invalid_operand_types(ISSUE_INF_ARITHMETIC_UNSUPPORTED, span);
        true
    }

    /// Reject a receiver method that computes a new `inf` value: any numeric
    /// method (`absolutum`, `potentia`, `signum`, …) on an `inf` receiver, and
    /// the `summa` of a collection of `inf`. Returns `true` when it reported.
    pub(super) fn reject_inf_method(
        &mut self,
        entry: &radix_intrinsics::MethodIntrinsic,
        receiver_ty: TypeId,
        result: TypeId,
        span: Span,
    ) -> bool {
        let carrier = self.value_carrier_type(receiver_ty);
        let numeric_on_inf = matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Numeric)
            && self.is_inf_type(carrier);
        let sum_of_inf = entry.faber_name == "summa" && self.is_inf_type(result);
        if !numeric_on_inf && !sum_of_inf {
            return false;
        }
        self.invalid_operand_types(ISSUE_INF_ARITHMETIC_UNSUPPORTED, span);
        true
    }

    /// Reject `↦`/`↤` out of `inf`, and into `inf` from a source that is not a
    /// bounded trapping integer (whose widening into `inf` is exact and
    /// total). Returns `true` when it reported.
    pub(super) fn reject_inf_conversio(
        &mut self,
        source_ty: TypeId,
        target_ty: TypeId,
        span: Span,
    ) -> bool {
        let source_inf = self.is_inf_type(source_ty);
        let target_inf = self.is_inf_type(target_ty);
        if !source_inf && !target_inf {
            return false;
        }
        if target_inf && !source_inf && self.is_bounded_integer_type(source_ty) {
            return false;
        }
        self.invalid_conversion(ISSUE_INF_CONVERSIO_UNSUPPORTED, span);
        true
    }

    fn is_bounded_integer_type(&self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        let (_, resolved) = self.types.peel_access_marker(resolved);
        radix_types::numeric_width::numerus_integer_width(&self.types, resolved)
            .is_some_and(|width| width != NumericWidth::Inf && width.integer_range().is_some())
    }
}
