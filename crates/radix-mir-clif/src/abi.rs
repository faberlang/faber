//! The generator side of the CLIF runtime ABI (goal `radix-mir-clif`, D3).
//!
//! One declaration table names every runtime symbol the generated code may
//! call, with its signature. `runtime/clif` must export exactly these names
//! (checked by `tests/abi_symbols.rs`); the generator imports a symbol only
//! through [`RuntimeSymbol`], so an emitted call outside the table cannot be
//! written.

/// A scalar ABI type; handles and pointers travel as the pointer type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiTy {
    I8,
    I32,
    I64,
    F64,
    Ptr,
}

/// One runtime symbol: C name, parameters, optional return.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeSymbol {
    pub name: &'static str,
    pub params: &'static [AbiTy],
    pub ret: Option<AbiTy>,
}

use AbiTy::{F64, I8, I32, I64, Ptr};

/// Print one signed integer line: (stream, value). Stream 0 is stdout
/// (`nota`, `scribe`), 1 is stderr (`vide`, `mone`).
pub const PRINT_I64: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_print_i64",
    params: &[I32, I64],
    ret: None,
};
/// Print one float line: (stream, value, f32 flag).
pub const PRINT_F64: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_print_f64",
    params: &[I32, F64, I32],
    ret: None,
};
/// Print one boolean line: (stream, value).
pub const PRINT_BOOL: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_print_bool",
    params: &[I32, I8],
    ret: None,
};
/// Print one text line: (stream, text handle).
pub const PRINT_TEXT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_print_text",
    params: &[I32, Ptr],
    ret: None,
};
/// Print one raw-bits line: (stream, zero-extended pattern, width in bits).
pub const PRINT_BITS: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_print_bits",
    params: &[I32, I64, I32],
    ret: None,
};
/// Render a raw-bits value as text: (pattern, width in bits) -> text handle.
pub const TEXT_FROM_BITS: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_from_bits",
    params: &[I64, I32],
    ret: Some(Ptr),
};
/// A list handle points at a runtime list of tagged elements; every element
/// carries one of these tags so the runtime can display and compare it.
pub const TAG_INT: i64 = 0;
pub const TAG_FLOAT: i64 = 1;
pub const TAG_BOOL: i64 = 2;
pub const TAG_TEXT: i64 = 3;
/// Raw-bits tags are `TAG_BITS + log2(width / 8)`: 4, 5, 6, 7.
pub const TAG_BITS: i64 = 4;
pub const TAG_LIST: i64 = 8;
pub const TAG_STRUCT: i64 = 9;
pub const TAG_OPT: i64 = 10;
pub const TAG_MAP: i64 = 11;

/// New empty map (flag 0) or set (flag 1).
pub const MAP_NEW: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_map_new",
    params: &[I32],
    ret: Some(Ptr),
};
/// Insert or replace: (map, key tag, key bits, value tag, value bits).
pub const MAP_SET: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_map_set",
    params: &[Ptr, I32, I64, I32, I64],
    ret: None,
};
/// Value bits for a key; a missing key ends the run.
pub const MAP_GET: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_map_get",
    params: &[Ptr, I32, I64],
    ret: Some(I64),
};
/// Value as an optional: present value or none.
pub const MAP_GET_OPT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_map_get_opt",
    params: &[Ptr, I32, I64],
    ret: Some(Ptr),
};
/// Element at an index as an optional: present element or none.
pub const LIST_GET_OPT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_get_opt",
    params: &[Ptr, I64],
    ret: Some(Ptr),
};
/// Character at an index as an optional: present text or none.
pub const TEXT_INDEX_OPT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_index_opt",
    params: &[Ptr, I64],
    ret: Some(Ptr),
};
/// A new list with one element appended.
pub const LIST_APPENDED: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_appended",
    params: &[Ptr, I32, I64],
    ret: Some(Ptr),
};
/// A new list of the clamped range `start..end`.
pub const LIST_SLICE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_slice",
    params: &[Ptr, I64, I64],
    ret: Some(Ptr),
};
/// A new text of the clamped character range `start..end`.
pub const TEXT_SLICE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_slice",
    params: &[Ptr, I64, I64],
    ret: Some(Ptr),
};
/// A new list: 0 take, 1 take-last, 2 drop-first, by count.
pub const LIST_WINDOW: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_window",
    params: &[Ptr, I32, I64],
    ret: Some(Ptr),
};
/// Remove and return the first element as an optional.
pub const LIST_REMOVE_FIRST: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_remove_first",
    params: &[Ptr],
    ret: Some(Ptr),
};
/// Remove and return the last element as an optional.
pub const LIST_REMOVE_LAST: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_remove_last",
    params: &[Ptr],
    ret: Some(Ptr),
};
/// Reverse a list in place.
pub const LIST_REVERSE_IN_PLACE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_reverse_in_place",
    params: &[Ptr],
    ret: None,
};
/// A transformed text: 0 uppercase, 1 lowercase, 2 trim.
pub const TEXT_MAP: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_map",
    params: &[Ptr, I32],
    ret: Some(Ptr),
};
/// A text test against a second text: 0 starts-with, 1 ends-with, 2 contains.
pub const TEXT_TEST: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_test",
    params: &[Ptr, Ptr, I32],
    ret: Some(I8),
};
/// A list of the pieces of a text between delimiters.
pub const TEXT_SPLIT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_split",
    params: &[Ptr, Ptr],
    ret: Some(Ptr),
};
/// A text with every occurrence of one text replaced.
pub const TEXT_REPLACE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_replace",
    params: &[Ptr, Ptr, Ptr],
    ret: Some(Ptr),
};
/// Checked sum of an integer list.
pub const LIST_SUM_I64: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_sum_i64",
    params: &[Ptr],
    ret: Some(I64),
};
/// Sum of a float list.
pub const LIST_SUM_F64: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_sum_f64",
    params: &[Ptr],
    ret: Some(F64),
};
/// Key membership.
pub const MAP_HAS: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_map_has",
    params: &[Ptr, I32, I64],
    ret: Some(I8),
};
/// Remove a key; true when it was present.
pub const MAP_REMOVE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_map_remove",
    params: &[Ptr, I32, I64],
    ret: Some(I8),
};
/// Entry count.
pub const MAP_LEN: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_map_len",
    params: &[Ptr],
    ret: Some(I64),
};
/// Keys as a new list.
pub const MAP_KEYS: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_map_keys",
    params: &[Ptr],
    ret: Some(Ptr),
};
/// Values as a new list.
pub const MAP_VALUES: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_map_values",
    params: &[Ptr],
    ret: Some(Ptr),
};
/// Print a map or set line.
pub const PRINT_MAP: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_print_map",
    params: &[I32, Ptr],
    ret: None,
};
/// Render a map or set as text.
pub const TEXT_FROM_MAP: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_from_map",
    params: &[Ptr],
    ret: Some(Ptr),
};

/// Box a present optional payload: (tag, bits) -> optional handle (null is none).
pub const OPT_SOME: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_opt_some",
    params: &[I32, I64],
    ret: Some(Ptr),
};
/// Payload bits of a present optional; none ends the run.
pub const OPT_BITS: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_opt_bits",
    params: &[Ptr],
    ret: Some(I64),
};
/// Payload bits, or the fallback bits when none: (optional, fallback).
pub const OPT_BITS_OR: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_opt_bits_or",
    params: &[Ptr, I64],
    ret: Some(I64),
};
/// Optional equality: both none, or both present with equal payloads.
pub const OPT_EQ: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_opt_eq",
    params: &[Ptr, Ptr],
    ret: Some(I8),
};
/// Print an optional line.
pub const PRINT_OPT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_print_opt",
    params: &[I32, Ptr],
    ret: None,
};
/// Render an optional as text.
pub const TEXT_FROM_OPT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_from_opt",
    params: &[Ptr],
    ret: Some(Ptr),
};

/// New empty record: (name text handle, variant flag). A variant with no
/// fields displays as its bare name.
pub const RECORD_NEW: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_record_new",
    params: &[Ptr, I32],
    ret: Some(Ptr),
};
/// Structural equality of two records or variants.
pub const RECORD_EQ: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_record_eq",
    params: &[Ptr, Ptr],
    ret: Some(I8),
};
/// Structural equality of two lists or tuples.
pub const LIST_EQ: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_eq",
    params: &[Ptr, Ptr],
    ret: Some(I8),
};
/// Set field `index`: (record, index, field-name text handle, tag, bits).
pub const RECORD_SET: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_record_set",
    params: &[Ptr, I64, Ptr, I32, I64],
    ret: None,
};
/// Field bits at `index`.
pub const RECORD_GET: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_record_get",
    params: &[Ptr, I64],
    ret: Some(I64),
};
/// Deep copy of a list or record: (tag, bits) -> bits.
pub const DUPLICATE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_duplicate",
    params: &[I32, I64],
    ret: Some(I64),
};
/// Print a record line.
pub const PRINT_RECORD: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_print_record",
    params: &[I32, Ptr],
    ret: None,
};
/// Render a record as text.
pub const TEXT_FROM_RECORD: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_from_record",
    params: &[Ptr],
    ret: Some(Ptr),
};

/// New empty list.
pub const LIST_NEW: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_new",
    params: &[],
    ret: Some(Ptr),
};
/// Append one element: (list, tag, bits).
pub const LIST_PUSH: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_push",
    params: &[Ptr, I32, I64],
    ret: None,
};
/// Append every element of the second list to the first.
pub const LIST_EXTEND: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_extend",
    params: &[Ptr, Ptr],
    ret: None,
};
/// Number of elements.
pub const LIST_LEN: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_len",
    params: &[Ptr],
    ret: Some(I64),
};
/// Element bits at an index; out of range aborts.
pub const LIST_GET: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_get",
    params: &[Ptr, I64],
    ret: Some(I64),
};
/// Overwrite the element at an index: (list, index, tag, bits).
pub const LIST_SET: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_set",
    params: &[Ptr, I64, I32, I64],
    ret: None,
};
/// Membership: (list, tag, bits) -> 0/1.
pub const LIST_CONTAINS: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_contains",
    params: &[Ptr, I32, I64],
    ret: Some(I8),
};
/// A new reversed copy.
pub const LIST_REVERSED: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_reversed",
    params: &[Ptr],
    ret: Some(Ptr),
};
/// A new sorted copy (ascending).
pub const LIST_SORTED: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_sorted",
    params: &[Ptr],
    ret: Some(Ptr),
};
/// A new copy ordered stably by a parallel list of keys.
pub const LIST_SORTED_BY_KEYS: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_sorted_by_keys",
    params: &[Ptr, Ptr],
    ret: Some(Ptr),
};
/// Print a list line.
pub const PRINT_LIST: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_print_list",
    params: &[I32, Ptr],
    ret: None,
};
/// Render a list as text.
pub const TEXT_FROM_LIST: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_from_list",
    params: &[Ptr],
    ret: Some(Ptr),
};
/// One character of a text: (text, index) -> new text handle. An index out of
/// range ends the run.
pub const TEXT_INDEX: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_index",
    params: &[Ptr, I64],
    ret: Some(Ptr),
};
/// Concatenate two texts: (left, right) -> new text handle.
pub const TEXT_CONCAT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_concat",
    params: &[Ptr, Ptr],
    ret: Some(Ptr),
};
/// Render an integer as text: value -> text handle.
pub const TEXT_FROM_I64: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_from_i64",
    params: &[I64],
    ret: Some(Ptr),
};
/// Render a float as text: (value, f32 flag) -> text handle.
pub const TEXT_FROM_F64: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_from_f64",
    params: &[F64, I32],
    ret: Some(Ptr),
};
/// Render a boolean as text with the program's output tokens.
pub const TEXT_FROM_BOOL: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_from_bool",
    params: &[I8],
    ret: Some(Ptr),
};
/// Text equality: (left, right) -> 0/1.
pub const TEXT_EQ: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_eq",
    params: &[Ptr, Ptr],
    ret: Some(I8),
};
/// Text ordering by bytes: (left, right) -> -1, 0 or 1.
pub const TEXT_CMP: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_cmp",
    params: &[Ptr, Ptr],
    ret: Some(I64),
};
/// Text length in characters.
pub const TEXT_LEN: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_len",
    params: &[Ptr],
    ret: Some(I64),
};
/// Abort with a text message (`abort: <message>` on stderr, exit 1). Never
/// returns.
pub const ABORT_TEXT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_abort_text",
    params: &[Ptr],
    ret: None,
};
/// Trap with a message: (pointer, length). Never returns.
pub const TRAP: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_trap",
    params: &[Ptr, I64],
    ret: None,
};
/// Trap on a failed implicit store: (value, suffix pointer, suffix length,
/// unsigned-slot flag). The runtime words it `error: <value><suffix>`, adding
/// the unsigned-slot note for a negative value.
pub const TRAP_STORE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_trap_store",
    params: &[I64, Ptr, I64, I8],
    ret: None,
};
/// Install the program's output tokens (true, false, none, tuple), each as
/// (pointer, length).
pub const SET_DISPLAY_TOKENS: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_set_display_tokens",
    params: &[Ptr, I64, Ptr, I64, Ptr, I64, Ptr, I64],
    ret: None,
};

/// Parse a text as an integer in a radix and range-check it against
/// `[low, high]`: (text, radix, low, high) -> value. The outcome is read
/// from [`PARSE_STATUS`] (0 ok, 1 malformed, 2 out of range).
pub const PARSE_INT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_parse_int",
    params: &[Ptr, I32, I64, I64],
    ret: Some(I64),
};
/// Parse a text as a float: (text) -> value. Status 0 ok, 3 malformed.
pub const PARSE_F64: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_parse_f64",
    params: &[Ptr],
    ret: Some(F64),
};
/// The outcome of the latest parse (see [`PARSE_INT`], [`PARSE_F64`]).
pub const PARSE_STATUS: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_parse_status",
    params: &[],
    ret: Some(I32),
};
/// The failure text of a nonzero parse status.
pub const CONVERT_MESSAGE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_convert_message",
    params: &[I32],
    ret: Some(Ptr),
};

/// Three-way compare of two tuples or lists: -1, 0 or 1.
pub const LIST_COMPARE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_compare",
    params: &[Ptr, Ptr],
    ret: Some(I64),
};
/// Set algebra into a new set: 0 union, 1 intersection, 2 difference,
/// 3 symmetric difference.
pub const SET_ALGEBRA: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_set_algebra",
    params: &[Ptr, Ptr, I32],
    ret: Some(Ptr),
};
/// Subset (mode 0) or superset (mode 1) test.
pub const SET_SUBSET: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_set_subset",
    params: &[Ptr, Ptr, I32],
    ret: Some(I8),
};
/// Scalar math: 0 exp, 1 ln, 2 log10, 3 sin, 4 cos, 5 tan.
pub const MATH_F64: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_math_f64",
    params: &[I32, F64],
    ret: Some(F64),
};
/// Float remainder with the sign of the dividend (`fmod`).
pub const FLOAT_REM: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_float_rem",
    params: &[F64, F64],
    ret: Some(F64),
};
/// Index of the largest (flag 1) or smallest (flag 0) element, as an optional.
pub const LIST_ARG_EXTREMUM: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_arg_extremum",
    params: &[Ptr, I32],
    ret: Some(Ptr),
};
/// The exact integer `≈`.
pub const APPROX_INT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_approx_int",
    params: &[I64, I64],
    ret: Some(I8),
};

/// A list into a new set (duplicates collapse).
pub const LIST_TO_SET: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_to_set",
    params: &[Ptr],
    ret: Some(Ptr),
};
/// A list of integer or raw-bits elements regrouped at another width through
/// the byte string they spell: (list, from bits, to bits, flags) -> list, or
/// null when the bytes do not divide into whole target elements. Flag 1 is
/// big-endian; bits 1 and 2 give the target kind (0 raw bits, 1 unsigned, 2
/// signed).
pub const LIST_REWIDTH: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_rewidth",
    params: &[Ptr, I32, I32, I32],
    ret: Some(Ptr),
};
/// A scalar read from a list of bytes in the byte order of a flag: (list,
/// byte count, flags) -> 64 bits. Flag 1 is big-endian, 2 sign-extends, 4
/// reads an IEEE float of that many bytes and returns it widened to f64 bits.
/// A list of the wrong length reads zeros past its end.
pub const LIST_READ_SCALAR: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_read_scalar",
    params: &[Ptr, I32, I32],
    ret: Some(I64),
};
/// A scalar written as bytes: (bits, byte count, flags) -> list of raw
/// bytes. Flag 1 is big-endian; flag 4 takes the bits as an f64 and writes an
/// IEEE float of that many bytes.
pub const SCALAR_BYTES: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_scalar_bytes",
    params: &[I64, I32, I32],
    ret: Some(Ptr),
};
/// Intersection (mode 0) or union (mode 1) of two integer intervals:
/// (mode, left, right) -> a boxed optional interval, null when the result is
/// empty (intersection) or the parts are disjoint (union).
pub const INTERVAL_COMBINE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_interval_combine",
    params: &[I32, Ptr, Ptr],
    ret: Some(Ptr),
};
/// A question about an integer interval and an integer: mode 0 length, 1
/// contains, 2 clamp of the value into the interval. (mode, interval, value)
/// -> integer.
pub const INTERVAL_QUERY: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_interval_query",
    params: &[I32, Ptr, I64],
    ret: Some(I64),
};
/// The integers of an interval, in order: interval -> list.
pub const INTERVAL_MATERIALIZE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_interval_materialize",
    params: &[Ptr],
    ret: Some(Ptr),
};
/// A copy of the range of a list an interval names; null when the range does
/// not lie fully inside the list: (list, interval) -> list.
pub const LIST_RANGE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_range",
    params: &[Ptr, Ptr],
    ret: Some(Ptr),
};
/// A copy of a list with every element tag replaced.
pub const LIST_RETAG: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_retag",
    params: &[Ptr, I32],
    ret: Some(Ptr),
};

/// Text to a list of UTF-8 (8), UTF-16 (16) or UTF-32 (32) units, tagged as
/// raw bits of that width: (text, width) -> list.
pub const TEXT_ENCODE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_encode",
    params: &[Ptr, I32],
    ret: Some(Ptr),
};
/// A non-negative integer as fixed-width zero-padded digits in radix 2, 8 or
/// 16 (lowercase): (value, width, radix) -> text. Null when the value is
/// negative or does not fit the width.
pub const TEXT_PACK: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_text_pack",
    params: &[I64, I64, I32],
    ret: Some(Ptr),
};
/// A list of unit patterns to text: mode 8, 16 or 32 decodes UTF-8, UTF-16 or
/// UTF-32, mode 1 narrows bytes to ascii. Null when the list is malformed.
pub const LIST_DECODE: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_list_decode",
    params: &[Ptr, I32],
    ret: Some(Ptr),
};

/// Whether a boxed union (or optional) payload carries a tag; null never does.
pub const UNION_HAS_TAG: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_union_has_tag",
    params: &[Ptr, I32],
    ret: Some(I8),
};
/// Whether a record is a `genus` instance of the named type.
pub const RECORD_IS_NAMED: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_record_is_named",
    params: &[Ptr, Ptr],
    ret: Some(I8),
};
/// Whether a boxed payload is a `genus` instance of the named type.
pub const UNION_IS_STRUCT: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_union_is_struct",
    params: &[Ptr, Ptr],
    ret: Some(I8),
};

/// The runtime entry the emitted `main` shim calls.
pub const MAIN: RuntimeSymbol = RuntimeSymbol {
    name: "__faber_clif_v1_main",
    params: &[I32, Ptr],
    ret: Some(I32),
};

/// Every symbol the generated code may import (the runtime must export all).
pub const RUNTIME_SYMBOLS: &[RuntimeSymbol] = &[
    MAIN,
    PRINT_I64,
    PRINT_F64,
    PRINT_BOOL,
    LIST_NEW,
    LIST_PUSH,
    LIST_EXTEND,
    LIST_LEN,
    LIST_GET,
    LIST_SET,
    LIST_CONTAINS,
    LIST_REVERSED,
    LIST_SORTED,
    LIST_SORTED_BY_KEYS,
    PRINT_LIST,
    TEXT_FROM_LIST,
    MAP_NEW,
    MAP_SET,
    MAP_GET,
    MAP_GET_OPT,
    LIST_GET_OPT,
    LIST_SUM_I64,
    LIST_SUM_F64,
    TEXT_MAP,
    TEXT_TEST,
    TEXT_SPLIT,
    TEXT_REPLACE,
    LIST_APPENDED,
    LIST_SLICE,
    TEXT_SLICE,
    LIST_WINDOW,
    LIST_REMOVE_FIRST,
    LIST_REMOVE_LAST,
    LIST_REVERSE_IN_PLACE,
    TEXT_INDEX_OPT,
    MAP_HAS,
    MAP_REMOVE,
    MAP_LEN,
    MAP_KEYS,
    MAP_VALUES,
    PRINT_MAP,
    TEXT_FROM_MAP,
    OPT_SOME,
    OPT_BITS,
    OPT_BITS_OR,
    OPT_EQ,
    PRINT_OPT,
    TEXT_FROM_OPT,
    DUPLICATE,
    RECORD_NEW,
    RECORD_EQ,
    LIST_EQ,
    RECORD_SET,
    RECORD_GET,
    PRINT_RECORD,
    TEXT_FROM_RECORD,
    PRINT_TEXT,
    PRINT_BITS,
    TEXT_FROM_BITS,
    TEXT_CONCAT,
    TEXT_FROM_I64,
    TEXT_FROM_F64,
    TEXT_FROM_BOOL,
    TEXT_EQ,
    TEXT_CMP,
    TEXT_LEN,
    TEXT_INDEX,
    ABORT_TEXT,
    TRAP,
    TRAP_STORE,
    PARSE_INT,
    PARSE_F64,
    PARSE_STATUS,
    CONVERT_MESSAGE,
    LIST_COMPARE,
    SET_ALGEBRA,
    SET_SUBSET,
    MATH_F64,
    FLOAT_REM,
    LIST_ARG_EXTREMUM,
    APPROX_INT,
    LIST_TO_SET,
    LIST_RETAG,
    LIST_READ_SCALAR,
    SCALAR_BYTES,
    INTERVAL_COMBINE,
    INTERVAL_QUERY,
    INTERVAL_MATERIALIZE,
    LIST_RANGE,
    LIST_REWIDTH,
    TEXT_ENCODE,
    TEXT_PACK,
    LIST_DECODE,
    UNION_HAS_TAG,
    UNION_IS_STRUCT,
    RECORD_IS_NAMED,
    SET_DISPLAY_TOKENS,
];

/// A text handle points at `[len: i64][bytes...]`; literals are static data in
/// that shape, runtime-built texts are leaked allocations of it.
pub const TEXT_HEADER: usize = 8;

/// The program entry the runtime calls (exported by generated code).
pub const PROGRAM_INCIPIT: &str = "faber_clif_incipit";
