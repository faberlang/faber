//! Regression coverage for matrix intrinsic method typecheck rules.
//!
//! These tests prove that `matrix<T, S>` receivers accept elementwise arithmetic
//! (`addita`, `subtrahe`, `multiplica`, `divida`), matrix multiplication (`matmul`),
//! and vector application (`applica`) with matching shapes, and reject shape
//! mismatches or non-numeric element types with targeted diagnostics.

use crate::diagnostics::DiagnosticArg;
use crate::driver::{analyze_source, Config, Session};

fn accepts(source: &str) {
    let session = Session::new(Config::default());
    let result = analyze_source(&session, "matrix_intrinsic.fab", source);
    assert!(result.is_ok(), "expected typecheck ok: {:?}", result.err());
}

fn rejects_with_issue(source: &str, issue: &str) {
    let session = Session::new(Config::default());
    let result = analyze_source(&session, "matrix_intrinsic.fab", source);
    let Err(diagnostics) = result else {
        panic!("expected typecheck failure, but source was accepted");
    };
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
    fixum matrix<f32, [2, 2]> c ← a.addita(b)
    fixum matrix<f32, [2, 2]> d ← c.subtrahe(b)
    fixum matrix<f32, [2, 2]> e ← d.multiplica(b)
    redde e.divida(b)
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
functio apply(matrix<f32, [2, 3]> m, vector<f32, 3> v) → vector<f32, 2> {
    redde m.applica(v)
}
",
    );
}

#[test]
fn matrix_applica_rejects_vector_width_mismatch() {
    rejects_with_issue(
        r"
functio apply(matrix<f32, [2, 2]> m, vector<f32, 3> v) → vector<f32, 2> {
    redde m.applica(v)
}
",
        "argument_type_mismatch",
    );
}
