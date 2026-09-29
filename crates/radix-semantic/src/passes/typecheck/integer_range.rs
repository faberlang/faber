//! Range typing for trapping integer expressions (d11-6 rulings 8, 12, 13, 16, 19, 20).
//!
//! An integer expression's type is the smallest integer type holding every
//! possible result, computed by interval arithmetic over the operands' declared
//! types (`radix_types::numeric_range`). Integer constants are untyped exact
//! values: beside a typed operand a constant takes part by value; alone it
//! defaults to `int`, or takes an integer slot's type. `wrapping` and
//! `saturating` words keep their ring and width, so they never reach this
//! module's result rules. Device functions keep today's typing until N6.

use super::ops::IntFamily;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_types::numeric_range::{self as range, IntRange};

/// Ruling 8: `u64` with a signed typed operand has no lossless join.
pub(super) const U64_SIGNED_ARITHMETIC_REQUIRES_CONVERSION: &str =
    "u64_signed_arithmetic_requires_conversion";
/// Ruling 16: integer, decimal and float never mix implicitly.
pub(super) const NUMERIC_FAMILY_CROSSING_REQUIRES_CONVERSIO: &str =
    "numeric_family_crossing_requires_conversio";
const FLOAT_CONSTANT_INEXACT: &str = "float_constant_inexact";

/// Exact value of an integer constant expression (literals and operators over
/// literals), or `None` when the expression is not a pure integer constant.
///
/// Floor division and remainder follow ruling 14; every operator computes the
/// exact value (ruling 17) and an overflowing intermediate is not folded.
#[must_use]
pub fn const_int_value(expr: &HirExpression) -> Option<i128> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => Some(i128::from(*value)),
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => {
            const_int_value(inner)?.checked_neg()
        }
        HirExpressionKind::Unary(radix_hir::HirUnOp::BitNot, inner) => {
            Some(!const_int_value(inner)?)
        }
        HirExpressionKind::Binary(op, lhs, rhs) => {
            fold_const_binary(*op, const_int_value(lhs)?, const_int_value(rhs)?)
        }
        _ => None,
    }
}

fn floor_div(a: i128, b: i128) -> Option<i128> {
    let q = a.checked_div(b)?;
    Some(if a % b != 0 && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    })
}

fn fold_const_binary(op: HirBinOp, a: i128, b: i128) -> Option<i128> {
    match op {
        HirBinOp::Add => a.checked_add(b),
        HirBinOp::Sub => a.checked_sub(b),
        HirBinOp::Mul => a.checked_mul(b),
        HirBinOp::Div => floor_div(a, b),
        HirBinOp::Mod => Some(a - b.checked_mul(floor_div(a, b)?)?),
        HirBinOp::BitAnd => Some(a & b),
        HirBinOp::BitOr => Some(a | b),
        HirBinOp::BitXor => Some(a ^ b),
        HirBinOp::Shl => {
            let n = u32::try_from(b).ok().filter(|n| *n < 100)?;
            a.checked_mul(1_i128 << n)
        }
        HirBinOp::Shr => {
            let n = u32::try_from(b).ok().filter(|n| *n < 127)?;
            Some(a >> n)
        }
        _ => None,
    }
}

/// How a scalar operand takes part in the range rule.
#[derive(Clone, Copy)]
enum Operand {
    /// Untyped integer constant with its exact value.
    Const(i128),
    /// Trapping integer of a declared width.
    Plain(NumericWidth),
    /// Float, decimal, word, or non-numeric: not range-typed.
    Other,
}

/// What an expected type means for a constant-only expression.
enum ConstSlot {
    /// An integer slot: the constant takes its type.
    Integer(TypeId),
    /// A float slot: the constant takes its type after an exactness check.
    Float(TypeId, NumericWidth),
    /// A word or decimal slot keeps the pre-range shaping.
    Legacy,
    /// No slot: the constant defaults to `int`.
    Default,
}

impl TypeChecker<'_> {
    /// Declared width of a trapping (plain) integer type; `None` for words,
    /// decimals, floats and non-numerics.
    fn plain_int_width(&self, ty: TypeId) -> Option<NumericWidth> {
        let resolved = self.resolve_inferred_type(ty);
        let (_, resolved) = self.types.peel_access_marker(resolved);
        radix_types::numeric_width::numerus_integer_width(&self.types, resolved)
    }

    pub(super) fn width_type(&mut self, width: NumericWidth) -> TypeId {
        if width == NumericWidth::I64 {
            self.numerus_type()
        } else {
            self.types.sized_numeric(Primitive::Numerus, width)
        }
    }

    fn const_slot(&self, expected: Option<TypeId>) -> ConstSlot {
        let Some(expected) = expected else {
            return ConstSlot::Default;
        };
        let expected = self.resolve_inferred_type(expected);
        let Some(shape) = self.numeric_shape(expected) else {
            return ConstSlot::Default;
        };
        let ty = self.numeric_expected_type(expected);
        if shape.family != IntFamily::Plain || shape.width.is_decimal() {
            return ConstSlot::Legacy;
        }
        if shape.prim == Primitive::Fractus {
            return ConstSlot::Float(ty, shape.width);
        }
        ConstSlot::Integer(ty)
    }

    /// Whether a constant-only pair under `expected` keeps the old shaping.
    pub(super) fn const_pair_is_legacy(&self, expected: Option<TypeId>) -> bool {
        matches!(self.const_slot(expected), ConstSlot::Legacy)
    }

    /// Check one integer-constant operand without shaping it to a peer.
    pub(super) fn check_const_operand(&mut self, expr: &mut HirExpression) -> TypeId {
        let wide = matches!(
            &expr.kind,
            HirExpressionKind::Literal(HirLiteral::Int(value)) if i64::try_from(*value).is_err()
        );
        if wide {
            let u64_ty = self
                .types
                .sized_numeric(Primitive::Numerus, NumericWidth::U64);
            return self.check_expr_with_expected(expr, Some(u64_ty));
        }
        self.check_expr(expr)
    }

    /// Check an integer-constant operand beside an already checked peer.
    ///
    /// A trapping-integer peer leaves the constant unshaped (it joins by
    /// value). Float and word peers give the constant their type (ruling 20
    /// checks the value), and an inference-hole peer keeps the old shaping.
    pub(super) fn check_const_beside(
        &mut self,
        expr: &mut HirExpression,
        peer: TypeId,
        expected: Option<TypeId>,
    ) -> TypeId {
        let resolved = self.resolve_inferred_type(peer);
        if self.is_infer(resolved) {
            let hint = self
                .literal_numeric_expected_from_peer(expr, peer)
                .or_else(|| self.literal_numeric_expected(expr, expected));
            return self.check_expr_with_expected(expr, hint);
        }
        if resolved == self.error_type
            || self.plain_int_width(resolved).is_some()
            || self.numeric_shape(resolved).is_none()
        {
            return self.check_const_operand(expr);
        }
        let hint = self.numeric_expected_type(resolved);
        self.check_expr_with_expected(expr, Some(hint))
    }

    fn classify_operand(&self, expr: &HirExpression, ty: TypeId) -> Operand {
        if let Some(value) = const_int_value(expr) {
            return Operand::Const(value);
        }
        self.plain_int_width(ty)
            .map_or(Operand::Other, Operand::Plain)
    }

    fn operand_range(operand: Operand) -> Option<IntRange> {
        match operand {
            Operand::Const(value) => Some((value, value)),
            Operand::Plain(width) => width.integer_range(),
            Operand::Other => None,
        }
    }

    /// Type of a constant-only expression: the integer slot's type, a float
    /// slot's type after the exactness check, or `int`.
    fn constant_expression_type(
        &mut self,
        value: Option<i128>,
        expected: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        match self.const_slot(expected) {
            ConstSlot::Legacy => None,
            ConstSlot::Integer(ty) => Some(ty),
            ConstSlot::Float(ty, width) => {
                let limit: i128 = match width {
                    NumericWidth::F32 => 1 << 24,
                    NumericWidth::F16 => 1 << 11,
                    NumericWidth::BF16 => 1 << 8,
                    _ => 1 << 53,
                };
                if value.is_some_and(|v| v.abs() > limit) {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        FLOAT_CONSTANT_INEXACT,
                        span,
                    );
                    return Some(self.error_type);
                }
                Some(ty)
            }
            ConstSlot::Default => Some(self.numerus_type()),
        }
    }

    /// Range-typed result of a scalar arithmetic operator (`+ - * / %`).
    ///
    /// `None` means the pair is not integer-range typed (float, decimal, word,
    /// inference hole, device function) and the caller keeps the family rules.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn arithmetic_range_type(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        expected: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let (l, r) = self.range_operands(lhs, rhs, lhs_ty, rhs_ty)?;
        if let (Operand::Const(a), Operand::Const(b)) = (l, r) {
            return self.constant_expression_type(fold_const_binary(op, a, b), expected, span);
        }
        if let (Operand::Plain(a), Operand::Plain(b)) = (l, r)
            && ((a == NumericWidth::U64 && b.is_signed_int())
                || (b == NumericWidth::U64 && a.is_signed_int()))
        {
            self.invalid_operand_types(U64_SIGNED_ARITHMETIC_REQUIRES_CONVERSION, span);
            return Some(self.error_type);
        }
        let (a, b) = (Self::operand_range(l)?, Self::operand_range(r)?);
        let result = match op {
            HirBinOp::Add => range::add(a, b),
            HirBinOp::Sub => range::sub(a, b),
            HirBinOp::Mul => range::mul(a, b),
            HirBinOp::Div => range::floor_div(a, b),
            HirBinOp::Mod => range::floor_rem(a, b),
            _ => return None,
        };
        Some(self.width_type(range::smallest_width(result)))
    }

    /// Result type of `÷` (d11-6 ruling 15): integer operands of any width,
    /// including constants, convert to `f64` and divide, so the result is
    /// `f64`. Float and decimal operands follow `/`'s family rules, and an
    /// integer beside a float or decimal is rejected like `/` (ruling 16).
    pub(super) fn true_division_type(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        if self.range_operands(lhs, rhs, lhs_ty, rhs_ty).is_some() {
            return self.fractus_type();
        }
        let joined = self.numeric_arithmetic_bin(lhs_ty, rhs_ty, span);
        let resolved = self.resolve_inferred_type(joined);
        let integer = self
            .numeric_shape(resolved)
            .is_some_and(|shape| shape.prim == Primitive::Numerus && !shape.width.is_decimal());
        if integer { self.fractus_type() } else { joined }
    }

    /// Range-typed result of `∧ ∨ ⊻ ⇐ ⇒` over trapping integers.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bitwise_range_type(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        expected: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let (l, r) = self.range_operands(lhs, rhs, lhs_ty, rhs_ty)?;
        if let (Operand::Const(a), Operand::Const(b)) = (l, r) {
            return self.constant_expression_type(fold_const_binary(op, a, b), expected, span);
        }
        let (a, b) = (Self::operand_range(l)?, Self::operand_range(r)?);
        let result = match op {
            HirBinOp::BitAnd => range::bit_and(a, b),
            HirBinOp::BitOr | HirBinOp::BitXor => range::hull(a, b),
            HirBinOp::Shl => range::shl(a, b),
            HirBinOp::Shr => a,
            _ => return None,
        };
        Some(self.width_type(range::smallest_width(result)))
    }

    fn range_operands(
        &self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<(Operand, Operand)> {
        if self.current_device_function {
            return None;
        }
        for ty in [lhs_ty, rhs_ty] {
            let resolved = self.resolve_inferred_type(ty);
            if resolved == self.error_type || self.is_infer(resolved) {
                return None;
            }
        }
        let l = self.classify_operand(lhs, lhs_ty);
        let r = self.classify_operand(rhs, rhs_ty);
        if matches!(l, Operand::Other) || matches!(r, Operand::Other) {
            return None;
        }
        Some((l, r))
    }

    /// Range-typed result of unary `-` / `¬` over a non-constant trapping
    /// integer; `None` keeps the operand's own type.
    pub(super) fn unary_range_type(
        &mut self,
        op: radix_hir::HirUnOp,
        operand: &HirExpression,
        operand_ty: TypeId,
    ) -> Option<TypeId> {
        if self.current_device_function || const_int_value(operand).is_some() {
            return None;
        }
        let width = self.plain_int_width(operand_ty)?;
        let a = width.integer_range()?;
        let result = match op {
            radix_hir::HirUnOp::Neg => range::neg(a),
            radix_hir::HirUnOp::BitNot => range::bit_not(a),
            radix_hir::HirUnOp::Not => return None,
        };
        Some(self.width_type(range::smallest_width(result)))
    }

    /// Branch/element join of two trapping integer types: the smallest type
    /// holding both ranges (ruling 21).
    pub(super) fn integer_hull_type(&mut self, a: TypeId, b: TypeId) -> Option<TypeId> {
        let (wa, wb) = (self.plain_int_width(a)?, self.plain_int_width(b)?);
        let joined = range::hull(wa.integer_range()?, wb.integer_range()?);
        Some(self.width_type(range::smallest_width(joined)))
    }

    /// Type of a `summa` reduction over integer terms: the element count is
    /// unbounded, so the sum sits at the 64-bit cap and keeps the term
    /// range's sign (ruling 21). Other term types keep their own type.
    pub(super) fn summa_result_type(&mut self, elem: TypeId) -> TypeId {
        if self.current_device_function {
            return elem;
        }
        let Some((lo, _)) = self
            .plain_int_width(elem)
            .and_then(NumericWidth::integer_range)
        else {
            return elem;
        };
        self.width_type(if lo < 0 {
            NumericWidth::I64
        } else {
            NumericWidth::U64
        })
    }

    /// Expected type handed to a unary operand. A literal takes the slot's
    /// width; any other operand keeps its own type (the result may be wider),
    /// so an open inference-hole slot must not bind to the operand's type.
    pub(super) fn unary_operand_expected(
        &self,
        operand: &HirExpression,
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        if self.current_device_function || const_int_value(operand).is_some() {
            return expected;
        }
        expected.filter(|ty| !self.is_infer(self.resolve_inferred_type(*ty)))
    }

    /// Expected type handed to the branches of a value conditional. An open
    /// (non-union) hole would bind to the first branch; the join is computed
    /// by [`Self::common_type`] instead (ruling 21).
    pub(super) fn branch_expected(&self, expected: Option<TypeId>) -> Option<TypeId> {
        if self.current_device_function {
            return expected;
        }
        expected.filter(|ty| {
            let resolved = self.resolve_inferred_type(*ty);
            !self.is_infer(resolved) || self.infer_union_var_of(resolved).is_some()
        })
    }

    /// Strict equality compares exact types, but an untyped integer constant
    /// beside a trapping-integer operand takes part by value (ruling 12):
    /// `x % 2 ≡ 0` holds for every width of `x % 2` when 0 fits it.
    pub(super) fn equality_constant_exempt(
        &self,
        lhs: &HirExpression,
        lhs_ty: TypeId,
        rhs: &HirExpression,
        rhs_ty: TypeId,
    ) -> bool {
        if self.current_device_function {
            return false;
        }
        let (value, other) = match (const_int_value(lhs), const_int_value(rhs)) {
            (Some(value), None) => (value, rhs_ty),
            (None, Some(value)) => (value, lhs_ty),
            _ => return false,
        };
        self.plain_int_width(other)
            .and_then(NumericWidth::integer_range)
            .is_some_and(|(lo, hi)| lo <= value && value <= hi)
    }
}
