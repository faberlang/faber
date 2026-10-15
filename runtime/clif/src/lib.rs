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
