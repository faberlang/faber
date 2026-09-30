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
const FLOAT_CONSTANT_OUT_OF_RANGE: &str = "float_constant_out_of_range";
const SHIFT_COUNT_NEGATIVE: &str = "shift_count_negative";

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
    Float(TypeId),
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

    /// Declared width of an integer that takes part in the range rule: a
    /// trapping integer, or a `saturating` word (which computes exactly like
    /// its width and clamps only where a value is stored into a slot).
    fn range_int_width(&self, ty: TypeId) -> Option<NumericWidth> {
        self.plain_int_width(ty).or_else(|| {
            let shape = self.numeric_shape(self.resolve_inferred_type(ty))?;
            (shape.family == IntFamily::Saturating && shape.prim == Primitive::Numerus)
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
            || self.range_int_width(resolved).is_some()
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
        self.range_int_width(ty)
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
            ConstSlot::Float(ty) => {
                if value.is_some_and(|v| !self.check_float_integer_constant(ty, v, span)) {
                    return Some(self.error_type);
                }
                Some(ty)
            }
            ConstSlot::Default => Some(self.numerus_type()),
        }
    }

    /// Width of a float type, `f64` for the bare `fractus`.
    fn float_slot_width(&self, float_ty: TypeId) -> NumericWidth {
        match self.types.get(self.resolve_inferred_type(float_ty)) {
            Type::SizedNumeric(Primitive::Fractus, width) => *width,
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
            || !const_int_value(count).is_some_and(|n| n < 0)
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
        if matches!((l, r), (Operand::Plain(_), Operand::Plain(_)))
            && !self.saturating_operands_agree(lhs, rhs, span)
        {
            return Some(self.error_type);
        }
        if let (Operand::Plain(a), Operand::Plain(b)) = (l, r)
            && ((a == NumericWidth::U64 && b.is_signed_int())
                || (b == NumericWidth::U64 && a.is_signed_int()))
        {
            self.invalid_operand_types(U64_SIGNED_ARITHMETIC_REQUIRES_CONVERSION, span);
            return Some(self.error_type);
        }
        let (a, b) = (Self::operand_range(l)?, Self::operand_range(r)?);
        let result = Self::scalar_binary_range(op, a, b)?;
        Some(self.width_type(range::smallest_width(result)))
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
        if self.current_device_function || const_int_value(expr).is_some() {
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
    fn value_range(&self, expr: &HirExpression, ty: Option<TypeId>) -> Option<IntRange> {
        if let Some(value) = const_int_value(expr) {
            return Some((value, value));
        }
        let ty = ty.or(expr.ty)?;
        let leaf = || Self::operand_range(self.classify_operand(expr, ty));
        match &expr.kind {
            HirExpressionKind::Summa { .. } => Some(self.cap_or_declared(ty)),
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
                self.hull_of(branches.map(|block| self.block_value_range(block)))
                    .or_else(leaf)
            }
            HirExpressionKind::Discerne { arms, .. } => self
                .hull_of(arms.iter().map(|arm| self.value_range(&arm.body, None)))
                .or_else(leaf),
            _ => leaf(),
        }
    }

    /// The hull of the branch intervals; `None` as soon as one branch has no
    /// integer interval (the caller then judges the whole by its declared type).
    fn hull_of(&self, branches: impl Iterator<Item = Option<IntRange>>) -> Option<IntRange> {
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
    fn binary_value_range(op: HirBinOp, a: IntRange, b: IntRange) -> Option<IntRange> {
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
            .map(|(expr, ty)| match const_int_value(expr) {
                Some(value) => Operand::Const(value),
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
        let (l, r) = self.range_operands(lhs, rhs, lhs_ty, rhs_ty)?;
        if let (Operand::Const(a), Operand::Const(b)) = (l, r) {
            return self.constant_expression_type(fold_const_binary(op, a, b), expected, span);
        }
        if matches!((l, r), (Operand::Plain(_), Operand::Plain(_)))
            && !matches!(op, HirBinOp::Shl | HirBinOp::Shr)
            && !self.saturating_operands_agree(lhs, rhs, span)
        {
            return Some(self.error_type);
        }
        let (a, b) = (Self::operand_range(l)?, Self::operand_range(r)?);
        if let (Operand::Plain(x), Operand::Plain(y)) = (l, r)
            && matches!(op, HirBinOp::BitAnd | HirBinOp::BitOr | HirBinOp::BitXor)
            && ((x == NumericWidth::U64 && y.is_signed_int())
                || (y == NumericWidth::U64 && x.is_signed_int()))
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
        if self.current_device_function || const_int_value(operand).is_some() {
            return None;
        }
        let width = self.range_int_width(operand_ty)?;
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
        let joined = range::hull(wa.integer_range()?, wb.integer_range()?);
        Some(self.join_range_type(joined, span))
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
    pub(super) fn join_intervallum_bounds(
        &mut self,
        start: (&HirExpression, TypeId),
        end: (&HirExpression, TypeId),
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if self.current_device_function {
            return None;
        }
        let side = |this: &Self, (expr, ty): (&HirExpression, TypeId)| {
            let width = this.plain_int_width(ty)?;
            Some((width, const_int_value(expr)))
        };
        let (start_width, start_const) = side(self, start)?;
        let (end_width, end_const) = side(self, end)?;
        let range_of = |width: NumericWidth, konst: Option<i128>| {
            konst.map_or_else(|| width.integer_range(), |value| Some((value, value)))
        };
        if start_const.is_some() && end_const.is_some() {
            return None;
        }
        let joined = range::hull(
            range_of(start_width, start_const)?,
            range_of(end_width, end_const)?,
        );
        Some(self.join_range_type(joined, span))
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
    /// beside a trapping-integer operand takes part by value (rulings 12, 22):
    /// `x % 2 ≡ 0` holds for every width of `x % 2`, and `a ≡ 300` with
    /// `a: u8` compares exactly and is simply false.
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
        let other = match (const_int_value(lhs), const_int_value(rhs)) {
            (Some(_), None) => rhs_ty,
            (None, Some(_)) => lhs_ty,
            _ => return false,
        };
        self.plain_int_width(other).is_some()
    }
}
