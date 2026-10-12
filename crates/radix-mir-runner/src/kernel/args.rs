#![allow(
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::needless_pass_by_value
)]

use super::super::Value;

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
