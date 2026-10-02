//! Unbounded-integer (`inf`) helpers for generated Rust: the traps and
//! conversions the [`Magnus`] operators do not spell themselves.
//!
//! Mirrors `radix-mir-runner`'s `inf` and `conversio/inf` modules, which are the
//! golden authority (trap texts, exact float and decimal comparison,
//! round-to-odd `f32`). Every operation on a [`Magnus`] is exact: `+ - *`,
//! `shl_by` and `potentia` never trap on size (beyond the type's own ceilings);
//! `/` and `%` floor. The failures are the ruled ones only: a zero divisor, a
//! negative shift count or exponent, and the bit-size ceiling. A width limit is
//! applied only at a store ([`store`]); nothing clamps or traps per operation.
//!
//! Generated code calls these as free functions, for example
//! `faber::inf::div(&a, &b)`.

// WHY: the bodies are the verbatim `inf` prelude the compiler used to emit into
// every generated file; the casts are the semantic ops of the round-to-odd
// narrowing (`u64` mantissas scaled into `f64` on purpose).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::missing_panics_doc,
    clippy::manual_assert,
    clippy::single_match_else
)]

use crate::{Magnus, MagnusError, Radix};
use std::cmp::Ordering;

/// A big integer literal from its decimal digits.
#[inline]
#[track_caller]
#[must_use]
pub fn big(digits: &str) -> Magnus {
    digits.parse::<Magnus>().expect("integer literal digits")
}

/// `10^9 * |a - b| <= max(|a|, |b|)`: the approximate (`≈`) comparison of
/// integers, the same answer as between bounded integers.
#[inline]
#[must_use]
pub fn approx(a: &Magnus, b: &Magnus) -> bool {
    let scaled = &(a - b).abs() * &Magnus::from_u64(1_000_000_000);
    let (a, b) = (a.abs(), b.abs());
    scaled <= if a >= b { a } else { b }
}

/// Floor division; a zero divisor traps.
#[inline]
#[track_caller]
#[must_use]
pub fn div(a: &Magnus, b: &Magnus) -> Magnus {
    a.div_floor(b)
        .unwrap_or_else(|_| panic!("numerus division failed"))
}

/// Floor remainder (the divisor's sign); a zero divisor traps.
#[inline]
#[track_caller]
#[must_use]
pub fn rem(a: &Magnus, b: &Magnus) -> Magnus {
    a.mod_floor(b)
        .unwrap_or_else(|_| panic!("numerus division failed"))
}

/// True division (`÷`) to `f64`; a zero divisor traps.
#[inline]
#[track_caller]
#[must_use]
pub fn true_div(a: &Magnus, b: &Magnus) -> f64 {
    if b.is_zero() {
        panic!("numerus division failed");
    }
    a.to_f64() / b.to_f64()
}

/// Shift left, unmasked; a negative count or an exhausted size traps.
#[inline]
#[track_caller]
#[must_use]
pub fn shl(a: &Magnus, count: &Magnus) -> Magnus {
    a.shl_by(count).unwrap_or_else(|error| match error {
        MagnusError::NegativeShift => panic!("negative shift count"),
        _ => panic!("inf size exhausted in shift"),
    })
}

/// Shift right, unmasked; a negative count traps.
#[inline]
#[track_caller]
#[must_use]
pub fn shr(a: &Magnus, count: &Magnus) -> Magnus {
    a.shr_by(count)
        .unwrap_or_else(|_| panic!("negative shift count"))
}

/// `base potentia exponent`; a negative exponent or an exhausted size traps.
#[inline]
#[track_caller]
#[must_use]
pub fn pow(base: &Magnus, exponent: &Magnus) -> Magnus {
    base.potentia(exponent).unwrap_or_else(|error| match error {
        MagnusError::NegativeExponent => {
            panic!("numerus potentia failed: negative exponent")
        }
        _ => panic!("inf size exhausted in potentia"),
    })
}

/// The value as a bounded integer type, or `None` when it does not fit.
#[inline]
#[must_use]
pub fn narrow<T: TryFrom<i128>>(value: &Magnus) -> Option<T> {
    value.to_i128().and_then(|exact| T::try_from(exact).ok())
}

/// The store of an `inf` into a bounded integer slot: traps when the value does
/// not fit (`at` is the trap position, `inferred` the inferred-slot note).
#[inline]
#[track_caller]
#[must_use]
pub fn store<T: TryFrom<i128>>(value: &Magnus, at: &str, inferred: &str) -> T {
    match value.to_i128().and_then(|exact| T::try_from(exact).ok()) {
        Some(stored) => stored,
        None => {
            let ty = std::any::type_name::<T>();
            let note = if value.is_negative() && ty.starts_with('u') {
                " (a negative value cannot be stored in an unsigned slot)"
            } else {
                ""
            };
            panic!("{value} does not fit in `{ty}` ({at}){inferred}{note}")
        }
    }
}

/// The value scaled by `scale` into a stored `d64` carrier (`i64`), or `None`
/// when it does not fit.
#[inline]
#[must_use]
pub fn to_dec(value: &Magnus, scale: i128) -> Option<i64> {
    value
        .to_i128()?
        .checked_mul(scale)
        .and_then(|scaled| i64::try_from(scaled).ok())
}

/// The value as an `f32`: round to odd at 53 bits first, so the narrowing rounds
/// once.
#[inline]
#[must_use]
pub fn f32(value: &Magnus) -> f32 {
    let len = value.bit_len();
    let odd = if len <= 53 {
        value.to_f64()
    } else if len > 1024 {
        if value.is_negative() {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }
    } else {
        let drop = len - 53;
        let magnitude = value.abs();
        let top = magnitude.shr(drop);
        let sticky = !top.shl(drop).is_ok_and(|rebuilt| rebuilt == magnitude);
        let mut mantissa = top.to_u64().unwrap_or(0);
        if sticky {
            mantissa |= 1;
        }
        let scaled = mantissa as f64 * f64::from_bits((1023 + drop) << 52);
        if value.is_negative() { -scaled } else { scaled }
    };
    odd as f32
}

/// The digits of a non-negative value in `radix`, zero-padded to `width`.
#[inline]
#[must_use]
pub fn pack(value: &Magnus, radix: Radix, width: usize) -> String {
    format!("{:0>width$}", value.to_string_radix(radix))
}

/// The exact ordering of an integer against a float (`None` for NaN).
#[inline]
#[must_use]
pub fn cmp_float(int: &Magnus, float: f64) -> Option<Ordering> {
    if float.is_nan() {
        return None;
    }
    if float == f64::INFINITY {
        return Some(Ordering::Less);
    }
    if float == f64::NEG_INFINITY {
        return Some(Ordering::Greater);
    }
    let whole = Magnus::from_f64(float.trunc()).ok()?;
    Some(match int.cmp(&whole) {
        Ordering::Equal => {
            let fraction = float - float.trunc();
            if fraction > 0.0 {
                Ordering::Less
            } else if fraction < 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }
        other => other,
    })
}

/// The exact ordering of an integer against an unstored decimal carrier
/// (`(carrier, scale)`).
#[inline]
#[must_use]
pub fn cmp_dec(int: &Magnus, dec: (i128, u32)) -> Ordering {
    let scaled = int * &Magnus::from_i128(10_i128.pow(dec.1));
    scaled.cmp(&Magnus::from_i128(dec.0))
}

#[cfg(test)]
#[path = "inf_test.rs"]
mod tests;
