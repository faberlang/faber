//! Numeric tensor `+` requires exact shapes; unretired tensor methods
//! retain their separate broadcast contract. Vector and matrix rules are unchanged.

use radix_diagnostics::DiagnosticArg;

fn accepts(source: &str) {
    let result = crate::test_support::analyze_source_la(source);
    assert!(
        result.is_ok(),
        "expected typecheck ok: {:?}",
        result.diagnostics
    );
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = crate::test_support::analyze_source_la(source);
    assert!(
        !result.is_ok(),
        "expected typecheck failure, but source was accepted"
    );
    let diagnostics = result.diagnostics.clone();
    let issue_arg = DiagnosticArg::new("issue", issue);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error() && d.args.contains(&issue_arg)),
        "expected a diagnostic with issue `{issue}`, got: {diagnostics:?}"
    );
}

#[test]
fn tensor_plus_same_shape_typechecks() {
    accepts(
        r"functio sum(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> {
    redde a + b
}",
    );
}

#[test]
fn vector_plus_same_shape_typechecks() {
    accepts(
        r"functio sum(vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b) → vector<f32, magnitudo 3> {
    redde a + b
}",
    );
}

#[test]
fn matrix_plus_same_shape_typechecks() {
    accepts(
        r"functio sum(matrix<f32, [2, 2]> a, matrix<f32, [2, 2]> b) → matrix<f32, [2, 2]> {
    redde a + b
}",
    );
}

#[test]
fn tensor_plus_implicit_broadcast_rejects() {
    rejects_with_issue(
        r"incipit {
  sit tf32[2] a ← vacua
  sit tf32[2, 1] b ← vacua
  sit tf32[2, 2] c ← a + b
  nota c.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn tensor_plus_incompatible_shapes_reject() {
    rejects_with_issue(
        r"incipit {
  sit tf32[3] a ← vacua
  sit tf32[2] b ← vacua
  sit tf32[3] c ← a + b
  nota c.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn tensor_plus_non_numeric_element_rejects() {
    rejects_with_issue(
        r"incipit {
  sit tensor<textus, [2]> a ← vacua
  sit tensor<textus, [2]> b ← vacua
  sit tensor<textus, [2]> c ← a + b
  nota c.longitudo()
}",
        "tensor_arithmetic_numeric_element_required",
    );
}

#[test]
fn tensor_plus_textus_rejects() {
    rejects_with_issue(
        r"functio sum(tensor<f32, [2]> a, textus s) → textus {
    redde a + s
}",
        "string_operands_required",
    );
}

#[test]
fn tensor_plus_linear_matmul_bias_typechecks() {
    accepts(
        r"functio linear<magnitudo M, magnitudo K, magnitudo N>(
    tensor<f32, [M, K]> x,
    tensor<f32, [K, N]> w,
    tensor<f32, [M, N]> b
) → tensor<f32, [M, N]> {
    redde (x · w) + b
}",
    );
}
