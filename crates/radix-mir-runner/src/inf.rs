//! The unbounded integer `inf` in the runner (inf-delivery-spec rulings 36-38, 40, 42, 44-46).
//!
//! `inf` values are exact, so every operation here is total except the three
//! ruled failures — a zero divisor, a negative shift count / exponent, and
//! resource exhaustion (ruling 45).
//!
//! **Canonical carrier.** A value of `inf` type is held in the smallest runner
//! carrier that can represent it: `Int` when it fits `i64`, `UInt` when it fits
//! `u64`, and [`Value::Magnus`] only beyond `[i64::MIN, u64::MAX]`
//! ([`from_magnus`]). So an `inf` operand may arrive as any of the three
//! ([`to_magnus`] reads them all), a widening from a bounded integer is the
//! identity on the value, and equality, ordering, hashing and display never
//! see two spellings of one integer.
//!
//! **Resource contract (ruling 45).** The language has no upper bound; the
//! runner enforces two implementation ceilings and fails fast with
//! `runner_inf_size_exhausted` naming the operation instead of thrashing or
//! aborting on allocation:
//! - [`RUNNER_INF_BIT_CEILING`]: the bit length of any single `inf` value;
//! - [`RUNNER_INF_MUL_WORK_CEILING`]: the schoolbook multiply work (limb
//!   products) of one `*`. `Magnus` multiplies in O(n·m), so without this a
//!   runaway `potentia` (repeated squaring) would spend hours below the bit
//!   ceiling; it is a runner limit like the step budget, not a language rule.

use crate::RunnerError;
use crate::exact_int::int_value;
use crate::value::Value;
use handroll::{Magnus, MagnusError};
use radix_lexer::Span;
use radix_mir::MirBinOp;
use radix_types::NumericWidth;
use std::cmp::Ordering;
use std::rc::Rc;

/// Bit length above which a single `inf` value is refused (ruling 45: 2^31
/// bits = 256 MiB). Not a language constant.
pub const RUNNER_INF_BIT_CEILING: u64 = 1 << 31;

/// Limb products above which one `inf` multiplication is refused (see the
/// module docs). 2^27 is a few seconds of schoolbook work in a debug build;
/// it still admits a product of two 2^18-bit (8192-limb) values.
pub const RUNNER_INF_MUL_WORK_CEILING: u64 = 1 << 27;

/// Longest decimal integer token (digits, sign excluded) the runner reads into
/// an `inf` from text (ruling 45 on the parse path). Decimal parsing is
/// O(n^2): nine digits per limb multiply over a growing magnitude. The ceiling
/// is the largest power of two whose parse work stays within
/// [`RUNNER_INF_MUL_WORK_CEILING`] (a runner limit like the step budget, not a
/// language rule); it is far below [`RUNNER_INF_BIT_CEILING`] because the bit
/// ceiling alone would admit a token that parses for hours.
pub const RUNNER_INF_PARSE_DIGIT_CEILING: usize = 1 << 17;

/// Schoolbook limb operations of parsing `digits` decimal digits: one multiply
/// per nine-digit chunk over a magnitude that grows to `digits * log2(10) / 32`
/// limbs (halved for the average length; rounded up, with `log2(10) < 3.33`).
const fn parse_work(digits: u64) -> u64 {
    let chunks = digits.div_ceil(9);
    let limbs = (digits * 333).div_ceil(100 * 32);
    chunks * limbs / 2
}

const _: () =
    assert!(parse_work(RUNNER_INF_PARSE_DIGIT_CEILING as u64) <= RUNNER_INF_MUL_WORK_CEILING);

/// Stable identity of a runner `inf` resource fault (ruling 45).
pub const RUNNER_ISSUE_INF_SIZE_EXHAUSTED: &str = "runner_inf_size_exhausted";

const DIVISION: &str = "numerus division failed";
const NEGATIVE_SHIFT: &str = "negative shift count";

/// The resource fault of ruling 45, naming the operation that hit a ceiling.
pub(crate) fn exhausted(span: Span, operation: &str) -> RunnerError {
    let mut error = RunnerError::internal_with_issue(
        span,
        format!(
            "inf size exhausted in {operation}: the value would exceed the runner ceiling of \
             {RUNNER_INF_BIT_CEILING} bits"
        ),
        RUNNER_ISSUE_INF_SIZE_EXHAUSTED,
    );
    error
        .args
        .push(("operation".to_owned(), operation.to_owned()));
    error
}

/// The resource fault of ruling 45 for a multiplication whose schoolbook work
/// alone passes [`RUNNER_INF_MUL_WORK_CEILING`] (same identity, distinct text).
fn exhausted_work(span: Span) -> RunnerError {
    let mut error = RunnerError::internal_with_issue(
        span,
        format!(
            "inf size exhausted in multiplication: the work exceeds the runner ceiling of \
             {RUNNER_INF_MUL_WORK_CEILING} limb products"
        ),
        RUNNER_ISSUE_INF_SIZE_EXHAUSTED,
    );
    error
        .args
        .push(("operation".to_owned(), "multiplication".to_owned()));
    error
}

/// The exact value of any integer carrier.
pub(crate) fn to_magnus(value: &Value) -> Option<Magnus> {
    match value {
        Value::Int(v) => Some(Magnus::from_i64(*v)),
        Value::UInt(v) => Some(Magnus::from_u64(*v)),
        Value::Magnus(v) => Some((**v).clone()),
        _ => None,
    }
}

/// The canonical carrier of an exact `inf` value.
pub(crate) fn from_magnus(value: Magnus) -> Value {
    if let Some(v) = value.to_i64() {
        return Value::Int(v);
    }
    if let Some(v) = value.to_u64() {
        return Value::UInt(v);
    }
    Value::Magnus(Rc::new(value))
}

/// Map a `Magnus` failure onto the runner's trap for that class.
fn arithmetic_error(error: MagnusError, span: Span, operation: &str) -> RunnerError {
    match error {
        MagnusError::DivisionByZero => RunnerError::internal(span, DIVISION),
        MagnusError::NegativeShift => RunnerError::internal(span, NEGATIVE_SHIFT),
        MagnusError::NegativeExponent => {
            RunnerError::internal(span, "potentia exponent must not be negative")
        }
        MagnusError::CeilingExceeded | MagnusError::Unrepresentable => exhausted(span, operation),
        MagnusError::NotFinite | MagnusError::Malformed => {
            RunnerError::internal(span, "inf arithmetic operand is not an integer")
        }
    }
}

fn limbs(value: &Magnus) -> u64 {
    value.bit_len().div_ceil(32)
}

/// One exact `inf` binary operation: `+ - * / % ∧ ∨ ⊻ ⇐ ⇒`. Floor division and
/// remainder, unmasked shifts, infinite two's complement (ruling 36).
pub(crate) fn binop(
    op: MirBinOp,
    a: &Magnus,
    b: &Magnus,
    span: Span,
) -> Result<Value, RunnerError> {
    let grew = |value: Magnus, operation: &str| {
        if value.exceeds_ceiling(RUNNER_INF_BIT_CEILING) {
            Err(exhausted(span, operation))
        } else {
            Ok(from_magnus(value))
        }
    };
    match op {
        MirBinOp::Add => grew(a + b, "addition"),
        MirBinOp::Sub => grew(a - b, "subtraction"),
        MirBinOp::Mul => {
            if limbs(a).saturating_mul(limbs(b)) > RUNNER_INF_MUL_WORK_CEILING {
                return Err(exhausted_work(span));
            }
            a.mul_within(b, RUNNER_INF_BIT_CEILING)
                .map(from_magnus)
                .map_err(|error| arithmetic_error(error, span, "multiplication"))
        }
        MirBinOp::Div => a
            .div_floor(b)
            .map(from_magnus)
            .map_err(|error| arithmetic_error(error, span, "division")),
        MirBinOp::Mod => a
            .mod_floor(b)
            .map(from_magnus)
            .map_err(|error| arithmetic_error(error, span, "remainder")),
        MirBinOp::BitAnd => Ok(from_magnus(a.bit_and(b))),
        MirBinOp::BitOr => Ok(from_magnus(a.bit_or(b))),
        MirBinOp::BitXor => Ok(from_magnus(a.bit_xor(b))),
        MirBinOp::Shl | MirBinOp::Shr if b.is_negative() => {
            Err(RunnerError::internal(span, NEGATIVE_SHIFT))
        }
        MirBinOp::Shl => shift_left(a, b, span),
        MirBinOp::Shr => Ok(from_magnus(a.shr(b.to_u64().unwrap_or(u64::MAX)))),
        _ => Err(RunnerError::internal(span, "invalid integer operator")),
    }
}

fn shift_left(a: &Magnus, count: &Magnus, span: Span) -> Result<Value, RunnerError> {
    if a.is_zero() {
        return Ok(Value::Int(0));
    }
    let Some(count) = count.to_u64() else {
        return Err(exhausted(span, "shift"));
    };
    a.shl_within(count, RUNNER_INF_BIT_CEILING)
        .map(from_magnus)
        .map_err(|error| arithmetic_error(error, span, "shift"))
}

/// Exact unary `-`.
pub(crate) fn neg(value: &Magnus) -> Value {
    from_magnus(-value)
}

/// Exact `¬x`, that is `-x - 1`.
pub(crate) fn bit_not(value: &Magnus) -> Value {
    from_magnus(value.bit_not())
}

/// Integer-family `≈` (inf-ap): the exact form of the relative `1e-9`
/// tolerance, `10^9 · |a − b| ≤ max(|a|, |b|)`, in integers. It is the one
/// rule for every integer carrier pair — bounded, `u64` and `inf` alike — so
/// `i32 ≈ i64`, `i32 ≈ inf` and `inf ≈ inf` agree. `None` when either value
/// is not an integer carrier.
pub(crate) fn fuzzy_equal(a: &Value, b: &Value) -> Option<bool> {
    const SCALE: i128 = 1_000_000_000;
    if let (Some(x), Some(y)) = (int_value(a), int_value(b)) {
        // |x - y| < 2^65 and SCALE < 2^30, so the product fits `i128`.
        return Some(SCALE * (x - y).abs() <= x.abs().max(y.abs()));
    }
    let (x, y) = (to_magnus(a)?, to_magnus(b)?);
    let scaled = (&(&x - &y).abs()) * Magnus::from_i128(SCALE);
    Some(scaled <= x.abs().max(y.abs()))
}

/// Exact three-way comparison of an `inf` with a float (ruling 37): the
/// integer is never rounded to the float's width; `±∞` order above and below
/// every integer; `None` for NaN.
pub(crate) fn cmp_magnus_float(integer: &Magnus, float: f64) -> Option<Ordering> {
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
    Some(match integer.cmp(&whole) {
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

/// Exact three-way comparison of an `inf` with a decimal `scaled / scale`
/// (`scale` a positive power of ten): `integer * scale` against `scaled`.
pub(crate) fn cmp_magnus_scaled(integer: &Magnus, scaled: i128, scale: i128) -> Ordering {
    (integer * Magnus::from_i128(scale)).cmp(&Magnus::from_i128(scaled))
}

/// `f64` of an `inf`, rounded to odd at 53 bits: a correctly rounded
/// narrower float (`f32`) can then be taken from it without the double
/// rounding that nearest-even at both steps would cause. Beyond `f64` range
/// it is `±∞`.
fn to_f64_round_to_odd(value: &Magnus) -> f64 {
    let len = value.bit_len();
    if len <= 53 {
        return value.to_f64();
    }
    let sign = if value.is_negative() { -1.0 } else { 1.0 };
    if len > 1024 {
        return sign * f64::INFINITY;
    }
    let drop = len - 53;
    let magnitude = value.abs();
    let top = magnitude.shr(drop);
    let sticky = top.shl(drop).map_or(true, |rebuilt| rebuilt != magnitude);
    let mut mantissa = top.to_u64().unwrap_or(0);
    if sticky {
        mantissa |= 1;
    }
    // `drop <= 971`: the scale is a finite power of two, and the product is
    // exact (`mantissa < 2^53`).
    #[allow(clippy::cast_precision_loss)]
    let scaled = mantissa as f64 * f64::from_bits((1023 + drop) << 52);
    sign * scaled
}

/// `↦ f16/bf16/f32/f64` (ruling 40): round to nearest-even, and a magnitude
/// beyond the float's finite range yields `±∞` (the IEEE default policy).
///
/// The runner carries every float as `f64`; `f32` and `bf16` round through
/// `f32` exactly like the runner's other float-width arithmetic
/// (`round_float`), and `f16` saturates to `±∞` at 65520 (the round-to-nearest
/// boundary above 65504) and is exact below it.
pub(crate) fn to_float(value: &Magnus, width: NumericWidth) -> f64 {
    match width {
        NumericWidth::F32 | NumericWidth::BF16 => {
            #[allow(clippy::cast_possible_truncation)]
            let narrowed = to_f64_round_to_odd(value) as f32;
            f64::from(narrowed)
        }
        NumericWidth::F16 => {
            if value.abs().to_u64().is_none_or(|v| v >= 65_520) {
                return if value.is_negative() {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
            }
            value.to_f64()
        }
        _ => value.to_f64(),
    }
}

/// `↦ wrapping<W>`: reduce modulo 2^W into the representatives of `W`
/// (ruling 24); total.
pub(crate) fn wrap_to_width(value: &Magnus, width: NumericWidth) -> Value {
    let Some((lo, hi)) = width.integer_range() else {
        return from_magnus(value.clone());
    };
    let bits = (hi - lo + 1).trailing_zeros();
    let mask = if bits >= 128 {
        u128::MAX
    } else {
        (1_u128 << bits) - 1
    };
    let low = value.low_u128_wrapping() & mask;
    // Unsigned widths hold `low` as it is; signed widths reinterpret the top
    // bit as the sign.
    let reduced = if lo < 0 && low >> (bits - 1) == 1 {
        #[allow(clippy::cast_possible_wrap)]
        let signed = low as i128;
        signed - (1_i128 << bits)
    } else {
        #[allow(clippy::cast_possible_wrap)]
        let unsigned = low as i128;
        unsigned
    };
    crate::value::modular_word_result(reduced, width)
}

/// `↦ saturating<W>`: clamp to `W`'s range (ruling 24); total.
pub(crate) fn clamp_to_width(value: &Magnus, width: NumericWidth) -> Value {
    let Some((lo, hi)) = width.integer_range() else {
        return from_magnus(value.clone());
    };
    let clamped = match value.to_i128() {
        Some(exact) => exact.clamp(lo, hi),
        None if value.is_negative() => lo,
        None => hi,
    };
    crate::value::saturating_word_result(clamped, width)
}
