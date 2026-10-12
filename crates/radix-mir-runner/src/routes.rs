//! Raw frame routes served by the runner's real hosts (`Host::sermo_route`).
//!
//! Shared leaves (`faber-leaf`) serve `solum:*`, `consolum:*`, `processus:*`
//! (except `processus:exi`), `tempus:*`, and `aleator:*`. `processus:exi`
//! stays on `Host::exit`. HTTP stays on `kernel/http.rs` until N5.

use super::{Host, MapKey, Value, kernel};
use faber_leaf::{self as leaf, Arg, Reply};
use radix_lexer::Span;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

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
    if route == "processus:exi" {
        return Some(exit_process(host, opener, span));
    }
    if leaf::is_route(route) {
        let args = opener_leaf_args(opener);
        return Some(match leaf::serve(route, &args) {
            Some(Ok(reply)) => Ok(reply_to_values(reply)),
            Some(Err(error)) => Err(Value::Str(error.to_string())),
            None => return None,
        });
    }
    None
}

fn exit_process<H: Host + ?Sized>(
    host: &mut H,
    opener: &Value,
    _span: Span,
) -> Result<Vec<Value>, Value> {
    let args = opener_args(opener);
    let [Value::Int(code)] = args.as_slice() else {
        return Err(Value::Str("processus:exi code must be numerus".to_owned()));
    };
    #[allow(clippy::cast_possible_truncation)]
    host.exit(*code as i32);
}

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
        Value::Nil | Value::Unit => Some(Arg::Nil),
        Value::Str(text) => Some(Arg::Str(text.clone())),
        Value::Int(n) => Some(Arg::Int(*n)),
        Value::Octeti(bytes) => Some(Arg::Bytes(bytes.clone())),
        Value::Array(items) => {
            let items = items.borrow();
            if items.iter().all(|item| matches!(item, Value::Str(_))) {
                Some(Arg::Strs(
                    items
                        .iter()
                        .filter_map(|item| match item {
                            Value::Str(text) => Some(text.clone()),
                            _ => None,
                        })
                        .collect(),
                ))
            } else {
                kernel::args::octeti_bytes(value).map(Arg::Bytes)
            }
        }
        Value::Map(map) => {
            let mut fields = Vec::new();
            for (key, value) in map.borrow().iter() {
                let MapKey::Str(name) = key else {
                    continue;
                };
                let Value::Str(text) = value else {
                    continue;
                };
                fields.push((name.clone(), text.clone()));
            }
            Some(Arg::Map(fields))
        }
        other => kernel::args::octeti_bytes(other).map(Arg::Bytes),
    }
}

fn reply_to_values(reply: Reply) -> Vec<Value> {
    match reply {
        Reply::Unit => Vec::new(),
        Reply::Bool(value) => vec![Value::Bool(value)],
        Reply::Int(value) => vec![Value::Int(value)],
        Reply::Float(value) => vec![Value::Float(value)],
        Reply::Str(value) => vec![Value::Str(value)],
        Reply::Bytes(value) => vec![Value::Octeti(value)],
        Reply::Strs(values) => values.into_iter().map(Value::Str).collect(),
        Reply::Map(fields) => {
            let mut map = FxHashMap::default();
            for (key, value) in fields {
                let items = reply_to_values(value);
                let Some(item) = items.into_iter().next() else {
                    continue;
                };
                map.insert(MapKey::Str(key), item);
            }
            vec![Value::Map(Rc::new(RefCell::new(map)))]
        }
    }
}
