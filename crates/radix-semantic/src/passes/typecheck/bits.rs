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
const BITS_CONVERSION_WIDTH_MISMATCH: &str = "bits_conversion_width_mismatch";
pub(super) const BITS_VIA_CLAUSE_NOT_ALLOWED: &str = "bits_via_clause_not_allowed";
const BITS_VIA_CLAUSE_REQUIRED: &str = "bits_via_clause_required";
const CONVERSIO_VIA_TARGET_TAKES_NO_HINT: &str = "conversio_via_target_takes_no_hint";

/// What the other side of a `bN` conversion is, for the same-width rows
/// (need, unit 1.4a).
enum BitsConversioPeer {
    /// The same bit width, as `bN`, `uN`, `iN` or an IEEE float: the pattern
    /// is kept and read as the other type (rulings C2 and C3).
    SameWidth,
    /// A bit width or number of a different fixed width: a list conversion
    /// takes a `via Le|Be` clause here (B5), a scalar one is refused.
    OtherWidth,
    /// A number with no fixed bit width (`numerus`, decimals, `inf`).
    Unsized,
}

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
        }
        // A refused literal still takes the cell's type, so the store checks
        // that follow do not pile mismatches on the named refusal.
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
                    return expected.map(|slot| self.bits_type_of(slot));
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

    /// The bits-side element/scalar type of a conversion side: the type itself
    /// or, for a `lista<T>`, its element.
    fn bits_conversio_cell(&self, ty: TypeId, listed: bool) -> TypeId {
        let resolved = self.resolve_inferred_type(ty);
        if listed && let Type::Array(elem) = self.types.get(resolved) {
            return *elem;
        }
        resolved
    }

    fn bits_conversio_peer(&self, width: BitsWidth, other: TypeId) -> Option<BitsConversioPeer> {
        use radix_types::NumericWidth as W;
        let other = self.numeric_expected_type(self.resolve_inferred_type(other));
        let same = |bits: u32| {
            if bits == width.bits() {
                BitsConversioPeer::SameWidth
            } else {
                BitsConversioPeer::OtherWidth
            }
        };
        match self.types.get(other) {
            Type::Bits(peer) => Some(same(peer.bits())),
            Type::SizedNumeric(_, W::U8) => Some(same(8)),
            Type::SizedNumeric(_, W::U16) => Some(same(16)),
            Type::SizedNumeric(_, W::U32) => Some(same(32)),
            Type::SizedNumeric(_, W::U64) => Some(same(64)),
            Type::SizedNumeric(_, W::I8) => Some(same(8)),
            Type::SizedNumeric(_, W::I16 | W::F16 | W::BF16) => Some(same(16)),
            Type::SizedNumeric(_, W::I32 | W::F32) => Some(same(32)),
            Type::SizedNumeric(_, W::I64 | W::F64) | Type::Primitive(Primitive::Fractus) => {
                Some(same(64))
            }
            Type::SizedNumeric(_, W::D64 | W::Inf) | Type::Primitive(Primitive::Numerus) => {
                Some(BitsConversioPeer::Unsized)
            }
            _ => None,
        }
    }

    /// `lista<bN> ↦ textus` (UTF-8, UTF-16, UTF-32 decode; fallible) and
    /// `textus ↦ lista<bN>` (encode; infallible) at widths 8, 16 and 32 (need
    /// 6a13d711, B4). The rule is an explicit allowance for bits-list sources
    /// only: `lista<uN> ↦ textus` and every other element type stay on the
    /// ordinary refusal. `b64` has no text conversion and takes the named
    /// bits width refusal. `None` when neither side is the bits-list/text
    /// pair, so the ordinary rows run.
    pub(super) fn check_bits_text_conversio(
        &mut self,
        source_ty: TypeId,
        target: TypeId,
        params: &[Symbol],
        recovery: Option<&mut HirExpression>,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let is_text = |this: &Self, ty: TypeId| {
            matches!(
                this.types.get(this.resolve_inferred_type(ty)),
                Type::Primitive(Primitive::Textus)
            )
        };
        let list_width =
            |this: &Self, ty: TypeId| match this.types.get(this.resolve_inferred_type(ty)) {
                Type::Array(elem) => this.bits_width_of(*elem),
                _ => None,
            };
        let (width, decode) = if is_text(self, target) && !is_text(self, source_ty) {
            (list_width(self, source_ty)?, true)
        } else if is_text(self, source_ty) && !is_text(self, target) {
            (list_width(self, target)?, false)
        } else {
            return None;
        };
        let result = self.resolve_inferred_type(target);
        if matches!(width, BitsWidth::B64) {
            self.invalid_conversion(BITS_CONVERSION_WIDTH_MISMATCH, span);
            return Some(self.error_type);
        }
        if !params.is_empty() {
            self.invalid_conversion(BITS_VIA_CLAUSE_NOT_ALLOWED, span);
            return Some(self.error_type);
        }
        if let Some(recovery) = recovery {
            self.note_conversio_recovery_checked(recovery, result);
        } else if decode {
            self.check_conversio_propagation_channel(source_ty, result, span);
        }
        Some(result)
    }

    /// `bN` conversions: scalar `bN ↔ uN | iN | fN` and the element-wise list
    /// forms at the same N take no `via` clause and keep the pattern. A list
    /// conversion that changes the bit width takes exactly a `via Le|Be`
    /// clause (B5: a clause exactly when the width changes); every other
    /// number pairing is a named refusal. `None` when no side is a `bN` (or
    /// the other side is not a number or bits), so the ordinary rows run.
    pub(super) fn check_bits_conversio(
        &mut self,
        source_ty: TypeId,
        target: TypeId,
        params: &[Symbol],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let has_hint = !params.is_empty();
        let scalar = (self.bits_width_of(source_ty), self.bits_width_of(target));
        let listed = |this: &Self, ty: TypeId| match this.types.get(this.resolve_inferred_type(ty))
        {
            Type::Array(elem) => this.bits_width_of(*elem).is_some(),
            _ => false,
        };
        let is_list = |this: &Self, ty: TypeId| {
            matches!(
                this.types.get(this.resolve_inferred_type(ty)),
                Type::Array(_)
            )
        };
        let lists = if scalar.0.is_some() || scalar.1.is_some() {
            false
        } else if (listed(self, source_ty) && is_list(self, target))
            || (listed(self, target) && is_list(self, source_ty))
        {
            true
        } else {
            // A numeric list target takes a `via` clause only from a bits
            // list (the lowering lets the clause through on any numeric list,
            // since only the checker sees the source).
            if has_hint
                && let Type::Array(elem) = self.types.get(self.resolve_inferred_type(target))
                && matches!(
                    self.types.get(self.numeric_expected_type(*elem)),
                    Type::SizedNumeric(..)
                        | Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                )
            {
                self.invalid_conversion(CONVERSIO_VIA_TARGET_TAKES_NO_HINT, span);
                return Some(self.error_type);
            }
            return None;
        };
        let source_cell = self.bits_conversio_cell(source_ty, lists);
        let target_cell = self.bits_conversio_cell(target, lists);
        let (width, other) = match (
            self.bits_width_of(source_cell),
            self.bits_width_of(target_cell),
        ) {
            (Some(width), _) => (width, target_cell),
            (None, Some(width)) => (width, source_cell),
            (None, None) => return None,
        };
        // A conversion between two bN of one width is the identity row.
        if self.types.equals(
            self.resolve_inferred_type(source_ty),
            self.resolve_inferred_type(target),
        ) {
            if has_hint {
                self.invalid_conversion(BITS_VIA_CLAUSE_NOT_ALLOWED, span);
                return Some(self.error_type);
            }
            return None;
        }
        let issue = match self.bits_conversio_peer(width, other)? {
            BitsConversioPeer::SameWidth if has_hint => BITS_VIA_CLAUSE_NOT_ALLOWED,
            BitsConversioPeer::SameWidth => return Some(self.resolve_inferred_type(target)),
            BitsConversioPeer::OtherWidth if lists && self.conversio_has_endian_hint(params) => {
                return Some(self.resolve_inferred_type(target));
            }
            BitsConversioPeer::OtherWidth if lists => BITS_VIA_CLAUSE_REQUIRED,
            BitsConversioPeer::OtherWidth | BitsConversioPeer::Unsized => {
                BITS_CONVERSION_WIDTH_MISMATCH
            }
        };
        self.invalid_conversion(issue, span);
        Some(self.error_type)
    }
}
