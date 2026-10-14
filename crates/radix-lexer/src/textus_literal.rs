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

/// Delimiter that owns a literal's escape contract.
///
/// A literal accepts only its own delimiter after a backslash (`\"` inside
/// `"…"`, `\»` inside `«…»`, ``\` `` inside a backtick template); the other
/// two delimiters are ordinary unknown escapes and are rejected by
/// [`first_invalid_escape`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDelimiter {
    /// `"…"` short string.
    Quote,
    /// `«…»` guillemet block string.
    Guillemet,
    /// Backtick-delimited string template.
    Backtick,
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

/// Find the first escape sequence in `raw` the delimiter does not allow.
///
/// After a backslash the only valid continuations are `\\`, the literal's own
/// delimiter, `n`, `t`, `r`, and `u{…}` with 1–6 hex digits naming a Unicode
/// scalar. Anything else is invalid, including a trailing backslash.
///
/// Returns the byte offset of the offending backslash and the character that
/// made the escape invalid (the backslash itself when nothing follows it).
#[must_use]
pub fn first_invalid_escape(raw: &str, delimiter: TextDelimiter) -> Option<(usize, char)> {
    let own = match delimiter {
        TextDelimiter::Quote => '"',
        TextDelimiter::Guillemet => '»',
        TextDelimiter::Backtick => '`',
    };

    let mut chars = raw.char_indices();
    while let Some((offset, ch)) = chars.next() {
        if ch != '\\' {
            continue;
        }
        let Some((_, escaped)) = chars.next() else {
            return Some((offset, '\\'));
        };
        if escaped == 'u' {
            let mut rest = raw[offset + ch.len_utf8() + escaped.len_utf8()..].chars();
            if unicode_escape(&mut rest).is_none() {
                return Some((offset, 'u'));
            }
            continue;
        }
        let allowed = matches!(escaped, '\\' | 'n' | 't' | 'r') || escaped == own;
        if !allowed {
            return Some((offset, escaped));
        }
    }
    None
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
                '»' => decoded.push('»'),
                '`' => decoded.push('`'),
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
pub(crate) fn unicode_escape(chars: &mut std::str::Chars<'_>) -> Option<char> {
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
