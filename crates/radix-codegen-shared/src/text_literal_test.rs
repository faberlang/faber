use super::unescape_faber_text;

#[test]
fn unescapes_c_style_escapes_to_real_bytes() {
    // The lexer preserves source escapes verbatim; raw-byte backends
    // render real bytes, so `\n` etc. must become real bytes (the Rust
    // lane re-processes them at rustc time instead).
    assert_eq!(
        unescape_faber_text("prima\\nsecunda\\n"),
        "prima\nsecunda\n"
    );
    assert_eq!(unescape_faber_text("a\\tb\\r"), "a\tb\r");
    assert_eq!(unescape_faber_text("say \\\"salve\\\""), "say \"salve\"");
    assert_eq!(unescape_faber_text("slash\\\\slash"), "slash\\slash");
    assert_eq!(unescape_faber_text("nul\\u{0}byte"), "nul\0byte");
    assert_eq!(unescape_faber_text("nul\\0byte"), "nul\\0byte");
}

#[test]
fn preserves_unknown_escapes_and_plain_text() {
    assert_eq!(unescape_faber_text("plain"), "plain");
    assert_eq!(unescape_faber_text("\\d+"), "\\d+");
    assert_eq!(unescape_faber_text("a\\q"), "a\\q");
    assert_eq!(unescape_faber_text("trailing\\"), "trailing\\");
}

#[test]
fn unescape_is_idempotent_on_real_newlines() {
    // A payload that already carries real bytes (e.g. real newlines from
    // a Rust-side fixture) must pass through unchanged.
    assert_eq!(unescape_faber_text("a\n\"\\\u{00e9}"), "a\n\"\\\u{00e9}");
}
