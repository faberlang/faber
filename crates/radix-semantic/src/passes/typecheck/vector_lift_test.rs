//! A2-LIFT (gap G02): the K4 arithmetic rule applies to a register `vector`
//! (rule 15.1, "the same lifting rule applies to `vector` and `matrix`"):
//! `+ - * / % ÷` against a scalar on either side, and unary `-`; `÷` and `⤒ ⤓`
//! also pair two vectors of one width. `vector * vector` and `vector / vector`
//! are the glyph diagnostics, `vector % vector` says `%` takes a scalar;
//! vector with vector `+ -` is unchanged. Comparisons and select are not
//! lifted over a vector. Each lane is typed by the scalar range rule for the
//! operator (operator ruling 2026-10-15, need), so an integer
//! intermediate widens and the width applies only at the store.

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
            "vector<f32, magnitudo 4> a, f32 s",
            "vector<f32, magnitudo 4>",
            &format!("a {op} s"),
        ));
        accepts(&function(
            "vector<f32, magnitudo 4> a, f32 s",
            "vector<f32, magnitudo 4>",
            &format!("s {op} a"),
        ));
    }
}

#[test]
fn an_untyped_literal_adopts_the_vector_element() {
    accepts(&function(
        "vector<f32, magnitudo 3> a",
        "vector<f32, magnitudo 3>",
        "a * 0.5",
    ));
    accepts(&function(
        "vector<f32, magnitudo 3> a",
        "vector<f32, magnitudo 3>",
        "2.0 * a",
    ));
    accepts(&function(
        "vector<f32, magnitudo 3> a",
        "vector<f32, magnitudo 3>",
        "a + 1",
    ));
    accepts(&function(
        "vector<i32, magnitudo 3> a",
        "vector<i64, magnitudo 3>",
        "a / 2",
    ));
}

#[test]
fn unary_minus_keeps_the_vector_type() {
    accepts(&function(
        "vector<f32, magnitudo 4> a",
        "vector<f32, magnitudo 4>",
        "-a",
    ));
    accepts(&function(
        "vector<i32, magnitudo 2> a",
        "vector<i32, magnitudo 2>",
        "-a",
    ));
}

#[test]
fn the_width_is_kept_and_a_wrong_result_width_is_rejected() {
    rejects(
        &function(
            "vector<f32, magnitudo 4> a, f32 s",
            "vector<f32, magnitudo 3>",
            "a * s",
        ),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
    );
}

#[test]
fn a_float_lane_joins_as_scalars_do() {
    // A wider float scalar joins to the wider float vector.
    accepts(&function(
        "vector<f32, magnitudo 3> a, f64 s",
        "vector<f64, magnitudo 3>",
        "a * s",
    ));
    // The result is not narrowed back to the vector's own element.
    rejects(
        &function(
            "vector<f32, magnitudo 3> a, f64 s",
            "vector<f32, magnitudo 3>",
            "a * s",
        ),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
    );
}

#[test]
fn an_integer_lane_takes_the_scalar_range_rule() {
    // Each lane is typed by the scalar range rule for its operator (operator
    // ruling 2026-10-15, need): an integer intermediate widens and
    // the width applies only at the store. `a * 2` over `i8` is [-256, 254]
    // (`i16`); `a % 4` over `i32` is [0, 3] (`u8`); `10 - a` over `u8` is
    // [-245, 10] (`i16`).
    accepts(&function(
        "vector<i8, magnitudo 3> a",
        "vector<i16, magnitudo 3>",
        "a * 2",
    ));
    accepts(&function(
        "vector<i32, magnitudo 3> a",
        "vector<i64, magnitudo 3>",
        "a / 2",
    ));
    accepts(&function(
        "vector<i32, magnitudo 3> a",
        "vector<u8, magnitudo 3>",
        "a % 4",
    ));
    accepts(&function(
        "vector<u8, magnitudo 3> a",
        "vector<i16, magnitudo 3>",
        "10 - a",
    ));
    // Unary `-` is not a binary pair, so the lane keeps its own element.
    accepts(&function(
        "vector<i32, magnitudo 3> a",
        "vector<i32, magnitudo 3>",
        "-a",
    ));
}

#[test]
fn an_integer_lane_narrows_only_at_the_return_store() {
    // The scalar range rule widens the intermediate: `i8` + `i32` spans
    // [-2^31-128, 2^31+126], so the lane is `i64`, not the `i32` scalar — the
    // lane is still never narrowed mid-expression (first assertion). The
    // vector<i8, size 3> return slot applies the checked element narrowing at the
    // store (ruling Q-X3, 2026-10-15, need).
    accepts(&function(
        "vector<i8, magnitudo 3> a, i32 s",
        "vector<i64, magnitudo 3>",
        "a + s",
    ));
    accepts(&function(
        "vector<i8, magnitudo 3> a, i32 s",
        "vector<i8, magnitudo 3>",
        "a + s",
    ));
}

#[test]
fn a_family_crossing_needs_an_explicit_conversion() {
    rejects(
        &function(
            "vector<f32, magnitudo 3> a, i32 s",
            "vector<f32, magnitudo 3>",
            "a * s",
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "numeric_family_crossing_requires_conversio",
    );
}

#[test]
fn a_non_numeric_peer_is_rejected() {
    rejects(
        &function(
            "vector<f32, magnitudo 3> a, textus s",
            "vector<f32, magnitudo 3>",
            "a * s",
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "numeric_operands_required",
    );
}

#[test]
fn vector_times_vector_points_at_the_glyphs() {
    rejects(
        &function(
            "vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b",
            "vector<f32, magnitudo 3>",
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
            "vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b",
            "vector<f32, magnitudo 3>",
            &format!("a {op} b"),
        ));
    }
    accepts(&function(
        "vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b",
        "f32",
        "a · b",
    ));
}

/// `a / b` and `a % b` between two vectors point at the elementwise glyphs
/// and the scalar rule, naming the aggregate.
#[test]
fn vector_by_vector_division_and_remainder_point_at_the_alternatives() {
    for (op, issue) in [
        ("/", "elementwise_division_requires_glyph"),
        ("%", "elementwise_remainder_requires_scalar"),
    ] {
        let source = function(
            "vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b",
            "vector<f32, magnitudo 3>",
            &format!("a {op} b"),
        );
        let result = analyze_source(&source);
        assert!(
            result.diagnostics.iter().any(|err| err.is_error()
                && err.kind == SemanticErrorKind::OperandTypesInvalid
                && issue_of(err) == Some(issue)
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "kind" && arg.value == "vector")),
            "{issue}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn true_division_lifts_over_a_vector() {
    accepts(&function(
        "vector<f32, magnitudo 3> a, f32 s",
        "vector<f32, magnitudo 3>",
        "a ÷ s",
    ));
    accepts(&function(
        "vector<f32, magnitudo 3> a, f32 s",
        "vector<f32, magnitudo 3>",
        "s ÷ a",
    ));
    accepts(&function(
        "vector<f32, magnitudo 3> a",
        "vector<f32, magnitudo 3>",
        "a ÷ 2",
    ));
    accepts(&function(
        "vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b",
        "vector<f32, magnitudo 3>",
        "a ÷ b",
    ));
    // Ruling 23: integers divide to a float, `i32` to `f64`, `u8` to `f32`.
    accepts(&function(
        "vector<i32, magnitudo 3> a",
        "vector<f64, magnitudo 3>",
        "a ÷ 2",
    ));
    accepts(&function(
        "vector<u8, magnitudo 3> a",
        "vector<f32, magnitudo 3>",
        "a ÷ 2",
    ));
    rejects(
        &function(
            "vector<i32, magnitudo 3> a",
            "vector<i32, magnitudo 3>",
            "a ÷ 2",
        ),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
    );
}

#[test]
fn true_division_pairs_vectors_of_one_width_only() {
    rejects(
        &function(
            "vector<f32, magnitudo 3> a, vector<f32, magnitudo 4> b",
            "vector<f32, magnitudo 3>",
            "a ÷ b",
        ),
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
    );
}

#[test]
fn the_extrema_lift_over_a_vector() {
    for op in ["⤒", "⤓"] {
        accepts(&function(
            "vector<f32, magnitudo 3> a, f32 s",
            "vector<f32, magnitudo 3>",
            &format!("a {op} s"),
        ));
        accepts(&function(
            "vector<f32, magnitudo 3> a",
            "vector<f32, magnitudo 3>",
            &format!("a {op} 1.0"),
        ));
        accepts(&function(
            "vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b",
            "vector<f32, magnitudo 3>",
            &format!("a {op} b"),
        ));
        rejects(
            &function(
                "vector<f32, magnitudo 3> a, vector<f32, magnitudo 4> b",
                "vector<f32, magnitudo 3>",
                &format!("a {op} b"),
            ),
            SemanticErrorKind::TypeMismatch,
            "incompatible_tensor_index",
        );
        rejects(
            &function(
                "vector<f32, magnitudo 3> a, tensor<f32, [3]> t",
                "vector<f32, magnitudo 3>",
                &format!("a {op} t"),
            ),
            SemanticErrorKind::OperandTypesInvalid,
            "argument_type_mismatch",
        );
    }
}

#[test]
fn approximate_equality_waits_for_the_tensor_ruling_over_a_vector() {
    for op in ["≈", "≉"] {
        for peer in ["vector<f32, magnitudo 3> b", "f32 b"] {
            rejects(
                &function(
                    &format!("vector<f32, magnitudo 3> a, {peer}"),
                    "bivalens",
                    &format!("a {op} b"),
                ),
                SemanticErrorKind::OperandTypesInvalid,
                "tensor_approx_comparison_deferred",
            );
        }
    }
}

#[test]
fn a_vector_beside_a_tensor_keeps_its_own_rule() {
    // The vector hook never claims a tensor peer.
    let result = analyze_source(&function(
        "vector<f32, magnitudo 3> a, tensor<f32, [3]> t",
        "vector<f32, magnitudo 3>",
        "a * t",
    ));
    assert!(!result.is_ok(), "{:?}", result.diagnostics);
}
