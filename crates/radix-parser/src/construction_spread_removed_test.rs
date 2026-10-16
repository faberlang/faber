//! Clean-break contract for `sparge` inside construction literals (DECISIONS D15.4).
//!
//! `sparge` was removed from construction literals (`Genus { … }`). Wherever it
//! appears among the fields the parser reports the named diagnostic
//! `construction_spread_removed`, which points at copy-with-changes. `sparge`
//! stays legal in list literals and call arguments.

use crate::ParseErrorKind;
use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok};

#[test]
fn trailing_sparge_in_construction_reports_named_diagnostic() {
    assert_parse_error_matches(
        "sit _ q ← Punctum { x = 9, sparge p }",
        ParseExpectation::Args(&[("issue", "construction_spread_removed")]),
    );
    assert_parse_error_matches(
        "sit _ q ← Punctum { x = 9, sparge p }",
        ParseExpectation::Kind(ParseErrorKind::Unexpected),
    );
}

#[test]
fn leading_sparge_in_construction_reports_named_diagnostic() {
    assert_parse_error_matches(
        "sit _ q ← Punctum { sparge p, x = 9 }",
        ParseExpectation::Args(&[("issue", "construction_spread_removed")]),
    );
    assert_parse_error_matches(
        "sit _ q ← Punctum { sparge p }",
        ParseExpectation::Args(&[("issue", "construction_spread_removed")]),
    );
}

#[test]
fn sparge_in_qualified_construction_reports_named_diagnostic() {
    assert_parse_error_matches(
        "sit _ q ← geo.Punctum { sparge p, x = 9 }",
        ParseExpectation::Args(&[("issue", "construction_spread_removed")]),
    );
}

#[test]
fn sparge_stays_legal_in_lists_and_calls() {
    let result = parse_ok("sit _ xs ← [1, sparge ys]\nsit _ r ← f(sparge ys)\n");
    assert_eq!(result.program.expect("program").statements.len(), 2);
}
