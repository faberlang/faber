//! Exact decimal (`d64`) helpers for generated Rust (D11.7, delivery rulings
//! 19-20, decimal-widths D5).
//!
//! An unstored decimal is an `i128` carrier at its own scale, a `(i128, u32)`
//! pair. `+ -` align scales, `*` adds scales, `/` rounds half-even at the larger
//! operand scale, every intermediate is bounded by the 128-bit carrier, and only
//! the store ([`store`]) rounds half-even to scale 8 and traps outside the `i64`
//! carrier. A stored `d64` is an `i64` at scale 8 ([`of`] lifts it). Mirrors
//! `radix-mir-runner`'s `decimal` module, which is the golden authority.
//!
//! Round-half-even rescale ([`rhe`]) and decimal-text parse ([`try_parse`],
//! [`parse`]) mirror the checked carrier math of `radix_codegen_shared::decimal`
//! (the copy compile-time literal folding uses) with the same failure mode:
//! integer division truncates toward zero, so the remainder decides whether the
//! quotient moves away from zero; a midpoint moves only when the truncated
//! quotient is odd. The parse accumulates the mantissa (`mantissa*10+digit`) and
//! the exponent shift through checked arithmetic, and every overflow traps with
//! the compile-time tier's message shape ("decimal carrier is outside the
//! declared width"); a denominator beyond `i128` rounds to a reasoned zero.
//!
//! Generated code calls these as free functions, for example
//! `faber::dec::store(faber::dec::add(faber::dec::of(a), faber::dec::of(b)), "", "")`.

// WHY: the bodies are the verbatim decimal prelude the compiler used to emit
// into every generated file; the casts are the semantic ops of the carrier
// (`i128` carriers narrowed or widened on purpose), and the single-letter names
// are the arithmetic's own.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::manual_assert,
    clippy::manual_let_else,
    clippy::redundant_closure_for_method_calls
)]

/// A stored `d64` (an `i64` at scale 8) as an exact `(carrier, scale)` pair.
#[inline]
#[must_use]
pub fn of(value: i64) -> (i128, u32) {
    (i128::from(value), 8)
}

/// An integer as an exact `(carrier, scale)` pair at scale 0.
#[inline]
#[must_use]
pub fn int(value: i128) -> (i128, u32) {
    (value, 0)
}

#[inline]
#[track_caller]
fn pow10(digits: u32, message: &str) -> i128 {
    10i128
        .checked_pow(digits)
        .unwrap_or_else(|| panic!("{message}"))
}

#[inline]
fn norm(carrier: i128, scale: u32) -> (i128, u32) {
    let (mut carrier, mut scale) = (carrier, scale);
    while scale > 8 && carrier % 10 == 0 {
        carrier /= 10;
        scale -= 1;
    }
    (carrier, scale)
}

#[inline]
#[track_caller]
fn align(a: (i128, u32), b: (i128, u32)) -> (i128, i128, u32) {
    let scale = a.1.max(b.1);
    let up = |(carrier, from): (i128, u32)| {
        carrier
            .checked_mul(pow10(scale - from, "numerus overflow"))
            .unwrap_or_else(|| panic!("numerus overflow"))
    };
    (up(a), up(b), scale)
}

/// Exact sum, at the larger operand scale.
#[inline]
#[must_use]
#[track_caller]
pub fn add(a: (i128, u32), b: (i128, u32)) -> (i128, u32) {
    let (x, y, scale) = align(a, b);
    norm(
        x.checked_add(y)
            .unwrap_or_else(|| panic!("numerus overflow")),
        scale,
    )
}

/// Exact difference, at the larger operand scale.
#[inline]
#[must_use]
#[track_caller]
pub fn sub(a: (i128, u32), b: (i128, u32)) -> (i128, u32) {
    let (x, y, scale) = align(a, b);
    norm(
        x.checked_sub(y)
            .unwrap_or_else(|| panic!("numerus overflow")),
        scale,
    )
}

/// Exact product: the scales add.
#[inline]
#[must_use]
#[track_caller]
pub fn mul(a: (i128, u32), b: (i128, u32)) -> (i128, u32) {
    let carrier =
        a.0.checked_mul(b.0)
            .unwrap_or_else(|| panic!("numerus overflow"));
    let scale =
        a.1.checked_add(b.1)
            .unwrap_or_else(|| panic!("numerus overflow"));
    norm(carrier, scale)
}

/// Quotient rounded half-even at the larger operand scale.
#[inline]
#[must_use]
#[track_caller]
pub fn div(a: (i128, u32), b: (i128, u32)) -> (i128, u32) {
    if b.0 == 0 {
        panic!("numerus division failed");
    }
    let scale = a.1.max(b.1);
    let numerator =
        a.0.checked_mul(pow10(scale + b.1 - a.1, "numerus division failed"))
            .unwrap_or_else(|| panic!("numerus division failed"));
    norm(rhe(numerator, b.0), scale)
}

/// Floor remainder (F9 ruling 29): the sign of the divisor, exact on the carriers.
#[inline]
#[must_use]
#[track_caller]
pub fn rem(a: (i128, u32), b: (i128, u32)) -> (i128, u32) {
    let (x, y, scale) = align(a, b);
    if y == 0 {
        panic!("numerus division failed");
    }
    let r = x.checked_rem(y).unwrap_or(0);
    norm(
        if r != 0 && ((r < 0) != (y < 0)) {
            r + y
        } else {
            r
        },
        scale,
    )
}

/// Exact negation.
#[inline]
#[must_use]
#[track_caller]
pub fn neg(a: (i128, u32)) -> (i128, u32) {
    norm(
        a.0.checked_neg()
            .unwrap_or_else(|| panic!("numerus overflow")),
        a.1,
    )
}

/// Three-way comparison: `-1`, `0` or `1`. An operand too large to align
/// compares by its sign.
#[inline]
#[must_use]
pub fn cmp(a: (i128, u32), b: (i128, u32)) -> i32 {
    let scale = a.1.max(b.1);
    let up = |(carrier, from): (i128, u32)| {
        10i128
            .checked_pow(scale - from)
            .and_then(|factor| carrier.checked_mul(factor))
    };
    match (up(a), up(b)) {
        (Some(x), Some(y)) => x.cmp(&y) as i32,
        (None, _) => {
            if a.0 < 0 {
                -1
            } else {
                1
            }
        }
        (_, None) => {
            if b.0 < 0 {
                1
            } else {
                -1
            }
        }
    }
}

/// The display text of an exact decimal: trailing fraction zeros trimmed.
#[must_use]
pub fn fmt(a: (i128, u32)) -> String {
    let digits = a.1 as usize;
    let text = format!("{:0>width$}", a.0.unsigned_abs(), width = digits + 1);
    let (integer, fraction) = text.split_at(text.len() - digits);
    let fraction = fraction.trim_end_matches('0');
    let sign = if a.0 < 0 { "-" } else { "" };
    if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}.{fraction}")
    }
}

/// The display text of a list of stored `d64` values: `[a, b, c]`.
#[must_use]
pub fn fmt_list(values: &[i64]) -> String {
    let parts: Vec<String> = values.iter().map(|value| fmt(of(*value))).collect();
    format!("[{}]", parts.join(", "))
}

/// The display text of an optional stored `d64`: the value or `nihil`.
#[must_use]
pub fn fmt_option(value: Option<i64>) -> String {
    fmt_option_with(value, &crate::display::DisplayTokens::LATIN)
}

/// [`fmt_option`] with the module's output tokens: an absent value prints
/// `tokens.none`.
#[must_use]
pub fn fmt_option_with(value: Option<i64>, tokens: &crate::display::DisplayTokens) -> String {
    match value {
        Some(value) => fmt(of(value)),
        None => tokens.none.to_owned(),
    }
}

/// Round half-even to scale 8; `None` outside the `i64` carrier.
#[inline]
#[must_use]
pub fn try_store(a: (i128, u32)) -> Option<i64> {
    let rounded = if a.1 > 8 {
        rhe(a.0, 10i128.checked_pow(a.1 - 8)?)
    } else {
        a.0.checked_mul(10i128.checked_pow(8 - a.1)?)?
    };
    i64::try_from(rounded).ok()
}

/// The store of an exact decimal into a `d64` cell: rounds half-even to scale 8
/// and traps outside the `i64` carrier (`at` and `inferred` word the trap).
#[inline]
#[must_use]
#[track_caller]
pub fn store(a: (i128, u32), at: &str, inferred: &str) -> i64 {
    try_store(a).unwrap_or_else(|| panic!("{} does not fit in `d64` ({at}){inferred}", fmt(a)))
}

/// Round half-even to an integer; `None` when the scale overflows `i128`.
#[inline]
#[must_use]
pub fn round(a: (i128, u32)) -> Option<i128> {
    Some(rhe(a.0, 10i128.checked_pow(a.1)?))
}

/// The nearest `f64` of an exact decimal.
#[inline]
#[must_use]
pub fn f64(a: (i128, u32)) -> f64 {
    match 10i128.checked_pow(a.1) {
        Some(factor) => (a.0 as f64) / (factor as f64),
        None => (a.0 as f64) / 10f64.powi(a.1 as i32),
    }
}

/// Round-half-even division of two carriers. Every overflow traps with
/// "decimal carrier is outside the declared width".
#[inline]
#[must_use]
#[track_caller]
pub fn rhe(numerator: i128, denominator: i128) -> i128 {
    if denominator == 0 {
        panic!("numerus division failed");
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder == 0 {
        return quotient;
    }
    let remainder_abs = remainder.unsigned_abs();
    let denominator_abs = denominator.unsigned_abs();
    let twice_remainder = match remainder_abs.checked_mul(2) {
        Some(twice_remainder) => twice_remainder,
        None => panic!("decimal carrier is outside the declared width"),
    };
    let rounds = twice_remainder > denominator_abs
        || (twice_remainder == denominator_abs && quotient % 2 != 0);
    if !rounds {
        return quotient;
    }
    let step = if (numerator < 0) == (denominator < 0) {
        1
    } else {
        -1
    };
    match quotient.checked_add(step) {
        Some(quotient) => quotient,
        None => panic!("decimal carrier is outside the declared width"),
    }
}

/// Parse decimal digit text (`4.2`, `1e-3`, `42`) to the carrier scaled by
/// `scale_digits`, rounding half-even; the failures are the `Err` text.
pub fn try_parse(text: &str, scale_digits: u32) -> Result<i128, &'static str> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let mut mantissa: i128 = 0;
    let mut exp10: i32 = 0;
    let mut seen_digit = false;
    let mut rest = digits;
    while let Some(digit) = rest.chars().next().filter(|c| c.is_ascii_digit()) {
        let digit = i128::from(digit as u8 - b'0');
        mantissa = match mantissa
            .checked_mul(10)
            .and_then(|scaled| scaled.checked_add(digit))
        {
            Some(mantissa) => mantissa,
            None => return Err("decimal carrier is outside the declared width"),
        };
        seen_digit = true;
        rest = &rest[1..];
    }
    if let Some(frac) = rest.strip_prefix('.') {
        rest = frac;
        while let Some(digit) = rest.chars().next().filter(|c| c.is_ascii_digit()) {
            let digit = i128::from(digit as u8 - b'0');
            mantissa = match mantissa
                .checked_mul(10)
                .and_then(|scaled| scaled.checked_add(digit))
            {
                Some(mantissa) => mantissa,
                None => return Err("decimal carrier is outside the declared width"),
            };
            exp10 -= 1;
            seen_digit = true;
            rest = &rest[1..];
        }
    }
    if let Some(exp) = rest.strip_prefix(['e', 'E']) {
        let (sign, exp) = match exp.strip_prefix('-') {
            Some(e) => (-1i32, e),
            None => (1i32, exp.strip_prefix('+').unwrap_or(exp)),
        };
        let e: i32 = match exp.parse() {
            Ok(e) => e,
            Err(_) => return Err("textus to numerus conversion failed"),
        };
        exp10 = match sign
            .checked_mul(e)
            .and_then(|shift| exp10.checked_add(shift))
        {
            Some(exp10) => exp10,
            None => return Err("decimal carrier is outside the declared width"),
        };
    } else if !rest.is_empty() {
        return Err("textus to numerus conversion failed");
    }
    if !seen_digit {
        return Err("textus to numerus conversion failed");
    }
    let mantissa = if negative { -mantissa } else { mantissa };
    let shift = exp10 + scale_digits as i32;
    Ok(if shift >= 0 {
        let factor = match 10i128.checked_pow(shift as u32) {
            Some(factor) => factor,
            None => return Err("decimal carrier is outside the declared width"),
        };
        match mantissa.checked_mul(factor) {
            Some(carrier) => carrier,
            None => return Err("decimal carrier is outside the declared width"),
        }
    } else {
        match 10i128.checked_pow((-shift) as u32) {
            Some(divisor) => rhe(mantissa, divisor),
            // A decimal denominator beyond i128 cannot produce a
            // non-zero rounded carrier (the mantissa fits i128).
            None => 0,
        }
    })
}

/// [`try_parse`] that traps with the failure text.
#[inline]
#[must_use]
#[track_caller]
pub fn parse(text: &str, scale_digits: u32) -> i128 {
    match try_parse(text, scale_digits) {
        Ok(carrier) => carrier,
        Err(message) => panic!("{message}"),
    }
}

#[cfg(test)]
#[path = "dec_test.rs"]
mod tests;
