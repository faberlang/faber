//! Raw frame routes served by the runner's real hosts (`Host::sermo_route`).
//!
//! Shared leaves (`faber-leaf`) serve `solum:*`, `consolum:*`, `processus:*`
//! (except `processus:exi`), `tempus:*`, `aleator:*`, `caelum:*`, `http:*`,
//! and `websocket:*`. `processus:exi` stays on `Host::exit`.

use super::{Host, MapKey, Value};
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
    if route == "processus:exi" {
        return Some(exit_process(host, opener, span));
    }
    if leaf::is_route(route) {
        let args = opener_leaf_args(opener);
        return Some(match leaf::serve(route, &args) {
            Some(Ok(reply)) => Ok(reply_to_values(reply)),
            Some(Err(error)) => Err(leaf_error_value(&error)),
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
    if let Some(bytes) = unsigned_byte_list(opener) {
        return vec![Arg::Bytes(bytes)];
    }
    match opener {
        Value::Array(items) => items.borrow().iter().filter_map(value_to_arg).collect(),
        Value::Unit | Value::Nil => Vec::new(),
        other => value_to_arg(other).into_iter().collect(),
    }
}

/// A whole-opener `lista<b8>`: a non-empty list of unsigned bit patterns
/// below 256.
///
/// WHY: it is one byte argument, not one positional argument per byte. `UInt`
/// has no scalar leaf form, so flattening it positionally drops every element
/// and the leaf reports `missing data`.
fn unsigned_byte_list(opener: &Value) -> Option<Vec<u8>> {
    let Value::Array(items) = opener else {
        return None;
    };
    let all_unsigned = {
        let items = items.borrow();
        !items.is_empty() && items.iter().all(|item| matches!(item, Value::UInt(_)))
    };
    if all_unsigned {
        octeti_bytes(opener)
    } else {
        None
    }
}

fn value_to_arg(value: &Value) -> Option<Arg> {
    match value {
        Value::Nil | Value::Unit => Some(Arg::Nil),
        Value::Str(text) => Some(Arg::Str(text.clone())),
        Value::Int(n) => Some(Arg::Int(*n)),
        Value::Bool(value) => Some(Arg::Bool(*value)),
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
            } else if let Some(bytes) = octeti_bytes(value) {
                // Byte list before the generic recursion: a nested list of
                // valid byte-valued integers is one `octeti` argument (the
                // base contract; `solum:funde` needs `Arg::Bytes`, not a
                // positional list of integers). A list that is not a valid
                // byte list keeps the generic list path below.
                Some(Arg::Bytes(bytes))
            } else {
                items
                    .iter()
                    .map(value_to_arg)
                    .collect::<Option<Vec<Arg>>>()
                    .map(Arg::List)
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
        other => octeti_bytes(other).map(Arg::Bytes),
    }
}

/// The bytes of an `octeti` argument: the byte value itself, or the
/// `lista<b8>` array a bit-pattern list arrives as (every element one
/// unsigned pattern below 256).
fn octeti_bytes(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Octeti(bytes) => Some(bytes.clone()),
        Value::Array(items) => items
            .borrow()
            .iter()
            .map(|item| match item {
                Value::UInt(v) => u8::try_from(*v).ok(),
                Value::Int(v) => u8::try_from(*v).ok(),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn reply_to_values(reply: Reply) -> Vec<Value> {
    match reply {
        Reply::Unit => Vec::new(),
        // `Strs` is the only terminal that expands to several content items;
        // every other reply maps to at most one value.
        Reply::Strs(values) => values.into_iter().map(Value::Str).collect(),
        other => reply_to_value(other).into_iter().collect(),
    }
}

fn reply_to_value(reply: Reply) -> Option<Value> {
    match reply {
        Reply::Unit => None,
        Reply::Bool(value) => Some(Value::Bool(value)),
        Reply::Int(value) => Some(Value::Int(value)),
        Reply::Float(value) => Some(Value::Float(value)),
        Reply::Str(value) => Some(Value::Str(value)),
        Reply::Bytes(value) => Some(Value::Octeti(value)),
        Reply::Strs(values) => Some(Value::Array(Rc::new(RefCell::new(
            values.into_iter().map(Value::Str).collect(),
        )))),
        Reply::Map(fields) => {
            let mut map = FxHashMap::default();
            for (key, value) in fields {
                if let Some(item) = reply_to_value(value) {
                    map.insert(MapKey::Str(key), item);
                }
            }
            Some(Value::Map(Rc::new(RefCell::new(map))))
        }
        Reply::List(items) => Some(Value::Array(Rc::new(RefCell::new(
            items.into_iter().filter_map(reply_to_value).collect(),
        )))),
    }
}

/// The failure terminal's data: the leaf's `kind` and `path` facts beside the
/// runner-internal `text` (the leaf's `Display`) that current consumers print.
///
/// WHY: kind and path must survive to `sermo_items` as diagnostic args while
/// `text` keeps every message byte-for-byte. `text` is runner-internal: only
/// `sermo::error_text` reads it, and it must never surface as a program field.
fn leaf_error_value(error: &leaf::LeafError) -> Value {
    let (kind, path) = error.facts();
    let mut map = FxHashMap::default();
    map.insert(MapKey::Str("kind".to_owned()), Value::Str(kind.to_owned()));
    map.insert(MapKey::Str("path".to_owned()), Value::Str(path.to_owned()));
    map.insert(
        MapKey::Str("text".to_owned()),
        Value::Str(error.to_string()),
    );
    Value::Map(Rc::new(RefCell::new(map)))
}

#[cfg(test)]
#[path = "routes_test.rs"]
mod tests;
