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
const EXPR_CASES: [(&str, &str); 28] = [
    ("conditional_else_marker", "sit _ x ← a ✓ b"),
    ("call_arguments_end", "sit _ x ← f(1"),
    ("index_end", "sit _ x ← a[1"),
    ("index_coordinate_comma", "sit _ x ← a[1, 2"),
    ("gradient_selectors_end", "sit _ x ← loss(x) ∇ [x"),
    ("optional_index_end", "sit _ x ← a?[1"),
    ("optional_call_arguments_end", "sit _ x ← a?(1"),
    ("non_null_index_end", "sit _ x ← a![1"),
    ("non_null_call_arguments_end", "sit _ x ← a!(1"),
    ("typed_constructor_fields_end", "sit _ x ← Point { a = 1"),
    ("inferred_constructor_elements_end", "sit _ x ← _ [1"),
    ("array_literal_end", "sit _ x ← [1"),
    ("parenthesized_expression_end", "sit _ x ← (1"),
    ("ratio_type_arguments_end", "sit _ x ← ratio<g: f32"),
    ("ratio_fields_start", "sit _ x ← ratio<g: f32> 1"),
    ("ratio_fields_end", "sit _ x ← ratio<g: f32> { g = 1.0"),
    ("type_constructor_fields_end", "sit _ x ← pkg.Box { a = 1"),
    ("finge_fields_end", "sit _ x ← finge Bonum { a = 1"),
    ("iuncta_type_arguments_start", "sit _ x ← iuncta [1]"),
    ("iuncta_elements_start", "sit _ x ← iuncta<numerus> 1"),
    ("iuncta_elements_end", "sit _ x ← iuncta<numerus> [1"),
    ("ad_opener_end", "sit _ x ← ad 'route'(1"),
    ("computed_key_end", "sit _ x ← Point { a = 1, [k"),
    ("json_key_separator", "sit _ x ← { \"a\" 1 }"),
    ("json_object_end", "sit _ x ← { \"a\": 1"),
    ("json_array_end", "sit _ x ← { \"a\": [1"),
    ("inferred_field_constructor_end", "sit _ x ← _ { a = 1"),
    ("scriptum_end", "sit _ x ← scriptum(\"a\""),
];

/// Contracts that no source can surface as a diagnostic. They still route
/// through the issue so the guard can move without reintroducing prose.
///
/// - `ratio_type_arguments_start`: the `ratio` constructor claims only when
///   `<` follows.
/// - `closure_parameters_end`, `closure_body_marker`: the compact closure
///   parses speculatively and rolls back every error raised before `∴` is
///   consumed, so the parenthesized or type-first reading takes over.
/// - `argument_label_separator`: `argument_label_start` already proved the
///   `:` is next.
/// - `call_type_arguments_end`, `constructor_type_arguments_start`,
///   `constructor_type_arguments_end`: both type-argument lists parse
///   speculatively and a failure restores the cursor, so `<` falls back to
///   comparison.
/// - `scriptum_start`: `scriptum` is claimed only when `(` follows.
const GUARDED_SLUGS: [&str; 8] = [
    "ratio_type_arguments_start",
    "closure_parameters_end",
    "closure_body_marker",
    "argument_label_separator",
    "call_type_arguments_end",
    "constructor_type_arguments_start",
    "constructor_type_arguments_end",
    "scriptum_start",
];

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
