//! Operator and literal rules for the raw-bits markers `b8`, `b16`, `b32`
//! and `b64` (need, unit 1.3a).
//!
//! A `bN` is exactly N bits and is not a number. It has equality, unsigned
//! ordering and the bit operations `∧ ∨ ⊻ ¬ ⇐ ⇒`; it has no arithmetic and no
//! mixing with numbers or other widths. An integer literal written where a
//! `bN` is expected takes that type when it fits N bits. Number types and
//! number shifts are untouched: these rules fire only when an operand's type
//! is a `bN`.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_types::BitsWidth;

const BITS_ARITHMETIC_REFUSED: &str = "bits_arithmetic_refused";
const BITS_OPERAND_MISMATCH: &str = "bits_operand_mismatch";
pub(super) const BITS_LITERAL_OUT_OF_RANGE: &str = "bits_literal_out_of_range";
const INTEGER_OPERANDS_REQUIRED: &str = "integer_operands_required";

impl TypeChecker<'_> {
    /// The `bN` width of `ty`, looking through aliases and resolved holes.
    pub(super) fn bits_width_of(&self, ty: TypeId) -> Option<BitsWidth> {
        let resolved = self.numeric_expected_type(self.resolve_inferred_type(ty));
        match self.types.get(resolved) {
            Type::Bits(width) => Some(*width),
            _ => None,
        }
    }

    /// The `bN` type behind `ty` with aliases and holes resolved away.
    fn bits_type_of(&self, ty: TypeId) -> TypeId {
        self.numeric_expected_type(self.resolve_inferred_type(ty))
    }

    /// The `bN` width an expected slot asks for, when it asks for one.
    pub(super) fn expected_bits_width(&self, expected: Option<TypeId>) -> Option<BitsWidth> {
        self.bits_width_of(expected?)
    }

    /// Whether `expr` is a written integer constant (`0xff`, `256`, `-1`).
    fn is_written_integer(expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::Literal(HirLiteral::Int(_) | HirLiteral::BigInt(_)) => true,
            HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => {
                Self::is_written_integer(inner)
            }
            _ => false,
        }
    }

    /// Type a written integer beside a `bN` peer as that `bN` (B3). `None`
    /// when `peer` is not a `bN` or `expr` is not a written integer, so the
    /// caller keeps its number path.
    pub(super) fn bits_peer_literal(
        &mut self,
        expr: &mut HirExpression,
        peer: TypeId,
    ) -> Option<TypeId> {
        self.bits_width_of(peer)?;
        if !Self::is_written_integer(expr) {
            return None;
        }
        let peer = self.bits_type_of(peer);
        Some(self.check_expr_with_expected(expr, Some(peer)))
    }

    /// Type an integer literal read in a `bN` slot, or report that it does
    /// not fit N bits.
    pub(super) fn bits_literal_type(
        &mut self,
        magnitude: Option<u64>,
        width: BitsWidth,
        expected: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> TypeId {
        let fits = magnitude.is_some_and(|value| width.bits() == 64 || value >> width.bits() == 0);
        if !fits {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                BITS_LITERAL_OUT_OF_RANGE,
                span,
            );
            return self.error_type;
        }
        self.bits_type_of(expected.expect("bits width came from an expected type"))
    }

    /// Unary `-` over a `bN`. `None` when neither the slot nor the operand is
    /// a `bN`.
    pub(super) fn bits_negation(
        &mut self,
        operand: &HirExpression,
        operand_ty: Option<TypeId>,
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        match operand_ty {
            None => {
                // `-1` into a b8: a negative written integer fits no bN.
                if self.expected_bits_width(expected).is_some() && Self::is_written_integer(operand)
                {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        BITS_LITERAL_OUT_OF_RANGE,
                        operand.span,
                    );
                    return Some(self.error_type);
                }
                None
            }
            Some(ty) => {
                self.bits_width_of(ty)?;
                self.invalid_operand_types(BITS_ARITHMETIC_REFUSED, operand.span);
                Some(self.error_type)
            }
        }
    }

    /// Binary operators with a `bN` operand. `None` when neither operand is
    /// a `bN`, so the ordinary number rules run unchanged.
    pub(super) fn bits_binary(
        &mut self,
        op: HirBinOp,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let lhs_width = self.bits_width_of(lhs_ty);
        let rhs_width = self.bits_width_of(rhs_ty);
        if lhs_width.is_none() && rhs_width.is_none() {
            return None;
        }
        let either_error = self.resolve_inferred_type(lhs_ty) == self.error_type
            || self.resolve_inferred_type(rhs_ty) == self.error_type;
        match op {
            HirBinOp::Add
            | HirBinOp::Sub
            | HirBinOp::Mul
            | HirBinOp::Div
            | HirBinOp::Mod
            | HirBinOp::TrueDiv => {
                if either_error {
                    return Some(self.error_type);
                }
                self.invalid_operand_types(BITS_ARITHMETIC_REFUSED, span);
                Some(self.error_type)
            }
            HirBinOp::Eq
            | HirBinOp::NotEq
            | HirBinOp::StrictEq
            | HirBinOp::StrictNotEq
            | HirBinOp::Is
            | HirBinOp::IsNot
            | HirBinOp::Lt
            | HirBinOp::Gt
            | HirBinOp::LtEq
            | HirBinOp::GtEq => {
                if !either_error && lhs_width != rhs_width {
                    self.invalid_operand_types(BITS_OPERAND_MISMATCH, span);
                }
                Some(self.bool_type())
            }
            HirBinOp::BitAnd | HirBinOp::BitOr | HirBinOp::BitXor => {
                if either_error {
                    return Some(self.error_type);
                }
                if lhs_width != rhs_width {
                    self.invalid_operand_types(BITS_OPERAND_MISMATCH, span);
                    return Some(self.error_type);
                }
                Some(self.bits_type_of(lhs_ty))
            }
            HirBinOp::Shl | HirBinOp::Shr => {
                if either_error {
                    return Some(self.error_type);
                }
                // The count is an integer of any width; a `bN` count or a
                // `bN` shifted by a non-integer is a mix.
                if lhs_width.is_none() || rhs_width.is_some() {
                    self.invalid_operand_types(BITS_OPERAND_MISMATCH, span);
                    return Some(self.error_type);
                }
                let count = self.resolve_inferred_type(rhs_ty);
                if self.is_infer(count) {
                    let numerus = self.numerus_type();
                    self.unify_assign_issue(rhs_ty, numerus, span, INTEGER_OPERANDS_REQUIRED);
                } else if self.integer_shape(count).is_none() {
                    self.invalid_operand_types(INTEGER_OPERANDS_REQUIRED, span);
                    return Some(self.error_type);
                }
                Some(self.bits_type_of(lhs_ty))
            }
            _ => None,
        }
    }
}
