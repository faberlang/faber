//! Regression coverage for matrix intrinsic method typecheck rules.
//!
//! These tests prove that `matrix<T, S>` receivers accept elementwise arithmetic
//! (`addita`, `subtracta`, `multiplicata`, `divisa`), matrix multiplication (`matmul`),
//! and vector application (`applica`) with matching shapes, and reject shape
//! mismatches or non-numeric element types with targeted diagnostics.

use radix_diagnostics::DiagnosticArg;

fn accepts(source: &str) {
    let result = crate::test_support::analyze_source(source);
    assert!(
        result.is_ok(),
        "expected typecheck ok: {:?}",
        result.diagnostics
    );
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = crate::test_support::analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected typecheck failure, but source was accepted"
    );
    let diagnostics = result.diagnostics.clone();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.args == vec![DiagnosticArg::new("issue", issue)]),
        "expected a diagnostic with issue `{issue}`, got: {diagnostics:?}"
    );
}

#[test]
fn matrix_elementwise_methods_typecheck_same_shape() {
    accepts(
        r"
functio matrix_ops(matrix<f32, [2, 2]> a, matrix<f32, [2, 2]> b) → matrix<f32, [2, 2]> {
    sit matrix<f32, [2, 2]> c ← a.addita(b)
    sit matrix<f32, [2, 2]> d ← c.subtracta(b)
    sit matrix<f32, [2, 2]> e ← d.multiplicata(b)
    redde e.divisa(b)
}
",
    );
}

#[test]
fn matrix_elementwise_methods_reject_shape_mismatch() {
    rejects_with_issue(
        r"
functio matrix_ops(matrix<f32, [2, 2]> a, matrix<f32, [2, 3]> b) → matrix<f32, [2, 2]> {
    redde a.addita(b)
}
",
        "argument_type_mismatch",
    );
}

#[test]
fn matrix_elementwise_methods_reject_non_numeric_elements() {
    rejects_with_issue(
        r"
functio matrix_ops(matrix<textus, [2, 2]> a, matrix<textus, [2, 2]> b) → matrix<textus, [2, 2]> {
    redde a.addita(b)
}
",
        "matrix_arithmetic_numeric_element_required",
    );
}

#[test]
fn matrix_matmul_typechecks_same_shape() {
    accepts(
        r"
functio product(matrix<f32, [2, 2]> a, matrix<f32, [2, 2]> b) → matrix<f32, [2, 2]> {
    redde a.matmul(b)
}
",
    );
}

#[test]
fn matrix_applica_typechecks_matching_vector_width() {
    accepts(
        r"
functio apply(matrix<f32, [2, 3]> m, vector<f32, magnitudo 3> v) → vector<f32, magnitudo 2> {
    redde m.applica(v)
}
",
    );
}

#[test]
fn matrix_applica_rejects_vector_width_mismatch() {
    rejects_with_issue(
        r"
functio apply(matrix<f32, [2, 2]> m, vector<f32, magnitudo 3> v) → vector<f32, magnitudo 2> {
    redde m.applica(v)
}
",
        "argument_type_mismatch",
    );
}

#[test]
fn matrix_applica_typechecks_parametric_shape() {
    // S0-G9: dimension-neutral matvec — parametric [R, C] applies a
    // vector of width C and returns a vector of width R.
    accepts(
        r"
functio apply<magnitudo R, magnitudo C>(matrix<f32, [R, C]> m, vector<f32, magnitudo C> v) → vector<f32, magnitudo R> {
    redde m.applica(v)
}
",
    );
}

#[test]
fn matrix_transpone_typechecks_and_swaps_literal_shape() {
    accepts(
        r"
functio transpose(matrix<f32, [2, 3]> m) → matrix<f32, [3, 2]> {
    redde m.transpone()
}
",
    );
}

#[test]
fn matrix_transpone_typechecks_and_swaps_parametric_shape() {
    // S0-G9: dimension-neutral transpose — [R, C] → [C, R] in a generic body.
    accepts(
        r"
functio transpose<magnitudo R, magnitudo C>(matrix<f32, [R, C]> m) → matrix<f32, [C, R]> {
    redde m.transpone()
}
",
    );
}

#[test]
fn matrix_transpone_rejects_arguments() {
    // The registry owns the zero-arity contract: an argument makes the
    // method unresolved rather than a wrong-arity call.
    rejects_with_issue(
        r"
functio transpose(matrix<f32, [2, 2]> m) → matrix<f32, [2, 2]> {
    redde m.transpone(1)
}
",
        "unknown_method",
    );
}
