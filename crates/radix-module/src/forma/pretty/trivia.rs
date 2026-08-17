//! Trivia emission for the `pretty-v1` policy.
//!
//! Blank-line policy: one blank line between top-level declarations; runs
//! collapse to one; the author's blank lines between logical sections inside a
//! body are preserved verbatim; no blank lines are manufactured between
//! statements.
//!
//! Comment policy: `#` comments are re-indented with their surrounding syntax,
//! the comment marker is normalized to exactly one space when absent, and the
//! comment text and line breaks are preserved byte-for-byte (no prose reflow).

use crate::lexer::Interner;
use crate::syntax::Trivia;

use super::super::author::{normalize_leading_trivia, TriviaScope};
use super::layout::PrettyWriter;

/// Emit leading trivia in source order into a pretty writer.
pub fn write_leading_trivia(trivia: &[Trivia], scope: TriviaScope, interner: &Interner, w: &mut PrettyWriter) {
    let normalized = normalize_leading_trivia(trivia, scope);
    let normalized = trim_supplied_boundary_newlines(&normalized, scope);
    let mut prev_was_comment = false;
    let mut pending_blank_lines = 0usize;

    let flush_blanks = |count: usize, w: &mut PrettyWriter| {
        for _ in 0..count {
            w.newline();
        }
    };

    for item in normalized {
        match item {
            Trivia::CommentLine(comment) => {
                flush_blanks(pending_blank_lines, w);
                pending_blank_lines = 0;
                write_comment_line(interner.resolve(comment.text), w);
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

/// Emit one `#` comment with the marker spacing normalized.
///
/// Rule (comment-marker spacing only): an empty payload emits a bare `#`; a
/// payload that already starts with whitespace is written unchanged; a payload
/// that does not start with whitespace gets exactly one space after the `#`
/// (`#foo` → `# foo`). The payload itself is preserved byte-for-byte.
pub fn write_comment_line(text: &str, w: &mut PrettyWriter) {
    w.write("#");
    if text.is_empty() {
        // Bare `#` comment marker; the author wrote it that way.
    } else if text.starts_with(char::is_whitespace) {
        w.write(text);
    } else {
        w.write(" ");
        w.write(text);
    }
    w.newline();
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
