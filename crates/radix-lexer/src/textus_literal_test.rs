//! Tests for textus literal decoding (escape processing).
//! Proves: basic escapes, `\u{…}` scalar escapes, unknown escape passthrough,
//! boundary cases (trailing backslash, empty string).

use crate::textus_literal::{TextusLiteralError, decode_textus_literal, try_decode_textus_literal};

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

#[test]
fn decode_unicode_escape_ascii_scalar() {
    assert_eq!(decode_textus_literal(r"\u{41}"), "A");
    assert_eq!(try_decode_textus_literal(r"\u{41}"), Ok("A".to_owned()));
}

#[test]
fn decode_unicode_escape_astral_scalar() {
    assert_eq!(decode_textus_literal(r"\u{1F600}"), "\u{1F600}");
    assert_eq!(decode_textus_literal(r"\u{10FFFF}"), "\u{10FFFF}");
}

#[test]
fn decode_unicode_escape_hex_case_and_padding() {
    assert_eq!(decode_textus_literal(r"\u{0000e9}"), "\u{e9}");
    assert_eq!(decode_textus_literal(r"\u{E9}"), "\u{e9}");
    assert_eq!(decode_textus_literal(r"\u{0}"), "\0");
}

#[test]
fn decode_unicode_escape_mixed_with_text_and_escapes() {
    assert_eq!(decode_textus_literal(r"a\u{42}c\n\u{43}"), "aBc\nC");
}

#[test]
fn decode_escaped_backslash_before_u_is_not_a_unicode_escape() {
    assert_eq!(decode_textus_literal(r"\\u{41}"), r"\u{41}");
    assert_eq!(
        try_decode_textus_literal(r"\\u{41}"),
        Ok(r"\u{41}".to_owned())
    );
}

#[test]
fn try_decode_rejects_surrogate_scalar() {
    assert_eq!(
        try_decode_textus_literal(r"\u{D800}"),
        Err(TextusLiteralError::InvalidUnicodeEscape)
    );
}

#[test]
fn try_decode_rejects_scalar_above_unicode_range() {
    assert_eq!(
        try_decode_textus_literal(r"\u{110000}"),
        Err(TextusLiteralError::InvalidUnicodeEscape)
    );
}

#[test]
fn try_decode_rejects_malformed_unicode_escape() {
    for raw in [
        r"\u{",
        r"\u{41",
        r"\u{}",
        r"\u41",
        r"\u",
        r"\u{4G}",
        r"\u{0000041}",
    ] {
        assert_eq!(
            try_decode_textus_literal(raw),
            Err(TextusLiteralError::InvalidUnicodeEscape),
            "{raw}"
        );
    }
}

#[test]
fn decode_keeps_malformed_unicode_escape_as_written() {
    assert_eq!(decode_textus_literal(r"\u{"), r"\u{");
    assert_eq!(decode_textus_literal(r"\u{D800}"), r"\u{D800}");
    assert_eq!(decode_textus_literal(r"\u00e9\n"), "\\u00e9\n");
}
