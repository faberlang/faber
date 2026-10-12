//! Shared decimal-carrier math for codegen tiers (decimal-widths D5).
//!
//! Decimal widths ride the tier's signed-integer carrier (`d64` =
//! `i64`×10⁻⁸) with round-half-even rescale. These pure helpers keep
//! every emit tier on the same carrier math as the MIR runner without
//! re-importing runner internals.

use radix_hir::{DefId, HirBinOp, HirExpression, HirExpressionKind, HirLiteral, HirUnOp};
use radix_lexer::Symbol;
use radix_types::{NumericWidth, Primitive, Type, TypeId, TypeTable};

#[cfg(test)]
#[path = "decimal_test.rs"]
mod tests;

/// Decimal scale as an integer factor (`d64` = 10^8).
#[must_use]
pub fn decimal_scale_factor(width: NumericWidth) -> Option<i128> {
    width
        .decimal_scale()
        .and_then(|digits| 10_i128.checked_pow(digits))
}

/// Carrier integer type spelling for a decimal width.
///
/// d64 rides `i64`; other widths return `None`.
#[must_use]
pub fn decimal_carrier_rust_type(width: NumericWidth) -> Option<&'static str> {
    match width {
        NumericWidth::D64 => Some("i64"),
        _ => None,
    }
}

/// The decimal width of a type id, when the type is a sized decimal numeric.
fn decimal_width_of_type(ty: TypeId, types: &TypeTable) -> Option<NumericWidth> {
    match types.get(ty) {
        Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => Some(*width),
        _ => None,
    }
}

/// The decimal width of an expression, if its HIR type is decimal.
#[must_use]
pub fn decimal_expr_width(expr: &HirExpression, types: &TypeTable) -> Option<NumericWidth> {
    expr.ty.and_then(|ty| decimal_width_of_type(ty, types))
}

/// The decimal width of an arithmetic operation over decimal operands.
#[must_use]
pub fn decimal_binop_width(
    op: HirBinOp,
    lhs: &HirExpression,
    rhs: &HirExpression,
    types: &TypeTable,
) -> Option<NumericWidth> {
    if !matches!(
        op,
        HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul | HirBinOp::Div | HirBinOp::Mod
    ) {
        return None;
    }
    let lhs_decimal = decimal_expr_width(lhs, types);
    let rhs_decimal = decimal_expr_width(rhs, types);
    lhs_decimal.or(rhs_decimal)
}

/// The decimal literal payload and width, if `expr` is a decimal literal.
#[must_use]
pub fn decimal_literal_payload(
    expr: &HirExpression,
    types: &TypeTable,
) -> Option<(Symbol, NumericWidth)> {
    let HirExpressionKind::Literal(HirLiteral::FloatText { text, .. }) = &expr.kind else {
        return None;
    };
    let width = expr.ty.and_then(|ty| decimal_width_of_type(ty, types))?;
    Some((*text, width))
}

/// Resolve a decimal source width, recovering a path binding's declared type.
#[must_use]
pub fn decimal_source_width<F>(
    expr: &HirExpression,
    types: &TypeTable,
    binding_type: F,
) -> Option<NumericWidth>
where
    F: Fn(DefId) -> Option<TypeId>,
{
    let ty = if let HirExpressionKind::Path(def_id) = &expr.kind {
        binding_type(*def_id).or(expr.ty)
    } else {
        expr.ty
    }?;
    decimal_width_of_type(ty, types)
}

/// Pure round-half-even quotient for values whose intermediate arithmetic has
/// already been checked. Integer division truncates toward zero, so the
/// remainder decides whether the quotient moves away from zero. A midpoint
/// moves only when the truncated quotient is odd.
#[must_use]
pub fn round_half_even_quotient(numerator: i128, denominator: i128) -> Option<i128> {
    if denominator == 0 {
        return None;
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder == 0 {
        return Some(quotient);
    }

    let remainder_abs = remainder.unsigned_abs();
    let denominator_abs = denominator.unsigned_abs();
    let twice_remainder = remainder_abs.checked_mul(2)?;
    let rounds = twice_remainder > denominator_abs
        || (twice_remainder == denominator_abs && quotient % 2 != 0);
    if !rounds {
        return Some(quotient);
    }

    let step = if (numerator < 0) == (denominator < 0) {
        1
    } else {
        -1
    };
    quotient.checked_add(step)
}

/// Convert decimal digit text into the scaled carrier for `width`, mirroring
/// the MIR runner's `decimal_text_to_carrier` exactly (round-half-even when the
/// text has more fractional digits than the width scale).
///
/// Accepts `4.2`, `1e-3`, `42`, `-0.5`; returns `None` for malformed text or a
/// carrier outside the width's signed range.
#[must_use]
pub fn decimal_text_to_carrier(text: &str, width: NumericWidth) -> Option<i64> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mantissa, exponent) = radix_types::numeric_width::decimal_text_to_scaled(digits)?;
    let mantissa = if negative { -mantissa } else { mantissa };
    let scale_digits = width.decimal_scale()?;
    let shift = exponent.checked_add(i32::try_from(scale_digits).ok()?)?;
    let carrier = if shift >= 0 {
        let factor = 10_i128.checked_pow(u32::try_from(shift).ok()?)?;
        mantissa.checked_mul(factor)?
    } else {
        let power = shift.checked_neg()?;
        let Some(divisor) = 10_i128.checked_pow(u32::try_from(power).ok()?) else {
            // The parsed mantissa fits i128, so a decimal denominator beyond
            // i128 cannot produce a non-zero rounded carrier.
            return Some(0);
        };
        round_half_even_quotient(mantissa, divisor)?
    };
    carrier_in_width(carrier, width)
}

/// Whether an i128 carrier fits the signed range of `width`.
#[must_use]
pub fn carrier_in_width(carrier: i128, width: NumericWidth) -> Option<i64> {
    let (lo, hi) = match width {
        NumericWidth::D64 => (i128::from(i64::MIN), i128::from(i64::MAX)),
        _ => return None,
    };
    if (lo..=hi).contains(&carrier) {
        i64::try_from(carrier).ok()
    } else {
        None
    }
}

/// Fraction digits of a stored `d64` (the scale every store node rounds to).
pub const STORED_SCALE: u32 = 8;

/// How an emit tier reads a decimal-typed expression (D11.7, ruling 19).
///
/// Decimal arithmetic is exact until the value lands in a slot, so an emit
/// tier evaluates a computation as an exact `(carrier, scale)` pair and only
/// rounds at the store. A leaf (path, literal, call, field, an already
/// stored value) is a stored `d64` carrier at scale 8, or an integer at
/// scale 0 when it joins decimal arithmetic.
pub enum ExactDecimal<'a> {
    /// `lhs op rhs` with `op` one of `+ - * / %`.
    Binary(HirBinOp, &'a HirExpression, &'a HirExpression),
    /// `-operand`.
    Neg(&'a HirExpression),
    /// Any other expression: a stored carrier (or an integer operand).
    Leaf(&'a HirExpression),
}

/// Classify `expr` for exact decimal evaluation.
#[must_use]
pub fn exact_decimal<'a>(expr: &'a HirExpression, types: &TypeTable) -> ExactDecimal<'a> {
    if decimal_expr_width(expr, types).is_some() {
        match &expr.kind {
            HirExpressionKind::Binary(
                op
                @ (HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul | HirBinOp::Div | HirBinOp::Mod),
                lhs,
                rhs,
            ) => return ExactDecimal::Binary(*op, lhs, rhs),
            HirExpressionKind::Unary(HirUnOp::Neg, operand) => return ExactDecimal::Neg(operand),
            _ => {}
        }
    }
    ExactDecimal::Leaf(expr)
}

/// Whether `expr` is decimal arithmetic whose exact value has not been stored.
#[must_use]
pub fn is_decimal_computation(expr: &HirExpression, types: &TypeTable) -> bool {
    !matches!(exact_decimal(expr, types), ExactDecimal::Leaf(_))
}

/// Whether `expr` is a conditional, block or `discerne` of decimal type that
/// yields unstored decimal arithmetic from some branch tail (n5c).
///
/// The tail set comes from [`radix_hir::tails::branch_tails`], the same
/// enumerator typecheck uses to decide where a store rounds, so the store and
/// the exact emission cannot disagree on which constructs are exact. Nested
/// branching tails are followed.
#[must_use]
pub fn yields_exact_decimal_tail(expr: &HirExpression, types: &TypeTable) -> bool {
    decimal_expr_width(expr, types).is_some()
        && radix_hir::tails::branch_tails(expr)
            .into_iter()
            .any(|tail| {
                is_decimal_computation(tail, types) || yields_exact_decimal_tail(tail, types)
            })
}

/// Scale of a leaf operand of decimal arithmetic: a decimal is stored at
/// scale 8, anything else (an integer operand) is scale 0.
#[must_use]
pub fn decimal_leaf_scale(expr: &HirExpression, types: &TypeTable) -> u32 {
    decimal_expr_width(expr, types)
        .and_then(NumericWidth::decimal_scale)
        .unwrap_or(0)
}
