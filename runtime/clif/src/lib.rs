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

/// A present optional payload.
pub struct Present {
    tag: i32,
    bits: i64,
}

/// A runtime `genus` instance: its display name and named, tagged fields in
/// declaration order.
pub struct Record {
    name: *const u8,
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
        return format!("{name} {{}}");
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
pub unsafe extern "C" fn __faber_clif_v1_record_new(name: *const u8) -> *mut Record {
    Box::into_raw(Box::new(Record {
        name,
        fields: Vec::new(),
    }))
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
        _ => left.cmp(&right),
    }
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
