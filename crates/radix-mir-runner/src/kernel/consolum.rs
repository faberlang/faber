//! Consolum kernel module for the MIR runner.
//!
//! Owns the `consolum` kernel verbs (`dic`, `scribe`, `mone`) that write to
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
        "dic" => {
            let [msg] = args else {
                return Err(RunnerError::internal(span, "consolum.dic provider arity"));
            };
            let msg = expect_str(msg, span, "consolum.dic message")?;
            host.write_stdout_raw(&msg)?;
            Ok(Value::Unit)
        }
        "scribe" => {
            let [msg] = args else {
                return Err(RunnerError::internal(
                    span,
                    "consolum.scribe provider arity",
                ));
            };
            let msg = expect_str(msg, span, "consolum.scribe message")?;
            host.scribe(MirDiagnosticKind::Nota, &msg)?;
            Ok(Value::Unit)
        }
        "mone" => {
            let [msg] = args else {
                return Err(RunnerError::internal(span, "consolum.mone provider arity"));
            };
            let msg = expect_str(msg, span, "consolum.mone message")?;
            host.scribe(MirDiagnosticKind::Mone, &msg)?;
            Ok(Value::Unit)
        }
        other => Err(RunnerError::unsupported(
            span,
            &format!("consolum kernel verb `{other}`"),
        )),
    }
}
