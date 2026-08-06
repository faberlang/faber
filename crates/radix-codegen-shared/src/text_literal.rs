//! Shared fab text-literal byte rendering for MIR text backends.
//!
//! The lexer preserves a fab `textus` literal's source escapes verbatim in
//! the interner payload. HIR-direct Rust emit re-processes those escapes at
//! rustc time, but raw-byte backends (LLVM text globals, Wasm linear-memory
//! literal tables) must decode the payload to real bytes before emission.
//!
//! The function is deliberately pure and dependency-free so every MIR text
//! backend can share one decoding authority (lifted from the LLVM leaf;
//! WE2-flagged shared-hub change landing with W11).

/// Decode the C-style escapes a fab `textus` literal keeps verbatim in its
/// interner payload.
///
/// Handles the escapes a Rust string literal accepts (`\\`, `\"`, `\'`,
/// `\n`, `\t`, `\r`, `\0`); unknown escape pairs are preserved verbatim so a
/// payload that is not a working fab-to-Rust string still emits faithfully.
#[must_use]
pub fn unescape_faber_text(raw: &str) -> String {
    let mut decoded = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            decoded.push('\\');
            break;
        };
        match next {
            '\\' => decoded.push('\\'),
            '"' => decoded.push('"'),
            '\'' => decoded.push('\''),
            'n' => decoded.push('\n'),
            't' => decoded.push('\t'),
            'r' => decoded.push('\r'),
            '0' => decoded.push('\0'),
            other => {
                decoded.push('\\');
                decoded.push(other);
            }
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
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
        assert_eq!(unescape_faber_text("nul\\0byte"), "nul\0byte");
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
}
