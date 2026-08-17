//! Decode Faber `textus` literal payloads.
//!
//! Escape processing (shared by short strings and guillemet block strings)
//! belongs here, not in the lexer. The lexer reports a raw `Textus(…)`
//! token with only shallow delimiter matching; this module owns the
//! full escape contract.

/// Decode the lexer payload of a `textus` literal.
///
/// The lexer keeps delimiter matching shallow; this phase owns the source escape
/// contract shared by short strings and guillemet block strings.
#[must_use]
pub fn decode_textus_literal(raw: &str) -> String {
    let mut decoded = String::new();
    let mut chars = raw.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let Some(next) = chars.next() else {
                decoded.push('\\');
                break;
            };
            match next {
                '\\' => decoded.push('\\'),
                '"' => decoded.push('"'),
                'n' => decoded.push('\n'),
                't' => decoded.push('\t'),
                'r' => decoded.push('\r'),
                _ => {
                    decoded.push('\\');
                    decoded.push(next);
                }
            }
        } else {
            decoded.push(ch);
        }
    }

    decoded
}

#[cfg(test)]
#[path = "textus_literal_test.rs"]
mod tests;
