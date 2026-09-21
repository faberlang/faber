//! Best-effort buffered output helpers for CLI rendering paths.

use std::fmt::Write;
use std::io;

/// Append one line to an in-memory explain buffer without propagating write errors.
pub(crate) fn writeln_buf(out: &mut String, args: impl std::fmt::Display) {
    writeln!(out, "{args}").ok();
}

/// Write a REPL prompt and flush stdout without failing the session on broken pipes.
pub fn write_prompt<W: io::Write>(out: &mut W, prompt: &str) {
    write!(out, "{prompt}").ok();
    out.flush().ok();
}
