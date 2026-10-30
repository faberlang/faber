//! Exact decimal values (`d64`).
//!
//! A decimal is a handle to an immutable `(carrier, scale)` pair: an `i128`
//! carrier at its own number of fraction digits. A stored `d64` is the carrier
//! at scale 8. The arithmetic mirrors the MIR runner's `decimal.rs`: `+ -`
//! align the scales and are exact, `*` is exact with the scales added, `/`
//! rounds half-even at the larger operand scale, and the value lands in a
//! `d64` slot only at a store (round half-even to scale 8, trap outside the
//! `i64` carrier). An exact result that is a scale-8 `i64` carrier is returned
//! as one, so a handle is the same value whatever computed it.

use super::{exact_trap, exit_trap, flush, text_new};
use crate::arena::ArenaPointer;
use radix_runtime_contract::trap::TrapKind;
use std::cmp::Ordering;

/// Fraction digits of a stored `d64`.
const STORED_SCALE: u32 = 8;

/// One decimal value.
pub(crate) struct Dec {
    pub(crate) carrier: i128,
    pub(crate) scale: u32,
}

fn overflow<T>() -> T {
    exact_trap("numerus overflow")
}

fn division<T>() -> T {
    exact_trap("numerus division failed")
}

fn pow10(digits: u32) -> Option<i128> {
    10_i128.checked_pow(digits)
}

/// The exact value of `carrier x 10^-scale`, as a scale-8 carrier when it is
/// exactly one.
fn canonical(mut carrier: i128, mut scale: u32) -> (i128, u32) {
    while scale > STORED_SCALE && carrier % 10 == 0 {
        carrier /= 10;
        scale -= 1;
    }
    if scale <= STORED_SCALE
        && let Some(factor) = pow10(STORED_SCALE - scale)
        && let Some(stored) = carrier.checked_mul(factor)
        && i64::try_from(stored).is_ok()
    {
        return (stored, STORED_SCALE);
    }
    (carrier, scale)
}

fn make(arena: *const super::Arena, carrier: i128, scale: u32) -> *const u8 {
    let (carrier, scale) = canonical(carrier, scale);
    arena.put(Dec { carrier, scale }).cast::<u8>()
}

/// # Safety
///
/// `handle` came from this module.
pub(crate) unsafe fn raw(handle: *const u8) -> (i128, u32) {
    unsafe { parts(handle) }
}

/// # Safety
///
/// `handle` came from this module.
unsafe fn parts(handle: *const u8) -> (i128, u32) {
    let dec = unsafe { handle.cast::<Dec>().read_unaligned() };
    (dec.carrier, dec.scale)
}

fn align(a: (i128, u32), b: (i128, u32)) -> (i128, i128, u32) {
    let scale = a.1.max(b.1);
    let up = |(carrier, from): (i128, u32)| -> i128 {
        if carrier == 0 {
            return 0;
        }
        pow10(scale - from)
            .and_then(|factor| carrier.checked_mul(factor))
            .unwrap_or_else(overflow)
    };
    (up(a), up(b), scale)
}

/// Quotient rounded half to even.
fn round_half_even_quotient(numerator: i128, denominator: i128) -> Option<i128> {
    if denominator == 0 {
        return None;
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder == 0 {
        return Some(quotient);
    }
    let twice_remainder = remainder.unsigned_abs().checked_mul(2)?;
    let denominator_abs = denominator.unsigned_abs();
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

fn floor_rem_checked(a: i128, b: i128) -> i128 {
    let r = a.checked_rem(b).unwrap_or(0);
    if r != 0 && ((r < 0) != (b < 0)) {
        r + b
    } else {
        r
    }
}

/// Shortest text of a decimal value: trailing fraction zeros are dropped.
pub(crate) fn format(carrier: i128, scale: u32) -> String {
    let sign = if carrier < 0 { "-" } else { "" };
    let magnitude = carrier.unsigned_abs();
    let digits = scale as usize;
    let text = format!("{magnitude:0>width$}", width = digits + 1);
    let (integer, fraction) = text.split_at(text.len() - digits);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}.{fraction}")
    }
}

fn compare(a: (i128, u32), b: (i128, u32)) -> Ordering {
    let scale = a.1.max(b.1);
    let up = |(carrier, from): (i128, u32)| {
        if carrier == 0 {
            return Some(0);
        }
        pow10(scale - from).and_then(|f| carrier.checked_mul(f))
    };
    match (up(a), up(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
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

/// Order of two decimal handles, for the element comparisons of the runtime.
///
/// # Safety
///
/// Both handles came from this module.
pub(crate) unsafe fn order(a: *const u8, b: *const u8) -> Ordering {
    compare(unsafe { parts(a) }, unsafe { parts(b) })
}

/// Text of a decimal handle.
///
/// # Safety
///
/// `handle` came from this module.
pub(crate) unsafe fn text(handle: *const u8) -> String {
    let (carrier, scale) = unsafe { parts(handle) };
    format(carrier, scale)
}

/// A stored carrier (scale 8) as a decimal.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_dec_stored(
    arena: *const super::Arena,
    carrier: i64,
) -> *const u8 {
    make(arena, i128::from(carrier), STORED_SCALE)
}

/// An integer (scale 0) as a decimal.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_dec_int(arena: *const super::Arena, value: i64) -> *const u8 {
    make(arena, i128::from(value), 0)
}

/// `left op right` for `op` 0 `+`, 1 `-`, 2 `*`, 3 `/`, 4 `%`.
///
/// # Safety
///
/// Both handles came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_dec_binary(
    arena: *const super::Arena,
    left: *const u8,
    right: *const u8,
    op: i32,
) -> *const u8 {
    let (a, b) = (unsafe { parts(left) }, unsafe { parts(right) });
    let (carrier, scale) = match op {
        0 => {
            let (x, y, scale) = align(a, b);
            (x.checked_add(y).unwrap_or_else(overflow), scale)
        }
        1 => {
            let (x, y, scale) = align(a, b);
            (x.checked_sub(y).unwrap_or_else(overflow), scale)
        }
        2 => (
            a.0.checked_mul(b.0).unwrap_or_else(overflow),
            a.1.checked_add(b.1).unwrap_or_else(overflow),
        ),
        3 => {
            if b.0 == 0 {
                division::<()>();
            }
            let scale = a.1.max(b.1);
            let numerator = pow10(scale + b.1 - a.1)
                .and_then(|factor| a.0.checked_mul(factor))
                .unwrap_or_else(division);
            let quotient = round_half_even_quotient(numerator, b.0).unwrap_or_else(division);
            (quotient, scale)
        }
        _ => {
            let (x, y, scale) = align(a, b);
            if y == 0 {
                division::<()>();
            }
            (floor_rem_checked(x, y), scale)
        }
    };
    make(arena, carrier, scale)
}

/// `-value`.
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_dec_neg(
    arena: *const super::Arena,
    value: *const u8,
) -> *const u8 {
    let (carrier, scale) = unsafe { parts(value) };
    make(arena, carrier.checked_neg().unwrap_or_else(overflow), scale)
}

/// Three-way comparison: -1, 0 or 1.
///
/// # Safety
///
/// Both handles came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_dec_compare(
    _arena: *const super::Arena,
    left: *const u8,
    right: *const u8,
) -> i32 {
    match unsafe { order(left, right) } {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// The value rounded half-even to scale 8, as a stored decimal; 0 when it is
/// outside the `i64` carrier (the caller then calls [`dec_trap_store`]).
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_dec_store(
    arena: *const super::Arena,
    value: *const u8,
) -> *const u8 {
    let (carrier, scale) = unsafe { parts(value) };
    let rounded = if scale > STORED_SCALE {
        pow10(scale - STORED_SCALE).and_then(|divisor| round_half_even_quotient(carrier, divisor))
    } else {
        pow10(STORED_SCALE - scale).and_then(|factor| carrier.checked_mul(factor))
    };
    match rounded.and_then(|rounded| i64::try_from(rounded).ok()) {
        Some(stored) => make(arena, i128::from(stored), STORED_SCALE),
        None => std::ptr::null(),
    }
}

/// Text of a decimal value.
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_dec_text(
    arena: *const super::Arena,
    value: *const u8,
) -> *const u8 {
    text_new(arena, unsafe { text(value) }.as_bytes())
}

/// End the run with a store trap for a decimal outside its slot.
///
/// # Safety
///
/// `value` came from this module; `suffix` is null or points to `length`
/// readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_dec_trap_store(
    _arena: *const super::Arena,
    value: *const u8,
    suffix: *const u8,
    length: i64,
) -> ! {
    flush();
    let length = usize::try_from(length).unwrap_or(0);
    let bytes = if suffix.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(suffix, length) }
    };
    let message = format!(
        "{}{}",
        unsafe { text(value) },
        String::from_utf8_lossy(bytes)
    );
    exit_trap(&message, TrapKind::ProgramLogic)
}

/// Exact order of an integer and a float: -1, 0, 1, or 2 when unordered.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_int_float_compare(
    _arena: *const super::Arena,
    int: i64,
    float: f64,
) -> i32 {
    if float.is_nan() {
        return 2;
    }
    if float >= 18_446_744_073_709_551_616.0 {
        return -1;
    }
    if float < -9_223_372_036_854_775_808.0 {
        return 1;
    }
    let floor = float.floor();
    #[allow(clippy::cast_possible_truncation)]
    let whole = floor as i128;
    match i128::from(int).cmp(&whole) {
        Ordering::Less => -1,
        Ordering::Greater => 1,
        Ordering::Equal => {
            if float > floor {
                -1
            } else {
                0
            }
        }
    }
}

fn set_status(status: i32) {
    super::PARSE_STATUS.store(status, std::sync::atomic::Ordering::Relaxed);
}

/// The scale-8 carrier of a decimal digit text (`4.2`, `1e-3`, `-7`):
/// `Err(1)` when the text is malformed, `Err(2)` when the rounded carrier does
/// not fit the `i64` slot.
fn text_carrier(text: &str) -> Result<i64, i32> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mut mantissa, mut exponent, mut seen) = (0_i128, 0_i32, false);
    let mut rest = digits;
    let mut take = |rest: &mut &str, fraction: bool| -> Option<()> {
        while let Some(digit) = rest.chars().next().filter(char::is_ascii_digit) {
            mantissa = mantissa
                .checked_mul(10)?
                .checked_add(i128::from(digit as u8 - b'0'))?;
            if fraction {
                exponent -= 1;
            }
            seen = true;
            *rest = &rest[1..];
        }
        Some(())
    };
    take(&mut rest, false).ok_or(1)?;
    if let Some(fraction) = rest.strip_prefix('.') {
        rest = fraction;
        take(&mut rest, true).ok_or(1)?;
    }
    if let Some(power) = rest.strip_prefix(['e', 'E']) {
        let (sign, power) = match power.strip_prefix('-') {
            Some(power) => (-1_i32, power),
            None => (1_i32, power.strip_prefix('+').unwrap_or(power)),
        };
        let power: i32 = power.parse().map_err(|_| 1)?;
        exponent = exponent
            .checked_add(sign.checked_mul(power).ok_or(1)?)
            .ok_or(1)?;
    } else if !rest.is_empty() {
        return Err(1);
    }
    if !seen {
        return Err(1);
    }
    let mantissa = if negative { -mantissa } else { mantissa };
    let shift = exponent.checked_add(8).ok_or(2)?;
    let carrier = if shift >= 0 {
        let factor = pow10(u32::try_from(shift).map_err(|_| 2)?).ok_or(2)?;
        mantissa.checked_mul(factor).ok_or(2)?
    } else {
        let power = u32::try_from(-i64::from(shift)).map_err(|_| 2)?;
        match pow10(power) {
            Some(divisor) => round_half_even_quotient(mantissa, divisor).ok_or(2)?,
            None => 0,
        }
    };
    i64::try_from(carrier).map_err(|_| 2)
}

fn from_carrier(arena: *const super::Arena, result: Result<i64, i32>) -> *const u8 {
    match result {
        Ok(carrier) => {
            set_status(0);
            make(arena, i128::from(carrier), STORED_SCALE)
        }
        Err(status) => {
            set_status(status);
            std::ptr::null()
        }
    }
}

/// An integer converted to a stored decimal; null (status 2) when the scaled
/// value does not fit.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_dec_from_int(
    arena: *const super::Arena,
    value: i64,
) -> *const u8 {
    from_carrier(
        arena,
        i128::from(value)
            .checked_mul(100_000_000)
            .and_then(|carrier| i64::try_from(carrier).ok())
            .ok_or(2),
    )
}

/// A float converted to a stored decimal through its shortest text; null
/// (status 1 or 2) when it is not a decimal number or does not fit.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_dec_from_float(
    arena: *const super::Arena,
    value: f64,
) -> *const u8 {
    from_carrier(arena, text_carrier(&value.to_string()))
}

/// A text converted to a stored decimal; null (status 1 or 2) when malformed
/// or out of range.
///
/// # Safety
///
/// `text` is a well-formed text handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_dec_from_text(
    arena: *const super::Arena,
    text: *const u8,
) -> *const u8 {
    let source = super::text_str(unsafe { super::text_bytes(text) });
    from_carrier(arena, text_carrier(&source))
}

/// A decimal as a float; `narrow` rounds it through `f32`.
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_dec_to_float(
    _arena: *const super::Arena,
    value: *const u8,
    narrow: i32,
) -> f64 {
    let (carrier, scale) = unsafe { parts(value) };
    #[allow(clippy::cast_precision_loss)]
    let float = (carrier as f64) / (pow10(scale).unwrap_or(1) as f64);
    if narrow == 0 {
        float
    } else {
        f64::from(float as f32)
    }
}

/// A decimal rounded to an integer in `[low, high]`: `mode` 0 floor, 1 ceil,
/// 2 half-even, 3 toward zero. Status 4 (out of range) or 5 (failed) and 0
/// returned when it does not fit.
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_dec_to_int(
    _arena: *const super::Arena,
    value: *const u8,
    mode: i32,
    low: i64,
    high: i64,
) -> i64 {
    let (carrier, scale) = unsafe { parts(value) };
    let Some(divisor) = pow10(scale) else {
        set_status(5);
        return 0;
    };
    let rounded = match mode {
        2 => round_half_even_quotient(carrier, divisor),
        3 => carrier.checked_div(divisor),
        _ => carrier.checked_div(divisor).and_then(|quotient| {
            if carrier.checked_rem(divisor)? == 0 {
                return Some(quotient);
            }
            // The truncated quotient sits toward zero; the exact value is above
            // it for a positive result and below it for a negative one.
            match (mode, carrier < 0) {
                (1, false) => quotient.checked_add(1),
                (0, true) => quotient.checked_sub(1),
                _ => Some(quotient),
            }
        }),
    };
    let Some(rounded) = rounded else {
        set_status(5);
        return 0;
    };
    if rounded < i128::from(low) || rounded > i128::from(high) {
        set_status(4);
        return 0;
    }
    set_status(0);
    i64::try_from(rounded).unwrap_or(0)
}
