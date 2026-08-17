//! Render leading trivia as `#` comment lines and blank lines for
//! author-mode formatting.
//!
//! Normalizes and emits [`Trivia`] items in source order. Boundary: owns
//! trivia emission shape; the caller (`emit.rs`) owns statement-level
//! layout and line-termination decisions.

use crate::codegen::CodeWriter;
use crate::lexer::Interner;
use crate::syntax::Trivia;

use super::layout::{normalize_leading_trivia, TriviaScope};

/// Emit leading trivia in source order.
pub fn write_leading_trivia(trivia: &[Trivia], scope: TriviaScope, interner: &Interner, w: &mut CodeWriter) {
    let normalized = normalize_leading_trivia(trivia, scope);
    let normalized = trim_supplied_boundary_newlines(&normalized, scope);
    let mut prev_was_comment = false;
    let mut pending_blank_lines = 0usize;

    let flush_blanks = |count: usize, w: &mut CodeWriter| {
        for _ in 0..count {
            w.newline();
        }
    };

    for item in normalized {
        match item {
            Trivia::CommentLine(comment) => {
                flush_blanks(pending_blank_lines, w);
                pending_blank_lines = 0;
                w.write("#");
                w.write(interner.resolve(comment.text));
                w.newline();
                prev_was_comment = true;
            }
            Trivia::Newline(_) => {
                if prev_was_comment {
                    // Line break already emitted after `# …`.
                    prev_was_comment = false;
                    continue;
                }
                pending_blank_lines += 1;
            }
        }
    }
    flush_blanks(pending_blank_lines, w);
}

fn trim_supplied_boundary_newlines(trivia: &[Trivia], scope: TriviaScope) -> &[Trivia] {
    // The surrounding layout already supplies statement boundary line breaks.
    // Drop boundary trivia structurally instead of post-processing emitted text.
    let mut start = 0usize;
    match scope {
        TriviaScope::ProgramStart => {
            while trivia
                .get(start)
                .is_some_and(|item| matches!(item, Trivia::Newline(_)))
            {
                start += 1;
            }
        }
        TriviaScope::Inner | TriviaScope::TopLevel => {
            if trivia
                .first()
                .is_some_and(|item| matches!(item, Trivia::Newline(_)))
            {
                start = 1;
            }
        }
    }
    &trivia[start..]
}
