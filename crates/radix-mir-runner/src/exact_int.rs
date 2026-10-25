//! Exact integer arithmetic for the runner (d11-6 rulings 9, 10, 14, 17, 19b).
//!
//! An integer expression computes the exact mathematical result. Every value,
//! operands and intermediates alike, lies in the single 64-bit range
//! `[-2^63, 2^64 - 1]` (ruling 17a): a value outside it traps. The width of a
//! trapping expression's type is not a second range check here — the store
//! into a slot applies the slot's policy.
//!
//! The one exception is the unbounded `inf` result kind (inf-delivery rulings
//! 35, 36): an expression whose result width is `inf`, or any operand that is
//! already a `Magnus`, computes on the bignum and never traps on size (only on
//! the runner ceilings, `crate::inf`). Each entry point below routes there
//! first.
//!
//! Division floors and `%` takes the divisor's sign (ruling 14). `¬x` is
//! `-x - 1`, and shifts are `x * 2^n` and `floor(x / 2^n)` with no count
//! masking; a negative count is an error (rulings 9, 10). `& | ^` are defined
//! on infinite two's-complement integers (ruling 19b), which `i128` embeds.

use super::RunnerError;
use crate::inf;
use crate::value::Value;
use radix_lexer::Span;
use radix_mir::MirBinOp;
use radix_types::NumericWidth;

/// Smallest value an exact integer may hold.
pub(crate) const EXACT_MIN: i128 = i64::MIN as i128;
/// Largest value an exact integer may hold.
pub(crate) const EXACT_MAX: i128 = u64::MAX as i128;

const OVERFLOW: &str = "numerus overflow";
const DIVISION: &str = "numerus division failed";
const NEGATIVE_SHIFT: &str = "negative shift count";

/// 2^127 as an `f64`: a float at or past it is beyond every exact integer, and
/// above every decimal magnitude that fits the 128-bit carrier.
const F64_EXACT_LIMIT: f64 = 170_141_183_460_469_231_731_687_303_715_884_105_728.0;

/// The exact value of an integer carrier.
pub(crate) fn int_value(value: &Value) -> Option<i128> {
    match value {
        Value::Int(v) => Some(i128::from(*v)),
        Value::UInt(v) => Some(i128::from(*v)),
        _ => None,
    }
}

/// The carrier of an exact value: negative values are `Int`, a `u64`-typed
/// result is always `UInt`, and anything else is `Int` unless it does not fit.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // value is in 0..=u64::MAX here
pub(crate) fn exact_carrier(value: i128, width: NumericWidth) -> Option<Value> {
    if !(EXACT_MIN..=EXACT_MAX).contains(&value) {
        return None;
    }
    Some(match i64::try_from(value) {
        Ok(v) if v < 0 || width != NumericWidth::U64 => Value::Int(v),
        _ => Value::UInt(value as u64),
    })
}

pub(crate) fn floor_div(a: i128, b: i128) -> i128 {
    let quotient = a / b;
    if a % b != 0 && ((a < 0) != (b < 0)) {
        quotient - 1
    } else {
        quotient
    }
}

pub(crate) fn floor_rem(a: i128, b: i128) -> i128 {
    a - b * floor_div(a, b)
}

/// Floor remainder without the `b * floor_div` product: safe for any `i128`
/// operands (`MIN % -1` is `0`). `b` must be nonzero.
pub(crate) fn floor_rem_checked(a: i128, b: i128) -> i128 {
    let r = a.checked_rem(b).unwrap_or(0);
    if r != 0 && ((r < 0) != (b < 0)) {
        r + b
    } else {
        r
    }
}

/// Float `%` (F9 ruling 29): `fmod`, then the divisor once when the signs
/// disagree (Python's definition). An exact multiple is a zero with the
/// divisor's sign; NaN and infinity classes are `fmod`'s.
pub(crate) fn float_floor_rem(a: f64, b: f64) -> f64 {
    let r = a % b;
    if r == 0.0 {
        0.0_f64.copysign(b)
    } else if (r < 0.0) != (b < 0.0) {
        r + b
    } else {
        r
    }
}

/// `2^count` times `value`, or `None` past the exact range.
fn shift_left(value: i128, count: i128) -> Option<i128> {
    if value == 0 {
        return Some(0);
    }
    if count > 64 {
        return None;
    }
    value.checked_mul(1i128 << count)
}

fn shift_right(value: i128, count: i128) -> i128 {
    if count >= 127 {
        if value < 0 { -1 } else { 0 }
    } else {
        value >> count
    }
}

/// One exact integer operation over exact operands. `None` means the exact
/// result is outside the 64-bit range.
fn exact_op(op: MirBinOp, a: i128, b: i128, span: Span) -> Result<Option<i128>, RunnerError> {
    let division = || RunnerError::internal(span, DIVISION);
    let value = match op {
        MirBinOp::Add => a.checked_add(b),
        MirBinOp::Sub => a.checked_sub(b),
        MirBinOp::Mul => a.checked_mul(b),
        MirBinOp::Div | MirBinOp::Mod if b == 0 => return Err(division()),
        MirBinOp::Div => Some(floor_div(a, b)),
        MirBinOp::Mod => Some(floor_rem(a, b)),
        MirBinOp::BitAnd => Some(a & b),
        MirBinOp::BitOr => Some(a | b),
        MirBinOp::BitXor => Some(a ^ b),
        MirBinOp::Shl | MirBinOp::Shr if b < 0 => {
            return Err(RunnerError::internal(span, NEGATIVE_SHIFT));
        }
        MirBinOp::Shl => shift_left(a, b),
        MirBinOp::Shr => Some(shift_right(a, b)),
        _ => return Err(RunnerError::internal(span, "invalid integer operator")),
    };
    Ok(value)
}

/// Execute one exact integer binary operation on two integer carriers.
///
/// `None` when either operand is not an integer carrier, so the caller can
/// fall through to the float and decimal paths.
pub(crate) fn exact_int_binop(
    left: &Value,
    right: &Value,
    op: MirBinOp,
    width: NumericWidth,
    span: Span,
) -> Option<Result<Value, RunnerError>> {
    if width == NumericWidth::Inf
        || matches!((left, right), (Value::Magnus(_), _) | (_, Value::Magnus(_)))
    {
        return Some(inf::binop(
            op,
            &inf::to_magnus(left)?,
            &inf::to_magnus(right)?,
            span,
        ));
    }
    let (a, b) = (int_value(left)?, int_value(right)?);
    Some(exact_op(op, a, b, span).and_then(|value| {
        value
            .and_then(|value| exact_carrier(value, width))
            .ok_or_else(|| RunnerError::internal(span, OVERFLOW))
    }))
}

/// Exact unary `-`.
pub(crate) fn exact_neg(
    value: &Value,
    width: NumericWidth,
    span: Span,
) -> Option<Result<Value, RunnerError>> {
    if width == NumericWidth::Inf || matches!(value, Value::Magnus(_)) {
        return Some(Ok(inf::neg(&inf::to_magnus(value)?)));
    }
    let a = int_value(value)?;
    Some(exact_carrier(-a, width).ok_or_else(|| RunnerError::internal(span, OVERFLOW)))
}

/// Exact `¬x`, that is `-x - 1`.
pub(crate) fn exact_bitnot(
    value: &Value,
    width: NumericWidth,
    span: Span,
) -> Option<Result<Value, RunnerError>> {
    if width == NumericWidth::Inf || matches!(value, Value::Magnus(_)) {
        return Some(Ok(inf::bit_not(&inf::to_magnus(value)?)));
    }
    let a = int_value(value)?;
    Some(exact_carrier(-a - 1, width).ok_or_else(|| RunnerError::internal(span, OVERFLOW)))
}

/// Exact three-way comparison of an integer with a float (ruling 22): the
/// integer is never rounded to the float's width, so `2^53 + 1 > 2^53 as f64`.
/// `None` when the float is NaN.
pub(crate) fn cmp_int_float(int: i128, float: f64) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    if float.is_nan() {
        return None;
    }
    if float >= F64_EXACT_LIMIT {
        return Some(Ordering::Less);
    }
    if float <= -F64_EXACT_LIMIT {
        return Some(Ordering::Greater);
    }
    let whole = float.trunc();
    // `whole` is integral and inside the i128 range, so the cast is exact.
    #[allow(clippy::cast_possible_truncation)]
    let whole_int = whole as i128;
    match int.cmp(&whole_int) {
        Ordering::Equal => {
            let fraction = float - whole;
            Some(if fraction > 0.0 {
                Ordering::Less
            } else if fraction < 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            })
        }
        other => Some(other),
    }
}

/// Exact three-way comparison of two numeric carriers of different families
/// (integer or float); `None` for a NaN operand or non-numeric carriers.
pub(crate) fn cmp_numeric(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        // `inf` beyond the 64-bit carriers (ruling 37): exact against every
        // integer, decimal-free float or other `inf`.
        (Value::Magnus(a), Value::Float(b)) => inf::cmp_magnus_float(a, *b),
        (Value::Float(a), Value::Magnus(b)) => {
            inf::cmp_magnus_float(b, *a).map(std::cmp::Ordering::reverse)
        }
        (Value::Magnus(_), _) | (_, Value::Magnus(_)) => {
            Some(inf::to_magnus(left)?.cmp(&inf::to_magnus(right)?))
        }
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Float(a), other) => {
            cmp_int_float(int_value(other)?, *a).map(std::cmp::Ordering::reverse)
        }
        (other, Value::Float(b)) => cmp_int_float(int_value(other)?, *b),
        (a, b) => Some(int_value(a)?.cmp(&int_value(b)?)),
    }
}

/// Exact three-way comparison of a decimal (`scaled / scale`, `scale` a power
/// of ten within the 128-bit carrier) with a float (ruling 22). `None` when
/// the float is NaN.
pub(crate) fn cmp_scaled_float(
    scaled: i128,
    scale: i128,
    float: f64,
) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    if float.is_nan() {
        return None;
    }
    if float >= F64_EXACT_LIMIT {
        return Some(Ordering::Less);
    }
    if float <= -F64_EXACT_LIMIT {
        return Some(Ordering::Greater);
    }
    // `float - floor` is exact only for a non-negative float, so a negative
    // comparison runs mirrored.
    if float < 0.0
        && let Some(negated) = scaled.checked_neg()
    {
        return cmp_scaled_float(negated, scale, -float).map(std::cmp::Ordering::reverse);
    }
    let floor = float.floor();
    let fraction = float - floor; // exact, in [0, 1)
    #[allow(clippy::cast_possible_truncation)]
    // `floor` is bounded by the F64_EXACT_LIMIT guard above
    let whole = floor as i128;
    let (quotient, remainder) = (floor_div(scaled, scale), floor_rem(scaled, scale));
    match quotient.cmp(&whole) {
        Ordering::Equal => {}
        other => return Some(other),
    }
    // Same integer part: compare remainder / scale with the float's fraction.
    if fraction == 0.0 {
        return Some(if remainder == 0 {
            Ordering::Equal
        } else {
            Ordering::Greater
        });
    }
    if remainder == 0 {
        return Some(Ordering::Less);
    }
    // fraction = mantissa * 2^-shift exactly, with mantissa < 2^53.
    let bits = fraction.to_bits();
    let exponent = i32::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let (mantissa, shift) = if exponent == 0 {
        (u128::from(bits & ((1 << 52) - 1)), 1074_u32)
    } else {
        (
            u128::from((bits & ((1 << 52) - 1)) | (1 << 52)),
            u32::try_from(1075 - exponent).unwrap_or(0),
        )
    };
    // remainder * 2^shift against mantissa * scale, both positive.
    let remainder = remainder.unsigned_abs();
    let scale = scale.unsigned_abs();
    let left_bits = (128 - remainder.leading_zeros()) + shift;
    let right_bits = (128 - mantissa.leading_zeros()) + (128 - scale.leading_zeros());
    // The product has `right_bits` or `right_bits - 1` bits.
    if left_bits > right_bits {
        return Some(Ordering::Greater);
    }
    if left_bits + 1 < right_bits {
        return Some(Ordering::Less);
    }
    // Within one bit of each other, both fit 256 bits.
    Some(wide_shl(remainder, shift).cmp(&wide_mul(mantissa, scale)))
}

/// `value << shift` as a 256-bit `(high, low)` pair; the caller guarantees the
/// result fits.
fn wide_shl(value: u128, shift: u32) -> (u128, u128) {
    match shift {
        0 => (0, value),
        1..=127 => (value >> (128 - shift), value << shift),
        128 => (value, 0),
        _ => (value << (shift - 128), 0),
    }
}

/// `a * b` as a 256-bit `(high, low)` pair.
fn wide_mul(a: u128, b: u128) -> (u128, u128) {
    const MASK: u128 = u64::MAX as u128;
    let (a_hi, a_lo) = (a >> 64, a & MASK);
    let (b_hi, b_lo) = (b >> 64, b & MASK);
    let low = a_lo * b_lo;
    let mid1 = a_hi * b_lo;
    let mid2 = a_lo * b_hi;
    let high = a_hi * b_hi;
    let (mid, carry) = mid1.overflowing_add(mid2);
    let (low_sum, carry_low) = low.overflowing_add(mid << 64);
    let high = high + (mid >> 64) + (u128::from(carry) << 64) + u128::from(carry_low);
    (high, low_sum)
}
