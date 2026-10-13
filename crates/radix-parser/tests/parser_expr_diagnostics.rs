//! Issue-identity proof for expression-parser token contracts.
//!
//! Each case parses the smallest source that breaks one punctuation contract
//! and asserts the `PARSE001` diagnostic names its stable issue instead of
//! carrying English prose. Rendering belongs to the reader packs; the pack
//! check here only proves every locale ships exactly one row for the issue.

use radix_lexer::lex;
use radix_parser::{ParseError, ParseErrorKind, parse};
use std::fs;
use std::path::PathBuf;

const LOCALES: [&str; 8] = ["en", "la", "ar", "hi", "vi", "th-TH", "zh-Hans", "zh-Hant"];

/// Minimal broken source per expression-level token contract.
const EXPR_CASES: [(&str, &str); 18] = [
    ("conditional_else_marker", "fixum _ x ← a ✓ b"),
    ("call_arguments_end", "fixum _ x ← f(1"),
    ("index_end", "fixum _ x ← a[1"),
    ("gradient_selectors_end", "fixum _ x ← loss(x) ∇ [x"),
    ("optional_index_end", "fixum _ x ← a?[1"),
    ("optional_call_arguments_end", "fixum _ x ← a?(1"),
    ("non_null_index_end", "fixum _ x ← a![1"),
    ("non_null_call_arguments_end", "fixum _ x ← a!(1"),
    ("first_match_source_separator", "fixum _ x ← primus_quem(a"),
    ("first_match_predicate_tail", "fixum _ x ← primus_quem(a, 1"),
    (
        "first_match_end",
        "fixum _ x ← primus_quem(a, ubi y { y ≻ 0.0 }",
    ),
    ("typed_constructor_fields_end", "fixum _ x ← Point { a = 1"),
    ("inferred_constructor_elements_end", "fixum _ x ← _ [1"),
    ("array_literal_end", "fixum _ x ← [1"),
    ("parenthesized_expression_end", "fixum _ x ← (1"),
    ("ratio_type_arguments_end", "fixum _ x ← ratio<g: f32"),
    ("ratio_fields_start", "fixum _ x ← ratio<g: f32> 1"),
    ("ratio_fields_end", "fixum _ x ← ratio<g: f32> { g = 1.0"),
];

/// Contracts whose dispatch already guarantees the token, so no source can
/// break them: `primus_quem` claims only when `(` follows, and the `ratio`
/// constructor claims only when `<` follows. They still route through the
/// issue so the guard can move without reintroducing prose.
const GUARDED_SLUGS: [&str; 2] = ["first_match_start", "ratio_type_arguments_start"];

fn issue_of(error: &ParseError) -> Option<&str> {
    error
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.as_str())
}

fn pack_issue_headers(locale: &str) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../locale")
        .join(locale)
        .join("pack.toml");
    let text = fs::read_to_string(&path).expect("reader pack is readable");
    text.lines()
        .filter(|line| line.starts_with("[diagnostics.PARSE001.issues."))
        .map(str::to_owned)
        .collect()
}

fn all_slugs() -> Vec<&'static str> {
    EXPR_CASES
        .iter()
        .map(|(slug, _)| *slug)
        .chain(GUARDED_SLUGS)
        .collect()
}

#[test]
fn expression_token_contracts_name_their_issue() {
    for (slug, source) in EXPR_CASES {
        let result = parse(lex(source));
        let error = result
            .errors
            .iter()
            .find(|error| issue_of(error) == Some(slug))
            .unwrap_or_else(|| {
                panic!(
                    "{slug}: expected a PARSE001 issue for {source:?}, got {:?}",
                    result.errors
                )
            });
        assert_eq!(error.kind, ParseErrorKind::Expected, "{slug}");
        assert!(
            error.args.iter().all(|arg| arg.name != "expected"),
            "{slug}: the issue must not also carry an expected-prose argument"
        );
        assert!(
            error.args.iter().all(|arg| arg.name != "legacy_message"),
            "{slug}: the issue must not carry a legacy message"
        );
    }
}

#[test]
fn expression_issue_slugs_are_unique() {
    let mut slugs = all_slugs();
    let total = slugs.len();
    slugs.sort_unstable();
    slugs.dedup();
    assert_eq!(slugs.len(), total);
}

#[test]
fn expression_issue_rows_exist_once_in_all_locale_packs() {
    for locale in LOCALES {
        let headers = pack_issue_headers(locale);
        for slug in all_slugs() {
            let header = format!("[diagnostics.PARSE001.issues.{slug}]");
            let rows = headers.iter().filter(|line| **line == header).count();
            assert_eq!(
                rows, 1,
                "{locale}.{slug} must have exactly one PARSE001 issue row"
            );
        }
    }
}
