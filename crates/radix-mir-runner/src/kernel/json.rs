#![allow(clippy::needless_lifetimes)]

use super::super::value::MapKey;
use super::super::{RunnerError, Value};
use super::args::{expect_str, optional_usize};
use crate::inf::{RUNNER_ISSUE_INF_SIZE_EXHAUSTED, check_parse_token, from_magnus};
use handroll::Magnus;
use radix_lexer::Span;
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) fn dispatch(verb: &str, args: &[Value], span: Span) -> Result<Value, RunnerError> {
    match verb {
        "parse" => {
            let [raw] = args else {
                return Err(RunnerError::internal(span, "json.parse provider arity"));
            };
            let raw = expect_str(raw, span, "json.parse input")?;
            parse_json(&raw, span)
        }
        "stringify" => {
            let (valor, indentum) = match args {
                [valor] => (valor, None),
                [valor, indentum] => (valor, optional_usize(indentum)),
                _ => return Err(RunnerError::internal(span, "json.stringify provider arity")),
            };
            serialize_json(valor, indentum, span).map(Value::Str)
        }
        "try_parse" => {
            let [raw] = args else {
                return Err(RunnerError::internal(span, "json.try_parse provider arity"));
            };
            let raw = expect_str(raw, span, "json.try_parse input")?;
            // Malformed text is `nihil`; a resource fault (ruling 45) is not.
            match parse_json(&raw, span) {
                Err(error) if error.issue == RUNNER_ISSUE_INF_SIZE_EXHAUSTED => Err(error),
                parsed => Ok(parsed.unwrap_or(Value::Nil)),
            }
        }
        other => Err(RunnerError::unsupported(
            span,
            &format!("kernel json verb `{other}`"),
        )),
    }
}

fn parse_json(raw: &str, span: Span) -> Result<Value, RunnerError> {
    let value = serde_json::from_str(raw).map_err(|err| {
        RunnerError::internal_with_issue(
            span,
            format!("json.parse failed: {err}"),
            crate::RUNNER_JSON_PARSE_FAILED,
        )
    })?;
    json_to_value(value, span)
}

fn serialize_json(
    value: &Value,
    indentum: Option<usize>,
    span: Span,
) -> Result<String, RunnerError> {
    let json = value_to_json(value, span)?;
    let result = if matches!(indentum, Some(n) if n > 0) {
        serde_json::to_string_pretty(&json)
    } else {
        serde_json::to_string(&json)
    };
    result.map_err(|err| RunnerError::internal(span, format!("json.stringify failed: {err}")))
}

/// The exact integer of a JSON number token that has no fraction or exponent
/// (`arbitrary_precision` keeps the source digits). A token past the parse
/// ceiling is refused by length before the quadratic decimal parse (ruling 45).
fn integer_token(number: &serde_json::Number, span: Span) -> Result<Option<Magnus>, RunnerError> {
    let token = number.to_string();
    if token.contains(['.', 'e', 'E']) {
        return Ok(None);
    }
    check_parse_token(token.trim_start_matches('-').len(), span, "json.parse")?;
    Ok(Magnus::parse_decimal(&token).ok())
}

fn json_to_value(value: serde_json::Value, span: Span) -> Result<Value, RunnerError> {
    match value {
        serde_json::Value::Null => Ok(Value::Nil),
        serde_json::Value::Bool(value) => Ok(Value::Bool(value)),
        serde_json::Value::Number(value) => {
            if let Some(number) = value.as_i64() {
                Ok(Value::Int(number))
            } else if let Some(number) = value.as_u64() {
                // F-084: json.parse large u64 (e.g. u64::MAX) must surface as
                // UInt, not fall through to the lossy as_f64() path.
                Ok(Value::UInt(number))
            } else if let Some(exact) = integer_token(&value, span)? {
                // Ruling 40: an integer token of any length is an `inf`.
                Ok(from_magnus(exact))
            } else if let Some(number) = value.as_f64() {
                Ok(Value::Float(number))
            } else {
                Err(RunnerError::internal(
                    span,
                    format!("json number out of representable range: {value}"),
                ))
            }
        }
        serde_json::Value::String(value) => Ok(Value::Str(value)),
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(json_to_value(item, span)?);
            }
            Ok(Value::Array(Rc::new(RefCell::new(out))))
        }
        serde_json::Value::Object(items) => {
            let mut out = rustc_hash::FxHashMap::default();
            #[allow(clippy::iter_without_into_iter)]
            for (key, item) in items {
                out.insert(MapKey::Str(key), json_to_value(item, span)?);
            }
            Ok(Value::Map(Rc::new(RefCell::new(out))))
        }
    }
}

fn value_to_json(value: &Value, span: Span) -> Result<serde_json::Value, RunnerError> {
    match value {
        Value::Nil | Value::Unit => Ok(serde_json::Value::Null),
        Value::Bool(value) => Ok(serde_json::Value::Bool(*value)),
        Value::Int(value) => Ok(serde_json::Value::Number((*value).into())),
        Value::UInt(value) => Ok(serde_json::Value::Number((*value).into())),
        // An `inf` beyond the 64-bit carriers renders as its digit string, a
        // bare JSON number (ruling 40).
        Value::Magnus(value) => value
            .to_string()
            .parse::<serde_json::Number>()
            .map(serde_json::Value::Number)
            .map_err(|err| RunnerError::internal(span, format!("inf is not a JSON number: {err}"))),
        Value::Dec(..) => Err(RunnerError::internal(
            span,
            "decimal intermediate reached JSON before a store",
        )),
        Value::Float(value) => {
            let Some(number) = serde_json::Number::from_f64(*value) else {
                return Err(RunnerError::internal(
                    span,
                    format!("fractus cannot be represented as JSON number: {value}"),
                ));
            };
            Ok(serde_json::Value::Number(number))
        }
        Value::Str(value) => Ok(serde_json::Value::String(value.clone())),
        Value::Ascii(b) => Ok(serde_json::Value::String(char::from(*b).to_string())),
        Value::Char(ch) => Ok(serde_json::Value::String(ch.to_string())),
        Value::Instans(value) => Ok(serde_json::Value::String(value.to_rfc3339())),
        Value::Octeti(bytes) => {
            let items = bytes
                .iter()
                .map(|byte| serde_json::Value::Number(i64::from(*byte).into()))
                .collect();
            Ok(serde_json::Value::Array(items))
        }
        Value::Array(items) => {
            let items = items.borrow();
            let mut out = Vec::with_capacity(items.len());
            for item in &*items {
                out.push(value_to_json(item, span)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        Value::Map(items) => {
            let items = items.borrow();
            let mut keys = items.keys().collect::<Vec<_>>();
            keys.sort_by_key(|key| format_map_key(key));
            let mut out = serde_json::Map::new();
            #[allow(clippy::iter_without_into_iter)]
            for key in keys {
                let MapKey::Str(key_text) = key else {
                    return Err(RunnerError::internal(span, "map key is not textus json"));
                };
                let item = items
                    .get(key)
                    .ok_or_else(|| RunnerError::internal(span, "json map key lookup failed"))?;
                out.insert(key_text.clone(), value_to_json(item, span)?);
            }
            Ok(serde_json::Value::Object(out))
        }
        Value::Record(fields, _) | Value::Struct(_, fields) => {
            let fields = fields.borrow();
            let mut keys = fields.keys().copied().collect::<Vec<_>>();
            keys.sort_by_key(|key| key.0);
            let mut out = serde_json::Map::new();
            #[allow(clippy::iter_without_into_iter)]
            for key in keys {
                let item = fields
                    .get(&key)
                    .ok_or_else(|| RunnerError::internal(span, "json record key lookup failed"))?;
                out.insert(format!("s{}", key.0), value_to_json(item, span)?);
            }
            Ok(serde_json::Value::Object(out))
        }
        Value::Tensor(tensor) => {
            let tensor = tensor.borrow();
            let mut out = Vec::with_capacity(tensor.data.len());
            for item in &tensor.data {
                out.push(value_to_json(item, span)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        Value::Function(_)
        | Value::Closure(_, _, _)
        | Value::ClosureEnvironment(_, _)
        | Value::SharedCell(_)
        | Value::Regex(_)
        | Value::Match(_)
        | Value::Sermo(_)
        | Value::Set(_)
        | Value::EnumVariant(_, _)
        | Value::Tuple(_)
        | Value::Vector(_)
        | Value::Matrix(_)
        | Value::GradientHandle(_)
        | Value::Cursor(_) => Err(RunnerError::internal(
            span,
            "value variant is outside json kernel",
        )),
    }
}

fn format_map_key(key: &MapKey) -> String {
    key.document_key()
}
