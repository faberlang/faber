//! Decode and validate single-quoted `ascii` literal payloads.
//!
//! Validation (non-ASCII scalars, unescaped controls, invalid escapes)
//! belongs here, not in the lexer. The lexer reports a raw `Ascii(…)` token;
//! this module owns the source-level contract for what constitutes a valid
//! ascii literal.

/// Error while decoding or validating an `ascii` literal payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsciiLiteralError {
    InvalidEscape,
    NonAsciiScalar(char),
    UnescapedControl(char),
}

/// Decode the lexer payload of an `ascii` literal (content between quotes).
///
/// # Errors
///
/// Returns [`AsciiLiteralError`] for invalid escape sequences, non-ASCII
/// scalars, or unescaped control characters.
pub fn decode_ascii_literal(raw: &str) -> Result<String, AsciiLiteralError> {
    let mut decoded = String::new();
    let mut chars = raw.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let Some(next) = chars.next() else {
                return Err(AsciiLiteralError::InvalidEscape);
            };
            let scalar = match next {
                '\\' => '\\',
                '\'' => '\'',
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                'u' => {
                    let Some(scalar) = crate::textus_literal::unicode_escape(&mut chars) else {
                        return Err(AsciiLiteralError::InvalidEscape);
                    };
                    if scalar as u32 > 0x7F {
                        return Err(AsciiLiteralError::InvalidEscape);
                    }
                    scalar
                }
                _ => return Err(AsciiLiteralError::InvalidEscape),
            };
            decoded.push(scalar);
            continue;
        }

        if ch.is_control() && ch != '\n' && ch != '\r' && ch != '\t' {
            return Err(AsciiLiteralError::UnescapedControl(ch));
        }

        if ch as u32 > 0x7F {
            return Err(AsciiLiteralError::NonAsciiScalar(ch));
        }

        decoded.push(ch);
    }

    Ok(decoded)
}
