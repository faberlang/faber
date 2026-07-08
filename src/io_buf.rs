//! Best-effort buffered output helpers for CLI rendering paths.

use std::fmt::Write;
use std::io;

/// Append one line to an in-memory explain buffer without propagating write errors.
#[allow(dead_code)]
pub(crate) fn writeln_buf(out: &mut String, args: impl std::fmt::Display) {
    let _ = writeln!(out, "{args}");
}

/// Write a REPL prompt and flush stdout without failing the session on broken pipes.
#[allow(dead_code)]
pub(crate) fn write_prompt<W: io::Write>(out: &mut W, prompt: &str) {
    let _ = write!(out, "{prompt}");
    let _ = out.flush();
}
