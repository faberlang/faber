//! Shared helpers for parser unit tests.
//!
//! Provides convenience functions (`parse_ok`, `span_for`, `assert_parse_error_matches`) that
//! reduce boilerplate in parser test modules. No test logic lives here — only reusable
//! assertions and parsing wrappers.

use super::{ParseError, ParseErrorKind, ParseResult, parse};
use radix_diagnostics::DiagnosticArg;
use radix_lexer::Span;
use radix_lexer::{KeywordPack, Symbol, TokenKind, lex, lex_with_locale_pack, lookup_keyword_spec};

/// Byte span covering `start..end` in hand-built fixtures.
#[must_use]
pub fn test_span_between(start: u32, end: u32) -> Span {
    Span::new(start, end)
}

/// Whether `args` omit legacy prose attachments.
#[must_use]
pub fn args_have_no_legacy_message(args: &[DiagnosticArg]) -> bool {
    !args.iter().any(|arg| arg.name == "legacy_message")
}

/// Assert structured args omit legacy prose attachments.
///
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_args_have_no_legacy_message(args: &[DiagnosticArg], context: &str) {
    assert!(
        args_have_no_legacy_message(args),
        "{context}: args must not carry legacy_message"
    );
}

/// Latin test pack for the pack-aware helpers below: canonical Latin keyword
/// spellings plus the historical `penes`/`exemplum` aliases.
struct LatinTestPack;

impl KeywordPack for LatinTestPack {
    fn pack_id(&self) -> &'static str {
        "la"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        const ALIASES: [(&str, &str); 2] = [("penes", "own"), ("exemplum", "copy")];
        if let Some(latin_key) = ALIASES
            .iter()
            .find_map(|(alias, latin_key)| (*alias == text).then_some(*latin_key))
        {
            return Some(latin_key);
        }
        lookup_keyword_spec(text)
            .filter(|spec| spec.token_kind.is_some())
            .map(|spec| spec.text)
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_program_with_pack(source: &str, pack: &dyn KeywordPack) -> ParseResult {
    parse(lex_with_locale_pack(source, pack))
}

/// Parse source and return the raw result (success or collected errors).
#[must_use]
pub fn parse_program(source: &str) -> ParseResult {
    parse(lex(source))
}

/// Latin-surface sibling of [`parse_program`], lexed through [`LatinTestPack`].
#[must_use]
pub fn parse_program_la(source: &str) -> ParseResult {
    parse_program_with_pack(source, &LatinTestPack)
}

/// Parse `source` and require that the only errors are the retired ASCII
/// comparison diagnostic (`comparison_ascii_retired`), at least one of them.
///
/// For tests that exercise the `<` / `>` generics-vs-comparison split, whose
/// ASCII comparison still recovers as a comparison but is no longer clean.
#[must_use]
pub fn parse_recovering_ascii_comparison(source: &str) -> ParseResult {
    require_recovering_ascii_comparison(parse_program(source))
}

/// Latin-surface sibling of [`parse_recovering_ascii_comparison`].
#[must_use]
pub fn parse_recovering_ascii_comparison_la(source: &str) -> ParseResult {
    require_recovering_ascii_comparison(parse_program_la(source))
}

fn require_recovering_ascii_comparison(result: ParseResult) -> ParseResult {
    assert!(
        !result.errors.is_empty()
            && result
                .errors
                .iter()
                .all(|err| has_arg(err, "issue", "comparison_ascii_retired")),
        "expected only comparison_ascii_retired errors, got: {:?}",
        result.errors
    );
    result
}

/// Parse source and assert success before returning the result.
///
/// # Panics
///
/// Panics if parsing fails (any parse errors).
#[must_use]
pub fn parse_ok(source: &str) -> ParseResult {
    require_parse_success(parse_program(source))
}

/// Latin-surface sibling of [`parse_ok`].
#[must_use]
pub fn parse_ok_la(source: &str) -> ParseResult {
    require_parse_success(parse_program_la(source))
}

fn require_parse_success(result: ParseResult) -> ParseResult {
    assert!(
        result.success(),
        "expected parse success, got errors: {:?}",
        result
            .errors
            .iter()
            .map(|err| format!("{err:?}"))
            .collect::<Vec<_>>()
    );
    result
}

#[must_use]
pub fn symbol_name(result: &ParseResult, symbol: Symbol) -> &str {
    result.interner.resolve(symbol)
}

/// Find the span of a literal substring for compact test assertions.
///
/// # Panics
///
/// Panics if `needle` is not found in `source`.
#[must_use]
pub fn span_for(source: &str, needle: &str) -> Span {
    let start = u32::try_from(source.find(needle).expect("needle in source"))
        .expect("test fixture byte offset fits u32");
    test_span_between(
        start,
        start + u32::try_from(needle.len()).expect("test fixture byte length fits u32"),
    )
}

#[derive(Clone, Copy)]
pub enum ParseExpectation {
    Kind(ParseErrorKind),
    Arg {
        name: &'static str,
        value: &'static str,
    },
    Args(&'static [(&'static str, &'static str)]),
    ArgPrefix {
        name: &'static str,
        prefix: &'static str,
    },
    Issue(&'static str),
}

/// Assert that parsing `source` produces an error matching the expected pattern.
///
/// # Panics
///
/// Panics if no parse error matches the expected pattern.
pub fn assert_parse_error_matches(source: &str, expected: ParseExpectation) {
    assert_result_has_matching_error(&parse_program(source), expected);
}

/// Latin-surface sibling of [`assert_parse_error_matches`].
pub fn assert_parse_error_matches_la(source: &str, expected: ParseExpectation) {
    assert_result_has_matching_error(&parse_program_la(source), expected);
}

fn assert_result_has_matching_error(result: &ParseResult, expected: ParseExpectation) {
    assert!(
        result
            .errors
            .iter()
            .any(|err| parse_error_matches_expected(err, expected)),
        "expected structured parse error, got {:?}",
        result.errors
    );
}

/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_parse_error_issue(source: &str, issue: &str) {
    assert_result_has_issue(&parse_program(source), issue);
}

/// Latin-surface sibling of [`assert_parse_error_issue`].
///
/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
pub fn assert_parse_error_issue_la(source: &str, issue: &str) {
    assert_result_has_issue(&parse_program_la(source), issue);
}

fn assert_result_has_issue(result: &ParseResult, issue: &str) {
    assert!(
        result.errors.iter().any(|err| has_arg(err, "issue", issue)),
        "expected parse error issue {issue:?}, got {:?}",
        result.errors
    );
}

pub fn assert_parse_error_has_no_legacy_message(err: &ParseError) {
    assert_args_have_no_legacy_message(
        &err.args,
        "migrated diagnostic should not choose prose at emit site",
    );
}

#[must_use]
pub fn parse_error_matches_expected(err: &ParseError, expected: ParseExpectation) -> bool {
    match expected {
        ParseExpectation::Kind(kind) => err.kind == kind,
        ParseExpectation::Arg { name, value } => has_arg(err, name, value),
        ParseExpectation::Args(args) => args.iter().all(|(name, value)| has_arg(err, name, value)),
        ParseExpectation::ArgPrefix { name, prefix } => err
            .args
            .iter()
            .any(|arg| arg.name == name && arg.value.starts_with(prefix)),
        ParseExpectation::Issue(issue) => has_arg(err, "issue", issue),
    }
}

#[must_use]
pub fn has_arg(err: &ParseError, name: &str, value: &str) -> bool {
    err.args
        .iter()
        .any(|arg| arg.name == name && arg.value == value)
}

#[cfg(test)]
mod latin_test_pack_tests {
    use super::{parse_ok_la, parse_program, parse_program_la};

    #[test]
    fn latin_test_pack_parses_incipit() {
        let result = parse_ok_la("incipit { }");
        assert!(result.success());
    }

    #[test]
    fn latin_test_pack_matches_packless_surface() {
        let source = "generis numerus LIMES = 4 * 1024";
        assert_eq!(
            parse_program_la(source).errors.len(),
            parse_program(source).errors.len()
        );
    }
}
