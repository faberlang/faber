//! ASCII `<` / `>` retired as comparison operators (TZ-2).
//!
//! Comparison is `≺ ≻ ≤ ≥`; `<` and `>` are generic delimiters only. An ASCII
//! one in comparison position records `PARSE030 comparison_ascii_retired`
//! (args `operator`, `glyph`) and still parses as the comparison so recovery
//! continues. Tests assert code, issue and args only, never prose.

use super::ParseResult;
use crate::test_support::parse_program_la;

const ISSUE: &str = "comparison_ascii_retired";

fn parse_source(source: &str) -> ParseResult {
    parse_program_la(source)
}

/// The `(operator, glyph)` args of every retired-comparison error, in order.
fn retired(source: &str) -> Vec<(String, String)> {
    let result = parse_source(source);
    result
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
            (arg("operator"), arg("glyph"))
        })
        .collect()
}

fn pair(op: &str, glyph: &str) -> (String, String) {
    (op.to_owned(), glyph.to_owned())
}

#[test]
fn ascii_less_than_is_rejected_in_if() {
    assert_eq!(
        retired("incipit { si a < b { nulla } }"),
        vec![pair("<", "≺")],
        "ascii `<` in an if condition"
    );
}

#[test]
fn ascii_greater_than_is_rejected_in_while() {
    assert_eq!(
        retired("incipit { dum a > b { nulla } }"),
        vec![pair(">", "≻")],
        "ascii `>` in a while condition"
    );
}

#[test]
fn chained_ascii_comparisons_all_report() {
    assert_eq!(
        retired("sit _ x ← a < b aut c > d"),
        vec![pair("<", "≺"), pair(">", "≻")]
    );
}

#[test]
fn glyph_comparisons_are_accepted() {
    for source in [
        "incipit { si a ≺ b { nulla } }",
        "incipit { dum a ≻ b { nulla } }",
        "incipit { si a ≤ b { nulla } }",
        "incipit { si a ≥ b { nulla } }",
        "sit _ x ← a ≺ b aut c ≻ d",
    ] {
        let result = parse_source(source);
        assert!(result.success(), "{source}: {:?}", result.errors);
    }
}

#[test]
fn generic_angle_brackets_are_untouched() {
    for source in [
        "sit lista<numerus> xs ← []",
        "sit tabula<textus, lista<numerus>> m ← {}",
        "sit _ n ← f<numerus>(x)",
    ] {
        assert!(retired(source).is_empty(), "{source}");
    }
}

#[test]
fn comparison_inside_a_generic_argument_list_still_reports() {
    // The comparison sits in the call's argument, not between the angle brackets.
    assert_eq!(retired("sit _ n ← f<numerus>(a < b)"), vec![pair("<", "≺")]);
}
