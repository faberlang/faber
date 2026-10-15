//! Clean-break contract for the removed ternary spellings (DECISIONS D4.7).
//!
//! `c ? a : b` and `c sic a secus b` (en `yields … else`) were removed in
//! favour of `c ✓ a ✗ b`. Each old spelling reports a named migration
//! diagnostic pointing at `✓ ✗`, not a generic parse error. The retired `sic`
//! stays a reserved control-flow word that carries this diagnostic; `?` keeps
//! its optional-chain forms.

use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok};

#[test]
fn question_colon_reports_migration_to_check_ballot() {
    assert_parse_error_matches(
        "sit _ v ← c ? 1 : 2",
        ParseExpectation::Issue("conditional_question_colon_removed"),
    );
}

#[test]
fn sic_secus_reports_migration_to_check_ballot() {
    assert_parse_error_matches(
        "sit _ v ← c sic 1 secus 2",
        ParseExpectation::Issue("conditional_sic_removed"),
    );
}

/// Pins the migration diagnostic by structure: error kind, code-bearing issue
/// key and the exact arg list, never prose.
fn assert_sic_migration_args(source: &str) {
    let result = crate::test_support::parse_program(source);
    let error = result
        .errors
        .iter()
        .find(|error| {
            error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "conditional_sic_removed")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected conditional_sic_removed for {source:?}: {:?}",
                result.errors
            )
        });
    assert_eq!(error.kind, crate::ParseErrorKind::ExpressionInvalid);
    assert_eq!(
        error.args.len(),
        1,
        "issue is the only arg: {:?}",
        error.args
    );
}

#[test]
fn sic_migration_diagnostic_keeps_kind_issue_and_args() {
    assert_sic_migration_args("sit _ v ← c sic 1 secus 2");
    assert_sic_migration_args("sit _ v ← a ∧ b sic 1 secus 2");
}

#[test]
fn retired_words_are_ordinary_identifiers() {
    // KW-1: `sic` and the retired predicate words left the registry. Only the
    // exact `c sic <value>` shape on one line reports the migration; a binding
    // or reference by that name parses.
    let _ = parse_ok("sit _ sic ← 1\nsit _ v ← sic");
    let _ = parse_ok("sit _ v ← c\nsit _ w ← 2\nsit _ positivum ← 3\nsit _ nonnulla ← positivum");
    let _ = parse_ok("sit _ v ← a\nsic ← 2");
}

#[test]
fn optional_chain_forms_survive_the_removal() {
    let _ = parse_ok("sit _ v ← a?.b\nsit _ w ← a?[0]\nsit _ x ← a?(1)");
}
