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
#[path = "text_literal_test.rs"]
mod tests;
