//! Exact integer arithmetic for the runner (d11-6 rulings 9, 10, 14, 17, 19b).
//!
//! An integer expression computes the exact mathematical result. Every value,
//! operands and intermediates alike, lies in the single 64-bit range
//! `[-2^63, 2^64 - 1]` (ruling 17a): a value outside it traps, until
//! `numerus<inf>` exists. The width of a trapping expression's type is not a
//! second range check here — the store into a slot applies the slot's policy.
//!
//! Division floors and `%` takes the divisor's sign (ruling 14). `¬x` is
//! `-x - 1`, and shifts are `x * 2^n` and `floor(x / 2^n)` with no count
//! masking; a negative count is an error (rulings 9, 10). `& | ^` are defined
//! on infinite two's-complement integers (ruling 19b), which `i128` embeds.

use super::RunnerError;
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
        MirBinOp::Div if b == 0 => return Err(division()),
        MirBinOp::Div => Some(floor_div(a, b)),
        MirBinOp::Mod if b == 0 => return Err(division()),
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
    let a = int_value(value)?;
    Some(exact_carrier(-a, width).ok_or_else(|| RunnerError::internal(span, OVERFLOW)))
}

/// Exact `¬x`, that is `-x - 1`.
pub(crate) fn exact_bitnot(
    value: &Value,
    width: NumericWidth,
    span: Span,
) -> Option<Result<Value, RunnerError>> {
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
    // 2^127 as f64: a float at or past it is beyond every exact integer.
    const LIMIT: f64 = 170_141_183_460_469_231_731_687_303_715_884_105_728.0;
    if float >= LIMIT {
        return Some(Ordering::Less);
    }
    if float <= -LIMIT {
        return Some(Ordering::Greater);
    }
    let whole = float.trunc();
    // `whole` is integral and inside the i128 range, so the cast is exact.
    match int.cmp(&(whole as i128)) {
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
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Float(a), other) => {
            cmp_int_float(int_value(other)?, *a).map(std::cmp::Ordering::reverse)
        }
        (other, Value::Float(b)) => cmp_int_float(int_value(other)?, *b),
        (a, b) => Some(int_value(a)?.cmp(&int_value(b)?)),
    }
}

/// Exact three-way comparison of a decimal (`scaled / scale`, `scale` a power
/// of ten up to 10^9) with a float (ruling 22). `None` when the float is NaN.
pub(crate) fn cmp_scaled_float(
    scaled: i128,
    scale: i128,
    float: f64,
) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    if float.is_nan() {
        return None;
    }
    // A decimal carrier is below 2^64 in magnitude, so a float past 2^100
    // dominates whatever the scale is.
    const HUGE: f64 = 1_267_650_600_228_229_401_496_703_205_376.0;
    if float >= HUGE {
        return Some(Ordering::Less);
    }
    if float <= -HUGE {
        return Some(Ordering::Greater);
    }
    let floor = float.floor();
    let fraction = float - floor; // exact, in [0, 1)
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
        ((bits & ((1 << 52) - 1)) as i128, 1074)
    } else {
        (
            ((bits & ((1 << 52) - 1)) | (1 << 52)) as i128,
            1075 - exponent,
        )
    };
    // remainder * 2^shift against mantissa * scale; the left side is at least
    // 2^91 once shift > 90, while the right side is below 2^83.
    if shift > 90 {
        return Some(Ordering::Greater);
    }
    Some((remainder << shift).cmp(&(mantissa * scale)))
}
