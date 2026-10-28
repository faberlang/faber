//! Operator consistency (2026-10-04): the K4 lifting rule applies to a
//! register `matrix` exactly as to a `vector` or a tensor (rule 15.1, "the
//! same lifting rule applies to `vector` and `matrix`"): `+ - * / % ÷` against
//! a scalar on either side, unary `-`, and `÷`, `⤒`, `⤓` between two matrices
//! of one shape. `matrix * matrix` and `matrix / matrix` point at the
//! elementwise glyphs, `matrix % matrix` says `%` takes a scalar, `≈ ≉` wait
//! for the tensor ruling, and matrix with matrix `+ -` and the glyph products
//! are unchanged. Each cell is typed by the scalar range rule for the operator
//! (operator ruling 2026-10-15, need; width applies only at the
//! store).

use crate::test_support::analyze_source;
use crate::{SemanticError, SemanticErrorKind};

const M: &str = "matrix<f32, [2, 3]>";

fn accepts(source: &str) {
    let result = analyze_source(source);
    assert!(
        result.is_ok(),
        "expected typecheck ok: {:?}\n{source}",
        result.diagnostics
    );
}

fn issue_of(err: &SemanticError) -> Option<&str> {
    err.args
        .iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.as_str())
}

fn rejects(source: &str, kind: SemanticErrorKind, issue: &str) {
    let result = analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected typecheck failure, but source was accepted:\n{source}"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && err.kind == kind && issue_of(err) == Some(issue)),
        "expected {kind:?} issue `{issue}`, got: {:?}\n{source}",
        result.diagnostics
    );
}

fn function(params: &str, ret: &str, body: &str) -> String {
    format!("functio lifted({params}) → {ret} {{\n    redde {body}\n}}")
}

#[test]
fn a_scalar_on_either_side_lifts_every_arithmetic_operator() {
    for op in ["+", "-", "*", "/", "%", "÷"] {
        accepts(&function(&format!("{M} a, f32 s"), M, &format!("a {op} s")));
        accepts(&function(&format!("{M} a, f32 s"), M, &format!("s {op} a")));
    }
}

#[test]
fn an_untyped_literal_adopts_the_matrix_element() {
    accepts(&function(&format!("{M} a"), M, "a * 0.5"));
    accepts(&function(&format!("{M} a"), M, "2.0 * a"));
    accepts(&function(&format!("{M} a"), M, "a + 1"));
    accepts(&function(
        "matrix<i32, [2, 2]> a",
        "matrix<i64, [2, 2]>",
        "a / 2",
    ));
}

#[test]
fn unary_minus_keeps_the_matrix_type() {
    accepts(&function(&format!("{M} a"), M, "-a"));
    accepts(&function(
        "matrix<i32, [2, 2]> a",
        "matrix<i32, [2, 2]>",
        "-a",
    ));
}

#[test]
fn the_shape_is_kept_and_a_wrong_result_shape_is_rejected() {
    rejects(
        &function(&format!("{M} a, f32 s"), "matrix<f32, [3, 2]>", "a * s"),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
    );
}

#[test]
fn a_float_lane_joins_as_scalars_do() {
    accepts(&function(
        &format!("{M} a, f64 s"),
        "matrix<f64, [2, 3]>",
        "a * s",
    ));
    rejects(
        &function(&format!("{M} a, f64 s"), M, "a * s"),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
    );
}

#[test]
fn an_integer_cell_takes_the_scalar_range_rule() {
    // Each cell is typed by the scalar range rule for its operator (operator
    // ruling 2026-10-15, need): an integer intermediate widens and
    // the width applies only at the store. `a * 2` over `i8` is [-256, 254]
    // (`i16`); `10 - a` over `u8` is [-245, 10] (`i16`).
    accepts(&function(
        "matrix<i8, [2, 2]> a",
        "matrix<i16, [2, 2]>",
        "a * 2",
    ));
    accepts(&function(
        "matrix<u8, [2, 2]> a",
        "matrix<i16, [2, 2]>",
        "10 - a",
    ));
    // Ruling 23: integers divide to a float, `i32` to `f64`, `u8` to `f32`.
    accepts(&function(
        "matrix<i32, [2, 2]> a",
        "matrix<f64, [2, 2]>",
        "a ÷ 2",
    ));
    accepts(&function(
        "matrix<u8, [2, 2]> a",
        "matrix<f32, [2, 2]>",
        "a ÷ 2",
    ));
}

#[test]
fn a_family_crossing_needs_an_explicit_conversion() {
    rejects(
        &function(&format!("{M} a, i32 s"), M, "a * s"),
        SemanticErrorKind::OperandTypesInvalid,
        "numeric_family_crossing_requires_conversio",
    );
}

#[test]
fn a_non_numeric_peer_is_rejected() {
    rejects(
        &function(&format!("{M} a, textus s"), M, "a * s"),
        SemanticErrorKind::OperandTypesInvalid,
        "numeric_operands_required",
    );
}

#[test]
fn matrix_times_matrix_points_at_the_glyphs() {
    rejects(
        &function(&format!("{M} a, {M} b"), M, "a * b"),
        SemanticErrorKind::OperandTypesInvalid,
        "matrix_multiply_requires_glyph",
    );
}

/// `a / b` and `a % b` between two matrices point at the elementwise glyphs
/// and the scalar rule, naming the aggregate.
#[test]
fn matrix_by_matrix_division_and_remainder_point_at_the_alternatives() {
    for (op, issue) in [
        ("/", "elementwise_division_requires_glyph"),
        ("%", "elementwise_remainder_requires_scalar"),
    ] {
        let source = function(&format!("{M} a, {M} b"), M, &format!("a {op} b"));
        let result = analyze_source(&source);
        assert!(
            result.diagnostics.iter().any(|err| err.is_error()
                && err.kind == SemanticErrorKind::OperandTypesInvalid
                && issue_of(err) == Some(issue)
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "kind" && arg.value == "matrix")),
            "{issue}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn matrix_with_matrix_plus_minus_and_the_glyph_products_are_unchanged() {
    for op in ["+", "-", "⊙", "⊘"] {
        accepts(&function(&format!("{M} a, {M} b"), M, &format!("a {op} b")));
    }
}

#[test]
fn true_division_and_the_extrema_pair_matrices_of_one_shape_only() {
    accepts(&function(&format!("{M} a, {M} b"), M, "a ÷ b"));
    for op in ["÷", "⤒", "⤓"] {
        rejects(
            &function(
                &format!("{M} a, matrix<f32, [3, 2]> b"),
                M,
                &format!("a {op} b"),
            ),
            SemanticErrorKind::TypeMismatch,
            "incompatible_tensor_index",
        );
    }
}

#[test]
fn the_extrema_lift_over_a_matrix() {
    for op in ["⤒", "⤓"] {
        accepts(&function(&format!("{M} a, f32 s"), M, &format!("a {op} s")));
        accepts(&function(&format!("{M} a"), M, &format!("a {op} 1.0")));
        accepts(&function(&format!("{M} a, {M} b"), M, &format!("a {op} b")));
        rejects(
            &function(
                &format!("{M} a, vector<f32, magnitudo 3> v"),
                M,
                &format!("a {op} v"),
            ),
            SemanticErrorKind::OperandTypesInvalid,
            "argument_type_mismatch",
        );
    }
}

#[test]
fn approximate_equality_waits_for_the_tensor_ruling_over_a_matrix() {
    for op in ["≈", "≉"] {
        for peer in [format!("{M} b"), "f32 b".to_owned()] {
            rejects(
                &function(&format!("{M} a, {peer}"), "bivalens", &format!("a {op} b")),
                SemanticErrorKind::OperandTypesInvalid,
                "tensor_approx_comparison_deferred",
            );
        }
    }
}

#[test]
fn a_matrix_beside_a_tensor_or_vector_keeps_its_own_rule() {
    for peer in ["tensor<f32, [2, 3]> t", "vector<f32, magnitudo 3> t"] {
        let result = analyze_source(&function(&format!("{M} a, {peer}"), M, "a * t"));
        assert!(!result.is_ok(), "{peer}: {:?}", result.diagnostics);
    }
}
