//! Shared author-format gates for SC-3 / SC-10 verification.
//!
//! Used by `forma` integration tests and `faber-cli` format tests so one
//! definition drives re-parse and idempotency checks.

use super::compile_author_str;
use crate::driver::Session;
use crate::lexer;
use crate::locale::LocalePack;
use crate::parser;

/// Lex and parse Faber source; returns a human-readable error on failure.
///
/// # Errors
///
/// Returns a string with lex or parse error details when the source is invalid.
pub fn lex_parse(code: &str) -> Result<(), String> {
    lex_parse_with_pack(code, None)
}

/// Lex and parse Faber source under an explicit reader pack (Latin default
/// when `None`), returning a human-readable error on failure.
///
/// Author-format output is emitted in the pack that lexed the input, so the
/// re-parse gate must use that same pack: a Latin-default re-parse would reject
/// (or mis-parse) localized keyword spellings.
///
/// # Errors
///
/// Returns a string with lex or parse error details when the source is invalid.
pub fn lex_parse_with_pack(code: &str, pack: Option<&LocalePack>) -> Result<(), String> {
    let lex = match pack {
        Some(pack) => lexer::lex_with_locale_pack(code, pack),
        None => lexer::lex(code),
    };
    if !lex.success() {
        return Err(format!("lex errors: {:?}", lex.errors));
    }
    let parse = parser::parse(lex);
    if !parse.success() {
        return Err(format!("parse errors: {:?}", parse.errors));
    }
    if parse.program.is_none() {
        return Err("parse succeeded but program is missing".to_owned());
    }
    Ok(())
}

/// One author-format pass via `compile_author_str`.
///
/// # Errors
///
/// Returns diagnostic error messages when formatting fails, or when formatting
/// succeeds without producing output.
pub fn author_format_once(name: &str, source: &str) -> Result<String, String> {
    let result = compile_author_str(name, source);
    format_result(result)
}

/// One author-format pass through a caller-selected reader locale.
pub fn author_format_once_with_session(session: &Session, name: &str, source: &str) -> Result<String, String> {
    let result = super::compile_author(session, name, source);
    format_result(result)
}

fn format_result(result: super::FormatCompileResult) -> Result<String, String> {
    if !result.success() {
        let messages: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| format!("{:?}:{:?}", d.code, d.issue()))
            .collect();
        return Err(format!("author format failed: {messages:?}"));
    }
    result
        .output
        .map(|output| output.code)
        .ok_or_else(|| "author format succeeded without output".to_owned())
}

/// Formatted output must lex and parse without errors.
///
/// # Errors
///
/// Returns a description of the parse failure when the code does not re-parse.
pub fn assert_author_reparses(code: &str, label: &str) -> Result<(), String> {
    lex_parse(code).map_err(|detail| format!("{label}: {detail}"))
}

/// Formatted output must lex and parse under the reader pack that produced it
/// (Latin default when `None`).
///
/// # Errors
///
/// Returns a description of the parse failure when the code does not re-parse.
pub fn assert_author_reparses_with(code: &str, label: &str, pack: Option<&LocalePack>) -> Result<(), String> {
    lex_parse_with_pack(code, pack).map_err(|detail| format!("{label}: {detail}"))
}

/// `author(author(x)) == author(x)` and both passes re-parse.
///
/// The reader pack is resolved from the source's frontmatter locale (Latin
/// default when none), exactly as [`super::compile_author`] does, and the same
/// session drives both passes: author output is body-only (frontmatter is
/// stripped), so the second pass cannot re-resolve a localized locale from the
/// first pass's output.
///
/// # Errors
///
/// Returns a string describing the failure when formatting is not idempotent,
/// or when any pass fails to lex/parse.
pub fn assert_author_idempotent(name: &str, source: &str) -> Result<String, String> {
    let session = author_session(name, source)?;
    assert_author_idempotent_with_session(&session, name, source)
}

/// `author(author(x)) == author(x)` under a caller-selected session; both
/// passes and both re-parse gates use the session's reader pack.
///
/// # Errors
///
/// Returns a string describing the failure when formatting is not idempotent,
/// or when any pass fails to lex/parse.
pub fn assert_author_idempotent_with_session(session: &Session, name: &str, source: &str) -> Result<String, String> {
    let first = author_format_once_with_session(session, name, source)?;
    assert_author_reparses_with_session(session, &first, &format!("{name} (first pass)"))?;
    let second = author_format_once_with_session(session, name, &first)?;
    if first != second {
        return Err(format!("{name}: author(author(x)) != author(x)"));
    }
    assert_author_reparses_with_session(session, &second, &format!("{name} (second pass)"))?;
    Ok(first)
}

/// Formatted output must lex and parse under a session's reader pack.
///
/// # Errors
///
/// Returns a description of the parse failure when the code does not re-parse.
pub fn assert_author_reparses_with_session(session: &Session, code: &str, label: &str) -> Result<(), String> {
    let pack = session.config.locale_pack.as_ref();
    lex_parse_with_pack(code, pack).map_err(|detail| format!("{label}: {detail}"))
}

/// Build the session `compile_author` would use for this source: the reader
/// pack selected by the frontmatter locale, pinned into the session so both
/// idempotency passes lex the same surface.
fn author_session(name: &str, source: &str) -> Result<Session, String> {
    use crate::driver::peel_raw_source;
    use crate::driver::Config;

    let base = Session::new(Config::default().with_dev_stdlib());
    let peeled = peel_raw_source(name, source).map_err(|error| error.to_string())?;
    let mut diagnostics = Vec::new();
    let pack = crate::driver::resolve_peeled_lex_pack(&base, &peeled, &mut diagnostics)
        .map_err(|()| format!("author pack resolution failed: {diagnostics:?}"))?;
    let config = match pack {
        Some(pack) => Config::default()
            .with_dev_stdlib()
            .with_locale_pack(pack)
            .with_locale_pack_explicit(),
        None => Config::default().with_dev_stdlib(),
    };
    Ok(Session::new(config))
}
