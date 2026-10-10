//! BB-3R: scalar index returns one-byte `ascii<1>`, not a heap `String`.

use super::RunnerError;
use super::value::{Value, ascii1_from_scalar, scalar_text_index};
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
    assert_eq!(err.issue, "runner_text_index_out_of_bounds");
}

#[test]
fn non_ascii_scalar_index_fails_closed() {
    let err = ascii1_from_scalar('é', span()).expect_err("non-ASCII");
    assert_eq!(err.issue, "runner_textus_index_trap");
    let _: RunnerError = err;
}

#[test]
fn one_byte_ascii_value_is_not_string() {
    let value = Value::Ascii(b'x');
    assert_ne!(value, Value::Str("x".to_owned()));
    assert_eq!(value, Value::Ascii(b'x'));
}

#[test]
fn non_ascii_scalar_index_is_the_ith_character() {
    // "Grüße": s[2] is `ü`, s[3] is `ß`; indexing counts characters, not bytes.
    let u = scalar_text_index("Grüße", 2, span()).expect("ü");
    assert_eq!(u, Value::Str("ü".to_owned()));
    let sz = scalar_text_index("Grüße", 3, span()).expect("ß");
    assert_eq!(sz, Value::Str("ß".to_owned()));
    // ASCII positions keep the compact carrier.
    assert_eq!(
        scalar_text_index("Grüße", 0, span()).expect("G"),
        Value::Ascii(b'G')
    );
}

#[test]
fn scalar_index_past_last_character_of_non_ascii_text_dies() {
    let err = scalar_text_index("Grüße", 5, span()).expect_err("past the end");
    assert_eq!(err.issue, "runner_text_index_out_of_bounds");
}
