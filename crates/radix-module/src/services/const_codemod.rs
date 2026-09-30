//! Span-based `static` → `const` migration for module-level constants (st1 R18).
//!
//! The module-level `static` (en) / `generis` (la) declaration and a top-level
//! `const T X = e` (la `fixum`) are the same constant (st1 R1/R3). The codemod
//! swaps the one keyword token of every module `static` for the reader pack's
//! `fixum` spelling and leaves every other byte of the file identical, so the
//! diff is exactly the migration.
//!
//! WHY span-based: HIR re-emission (`faber format`) is lossy for a migration
//! diff. WHY this is meaning-preserving: both spellings lower to the same
//! `HirConst { statica: Some, .. }` through the same initializer path; the
//! rewriter never touches an initializer, annotation, comment, or line break,
//! and re-parses its own output to prove the declaration text after the
//! keyword is unchanged.
//!
//! Scope: top-level `static` statements only. Genus `generis` fields are
//! `ClassMemberKind::Field` and never match.
//!
//! This module is deleted with the lock unit (st1 U3); the retired-spelling
//! diagnostic is the lasting migration aid.

use crate::lexer::{self, Token, TokenKind};
use crate::locale::{KeywordSurface, LocalePack};
use crate::parser;
use crate::syntax::{Stmt, StmtKind};

/// Result of migrating one source document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstCodemod {
    /// The rewritten document (the input unchanged when nothing was converted).
    pub source: String,
    /// Number of module `static` keywords swapped for `fixum`.
    pub converted: usize,
}

/// One module `static` statement: where its keyword token sits.
struct Site {
    keyword: std::ops::Range<usize>,
}

/// Parse facts the rewrite and its proof read.
struct Survey {
    sites: Vec<Site>,
    statement_shape: Vec<std::mem::Discriminant<StmtKind>>,
    /// End byte of every top-level statement, in order.
    statement_ends: Vec<usize>,
    /// Top-level `=` constants (`compile_time` declarations).
    compile_time_constants: usize,
}

/// Swap every module `static` keyword in `source` for `pack`'s `fixum`.
///
/// `source` is a whole document (optional frontmatter included); `pack` is the
/// file's reader pack. The document must lex and parse cleanly; a source that
/// does not is refused rather than rewritten by guesswork. The rewrite is
/// re-parsed to prove no module `static` remains, the statement sequence is
/// unchanged, and every declaration's text after the keyword is identical.
///
/// # Errors
///
/// Returns a message when the frontmatter is invalid, the body does not lex or
/// parse, a site has no keyword token, or the rewritten body fails the proof.
pub fn convert_static_to_const(pack: &LocalePack, source: &str) -> Result<ConstCodemod, String> {
    let split = crate::driver::split_frontmatter(source).map_err(|error| format!("invalid frontmatter: {error}"))?;
    let body_offset = split.body_byte_offset as usize;
    let body = &source[body_offset..];
    let survey = survey(pack, body)?;
    if survey.sites.is_empty() {
        return Ok(ConstCodemod { source: source.to_owned(), converted: 0 });
    }

    let word = KeywordSurface::new(pack).keyword(TokenKind::Fixum);
    let mut rewritten = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for site in &survey.sites {
        rewritten.push_str(&body[cursor..site.keyword.start]);
        rewritten.push_str(word);
        cursor = site.keyword.end;
    }
    rewritten.push_str(&body[cursor..]);

    prove(pack, &survey, &rewritten, word)?;
    Ok(ConstCodemod { source: format!("{}{rewritten}", &source[..body_offset]), converted: survey.sites.len() })
}

/// Lex and parse `body`, refusing anything that is not clean, and collect the
/// module `static` sites.
fn survey(pack: &LocalePack, body: &str) -> Result<Survey, String> {
    let lexed = lexer::lex_with_locale_pack(body, pack);
    if !lexed.success() {
        return Err(format!("source does not lex ({} error(s))", lexed.errors.len()));
    }
    let tokens: Vec<Token> = lexed.tokens.clone();
    // Bodyless declarations (`@ externa`, contracts) are legal in packages;
    // the codemod parses leniently and only reads statement spans.
    let parsed = parser::parse_with_options(lexed, parser::ParseOptions { allow_bodyless_functions: true });
    if let Some(first) = parsed.errors.first() {
        return Err(format!(
            "source does not parse ({} error(s), first {:?})",
            parsed.errors.len(),
            first.kind
        ));
    }
    let program = parsed
        .program
        .ok_or_else(|| "parser returned no program".to_owned())?;

    let static_word = KeywordSurface::new(pack).keyword(TokenKind::Generis);
    let mut sites = Vec::new();
    for statement in &program.statements {
        if !matches!(&statement.kind, StmtKind::Var(decl) if decl.is_static) {
            continue;
        }
        sites.push(site_of(statement, &tokens, body, static_word)?);
    }
    let statement_shape = program
        .statements
        .iter()
        .map(|statement| std::mem::discriminant(&statement.kind))
        .collect();
    let statement_ends = program
        .statements
        .iter()
        .map(|statement| statement.span.end as usize)
        .collect();
    let compile_time_constants = program
        .statements
        .iter()
        .filter(|statement| matches!(&statement.kind, StmtKind::Var(decl) if decl.compile_time))
        .count();
    Ok(Survey { sites, statement_shape, statement_ends, compile_time_constants })
}

/// The keyword token of one `static` statement, after its annotations.
fn site_of(statement: &Stmt, tokens: &[Token], body: &str, static_word: &str) -> Result<Site, String> {
    let floor = statement
        .annotations
        .iter()
        .map(|annotation| annotation.span.end)
        .chain(std::iter::once(statement.span.start))
        .max()
        .unwrap_or(statement.span.start);
    let first = tokens
        .iter()
        .find(|token| token.span.start >= floor && !token.kind.is_trivia())
        .ok_or_else(|| "static site has no first token".to_owned())?;
    let keyword = first.span.start as usize..first.span.end as usize;
    if body.get(keyword.clone()) != Some(static_word) {
        return Err(format!(
            "static site at byte {} does not start with `{static_word}`",
            keyword.start
        ));
    }
    Ok(Site { keyword })
}

/// Re-parse the rewrite: no module `static` left, every converted site is now
/// a top-level `=` constant, the statement sequence is unchanged, and every
/// statement ends exactly where the keyword width change says it should (so no
/// declaration swallowed or released a byte).
fn prove(pack: &LocalePack, survey: &Survey, rewritten: &str, word: &str) -> Result<(), String> {
    let after = self::survey(pack, rewritten).map_err(|message| format!("rewrite did not re-parse: {message}"))?;
    if !after.sites.is_empty() {
        return Err(format!("rewrite left {} module static(s)", after.sites.len()));
    }
    if after.statement_shape != survey.statement_shape {
        return Err("rewrite changed the top-level statement sequence".to_owned());
    }
    if after.compile_time_constants != survey.compile_time_constants + survey.sites.len() {
        return Err("rewrite did not turn every module static into a top-level `=` constant".to_owned());
    }
    let static_width = survey.sites.first().map_or(0, |site| site.keyword.len());
    for (index, (&old_end, &new_end)) in survey
        .statement_ends
        .iter()
        .zip(&after.statement_ends)
        .enumerate()
    {
        let swapped = survey
            .sites
            .iter()
            .filter(|site| site.keyword.start < old_end)
            .count();
        let expected = old_end + swapped * word.len() - swapped * static_width;
        if new_end != expected {
            return Err(format!("statement {index} changed extent after the keyword swap"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "const_codemod_test.rs"]
mod tests;
