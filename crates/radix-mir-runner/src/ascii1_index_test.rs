//! BB-3R: scalar index returns one-byte `ascii<1>`, not a heap `String`.

use super::value::{ascii1_from_scalar, scalar_text_index, Value};
use super::RunnerError;
use radix_lexer::Span;

fn span() -> Span {
    Span::default()
}

#[test]
fn scalar_textus_index_returns_ascii1_not_heap_string() {
    let value = scalar_text_index("ab", 0, span()).expect("in-range ASCII index");
    assert_eq!(value, Value::Ascii(b'a'));
    assert!(
        !matches!(value, Value::Str(_)),
        "scalar index must not allocate a heap String, got {value:?}"
    );
}

#[test]
fn scalar_ascii_index_second_byte_is_ascii1() {
    let value = scalar_text_index("ab", 1, span()).expect("second ASCII byte");
    assert_eq!(value, Value::Ascii(b'b'));
}

#[test]
fn scalar_index_out_of_bounds_fails_closed() {
    let err = scalar_text_index("ab", 2, span()).expect_err("OOB");
    assert!(
        err.message.contains("text index out of bounds"),
        "got {err:?}"
    );
}

#[test]
fn non_ascii_scalar_index_fails_closed() {
    let err = ascii1_from_scalar('é', span()).expect_err("non-ASCII");
    assert!(err.message.contains("textus index trap"), "got {err:?}");
    let _ty: RunnerError = err;
}

#[test]
fn one_byte_ascii_value_is_not_string() {
    let value = Value::Ascii(b'x');
    assert_ne!(value, Value::Str("x".to_owned()));
    assert_eq!(value, Value::Ascii(b'x'));
}
