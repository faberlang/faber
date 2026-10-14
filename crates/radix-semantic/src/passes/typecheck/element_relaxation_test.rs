//! Regression coverage for tensor element relaxation — non-numeric element types in tensor construction and arithmetic.
//!
//! These tests prove that tensor<textus> construction via `structa`/`accipe` is
//! accepted while elementwise arithmetic on non-numeric tensor elements is
//! rejected with a targeted diagnostic.

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
}

fn accepts(result: &crate::SemanticResult) {
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

fn rejects_with_issue(result: &crate::SemanticResult, issue: &str) {
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn tensor_textus_construction_and_accipe_typecheck() {
    accepts(&compile(
        r#"incipit {
  fixum lista<textus> flat ← ["a", "b", "c"]
  fixum tensor<textus, [3]> grid ← flat ↦ tensor<textus, [3]>
  fixum textus cell ← grid.accipe([0]) vel ""
  nota cell
}"#,
    ));
}

#[test]
fn tensor_textus_plus_rejects() {
    rejects_with_issue(
        &compile(
            r#"incipit {
  fixum lista<textus> flat ← ["a", "b", "c"]
  fixum tensor<textus, [3]> left ← flat ↦ tensor<textus, [3]>
  fixum tensor<textus, [3]> right ← flat ↦ tensor<textus, [3]>
  fixum _ sum ← left + right
}"#,
        ),
        "tensor_arithmetic_numeric_element_required",
    );
}
