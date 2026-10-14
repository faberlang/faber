//! Tests for ASCII literal decoding and validation.
//! Proves: basic escape and control-character handling; rejects non-ASCII
//! scalars and invalid escapes.

use crate::ascii_literal::{AsciiLiteralError, decode_ascii_literal};

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
    assert_eq!(
        decode_ascii_literal("café"),
        Err(AsciiLiteralError::NonAsciiScalar('é'))
    );
}

#[test]
fn reject_invalid_escape() {
    assert_eq!(
        decode_ascii_literal(r"\x41"),
        Err(AsciiLiteralError::InvalidEscape)
    );
}

#[test]
fn reject_unescaped_control() {
    assert_eq!(
        decode_ascii_literal("\u{0001}"),
        Err(AsciiLiteralError::UnescapedControl('\u{1}'))
    );
}

#[test]
fn ascii_unicode_escape_ascii_scalar() {
    assert_eq!(decode_ascii_literal(r"\u{41}").unwrap(), "A");
}

#[test]
fn ascii_unicode_escape_mid_string() {
    assert_eq!(decode_ascii_literal(r"a\u{42}c").unwrap(), "aBc");
}

#[test]
fn ascii_unicode_escape_control() {
    assert_eq!(decode_ascii_literal(r"\u{1B}").unwrap(), "\u{1b}");
}

#[test]
fn ascii_unicode_escape_del_boundary() {
    assert_eq!(decode_ascii_literal(r"\u{7F}").unwrap(), "\u{7f}");
}

#[test]
fn ascii_unicode_escape_above_7f_rejected() {
    assert_eq!(
        decode_ascii_literal(r"\u{80}"),
        Err(AsciiLiteralError::InvalidEscape)
    );
    assert_eq!(
        decode_ascii_literal(r"\u{E9}"),
        Err(AsciiLiteralError::InvalidEscape)
    );
}

#[test]
fn ascii_unicode_escape_not_a_scalar_rejected() {
    assert_eq!(
        decode_ascii_literal(r"\u{110000}"),
        Err(AsciiLiteralError::InvalidEscape)
    );
    assert_eq!(
        decode_ascii_literal(r"\u{D800}"),
        Err(AsciiLiteralError::InvalidEscape)
    );
}

#[test]
fn ascii_unicode_escape_malformed_rejected() {
    assert_eq!(
        decode_ascii_literal(r"\u{}"),
        Err(AsciiLiteralError::InvalidEscape)
    );
    assert_eq!(
        decode_ascii_literal(r"\u{41"),
        Err(AsciiLiteralError::InvalidEscape)
    );
    assert_eq!(
        decode_ascii_literal(r"\u41"),
        Err(AsciiLiteralError::InvalidEscape)
    );
}
