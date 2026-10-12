//! Shared helpers for parser unit tests.
//!
//! Provides convenience functions (`parse_ok`, `span_for`, `assert_parse_error_matches`) that
//! reduce boilerplate in parser test modules. No test logic lives here — only reusable
//! assertions and parsing wrappers.

use super::{ParseError, ParseErrorKind, ParseResult, parse};
use radix_diagnostics::DiagnosticArg;
use radix_lexer::Span;
use radix_lexer::{Symbol, lex};

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

/// Parse source and return the raw result (success or collected errors).
#[must_use]
pub fn parse_program(source: &str) -> ParseResult {
    parse(lex(source))
}

/// Parse `source` and require that the only errors are the retired ASCII
/// comparison diagnostic (`comparison_ascii_retired`), at least one of them.
///
/// For tests that exercise the `<` / `>` generics-vs-comparison split, whose
/// ASCII comparison still recovers as a comparison but is no longer clean.
#[must_use]
pub fn parse_recovering_ascii_comparison(source: &str) -> ParseResult {
    let result = parse_program(source);
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
    let result = parse_program(source);
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
    let result = parse_program(source);
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
    let result = parse_program(source);
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
