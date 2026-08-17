//! Indentation and blank-line policy for author-mode emit (D-3).
//!
//! Shares the canonical [`CodeWriter`] four-space indent and Stroustrup brace
//! placement. Blank-line policy preserves author-written gaps but collapses runs
//! of more than one blank line between top-level declarations to a single gap.

use crate::codegen::CodeWriter;
use crate::lexer::Span;
use crate::syntax::{Newline, Trivia};

/// Two newline tokens encode one visual blank line at file and top-level edges.
const TOP_LEVEL_BLANK_LINE_TOKENS: usize = 2;

/// Layout scope for trivia normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriviaScope {
    /// Before the first top-level statement — discard file-leading blank lines.
    ProgramStart,
    /// Inside blocks and member lists — preserve trivia verbatim.
    Inner,
    /// Between top-level program statements — collapse extra blank lines.
    TopLevel,
}

/// Normalize a leading-trivia run before emission.
///
/// Comment lines are preserved in order. Consecutive [`Trivia::Newline`] runs are
/// kept as-is in [`TriviaScope::Inner`]. At [`TriviaScope::ProgramStart`] and
/// [`TriviaScope::TopLevel`], runs longer than two newline tokens (more than one
/// visual blank line) collapse to two tokens (one blank line).
#[must_use]
pub fn normalize_leading_trivia(trivia: &[Trivia], scope: TriviaScope) -> Vec<Trivia> {
    if trivia.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(trivia.len());
    let mut newline_run = 0usize;
    let mut newline_span = Span::default();

    let flush_newlines = |run: usize, span: Span, out: &mut Vec<Trivia>| {
        if run == 0 {
            return;
        }
        let kept = match scope {
            TriviaScope::Inner => run,
            TriviaScope::ProgramStart | TriviaScope::TopLevel => run.min(TOP_LEVEL_BLANK_LINE_TOKENS),
        };
        for _ in 0..kept {
            out.push(Trivia::Newline(Newline { span }));
        }
    };

    for item in trivia {
        match item {
            Trivia::Newline(newline) => {
                if newline_run == 0 {
                    newline_span = newline.span;
                }
                newline_run += 1;
            }
            other @ Trivia::CommentLine(_) => {
                flush_newlines(newline_run, newline_span, &mut out);
                newline_run = 0;
                out.push(other.clone());
            }
        }
    }
    flush_newlines(newline_run, newline_span, &mut out);
    out
}

/// Choose leading-trivia normalization for one top-level program statement.
#[must_use]
pub fn top_level_trivia_scope(stmt_index: usize) -> TriviaScope {
    if stmt_index == 0 {
        TriviaScope::ProgramStart
    } else {
        TriviaScope::TopLevel
    }
}

/// Author-mode code writer factory (four-space indent, shared with canonical).
pub fn new_writer() -> CodeWriter {
    CodeWriter::new()
}
