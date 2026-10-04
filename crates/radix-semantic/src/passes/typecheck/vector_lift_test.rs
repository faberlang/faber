//! A2-LIFT (gap G02): the K4 arithmetic rule applies to a register `vector`
//! (rule 15.1, "the same lifting rule applies to `vector` and `matrix`"):
//! `+ - * / %` against a scalar on either side, and unary `-`. `vector *
//! vector` is the glyph diagnostic; vector with vector `+ -` is unchanged.
//! `÷`, the matrix register, comparisons and select are not lifted over a
//! vector. Each lane is the scalar operation: a float lane joins as scalars
//! do, and an integer lane takes the scalar range type (`u8 + u8` is `u16`,
//! `-i8` is `i16`), so no lane wraps or clamps inside the expression.

use crate::test_support::analyze_source;
use crate::{SemanticError, SemanticErrorKind};

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
    for op in ["+", "-", "*", "/", "%"] {
        accepts(&function(
            "vector<f32, 4> a, f32 s",
            "vector<f32, 4>",
            &format!("a {op} s"),
        ));
        accepts(&function(
            "vector<f32, 4> a, f32 s",
            "vector<f32, 4>",
            &format!("s {op} a"),
        ));
    }
}

#[test]
fn an_untyped_literal_adopts_the_vector_element() {
    accepts(&function("vector<f32, 3> a", "vector<f32, 3>", "a * 0.5"));
    accepts(&function("vector<f32, 3> a", "vector<f32, 3>", "2.0 * a"));
    accepts(&function("vector<f32, 3> a", "vector<f32, 3>", "a + 1"));
    accepts(&function("vector<i32, 3> a", "vector<i32, 3>", "a / 2"));
}

#[test]
fn unary_minus_keeps_the_vector_type() {
    accepts(&function("vector<f32, 4> a", "vector<f32, 4>", "-a"));
    accepts(&function("vector<i32, 2> a", "vector<i32, 2>", "-a"));
}

#[test]
fn the_width_is_kept_and_a_wrong_result_width_is_rejected() {
    rejects(
        &function("vector<f32, 4> a, f32 s", "vector<f32, 3>", "a * s"),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
    );
}

#[test]
fn a_float_lane_joins_as_scalars_do() {
    // A wider float scalar joins to the wider float vector.
    accepts(&function(
        "vector<f32, 3> a, f64 s",
        "vector<f64, 3>",
        "a * s",
    ));
    // The result is not narrowed back to the vector's own element.
    rejects(
        &function("vector<f32, 3> a, f64 s", "vector<f32, 3>", "a * s"),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
    );
}

#[test]
fn an_integer_lane_keeps_its_element_and_is_checked_where_it_is_stored() {
    // The element is the F9 join (a constant takes part by value), so an
    // integer vector stays its own type; the lane traps at the operation
    // when it leaves the element's range (the K4 tensor behaviour).
    accepts(&function("vector<i8, 3> a", "vector<i8, 3>", "a * 2"));
    accepts(&function("vector<i32, 3> a", "vector<i32, 3>", "a / 2"));
    accepts(&function("vector<i32, 3> a", "vector<i32, 3>", "a % 4"));
    accepts(&function("vector<u8, 3> a", "vector<u8, 3>", "10 - a"));
    accepts(&function("vector<i32, 3> a", "vector<i32, 3>", "-a"));
}

#[test]
fn an_integer_lane_is_never_narrowed_by_a_wider_scalar_implicitly() {
    // A wider typed integer scalar joins to the wider integer vector.
    accepts(&function(
        "vector<i8, 3> a, i32 s",
        "vector<i32, 3>",
        "a + s",
    ));
    rejects(
        &function("vector<i8, 3> a, i32 s", "vector<i8, 3>", "a + s"),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
    );
}

#[test]
fn a_family_crossing_needs_an_explicit_conversion() {
    rejects(
        &function("vector<f32, 3> a, i32 s", "vector<f32, 3>", "a * s"),
        SemanticErrorKind::OperandTypesInvalid,
        "numeric_family_crossing_requires_conversio",
    );
}

#[test]
fn a_non_numeric_peer_is_rejected() {
    rejects(
        &function("vector<f32, 3> a, textus s", "vector<f32, 3>", "a * s"),
        SemanticErrorKind::OperandTypesInvalid,
        "numeric_operands_required",
    );
}

#[test]
fn vector_times_vector_points_at_the_glyphs() {
    rejects(
        &function(
            "vector<f32, 3> a, vector<f32, 3> b",
            "vector<f32, 3>",
            "a * b",
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "vector_multiply_requires_glyph",
    );
}

#[test]
fn vector_with_vector_plus_minus_and_the_glyph_products_are_unchanged() {
    for op in ["+", "-", "⊙", "⊘"] {
        accepts(&function(
            "vector<f32, 3> a, vector<f32, 3> b",
            "vector<f32, 3>",
            &format!("a {op} b"),
        ));
    }
    accepts(&function(
        "vector<f32, 3> a, vector<f32, 3> b",
        "f32",
        "a · b",
    ));
}

#[test]
fn vector_by_vector_division_and_remainder_stay_rejected() {
    for op in ["/", "%"] {
        rejects(
            &function(
                "vector<f32, 3> a, vector<f32, 3> b",
                "vector<f32, 3>",
                &format!("a {op} b"),
            ),
            SemanticErrorKind::OperandTypesInvalid,
            "numeric_operands_required",
        );
    }
}

#[test]
fn true_division_is_not_lifted_over_a_vector() {
    rejects(
        &function("vector<f32, 3> a, f32 s", "vector<f32, 3>", "a ÷ s"),
        SemanticErrorKind::OperandTypesInvalid,
        "numeric_operands_required",
    );
}

#[test]
fn a_vector_beside_a_tensor_keeps_its_own_rule() {
    // The vector hook never claims a tensor peer.
    let result = analyze_source(&function(
        "vector<f32, 3> a, tensor<f32, [3]> t",
        "vector<f32, 3>",
        "a * t",
    ));
    assert!(!result.is_ok(), "{:?}", result.diagnostics);
}
