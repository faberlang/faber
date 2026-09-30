//! The exact value of an integer literal too large for `u64` (inf track,
//! ruling 34), and the shared bignum every constant fold uses (ruling 44).
//!
//! The lexer carries a big literal as its cleaned digit text (decimal, or
//! `0x`/`0o`/`0b` prefixed); semantic lowering normalises it to canonical
//! decimal digits. [`Magnus`] is the std-only bignum of `handroll`.

use handroll::Radix;
pub use handroll::{Magnus, MagnusError};

/// Parse an integer literal spelling of any length into its exact value:
/// decimal digits, or a `0x` / `0o` / `0b` prefixed digit string (either
/// case). Separators must already be removed. `None` when the text is not a
/// non-empty digit string of its radix.
#[must_use]
pub fn parse_big_literal(text: &str) -> Option<Magnus> {
    let (radix, digits) = match text.get(..2) {
        Some("0x" | "0X") => (Radix::Hex, &text[2..]),
        Some("0o" | "0O") => (Radix::Oct, &text[2..]),
        Some("0b" | "0B") => (Radix::Bin, &text[2..]),
        _ => (Radix::Dec, text),
    };
    if digits.is_empty() || digits.starts_with(['+', '-']) {
        return None;
    }
    Magnus::parse_radix(digits, radix).ok()
}

#[cfg(test)]
#[path = "big_literal_test.rs"]
mod tests;
