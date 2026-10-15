//! `runtime/clif` — the public host runtime a CLIF-compiled Faber program
//! links (goal `docs/factory/radix-mir-clif/goal.md`).
//!
//! The C `main` is emitted into the program object (the LLVM route's
//! pattern), so this archive never defines `main` and links cleanly as an
//! rlib. The runtime captures argv, installs the program's output tokens,
//! runs the program's `faber_clif_incipit`, flushes, and exits with the
//! runner's codes (D5: 0 normally, 1 on an uncaught failure or a trap).
//! Display goes through `radix-runtime-contract`, the same helpers the MIR
//! runner uses, so scalar output cannot drift from the oracle.
//!
//! Faber programs on this target are single-threaded (`@ kernel` and device
//! blocks are declined), so the process-global state needs no locking beyond
//! what a `static` demands.

use std::ffi::CStr;
use std::ffi::c_char;
use std::io::Write;
use std::sync::Mutex;
use std::sync::OnceLock;

use radix_runtime_contract::display::{display_bivalens, display_fractus};
use radix_runtime_contract::display_tokens::DisplayTokens;

// The program's `incipit`, emitted into the linked object by the backend.
unsafe extern "C" {
    fn faber_clif_incipit();
}

/// Process-global runtime state, written once by [`__faber_clif_v1_main`].
struct Context {
    arguments: Vec<String>,
}

static CONTEXT: OnceLock<Context> = OnceLock::new();
static TOKENS: OnceLock<DisplayTokens> = OnceLock::new();
static OUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// The runtime's argument list for `argumenta`.
///
/// Empty before [`__faber_clif_v1_main`] has run.
pub fn arguments() -> &'static [String] {
    CONTEXT
        .get()
        .map_or(&[], |context| context.arguments.as_slice())
}

fn tokens() -> &'static DisplayTokens {
    TOKENS.get_or_init(|| DisplayTokens {
        true_: "true".to_owned(),
        false_: "false".to_owned(),
        none: "none".to_owned(),
        tuple: "tuple".to_owned(),
    })
}

fn capture_arguments(argument_count: i32, argument_values: *const *const c_char) -> Vec<String> {
    if argument_values.is_null() {
        return Vec::new();
    }
    (0..usize::try_from(argument_count).unwrap_or(0))
        .filter_map(|index| {
            let pointer = unsafe { *argument_values.add(index) };
            if pointer.is_null() {
                return None;
            }
            let bytes = unsafe { CStr::from_ptr(pointer) };
            bytes.to_str().ok().map(str::to_owned)
        })
        .collect()
}

/// Flush buffered stdout. Called before every exit path.
fn flush() {
    if let Ok(mut buffer) = OUT.lock() {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(&buffer);
        let _ = stdout.flush();
        buffer.clear();
    }
}

fn emit(stream: i32, line: &str) {
    if stream == 0 {
        if let Ok(mut buffer) = OUT.lock() {
            buffer.extend_from_slice(line.as_bytes());
            buffer.push(b'\n');
            if buffer.len() > 1 << 16 {
                let mut stdout = std::io::stdout();
                let _ = stdout.write_all(&buffer);
                buffer.clear();
            }
        }
    } else {
        let mut stderr = std::io::stderr();
        let _ = writeln!(stderr, "{line}");
    }
}

/// The runtime entry the emitted `main` shim calls.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_main(
    argument_count: i32,
    argument_values: *const *const c_char,
) -> i32 {
    let _ = CONTEXT.set(Context {
        arguments: capture_arguments(argument_count, argument_values),
    });
    unsafe { faber_clif_incipit() };
    flush();
    0
}

/// Install the program's output tokens: true, false, none, tuple.
///
/// # Safety
///
/// Each pointer addresses its length in readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_set_display_tokens(
    true_pointer: *const u8,
    true_length: i64,
    false_pointer: *const u8,
    false_length: i64,
    none_pointer: *const u8,
    none_length: i64,
    tuple_pointer: *const u8,
    tuple_length: i64,
) {
    let word = |pointer: *const u8, length: i64| -> String {
        let length = usize::try_from(length).unwrap_or(0);
        let bytes = unsafe { std::slice::from_raw_parts(pointer, length) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    let _ = TOKENS.set(DisplayTokens {
        true_: word(true_pointer, true_length),
        false_: word(false_pointer, false_length),
        none: word(none_pointer, none_length),
        tuple: word(tuple_pointer, tuple_length),
    });
}

/// Print one signed integer line (stream 0 stdout, 1 stderr).
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_print_i64(stream: i32, value: i64) {
    emit(stream, &value.to_string());
}

/// Print one float line; `narrow` selects the `f32` display contract.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_print_f64(stream: i32, value: f64, narrow: i32) {
    #[allow(clippy::cast_possible_truncation)]
    let text = if narrow == 0 {
        display_fractus(value)
    } else {
        display_fractus(value as f32)
    };
    emit(stream, &text);
}

/// Print one boolean line using the program's output tokens.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_print_bool(stream: i32, value: i8) {
    emit(stream, display_bivalens(value != 0, tokens()));
}

/// Text handles point at `[len: i64][bytes...]` (literals are static data in
/// that shape; computed texts are leaked allocations of it).
///
/// # Safety
///
/// `handle` addresses a well-formed text.
unsafe fn text_bytes<'a>(handle: *const u8) -> &'a [u8] {
    let length = unsafe { std::ptr::read_unaligned(handle.cast::<i64>()) };
    unsafe { std::slice::from_raw_parts(handle.add(8), usize::try_from(length).unwrap_or(0)) }
}

fn text_new(bytes: &[u8]) -> *const u8 {
    let mut buffer = Vec::with_capacity(8 + bytes.len());
    buffer.extend_from_slice(&i64::try_from(bytes.len()).unwrap_or(0).to_ne_bytes());
    buffer.extend_from_slice(bytes);
    Box::leak(buffer.into_boxed_slice()).as_ptr()
}

fn text_str(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

fn bits_text(pattern: i64, width: i32) -> String {
    let digits = usize::try_from(width / 4).unwrap_or(0);
    let pattern = u64::from_ne_bytes(pattern.to_ne_bytes());
    format!("0x{pattern:0digits$x}")
}

/// Print one raw-bits line: `0x` and lowercase hex digits at the full width.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_print_bits(stream: i32, pattern: i64, width: i32) {
    emit(stream, &bits_text(pattern, width));
}

/// Render a raw-bits value as text.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_text_from_bits(pattern: i64, width: i32) -> *const u8 {
    text_new(bits_text(pattern, width).as_bytes())
}

/// A runtime list: tagged 64-bit elements (see `abi::TAG_*` in the generator).
pub struct List {
    items: Vec<(i32, i64)>,
}

const TAG_FLOAT: i32 = 1;
const TAG_BOOL: i32 = 2;
const TAG_TEXT: i32 = 3;
const TAG_BITS: i32 = 4;
const TAG_LIST: i32 = 8;
const TAG_STRUCT: i32 = 9;
const TAG_OPT: i32 = 10;
const TAG_MAP: i32 = 11;

/// A runtime map (`set` false) or set (`set` true): tagged key/value entries
/// in insertion order.
pub struct Map {
    set: bool,
    entries: Vec<(i32, i64, i32, i64)>,
}

/// A present optional payload.
pub struct Present {
    tag: i32,
    bits: i64,
}

/// A runtime `genus` instance: its display name and named, tagged fields in
/// declaration order.
pub struct Record {
    name: *const u8,
    variant: bool,
    fields: Vec<(*const u8, i32, i64)>,
}

/// # Safety
///
/// `handle` is a list returned by `__faber_clif_v1_list_new` family calls.
unsafe fn list_ref<'a>(handle: *const List) -> &'a List {
    unsafe { &*handle }
}

/// # Safety
///
/// As [`list_ref`], and no other reference to the list is live.
#[allow(clippy::mut_from_ref)]
unsafe fn list_mut<'a>(handle: *mut List) -> &'a mut List {
    unsafe { &mut *handle }
}

fn list_new_from(items: Vec<(i32, i64)>) -> *mut List {
    Box::into_raw(Box::new(List { items }))
}

/// Display one element; text elements are quoted as the runner quotes them.
fn element_text(tag: i32, bits: i64) -> String {
    match tag {
        TAG_FLOAT => display_fractus(f64::from_bits(u64::from_ne_bytes(bits.to_ne_bytes()))),
        TAG_BOOL => display_bivalens(bits != 0, tokens()).to_owned(),
        TAG_TEXT => format!("{:?}", text_str(unsafe { text_bytes(bits as *const u8) })),
        TAG_LIST => list_text(unsafe { list_ref(bits as *const List) }),
        TAG_STRUCT => record_text(unsafe { &*(bits as *const Record) }),
        TAG_OPT => match unsafe { (bits as *const Present).as_ref() } {
            Some(present) => element_text(present.tag, present.bits),
            None => tokens().none.clone(),
        },
        TAG_MAP => map_text(unsafe { &*(bits as *const Map) }),
        tag if (TAG_BITS..TAG_BITS + 4).contains(&tag) => bits_text(bits, 8 << (tag - TAG_BITS)),
        _ => bits.to_string(),
    }
}

fn list_text(list: &List) -> String {
    let parts: Vec<String> = list
        .items
        .iter()
        .map(|&(tag, bits)| element_text(tag, bits))
        .collect();
    format!("[{}]", parts.join(", "))
}

fn record_text(record: &Record) -> String {
    let name = text_str(unsafe { text_bytes(record.name) }).into_owned();
    if record.fields.is_empty() {
        return if record.variant {
            name
        } else {
            format!("{name} {{}}")
        };
    }
    let parts: Vec<String> = record
        .fields
        .iter()
        .map(|&(field, tag, bits)| {
            format!(
                "{} = {}",
                text_str(unsafe { text_bytes(field) }),
                element_text(tag, bits)
            )
        })
        .collect();
    format!("{name} {{ {} }}", parts.join(", "))
}

fn duplicate(tag: i32, bits: i64) -> i64 {
    match tag {
        TAG_MAP => {
            let source = unsafe { &*(bits as *const Map) };
            let entries = source
                .entries
                .iter()
                .map(|&(kt, kb, vt, vb)| (kt, duplicate(kt, kb), vt, duplicate(vt, vb)))
                .collect();
            Box::into_raw(Box::new(Map {
                set: source.set,
                entries,
            })) as i64
        }
        TAG_OPT => match unsafe { (bits as *const Present).as_ref() } {
            Some(present) => Box::into_raw(Box::new(Present {
                tag: present.tag,
                bits: duplicate(present.tag, present.bits),
            })) as i64,
            None => 0,
        },
        TAG_LIST => {
            let source = unsafe { list_ref(bits as *const List) };
            let items = source
                .items
                .iter()
                .map(|&(tag, bits)| (tag, duplicate(tag, bits)))
                .collect();
            list_new_from(items) as i64
        }
        TAG_STRUCT => {
            let source = unsafe { &*(bits as *const Record) };
            let fields = source
                .fields
                .iter()
                .map(|&(name, tag, bits)| (name, tag, duplicate(tag, bits)))
                .collect();
            Box::into_raw(Box::new(Record {
                name: source.name,
                variant: source.variant,
                fields,
            })) as i64
        }
        _ => bits,
    }
}

/// Deep copy of a list or record value; other values copy as themselves.
///
/// # Safety
///
/// `bits` is a live handle of the kind `tag` names.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_duplicate(tag: i32, bits: i64) -> i64 {
    duplicate(tag, bits)
}

fn map_text(map: &Map) -> String {
    if map.set {
        let parts: Vec<String> = map
            .entries
            .iter()
            .map(|&(tag, bits, _, _)| element_text(tag, bits))
            .collect();
        return format!("[{}]", parts.join(", "));
    }
    let parts: Vec<String> = map
        .entries
        .iter()
        .map(|&(kt, kb, vt, vb)| {
            let key = if kt == TAG_TEXT {
                format!("\"{}\"", text_str(unsafe { text_bytes(kb as *const u8) }))
            } else {
                element_text(kt, kb)
            };
            let value = if vt == TAG_TEXT {
                text_str(unsafe { text_bytes(vb as *const u8) }).into_owned()
            } else {
                element_text(vt, vb)
            };
            format!("{key}: {value}")
        })
        .collect();
    format!("{{{}}}", parts.join(", "))
}

fn map_find(map: &Map, tag: i32, bits: i64) -> Option<usize> {
    map.entries
        .iter()
        .position(|&(kt, kb, _, _)| kt == tag && element_equal(tag, kb, bits))
}

/// New empty map (flag 0) or set (flag 1).
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_map_new(set: i32) -> *mut Map {
    Box::into_raw(Box::new(Map {
        set: set != 0,
        entries: Vec::new(),
    }))
}

/// Insert or replace an entry.
///
/// # Safety
///
/// `map` is a live runtime map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_map_set(
    map: *mut Map,
    key_tag: i32,
    key: i64,
    value_tag: i32,
    value: i64,
) {
    let map = unsafe { &mut *map };
    match map_find(map, key_tag, key) {
        Some(at) => map.entries[at] = (key_tag, key, value_tag, value),
        None => map.entries.push((key_tag, key, value_tag, value)),
    }
}

/// Value bits for a key; a missing key ends the run.
///
/// # Safety
///
/// `map` is a live runtime map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_map_get(map: *const Map, key_tag: i32, key: i64) -> i64 {
    let map = unsafe { &*map };
    match map_find(map, key_tag, key) {
        Some(at) => map.entries[at].3,
        None => unsafe { __faber_clif_v1_trap(c"key not found".as_ptr().cast(), 13) },
    }
}

/// Value as an optional.
///
/// # Safety
///
/// `map` is a live runtime map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_map_get_opt(
    map: *const Map,
    key_tag: i32,
    key: i64,
) -> *const Present {
    let map = unsafe { &*map };
    match map_find(map, key_tag, key) {
        Some(at) => {
            let (_, _, tag, bits) = map.entries[at];
            Box::into_raw(Box::new(Present { tag, bits }))
        }
        None => std::ptr::null(),
    }
}

/// Key membership.
///
/// # Safety
///
/// `map` is a live runtime map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_map_has(map: *const Map, key_tag: i32, key: i64) -> i8 {
    i8::from(map_find(unsafe { &*map }, key_tag, key).is_some())
}

/// Remove a key; true when it was present.
///
/// # Safety
///
/// `map` is a live runtime map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_map_remove(map: *mut Map, key_tag: i32, key: i64) -> i8 {
    let map = unsafe { &mut *map };
    match map_find(map, key_tag, key) {
        Some(at) => {
            map.entries.remove(at);
            1
        }
        None => 0,
    }
}

/// Entry count.
///
/// # Safety
///
/// `map` is a live runtime map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_map_len(map: *const Map) -> i64 {
    i64::try_from(unsafe { &*map }.entries.len()).unwrap_or(i64::MAX)
}

/// Keys as a new list.
///
/// # Safety
///
/// `map` is a live runtime map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_map_keys(map: *const Map) -> *mut List {
    let items = unsafe { &*map }
        .entries
        .iter()
        .map(|&(tag, bits, _, _)| (tag, bits))
        .collect();
    list_new_from(items)
}

/// Values as a new list.
///
/// # Safety
///
/// `map` is a live runtime map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_map_values(map: *const Map) -> *mut List {
    let items = unsafe { &*map }
        .entries
        .iter()
        .map(|&(_, _, tag, bits)| (tag, bits))
        .collect();
    list_new_from(items)
}

/// Print one map or set line.
///
/// # Safety
///
/// `map` is a live runtime map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_print_map(stream: i32, map: *const Map) {
    emit(stream, &map_text(unsafe { &*map }));
}

/// Render a map or set as text.
///
/// # Safety
///
/// `map` is a live runtime map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_from_map(map: *const Map) -> *const u8 {
    text_new(map_text(unsafe { &*map }).as_bytes())
}

/// Box a present optional payload.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_opt_some(tag: i32, bits: i64) -> *const Present {
    Box::into_raw(Box::new(Present { tag, bits }))
}

/// Payload bits of a present optional; none ends the run.
///
/// # Safety
///
/// `handle` is null or a present optional.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_opt_bits(handle: *const Present) -> i64 {
    match unsafe { handle.as_ref() } {
        Some(present) => present.bits,
        None => unsafe { __faber_clif_v1_trap(c"unwrapped a none value".as_ptr().cast(), 22) },
    }
}

/// Payload bits, or `fallback` when none.
///
/// # Safety
///
/// `handle` is null or a present optional.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_opt_bits_or(handle: *const Present, fallback: i64) -> i64 {
    unsafe { handle.as_ref() }.map_or(fallback, |present| present.bits)
}

/// Optional equality.
///
/// # Safety
///
/// Both are null or present optionals.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_opt_eq(left: *const Present, right: *const Present) -> i8 {
    match unsafe { (left.as_ref(), right.as_ref()) } {
        (None, None) => 1,
        (Some(a), Some(b)) => i8::from(a.tag == b.tag && element_equal(a.tag, a.bits, b.bits)),
        _ => 0,
    }
}

fn opt_top_text(handle: *const Present) -> String {
    match unsafe { handle.as_ref() } {
        None => tokens().none.clone(),
        Some(present) if present.tag == TAG_TEXT => {
            text_str(unsafe { text_bytes(present.bits as *const u8) }).into_owned()
        }
        Some(present) => element_text(present.tag, present.bits),
    }
}

/// Print one optional line.
///
/// # Safety
///
/// `handle` is null or a present optional.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_print_opt(stream: i32, handle: *const Present) {
    emit(stream, &opt_top_text(handle));
}

/// Render an optional as text.
///
/// # Safety
///
/// `handle` is null or a present optional.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_from_opt(handle: *const Present) -> *const u8 {
    text_new(opt_top_text(handle).as_bytes())
}

/// New empty record.
///
/// # Safety
///
/// `name` addresses a well-formed text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_record_new(name: *const u8, variant: i32) -> *mut Record {
    Box::into_raw(Box::new(Record {
        name,
        variant: variant != 0,
        fields: Vec::new(),
    }))
}

fn record_equal(left: &Record, right: &Record) -> bool {
    (unsafe { text_bytes(left.name) == text_bytes(right.name) })
        && left.fields.len() == right.fields.len()
        && left
            .fields
            .iter()
            .zip(&right.fields)
            .all(|(a, b)| a.1 == b.1 && element_equal(a.1, a.2, b.2))
}

/// Structural equality of two records or variants.
///
/// # Safety
///
/// Both are live runtime records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_record_eq(
    left: *const Record,
    right: *const Record,
) -> i8 {
    i8::from(record_equal(unsafe { &*left }, unsafe { &*right }))
}

/// Set (or extend to) field `index`.
///
/// # Safety
///
/// `record` is a live runtime record and `field` a well-formed text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_record_set(
    record: *mut Record,
    index: i64,
    field: *const u8,
    tag: i32,
    bits: i64,
) {
    let record = unsafe { &mut *record };
    let index = usize::try_from(index).unwrap_or(0);
    if record.fields.len() <= index {
        record.fields.resize(index + 1, (field, 0, 0));
    }
    record.fields[index] = (field, tag, bits);
}

/// Field bits at `index`.
///
/// # Safety
///
/// `record` is a live runtime record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_record_get(record: *const Record, index: i64) -> i64 {
    let record = unsafe { &*record };
    usize::try_from(index)
        .ok()
        .and_then(|at| record.fields.get(at))
        .map_or(0, |&(_, _, bits)| bits)
}

/// Print one record line.
///
/// # Safety
///
/// `record` is a live runtime record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_print_record(stream: i32, record: *const Record) {
    emit(stream, &record_text(unsafe { &*record }));
}

/// Render a record as text.
///
/// # Safety
///
/// `record` is a live runtime record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_from_record(record: *const Record) -> *const u8 {
    text_new(record_text(unsafe { &*record }).as_bytes())
}

/// New empty list.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_list_new() -> *mut List {
    list_new_from(Vec::new())
}

/// Append one element.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_push(list: *mut List, tag: i32, bits: i64) {
    unsafe { list_mut(list) }.items.push((tag, bits));
}

/// Append every element of `source` to `list`.
///
/// # Safety
///
/// Both are live runtime lists.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_extend(list: *mut List, source: *const List) {
    let copied = unsafe { list_ref(source) }.items.clone();
    unsafe { list_mut(list) }.items.extend(copied);
}

/// Number of elements.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_len(list: *const List) -> i64 {
    i64::try_from(unsafe { list_ref(list) }.items.len()).unwrap_or(i64::MAX)
}

fn index_out_of_bounds() -> ! {
    unsafe { __faber_clif_v1_trap(c"array index out of bounds".as_ptr().cast(), 25) }
}

/// Element bits at `index`; an index out of range ends the run.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_get(list: *const List, index: i64) -> i64 {
    let list = unsafe { list_ref(list) };
    match usize::try_from(index)
        .ok()
        .and_then(|at| list.items.get(at))
    {
        Some(&(_, bits)) => bits,
        None => index_out_of_bounds(),
    }
}

/// Overwrite the element at `index`; an index out of range ends the run.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_set(
    list: *mut List,
    index: i64,
    tag: i32,
    bits: i64,
) {
    let list = unsafe { list_mut(list) };
    match usize::try_from(index)
        .ok()
        .and_then(|at| list.items.get_mut(at))
    {
        Some(slot) => *slot = (tag, bits),
        None => index_out_of_bounds(),
    }
}

fn element_equal(tag: i32, left: i64, right: i64) -> bool {
    match tag {
        TAG_FLOAT => {
            f64::from_bits(u64::from_ne_bytes(left.to_ne_bytes()))
                == f64::from_bits(u64::from_ne_bytes(right.to_ne_bytes()))
        }
        TAG_TEXT => unsafe { text_bytes(left as *const u8) == text_bytes(right as *const u8) },
        TAG_STRUCT => unsafe {
            record_equal(&*(left as *const Record), &*(right as *const Record))
        },
        TAG_LIST => unsafe {
            let (left, right) = (
                list_ref(left as *const List),
                list_ref(right as *const List),
            );
            left.items.len() == right.items.len()
                && left
                    .items
                    .iter()
                    .zip(&right.items)
                    .all(|(a, b)| a.0 == b.0 && element_equal(a.0, a.1, b.1))
        },
        _ => left == right,
    }
}

/// Membership test.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_contains(
    list: *const List,
    tag: i32,
    bits: i64,
) -> i8 {
    i8::from(
        unsafe { list_ref(list) }
            .items
            .iter()
            .any(|&(item_tag, item)| item_tag == tag && element_equal(tag, item, bits)),
    )
}

/// A reversed copy.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_reversed(list: *const List) -> *mut List {
    let mut items = unsafe { list_ref(list) }.items.clone();
    items.reverse();
    list_new_from(items)
}

fn element_order(tag: i32, left: i64, right: i64) -> std::cmp::Ordering {
    match tag {
        TAG_FLOAT => f64::from_bits(u64::from_ne_bytes(left.to_ne_bytes()))
            .total_cmp(&f64::from_bits(u64::from_ne_bytes(right.to_ne_bytes()))),
        TAG_TEXT => unsafe { text_bytes(left as *const u8).cmp(text_bytes(right as *const u8)) },
        TAG_BITS..=7 => {
            u64::from_ne_bytes(left.to_ne_bytes()).cmp(&u64::from_ne_bytes(right.to_ne_bytes()))
        }
        TAG_LIST => unsafe {
            list_order(
                list_ref(left as *const List),
                list_ref(right as *const List),
            )
        },
        _ => left.cmp(&right),
    }
}

/// Lexicographic order of two lists (tuples compare the same way).
fn list_order(left: &List, right: &List) -> std::cmp::Ordering {
    for (a, b) in left.items.iter().zip(&right.items) {
        let ordering = element_order(a.0, a.1, b.1);
        if ordering.is_ne() {
            return ordering;
        }
    }
    left.items.len().cmp(&right.items.len())
}

/// Three-way compare of two tuples or lists: -1, 0 or 1.
///
/// # Safety
///
/// Both are live runtime lists.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_compare(
    left: *const List,
    right: *const List,
) -> i64 {
    list_order(unsafe { list_ref(left) }, unsafe { list_ref(right) }) as i64
}

/// An ascending sorted copy.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_sorted(list: *const List) -> *mut List {
    let mut items = unsafe { list_ref(list) }.items.clone();
    items.sort_by(|a, b| element_order(a.0, a.1, b.1));
    list_new_from(items)
}

/// A new copy ordered stably by a parallel list of keys; a length mismatch
/// ends the run.
///
/// # Safety
///
/// Both are live runtime lists.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_sorted_by_keys(
    list: *const List,
    keys: *const List,
) -> *mut List {
    let items = &unsafe { list_ref(list) }.items;
    let keys = &unsafe { list_ref(keys) }.items;
    if items.len() != keys.len() {
        unsafe { __faber_clif_v1_trap(c"sort_by_keys length mismatch".as_ptr().cast(), 28) }
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| element_order(keys[a].0, keys[a].1, keys[b].1));
    list_new_from(order.into_iter().map(|at| items[at]).collect())
}

/// Print one list line.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_print_list(stream: i32, list: *const List) {
    emit(stream, &list_text(unsafe { list_ref(list) }));
}

/// Render a list as text.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_from_list(list: *const List) -> *const u8 {
    text_new(list_text(unsafe { list_ref(list) }).as_bytes())
}

/// Print one text line (stream 0 stdout, 1 stderr).
///
/// # Safety
///
/// `handle` addresses a well-formed text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_print_text(stream: i32, handle: *const u8) {
    emit(stream, &text_str(unsafe { text_bytes(handle) }));
}

/// Concatenate two texts into a new text.
///
/// # Safety
///
/// Both handles address well-formed texts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_concat(
    left: *const u8,
    right: *const u8,
) -> *const u8 {
    let (left, right) = unsafe { (text_bytes(left), text_bytes(right)) };
    let mut joined = Vec::with_capacity(left.len() + right.len());
    joined.extend_from_slice(left);
    joined.extend_from_slice(right);
    text_new(&joined)
}

/// Render an integer as text.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_text_from_i64(value: i64) -> *const u8 {
    text_new(value.to_string().as_bytes())
}

/// Render a float as text; `narrow` selects the `f32` display contract.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_text_from_f64(value: f64, narrow: i32) -> *const u8 {
    #[allow(clippy::cast_possible_truncation)]
    let text = if narrow == 0 {
        display_fractus(value)
    } else {
        display_fractus(value as f32)
    };
    text_new(text.as_bytes())
}

/// Render a boolean as text with the program's output tokens.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_text_from_bool(value: i8) -> *const u8 {
    text_new(display_bivalens(value != 0, tokens()).as_bytes())
}

/// Text equality.
///
/// # Safety
///
/// Both handles address well-formed texts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_eq(left: *const u8, right: *const u8) -> i8 {
    i8::from(unsafe { text_bytes(left) == text_bytes(right) })
}

/// Text ordering by bytes: -1, 0 or 1.
///
/// # Safety
///
/// Both handles address well-formed texts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_cmp(left: *const u8, right: *const u8) -> i64 {
    match unsafe { text_bytes(left).cmp(text_bytes(right)) } {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Text length in characters.
///
/// # Safety
///
/// `handle` addresses a well-formed text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_len(handle: *const u8) -> i64 {
    let count = text_str(unsafe { text_bytes(handle) }).chars().count();
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// The character at `index`; an index out of range ends the run.
///
/// # Safety
///
/// `handle` addresses a well-formed text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_index(handle: *const u8, index: i64) -> *const u8 {
    let text = text_str(unsafe { text_bytes(handle) });
    match usize::try_from(index)
        .ok()
        .and_then(|at| text.chars().nth(at))
    {
        Some(found) => {
            let mut buffer = [0_u8; 4];
            text_new(found.encode_utf8(&mut buffer).as_bytes())
        }
        None => unsafe { __faber_clif_v1_trap(c"text index out of bounds".as_ptr().cast(), 24) },
    }
}

/// The character at `index` as an optional: none out of range.
///
/// # Safety
///
/// `handle` addresses a well-formed text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_index_opt(
    handle: *const u8,
    index: i64,
) -> *const Present {
    let text = text_str(unsafe { text_bytes(handle) });
    match usize::try_from(index)
        .ok()
        .and_then(|at| text.chars().nth(at))
    {
        Some(found) => {
            let mut buffer = [0_u8; 4];
            let character = text_new(found.encode_utf8(&mut buffer).as_bytes());
            Box::into_raw(Box::new(Present {
                tag: TAG_TEXT,
                bits: character as i64,
            }))
        }
        None => std::ptr::null(),
    }
}

/// The element at `index` as an optional: none out of range.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_get_opt(
    list: *const List,
    index: i64,
) -> *const Present {
    let list = unsafe { list_ref(list) };
    match usize::try_from(index)
        .ok()
        .and_then(|at| list.items.get(at))
    {
        Some(&(tag, bits)) => Box::into_raw(Box::new(Present { tag, bits })),
        None => std::ptr::null(),
    }
}

/// A new list with one element appended.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_appended(
    list: *const List,
    tag: i32,
    bits: i64,
) -> *mut List {
    let mut items = unsafe { list_ref(list) }.items.clone();
    items.push((tag, bits));
    list_new_from(items)
}

fn clamp_range(start: i64, end: i64, length: usize) -> (usize, usize) {
    let end = usize::try_from(end).unwrap_or(0).min(length);
    let start = usize::try_from(start).unwrap_or(0).min(end);
    (start, end)
}

/// A new list of the clamped range `start..end`.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_slice(
    list: *const List,
    start: i64,
    end: i64,
) -> *mut List {
    let items = &unsafe { list_ref(list) }.items;
    let (start, end) = clamp_range(start, end, items.len());
    list_new_from(items[start..end].to_vec())
}

/// A new text of the clamped character range `start..end`.
///
/// # Safety
///
/// `handle` addresses a well-formed text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_slice(
    handle: *const u8,
    start: i64,
    end: i64,
) -> *const u8 {
    let text = text_str(unsafe { text_bytes(handle) });
    let chars: Vec<char> = text.chars().collect();
    let (start, end) = clamp_range(start, end, chars.len());
    let sliced: String = chars[start..end].iter().collect();
    text_new(sliced.as_bytes())
}

/// A new list: 0 take, 1 take-last, 2 drop-first, by count.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_window(
    list: *const List,
    mode: i32,
    count: i64,
) -> *mut List {
    let items = &unsafe { list_ref(list) }.items;
    let length = items.len();
    let count = usize::try_from(count).unwrap_or(0);
    let (start, end) = match mode {
        0 => (0, count.min(length)),
        1 => (length.saturating_sub(count), length),
        _ => (count.min(length), length),
    };
    list_new_from(items[start..end].to_vec())
}

fn present_box(item: Option<(i32, i64)>) -> *const Present {
    match item {
        Some((tag, bits)) => Box::into_raw(Box::new(Present { tag, bits })),
        None => std::ptr::null(),
    }
}

/// Remove and return the first element as an optional.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_remove_first(list: *mut List) -> *const Present {
    let items = &mut unsafe { list_mut(list) }.items;
    present_box(if items.is_empty() {
        None
    } else {
        Some(items.remove(0))
    })
}

/// Remove and return the last element as an optional.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_remove_last(list: *mut List) -> *const Present {
    present_box(unsafe { list_mut(list) }.items.pop())
}

/// Reverse a list in place.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_reverse_in_place(list: *mut List) {
    unsafe { list_mut(list) }.items.reverse();
}

/// A transformed text: 0 uppercase, 1 lowercase, 2 trim.
///
/// # Safety
///
/// `handle` addresses a well-formed text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_map(handle: *const u8, mode: i32) -> *const u8 {
    let text = text_str(unsafe { text_bytes(handle) });
    let changed = match mode {
        0 => text.to_uppercase(),
        1 => text.to_lowercase(),
        _ => text.trim().to_owned(),
    };
    text_new(changed.as_bytes())
}

/// A text test: 0 starts-with, 1 ends-with.
///
/// # Safety
///
/// Both handles address well-formed texts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_test(
    left: *const u8,
    right: *const u8,
    mode: i32,
) -> i8 {
    let left = text_str(unsafe { text_bytes(left) });
    let right = text_str(unsafe { text_bytes(right) });
    i8::from(match mode {
        0 => left.starts_with(&*right),
        1 => left.ends_with(&*right),
        _ => left.contains(&*right),
    })
}

/// The pieces of a text between delimiters.
///
/// # Safety
///
/// Both handles address well-formed texts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_split(
    handle: *const u8,
    delimiter: *const u8,
) -> *mut List {
    let text = text_str(unsafe { text_bytes(handle) });
    let delimiter = text_str(unsafe { text_bytes(delimiter) });
    let items = text
        .split(&*delimiter)
        .map(|part| (TAG_TEXT, text_new(part.as_bytes()) as i64))
        .collect();
    list_new_from(items)
}

/// A text with every occurrence of `from` replaced by `to`.
///
/// # Safety
///
/// All handles address well-formed texts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_replace(
    handle: *const u8,
    from: *const u8,
    to: *const u8,
) -> *const u8 {
    let text = text_str(unsafe { text_bytes(handle) });
    let from = text_str(unsafe { text_bytes(from) });
    let to = text_str(unsafe { text_bytes(to) });
    text_new(text.replace(&*from, &to).as_bytes())
}

/// Checked sum of an integer list; an overflow ends the run.
///
/// # Safety
///
/// `list` is a live runtime list of integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_sum_i64(list: *const List) -> i64 {
    let mut total: i64 = 0;
    for &(_, bits) in &unsafe { list_ref(list) }.items {
        match total.checked_add(bits) {
            Some(next) => total = next,
            None => unsafe { __faber_clif_v1_trap(c"numerus overflow".as_ptr().cast(), 16) },
        }
    }
    total
}

/// Sum of a float list.
///
/// # Safety
///
/// `list` is a live runtime list of floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_sum_f64(list: *const List) -> f64 {
    unsafe { list_ref(list) }
        .items
        .iter()
        .map(|&(_, bits)| f64::from_bits(u64::from_ne_bytes(bits.to_ne_bytes())))
        .sum()
}

/// Abort with a text message, the runner's `abort: <message>` (D5).
///
/// # Safety
///
/// `handle` addresses a well-formed text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_abort_text(handle: *const u8) -> ! {
    flush();
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "abort: {}", text_str(unsafe { text_bytes(handle) }));
    std::process::exit(1);
}

/// Report a failed run and exit with the runner's code (D5).
///
/// # Safety
///
/// `message` is null or points to `length` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_trap(message: *const u8, length: i64) -> ! {
    flush();
    let length = usize::try_from(length).unwrap_or(0);
    let bytes = if message.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(message, length) }
    };
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "error: {}", String::from_utf8_lossy(bytes));
    std::process::exit(1);
}

/// Report a failed implicit store and exit with the runner's code (D5).
///
/// The message is `error: <value><suffix>`; the suffix is baked by the
/// compiler (`radix_mir::store_trap`). A negative value in an unsigned slot
/// adds the runner's note.
///
/// # Safety
///
/// `suffix` is null or points to `length` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_trap_store(
    value: i64,
    suffix: *const u8,
    length: i64,
    unsigned_slot: i8,
) -> ! {
    flush();
    let length = usize::try_from(length).unwrap_or(0);
    let bytes = if suffix.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(suffix, length) }
    };
    let mut message = format!("{value}{}", String::from_utf8_lossy(bytes));
    if value < 0 && unsigned_slot != 0 {
        message.push_str(" (a negative value cannot be stored in an unsigned slot)");
    }
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "error: {message}");
    std::process::exit(1);
}

/// The outcome of the latest parse: 0 ok, 1 malformed integer, 2 integer out
/// of range, 3 malformed float.
static PARSE_STATUS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Parse a text as an integer in `radix`; a malformed text or a value outside
/// `[low, high]` sets a nonzero status and returns 0.
///
/// # Safety
///
/// `text` is a well-formed text handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_parse_int(
    text: *const u8,
    radix: i32,
    low: i64,
    high: i64,
) -> i64 {
    let source = text_str(unsafe { text_bytes(text) });
    let radix = u32::try_from(radix).unwrap_or(10);
    let (status, value) = match i128::from_str_radix(&source, radix) {
        Err(_) => (1, 0),
        Ok(parsed) => {
            if parsed >= i128::from(low) && parsed <= i128::from(high) {
                (0, i64::try_from(parsed).unwrap_or(0))
            } else {
                (2, 0)
            }
        }
    };
    PARSE_STATUS.store(status, std::sync::atomic::Ordering::Relaxed);
    value
}

/// Parse a text as a float; a malformed text sets status 3 and returns 0.
///
/// # Safety
///
/// `text` is a well-formed text handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_parse_f64(text: *const u8) -> f64 {
    let source = text_str(unsafe { text_bytes(text) });
    let (status, value) = match source.parse::<f64>() {
        Ok(parsed) => (0, parsed),
        Err(_) => (3, 0.0),
    };
    PARSE_STATUS.store(status, std::sync::atomic::Ordering::Relaxed);
    value
}

/// The outcome of the latest parse.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_parse_status() -> i32 {
    PARSE_STATUS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The failure text of a nonzero parse status (the runner's wording).
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_convert_message(status: i32) -> *const u8 {
    let message = match status {
        2 => "textus to numerus conversion out of range",
        3 => "textus to fractus conversion failed",
        _ => "textus to numerus conversion failed",
    };
    text_new(message.as_bytes())
}

/// Set algebra into a new set: 0 union, 1 intersection, 2 difference,
/// 3 symmetric difference.
///
/// # Safety
///
/// Both are live runtime sets.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_set_algebra(
    left: *const Map,
    right: *const Map,
    op: i32,
) -> *mut Map {
    let (left, right) = unsafe { (&*left, &*right) };
    let has = |set: &Map, entry: &(i32, i64, i32, i64)| map_find(set, entry.0, entry.1).is_some();
    let mut entries: Vec<(i32, i64, i32, i64)> = Vec::new();
    match op {
        0 => {
            entries.extend(left.entries.iter().copied());
            entries.extend(right.entries.iter().filter(|e| !has(left, e)).copied());
        }
        1 => entries.extend(left.entries.iter().filter(|e| has(right, e)).copied()),
        2 => entries.extend(left.entries.iter().filter(|e| !has(right, e)).copied()),
        _ => {
            entries.extend(left.entries.iter().filter(|e| !has(right, e)).copied());
            entries.extend(right.entries.iter().filter(|e| !has(left, e)).copied());
        }
    }
    Box::into_raw(Box::new(Map { set: true, entries }))
}

/// Subset (mode 0: left within right) or superset (mode 1) test.
///
/// # Safety
///
/// Both are live runtime sets.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_set_subset(
    left: *const Map,
    right: *const Map,
    mode: i32,
) -> i8 {
    let (left, right) = unsafe { (&*left, &*right) };
    let (inner, outer) = if mode == 0 {
        (left, right)
    } else {
        (right, left)
    };
    i8::from(
        inner
            .entries
            .iter()
            .all(|entry| map_find(outer, entry.0, entry.1).is_some()),
    )
}

/// Scalar math the instruction set lacks: 0 exp, 1 ln, 2 log10, 3 sin,
/// 4 cos, 5 tan (the same libm calls the runner makes).
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_math_f64(op: i32, value: f64) -> f64 {
    match op {
        0 => value.exp(),
        1 => value.ln(),
        2 => value.log10(),
        3 => value.sin(),
        4 => value.cos(),
        _ => value.tan(),
    }
}

/// Index of the largest (flag 1) or smallest (flag 0) element as an
/// optional; NaN never replaces a number and the first of equals wins.
///
/// # Safety
///
/// `list` is a live runtime list of integers or floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_arg_extremum(
    list: *const List,
    want_max: i32,
) -> *const Present {
    let items = &unsafe { list_ref(list) }.items;
    let mut winner: Option<usize> = None;
    for (index, &(tag, bits)) in items.iter().enumerate() {
        let Some(best) = winner else {
            winner = Some(index);
            continue;
        };
        let (best_tag, best_bits) = items[best];
        let replaces = if tag == 0 && best_tag == 0 {
            if want_max != 0 {
                bits > best_bits
            } else {
                bits < best_bits
            }
        } else if tag == TAG_FLOAT && best_tag == TAG_FLOAT {
            let best = f64::from_bits(u64::from_ne_bytes(best_bits.to_ne_bytes()));
            let value = f64::from_bits(u64::from_ne_bytes(bits.to_ne_bytes()));
            if best.is_nan() {
                !value.is_nan()
            } else if value.is_nan() {
                false
            } else if want_max != 0 {
                value > best
            } else {
                value < best
            }
        } else {
            unsafe {
                __faber_clif_v1_trap(c"argmax/argmin receiver type mismatch".as_ptr().cast(), 36)
            }
        };
        if replaces {
            winner = Some(index);
        }
    }
    match winner {
        Some(index) => Box::into_raw(Box::new(Present {
            tag: 0,
            bits: i64::try_from(index).unwrap_or(0),
        })),
        None => std::ptr::null(),
    }
}

/// The exact integer `≈`: `10^9 · |a − b| ≤ max(|a|, |b|)`.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_approx_int(left: i64, right: i64) -> i8 {
    let (x, y) = (i128::from(left), i128::from(right));
    i8::from(1_000_000_000 * (x - y).abs() <= x.abs().max(y.abs()))
}

/// A list into a new set; equal elements collapse to the first.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_to_set(list: *const List) -> *mut Map {
    let mut set = Map {
        set: true,
        entries: Vec::new(),
    };
    for &(tag, bits) in &unsafe { list_ref(list) }.items {
        if map_find(&set, tag, bits).is_none() {
            set.entries.push((tag, bits, 0, 0));
        }
    }
    Box::into_raw(Box::new(set))
}

/// A copy of a list whose elements keep their bits under a new tag.
///
/// # Safety
///
/// `list` is a live runtime list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_retag(list: *const List, tag: i32) -> *mut List {
    list_new_from(
        unsafe { list_ref(list) }
            .items
            .iter()
            .map(|&(_, bits)| (tag, bits))
            .collect(),
    )
}

/// Text to a list of UTF-8, UTF-16 or UTF-32 units tagged as raw bits.
///
/// # Safety
///
/// `text` is a well-formed text handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_text_encode(text: *const u8, width: i32) -> *mut List {
    let source = text_str(unsafe { text_bytes(text) });
    let (tag, units): (i32, Vec<i64>) = match width {
        8 => (TAG_BITS, source.bytes().map(i64::from).collect()),
        16 => (TAG_BITS + 1, source.encode_utf16().map(i64::from).collect()),
        _ => (
            TAG_BITS + 2,
            source.chars().map(|ch| i64::from(u32::from(ch))).collect(),
        ),
    };
    list_new_from(units.into_iter().map(|unit| (tag, unit)).collect())
}

/// A list of unit patterns to text; null when the list does not decode.
///
/// # Safety
///
/// `list` is a live runtime list of raw-bits elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_list_decode(list: *const List, mode: i32) -> *const u8 {
    let units: Vec<u64> = unsafe { list_ref(list) }
        .items
        .iter()
        .map(|&(_, bits)| u64::from_ne_bytes(bits.to_ne_bytes()))
        .collect();
    let decoded: Option<String> = match mode {
        8 | 1 => units
            .iter()
            .map(|unit| u8::try_from(*unit).ok())
            .collect::<Option<Vec<_>>>()
            .and_then(|bytes| {
                if mode == 1 && !bytes.is_ascii() {
                    None
                } else {
                    String::from_utf8(bytes).ok()
                }
            }),
        16 => units
            .iter()
            .map(|unit| u16::try_from(*unit).ok())
            .collect::<Option<Vec<_>>>()
            .and_then(|words| String::from_utf16(&words).ok()),
        _ => units
            .iter()
            .map(|unit| u32::try_from(*unit).ok().and_then(char::from_u32))
            .collect::<Option<String>>(),
    };
    decoded.map_or(std::ptr::null(), |text| text_new(text.as_bytes()))
}

/// Whether a boxed payload carries `tag`; null (a none) never does.
///
/// # Safety
///
/// `handle` is null or a live boxed payload.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_union_has_tag(handle: *const Present, tag: i32) -> i8 {
    i8::from(unsafe { handle.as_ref() }.is_some_and(|present| present.tag == tag))
}

/// Whether a boxed payload is a `genus` instance named `name`.
///
/// # Safety
///
/// `handle` is null or a live boxed payload; `name` is a text handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_union_is_struct(
    handle: *const Present,
    name: *const u8,
) -> i8 {
    let Some(present) = (unsafe { handle.as_ref() }) else {
        return 0;
    };
    if present.tag != TAG_STRUCT {
        return 0;
    }
    let record = unsafe { &*(present.bits as *const Record) };
    i8::from(!record.variant && unsafe { text_bytes(record.name) == text_bytes(name) })
}
