//! UTF-8 cursor for lexer-owned source traversal.
//!
//! The cursor is the narrow abstraction that lets token scanners reason in
//! Unicode scalar values while spans remain byte offsets into the original
//! source buffer. It deliberately does not classify tokens, normalize text, or
//! collect diagnostics; it only advances through valid UTF-8 boundaries and
//! exposes small lookahead primitives.
//!
//! INVARIANTS
//! ==========
//! - `pos` is always the byte offset of the next character to read.
//! - `chars` is always positioned at `source[pos..]`.
//! - Offsets returned by this cursor are valid UTF-8 boundaries and can be fed
//!   back into [`Cursor::slice`].
//!
//! TRADE-OFFS
//! ==========
//! Lookahead clones the standard `Chars` iterator instead of maintaining a
//! side buffer. The lexer only needs one- and two-character lookahead today, so
//! cloning keeps cursor state simple while preserving zero-copy source slices.

use std::str::Chars;

// =============================================================================
// CORE TYPE
// =============================================================================

/// Position-aware character cursor over one source buffer.
///
/// The lexer constructs all [`Span`](super::token::Span) values from cursor
/// positions, so every mutation must flow through [`Cursor::advance`] or a
/// helper that calls it.
pub struct Cursor<'a> {
    source: &'a str,
    chars: Chars<'a>,
    pos: u32,
}

impl<'a> Cursor<'a> {
    /// Create a new cursor at the beginning of source text.
    #[must_use]
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.chars(),
            pos: 0,
        }
    }

    /// Current byte position in the original source.
    ///
    /// The lexer stores spans as `u32` to keep tokens compact. File loading is
    /// responsible for rejecting sources too large for that representation.
    #[must_use]
    pub fn pos(&self) -> u32 {
        self.pos
    }

    /// Peek at the next character without consuming it.
    #[must_use]
    pub fn peek(&self) -> Option<char> {
        self.chars.clone().next()
    }

    /// Peek at the character after the next one without consuming either.
    ///
    /// This is intentionally the deepest lookahead primitive because the lexer
    /// grammar currently distinguishes only short compound tokens and decimal
    /// float starts (`.` followed by a digit).
    #[must_use]
    pub fn peek_next(&self) -> Option<char> {
        let mut iter = self.chars.clone();
        iter.next();
        iter.next()
    }

    /// Consume and return the next Unicode scalar value.
    ///
    /// The byte cursor advances by `len_utf8`, preserving the span invariant
    /// that all positions are valid string slice boundaries.
    #[allow(clippy::cast_possible_truncation)]
    pub fn advance(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        self.pos += c.len_utf8() as u32;
        Some(c)
    }

    /// Consume the next character only when it matches `expected`.
    pub fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consume contiguous characters accepted by `predicate`.
    pub fn eat_while(&mut self, predicate: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&predicate) {
            self.advance();
        }
    }

    /// Check if we've reached end of file.
    #[must_use]
    pub fn is_eof(&self) -> bool {
        self.peek().is_none()
    }

    /// Borrow source text between two cursor-derived byte positions.
    ///
    /// Callers must pass offsets produced by this cursor or otherwise guarantee
    /// UTF-8 boundary correctness. The method intentionally stays unchecked so
    /// token extraction remains a cheap hot-path operation.
    #[must_use]
    pub fn slice(&self, start: u32, end: u32) -> &'a str {
        debug_assert!(start <= end, "invalid cursor slice: start > end");
        debug_assert!(
            self.source.is_char_boundary(start as usize)
                && self.source.is_char_boundary(end as usize),
            "cursor slice offsets must be UTF-8 boundaries"
        );
        &self.source[start as usize..end as usize]
    }

    /// Borrow the unconsumed suffix of the source buffer.
    #[must_use]
    pub fn rest(&self) -> &'a str {
        &self.source[self.pos as usize..]
    }
}

#[cfg(test)]
#[path = "cursor_test.rs"]
mod tests;
