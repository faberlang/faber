//! Tests for textus literal decoding (escape processing).
//! Proves: basic escapes, unknown escape passthrough, boundary cases
//! (trailing backslash, empty string).

use crate::textus_literal::decode_textus_literal;

#[test]
fn decode_plain_text_passthrough() {
    assert_eq!(decode_textus_literal("salve"), "salve");
}

#[test]
fn decode_backslash_escape() {
    assert_eq!(decode_textus_literal(r"back\\slash"), "back\\slash");
}

#[test]
fn decode_quote_escape() {
    assert_eq!(decode_textus_literal(r#"quote\"end"#), "quote\"end");
}

#[test]
fn decode_newline_escape() {
    assert_eq!(decode_textus_literal(r"line\nbreak"), "line\nbreak");
}

#[test]
fn decode_tab_escape() {
    assert_eq!(decode_textus_literal(r"tab\there"), "tab\there");
}

#[test]
fn decode_carriage_return_escape() {
    assert_eq!(decode_textus_literal(r"cr\rhere"), "cr\rhere");
}

#[test]
fn decode_preserves_unknown_escape_pairs() {
    assert_eq!(decode_textus_literal(r"\d+"), r"\d+");
    assert_eq!(decode_textus_literal(r"\x41"), r"\x41");
}

#[test]
fn decode_trailing_backslash_preserved() {
    assert_eq!(decode_textus_literal(r"trail\"), r"trail\");
}

#[test]
fn decode_empty_string() {
    assert_eq!(decode_textus_literal(""), "");
}
