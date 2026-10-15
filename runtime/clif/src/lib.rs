//! `runtime/clif` — the public host runtime a CLIF-compiled Faber program
//! links (goal `docs/factory/radix-mir-clif/goal.md`, unit C1).
//!
//! Skeleton capability: the C entry point captures argv into the
//! process-global context, calls the program's `faber_clif_incipit`, prints
//! the scalar result, flushes, and exits with the runner's exit codes (D5:
//! 0 here, 1 on an uncaught failure once C12 wires the error channel).
//! Scalar print formats directly until C6 installs the full display path
//! with locale tokens from the descriptor; `radix-runtime-contract` supplies
//! the display helpers that exist there today.
//!
//! Faber programs on this target are single-threaded (`@ kernel` and device
//! blocks are declined), so the process-global context needs no locking.

use std::ffi::CStr;
use std::ffi::c_char;
use std::io::Write;
use std::sync::OnceLock;

// The program's `incipit`, emitted into the linked object by the backend.
unsafe extern "C" {
    fn faber_clif_incipit() -> i64;
}

/// Process-global runtime state, written once by [`main`].
struct Context {
    arguments: Vec<String>,
}

static CONTEXT: OnceLock<Context> = OnceLock::new();

/// The runtime's argument list for `argumenta` (unit C19).
///
/// # Errors
///
/// Returns an empty slice before [`main`] has run or after a capture failure.
pub fn arguments() -> &'static [String] {
    CONTEXT
        .get()
        .map_or(&[], |context| context.arguments.as_slice())
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

/// The runtime entry the emitted `main` shim calls (D3 naming: the ABI
/// symbol belongs to the pair; the backend emits the `main` symbol itself,
/// as the LLVM route does, so the archive never defines `main`).
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_main(
    argument_count: i32,
    argument_values: *const *const c_char,
) -> i32 {
    let _ = CONTEXT.set(Context {
        arguments: capture_arguments(argument_count, argument_values),
    });

    let result = unsafe { faber_clif_incipit() };

    let mut stdout = std::io::stdout();
    let line = format!("{result}\n");
    if stdout.write_all(line.as_bytes()).is_err() || stdout.flush().is_err() {
        return 1;
    }
    0
}
