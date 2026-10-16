//! `≠` (U+2260) retired as the negation of `≡` (ruling 6b5c1c615, need
//! a99bbff8). `≢` is the one negation. A `≠` in equality position records
//! `PARSE030 neq_glyph_retired` (args `glyph`, `replacement`) and still parses
//! as `NotEq` so recovery continues. Tests assert code, issue and args only.

use super::ParseResult;
use crate::test_support::parse_program_la;

const ISSUE: &str = "neq_glyph_retired";

fn parse_source(source: &str) -> ParseResult {
    parse_program_la(source)
}

/// The `(glyph, replacement)` args of every retired-glyph error, in order.
fn retired(source: &str) -> Vec<(String, String)> {
    parse_source(source)
        .errors
        .iter()
        .filter(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == ISSUE)
        })
        .map(|err| {
            let arg = |name: &str| {
                err.args
                    .iter()
                    .find(|arg| arg.name == name)
                    .map(|arg| arg.value.clone())
                    .unwrap_or_default()
            };
            (arg("glyph"), arg("replacement"))
        })
        .collect()
}

#[test]
fn retired_not_equal_glyph_names_the_replacement() {
    assert_eq!(
        retired("incipit { si a ≠ b { nulla } }"),
        vec![("≠".to_owned(), "≢".to_owned())]
    );
}

#[test]
fn every_retired_not_equal_site_reports() {
    assert_eq!(retired("sit _ x ← a ≠ b aut c ≠ d").len(), 2);
}

#[test]
fn negation_glyph_is_accepted() {
    let result = parse_source("incipit { si a ≢ b { nulla } }");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
}
