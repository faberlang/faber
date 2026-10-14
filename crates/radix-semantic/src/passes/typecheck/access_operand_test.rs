//! Regression coverage for structured diagnostic issue arguments on index/access operand type mismatches.
//!
//! These tests prove that textus, lista, tabula, and unsupported index receivers
//! each emit a targeted issue string rather than a generic type mismatch.

use radix_diagnostics::DiagnosticArg;

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = compile(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.args == vec![DiagnosticArg::new("issue", issue)]),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn textus_index_rejects_non_numeric_index_with_structured_issue_arg() {
    rejects_with_issue(
        r#"incipit {
  fixum textus value ← "abc"
  nota value["x"]
}"#,
        "textus_index_must_be_numerus_or_range",
    );
}

#[test]
fn lista_index_rejects_non_numeric_index_with_structured_issue_arg() {
    rejects_with_issue(
        r#"incipit {
  fixum lista<numerus> value ← [1, 2, 3]
  nota value["x"]
}"#,
        "array_index_must_be_numerus",
    );
}

#[test]
fn unsupported_index_receiver_uses_structured_issue_arg() {
    rejects_with_issue(
        r"incipit {
  fixum numerus value ← 1
  nota value[0]
}",
        "indexing_requires_array_or_map",
    );
}

#[test]
fn map_index_type_mismatch_uses_structured_issue_arg() {
    rejects_with_issue(
        r"incipit {
  fixum tabula<textus, numerus> value ← vacua
  nota value[1]
}",
        "map_index_type_mismatch",
    );
}

// --- tuple receivers through `!` and `?` index segments ---
//
// `t[0]` resolves the element by position. The non-null and optional-chain
// index segments must resolve it the same way, not fall to the error type.

#[test]
fn non_null_index_on_tuple_yields_the_element_type() {
    let result = compile(
        r"functio second(iuncta<numerus, textus> t) → textus {
    fixum textus s ← t![1]
    redde s
}",
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn non_null_index_on_tuple_checks_the_bound() {
    rejects_with_issue(
        r"functio past(iuncta<numerus, textus> t) → numerus {
    fixum numerus s ← t![5]
    redde s
}",
        "tuple_index_out_of_bounds",
    );
}

#[test]
fn optional_chain_index_on_tuple_yields_the_element_type() {
    let accepted = compile(
        r"functio second(iuncta<numerus, textus> t) → textus ∪ nihil {
    fixum textus ∪ nihil s ← t?[1]
    redde s
}",
    );
    assert!(accepted.is_ok(), "diagnostics: {:?}", accepted.diagnostics);
    // The element type is real: a mismatched slot is refused.
    let rejected = compile(
        r"functio wrong(iuncta<numerus, textus> t) → numerus ∪ nihil {
    fixum numerus ∪ nihil s ← t?[1]
    redde s
}",
    );
    assert!(
        !rejected.is_ok(),
        "expected a type error, got success: {:?}",
        rejected.diagnostics
    );
}
