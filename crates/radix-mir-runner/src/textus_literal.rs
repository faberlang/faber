//! Textus literal decoder for the MIR runner.
//!
//! Owns the escape-sequence decode contract for Faber textus literals. Shared
//! by short strings and guillemet block strings. The lexer keeps delimiter
//! matching shallow; this module resolves character escapes.

/// Decode the lexer payload of a `textus` literal.
///
/// The lexer keeps delimiter matching shallow; this phase owns the source escape
/// contract shared by short strings and guillemet block strings.
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
