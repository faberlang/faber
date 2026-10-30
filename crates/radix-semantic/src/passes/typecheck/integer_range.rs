//! Range typing for trapping integer expressions (d11-6 rulings 8, 12, 13, 16, 19, 20).
//!
//! An integer expression's type is the smallest integer type holding every
//! possible result, computed by interval arithmetic over the operands' declared
//! types (`radix_types::numeric_range`). Integer constants are untyped exact
//! values: beside a typed operand a constant takes part by value; alone it
//! defaults to `int`, or takes an integer slot's type. `wrapping` and
//! `saturating` words compute exactly like their width: the word reduces or
//! clamps only where a value is stored into a slot. Device functions keep
//! today's typing until N6.

use super::ops::IntFamily;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_types::Magnus;
use radix_types::big_literal::parse_big_literal;
use radix_types::numeric_range::{self as range, IntRange};

/// Ruling 8: `u64` with a signed typed operand has no lossless join.
pub(super) const U64_SIGNED_ARITHMETIC_REQUIRES_CONVERSION: &str =
    "u64_signed_arithmetic_requires_conversion";
/// Ruling 16: integer, decimal and float never mix implicitly.
pub(super) const NUMERIC_FAMILY_CROSSING_REQUIRES_CONVERSIO: &str =
    "numeric_family_crossing_requires_conversio";
const FLOAT_CONSTANT_INEXACT: &str = "float_constant_inexact";
const FLOAT_CONSTANT_OUT_OF_RANGE: &str = "float_constant_out_of_range";
const SHIFT_COUNT_NEGATIVE: &str = "shift_count_negative";
/// Ruling 34: an exact constant that cannot land where it is written.
const NUMERUS_LITERAL_OUT_OF_RANGE: &str = "numerus_literal_out_of_range";
const POTENTIA_EXPONENT_NEGATIVE: &str = "potentia_exponent_negative";

/// Bit-length ceiling of a compile-time fold (ruling 45's implementation
/// ceiling, applied to the compiler): a constant expression whose exact value
/// would exceed it (`1 ⇐ 4000000000`) is not folded, so it is not a constant
/// and the operation is left to run time.
const FOLD_CEILING_BITS: u64 = 1 << 20;

/// Exact value of an integer constant expression (literals and operators over
/// literals), at any size (inf track, ruling 34), or `None` when the
/// expression is not a pure integer constant.
///
/// Floor division and remainder follow ruling 14; every operator computes the
/// exact value (ruling 17). The fold runs on `i128` while both operands fit
/// it and on the shared bignum otherwise, so it never gives up on size; it
/// gives up only on a zero divisor, a negative shift count and the
/// [`FOLD_CEILING_BITS`] ceiling.
#[must_use]
pub fn const_int_exact(expr: &HirExpression) -> Option<Magnus> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => Some(Magnus::from_u64(*value)),
        HirExpressionKind::Literal(HirLiteral::BigInt(digits)) => parse_big_literal(digits),
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => Some(-const_int_exact(inner)?),
        HirExpressionKind::Unary(radix_hir::HirUnOp::BitNot, inner) => {
            Some(const_int_exact(inner)?.bit_not())
        }
        HirExpressionKind::Binary(op, lhs, rhs) => {
            fold_const_binary(*op, &const_int_exact(lhs)?, &const_int_exact(rhs)?)
        }
        _ => None,
    }
}

/// Exact value of an integer constant expression when it fits `i128`; `None`
/// for a non-constant or a constant beyond `i128` (see [`const_int_exact`]).
#[must_use]
pub fn const_int_value(expr: &HirExpression) -> Option<i128> {
    const_int_exact(expr)?.to_i128()
}

/// Whether `expr` is an integer constant expression of any size.
#[must_use]
pub fn is_const_int(expr: &HirExpression) -> bool {
    const_int_exact(expr).is_some()
}

/// Exact `i128` value of an integer constant expression that may also name a
/// recorded compile-time integer binding through `values`:
/// an immutable `const int` local or a module constant. A bare `Path` reads
/// its binding's recorded value; operators fold over operands resolved the
/// same way, so `K + 1` works when `K` is a named constant. `None` when any
/// operand is not an integer constant or a reference has no recorded value
/// (a `var` binding, a runtime parameter).
#[must_use]
pub(crate) fn const_int_value_resolved(
    expr: &HirExpression,
    values: &handroll::FxHashMap<radix_types::DefId, i128>,
) -> Option<i128> {
    match &expr.kind {
        HirExpressionKind::Path(def_id) => values.get(def_id).copied(),
        HirExpressionKind::Literal(HirLiteral::Int(value)) => i128::from(*value).into(),
        HirExpressionKind::Literal(HirLiteral::BigInt(_)) => const_int_value(expr),
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => {
            const_int_value_resolved(inner, values)?.checked_neg()
        }
        HirExpressionKind::Unary(radix_hir::HirUnOp::BitNot, inner) => {
            Some(!const_int_value_resolved(inner, values)?)
        }
        HirExpressionKind::Binary(op, lhs, rhs) => fold_const_i128(
            *op,
            const_int_value_resolved(lhs, values)?,
            const_int_value_resolved(rhs, values)?,
        ),
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

/// The `i128` fast path of [`fold_const_binary`]: `None` when an operand or
/// the result leaves `i128` (the caller then folds on the bignum).
fn fold_const_i128(op: HirBinOp, a: i128, b: i128) -> Option<i128> {
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

fn fold_const_binary(op: HirBinOp, a: &Magnus, b: &Magnus) -> Option<Magnus> {
    if let (Some(x), Some(y)) = (a.to_i128(), b.to_i128())
        && let Some(folded) = fold_const_i128(op, x, y)
    {
        return Some(Magnus::from_i128(folded));
    }
    let folded = match op {
        HirBinOp::Add => a + b,
        HirBinOp::Sub => a - b,
        HirBinOp::Mul => a.mul_within(b, FOLD_CEILING_BITS).ok()?,
        HirBinOp::Div => a.div_floor(b).ok()?,
        HirBinOp::Mod => a.mod_floor(b).ok()?,
        HirBinOp::BitAnd => a.bit_and(b),
        HirBinOp::BitOr => a.bit_or(b),
        HirBinOp::BitXor => a.bit_xor(b),
        HirBinOp::Shl => a.shl_within(b.to_u64()?, FOLD_CEILING_BITS).ok()?,
        HirBinOp::Shr => a.shr_by(b).ok()?,
        _ => return None,
    };
    (!folded.exceeds_ceiling(FOLD_CEILING_BITS)).then_some(folded)
}

/// How a scalar operand takes part in the range rule.
#[derive(Clone, Copy)]
enum Operand {
    /// Untyped integer constant with its exact value.
    Const(i128),
    /// Untyped integer constant beyond `i128` (only an `inf` can hold it).
    Big,
    /// Trapping integer of a declared width.
    Plain(NumericWidth),
    /// Float, decimal, word, or non-numeric: not range-typed.
    Other,
}

/// The operand of an untyped integer constant expression, at any size.
fn const_operand(expr: &HirExpression) -> Option<Operand> {
    const_int_exact(expr).map(|value| value.to_i128().map_or(Operand::Big, Operand::Const))
}

/// Ruling 8: a `u64` beside a signed typed integer has no lossless join.
fn u64_meets_signed(a: NumericWidth, b: NumericWidth) -> bool {
    (a == NumericWidth::U64 && b.is_signed_int()) || (b == NumericWidth::U64 && a.is_signed_int())
}

/// What an expected type means for a constant-only expression.
enum ConstSlot {
    /// An integer slot: the constant takes its type.
    Integer(TypeId),
    /// A float slot: the constant takes its type after an exactness check.
    Float(TypeId),
    /// A word or decimal slot keeps the pre-range shaping.
    Legacy,
    /// No slot: the constant defaults to `int`.
    Default,
}

impl TypeChecker<'_> {
    /// Declared width of a trapping (plain) integer type; `None` for words,
    /// decimals, floats and non-numerics.
    ///
    /// The unbounded `inf` is an integer width (ruling 32) whose
    /// `integer_range()` is `None`: it has no interval, so every range rule
    /// below answers it before any interval arithmetic (ruling 35: a typed
    /// `inf` operand makes the result `inf`).
    fn plain_int_width(&self, ty: TypeId) -> Option<NumericWidth> {
        let resolved = self.resolve_inferred_type(ty);
        let (_, resolved) = self.types.peel_access_marker(resolved);
        radix_types::numeric_width::numerus_integer_width(self.types, resolved)
    }

    /// The value an immutable integer cell declared as `declared` holds after
    /// `value` is stored into it, or `None` when the binding is not an integer
    /// constant (a float, text, decimal or `inf` cell, or a value that does not
    /// fit a trapping slot).
    ///
    /// WHY: width limits apply at the store and a word policy reduces there,
    /// so `fixum modulus<u8> k ← 256` holds 0, and a named axis must read what
    /// the cell holds rather than the raw initializer.
    pub(crate) fn stored_const_int(&self, declared: Option<TypeId>, value: i128) -> Option<i128> {
        let Some(declared) = declared else {
            return Some(value);
        };
        let mut ty = self.resolve_inferred_type(declared);
        ty = self.types.peel_access_marker(ty).1;
        while let Type::Alias(_, inner) = self.types.get(ty) {
            ty = self.resolve_inferred_type(*inner);
        }
        let within = |(lo, hi): IntRange| (lo..=hi).contains(&value).then_some(value);
        match self.types.get(ty) {
            Type::Primitive(Primitive::Numerus) => {
                within((i128::from(i64::MIN), i128::from(i64::MAX)))
            }
            Type::SizedNumeric(Primitive::Numerus, width) => within(width.integer_range()?),
            Type::ModularWord(width) => {
                // The word is the ring of 2^bits values starting at the
                // range's floor, so signed words reinterpret the residue.
                let (lo, hi) = width.integer_range()?;
                Some(lo + (value - lo).rem_euclid(hi - lo + 1))
            }
            Type::SaturatingWord(width) => {
                let (lo, hi) = width.integer_range()?;
                Some(value.clamp(lo, hi))
            }
            // A ranged cell applies its policy to the range once, at the store.
            Type::RangedInt { policy, lo, hi, .. } => Some(match policy {
                radix_types::IntPolicy::Trapping => within((*lo, *hi))?,
                radix_types::IntPolicy::Wrapping => lo + (value - lo).rem_euclid(hi - lo + 1),
                radix_types::IntPolicy::Saturating => value.clamp(*lo, *hi),
            }),
            _ => None,
        }
    }

    /// Declared width of an integer that takes part in the range rule: a
    /// trapping integer, or a `wrapping` or `saturating` word (which computes
    /// exactly like its width and reduces or clamps only where a value is
    /// stored into a slot).
    fn range_int_width(&self, ty: TypeId) -> Option<NumericWidth> {
        self.plain_int_width(ty).or_else(|| {
            let shape = self.numeric_shape(self.resolve_inferred_type(ty))?;
            (matches!(shape.family, IntFamily::Saturating | IntFamily::Modular)
                && shape.prim == Primitive::Numerus)
                .then_some(shape.width)
        })
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
            return ConstSlot::Float(ty);
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
        // Ruling 34: a constant is untyped and exact, so one beyond every
        // 64-bit integer (`18446744073709551616`, `2 ⇐ 200`) is checked as an
        // `inf` here and judged where it lands (`constant_expression_type`,
        // the store and the range rule), never by its own literal.
        // A (negated) `u64`-sized literal keeps the literal rule's own error.
        let plain_literal = match &expr.kind {
            HirExpressionKind::Literal(HirLiteral::Int(_)) => true,
            HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => {
                matches!(inner.kind, HirExpressionKind::Literal(HirLiteral::Int(_)))
            }
            _ => false,
        };
        if !plain_literal
            && const_int_exact(expr).is_some_and(|value| !Self::fits_container_value(&value))
        {
            let inf = self.inf_type();
            return self.check_expr_with_expected(expr, Some(inf));
        }
        self.check_expr(expr)
    }

    /// Check one operand of a comparison or equality: an integer constant is
    /// untyped and exact (ruling 34), so a constant beyond every 64-bit type
    /// compares by value as an `inf` operand instead of being judged as a
    /// literal; every other operand is checked as usual.
    pub(super) fn check_comparison_operand(&mut self, expr: &mut HirExpression) -> TypeId {
        if !self.current_device_function && is_const_int(expr) {
            return self.check_const_operand(expr);
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
        if self.is_inf_type(resolved) {
            // Ruling 35: an untyped constant beside an `inf` joins by exact
            // value, whatever its size.
            return self.check_expr_with_expected(expr, Some(resolved));
        }
        if resolved == self.error_type
            || (self.range_int_width(resolved).is_some()
                && !self.constant_joins_word_ring(expr, resolved))
            || self.numeric_shape(resolved).is_none()
        {
            return self.check_const_operand(expr);
        }
        let hint = self.numeric_expected_type(resolved);
        self.check_expr_with_expected(expr, Some(hint))
    }

    fn classify_operand(&self, expr: &HirExpression, ty: TypeId) -> Operand {
        if let Some(operand) = const_operand(expr) {
            return operand;
        }
        self.range_int_width(ty)
            .map_or(Operand::Other, Operand::Plain)
    }

    /// Ruling 35: a typed `inf` operand makes the result `inf` for every
    /// integer operator; an untyped constant beside it joins by exact value.
    fn inf_join(&mut self, l: Operand, r: Operand) -> Option<TypeId> {
        let is_inf = |operand: Operand| matches!(operand, Operand::Plain(NumericWidth::Inf));
        (is_inf(l) || is_inf(r)).then(|| self.inf_type())
    }

    pub(super) fn inf_type(&mut self) -> TypeId {
        self.width_type(NumericWidth::Inf)
    }

    /// Ruling 34: an untyped constant beside a bounded operand must fit a
    /// 64-bit integer; one that fits none (a big literal, `2 ⇐ 200`) is
    /// `numerus_literal_out_of_range`, never a value outside its own type.
    /// Reports and returns `true` when a constant does not fit.
    fn reject_unfit_constant(&mut self, sides: [(Operand, &HirExpression); 2]) -> bool {
        for (operand, expr) in sides {
            let fits = match operand {
                Operand::Big => false,
                Operand::Const(value) => range::fits_container((value, value)),
                Operand::Plain(_) | Operand::Other => true,
            };
            if !fits {
                self.numerus_literal_out_of_range(expr.span);
                return true;
            }
        }
        false
    }

    pub(super) fn numerus_literal_out_of_range(&mut self, span: radix_lexer::Span) {
        self.error_issue(
            SemanticErrorKind::TypeMismatch,
            NUMERUS_LITERAL_OUT_OF_RANGE,
            span,
        );
    }

    fn operand_range(operand: Operand) -> Option<IntRange> {
        match operand {
            Operand::Const(value) => Some((value, value)),
            Operand::Plain(width) => width.integer_range(),
            Operand::Big | Operand::Other => None,
        }
    }

    /// Exact interval of a declared width, or of a constant value that takes
    /// part by value in its place.
    fn width_or_const_range(width: NumericWidth, konst: Option<i128>) -> Option<IntRange> {
        konst.map_or_else(|| width.integer_range(), |value| Some((value, value)))
    }

    /// Type of a constant-only expression: the integer slot's type, a float
    /// slot's type after the exactness check, or `int`.
    fn constant_expression_type(
        &mut self,
        value: Option<Magnus>,
        expected: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        match self.const_slot(expected) {
            ConstSlot::Legacy => None,
            ConstSlot::Integer(ty) => Some(ty),
            ConstSlot::Float(ty) => {
                if value.is_some_and(|v| !self.check_float_exact_constant(ty, &v, span)) {
                    return Some(self.error_type);
                }
                Some(ty)
            }
            ConstSlot::Default => {
                // Ruling 34: a lone constant defaults to `int`, so it must
                // fit a 64-bit integer; only an `inf` slot holds more.
                if value.is_some_and(|v| !Self::fits_container_value(&v)) {
                    self.numerus_literal_out_of_range(span);
                    return Some(self.error_type);
                }
                Some(self.numerus_type())
            }
        }
    }

    /// Whether an exact value lies in the single 64-bit range
    /// [-2^63, 2^64 - 1] some 64-bit integer holds (ruling 17).
    pub(super) fn fits_container_value(value: &Magnus) -> bool {
        value
            .to_i128()
            .is_some_and(|v| range::fits_container((v, v)))
    }

    /// Width of a float type, `f64` for the bare `fractus`.
    fn float_slot_width(&self, float_ty: TypeId) -> NumericWidth {
        match self.types.get(self.resolve_inferred_type(float_ty)) {
            Type::SizedNumeric(Primitive::Fractus, width) | Type::PolicyFloat(_, width) => *width,
            _ => NumericWidth::F64,
        }
    }

    /// Ruling 20(1)/(2): an integer constant taking a float type must be
    /// exactly representable there. Reports and returns `false` when it is not.
    pub(super) fn check_float_integer_constant(
        &mut self,
        float_ty: TypeId,
        value: i128,
        span: radix_lexer::Span,
    ) -> bool {
        let width = self.float_slot_width(float_ty);
        let issue = if radix_types::float_overflows(width, value as f64) {
            FLOAT_CONSTANT_OUT_OF_RANGE
        } else if !radix_types::float_holds_integer(width, value) {
            FLOAT_CONSTANT_INEXACT
        } else {
            return true;
        };
        self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
        false
    }

    /// [`Self::check_float_integer_constant`] for a constant of any size: one
    /// beyond `i128` is out of range unless the float width holds it, and
    /// exact only when it is a float's integral value.
    pub(super) fn check_float_exact_constant(
        &mut self,
        float_ty: TypeId,
        value: &Magnus,
        span: radix_lexer::Span,
    ) -> bool {
        if let Some(small) = value.to_i128() {
            return self.check_float_integer_constant(float_ty, small, span);
        }
        let width = self.float_slot_width(float_ty);
        let approx = value.to_f64();
        let issue = if Self::float_constant_overflows(width, approx) {
            FLOAT_CONSTANT_OUT_OF_RANGE
        } else if Magnus::from_f64(approx).ok().as_ref() != Some(value) {
            FLOAT_CONSTANT_INEXACT
        } else {
            return true;
        };
        self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
        false
    }

    /// Ruling 20 on `↦`: an integer constant converted into a float type must
    /// lie inside its finite range (rounding to nearest is the conversion's
    /// job and is not checked). Reports and returns `false` when it does not.
    pub(super) fn check_float_conversion_constant(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        let Some(float_ty) = self.expected_float_slot_type(Some(target)) else {
            return true;
        };
        let Some(value) = const_int_exact(source) else {
            return true;
        };
        let width = self.float_slot_width(float_ty);
        if Self::float_constant_overflows(width, value.to_f64()) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                FLOAT_CONSTANT_OUT_OF_RANGE,
                span,
            );
            return false;
        }
        true
    }

    /// Whether an integer constant's nearest `f64` (infinite beyond `f64`'s own
    /// range) lies outside the finite range of `width`.
    fn float_constant_overflows(width: NumericWidth, approx: f64) -> bool {
        approx.is_infinite() || radix_types::float_overflows(width, approx)
    }

    /// Ruling 20(2): a written float that rounds to infinity in the float
    /// type is a compile error. Reports and returns `false` when it does.
    pub(super) fn check_float_literal_range(
        &mut self,
        float_ty: TypeId,
        value: f64,
        span: radix_lexer::Span,
    ) -> bool {
        let width = self.float_slot_width(float_ty);
        if radix_types::float_overflows(width, value) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                FLOAT_CONSTANT_OUT_OF_RANGE,
                span,
            );
            return false;
        }
        true
    }

    /// Ruling 10: a negative shift count is an error, and a compile error when
    /// the count is a constant. Returns `true` when it reported one.
    pub(super) fn reject_negative_shift_count(
        &mut self,
        op: HirBinOp,
        count: &HirExpression,
    ) -> bool {
        if !matches!(op, HirBinOp::Shl | HirBinOp::Shr)
            || !const_int_exact(count).is_some_and(|n| n.is_negative())
        {
            return false;
        }
        self.error_issue(
            SemanticErrorKind::TypeMismatch,
            SHIFT_COUNT_NEGATIVE,
            count.span,
        );
        true
    }

    /// Ruling 28: a negative `potentia` exponent on an integer receiver (any
    /// policy family) is an error of the negative-shift-count class, and a
    /// compile error when the exponent is a constant. A `fractus` receiver
    /// keeps IEEE `pow`. Returns `true` when it reported one.
    pub(super) fn reject_negative_potentia_exponent(
        &mut self,
        entry: &radix_intrinsics::MethodIntrinsic,
        receiver_ty: TypeId,
        args: &[radix_hir::HirCallArg],
    ) -> bool {
        if entry.faber_name != "potentia"
            || !matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Numeric)
            || !self.is_integer(receiver_ty)
        {
            return false;
        }
        let Some(exponent) = args
            .first()
            .filter(|arg| const_int_exact(&arg.expr).is_some_and(|n| n.is_negative()))
        else {
            return false;
        };
        self.error_issue(
            SemanticErrorKind::TypeMismatch,
            POTENTIA_EXPONENT_NEGATIVE,
            exponent.expr.span,
        );
        true
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
        if self.in_word_ring(lhs, &[lhs_ty, rhs_ty]) {
            return None;
        }
        let (l, r) = self.range_operands(lhs, rhs, lhs_ty, rhs_ty)?;
        // A `saturating` word never meets another integer family, `inf`
        // included (it is a trapping type), so the family check comes first.
        if matches!((l, r), (Operand::Plain(_), Operand::Plain(_)))
            && !self.word_operands_agree(lhs, rhs, span)
        {
            return Some(self.error_type);
        }
        if let Some(inf) = self.inf_join(l, r) {
            return Some(inf);
        }
        if Self::both_constant(l, r) {
            return self.constant_expression_type(
                Self::fold_constants(op, lhs, rhs),
                expected,
                span,
            );
        }
        if self.reject_unfit_constant([(l, lhs), (r, rhs)]) {
            return Some(self.error_type);
        }
        if let (Operand::Plain(a), Operand::Plain(b)) = (l, r)
            && u64_meets_signed(a, b)
        {
            self.invalid_operand_types(U64_SIGNED_ARITHMETIC_REQUIRES_CONVERSION, span);
            return Some(self.error_type);
        }
        let (a, b) = (Self::operand_range(l)?, Self::operand_range(r)?);
        let result = Self::scalar_binary_range(op, a, b)?;
        Some(self.width_type(range::smallest_width(result)))
    }

    fn both_constant(l: Operand, r: Operand) -> bool {
        matches!(l, Operand::Const(_) | Operand::Big)
            && matches!(r, Operand::Const(_) | Operand::Big)
    }

    /// The exact fold of a constant-only binary expression, at any size.
    fn fold_constants(op: HirBinOp, lhs: &HirExpression, rhs: &HirExpression) -> Option<Magnus> {
        fold_const_binary(op, &const_int_exact(lhs)?, &const_int_exact(rhs)?)
    }

    /// Exact range of `+ - * / %` over operand ranges (ruling 19a).
    fn scalar_binary_range(op: HirBinOp, a: IntRange, b: IntRange) -> Option<IntRange> {
        Some(match op {
            HirBinOp::Add => range::add(a, b),
            HirBinOp::Sub => range::sub(a, b),
            HirBinOp::Mul => range::mul(a, b),
            HirBinOp::Div => range::floor_div(a, b),
            HirBinOp::Mod => range::floor_rem(a, b),
            HirBinOp::Shl => range::shl(a, b),
            _ => return None,
        })
    }

    /// Range-typed result of the integer methods that have no operator:
    /// `absolutum` (`|x|`) and `potentia` (`x ^ n`).
    ///
    /// Only trapping integer receivers are range typed; every other receiver
    /// (float, decimal, word) keeps the registry's `SameAsReceiver` result.
    /// `potentia` with a constant exponent gets the exact interval of `x^n`;
    /// with a variable exponent it sits at the 64-bit cap (ruling 13), and
    /// the store check of ruling 17 applies where the value is used.
    pub(super) fn integer_method_range_type(
        &mut self,
        entry: &radix_intrinsics::MethodIntrinsic,
        receiver_ty: TypeId,
        args: &[radix_hir::HirCallArg],
        result: TypeId,
    ) -> TypeId {
        if entry.faber_name == "absolutum"
            && matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Tensor)
            && !self.current_device_function
        {
            return self.tensor_abs_range_type(receiver_ty, result);
        }
        if !matches!(entry.faber_name, "absolutum" | "potentia")
            || !matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Numeric)
            || self.current_device_function
        {
            return result;
        }
        let carrier = self.value_carrier_type(receiver_ty);
        let Some(base) = self
            .plain_int_width(carrier)
            .and_then(NumericWidth::integer_range)
        else {
            return result;
        };
        let range = if entry.faber_name == "absolutum" {
            range::abs(base)
        } else {
            match args.first().and_then(|arg| const_int_value(&arg.expr)) {
                Some(exponent) if (0..=range::MAX_CONST_EXPONENT).contains(&exponent) => {
                    range::pow_const(base, exponent)
                }
                _ => range::pow_unbounded(base),
            }
        };
        self.width_type(range::smallest_width(range))
    }

    /// Tensor `absolutum` follows the scalar `absolutum` typing (K5, rule 15:
    /// lifting is "the scalar operation per element"). A trapping integer
    /// element is range typed to the smallest width holding `|x|` (an `i8`
    /// tensor gives a `u8` tensor); a float, decimal or word element keeps
    /// the receiver's own type. Device functions keep the element type.
    fn tensor_abs_range_type(&mut self, receiver_ty: TypeId, result: TypeId) -> TypeId {
        let carrier = self.value_carrier_type(receiver_ty);
        let (_, mut payload) = self.types.peel_access_marker(carrier);
        while let Type::Alias(_, inner) = self.types.get(payload) {
            payload = *inner;
        }
        let Type::Tensor(element, shape) = self.types.get(payload) else {
            return result;
        };
        let (element, shape) = (*element, *shape);
        let Some(base) = self
            .plain_int_width(element)
            .and_then(NumericWidth::integer_range)
        else {
            return result;
        };
        let width = range::smallest_width(range::abs(base));
        let element = self.width_type(width);
        self.types.tensor_with_shape(element, shape)
    }

    /// Whether an integer expression sits at the 64-bit cap with a value that
    /// can reach past `i64::MAX` (`i64 + i64`, `-i64`, `i64 ⇐ n`, a `summa`,
    /// or any of them nested or yielded by a conditional). Its type is `i64`
    /// but its value is not bounded by it, so a store into an `i64` slot is a
    /// real check (ruling 17b) and cannot be omitted.
    pub(super) fn exceeds_i64(&self, expr: &HirExpression, ty: TypeId) -> bool {
        if self.current_device_function || is_const_int(expr) {
            return false;
        }
        self.value_range(expr, Some(ty))
            .is_some_and(|(_, hi)| hi > i128::from(i64::MAX))
    }

    /// The exact interval an integer expression can evaluate to. At the 64-bit
    /// cap the expression's *type* is `i64` while its exact value is not
    /// bounded by it (ruling 13), so the interval is computed from the value
    /// ranges of the operands, recursively: only a leaf (a binding, field,
    /// element, call result or converted value, each already checked at the
    /// store that produced it) is bounded by its declared type. `None` when
    /// the expression is not range-typed integer arithmetic.
    pub(super) fn value_range(&self, expr: &HirExpression, ty: Option<TypeId>) -> Option<IntRange> {
        if let Some(value) = const_int_value(expr) {
            return Some((value, value));
        }
        let ty = ty.or(expr.ty)?;
        let leaf = || Self::operand_range(self.classify_operand(expr, ty));
        match &expr.kind {
            HirExpressionKind::Reduction { .. } | HirExpressionKind::Scan { .. } => {
                Some(self.cap_or_declared(ty))
            }
            HirExpressionKind::Binary(op, lhs, rhs) => Self::binary_value_range(
                *op,
                self.value_range(lhs, None)?,
                self.value_range(rhs, None)?,
            ),
            HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, operand) => {
                self.value_range(operand, None).map(range::neg)
            }
            HirExpressionKind::Unary(radix_hir::HirUnOp::BitNot, operand) => {
                self.value_range(operand, None).map(range::bit_not)
            }
            HirExpressionKind::MethodCall(receiver, name, _, args, _) => self
                .method_value_range(receiver, *name, args, ty)
                .or_else(leaf),
            HirExpressionKind::Block(block) => self.block_value_range(block).or_else(leaf),
            HirExpressionKind::Si {
                then_block,
                else_block,
                ..
            } => {
                let branches = std::iter::once(then_block).chain(else_block.as_ref());
                Self::hull_of(branches.map(|block| self.block_value_range(block))).or_else(leaf)
            }
            HirExpressionKind::Discerne { arms, .. } => {
                Self::hull_of(arms.iter().map(|arm| self.value_range(&arm.body, None)))
                    .or_else(leaf)
            }
            _ => leaf(),
        }
    }

    /// The hull of the branch intervals; `None` as soon as one branch has no
    /// integer interval (the caller then judges the whole by its declared type).
    fn hull_of(branches: impl Iterator<Item = Option<IntRange>>) -> Option<IntRange> {
        let mut hull: Option<IntRange> = None;
        for branch in branches {
            let branch = branch?;
            hull = Some(hull.map_or(branch, |seen| range::hull(seen, branch)));
        }
        hull
    }

    fn block_value_range(&self, block: &radix_hir::HirBlock) -> Option<IntRange> {
        self.value_range(block.expr.as_deref()?, None)
    }

    /// An expression at the 64-bit cap whose exact bound is not known (a
    /// `summa`, or `potentia` with a variable exponent): a signed `i64` reaches
    /// anywhere in the single 64-bit range; any other width keeps its own.
    fn cap_or_declared(&self, ty: TypeId) -> IntRange {
        let anywhere = (i128::from(i64::MIN), i128::from(u64::MAX));
        match self.plain_int_width(ty) {
            Some(NumericWidth::I64) | None => anywhere,
            Some(width) => width.integer_range().unwrap_or(anywhere),
        }
    }

    /// Interval of a binary operator over operand intervals. `& | ^ >>` never
    /// leave the 64-bit range when their operands sit inside it; an operand
    /// beyond it lets the result go anywhere in the single 64-bit range.
    pub(super) fn binary_value_range(op: HirBinOp, a: IntRange, b: IntRange) -> Option<IntRange> {
        if let Some(result) = Self::scalar_binary_range(op, a, b) {
            return Some(result);
        }
        let signed = (i128::from(i64::MIN), i128::from(i64::MAX));
        let within = |r: IntRange| signed.0 <= r.0 && r.1 <= signed.1;
        let anywhere = (i128::from(i64::MIN), i128::from(u64::MAX));
        match op {
            HirBinOp::BitAnd => Some(range::bit_and(a, b)),
            HirBinOp::BitOr | HirBinOp::BitXor => Some(if within(a) && within(b) {
                signed
            } else {
                anywhere
            }),
            HirBinOp::Shr => Some((a.0.min(0), a.1.max(0))),
            _ => None,
        }
    }

    /// Interval of the receiver methods whose result is not bounded by their
    /// declared type: `summa` over a list or tensor (ruling 21) and `potentia`.
    fn method_value_range(
        &self,
        receiver: &HirExpression,
        name: Symbol,
        args: &[radix_hir::HirCallArg],
        ty: TypeId,
    ) -> Option<IntRange> {
        let name = self.interner?.resolve(name);
        if name == "summa" && args.is_empty() {
            let peeled = self
                .types
                .peel_access_marker(self.resolve_inferred_type(receiver.ty?))
                .1;
            return matches!(self.types.get(peeled), Type::Array(_) | Type::Tensor(..))
                .then(|| self.cap_or_declared(ty));
        }
        if name != "potentia" || args.len() != 1 || self.plain_int_width(ty)? != NumericWidth::I64 {
            return None;
        }
        let base = self.value_range(receiver, None)?;
        Some(match const_int_value(&args[0].expr) {
            Some(exponent) if (0..=range::MAX_CONST_EXPONENT).contains(&exponent) => {
                range::pow_const(base, exponent)
            }
            _ => range::pow_unbounded(base),
        })
    }

    /// Result type of `÷` (d11-6 rulings 15 and 23): an integer `÷` yields the
    /// smallest float that holds every value of both operand types exactly,
    /// never below `f32` — `f32` when the widest integer operand is 8 or 16
    /// bits, `f64` for 32 or 64. An untyped constant joins by value beside a
    /// typed operand and defaults to `int` alone. Float and decimal operands
    /// follow `/`'s family rules, and an integer beside a float or decimal is
    /// rejected like `/` (ruling 16).
    pub(super) fn true_division_type(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        if let Some((l, r)) = self.range_operands(lhs, rhs, lhs_ty, rhs_ty) {
            return self.division_float_type(&[l, r]);
        }
        let joined = self.numeric_arithmetic_bin(lhs_ty, rhs_ty, span);
        let resolved = self.resolve_inferred_type(joined);
        let integer = self
            .numeric_shape(resolved)
            .is_some_and(|shape| shape.prim == Primitive::Numerus && !shape.width.is_decimal());
        if !integer {
            return joined;
        }
        // Words and device functions: the declared widths decide; a constant
        // operand joins by value.
        let operands: Vec<Operand> = [(lhs, lhs_ty), (rhs, rhs_ty)]
            .iter()
            .map(|(expr, ty)| match const_operand(expr) {
                Some(operand) => operand,
                None => self
                    .numeric_shape(self.resolve_inferred_type(*ty))
                    .map_or(Operand::Plain(NumericWidth::I64), |shape| {
                        Operand::Plain(shape.width)
                    }),
            })
            .collect();
        self.division_float_type(&operands)
    }

    /// The float an integer `÷` produces from its operands (ruling 23).
    fn division_float_type(&mut self, operands: &[Operand]) -> TypeId {
        let narrow = |width: NumericWidth| {
            matches!(
                width,
                NumericWidth::I8 | NumericWidth::U8 | NumericWidth::I16 | NumericWidth::U16
            )
        };
        let typed: Vec<NumericWidth> = operands
            .iter()
            .filter_map(|operand| match operand {
                Operand::Plain(width) => Some(*width),
                _ => None,
            })
            .collect();
        // Constants join by value: they widen the result only past 16 bits.
        let constants_fit = operands.iter().all(|operand| match operand {
            Operand::Const(value) => narrow(range::smallest_width((*value, *value))),
            Operand::Big => false,
            _ => true,
        });
        if !typed.is_empty() && typed.iter().all(|width| narrow(*width)) && constants_fit {
            self.types
                .sized_numeric(Primitive::Fractus, NumericWidth::F32)
        } else {
            self.fractus_type()
        }
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
        if self.in_word_ring(lhs, &[lhs_ty, rhs_ty]) {
            return None;
        }
        let (l, r) = self.range_operands(lhs, rhs, lhs_ty, rhs_ty)?;
        if matches!((l, r), (Operand::Plain(_), Operand::Plain(_)))
            && !matches!(op, HirBinOp::Shl | HirBinOp::Shr)
            && !self.word_operands_agree(lhs, rhs, span)
        {
            return Some(self.error_type);
        }
        if let Some(inf) = self.inf_join(l, r) {
            return Some(inf);
        }
        if Self::both_constant(l, r) {
            return self.constant_expression_type(
                Self::fold_constants(op, lhs, rhs),
                expected,
                span,
            );
        }
        if self.reject_unfit_constant([(l, lhs), (r, rhs)]) {
            return Some(self.error_type);
        }
        let (a, b) = (Self::operand_range(l)?, Self::operand_range(r)?);
        if let (Operand::Plain(x), Operand::Plain(y)) = (l, r)
            && matches!(op, HirBinOp::BitAnd | HirBinOp::BitOr | HirBinOp::BitXor)
            && u64_meets_signed(x, y)
        {
            self.invalid_operand_types(U64_SIGNED_ARITHMETIC_REQUIRES_CONVERSION, span);
            return Some(self.error_type);
        }
        let result = match op {
            HirBinOp::BitAnd => range::bit_and(a, b),
            HirBinOp::BitOr | HirBinOp::BitXor => {
                let joined = range::hull(a, b);
                if joined.0 < 0 && joined.1 > i128::from(i64::MAX) {
                    self.invalid_operand_types(U64_SIGNED_ARITHMETIC_REQUIRES_CONVERSION, span);
                    return Some(self.error_type);
                }
                joined
            }
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
        if self.current_device_function
            || is_const_int(operand)
            || self.in_word_ring(operand, &[operand_ty])
        {
            return None;
        }
        let width = self.range_int_width(operand_ty)?;
        if width == NumericWidth::Inf {
            return match op {
                radix_hir::HirUnOp::Neg | radix_hir::HirUnOp::BitNot => Some(self.inf_type()),
                radix_hir::HirUnOp::Not => None,
            };
        }
        let a = width.integer_range()?;
        let result = match op {
            radix_hir::HirUnOp::Neg => range::neg(a),
            radix_hir::HirUnOp::BitNot => range::bit_not(a),
            radix_hir::HirUnOp::Not => return None,
        };
        Some(self.width_type(range::smallest_width(result)))
    }

    /// Branch/element join of two trapping integer types: the smallest type
    /// holding both ranges (ruling 21). A range that no single 64-bit type
    /// holds (`u64` with a signed type) is the ruling-8 error, not a value
    /// outside its own type.
    pub(super) fn integer_hull_type(
        &mut self,
        a: TypeId,
        b: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let (wa, wb) = (self.plain_int_width(a)?, self.plain_int_width(b)?);
        if wa == NumericWidth::Inf || wb == NumericWidth::Inf {
            return Some(self.inf_type());
        }
        let joined = range::hull(wa.integer_range()?, wb.integer_range()?);
        Some(self.join_range_type(joined, span))
    }

    /// Element join of two integer operands of an elementwise tensor operator
    /// (F9, K4): the smallest type holding both operand ranges, an untyped
    /// constant taking part by value. The operator stores each result per
    /// element, so the join is the element type, not the widened range type
    /// of a scalar expression. `None` unless both sides are trapping integers.
    pub(super) fn integer_operand_join(
        &mut self,
        a: (TypeId, Option<i128>),
        b: (TypeId, Option<i128>),
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if self.current_device_function {
            return None;
        }
        // Both sides must be integers even where a constant supplies the range.
        let (wa, wb) = (self.plain_int_width(a.0)?, self.plain_int_width(b.0)?);
        if wa == NumericWidth::Inf || wb == NumericWidth::Inf {
            return Some(self.inf_type());
        }
        let joined = range::hull(
            Self::width_or_const_range(wa, a.1)?,
            Self::width_or_const_range(wb, b.1)?,
        );
        Some(self.join_range_type(joined, span))
    }

    /// Element type of a lifted integer arithmetic operator (operator ruling
    /// 2026-10-15, need): the scalar range rule for `op` over the
    /// operand ranges, an untyped constant taking part by value. `None` unless
    /// both sides are trapping integers — a word, float, decimal, `inf`,
    /// inference hole or device function keeps the caller's F9 join.
    pub(super) fn integer_arith_element_type(
        &mut self,
        op: HirBinOp,
        l: (TypeId, Option<i128>),
        r: (TypeId, Option<i128>),
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if self.current_device_function {
            return None;
        }
        let (wa, wb) = (self.plain_int_width(l.0)?, self.plain_int_width(r.0)?);
        if wa == NumericWidth::Inf || wb == NumericWidth::Inf {
            return Some(self.inf_type());
        }
        // Ruling 8: a `u64` beside a signed typed operand has no lossless join.
        if l.1.is_none() && r.1.is_none() && u64_meets_signed(wa, wb) {
            self.invalid_operand_types(U64_SIGNED_ARITHMETIC_REQUIRES_CONVERSION, span);
            return Some(self.error_type);
        }
        let (a, b) = (
            Self::width_or_const_range(wa, l.1)?,
            Self::width_or_const_range(wb, r.1)?,
        );
        let result = Self::scalar_binary_range(op, a, b)?;
        Some(self.width_type(range::smallest_width(result)))
    }

    /// The type of a joined range, or the ruling-8 error when it spans both
    /// negative values and values above `i64::MAX`.
    fn join_range_type(&mut self, joined: IntRange, span: radix_lexer::Span) -> TypeId {
        if joined.0 < 0 && joined.1 > i128::from(i64::MAX) {
            self.invalid_operand_types(U64_SIGNED_ARITHMETIC_REQUIRES_CONVERSION, span);
            return self.error_type;
        }
        self.width_type(range::smallest_width(joined))
    }

    /// Exact range of an untyped integer-constant branch or element.
    pub(super) fn const_join_range(expr: &HirExpression) -> Option<IntRange> {
        const_int_value(expr).map(|value| (value, value))
    }

    /// Join of two branch or element types where either may be an untyped
    /// constant of a known range (ruling 8 extended: constants are exempt).
    ///
    /// A constant beside a typed integer joins by value, so `c ✓ x ✗ 0` with
    /// `x: u64` stays `u64`; only a range no 64-bit type holds is an error.
    /// Anything else is the plain [`Self::common_type`].
    pub(super) fn common_type_with_consts(
        &mut self,
        a: (TypeId, Option<IntRange>),
        b: (TypeId, Option<IntRange>),
        span: radix_lexer::Span,
    ) -> TypeId {
        let (typed, konst) = match (a.1, b.1) {
            (None, Some(range)) => (a.0, range),
            (Some(range), None) => (b.0, range),
            _ => return self.common_type(a.0, b.0, span),
        };
        let Some(width) = self
            .plain_int_width(typed)
            .filter(|_| !self.current_device_function)
        else {
            return self.common_type(a.0, b.0, span);
        };
        if width == NumericWidth::Inf {
            // Ruling 35: a constant beside an `inf` joins by exact value.
            return self.inf_type();
        }
        if width != NumericWidth::U64 && konst.1 <= i128::from(i64::MAX) {
            return self.common_type(a.0, b.0, span);
        }
        let Some(typed_range) = width.integer_range() else {
            return self.common_type(a.0, b.0, span);
        };
        let joined = range::hull(typed_range, konst);
        self.join_range_type(joined, span)
    }

    /// Join of the two endpoints of an `intervallum` (ruling 12): an untyped
    /// integer constant joins the other endpoint by value, and two trapping
    /// integers of different widths join like any binary pair. `None` when the
    /// pair is not a trapping-integer pair or is already two constants, so the
    /// exact-type rule still applies (int against float stays an error).
    ///
    /// Ruling 12 joins kernel endpoints by the same rule as host endpoints:
    /// an integer literal joined with an `i32`/`u32` endpoint is an `i32`/`u32`
    /// in both places, so a kernel range over `0‥n` takes the joined width.
    pub(super) fn join_intervallum_bounds(
        &mut self,
        start: (&HirExpression, TypeId),
        end: (&HirExpression, TypeId),
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let side = |this: &Self, (expr, ty): (&HirExpression, TypeId)| {
            let width = this.plain_int_width(ty)?;
            Some((width, const_int_value(expr)))
        };
        let (start_width, start_const) = side(self, start)?;
        let (end_width, end_const) = side(self, end)?;
        if start_const.is_none() || end_const.is_none() {
            // Ruling 35/43: an `inf` endpoint makes the binder an `inf`.
            if start_width == NumericWidth::Inf || end_width == NumericWidth::Inf {
                return Some(self.inf_type());
            }
        }
        if start_const.is_some() && end_const.is_some() {
            return None;
        }
        let joined = range::hull(
            Self::width_or_const_range(start_width, start_const)?,
            Self::width_or_const_range(end_width, end_const)?,
        );
        Some(self.join_range_type(joined, span))
    }

    /// Type of an integer total over reduction terms — a `summa` or a
    /// `factum`: the element count is unbounded, so the total sits at the
    /// 64-bit cap and keeps the term range's sign (ruling 21). A `wrapping`
    /// or `saturating` word term totals exactly like its width: the policy
    /// reduces or clamps only where the total is stored into a slot, so the
    /// total itself is a 64-bit-cap integer. Non-integer terms keep their own
    /// type.
    pub(super) fn summa_result_type(&mut self, elem: TypeId) -> TypeId {
        if self.current_device_function {
            return elem;
        }
        let Some((lo, _)) = self
            .range_int_width(elem)
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
        if self.current_device_function || is_const_int(operand) {
            return expected;
        }
        // A non-constant operand keeps its own type, so a plain integer slot
        // is not handed to it as an expected type: `¬u` (u: `u64`) into an
        // `int` slot is a checked narrowing of the range-typed result, not a
        // mismatch of the operand (ruling 19a).
        expected.filter(|ty| {
            let resolved = self.resolve_inferred_type(*ty);
            !self.is_infer(resolved) && self.range_int_width(resolved).is_none()
        })
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
            // An integer slot is not a hint for the branches: each is typed on
            // its own, the join is the range rule's, and the store into the
            // slot narrows the joined value (rulings 17b, 21).
            (!self.is_infer(resolved) || self.infer_union_var_of(resolved).is_some())
                && self.store_int_slot(resolved).is_none()
        })
    }

    /// Strict equality compares exact types, but an untyped integer constant
    /// beside an integer operand takes part by value (rulings 12, 22):
    /// `x % 2 ≡ 0` holds for every width of `x % 2`, and `a ≡ 300` with
    /// `a: u8` compares exactly and is simply false. A `wrapping` or
    /// `saturating` word is a policy cell of the same width: the constant
    /// adopts the cell's type exactly as it does beside the bare width
    /// (operator ruling 2026-10-02, need).
    pub(super) fn equality_constant_exempt(
        &self,
        lhs: &HirExpression,
        lhs_ty: TypeId,
        rhs: &HirExpression,
        rhs_ty: TypeId,
    ) -> bool {
        let other = match (is_const_int(lhs), is_const_int(rhs)) {
            (true, false) => rhs_ty,
            (false, true) => lhs_ty,
            _ => return false,
        };
        self.plain_int_width(other).is_some()
            || self
                .numeric_shape(self.resolve_inferred_type(other))
                .is_some_and(|shape| {
                    shape.prim == Primitive::Numerus
                        && !shape.width.is_decimal()
                        && matches!(shape.family, IntFamily::Modular | IntFamily::Saturating)
                })
    }
}
