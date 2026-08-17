//! Consolum kernel module for the MIR runner.
//!
//! Owns the `consolum` kernel verbs (`write_partial`, `write_line`, `warn_line`) that write to
//! stdout or scribe diagnostics. Part of the kernel module dispatch handled by
//! `super::dispatch_kernel`.

use super::super::{Host, RunnerError, Value};
use super::args::expect_str;
use radix_lexer::Span;
use radix_mir::MirDiagnosticKind;

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
        other => Err(RunnerError::unsupported(
            span,
            &format!("consolum kernel verb `{other}`"),
        )),
    }
}
