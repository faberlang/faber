//! Shared decimal-carrier math for codegen tiers (decimal-widths D5).
//!
//! Decimal widths ride the tier's signed-integer carrier (`d32` = `i32`×10⁻⁷,
//! `d64` = `i64`×10⁻⁹) with round-half-even rescale. These pure helpers keep
//! every emit tier on the same carrier math as the MIR runner without
//! re-importing runner internals.

use radix_types::NumericWidth;

#[cfg(test)]
#[path = "decimal_test.rs"]
mod tests;

/// Decimal scale as an integer factor (`d32` = 10^7, `d64` = 10^9).
#[must_use]
pub fn decimal_scale_factor(width: NumericWidth) -> Option<i128> {
    width
        .decimal_scale()
        .and_then(|digits| 10_i128.checked_pow(digits))
}

/// Carrier integer type spelling for a decimal width.
///
/// d32 rides `i32`, d64 rides `i64`; other widths return `None`.
#[must_use]
pub fn decimal_carrier_rust_type(width: NumericWidth) -> Option<&'static str> {
    match width {
        NumericWidth::D32 => Some("i32"),
        NumericWidth::D64 => Some("i64"),
        _ => None,
    }
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
        NumericWidth::D32 => (i128::from(i32::MIN), i128::from(i32::MAX)),
        NumericWidth::D64 => (i128::from(i64::MIN), i128::from(i64::MAX)),
        _ => return None,
    };
    if (lo..=hi).contains(&carrier) {
        i64::try_from(carrier).ok()
    } else {
        None
    }
}
