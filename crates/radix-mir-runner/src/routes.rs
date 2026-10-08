//! Raw frame routes served by the runner's real hosts (`Host::sermo_route`).
//!
//! A raw `call 'solum:mensura' (path) ↦ numerus` is an `ad` expression the
//! runner answers from its static table, `runtime:echo`, then the host. This
//! module is the host answer for the process-capability routes a Faber
//! application reaches without a norma wrapper. Each route reuses the native
//! kernel verb the interpreted-package bridge dispatches, so the two paths
//! share one implementation, including the 64 MiB `read_range` cap.
//!
//! | route               | kernel verb                 | opener                  |
//! | ------------------- | --------------------------- | ----------------------- |
//! | `solum:partem`      | `solum.read_range`          | `[via, initium, longitudo]` |
//! | `solum:mensura`     | `solum.byte_length`         | `via` or `[via]`        |
//! | `solum:hauri`       | `solum.read_bytes`          | `via` or `[via, max]`   |
//! | `solum:digestio`    | `solum.digest_file`         | `via` or `[via]`        |
//! | `consolum:dic`      | `consolum.write_partial`    | `text` or `[text]`      |
//! | `consolum:mone`     | `consolum.warn_line`        | `text` or `[text]`      |
//! | `processus:exi`     | `processus.exit_process`    | `code` or `[code]`      |
//!
//! The `http:*` family (`listen`, `accept`, `respond`, `respond_open`,
//! `respond_chunk`, `respond_finish`, `stop`) is served by `kernel/http.rs`
//! directly: listeners and connections are runner-thread state, not a
//! `KernelModule` verb.
//!
//! Unknown routes return `None` and stay unrouted (the router fails closed).

use super::{Host, RunnerError, Value, kernel};
use radix_lexer::Span;
use radix_mir::KernelModule;
use std::fs;

/// Answer a raw route, or `None` when the route is not served here.
///
/// `Some(Ok(items))` carries the content items; `Some(Err(data))` is an
/// `error` terminal whose data is the kernel error message.
pub fn serve_route<H: Host + ?Sized>(
    host: &mut H,
    route: &str,
    opener: &Value,
    span: Span,
) -> Option<Result<Vec<Value>, Value>> {
    if let Some(reply) = kernel::http::serve(route, opener, span) {
        return Some(match reply {
            Ok(Value::Unit | Value::Nil) => Ok(Vec::new()),
            Ok(value) => Ok(vec![value]),
            Err(error) => Err(Value::Str(error.message)),
        });
    }
    let (module, verb) = match route {
        "solum:partem" => (KernelModule::Solum, "read_range"),
        "solum:mensura" => (KernelModule::Solum, "byte_length"),
        "solum:hauri" => (KernelModule::Solum, "read_bytes"),
        "solum:digestio" => (KernelModule::Solum, "digest_file"),
        "consolum:dic" => (KernelModule::Consolum, "write_partial"),
        "consolum:mone" => (KernelModule::Consolum, "warn_line"),
        "processus:exi" => (KernelModule::Processus, "exit_process"),
        _ => return None,
    };
    let mut args = opener_args(opener);
    let reply = ceiling_check(route, &mut args, span)
        .and_then(|()| kernel::dispatch_kernel(host, module, verb, &args, span));
    Some(match reply {
        Ok(Value::Unit | Value::Nil) => Ok(Vec::new()),
        Ok(value) => Ok(vec![value]),
        Err(error) => Err(Value::Str(error.message)),
    })
}

/// The opener is a bare valor or a `lista<valor>` of positional arguments.
fn opener_args(opener: &Value) -> Vec<Value> {
    match opener {
        Value::Array(items) => items.borrow().clone(),
        other => vec![other.clone()],
    }
}

/// `solum:hauri` takes an optional byte ceiling `[via, max]`: refuse a larger
/// file before reading it whole, then hand the kernel the path alone.
fn ceiling_check(route: &str, args: &mut Vec<Value>, span: Span) -> Result<(), RunnerError> {
    if route != "solum:hauri" || args.len() != 2 {
        return Ok(());
    }
    let (Value::Str(path), Value::Int(max)) = (&args[0], &args[1]) else {
        return Err(RunnerError::internal(
            span,
            "solum.read_bytes expects [textus, numerus]",
        ));
    };
    let max = u64::try_from(*max)
        .map_err(|_| RunnerError::internal(span, "solum.read_bytes max must be non-negative"))?;
    let size = fs::metadata(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.read_bytes failed: {err}")))?
        .len();
    if size > max {
        return Err(RunnerError::internal(
            span,
            format!("solum.read_bytes file is {size} bytes, exceeding the {max}-byte ceiling"),
        ));
    }
    args.truncate(1);
    Ok(())
}
