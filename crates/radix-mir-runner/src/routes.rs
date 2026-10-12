//! Raw frame routes served by the runner's real hosts (`Host::sermo_route`).
//!
//! `solum:*` is the shared fs leaf (`faber-leaf`). Console and process
//! leftovers still reuse kernel verbs until those leaves land. HTTP stays
//! on `kernel/http.rs` until N5; A2 forbids restoring `hosts`.
//!
//! Unknown routes return `None` and stay unrouted (the router fails closed).

use super::{Host, Value, kernel};
use faber_leaf::fs::{self as leaf, Arg, Reply};
use radix_lexer::Span;
use radix_mir::KernelModule;

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
    if leaf::is_route(route) {
        let args = opener_leaf_args(opener);
        return Some(match leaf::serve(route, &args) {
            Some(Ok(reply)) => Ok(reply_to_values(reply)),
            Some(Err(error)) => Err(Value::Str(error.to_string())),
            None => return None,
        });
    }
    let (module, verb) = match route {
        "consolum:dic" => (KernelModule::Consolum, "write_partial"),
        "consolum:mone" => (KernelModule::Consolum, "warn_line"),
        "processus:exi" => (KernelModule::Processus, "exit_process"),
        _ => return None,
    };
    let args = opener_args(opener);
    let reply = kernel::dispatch_kernel(host, module, verb, &args, span);
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

fn opener_leaf_args(opener: &Value) -> Vec<Arg> {
    match opener {
        Value::Array(items) => items.borrow().iter().filter_map(value_to_arg).collect(),
        Value::Unit | Value::Nil => Vec::new(),
        other => value_to_arg(other).into_iter().collect(),
    }
}

fn value_to_arg(value: &Value) -> Option<Arg> {
    match value {
        Value::Str(text) => Some(Arg::Str(text.clone())),
        Value::Int(n) => Some(Arg::Int(*n)),
        Value::Octeti(bytes) => Some(Arg::Bytes(bytes.clone())),
        other => kernel::args::octeti_bytes(other).map(Arg::Bytes),
    }
}

fn reply_to_values(reply: Reply) -> Vec<Value> {
    match reply {
        Reply::Unit => Vec::new(),
        Reply::Bool(value) => vec![Value::Bool(value)],
        Reply::Int(value) => vec![Value::Int(value)],
        Reply::Str(value) => vec![Value::Str(value)],
        Reply::Bytes(value) => vec![Value::Octeti(value)],
        // `↦ lista<T>` materializes one content frame per element.
        Reply::Strs(values) => values.into_iter().map(Value::Str).collect(),
    }
}
