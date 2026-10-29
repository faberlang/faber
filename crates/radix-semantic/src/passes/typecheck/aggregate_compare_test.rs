//! Comparisons, logic and masks over aggregates (advisor rulings 2026-10-04,
//! gpu-reset rule 15): `≡`/`≢` compare WHOLE values on every type, the numeric
//! comparison glyphs `< > ≤ ≥ ≅ ≇` lift per element over a tensor, vector or
//! matrix (against a scalar on either side or a same-shape peer), `et`/`aut`/
//! `non` and the `✓ ✗` select lift over a bool aggregate, and a bool aggregate
//! is never a truth value.
//!
//! Every rejection is pinned by kind (SEM code), `issue` and args.

use crate::test_support::analyze_source;
use crate::{SemanticError, SemanticErrorKind};

const T: &str = "tensor<f32, [2, 2]>";
const V: &str = "vector<f32, magnitudo 3>";
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

/// Reject with an error of `kind` carrying `issue` and every `(name, value)` in `args`.
fn rejects(source: &str, kind: SemanticErrorKind, issue: &str, args: &[(&str, &str)]) {
    let result = analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected typecheck failure, but source was accepted:\n{source}"
    );
    let found = result.diagnostics.iter().any(|err| {
        err.is_error()
            && err.kind == kind
            && issue_of(err) == Some(issue)
            && args.iter().all(|(name, value)| {
                err.args
                    .iter()
                    .any(|arg| arg.name == *name && arg.value == *value)
            })
    });
    assert!(
        found,
        "expected {kind:?} issue `{issue}` args {args:?}, got: {:?}\n{source}",
        result.diagnostics
    );
}

fn function(params: &str, ret: &str, body: &str) -> String {
    format!("functio lifted({params}) → {ret} {{\n    redde {body}\n}}")
}

// --- `≡ ≢` compare whole values ------------------------------------------------

#[test]
fn tensor_equality_is_one_bivalens() {
    for op in ["≡", "≢"] {
        accepts(&function(
            &format!("{T} a, {T} b"),
            "bivalens",
            &format!("a {op} b"),
        ));
    }
    // The result is never a tensor.
    rejects(
        &function(
            &format!("{T} a, {T} b"),
            "tensor<bivalens, [2, 2]>",
            "a ≡ b",
        ),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
        &[],
    );
    // It is a truth value, so it may stand as a condition.
    accepts(&format!(
        "functio lifted({T} a, {T} b) → bivalens {{\n    si a ≡ b {{ redde verum }}\n    redde falsum\n}}"
    ));
}

#[test]
fn tensor_equality_needs_identical_element_types() {
    rejects(
        &function("tensor<u8, [2]> a, tensor<u16, [2]> b", "bivalens", "a ≡ b"),
        SemanticErrorKind::TypeMismatch,
        "equality_exact_types_required",
        &[],
    );
}

#[test]
fn a_scalar_is_never_equal_to_an_aggregate() {
    for (ty, kind) in [(T, "tensor"), (V, "vector"), (M, "matrix")] {
        for op in ["≡", "≢"] {
            rejects(
                &function(&format!("{ty} a"), "bivalens", &format!("a {op} 2.0")),
                SemanticErrorKind::OperandTypesInvalid,
                "aggregate_equality_scalar_operand",
                &[("kind", kind)],
            );
            rejects(
                &function(&format!("{ty} a"), "bivalens", &format!("2.0 {op} a")),
                SemanticErrorKind::OperandTypesInvalid,
                "aggregate_equality_scalar_operand",
                &[("kind", kind)],
            );
        }
    }
    rejects(
        &function("tensor<i32, [2]> a", "bivalens", "a ≡ 2"),
        SemanticErrorKind::OperandTypesInvalid,
        "aggregate_equality_scalar_operand",
        &[("kind", "tensor")],
    );
    rejects(
        &function("tensor<bivalens, [2]> a", "bivalens", "a ≡ verum"),
        SemanticErrorKind::OperandTypesInvalid,
        "aggregate_equality_scalar_operand",
        &[("kind", "tensor")],
    );
}

#[test]
fn a_nihil_test_on_an_aggregate_keeps_its_own_rule() {
    accepts(&function(&format!("{T} a"), "bivalens", "a ≡ nulla"));
}

#[test]
fn a_shape_that_is_part_of_the_type_must_agree() {
    rejects(
        &function(
            "tensor<f32, [3]> a, tensor<f32, [2]> b",
            "bivalens",
            "a ≡ b",
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "aggregate_equality_shape_mismatch",
        &[("kind", "tensor")],
    );
    // A different rank is a different type as well.
    rejects(
        &function(
            "tensor<f32, [2, 2]> a, tensor<f32, [4]> b",
            "bivalens",
            "a ≢ b",
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "aggregate_equality_shape_mismatch",
        &[("kind", "tensor")],
    );
    rejects(
        &function(
            "vector<f32, magnitudo 3> a, vector<f32, magnitudo 4> b",
            "bivalens",
            "a ≡ b",
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "aggregate_equality_shape_mismatch",
        &[("kind", "vector")],
    );
    rejects(
        &function(
            "matrix<f32, [2, 3]> a, matrix<f32, [3, 2]> b",
            "bivalens",
            "a ≡ b",
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "aggregate_equality_shape_mismatch",
        &[("kind", "matrix")],
    );
}

#[test]
fn the_shape_mismatch_names_both_types() {
    let result = analyze_source(&function(
        "tensor<f32, [3]> a, tensor<f32, [2]> b",
        "bivalens",
        "a ≡ b",
    ));
    let error = result
        .diagnostics
        .iter()
        .find(|err| issue_of(err) == Some("aggregate_equality_shape_mismatch"))
        .expect("shape mismatch diagnostic");
    let arg = |name: &str| {
        error
            .args
            .iter()
            .find(|arg| arg.name == name)
            .map(|arg| arg.value.clone())
            .expect("named arg")
    };
    assert!(arg("left").contains('3'), "left names its shape: {error:?}");
    assert!(
        arg("right").contains('2'),
        "right names its shape: {error:?}"
    );
}

#[test]
fn a_shape_known_only_at_run_time_is_a_value_question() {
    // Distinct size parameters, a size parameter against a literal and a
    // function-parameter axis are not statically different.
    accepts(
        r"functio lifted<magnitudo N, magnitudo M>(tensor<f32, [N]> a, tensor<f32, [M]> b) → bivalens {
    redde a ≡ b
}",
    );
    accepts(
        r"functio lifted<magnitudo N>(tensor<f32, [N]> a, tensor<f32, [3]> b) → bivalens {
    redde a ≢ b
}",
    );
}

// --- numeric comparisons lift over a vector or matrix ---------------------------

#[test]
fn a_register_comparison_gives_a_bool_register_of_the_same_shape() {
    for op in ["≺", "≻", "≤", "≥", "≅", "≇"] {
        accepts(&function(
            &format!("{V} a, {V} b"),
            "vector<bivalens, magnitudo 3>",
            &format!("a {op} b"),
        ));
        accepts(&function(
            &format!("{V} a"),
            "vector<bivalens, magnitudo 3>",
            &format!("a {op} 1.0"),
        ));
        accepts(&function(
            &format!("{V} a"),
            "vector<bivalens, magnitudo 3>",
            &format!("1.0 {op} a"),
        ));
        accepts(&function(
            &format!("{M} a, {M} b"),
            "matrix<bivalens, [2, 3]>",
            &format!("a {op} b"),
        ));
        accepts(&function(
            &format!("{M} a"),
            "matrix<bivalens, [2, 3]>",
            &format!("a {op} 1.0"),
        ));
        accepts(&function(
            &format!("{M} a"),
            "matrix<bivalens, [2, 3]>",
            &format!("1.0 {op} a"),
        ));
    }
}

#[test]
fn a_register_comparison_is_never_a_scalar() {
    rejects(
        &function(&format!("{V} a, {V} b"), "bivalens", "a ≺ b"),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
        &[],
    );
    rejects(
        &function(&format!("{M} a, {M} b"), "bivalens", "a ≅ b"),
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
        &[],
    );
}

#[test]
fn register_comparisons_join_elements_by_value_as_scalars_do() {
    accepts(&function(
        "vector<i32, magnitudo 3> a, vector<f32, magnitudo 3> b",
        "vector<bivalens, magnitudo 3>",
        "a ≺ b",
    ));
    accepts(&function(
        "vector<u8, magnitudo 3> a",
        "vector<bivalens, magnitudo 3>",
        "a ≻ 100",
    ));
    accepts(&function(
        "matrix<u8, [2, 2]> a, matrix<u16, [2, 2]> b",
        "matrix<bivalens, [2, 2]>",
        "a ≅ b",
    ));
}

#[test]
fn register_comparisons_need_the_same_shape_and_numeric_operands() {
    rejects(
        &function(
            "vector<f32, magnitudo 3> a, vector<f32, magnitudo 4> b",
            "vector<bivalens, magnitudo 3>",
            "a ≺ b",
        ),
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
        &[],
    );
    rejects(
        &function(
            "matrix<f32, [2, 3]> a, matrix<f32, [3, 2]> b",
            "matrix<bivalens, [2, 3]>",
            "a ≤ b",
        ),
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
        &[],
    );
    rejects(
        &function(
            &format!("{V} a, bivalens s"),
            "vector<bivalens, magnitudo 3>",
            "a ≺ s",
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "numeric_operands_required",
        &[],
    );
}

#[test]
fn approximate_comparison_over_a_register_stays_deferred() {
    for op in ["≈", "≉"] {
        rejects(
            &function(&format!("{V} a, {V} b"), "bivalens", &format!("a {op} b")),
            SemanticErrorKind::OperandTypesInvalid,
            "tensor_approx_comparison_deferred",
            &[("op", op)],
        );
        rejects(
            &function(&format!("{M} a"), "bivalens", &format!("a {op} 1.0")),
            SemanticErrorKind::OperandTypesInvalid,
            "tensor_approx_comparison_deferred",
            &[("op", op)],
        );
    }
}

// --- logic and select over a bool register ---------------------------------------

#[test]
fn logic_lifts_over_bool_registers() {
    accepts(
        r"functio f(vector<bivalens, magnitudo 3> a, vector<bivalens, magnitudo 3> b) → vector<bivalens, magnitudo 3> { redde a et b }
functio g(vector<bivalens, magnitudo 3> a, vector<bivalens, magnitudo 3> b) → vector<bivalens, magnitudo 3> { redde a aut b }
functio h(vector<bivalens, magnitudo 3> a) → vector<bivalens, magnitudo 3> { redde non a }
functio i(vector<bivalens, magnitudo 3> a) → vector<bivalens, magnitudo 3> { redde a et verum }
functio j(vector<bivalens, magnitudo 3> a) → vector<bivalens, magnitudo 3> { redde falsum aut a }
functio k(matrix<bivalens, [2, 2]> a, matrix<bivalens, [2, 2]> b) → matrix<bivalens, [2, 2]> { redde a et b }
functio l(matrix<bivalens, [2, 2]> a) → matrix<bivalens, [2, 2]> { redde non a }
functio m(vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b) → vector<bivalens, magnitudo 3> { redde (a ≻ b) aut (a ≺ b) }",
    );
}

#[test]
fn logic_over_registers_needs_bool_elements_and_one_shape() {
    rejects(
        &function(
            &format!("{V} a, {V} b"),
            "vector<bivalens, magnitudo 3>",
            "a et b",
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "boolean_operands_required",
        &[],
    );
    rejects(
        &function(&format!("{V} a"), "vector<bivalens, magnitudo 3>", "non a"),
        SemanticErrorKind::OperandTypesInvalid,
        "boolean_operand_required",
        &[],
    );
    rejects(
        &function(
            "vector<bivalens, magnitudo 3> a, vector<bivalens, magnitudo 4> b",
            "vector<bivalens, magnitudo 3>",
            "a et b",
        ),
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
        &[],
    );
}

#[test]
fn select_lifts_over_a_bool_register_mask() {
    accepts(
        r"functio f(vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b) → vector<f32, magnitudo 3> { redde (a ≻ b) ✓ a ✗ b }
functio g(vector<f32, magnitudo 3> a) → vector<f32, magnitudo 3> { redde (a ≻ 0.0) ✓ a ✗ 0.0 }
functio h(vector<f32, magnitudo 3> a) → vector<f32, magnitudo 3> { redde (a ≻ 0.0) ✓ 1.0 ✗ 0.0 }
functio i(matrix<f32, [2, 2]> a, matrix<f32, [2, 2]> b) → matrix<f32, [2, 2]> { redde (a ≻ b) ✓ a ✗ b }
functio j(matrix<f32, [2, 2]> a) → matrix<f32, [2, 2]> { redde (a ≻ 0.0) ✓ a ✗ 0.0 }",
    );
    // A branch of another kind or shape is rejected.
    rejects(
        "functio f(vector<f32, magnitudo 3> a, vector<f32, magnitudo 4> b) → vector<f32, magnitudo 3> { redde (a ≻ a) ✓ a ✗ b }",
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
        &[],
    );
    rejects(
        "functio f(vector<f32, magnitudo 3> a, tensor<f32, [3]> b) → vector<f32, magnitudo 3> { redde (a ≻ a) ✓ a ✗ b }",
        SemanticErrorKind::OperandTypesInvalid,
        "common_type_mismatch",
        &[],
    );
}

// --- a bool aggregate is never a truth value ---------------------------------------

#[test]
fn a_bool_aggregate_is_not_a_truth_value() {
    for (ty, kind) in [
        ("tensor<bivalens, [2]>", "tensor"),
        ("vector<bivalens, magnitudo 3>", "vector"),
        ("matrix<bivalens, [2, 2]>", "matrix"),
    ] {
        rejects(
            &format!("functio lifted({ty} m) {{\n    si m {{ nota \"yes\" }}\n}}"),
            SemanticErrorKind::OperandTypesInvalid,
            "aggregate_mask_not_truth_value",
            &[("kind", kind)],
        );
    }
    // A non-bool aggregate keeps the plain condition diagnostic.
    rejects(
        "functio lifted(tensor<f32, [2]> m) {\n    si m { nota \"yes\" }\n}",
        SemanticErrorKind::OperandTypesInvalid,
        "condition_must_be_bivalens",
        &[],
    );
}

// --- lists join the aggregate diagnostics (Q-E, 2026-10-04 comparisons ruling) --------

#[test]
fn list_equality_against_a_scalar_names_the_per_element_glyph() {
    for ty in ["lista<f32>", "lista<f32, magnitudo 4>", "lista<numerus>"] {
        for op in ["≡", "≢"] {
            rejects(
                &function(&format!("{ty} a"), "bivalens", &format!("a {op} 2")),
                SemanticErrorKind::OperandTypesInvalid,
                "aggregate_equality_scalar_operand",
                &[("kind", "list")],
            );
        }
    }
    // Two lists keep the whole-value rule.
    accepts(&function(
        "lista<numerus> a, lista<numerus> b",
        "bivalens",
        "a ≡ b",
    ));
}

#[test]
fn a_bool_list_is_not_a_truth_value() {
    for ty in ["lista<bivalens>", "lista<bivalens, magnitudo 3>"] {
        rejects(
            &format!("functio lifted({ty} m) {{\n    si m {{ nota \"yes\" }}\n}}"),
            SemanticErrorKind::OperandTypesInvalid,
            "aggregate_mask_not_truth_value",
            &[("kind", "list")],
        );
    }
}

#[test]
fn hadamard_glyphs_with_a_scalar_operand_name_the_scalar_forms() {
    for (ty, kind) in [
        (T, "tensor"),
        (V, "vector"),
        (M, "matrix"),
        ("lista<f32>", "list"),
        ("lista<f32, magnitudo 3>", "list"),
    ] {
        for op in ["⊙", "⊘"] {
            rejects(
                &function(&format!("{ty} a"), ty, &format!("a {op} 2.0")),
                SemanticErrorKind::OperandTypesInvalid,
                "glyph_scalar_operand",
                &[("kind", kind), ("op", op)],
            );
            rejects(
                &function(&format!("{ty} a, f32 s"), ty, &format!("s {op} a")),
                SemanticErrorKind::OperandTypesInvalid,
                "glyph_scalar_operand",
                &[("kind", kind), ("op", op)],
            );
        }
    }
}

#[test]
fn approximate_comparison_over_lists_fails_closed_at_check_time() {
    for op in ["≈", "≉"] {
        for body in [
            format!("a {op} b"),
            format!("a {op} 1.0"),
            format!("1.0 {op} a"),
        ] {
            rejects(
                &function("lista<f32> a, lista<f32> b", "bivalens", &body),
                SemanticErrorKind::OperandTypesInvalid,
                "tensor_approx_comparison_deferred",
                &[("op", op), ("kind", "list")],
            );
        }
        rejects(
            &function(
                "lista<f32, magnitudo 4> a, lista<f32, magnitudo 4> b",
                "bivalens",
                &format!("a {op} b"),
            ),
            SemanticErrorKind::OperandTypesInvalid,
            "tensor_approx_comparison_deferred",
            &[("op", op), ("kind", "list")],
        );
    }
}

#[test]
fn approximate_comparison_deferral_names_its_aggregate_kind() {
    rejects(
        &function(&format!("{V} a, {V} b"), "bivalens", "a ≈ b"),
        SemanticErrorKind::OperandTypesInvalid,
        "tensor_approx_comparison_deferred",
        &[("op", "≈"), ("kind", "vector")],
    );
    rejects(
        &function(&format!("{M} a"), "bivalens", "a ≉ 1.0"),
        SemanticErrorKind::OperandTypesInvalid,
        "tensor_approx_comparison_deferred",
        &[("op", "≉"), ("kind", "matrix")],
    );
    rejects(
        &function(&format!("{T} a, {T} b"), "bivalens", "a ≈ b"),
        SemanticErrorKind::OperandTypesInvalid,
        "tensor_approx_comparison_deferred",
        &[("op", "≈"), ("kind", "tensor")],
    );
}
