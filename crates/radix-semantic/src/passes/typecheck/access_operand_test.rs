//! Regression coverage for structured diagnostic issue arguments on index/access operand type mismatches.
//!
//! These tests prove that textus, lista, tabula, and unsupported index receivers
//! each emit a targeted issue string rather than a generic type mismatch.

use radix_diagnostics::DiagnosticArg;
use radix_target::Target;

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source(source)
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
