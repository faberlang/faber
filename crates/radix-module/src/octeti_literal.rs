//! Decode and validate pipe-delimited `octeti` hex literal payloads.
//!
//! Validation (invalid hex characters, odd digit count) belongs here,
//! not in the lexer. The lexer reports a raw `Octeti(…)` token; this
//! module owns the byte-decoding contract.

/// Error while decoding or validating an `octeti` hex literal payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OctetiLiteralError {
    InvalidCharacter(char),
    OddDigitCount,
}

/// Decode the lexer payload of an `octeti` literal (hex between `|...|`).
///
/// # Errors
///
/// Returns [`OctetiLiteralError`] for invalid hex characters or an odd number
/// of hex digits.
pub fn decode_octeti_hex(raw: &str) -> Result<Vec<u8>, OctetiLiteralError> {
    let mut digits = String::new();

    for ch in raw.chars() {
        if ch.is_ascii_whitespace() {
            continue;
        }
        if ch.is_ascii_hexdigit() {
            digits.push(ch);
        } else {
            return Err(OctetiLiteralError::InvalidCharacter(ch));
        }
    }

    if !digits.len().is_multiple_of(2) {
        return Err(OctetiLiteralError::OddDigitCount);
    }

    let mut bytes = Vec::with_capacity(digits.len() / 2);
    let mut hex = digits.chars();
    while let (Some(hi), Some(lo)) = (hex.next(), hex.next()) {
        bytes.push(hex_pair_byte(hi, lo)?);
    }

    Ok(bytes)
}

fn hex_pair_byte(hi: char, lo: char) -> Result<u8, OctetiLiteralError> {
    let Some(hi) = hi.to_digit(16) else {
        return Err(OctetiLiteralError::InvalidCharacter(hi));
    };
    let Some(lo) = lo.to_digit(16) else {
        return Err(OctetiLiteralError::InvalidCharacter(lo));
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "hex nibbles 0..=15 bound (hi << 4) | lo to u8"
    )]
    Ok(((hi << 4) | lo) as u8)
}

#[cfg(test)]
#[path = "octeti_literal_test.rs"]
mod tests;
