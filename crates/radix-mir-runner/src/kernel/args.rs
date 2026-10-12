#![allow(
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::needless_pass_by_value
)]

use super::super::{RunnerError, Value};
use radix_lexer::Span;

pub(crate) fn expect_str(value: &Value, span: Span, label: &str) -> Result<String, RunnerError> {
    match value {
        Value::Str(text) => Ok(text.clone()),
        _ => Err(RunnerError::internal(
            span,
            format!("{label} expects textus"),
        )),
    }
}

pub(crate) fn optional_usize(value: &Value) -> Option<usize> {
    match value {
        Value::Nil | Value::Unit => None,
        Value::Int(v) => usize::try_from(*v).ok(),
        _ => None,
    }
}

pub(crate) fn value_lista_to_strings(
    value: &Value,
    span: Span,
) -> Result<Vec<String>, RunnerError> {
    let Value::Array(items) = value else {
        return Err(RunnerError::internal(span, "expected lista<textus>"));
    };
    items
        .borrow()
        .iter()
        .map(|item| expect_str(item, span, "lista element"))
        .collect()
}

/// The bytes of an `octeti` argument: the byte value itself, or the
/// `lista<b8>` array a bit-pattern list arrives as (every element one
/// unsigned pattern below 256).
pub(crate) fn octeti_bytes(value: &Value) -> Option<Vec<u8>> {
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
