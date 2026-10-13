//! Decode Faber `textus` literal payloads.
//!
//! Escape processing (shared by short strings and guillemet block strings)
//! belongs here, not in the lexer. The lexer reports a raw `Textus(…)`
//! token with only shallow delimiter matching; this module owns the
//! full escape contract.

/// Error while decoding a `textus` literal payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextusLiteralError {
    /// `\u` not followed by `{`, 1–6 hex digits and `}`, or a value that is
    /// not a Unicode scalar (surrogate, above `10FFFF`).
    InvalidUnicodeEscape,
}

/// Decode the lexer payload of a `textus` literal.
///
/// The lexer keeps delimiter matching shallow; this phase owns the source escape
/// contract shared by short strings and guillemet block strings.
///
/// WHY: infallible on purpose. Unknown escapes (and a malformed `\u`) are still
/// kept as written until the unknown-escape error lands with the regex literal;
/// [`try_decode_textus_literal`] is the form that reports a malformed `\u`.
#[must_use]
pub fn decode_textus_literal(raw: &str) -> String {
    decode_escapes(raw, false).unwrap_or_default()
}

/// Decode a `textus` literal payload, rejecting a malformed `\u{…}` escape.
///
/// # Errors
///
/// Returns [`TextusLiteralError::InvalidUnicodeEscape`] when a `\u` escape is
/// not `\u{H…}` with 1–6 hex digits naming a Unicode scalar.
pub fn try_decode_textus_literal(raw: &str) -> Result<String, TextusLiteralError> {
    decode_escapes(raw, true)
}

fn decode_escapes(raw: &str, strict: bool) -> Result<String, TextusLiteralError> {
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
                'u' => match unicode_escape(&mut chars) {
                    Some(scalar) => decoded.push(scalar),
                    None if strict => return Err(TextusLiteralError::InvalidUnicodeEscape),
                    None => decoded.push_str("\\u"),
                },
                _ => {
                    decoded.push('\\');
                    decoded.push(next);
                }
            }
        } else {
            decoded.push(ch);
        }
    }

    Ok(decoded)
}

/// Read `{H…}` after `\u`; `chars` advances only when the escape is well formed.
fn unicode_escape(chars: &mut std::str::Chars<'_>) -> Option<char> {
    const MAX_DIGITS: usize = 6;

    let mut ahead = chars.clone();
    if ahead.next()? != '{' {
        return None;
    }

    let mut value: u32 = 0;
    let mut digits = 0;
    loop {
        let ch = ahead.next()?;
        if ch == '}' {
            break;
        }
        digits += 1;
        if digits > MAX_DIGITS {
            return None;
        }
        value = value * 16 + ch.to_digit(16)?;
    }
    if digits == 0 {
        return None;
    }

    let scalar = char::from_u32(value)?;
    *chars = ahead;
    Some(scalar)
}

#[cfg(test)]
#[path = "textus_literal_test.rs"]
mod tests;
