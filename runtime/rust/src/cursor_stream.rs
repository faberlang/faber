//! Cursor-stream materialization host binding (promotion packet P5).
//!
//! The shared `cursor_stream` host-ABI row
//! ([`crate::contract::abi::SYMBOL_CURSOR_STREAM`], `__faber_rt_v1_cursor_stream`)
//! materializes a generator into a `lista<T>`: the host invokes the generator
//! to completion and collects its `cede` yields. The generator's own return
//! value is discarded. Reference semantics: the MIR stepper's
//! `eval_cursor_stream`.
//!
//! The row's signature carries the generator function id:
//!
//! ```text
//! __faber_rt_v1_cursor_stream(i64 generator_function_id, ...generator_args)
//!   -> ptr   // materialized `lista<T>` handle
//! ```
//!
//! This module is the target-neutral materialization contract hosts implement
//! against. `materialize_cursor_stream` runs a generator-as-callback to
//! completion, routing each `cede` yield through [`CursorStreamSink`], and
//! returns the collected list.

use crate::contract::abi::SYMBOL_CURSOR_STREAM;

/// The `cede` yield sink a running generator reports each yielded value
/// through.
///
/// Mirrors the host side of a `cede` runtime call: every yield appends to
/// the materialized `lista<T>` in program order (append, never prepend, so
/// generator order matches the Rust iterator order).
#[derive(Debug)]
pub struct CursorStreamSink<'a, T> {
    yields: &'a mut Vec<T>,
}

impl<T> CursorStreamSink<'_, T> {
    /// Record one `cede` yield in the materialized list.
    pub fn cede(&mut self, value: T) {
        self.yields.push(value);
    }
}

/// Materialize a generator into a `Vec<T>` (the `lista<T>` carrier) by
/// running it to completion and collecting its `cede` yields.
///
/// `generator` is the generator-as-callback: the host invokes it once,
/// passing the yield sink; each `cede <expr>` inside the generator body
/// reports one yielded value through the sink. The generator's own return
/// value is discarded — the materialized list is the yield sequence alone
/// (reference: the stepper's `eval_cursor_stream`).
///
/// The `i64 generator_function_id` of the ABI signature selects the generator
/// wasm function; on the Rust host the callback plays that role.
pub fn materialize_cursor_stream<T, R>(
    generator: impl FnOnce(CursorStreamSink<'_, T>) -> R,
) -> Vec<T> {
    let mut yields = Vec::new();
    let sink = CursorStreamSink {
        yields: &mut yields,
    };
    let _own_value = generator(sink);
    yields
}

/// The shared ABI symbol this binding implements.
#[must_use]
pub const fn cursor_stream_abi_symbol() -> &'static str {
    SYMBOL_CURSOR_STREAM
}

#[cfg(test)]
#[path = "cursor_stream_test.rs"]
mod tests;
