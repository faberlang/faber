//! Fail-closed MIR gate for the unbounded integer `inf` until the exact
//! carrier lands (inf track U4; inf-delivery rulings 36, 38, 40, 46).
//!
//! Typecheck (inf track U3) now accepts every `inf` operation with its ruled
//! type, but MIR has no bignum carrier yet: lowering an `inf` operation onto
//! the 64-bit integer carrier would compute a wrong answer (wrap, saturate or
//! trap at 2^63) instead of the exact one. Every operation that would
//! *compute* an `inf` value, or convert out of or into one beyond a plain
//! bounded-integer widening, is therefore a named MIR error here. Value flow
//! (declare, store, pass, return, collect, compare) keeps lowering as before,
//! because it never changes a value; it is exact for every value that fits the
//! carrier, and a value that does not can only be built by an operation this
//! gate refuses or by a big literal (`lower_literal`).

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;

/// Issue prefix of every `inf` MIR rejection: `inf_mir_unsupported_<operation>`.
const ISSUE_INF_MIR_UNSUPPORTED: &str = "inf_mir_unsupported";

impl MirError {
    /// An `inf` operation MIR cannot lower exactly yet (`what` names it).
    pub(super) fn inf_unsupported(span: Span, what: &str) -> Self {
        Self {
            message: format!(
                "the unbounded integer `inf` {what} needs the exact bignum MIR runner \
                 (inf track U4), which has not landed"
            ),
            issue: mir_lower_issue(ISSUE_INF_MIR_UNSUPPORTED, what),
            span,
            definition_span: None,
        }
    }
}

/// Binary operators that compute a new integer value. Comparisons, equality,
/// logic and coalescing only read `inf` values and stay lowerable.
fn is_value_computing_op(op: HirBinOp) -> bool {
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

impl FunctionBuilder<'_> {
    fn is_inf(&self, ty: Option<TypeId>) -> bool {
        ty.is_some_and(|ty| {
            let (_, ty) = self.types.peel_access_marker(ty);
            radix_types::numeric_width::is_unbounded_integer_type(self.types, ty)
        })
    }

    fn reject_inf(&mut self, span: Span, what: &str) -> bool {
        self.errors.push(MirError::inf_unsupported(span, what));
        true
    }

    /// Unary `-` / `¬` over `inf`. Returns `true` when it reported.
    pub(super) fn reject_inf_unary(&mut self, op: HirUnOp, operand: &HirExpression, expr: &HirExpression) -> bool {
        let computes = matches!(op, HirUnOp::Neg | HirUnOp::BitNot);
        (computes && (self.is_inf(expr.ty) || self.is_inf(operand.ty)))
            && self.reject_inf(expr.span, "unary arithmetic")
    }

    /// Arithmetic, bitwise or shift operator with an `inf` operand or result.
    pub(super) fn reject_inf_binary(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> bool {
        (is_value_computing_op(op) && (self.is_inf(expr.ty) || self.is_inf(lhs.ty) || self.is_inf(rhs.ty)))
            && self.reject_inf(expr.span, "arithmetic")
    }

    /// `↦` out of `inf`, `↦` into it from anything but a bounded integer, and
    /// the implicit checked narrowing of an `inf` into a bounded slot.
    pub(super) fn reject_inf_conversio(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        expr: &HirExpression,
    ) -> bool {
        let source_inf = self.is_inf(source.ty);
        let target_inf = self.is_inf(Some(target));
        if !source_inf && !target_inf {
            return false;
        }
        if target_inf && !source_inf && self.is_bounded_integer(source.ty) {
            // Exact widening of a value the 64-bit carrier already holds.
            return false;
        }
        self.reject_inf(expr.span, "conversion")
    }

    fn is_bounded_integer(&self, ty: Option<TypeId>) -> bool {
        let Some(ty) = ty else { return false };
        let (_, ty) = self.types.peel_access_marker(ty);
        radix_types::numeric_width::numerus_integer_width(self.types, ty)
            .is_some_and(|width| !width.is_unbounded() && width.integer_range().is_some())
    }

    /// A numeric method on an `inf` receiver (`potentia`, `absolutum`, …), or a
    /// `summa` that yields `inf`.
    pub(super) fn reject_inf_method(&mut self, receiver: &HirExpression, method: Symbol, expr: &HirExpression) -> bool {
        let sum_of_inf = self.is_inf(expr.ty)
            && self
                .context
                .interner
                .is_some_and(|i| i.resolve(method) == "summa");
        (self.is_inf(receiver.ty) || sum_of_inf) && self.reject_inf(expr.span, "numeric method")
    }

    /// A `summa` term-fold whose sum is an `inf`.
    pub(super) fn reject_inf_summa(&mut self, expr: &HirExpression) -> bool {
        self.is_inf(expr.ty) && self.reject_inf(expr.span, "summa")
    }

    /// `x ↑` / `x ↓` on an `inf` place.
    pub(super) fn reject_inf_inc_dec(&mut self, place_ty: Option<TypeId>, span: Span) -> bool {
        self.is_inf(place_ty) && self.reject_inf(span, "increment")
    }

    /// An integer literal beyond `u64` (only an `inf` slot holds it).
    pub(super) fn reject_inf_literal(&mut self, span: Span) -> bool {
        self.reject_inf(span, "literal beyond u64")
    }
}
