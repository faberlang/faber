//! Exact rational arithmetic for literal-only float constants (F9 ruling 31,
//! inf-U7).
//!
//! A constant expression made only of literals is one exact number, rounded to
//! the float width of the slot it lands in exactly once. [`ExactRational`] is
//! that number: a numerator and a positive denominator on the shared bignum
//! ([`Magnus`]). Decimal spellings (`0.1`, `1.5e-3`) convert without rounding,
//! the four operators and the floor remainder are exact, and the conversions
//! to `f64`, `f32`, `f16` and `bf16` round the exact value to nearest, ties to
//! even, with subnormals and overflow to `±∞` (IEEE default, like
//! [`Magnus::to_f64`]). Rounding never goes through an intermediate width, so
//! an `f32` result is never a double rounding. Zero has one sign: `-0.0` and
//! `0.0` are the same value here, so a conversion of an exact zero is `+0.0`.
//!
//! Every operation is bounded: a value whose numerator or denominator would
//! pass [`CEILING_BITS`] is refused (`None`) so a caller leaves the expression
//! to run time, like the integer fold's cap.

use std::cmp::Ordering;

use handroll::Magnus;

use crate::numeric_width::NumericWidth;

/// Bit-length cap of a numerator or denominator (ruling 45's ceiling, applied
/// to the compiler's constant fold).
pub const CEILING_BITS: u64 = 1 << 20;

/// An exact rational number: `num / den` with `den > 0`. Not reduced; two
/// spellings of one value compare equal.
#[derive(Debug, Clone)]
pub struct ExactRational {
    num: Magnus,
    den: Magnus,
}

/// The target format of a correctly rounded conversion.
#[derive(Clone, Copy)]
struct Format {
    /// Significand bits including the implicit leading one.
    mantissa_bits: u32,
    /// Exponent of the smallest normal number.
    emin: i32,
    /// Exponent of the largest finite number.
    emax: i32,
}

const F16: Format = Format {
    mantissa_bits: 11,
    emin: -14,
    emax: 15,
};
const BF16: Format = Format {
    mantissa_bits: 8,
    emin: -126,
    emax: 127,
};
const F32: Format = Format {
    mantissa_bits: 24,
    emin: -126,
    emax: 127,
};
const F64: Format = Format {
    mantissa_bits: 53,
    emin: -1022,
    emax: 1023,
};

impl ExactRational {
    /// The integer `value`.
    ///
    /// Not capped: an integer past [`CEILING_BITS`] is taken as given, and the
    /// arithmetic that follows it refuses.
    #[must_use]
    pub fn from_integer(value: Magnus) -> Self {
        Self {
            num: value,
            den: Magnus::one(),
        }
    }

    /// `num / den`; `None` for a zero denominator or a value past the cap.
    #[must_use]
    pub fn from_ratio(num: Magnus, den: Magnus) -> Option<Self> {
        if den.is_zero() {
            return None;
        }
        Self::capped(num, den)
    }

    /// The exact value of a decimal spelling: `digits[.digits][e[+-]digits]`
    /// with an optional leading sign and no separators. `None` for any other
    /// text or a value past the cap.
    #[must_use]
    pub fn from_decimal_text(text: &str) -> Option<Self> {
        let (negative, unsigned) = match text.as_bytes().first()? {
            b'-' => (true, &text[1..]),
            b'+' => (false, &text[1..]),
            _ => (false, text),
        };
        let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
            Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
            None => (unsigned, None),
        };
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits = format!("{whole}{fraction}");
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let exponent: i64 = match exponent {
            Some(text) => text.parse().ok()?,
            None => 0,
        };
        let magnitude = Magnus::parse_decimal(&digits).ok()?;
        let scale = exponent.checked_sub(i64::try_from(fraction.len()).ok()?)?;
        let power = Magnus::from_u64(10)
            .potentia_within(&Magnus::from_u64(scale.unsigned_abs()), CEILING_BITS)
            .ok()?;
        let signed = if negative { -magnitude } else { magnitude };
        if scale >= 0 {
            Self::capped(signed.mul_within(&power, CEILING_BITS).ok()?, Magnus::one())
        } else {
            Self::capped(signed, power)
        }
    }

    /// The exact value of a finite `f64`; `None` for NaN and `±∞`.
    #[must_use]
    pub fn from_f64(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        let bits = value.to_bits();
        let exponent_field = i64::try_from((bits >> 52) & 0x7FF).ok()?;
        let fraction = bits & ((1_u64 << 52) - 1);
        let (mantissa, exponent) = if exponent_field == 0 {
            (fraction, -1074)
        } else {
            (fraction | (1_u64 << 52), exponent_field - 1075)
        };
        let magnitude = Magnus::from_u64(mantissa);
        let (num, den) = if exponent >= 0 {
            (magnitude.shl(exponent.unsigned_abs()).ok()?, Magnus::one())
        } else {
            (magnitude, Magnus::one().shl(exponent.unsigned_abs()).ok()?)
        };
        Self::capped(if bits >> 63 == 1 { -num } else { num }, den)
    }

    /// Refuses a numerator or denominator past [`CEILING_BITS`] and sets a
    /// positive denominator, so `den > 0` holds for every value.
    fn capped(num: Magnus, den: Magnus) -> Option<Self> {
        if num.exceeds_ceiling(CEILING_BITS) || den.exceeds_ceiling(CEILING_BITS) {
            return None;
        }
        Some(if den.is_negative() {
            Self {
                num: -num,
                den: -den,
            }
        } else {
            Self { num, den }
        })
    }

    /// Whether the value is exactly zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.num.is_zero()
    }

    /// Whether the value is below zero.
    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.num.is_negative()
    }

    /// `self + rhs`; `None` past the cap.
    #[must_use]
    pub fn add(&self, rhs: &Self) -> Option<Self> {
        let left = self.num.mul_within(&rhs.den, CEILING_BITS).ok()?;
        let right = rhs.num.mul_within(&self.den, CEILING_BITS).ok()?;
        Self::capped(
            left + right,
            self.den.mul_within(&rhs.den, CEILING_BITS).ok()?,
        )
    }

    /// `self - rhs`; `None` past the cap.
    #[must_use]
    pub fn sub(&self, rhs: &Self) -> Option<Self> {
        self.add(&rhs.neg())
    }

    /// `self * rhs`; `None` past the cap.
    #[must_use]
    pub fn mul(&self, rhs: &Self) -> Option<Self> {
        Self::capped(
            self.num.mul_within(&rhs.num, CEILING_BITS).ok()?,
            self.den.mul_within(&rhs.den, CEILING_BITS).ok()?,
        )
    }

    /// `self / rhs`; `None` for a zero divisor or past the cap.
    #[must_use]
    pub fn div(&self, rhs: &Self) -> Option<Self> {
        if rhs.is_zero() {
            return None;
        }
        Self::capped(
            self.num.mul_within(&rhs.den, CEILING_BITS).ok()?,
            self.den.mul_within(&rhs.num, CEILING_BITS).ok()?,
        )
    }

    /// `-self`.
    #[must_use]
    pub fn neg(&self) -> Self {
        Self {
            num: -&self.num,
            den: self.den.clone(),
        }
    }

    /// The floor remainder `self - rhs * floor(self / rhs)` (ruling 29): the
    /// result takes the sign of `rhs`. `None` for a zero divisor or past the cap.
    #[must_use]
    pub fn rem_floor(&self, rhs: &Self) -> Option<Self> {
        let quotient = self.div(rhs)?;
        let floor = quotient.num.div_floor(&quotient.den).ok()?;
        self.sub(&rhs.mul(&Self::from_integer(floor))?)
    }

    fn cross(&self, rhs: &Self) -> (Magnus, Magnus) {
        (&self.num * &rhs.den, &rhs.num * &self.den)
    }

    /// Nearest `f64`, ties to even; `±∞` past the format's range.
    #[must_use]
    pub fn to_f64(&self) -> f64 {
        self.round(F64)
    }

    /// Nearest `f32`, ties to even, rounded once from the exact value.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // the value is already an exact f32
    pub fn to_f32(&self) -> f32 {
        self.round(F32) as f32
    }

    /// Nearest `f16` (IEEE binary16), ties to even, widened exactly to `f32`.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // the value is already an exact f16
    pub fn to_f16(&self) -> f32 {
        self.round(F16) as f32
    }

    /// Nearest `bf16`, ties to even, widened exactly to `f32`.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // the value is already an exact bf16
    pub fn to_bf16(&self) -> f32 {
        self.round(BF16) as f32
    }

    /// The exact value nearest `self` in the float `width`, carried in an
    /// `f64` (every float width is a subset of it); `None` when `width` is not
    /// a float width.
    #[must_use]
    pub fn round_to_width(&self, width: NumericWidth) -> Option<f64> {
        let format = match width {
            NumericWidth::F16 => F16,
            NumericWidth::BF16 => BF16,
            NumericWidth::F32 => F32,
            NumericWidth::F64 => F64,
            _ => return None,
        };
        Some(self.round(format))
    }

    fn round(&self, format: Format) -> f64 {
        // A failure would read as NaN, never a plausible wrong number, and none
        // is reachable: `den > 0`, and a value past the format's range returns
        // infinity before any shift, so every shift is bounded by the format.
        let magnitude = round_positive(&self.num.abs(), &self.den, format).unwrap_or(f64::NAN);
        if self.num.is_negative() {
            -magnitude
        } else {
            magnitude
        }
    }
}

/// Round `num / den` (both positive or `num` zero) to nearest even in
/// `format`: pick the quantum `2^k` of the binade holding the value (clamped
/// at the subnormal quantum), divide once, round the quotient with the
/// remainder, and scale back by the exact power of two.
fn round_positive(num: &Magnus, den: &Magnus, format: Format) -> Option<f64> {
    if num.is_zero() {
        return Some(0.0);
    }
    let diff = i64::try_from(num.bit_len()).ok()? - i64::try_from(den.bit_len()).ok()?;
    let at_least_binade = if diff >= 0 {
        *num >= den.shl(diff.unsigned_abs()).ok()?
    } else {
        num.shl(diff.unsigned_abs()).ok()? >= *den
    };
    let exponent = if at_least_binade { diff } else { diff - 1 };
    if exponent > i64::from(format.emax) {
        // Out of range: the shift below is bounded only while `exponent <= emax`,
        // and `ExactRational::from_integer` admits an integer of any length.
        return Some(f64::INFINITY);
    }
    let quantum = exponent.max(i64::from(format.emin)) - i64::from(format.mantissa_bits - 1);
    let (scaled_num, scaled_den) = if quantum < 0 {
        (num.shl(quantum.unsigned_abs()).ok()?, den.clone())
    } else {
        (num.clone(), den.shl(quantum.unsigned_abs()).ok()?)
    };
    let (quotient, remainder) = scaled_num.div_mod_floor(&scaled_den).ok()?;
    let mut significand = quotient.to_u64()?;
    match remainder.shl(1).ok()?.cmp(&scaled_den) {
        Ordering::Greater => significand += 1,
        Ordering::Equal if significand & 1 == 1 => significand += 1,
        _ => {}
    }
    if significand == 0 {
        // Below half of the smallest subnormal.
        return Some(0.0);
    }
    let top = i64::from(significand.ilog2()) + quantum;
    if top > i64::from(format.emax) {
        // The rounding above carried past the format's largest finite value.
        return Some(f64::INFINITY);
    }
    // `significand <= 2^53` converts exactly, and `significand * 2^quantum`
    // is representable in `format`, hence in `f64`.
    #[allow(clippy::cast_precision_loss)]
    let value = significand as f64;
    Some(value * power_of_two(i32::try_from(quantum).ok()?))
}

/// `2^k` for `k` in `[-1074, 1023]`, exact (subnormal below `-1022`).
fn power_of_two(k: i32) -> f64 {
    if k >= -1022 {
        f64::from_bits(u64::try_from(k + 1023).unwrap_or(0) << 52)
    } else {
        f64::from_bits(1_u64 << u32::try_from(k + 1074).unwrap_or(0))
    }
}

impl PartialEq for ExactRational {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for ExactRational {}

impl PartialOrd for ExactRational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ExactRational {
    fn cmp(&self, other: &Self) -> Ordering {
        let (left, right) = self.cross(other);
        left.cmp(&right)
    }
}

#[cfg(test)]
#[path = "exact_rational_test.rs"]
mod tests;
