//! Need, unit 2: the raw-byte backends decode `\u{H…}` through the
//! lexer decoder, so they agree with the typechecker on what the text means.

use super::unescape_faber_text;

#[test]
fn unicode_scalar_escape_decodes_to_the_scalar() {
    assert_eq!(unescape_faber_text("\\u{41}"), "A");
    assert_eq!(unescape_faber_text("a\\u{e9}b\\n"), "a\u{e9}b\n");
    assert_eq!(unescape_faber_text("\\u{1F600}"), "\u{1F600}");
}

#[test]
fn escaped_backslash_before_u_is_not_a_unicode_escape() {
    assert_eq!(unescape_faber_text("\\\\u{41}"), "\\u{41}");
}

#[test]
fn malformed_unicode_escape_keeps_the_text_as_written() {
    assert_eq!(unescape_faber_text("\\u{41"), "\\u{41");
    assert_eq!(unescape_faber_text("\\u{}"), "\\u{}");
    assert_eq!(unescape_faber_text("\\u{D800}"), "\\u{D800}");
    assert_eq!(unescape_faber_text("\\u{0000041}"), "\\u{0000041}");
    assert_eq!(unescape_faber_text("\\u41"), "\\u41");
    assert_eq!(unescape_faber_text("trailing\\u"), "trailing\\u");
}
