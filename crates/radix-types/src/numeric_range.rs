//! Integer range arithmetic for the d11-6 range rule (rulings 12, 13, 19).
//!
//! A trapping integer expression is typed by the smallest integer width that
//! holds every possible result, computed by interval arithmetic over the
//! operand ranges. Ranges are exact `i128` intervals; every operation
//! saturates instead of overflowing, so an interval beyond the 64-bit
//! containers simply lands on the cap.

use crate::numeric_width::NumericWidth;

/// Inclusive interval of exact integer values.
pub type IntRange = (i128, i128);

/// Smallest integer width holding `range`; at the 64-bit cap the sign of the
/// range decides (`i64` when it includes negatives, otherwise `u64`).
#[must_use]
pub fn smallest_width(range: IntRange) -> NumericWidth {
    let (lo, hi) = range;
    if lo >= 0 {
        for width in [
            NumericWidth::U8,
            NumericWidth::U16,
            NumericWidth::U32,
            NumericWidth::U64,
        ] {
            if width.integer_range().is_some_and(|(_, max)| hi <= max) {
                return width;
            }
        }
        return NumericWidth::U64;
    }
    for width in [
        NumericWidth::I8,
        NumericWidth::I16,
        NumericWidth::I32,
        NumericWidth::I64,
    ] {
        if width
            .integer_range()
            .is_some_and(|(min, max)| lo >= min && hi <= max)
        {
            return width;
        }
    }
    NumericWidth::I64
}

/// Whether `range` lies inside the single 64-bit range [-2^63, 2^64 - 1]
/// (ruling 17): the values some 64-bit integer can hold.
#[must_use]
pub fn fits_container(range: IntRange) -> bool {
    range.0 >= i128::from(i64::MIN) && range.1 <= i128::from(u64::MAX)
}

/// The smallest interval holding both operands.
#[must_use]
pub fn hull(a: IntRange, b: IntRange) -> IntRange {
    (a.0.min(b.0), a.1.max(b.1))
}

/// `a + b`: `[a0 + b0, a1 + b1]`.
#[must_use]
pub fn add(a: IntRange, b: IntRange) -> IntRange {
    (a.0.saturating_add(b.0), a.1.saturating_add(b.1))
}

/// `a - b`: `[a0 - b1, a1 - b0]`.
#[must_use]
pub fn sub(a: IntRange, b: IntRange) -> IntRange {
    (a.0.saturating_sub(b.1), a.1.saturating_sub(b.0))
}

/// `a * b`: the minimum and maximum of the four corner products.
#[must_use]
pub fn mul(a: IntRange, b: IntRange) -> IntRange {
    let corners = [
        a.0.saturating_mul(b.0),
        a.0.saturating_mul(b.1),
        a.1.saturating_mul(b.0),
        a.1.saturating_mul(b.1),
    ];
    (
        corners.into_iter().min().unwrap_or(0),
        corners.into_iter().max().unwrap_or(0),
    )
}

/// Floor division: `[0, a1]` when both operands are non-negative, otherwise
/// `[-M, M]` with `M = max(|a0|, |a1|)`.
#[must_use]
pub fn floor_div(a: IntRange, b: IntRange) -> IntRange {
    if a.0 >= 0 && b.0 >= 0 {
        return (0, a.1);
    }
    let m = a.0.saturating_abs().max(a.1.saturating_abs());
    (-m, m)
}

/// Floor remainder: `[min(0, b0 + 1), max(0, b1 - 1)]`.
#[must_use]
pub fn floor_rem(_a: IntRange, b: IntRange) -> IntRange {
    (0.min(b.0.saturating_add(1)), 0.max(b.1.saturating_sub(1)))
}

/// `-a`: `[-a1, -a0]`.
#[must_use]
pub fn neg(a: IntRange) -> IntRange {
    (a.1.saturating_neg(), a.0.saturating_neg())
}

/// `¬a` is `-a - 1`.
#[must_use]
pub fn bit_not(a: IntRange) -> IntRange {
    (
        a.1.saturating_neg().saturating_sub(1),
        a.0.saturating_neg().saturating_sub(1),
    )
}

/// `|a|`: zero is included when the interval spans it.
#[must_use]
pub fn abs(a: IntRange) -> IntRange {
    let (lo, hi) = (a.0.saturating_abs(), a.1.saturating_abs());
    let lower = if a.0 <= 0 && a.1 >= 0 { 0 } else { lo.min(hi) };
    (lower, lo.max(hi))
}

/// Largest constant exponent `pow_const` folds; a larger exponent clamps to
/// it.
pub const MAX_CONST_EXPONENT: i128 = 128;

fn saturating_pow(base: i128, exponent: u32) -> i128 {
    (0..exponent).fold(1_i128, |acc, _| acc.saturating_mul(base))
}

/// `a ^ n` for a constant exponent `0 <= n <= MAX_CONST_EXPONENT`; an exponent
/// outside that range clamps into it.
#[must_use]
pub fn pow_const(a: IntRange, n: i128) -> IntRange {
    let n = u32::try_from(n.clamp(0, MAX_CONST_EXPONENT)).unwrap_or(0);
    if n == 0 {
        return (1, 1);
    }
    let (lo, hi) = (saturating_pow(a.0, n), saturating_pow(a.1, n));
    if n % 2 == 1 || a.0 >= 0 {
        (lo, hi)
    } else if a.1 <= 0 {
        (hi, lo)
    } else {
        (0, lo.max(hi))
    }
}

/// `a ^ n` for an exponent that is not a constant: the 64-bit cap, whose sign
/// follows the base (ruling 13).
#[must_use]
pub fn pow_unbounded(a: IntRange) -> IntRange {
    (
        if a.0 < 0 { i128::from(i64::MIN) } else { 0 },
        i128::from(u64::MAX),
    )
}

fn pow2(n: i128) -> i128 {
    // `2^126` is the largest power of two an `i128` holds; at or past it the
    // scale saturates.
    if n >= 126 {
        i128::MAX
    } else {
        1_i128 << n.max(0)
    }
}

/// `a ⇐ n`: `a` scaled by `2^[n0, n1]`; a wide count reaches the cap at once.
#[must_use]
pub fn shl(a: IntRange, n: IntRange) -> IntRange {
    let lo = if a.0 < 0 {
        a.0.saturating_mul(pow2(n.1))
    } else {
        a.0.saturating_mul(pow2(n.0))
    };
    let hi = if a.1 >= 0 {
        a.1.saturating_mul(pow2(n.1))
    } else {
        a.1.saturating_mul(pow2(n.0))
    };
    (lo, hi)
}

/// `a ∧ b`: bounded by the non-negative operand's maximum, else the hull.
#[must_use]
pub fn bit_and(a: IntRange, b: IntRange) -> IntRange {
    match (a.0 >= 0, b.0 >= 0) {
        (true, true) => (0, a.1.min(b.1)),
        (true, false) => (0, a.1),
        (false, true) => (0, b.1),
        (false, false) => hull(a, b),
    }
}

#[cfg(test)]
#[path = "numeric_range_test.rs"]
mod tests;
