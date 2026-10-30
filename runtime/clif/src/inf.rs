//! The unbounded integer `inf`.
//!
//! A value is a handle to an immutable `Magnus`, the one bignum the compiler
//! ships (`crates/handroll/src/magnus.rs`, included by path so there is a
//! single implementation; the Rust runtime keeps a byte-identical copy).
//! Arithmetic is exact: `/` and `%` are the floor quotient and remainder, and
//! only a zero divisor fails.

use super::magnus::{Magnus, Radix};
use super::{exact_trap, text_new};
use crate::arena::ArenaPointer;
use std::cmp::Ordering;

fn make(arena: *const super::Arena, value: Magnus) -> *const u8 {
    arena.put(value).cast::<u8>()
}

/// # Safety
///
/// `handle` came from this module.
unsafe fn big<'a>(handle: *const u8) -> &'a Magnus {
    unsafe { &*handle.cast::<Magnus>() }
}

/// The bignum behind an inf handle.
///
/// # Safety
///
/// `handle` came from this module.
pub(crate) unsafe fn magnus<'a>(handle: *const u8) -> &'a Magnus {
    unsafe { big(handle) }
}

/// Order of two inf handles, for the element comparisons of the runtime.
///
/// # Safety
///
/// Both handles came from this module.
pub(crate) unsafe fn order(a: *const u8, b: *const u8) -> Ordering {
    unsafe { big(a).cmp(big(b)) }
}

/// Decimal text of an inf handle.
///
/// # Safety
///
/// `handle` came from this module.
pub(crate) unsafe fn text(handle: *const u8) -> String {
    unsafe { big(handle) }.to_string_radix(Radix::Dec)
}

/// An inf holding `value`, as the canonical element payload: an integer in the
/// `i64` range is returned as `Some(bits)` for the plain integer tag.
///
/// # Safety
///
/// `handle` came from this module.
pub(crate) unsafe fn as_i64(handle: *const u8) -> Option<i64> {
    unsafe { big(handle) }.to_i64()
}

/// An inf from an `i64`.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_inf_int(_arena: *const super::Arena, value: i64) -> *const u8 {
    make(_arena, Magnus::from_i64(value))
}

/// An inf from a 128-bit exact intermediate (low half, high half).
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_inf_wide(
    _arena: *const super::Arena,
    low: i64,
    high: i64,
) -> *const u8 {
    make(_arena, Magnus::from_i128(super::join_wide(low, high)))
}

/// An inf from its decimal digits (a constant).
///
/// # Safety
///
/// `digits` is a well-formed text handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_inf_digits(
    _arena: *const super::Arena,
    digits: *const u8,
) -> *const u8 {
    let source = super::text_str(unsafe { super::text_bytes(digits) });
    match Magnus::parse_decimal(&source) {
        Ok(value) => make(_arena, value),
        Err(_) => exact_trap("malformed integer constant"),
    }
}

/// `left op right` for `op` 0 `+`, 1 `-`, 2 `*`, 3 `/`, 4 `%`.
///
/// # Safety
///
/// Both handles came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_inf_binary(
    _arena: *const super::Arena,
    left: *const u8,
    right: *const u8,
    op: i32,
) -> *const u8 {
    let (a, b) = unsafe { (big(left), big(right)) };
    let value = match op {
        0 => a + b,
        1 => a - b,
        2 => a * b,
        3 => a
            .div_floor(b)
            .unwrap_or_else(|_| exact_trap("numerus division failed")),
        _ => a
            .mod_floor(b)
            .unwrap_or_else(|_| exact_trap("numerus division failed")),
    };
    make(_arena, value)
}

/// `-value`.
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_inf_neg(
    _arena: *const super::Arena,
    value: *const u8,
) -> *const u8 {
    let value = unsafe { big(value) };
    make(_arena, -value)
}

/// Three-way comparison: -1, 0 or 1.
///
/// # Safety
///
/// Both handles came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_inf_compare(
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

/// Decimal text of an inf.
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_inf_text(
    _arena: *const super::Arena,
    value: *const u8,
) -> *const u8 {
    text_new(_arena, unsafe { text(value) }.as_bytes())
}

/// The integer in `[low, high]`; parse status 2 (out of range) and 0 returned
/// when it does not fit.
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_inf_to_int(
    _arena: *const super::Arena,
    value: *const u8,
    low: i64,
    high: i64,
) -> i64 {
    let value = unsafe { big(value) };
    match value.to_i64() {
        Some(integer) if integer >= low && integer <= high => {
            super::PARSE_STATUS.store(0, std::sync::atomic::Ordering::Relaxed);
            integer
        }
        _ => {
            super::PARSE_STATUS.store(6, std::sync::atomic::Ordering::Relaxed);
            0
        }
    }
}

/// An inf as open data: a plain integer payload when it fits `i64`, else an
/// inf payload.
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_inf_box(
    _arena: *const super::Arena,
    value: *const u8,
) -> *const super::Present {
    let (tag, bits) = match unsafe { as_i64(value) } {
        Some(integer) => (super::TAG_INT, integer),
        None => (super::TAG_INF, value as i64),
    };
    super::__faber_clif_v1_opt_some(_arena, tag, bits)
}

/// The inf an open-data box holds (an integer or an inf payload), or null.
///
/// # Safety
///
/// `value` is null or a live boxed payload.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_inf_unbox(
    _arena: *const super::Arena,
    value: *const super::Present,
) -> *const u8 {
    match unsafe { value.as_ref() } {
        Some(present) if present.tag == super::TAG_INT => {
            make(_arena, Magnus::from_i64(present.bits))
        }
        Some(present) if present.tag == super::TAG_INF => present.bits as *const u8,
        _ => std::ptr::null(),
    }
}
