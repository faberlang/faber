//! Exact decimal arithmetic for the runner (D11.7, spec ruling 19).
//!
//! A stored `d64` is `Value::Int(carrier)` with the carrier at scale 8. A
//! decimal expression computes exactly until its value lands in a slot, so an
//! intermediate that is not a scale-8 carrier is `Value::Dec(carrier, scale)`:
//! an `i128` carrier at its own scale. `+ -` align scales and are exact, `*`
//! is exact with the scales added, `/` rounds half-even to the larger operand
//! scale, and every intermediate is bounded by the 128-bit carrier (a value
//! past it traps). The store rounds half-even to scale 8 and traps outside the
//! `i64` carrier range.
//!
//! An exact result that is representable as a scale-8 `i64` carrier is
//! returned as `Value::Int` ([`canonical`]); this changes no value, it only
//! keeps stored-shaped decimals in the representation the rest of the runner
//! already understands.

use super::RunnerError;
use crate::value::round_half_even_quotient;
use radix_lexer::Span;
use radix_mir::MirBinOp;
use std::cmp::Ordering;

/// Fraction digits of a stored `d64`.
pub(crate) const STORED_SCALE: u32 = 8;

const OVERFLOW: &str = "numerus overflow";
const DIVISION: &str = "numerus division failed";

/// `10^digits`, or `None` past the 128-bit carrier.
pub(crate) fn pow10(digits: u32) -> Option<i128> {
    10_i128.checked_pow(digits)
}

/// Carrier and scale of a decimal value: a stored carrier is scale 8.
pub(crate) fn parts(value: &crate::Value) -> Option<(i128, u32)> {
    match value {
        crate::Value::Int(v) => Some((i128::from(*v), STORED_SCALE)),
        crate::Value::Dec(carrier, scale) => Some((*carrier, *scale)),
        _ => None,
    }
}

/// The value of an exact `carrier × 10^-scale`, as a stored carrier when it is
/// exactly one and as a scaled intermediate otherwise.
pub(crate) fn canonical(mut carrier: i128, mut scale: u32) -> crate::Value {
    while scale > STORED_SCALE && carrier % 10 == 0 {
        carrier /= 10;
        scale -= 1;
    }
    if scale <= STORED_SCALE
        && let Some(factor) = pow10(STORED_SCALE - scale)
        && let Some(stored) = carrier.checked_mul(factor)
        && let Ok(stored) = i64::try_from(stored)
    {
        return crate::Value::Int(stored);
    }
    crate::Value::Dec(carrier, scale)
}

/// Bring two carriers to the larger scale.
fn align(a: (i128, u32), b: (i128, u32), span: Span) -> Result<(i128, i128, u32), RunnerError> {
    let scale = a.1.max(b.1);
    let up = |(carrier, from): (i128, u32)| -> Result<i128, RunnerError> {
        // A zero carrier is zero at every scale: it never goes through
        // `pow10`, which overflows past a gap of 38 digits and would trap an
        // exact value.
        if carrier == 0 {
            return Ok(0);
        }
        pow10(scale - from)
            .and_then(|factor| carrier.checked_mul(factor))
            .ok_or_else(|| RunnerError::internal(span, OVERFLOW))
    };
    Ok((up(a)?, up(b)?, scale))
}

/// Execute one decimal arithmetic operation over exact operands.
pub(crate) fn binop(
    left: &crate::Value,
    right: &crate::Value,
    op: MirBinOp,
    span: Span,
) -> Result<crate::Value, RunnerError> {
    let (Some(a), Some(b)) = (parts(left), parts(right)) else {
        return Err(RunnerError::internal(
            span,
            "decimal operand requires scaled numerus",
        ));
    };
    let overflow = || RunnerError::internal(span, OVERFLOW);
    let (carrier, scale) = match op {
        MirBinOp::Add => {
            let (x, y, scale) = align(a, b, span)?;
            (x.checked_add(y).ok_or_else(overflow)?, scale)
        }
        MirBinOp::Sub => {
            let (x, y, scale) = align(a, b, span)?;
            (x.checked_sub(y).ok_or_else(overflow)?, scale)
        }
        MirBinOp::Mul => (
            a.0.checked_mul(b.0).ok_or_else(overflow)?,
            a.1.checked_add(b.1).ok_or_else(overflow)?,
        ),
        MirBinOp::Div => {
            if b.0 == 0 {
                return Err(RunnerError::internal(span, DIVISION));
            }
            // (ca / 10^sa) / (cb / 10^sb) at scale s = max(sa, sb):
            // round(ca * 10^(s + sb - sa) / cb).
            let scale = a.1.max(b.1);
            let numerator = pow10(scale + b.1 - a.1)
                .and_then(|factor| a.0.checked_mul(factor))
                .ok_or_else(|| RunnerError::internal(span, DIVISION))?;
            let quotient = round_half_even_quotient(numerator, b.0)
                .ok_or_else(|| RunnerError::internal(span, DIVISION))?;
            (quotient, scale)
        }
        MirBinOp::Mod => {
            let (x, y, scale) = align(a, b, span)?;
            if y == 0 {
                return Err(RunnerError::internal(span, DIVISION));
            }
            (crate::exact_int::floor_rem_checked(x, y), scale)
        }
        _ => {
            return Err(RunnerError::internal(
                span,
                "invalid decimal arithmetic operator",
            ));
        }
    };
    Ok(canonical(carrier, scale))
}

/// `-value`, exact.
pub(crate) fn neg(value: &crate::Value, span: Span) -> Result<crate::Value, RunnerError> {
    let (carrier, scale) =
        parts(value).ok_or_else(|| RunnerError::internal(span, "negation type mismatch"))?;
    let carrier = carrier
        .checked_neg()
        .ok_or_else(|| RunnerError::internal(span, OVERFLOW))?;
    Ok(canonical(carrier, scale))
}

/// Exact three-way comparison of two decimal values.
pub(crate) fn compare(a: (i128, u32), b: (i128, u32)) -> Ordering {
    let scale = a.1.max(b.1);
    // A zero carrier is zero at every scale: it never goes through `pow10`,
    // which overflows past a gap of 38 digits and would misread it as a
    // dominating magnitude.
    let up = |(carrier, from): (i128, u32)| {
        if carrier == 0 {
            return Some(0);
        }
        pow10(scale - from).and_then(|f| carrier.checked_mul(f))
    };
    match (up(a), up(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        // A nonzero side that overflows the shared scale dominates the other
        // in magnitude, so its sign decides.
        (None, _) => {
            if a.0 < 0 {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (_, None) => {
            if b.0 < 0 {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
    }
}

/// Where a decimal value lands in a `d64` slot: round half-even to scale 8.
/// `None` when the rounded carrier is outside the slot's range.
pub(crate) fn store_carrier(carrier: i128, scale: u32) -> Option<i64> {
    let rounded = if scale > STORED_SCALE {
        round_half_even_quotient(carrier, pow10(scale - STORED_SCALE)?)?
    } else {
        carrier.checked_mul(pow10(STORED_SCALE - scale)?)?
    };
    i64::try_from(rounded).ok()
}

/// Shortest text of a decimal value: trailing fraction zeros are dropped.
pub(crate) fn format(carrier: i128, scale: u32) -> String {
    let negative = carrier < 0;
    let magnitude = carrier.unsigned_abs();
    let digits = scale as usize;
    let text = format!("{magnitude:0>width$}", width = digits + 1);
    let (integer, fraction) = text.split_at(text.len() - digits);
    let fraction = fraction.trim_end_matches('0');
    let sign = if negative { "-" } else { "" };
    if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}.{fraction}")
    }
}

#[cfg(test)]
#[path = "decimal_exact_test.rs"]
mod tests;
