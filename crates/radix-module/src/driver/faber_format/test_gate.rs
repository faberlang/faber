//! Shared format gates for the canonical (HIR) pipeline.
//!
//! Used by Faber re-emit tests and `faber-cli` format tests so one definition
//! drives re-parse and idempotency checks.

use super::reemit_faber;
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

/// Lex and parse Faber source under an explicit reader pack (the no-locale
/// default when `None`), returning a human-readable error on failure.
///
/// Canonical-format output is emitted in the pack that lexed the input, so the
/// re-parse gate must use that same pack: a Latin-default re-parse would reject
/// (or mis-parse) localized keyword spellings.
///
/// # Errors
///
/// Returns a string with lex or parse error details when the source is invalid.
pub fn lex_parse_with_pack(code: &str, pack: Option<&LocalePack>) -> Result<(), String> {
    let lex = match pack {
        Some(pack) => lexer::lex_with_locale_pack(code, pack),
        None => match crate::driver::default_code_pack(&crate::driver::Config::default()) {
            Some(pack) => lexer::lex_with_locale_pack(code, &pack),
            None => lexer::lex(code),
        },
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

/// One canonical (HIR) format pass via [`super::reemit_faber`].
///
/// The session pack is the source's own locale (frontmatter, else Latin
/// default), matching the D-B format entry.
///
/// # Errors
///
/// Returns diagnostic error messages when formatting fails, or when formatting
/// succeeds without producing output.
pub fn canonical_format_once(name: &str, source: &str) -> Result<String, String> {
    let session = source_locale_session(name, source)?;
    canonical_format_once_with_session(&session, name, source)
}

/// One canonical (HIR) format pass through a caller-selected reader locale.
///
/// # Errors
///
/// Returns diagnostic error messages when formatting fails, or when formatting
/// succeeds without producing output.
pub fn canonical_format_once_with_session(session: &Session, name: &str, source: &str) -> Result<String, String> {
    let emit_pack = session
        .config
        .locale_pack
        .as_ref()
        .ok_or_else(|| "canonical format session has no explicit locale pack".to_owned())?;
    let result = reemit_faber(session, emit_pack, name, source);
    format_result("canonical", result)
}

fn format_result(kind: &str, result: super::FaberReemitResult) -> Result<String, String> {
    if !result.success() {
        let messages: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| format!("{:?}:{:?}", d.code, d.issue()))
            .collect();
        return Err(format!("{kind} format failed: {messages:?}"));
    }
    result
        .output
        .map(|output| output.code)
        .ok_or_else(|| format!("{kind} format succeeded without output"))
}

/// `canonical(canonical(x)) == canonical(x)` and both passes re-parse under
/// the source's own locale pack.
///
/// # Errors
///
/// Returns a string describing the failure when formatting is not idempotent,
/// or when any pass fails to lex/parse.
pub fn assert_canonical_idempotent(name: &str, source: &str) -> Result<String, String> {
    let session = source_locale_session(name, source)?;
    assert_canonical_idempotent_with_session(&session, name, source)
}

/// `canonical(canonical(x)) == canonical(x)` under a caller-selected session;
/// both passes and both re-parse gates use the session's reader pack.
///
/// # Errors
///
/// Returns a string describing the failure when formatting is not idempotent,
/// or when any pass fails to lex/parse.
pub fn assert_canonical_idempotent_with_session(session: &Session, name: &str, source: &str) -> Result<String, String> {
    let first = canonical_format_once_with_session(session, name, source)?;
    assert_canonical_reparses_with_session(session, &first, &format!("{name} (first pass)"))?;
    let second = canonical_format_once_with_session(session, name, &first)?;
    if first != second {
        return Err(format!("{name}: canonical(canonical(x)) != canonical(x)"));
    }
    assert_canonical_reparses_with_session(session, &second, &format!("{name} (second pass)"))?;
    Ok(first)
}

/// Canonical output must lex and parse without errors (Latin default pack).
///
/// # Errors
///
/// Returns a description of the parse failure when the code does not re-parse.
pub fn assert_canonical_reparses(code: &str, label: &str) -> Result<(), String> {
    lex_parse(code).map_err(|detail| format!("{label}: {detail}"))
}

/// Canonical output must lex and parse under the reader pack that produced it
/// (Latin default when `None`).
///
/// # Errors
///
/// Returns a description of the parse failure when the code does not re-parse.
pub fn assert_canonical_reparses_with(code: &str, label: &str, pack: Option<&LocalePack>) -> Result<(), String> {
    lex_parse_with_pack(code, pack).map_err(|detail| format!("{label}: {detail}"))
}

/// Canonical output must lex and parse under a session's reader pack.
///
/// # Errors
///
/// Returns a description of the parse failure when the code does not re-parse.
pub fn assert_canonical_reparses_with_session(session: &Session, code: &str, label: &str) -> Result<(), String> {
    let pack = session.config.locale_pack.as_ref();
    lex_parse_with_pack(code, pack).map_err(|detail| format!("{label}: {detail}"))
}

/// Build the session a format entry uses for this source: the reader pack
/// selected by the frontmatter locale, pinned into the session so both
/// idempotency passes lex the same surface.
fn source_locale_session(name: &str, source: &str) -> Result<Session, String> {
    use crate::driver::Config;
    use crate::driver::peel_raw_source;

    let base = Session::new(Config::default().with_dev_locale_root());
    let peeled = peel_raw_source(name, source).map_err(|error| error.to_string())?;
    let mut diagnostics = Vec::new();
    let pack = crate::driver::resolve_peeled_lex_pack(&base, &peeled, &mut diagnostics)
        .map_err(|()| format!("source locale pack resolution failed: {diagnostics:?}"))?;
    let config = match pack {
        Some(pack) => Config::default()
            .with_dev_locale_root()
            .with_resolved_locale_pack(pack),
        None => Config::default()
            .with_dev_locale_root()
            .with_resolved_locale_pack(crate::locale::latin_locale_pack()),
    };
    Ok(Session::new(config))
}
