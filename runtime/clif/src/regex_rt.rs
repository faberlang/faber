//! The `regex` and `match` values: compiled programs and match records behind
//! runtime handles. The engine is the `regex` crate through
//! [`crate::regex_program`], the same code path as the MIR runner, so a
//! pattern is accepted, rejected and matched the same way.
//!
//! A regex handle points at a leaked [`RegexProgram`] and a match handle at a
//! leaked [`MatchValue`]; like text, nothing is reclaimed.

use super::{
    List, Present, TAG_MATCH, TAG_TEXT, exit_trap, list_new_from, list_ref, text_bytes, text_new,
    text_str,
};
use crate::regex_program::{self, MatchValue, RegexProgram};
use radix_runtime_contract::trap::TrapKind;
use std::sync::Mutex;

/// The rejection payload of the latest failed compile.
static REGEX_ERROR: Mutex<String> = Mutex::new(String::new());

fn leak<T>(value: T) -> *const T {
    Box::into_raw(Box::new(value))
}

/// # Safety
///
/// `handle` is a regex handle returned by this module.
unsafe fn program<'a>(handle: *const RegexProgram) -> &'a RegexProgram {
    unsafe { &*handle }
}

/// # Safety
///
/// `handle` is a match handle returned by this module.
unsafe fn found<'a>(handle: *const MatchValue) -> &'a MatchValue {
    unsafe { &*handle }
}

/// # Safety
///
/// `text` is a live runtime text.
unsafe fn text_of(text: *const u8) -> String {
    text_str(unsafe { text_bytes(text) }).into_owned()
}

fn some_text(text: Option<&str>) -> *const Present {
    match text {
        Some(text) => leak(Present {
            tag: TAG_TEXT,
            bits: text_new(text.as_bytes()) as i64,
        }),
        None => std::ptr::null(),
    }
}

/// Compile a pattern text; null on a rejected pattern, whose payload
/// [`__faber_clif_v1_regex_error`] returns.
///
/// # Safety
///
/// `pattern` is a live runtime text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_regex_compile(pattern: *const u8) -> *const RegexProgram {
    match RegexProgram::compile(&unsafe { text_of(pattern) }) {
        Ok(compiled) => leak(compiled),
        Err(payload) => {
            if let Ok(mut slot) = REGEX_ERROR.lock() {
                *slot = payload;
            }
            std::ptr::null()
        }
    }
}

/// The payload of the latest rejected pattern, as a text.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_regex_error() -> *const u8 {
    let payload = REGEX_ERROR
        .lock()
        .map(|slot| slot.clone())
        .unwrap_or_default();
    text_new(payload.as_bytes())
}

/// A regex literal: compiled once per evaluation; an invalid constant ends the
/// run, as in the runner.
///
/// # Safety
///
/// `pattern` is a live runtime text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_regex_literal(pattern: *const u8) -> *const RegexProgram {
    match RegexProgram::compile(&unsafe { text_of(pattern) }) {
        Ok(compiled) => leak(compiled),
        Err(payload) => {
            exit_trap(
                &format!("constant regex is invalid: {payload}"),
                TrapKind::Conversion,
            );
        }
    }
}

/// Whether the pattern matches anywhere in the text.
///
/// # Safety
///
/// Both handles are live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_regex_matches(
    regex: *const RegexProgram,
    text: *const u8,
) -> i8 {
    i8::from(unsafe { program(regex) }.is_match(&unsafe { text_of(text) }))
}

/// The leftmost-first match as a present optional, or null for none.
///
/// # Safety
///
/// Both handles are live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_regex_find(
    regex: *const RegexProgram,
    text: *const u8,
) -> *const Present {
    match unsafe { program(regex) }.find(&unsafe { text_of(text) }) {
        Some(value) => leak(Present {
            tag: TAG_MATCH,
            bits: leak(value) as i64,
        }),
        None => std::ptr::null(),
    }
}

/// Every non-overlapping match, left to right, as a list.
///
/// # Safety
///
/// Both handles are live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_regex_find_all(
    regex: *const RegexProgram,
    text: *const u8,
) -> *mut List {
    let items = unsafe { program(regex) }
        .find_all(&unsafe { text_of(text) })
        .into_iter()
        .map(|value| (TAG_MATCH, leak(value) as i64))
        .collect();
    list_new_from(items)
}

/// The pieces of the text between the matches, as a list of texts.
///
/// # Safety
///
/// Both handles are live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_regex_split(
    regex: *const RegexProgram,
    text: *const u8,
) -> *mut List {
    let items = unsafe { program(regex) }
        .split(&unsafe { text_of(text) })
        .into_iter()
        .map(|piece| (TAG_TEXT, text_new(piece.as_bytes()) as i64))
        .collect();
    list_new_from(items)
}

/// The text with every match replaced by the literal replacement.
///
/// # Safety
///
/// All three handles are live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_regex_replace(
    regex: *const RegexProgram,
    text: *const u8,
    replacement: *const u8,
) -> *const u8 {
    let replaced = unsafe { program(regex) }
        .replace_all_literal(&unsafe { text_of(text) }, &unsafe { text_of(replacement) });
    text_new(replaced.as_bytes())
}

/// The text with the i-th match replaced by the i-th replacement; a count
/// mismatch ends the run.
///
/// # Safety
///
/// All handles are live and `replacements` is a list of texts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_regex_replace_each(
    regex: *const RegexProgram,
    text: *const u8,
    replacements: *const List,
) -> *const u8 {
    let pieces: Vec<String> = unsafe { list_ref(replacements) }
        .items
        .iter()
        .map(|&(_, bits)| unsafe { text_of(bits as *const u8) })
        .collect();
    let Some(replaced) = unsafe { program(regex) }.replace_each(&unsafe { text_of(text) }, &pieces)
    else {
        exit_trap("replace-each count mismatch", TrapKind::ProgramLogic);
    };
    text_new(replaced.as_bytes())
}

/// The text of a match.
///
/// # Safety
///
/// `value` is a live match handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_match_text(value: *const MatchValue) -> *const u8 {
    text_new(unsafe { found(value) }.text.as_bytes())
}

/// The code-point offset where a match starts.
///
/// # Safety
///
/// `value` is a live match handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_match_start(value: *const MatchValue) -> i64 {
    i64::try_from(unsafe { found(value) }.start).unwrap_or(i64::MAX)
}

/// The code-point offset where a match ends.
///
/// # Safety
///
/// `value` is a live match handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_match_end(value: *const MatchValue) -> i64 {
    i64::try_from(unsafe { found(value) }.end).unwrap_or(i64::MAX)
}

/// A numbered group as a present text, or null when it is out of range or did
/// not take part.
///
/// # Safety
///
/// `value` is a live match handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_match_group(
    value: *const MatchValue,
    index: i64,
) -> *const Present {
    let group = usize::try_from(index)
        .ok()
        .and_then(|index| unsafe { found(value) }.group(index));
    some_text(group)
}

/// A named group as a present text, or null.
///
/// # Safety
///
/// `value` is a live match handle and `name` a live text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_match_named(
    value: *const MatchValue,
    name: *const u8,
) -> *const Present {
    some_text(unsafe { found(value) }.named(&unsafe { text_of(name) }))
}

/// A pattern text matching the given text literally.
///
/// # Safety
///
/// `text` is a live runtime text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_escape(text: *const u8) -> *const u8 {
    text_new(regex_program::escape(&unsafe { text_of(text) }).as_bytes())
}

/// The pattern of a regex, as a text (its display).
///
/// # Safety
///
/// `regex` is a live regex handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_regex_text(regex: *const RegexProgram) -> *const u8 {
    text_new(unsafe { program(regex) }.pattern().as_bytes())
}

/// Display of a regex element: its pattern.
pub(crate) fn regex_element_text(bits: i64) -> String {
    unsafe { program(bits as *const RegexProgram) }
        .pattern()
        .to_owned()
}

/// Display of a match element: its text.
pub(crate) fn match_element_text(bits: i64) -> String {
    unsafe { found(bits as *const MatchValue) }.text.clone()
}

/// Whether two regex elements hold the same pattern.
pub(crate) fn regex_element_equal(left: i64, right: i64) -> bool {
    unsafe {
        program(left as *const RegexProgram).pattern()
            == program(right as *const RegexProgram).pattern()
    }
}

/// Whether two match elements are equal by value.
pub(crate) fn match_element_equal(left: i64, right: i64) -> bool {
    unsafe { found(left as *const MatchValue) == found(right as *const MatchValue) }
}
