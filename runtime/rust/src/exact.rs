//! Exact-integer helpers for generated Rust (d11-6 rulings 9, 10, 14, 17, 22).
//!
//! Every integer intermediate is an `i128` in `[-2^63, 2^64 - 1]`; `/` floors
//! and `%` takes the divisor's sign; shifts are unmasked; comparisons against a
//! float use the true values. A width limit is applied only at a store
//! ([`store`]); nothing clamps, traps or saturates per operation beyond the
//! representation range check ([`int`]). Mirrors `radix-mir-runner`'s
//! `exact_int` module, which is the golden authority.
//!
//! Generated code calls these as free functions, for example
//! `faber::exact::store::<i64>(faber::exact::int(a.checked_add(b)), "return", "")`.

// WHY: the bodies are the verbatim exact-integer prelude the compiler used to
// emit into every generated file; the casts are the semantic ops of the exact
// window (`i128` carriers narrowed or widened on purpose), and the single-letter
// names are the arithmetic's own.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::manual_range_contains,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::missing_panics_doc,
    clippy::items_after_statements,
    clippy::collapsible_if,
    clippy::manual_assert
)]

use std::cmp::Ordering;

/// `|a - b| <= larger / 1e9`: the approximate (`≈`) comparison of integers.
///
/// `const fn`: genus `static` fields are Rust associated consts, and
/// `Ord::max` is not const.
#[inline]
#[must_use]
pub const fn approx_int(a: i128, b: i128) -> bool {
    let (a_abs, b_abs) = (a.abs(), b.abs());
    let larger = if a_abs >= b_abs { a_abs } else { b_abs };
    1_000_000_000 * (a - b).abs() <= larger
}

/// The approximate (`≈`) comparison of `f64` values.
#[inline]
#[must_use]
pub fn approx_f64(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
}

/// The approximate (`≈`) comparison of `f32` values (both operands arrive
/// widened to `f64`).
#[inline]
#[must_use]
pub fn approx_f32(a: f64, b: f64) -> bool {
    // The runner narrows both operands and the tolerance to `f32`, then
    // compares in `f64`.
    let (a, b) = (f64::from(a as f32), f64::from(b as f32));
    (a - b).abs() <= f64::from(1e-9_f32) * a.abs().max(b.abs())
}

/// The range check of an exact integer intermediate: `None` (a failed
/// `checked_*`) or a value outside `[-2^63, 2^64 - 1]` traps `numerus overflow`.
#[inline]
#[must_use]
#[track_caller]
pub fn int(value: Option<i128>) -> i128 {
    match value {
        Some(value)
            if (-9_223_372_036_854_775_808..=18_446_744_073_709_551_615).contains(&value) =>
        {
            value
        }
        _ => panic!("numerus overflow"),
    }
}

/// Narrow an exact intermediate to a concrete integer type; a value outside it
/// traps `numerus overflow`.
#[must_use]
#[track_caller]
pub fn fit<T: TryFrom<i128>>(value: i128) -> T {
    T::try_from(value).unwrap_or_else(|_| panic!("numerus overflow"))
}

/// Exact integer power by repeated squaring; a negative exponent traps.
#[inline]
#[must_use]
#[track_caller]
pub fn pow(base: i128, exponent: i128) -> i128 {
    if exponent < 0 {
        panic!("numerus potentia failed: negative exponent");
    }
    let mut accumulator: i128 = 1;
    let mut base = base;
    let mut exponent = exponent;
    while exponent > 0 {
        if exponent % 2 != 0 {
            accumulator = int(accumulator.checked_mul(base));
        }
        exponent /= 2;
        if exponent > 0 {
            base = int(base.checked_mul(base));
        }
    }
    accumulator
}

/// Floor division; a zero divisor traps `numerus division failed`.
#[inline]
#[must_use]
#[track_caller]
pub fn div(a: i128, b: i128) -> i128 {
    if b == 0 {
        panic!("numerus division failed");
    }
    let quotient = a / b;
    if a % b != 0 && ((a < 0) != (b < 0)) {
        quotient - 1
    } else {
        quotient
    }
}

/// Remainder with the divisor's sign (`a - b * floor(a / b)`).
#[inline]
#[must_use]
#[track_caller]
pub fn rem(a: i128, b: i128) -> i128 {
    a - b * div(a, b)
}

/// Unmasked left shift; a negative count traps `negative shift count`.
#[inline]
#[must_use]
#[track_caller]
pub fn shl(value: i128, count: i128) -> i128 {
    if count < 0 {
        panic!("negative shift count");
    }
    if value == 0 {
        return 0;
    }
    if count > 64 {
        panic!("numerus overflow");
    }
    int(value.checked_mul(1_i128 << count))
}

/// Unmasked arithmetic right shift; a negative count traps
/// `negative shift count`.
#[inline]
#[must_use]
#[track_caller]
pub fn shr(value: i128, count: i128) -> i128 {
    if count < 0 {
        panic!("negative shift count");
    }
    if count >= 127 {
        if value < 0 { -1 } else { 0 }
    } else {
        value >> count
    }
}

/// The store: narrow an exact intermediate into a bounded integer slot. The
/// width limit is applied here and only here; a value outside the slot traps
/// with the slot's type name, the store site (`at`) and the inferred note.
#[must_use]
#[track_caller]
pub fn store<T: TryFrom<i128>>(value: i128, at: &str, inferred: &str) -> T {
    T::try_from(value).unwrap_or_else(|_| {
        let ty = std::any::type_name::<T>();
        let note = if value < 0 && ty.starts_with('u') {
            " (a negative value cannot be stored in an unsigned slot)"
        } else {
            ""
        };
        panic!("{value} does not fit in `{ty}` ({at}){inferred}{note}")
    })
}

/// Compare an exact integer with a float by their true values; `None` when the
/// float is NaN.
#[inline]
#[must_use]
pub fn cmp_int_float(int: i128, float: f64) -> Option<Ordering> {
    if float.is_nan() {
        return None;
    }
    const LIMIT: f64 = 170_141_183_460_469_231_731_687_303_715_884_105_728.0;
    if float >= LIMIT {
        return Some(Ordering::Less);
    }
    if float <= -LIMIT {
        return Some(Ordering::Greater);
    }
    let whole = float.trunc();
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

/// Compare a scaled decimal carrier (`scaled / scale`) with a float by their
/// true values; `None` when the float is NaN.
#[must_use]
pub fn cmp_scaled_float(scaled: i128, scale: i128, float: f64) -> Option<Ordering> {
    if float.is_nan() {
        return None;
    }
    const HUGE: f64 = 170_141_183_460_469_231_731_687_303_715_884_105_728.0;
    if float >= HUGE {
        return Some(Ordering::Less);
    }
    if float <= -HUGE {
        return Some(Ordering::Greater);
    }
    if float < 0.0 {
        if let Some(negated) = 0i128.checked_sub(scaled) {
            return cmp_scaled_float(negated, scale, -float).map(Ordering::reverse);
        }
    }
    let floor = float.floor();
    let fraction = float - floor;
    let whole = floor as i128;
    let quotient = div(scaled, scale);
    let remainder = rem(scaled, scale);
    match quotient.cmp(&whole) {
        Ordering::Equal => {}
        other => return Some(other),
    }
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
    let bits = fraction.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let (mantissa, shift) = if exponent == 0 {
        ((bits & ((1 << 52) - 1)) as u128, 1074_u32)
    } else {
        (
            (((bits & ((1 << 52) - 1)) | (1 << 52)) as u128),
            (1075 - exponent) as u32,
        )
    };
    let remainder = remainder.unsigned_abs();
    let scale = scale.unsigned_abs();
    let left_bits = (128 - remainder.leading_zeros()) + shift;
    let right_bits = (128 - mantissa.leading_zeros()) + (128 - scale.leading_zeros());
    if left_bits > right_bits {
        return Some(Ordering::Greater);
    }
    if left_bits + 1 < right_bits {
        return Some(Ordering::Less);
    }
    let left = match shift {
        0 => (0u128, remainder),
        1..=127 => (remainder >> (128 - shift), remainder << shift),
        128 => (remainder, 0),
        _ => (remainder << (shift - 128), 0),
    };
    let mask = u64::MAX as u128;
    let (a_hi, a_lo) = (mantissa >> 64, mantissa & mask);
    let (b_hi, b_lo) = (scale >> 64, scale & mask);
    let low = a_lo * b_lo;
    let mid1 = a_hi * b_lo;
    let mid2 = a_lo * b_hi;
    let (mid, carry) = mid1.overflowing_add(mid2);
    let (low_sum, carry_low) = low.overflowing_add(mid << 64);
    let high = a_hi * b_hi + (mid >> 64) + ((carry as u128) << 64) + (carry_low as u128);
    Some(left.cmp(&(high, low_sum)))
}

#[cfg(test)]
#[path = "exact_test.rs"]
mod tests;
