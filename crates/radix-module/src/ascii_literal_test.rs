//! Tests for ASCII literal decoding and validation.
//! Proves: basic escape and control-character handling; rejects non-ASCII
//! scalars and invalid escapes.

use super::{decode_ascii_literal, AsciiLiteralError};

#[test]
fn decode_ascii_plain_passthrough() {
    assert_eq!(decode_ascii_literal("pg:query").unwrap(), "pg:query");
}

#[test]
fn decode_ascii_backslash_escape() {
    assert_eq!(decode_ascii_literal(r"back\\slash").unwrap(), "back\\slash");
}

#[test]
fn decode_ascii_newline_escape() {
    assert_eq!(decode_ascii_literal(r"line\nbreak").unwrap(), "line\nbreak");
}

#[test]
fn decode_ascii_tab_escape() {
    assert_eq!(decode_ascii_literal(r"tab\there").unwrap(), "tab\there");
}

#[test]
fn decode_ascii_carriage_return_escape() {
    assert_eq!(decode_ascii_literal(r"cr\rhere").unwrap(), "cr\rhere");
}

#[test]
fn decode_ascii_quote_escape() {
    assert_eq!(decode_ascii_literal(r"quote\'end").unwrap(), "quote'end");
}

#[test]
fn decode_ascii_empty_string() {
    assert_eq!(decode_ascii_literal("").unwrap(), "");
}

#[test]
fn reject_non_ascii_scalar() {
    assert_eq!(decode_ascii_literal("café"), Err(AsciiLiteralError::NonAsciiScalar('é')));
}

#[test]
fn reject_invalid_escape() {
    assert_eq!(decode_ascii_literal(r"\x41"), Err(AsciiLiteralError::InvalidEscape));
}

#[test]
fn reject_unescaped_control() {
    assert_eq!(
        decode_ascii_literal("\u{0001}"),
        Err(AsciiLiteralError::UnescapedControl('\u{1}'))
    );
}
