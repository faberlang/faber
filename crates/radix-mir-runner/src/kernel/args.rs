#![allow(
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::needless_pass_by_value
)]

use super::super::{RunnerError, Value};
use radix_lexer::Span;
use std::cell::RefCell;
use std::rc::Rc;

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

pub(crate) fn strings_to_value_lista(items: Vec<String>) -> Value {
    Value::Array(Rc::new(RefCell::new(
        items.into_iter().map(Value::Str).collect(),
    )))
}
