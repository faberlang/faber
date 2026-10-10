//! Consolum kernel module for the MIR runner.
//!
//! Owns the `consolum` kernel verbs that write to stdout/stderr, read stdin,
//! and probe TTY state. Unknown verbs fail closed. Part of the kernel module
//! dispatch handled by `super::dispatch_kernel`.

use super::super::{Host, RunnerError, Value};
use super::args::expect_str;
use super::solum::octeti_bytes;
use radix_lexer::Span;
use radix_mir::MirDiagnosticKind;

/// Hosts-provider bound: `consolum:hauri` rejects reads larger than 1 MiB.
const MAX_STDIN_READ_BYTES: i64 = 1024 * 1024;

pub(crate) fn dispatch<H: Host + ?Sized>(
    host: &mut H,
    verb: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, RunnerError> {
    match verb {
        "write_partial" => {
            let [msg] = args else {
                return Err(RunnerError::internal(
                    span,
                    "consolum.write_partial provider arity",
                ));
            };
            let msg = expect_str(msg, span, "consolum.write_partial message")?;
            host.write_stdout_raw(&msg)?;
            Ok(Value::Unit)
        }
        "write_bytes" => {
            let [data] = args else {
                return Err(RunnerError::internal(
                    span,
                    "consolum.write_bytes provider arity",
                ));
            };
            let bytes = expect_octeti(data, span, "consolum.write_bytes data")?;
            host.write_stdout_bytes(&bytes)?;
            Ok(Value::Unit)
        }
        "write_line" => {
            let [msg] = args else {
                return Err(RunnerError::internal(
                    span,
                    "consolum.write_line provider arity",
                ));
            };
            let msg = expect_str(msg, span, "consolum.write_line message")?;
            host.scribe(MirDiagnosticKind::Nota, &msg)?;
            Ok(Value::Unit)
        }
        "warn_line" => {
            let [msg] = args else {
                return Err(RunnerError::internal(
                    span,
                    "consolum.warn_line provider arity",
                ));
            };
            let msg = expect_str(msg, span, "consolum.warn_line message")?;
            host.scribe(MirDiagnosticKind::Mone, &msg)?;
            Ok(Value::Unit)
        }
        "debug_line" => {
            let [msg] = args else {
                return Err(RunnerError::internal(
                    span,
                    "consolum.debug_line provider arity",
                ));
            };
            let msg = expect_str(msg, span, "consolum.debug_line message")?;
            // Hosts provider writes `vide` on the same stderr-line path as `mone`.
            host.scribe(MirDiagnosticKind::Mone, &msg)?;
            Ok(Value::Unit)
        }
        "read_line" => {
            if !args.is_empty() {
                return Err(RunnerError::internal(
                    span,
                    "consolum.read_line provider arity",
                ));
            }
            Ok(Value::Str(host.read_line()?.unwrap_or_default()))
        }
        "read_bytes" => {
            let [length] = args else {
                return Err(RunnerError::internal(
                    span,
                    "consolum.read_bytes provider arity",
                ));
            };
            let length = expect_int(length, span, "consolum.read_bytes length")?;
            let length = bounded_stdin_len(length, span)?;
            Ok(Value::Octeti(host.read_bytes(length)?))
        }
        "stdin_is_tty" => {
            if !args.is_empty() {
                return Err(RunnerError::internal(
                    span,
                    "consolum.stdin_is_tty provider arity",
                ));
            }
            Ok(Value::Bool(host.stdin_is_tty()?))
        }
        "stdout_is_tty" => {
            if !args.is_empty() {
                return Err(RunnerError::internal(
                    span,
                    "consolum.stdout_is_tty provider arity",
                ));
            }
            Ok(Value::Bool(host.stdout_is_tty()?))
        }
        "stderr_is_tty" => {
            if !args.is_empty() {
                return Err(RunnerError::internal(
                    span,
                    "consolum.stderr_is_tty provider arity",
                ));
            }
            Ok(Value::Bool(host.stderr_is_tty()?))
        }
        other => Err(RunnerError::unsupported(
            span,
            &format!("consolum kernel verb `{other}`"),
        )),
    }
}

/// The bytes of an `octeti` argument: the byte value, or the `list<b8>`
/// array the retired-word spelling lowers to.
fn expect_octeti(value: &Value, span: Span, label: &str) -> Result<Vec<u8>, RunnerError> {
    octeti_bytes(value)
        .ok_or_else(|| RunnerError::internal(span, format!("{label} expects octeti")))
}

fn expect_int(value: &Value, span: Span, label: &str) -> Result<i64, RunnerError> {
    match value {
        Value::Int(n) => Ok(*n),
        _ => Err(RunnerError::internal(
            span,
            format!("{label} expects numerus"),
        )),
    }
}

fn bounded_stdin_len(value: i64, span: Span) -> Result<usize, RunnerError> {
    if value <= 0 {
        return Ok(0);
    }
    if value > MAX_STDIN_READ_BYTES {
        return Err(RunnerError::internal(
            span,
            format!("consolum.read_bytes length must be at most {MAX_STDIN_READ_BYTES} bytes"),
        ));
    }
    usize::try_from(value)
        .map_err(|_| RunnerError::internal(span, "consolum.read_bytes length is too large"))
}
