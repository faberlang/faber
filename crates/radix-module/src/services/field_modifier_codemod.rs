//! Span-based `varia` insertion for unmarked `genus` fields (DECISIONS D16.4(1)).
//!
//! D16.1 makes every field declare `sit` / `varia` / `fixum`. Existing
//! sources are migrated by inserting the reader pack's `varia` spelling
//! (`var` in en) before each unmarked field. The meaning is unchanged: an
//! unmarked field is writable today.
//!
//! WHY span-based: HIR re-emission (`faber format`) is lossy for this job
//! (it normalizes layout and comments). This rewriter only inserts
//! `"<varia> "` at each field's first token and leaves every other byte of
//! the file identical, so the diff is exactly the migration.
//!
//! Scope: top-level `genus` fields only. `discretio` variant payload fields
//! and union member fields keep today's form (operator fork F7 held).

use crate::lexer::{self, Token, TokenKind};
use crate::locale::{KeywordSurface, LocalePack};
use crate::parser;
use crate::syntax::{ClassMemberKind, StmtKind};

/// Result of migrating one source document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldModifierCodemod {
    /// The rewritten document (the input unchanged when nothing was inserted).
    pub source: String,
    /// Number of `varia` keywords inserted.
    pub inserted: usize,
}

/// Insert `pack`'s `varia` spelling before every unmarked `genus` field.
///
/// `source` is a whole document (optional frontmatter included); `pack` is
/// the file's reader pack. The document must lex and parse cleanly; a source
/// that does not is refused rather than rewritten by guesswork. The rewrite is
/// re-parsed to prove it left no unmarked field behind.
///
/// # Errors
///
/// Returns a message when the frontmatter is invalid, the body does not lex or
/// parse, or the rewritten body fails the re-parse proof.
pub fn insert_field_modifiers(pack: &LocalePack, source: &str) -> Result<FieldModifierCodemod, String> {
    let split = crate::driver::split_frontmatter(source).map_err(|error| format!("invalid frontmatter: {error}"))?;
    let body_offset = split.body_byte_offset as usize;
    let body = &source[body_offset..];
    let offsets = unmarked_field_offsets(pack, body)?;
    if offsets.is_empty() {
        return Ok(FieldModifierCodemod { source: source.to_owned(), inserted: 0 });
    }

    let word = KeywordSurface::new(pack).keyword(TokenKind::Varia);
    let mut rewritten = String::with_capacity(source.len() + offsets.len() * (word.len() + 1));
    let mut cursor = 0usize;
    for offset in &offsets {
        let at = body_offset + offset;
        rewritten.push_str(&source[cursor..at]);
        rewritten.push_str(word);
        rewritten.push(' ');
        cursor = at;
    }
    rewritten.push_str(&source[cursor..]);

    let remaining = unmarked_field_offsets(pack, &rewritten[body_offset..])
        .map_err(|message| format!("rewrite did not re-parse: {message}"))?;
    if !remaining.is_empty() {
        return Err(format!("rewrite left {} unmarked field(s)", remaining.len()));
    }
    Ok(FieldModifierCodemod { source: rewritten, inserted: offsets.len() })
}

/// Body byte offsets of the first token of every unmarked `genus` field, ascending.
fn unmarked_field_offsets(pack: &LocalePack, body: &str) -> Result<Vec<usize>, String> {
    let lexed = lexer::lex_with_locale_pack(body, pack);
    if !lexed.success() {
        return Err(format!("source does not lex ({} error(s))", lexed.errors.len()));
    }
    let tokens: Vec<Token> = lexed.tokens.clone();
    // Bodyless declarations (`@ externa`, contracts) are legal in packages;
    // the codemod parses leniently and only reads field spans.
    let parsed = parser::parse_with_options(lexed, parser::ParseOptions { allow_bodyless_functions: true });
    // An unmarked field is itself a (recoverable) parse error since D5c; it is
    // exactly what this codemod repairs, so only other errors refuse the file.
    let mut blocking = parsed
        .errors
        .iter()
        .filter(|err| !is_missing_field_modifier(err))
        .peekable();
    if blocking.peek().is_some() || parsed.program.is_none() {
        let blocking: Vec<_> = blocking.collect();
        let first = blocking
            .first()
            .map(|err| format!("{:?}", err.kind))
            .unwrap_or_default();
        return Err(format!("source does not parse ({} error(s), first {first})", blocking.len()));
    }
    let program = parsed
        .program
        .ok_or_else(|| "parser returned no program".to_owned())?;

    let mut offsets = Vec::new();
    for statement in &program.statements {
        let StmtKind::Class(class) = &statement.kind else {
            continue;
        };
        for member in &class.members {
            let ClassMemberKind::Field(field) = &member.kind else {
                continue;
            };
            if field.modifier.is_some() {
                continue;
            }
            // The field's first token follows its annotations (if any).
            let floor = member
                .annotations
                .iter()
                .map(|annotation| annotation.span.end)
                .max()
                .unwrap_or(member.span.start);
            let first = tokens
                .iter()
                .find(|token| token.span.start >= floor && !token.kind.is_trivia())
                .ok_or_else(|| "field has no first token".to_owned())?;
            offsets.push(first.span.start as usize);
        }
    }
    offsets.sort_unstable();
    Ok(offsets)
}

/// Whether a parse error is the D16.1 missing-field-modifier error.
fn is_missing_field_modifier(error: &parser::ParseError) -> bool {
    error
        .args
        .iter()
        .any(|arg| arg.name == "issue" && arg.value == "field_modifier_missing")
}

#[cfg(test)]
#[path = "field_modifier_codemod_test.rs"]
mod tests;
