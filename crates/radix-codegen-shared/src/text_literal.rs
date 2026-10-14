//! Shared fab text-literal byte rendering for MIR text backends.
//!
//! The lexer preserves a fab `textus` literal's source escapes verbatim in
//! the interner payload. HIR-direct Rust emit re-processes those escapes at
//! rustc time, but raw-byte backends (LLVM text globals, Wasm linear-memory
//! literal tables) must decode the payload to real bytes before emission.
//!
//! The function is deliberately pure so every MIR text backend can share one
//! decoding authority (lifted from the LLVM leaf; WE2-flagged shared-hub change
//! landing with W11). The `\u{H…}` scalar escape goes through the lexer's
//! decoder, so the accepted forms cannot drift from the typechecker's.

use radix_lexer::textus_literal::try_decode_textus_literal;

/// Decode the C-style escapes a fab `textus` literal keeps verbatim in its
/// interner payload.
///
/// Handles the escapes a Rust string literal accepts (`\\`, `\"`, `\'`,
/// `\n`, `\t`, `\r`) plus `\u{H…}` (a NUL byte is written `\u{0}`); unknown
/// escape pairs and a malformed `\u` are preserved verbatim so a payload that
/// is not a working fab-to-Rust string still emits faithfully.
#[must_use]
pub fn unescape_faber_text(raw: &str) -> String {
    let mut decoded = String::new();
    let mut chars = raw.chars();
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
            'u' => match unicode_escape(&mut chars) {
                Some(scalar) => decoded.push(scalar),
                None => decoded.push_str("\\u"),
            },
            other => {
                decoded.push('\\');
                decoded.push(other);
            }
        }
    }
    decoded
}

/// Read `{H…}` after `\u`; `chars` advances only when the escape is well formed.
fn unicode_escape(chars: &mut std::str::Chars<'_>) -> Option<char> {
    // `{`, at most six hex digits, `}`: eight characters bound the scan.
    let rest = chars.as_str();
    let (close, _) = rest.char_indices().take(8).find(|&(_, ch)| ch == '}')?;
    let decoded = try_decode_textus_literal(&format!("\\u{}", &rest[..=close])).ok()?;
    *chars = rest[close + 1..].chars();
    decoded.chars().next()
}

#[cfg(test)]
#[path = "text_literal_test.rs"]
mod tests;
#[cfg(test)]
#[path = "text_literal_unicode_escape_test.rs"]
mod unicode_escape_tests;
