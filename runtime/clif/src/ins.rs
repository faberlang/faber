//! Instants (`instans`): a handle to an immutable `(nanoseconds, precision)`
//! pair from the shared runtime contract. Comparison is at the coarser of the
//! two precisions, as the oracle compares.

use super::text_new;
use crate::arena::ArenaPointer;
use radix_runtime_contract::instans::{Instans, InstansPraecisio};
use std::cmp::Ordering;

fn precision(code: i32) -> InstansPraecisio {
    match code {
        0 => InstansPraecisio::Secunda,
        1 => InstansPraecisio::Millisecunda,
        2 => InstansPraecisio::Microsecunda,
        _ => InstansPraecisio::Nanosecunda,
    }
}

fn make(arena: *const super::Arena, value: Instans) -> *const u8 {
    arena.put(value).cast::<u8>()
}

/// # Safety
///
/// `handle` came from this module.
unsafe fn get(handle: *const u8) -> Instans {
    unsafe { handle.cast::<Instans>().read_unaligned() }
}

/// Order of two instant handles, for the element comparisons of the runtime.
///
/// # Safety
///
/// Both handles came from this module.
pub(crate) unsafe fn order(a: *const u8, b: *const u8) -> Ordering {
    unsafe { get(a).cmp_at_coarser(get(b)) }
}

/// RFC 3339 text of an instant handle.
///
/// # Safety
///
/// `handle` came from this module.
pub(crate) unsafe fn text(handle: *const u8) -> String {
    unsafe { get(handle) }.to_rfc3339()
}

/// An instant from RFC 3339 text at `precision`; null when malformed.
///
/// # Safety
///
/// `text` is a well-formed text handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_ins_parse(
    arena: *const super::Arena,
    text: *const u8,
    code: i32,
) -> *const u8 {
    let source = super::text_str(unsafe { super::text_bytes(text) });
    Instans::from_rfc3339(&source, precision(code))
        .map_or(std::ptr::null(), |value| make(arena, value))
}

/// An instant from an epoch integer in the precision's native units.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_ins_from_epoch(
    arena: *const super::Arena,
    epoch: i64,
    code: i32,
) -> *const u8 {
    make(arena, Instans::from_epoch_integer(epoch, precision(code)))
}

/// An instant at another precision.
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_ins_retag(
    arena: *const super::Arena,
    value: *const u8,
    code: i32,
) -> *const u8 {
    make(
        arena,
        unsafe { get(value) }.ad_praecisionem(precision(code)),
    )
}

/// RFC 3339 text of an instant.
///
/// # Safety
///
/// `value` came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_ins_text(
    arena: *const super::Arena,
    value: *const u8,
) -> *const u8 {
    text_new(arena, unsafe { text(value) }.as_bytes())
}

/// Three-way comparison at the coarser precision: -1, 0, 1.
///
/// # Safety
///
/// Both handles came from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_ins_compare(
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

/// An instant as open data.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_ins_box(
    arena: *const super::Arena,
    value: *const u8,
) -> *const super::Present {
    super::__faber_clif_v1_opt_some(arena, super::TAG_INS, value as i64)
}

/// The instant an open-data box holds at `precision` (an instant, RFC 3339
/// text or an epoch integer), or null.
///
/// # Safety
///
/// `value` is null or a live boxed payload.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_ins_unbox(
    arena: *const super::Arena,
    value: *const super::Present,
    code: i32,
) -> *const u8 {
    let Some(present) = (unsafe { value.as_ref() }) else {
        return std::ptr::null();
    };
    let wanted = precision(code);
    match present.tag {
        super::TAG_INS => make(
            arena,
            unsafe { get(present.bits as *const u8) }.ad_praecisionem(wanted),
        ),
        super::TAG_TEXT => {
            let source = super::text_str(unsafe { super::text_bytes(present.bits as *const u8) });
            Instans::from_rfc3339(&source, wanted)
                .map_or(std::ptr::null(), |value| make(arena, value))
        }
        super::TAG_INT => make(arena, Instans::from_epoch_integer(present.bits, wanted)),
        _ => std::ptr::null(),
    }
}
