//! Leading and trailing source trivia attached to syntax boundaries.
//!
//! Trivia items preserve `#` line comments and explicit newline tokens in source
//! order. They attach forward to the next statement or declaration, mirroring the
//! annotation attachment pattern.

use radix_lexer::{Span, Symbol};

/// One trivia item in source order before (or after) a syntax node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trivia {
    /// `#` + payload through end of line (newline not included).
    CommentLine(CommentLine),
    /// One logical line break after the previous item.
    Newline(Newline),
}

/// A single `#` line comment without its trailing newline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentLine {
    pub text: Symbol,
    pub span: Span,
}

/// One explicit newline token from the lexer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Newline {
    pub span: Span,
}
