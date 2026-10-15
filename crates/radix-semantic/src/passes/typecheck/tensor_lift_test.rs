//! Tensor elementwise lifting (gpu-reset rule 15, faber-language-decisions K4):
//! `+ - * / % ÷ ⤒ ⤓`, comparisons, `et`/`aut`/`non`, the `✓ ✗` select and `vel`
//! lift over tensors against a scalar or a same-shape tensor. Element widths
//! follow F9: the scalar rule per element, never a second tensor model.
//!
//! Every rejection is pinned by kind (SEM code), `issue` and args.

use crate::test_support::analyze_source;
use crate::{SemanticError, SemanticErrorKind};

const FAMILY_CROSSING: &str = "numeric_family_crossing_requires_conversio";
const NUMERIC_OPERANDS_REQUIRED: &str = "numeric_operands_required";
const NUMERIC_ELEMENT_REQUIRED: &str = "tensor_arithmetic_numeric_element_required";
const EXACT_EQUALITY: &str = "equality_exact_types_required";
const BOOLEAN_OPERANDS_REQUIRED: &str = "boolean_operands_required";

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

fn operands_invalid(source: &str, issue: &str) {
    rejects(source, SemanticErrorKind::OperandTypesInvalid, issue, &[]);
}

// --- arithmetic against a scalar -------------------------------------------

#[test]
fn plus_and_minus_lift_against_a_scalar_on_either_side() {
    accepts(
        r"functio f(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde a + 1.0 }
functio g(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde 1.0 + a }
functio h(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde a - 1.0 }
functio i(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde 1.0 - a }",
    );
}

#[test]
fn unary_minus_lifts_over_a_tensor() {
    accepts("functio f(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde -a }");
    accepts("functio f(tensor<i8, [3]> a) → tensor<i8, [3]> { redde -a }");
}

#[test]
fn times_divide_and_remainder_lift_against_a_scalar_only() {
    accepts(
        r"functio f(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde a * 0.5 }
functio g(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde 2.0 * a }
functio h(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde a / 2.0 }
functio i(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde a % 2.0 }
functio j(tensor<numerus, [4]> a) → tensor<numerus, [4]> { redde a / 3 }
functio k(tensor<numerus, [4]> a) → tensor<u8, [4]> { redde a % 3 }",
    );
}

/// Tensor multiplication and division point to the rule's alternatives;
/// remainder says it lifts only against a scalar.
#[test]
fn tensor_times_tensor_rejects_with_the_rule_alternatives() {
    operands_invalid(
        "functio f(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> { redde a * b }",
        "tensor_multiply_requires_glyph",
    );
    for (op, issue) in [
        ("/", "elementwise_division_requires_glyph"),
        ("%", "elementwise_remainder_requires_scalar"),
    ] {
        rejects(
            &format!(
                "functio f(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> {{ redde a {op} b }}"
            ),
            SemanticErrorKind::OperandTypesInvalid,
            issue,
            &[("kind", "tensor")],
        );
    }
}

#[test]
fn an_untyped_constant_adopts_the_tensor_element_type() {
    accepts("functio f(tensor<f32, [2]> a) → tensor<f32, [2]> { redde a * 2 }");
    accepts("functio f(tensor<f32, [2]> a) → tensor<f32, [2]> { redde a + 0.25 }");
    // The constant takes part by value and the range rule widens the
    // intermediate: [0,255] + 1 is [1,256] → `u16`.
    accepts("functio f(tensor<u8, [2]> a) → tensor<u16, [2]> { redde a + 1 }");
    // A constant beside an integer tensor joins by value: 300 needs `u16`.
    accepts("functio f(tensor<u8, [2]> a) → tensor<u16, [2]> { redde a + 300 }");
}

#[test]
fn family_crossing_needs_a_conversion() {
    operands_invalid(
        "functio f(tensor<f32, [2]> a, numerus n) → tensor<f32, [2]> { redde a + n }",
        FAMILY_CROSSING,
    );
    operands_invalid(
        "functio f(tensor<numerus, [2]> a, f64 x) → tensor<numerus, [2]> { redde a * x }",
        FAMILY_CROSSING,
    );
}

#[test]
fn integer_elements_take_the_scalar_range_rule_per_operator() {
    // The element takes the scalar range rule per operator: `u8 + u16` is
    // [0,65790] → `u32`, and `i8 - u8` is [-383,127] → `i16`.
    accepts(
        r"functio f(tensor<u8, [2]> a, tensor<u16, [2]> b) → tensor<u32, [2]> { redde a + b }
functio g(tensor<u8, [2]> a, u16 s) → tensor<u32, [2]> { redde a + s }
functio h(tensor<i8, [2]> a, tensor<u8, [2]> b) → tensor<i16, [2]> { redde a - b }",
    );
    // A float pair joins by the lossless lattice like the scalar rule.
    accepts(
        r"functio f(tensor<f32, [2]> a, tensor<f64, [2]> b) → tensor<f64, [2]> { redde a + b }
functio g(tensor<f32, [2]> a, f64 s) → tensor<f64, [2]> { redde a * s }",
    );
}

#[test]
fn tensor_lift_range_rule_intermediate_add_widens_to_u16() {
    accepts("functio f(tensor<u8, [2]> a, tensor<u8, [2]> b) → tensor<u16, [2]> { redde a + b }");
}

#[test]
fn tensor_lift_range_rule_intermediate_sub_widens_to_i16() {
    accepts("functio f(tensor<u8, [2]> a, tensor<u8, [2]> b) → tensor<i16, [2]> { redde a - b }");
}

#[test]
fn tensor_lift_range_rule_intermediate_is_not_narrowed_to_the_operand() {
    // The intermediate is `u16`; the width applies only at the store, so a
    // `u8` return type is a type mismatch.
    rejects(
        "functio f(tensor<u8, [2]> a, tensor<u8, [2]> b) → tensor<u8, [2]> { redde a + b }",
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
        &[],
    );
}

#[test]
fn tensor_lift_range_rule_intermediate_chain_keeps_the_range_rule_type() {
    // `/` is floor division; `range::floor_div` answers [0, a₁] for two
    // non-negative operands, so [0,510] / [2,2] stays [0,510] → `u16`. Floor
    // division does not narrow the range, and the width applies only at the
    // store (the value is still 150).
    accepts(
        "functio f(tensor<u8, [2]> a, tensor<u8, [2]> b) → tensor<u16, [2]> { redde (a + b) / 2 }",
    );
}

#[test]
fn a_narrower_join_result_is_not_assignable_to_the_wider_operand_type() {
    operands_invalid_or_mismatch(
        "functio f(tensor<u8, [2]> a, tensor<u16, [2]> b) → tensor<u8, [2]> { redde a + b }",
    );
}

fn operands_invalid_or_mismatch(source: &str) {
    let result = analyze_source(source);
    assert!(!result.is_ok(), "expected failure:\n{source}");
}

#[test]
fn u64_with_a_signed_element_has_no_lossless_join() {
    operands_invalid(
        "functio f(tensor<u64, [2]> a, tensor<i64, [2]> b) → tensor<i64, [2]> { redde a + b }",
        "u64_signed_arithmetic_requires_conversion",
    );
}

#[test]
fn non_numeric_elements_do_not_lift() {
    operands_invalid(
        "functio f(tensor<textus, [2]> a) → tensor<textus, [2]> { redde a + 1 }",
        NUMERIC_ELEMENT_REQUIRED,
    );
    operands_invalid(
        "functio f(tensor<bivalens, [2]> a) → tensor<bivalens, [2]> { redde -a }",
        "tensor_arithmetic_numeric_element_required",
    );
}

#[test]
fn same_shape_is_required_between_tensors() {
    rejects(
        "functio f(tensor<f32, [2, 2]> a, tensor<f32, [3, 2]> b) → tensor<f32, [2, 2]> { redde a ÷ b }",
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
        &[],
    );
}

// --- true division ----------------------------------------------------------

#[test]
fn true_division_lifts_on_any_shape_with_ruling_23_widths() {
    // Floats keep their width.
    accepts("functio f(tensor<f32, [2]> a) → tensor<f32, [2]> { redde a ÷ 2.0 }");
    accepts("functio f(tensor<f32, [2]> a, tensor<f32, [2]> b) → tensor<f32, [2]> { redde a ÷ b }");
    // 8 and 16 bit integers divide to f32; 32/64 bit to f64.
    accepts(
        r"functio f(tensor<u8, [2]> a, tensor<u8, [2]> b) → tensor<f32, [2]> { redde a ÷ b }
functio g(tensor<i16, [2]> a, tensor<u8, [2]> b) → tensor<f32, [2]> { redde a ÷ b }
functio h(tensor<i32, [2]> a, tensor<i32, [2]> b) → tensor<f64, [2]> { redde a ÷ b }
functio i(tensor<numerus, [2]> a) → tensor<f64, [2]> { redde a ÷ 3 }
functio j(tensor<u8, [2]> a) → tensor<f32, [2]> { redde a ÷ 2 }
functio k(tensor<u8, [2]> a) → tensor<f32, [2]> { redde 100 ÷ a }
functio l(tensor<u8, [2]> a, i16 s) → tensor<f32, [2]> { redde a ÷ s }",
    );
    // The result is never an integer tensor, and f64 does not narrow to f32.
    rejects(
        "functio f(tensor<u8, [2]> a, tensor<u8, [2]> b) → tensor<u8, [2]> { redde a ÷ b }",
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
        &[],
    );
    rejects(
        "functio f(tensor<i32, [2]> a, tensor<i32, [2]> b) → tensor<f32, [2]> { redde a ÷ b }",
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
        &[],
    );
}

// --- extrema ----------------------------------------------------------------

#[test]
fn extrema_lift_tensor_against_tensor() {
    accepts(
        r"functio f(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> { redde a ⤒ b }
functio g(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> { redde a ⤓ b }
functio h(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde a ⤒ 2.5 }
functio i(tensor<u8, [2]> a, tensor<u16, [2]> b) → tensor<u16, [2]> { redde a ⤒ b }",
    );
    rejects(
        "functio f(tensor<f32, [2, 2]> a, tensor<f32, [3, 2]> b) → tensor<f32, [2, 2]> { redde a ⤒ b }",
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
        &[],
    );
}

// --- comparisons -------------------------------------------------------------

#[test]
fn comparisons_produce_a_bivalens_tensor() {
    // Numeric comparisons lift per element; `≡ ≢` compare whole values and are
    // pinned in `aggregate_compare_test`.
    for op in ["≺", "≻", "≤", "≥", "≅", "≇"] {
        accepts(&format!(
            "functio f(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<bivalens, [2, 2]> {{ redde a {op} b }}"
        ));
    }
    for op in ["≺", "≻", "≤", "≥", "≅", "≇"] {
        accepts(&format!(
            "functio f(tensor<f32, [2, 2]> a) → tensor<bivalens, [2, 2]> {{ redde a {op} 0.0 }}"
        ));
        accepts(&format!(
            "functio f(tensor<f32, [2, 2]> a) → tensor<bivalens, [2, 2]> {{ redde 1.0 {op} a }}"
        ));
    }
    accepts("functio f(tensor<u8, [3]> a) → tensor<bivalens, [3]> { redde a ≻ 100 }");
    // The result is a tensor, never a scalar.
    rejects(
        "functio f(tensor<f32, [2]> a, tensor<f32, [2]> b) → bivalens { redde a ≻ b }",
        SemanticErrorKind::TypeMismatch,
        "return_type_mismatch",
        &[],
    );
}

#[test]
fn exact_equality_needs_identical_elements_and_value_equality_does_not() {
    rejects(
        "functio f(tensor<u8, [2]> a, tensor<u16, [2]> b) → bivalens { redde a ≡ b }",
        SemanticErrorKind::TypeMismatch,
        EXACT_EQUALITY,
        &[],
    );
    accepts(
        "functio f(tensor<u8, [2]> a, tensor<u16, [2]> b) → tensor<bivalens, [2]> { redde a ≅ b }",
    );
    accepts(
        "functio f(tensor<i32, [2]> a, tensor<f32, [2]> b) → tensor<bivalens, [2]> { redde a ≺ b }",
    );
}

#[test]
fn approximate_comparison_is_deferred_to_the_numeric_contract() {
    for (op, glyph) in [("≈", "≈"), ("≉", "≉")] {
        rejects(
            &format!(
                "functio f(tensor<f32, [2]> a, tensor<f32, [2]> b) → tensor<bivalens, [2]> {{ redde a {op} b }}"
            ),
            SemanticErrorKind::OperandTypesInvalid,
            "tensor_approx_comparison_deferred",
            &[("op", glyph)],
        );
    }
}

#[test]
fn comparison_shapes_must_match() {
    rejects(
        "functio f(tensor<f32, [2, 2]> a, tensor<f32, [3, 2]> b) → tensor<bivalens, [2, 2]> { redde a ≻ b }",
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
        &[],
    );
}

// --- logic and select --------------------------------------------------------

#[test]
fn logic_lifts_over_bivalens_tensors() {
    accepts(
        r"functio f(tensor<bivalens, [2]> a, tensor<bivalens, [2]> b) → tensor<bivalens, [2]> { redde a et b }
functio g(tensor<bivalens, [2]> a, tensor<bivalens, [2]> b) → tensor<bivalens, [2]> { redde a aut b }
functio h(tensor<bivalens, [2]> a) → tensor<bivalens, [2]> { redde non a }
functio i(tensor<bivalens, [2]> a) → tensor<bivalens, [2]> { redde a et verum }
functio j(tensor<f32, [2]> a, tensor<f32, [2]> b) → tensor<bivalens, [2]> { redde (a ≻ b) aut (a ≺ b) }",
    );
    operands_invalid(
        "functio f(tensor<f32, [2]> a, tensor<f32, [2]> b) → tensor<bivalens, [2]> { redde a et b }",
        BOOLEAN_OPERANDS_REQUIRED,
    );
    operands_invalid(
        "functio f(tensor<f32, [2]> a) → tensor<bivalens, [2]> { redde non a }",
        "boolean_operand_required",
    );
}

#[test]
fn select_lifts_the_value_conditional_over_a_mask() {
    accepts(
        r"functio f(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> { redde (a ≻ b) ✓ a ✗ b }
functio g(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde (a ≻ 0.0) ✓ a ✗ 0.0 }
functio h(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde (a ≻ 0.0) ✓ 1.0 ✗ a }
functio i(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> { redde (a ≻ 0.0) ✓ 1.0 ✗ 0.0 }",
    );
    accepts("functio f(tensor<u8, [3]> a) → tensor<u8, [3]> { redde (a ≻ 100) ✓ a ✗ 0 }");
}

#[test]
fn select_needs_a_bivalens_mask_and_same_shape_branches() {
    rejects(
        "functio f(tensor<f32, [2]> a) → tensor<f32, [2]> { redde a ✓ a ✗ a }",
        SemanticErrorKind::OperandTypesInvalid,
        "condition_must_be_bivalens",
        &[],
    );
    rejects(
        "functio f(tensor<f32, [2, 2]> a, tensor<f32, [3, 2]> b) → tensor<f32, [2, 2]> { redde (a ≻ a) ✓ a ✗ b }",
        SemanticErrorKind::TypeMismatch,
        "incompatible_tensor_index",
        &[],
    );
}

// --- vel ----------------------------------------------------------------------

#[test]
fn vel_lifts_over_nullable_elements() {
    accepts(
        r"functio f(tensor<numerus ∪ nihil, [2]> a, tensor<numerus, [2]> b) → tensor<numerus, [2]> { redde a vel b }
functio g(tensor<numerus ∪ nihil, [2]> a) → tensor<numerus, [2]> { redde a vel 0 }
functio h(tensor<numerus ∪ nihil, [2]> a, tensor<numerus ∪ nihil, [2]> b) → tensor<numerus, [2]> { redde a vel b vel 0 }",
    );
}

#[test]
fn vel_on_a_tensor_with_non_nullable_elements_is_an_error() {
    rejects(
        "functio f(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> { redde a vel b }",
        SemanticErrorKind::TypeMismatch,
        "tensor_coalesce_element_nullable_required",
        &[],
    );
}

#[test]
fn vel_fallback_type_must_match_the_element() {
    rejects(
        "functio f(tensor<numerus ∪ nihil, [2]> a) → tensor<numerus, [2]> { redde a vel \"x\" }",
        SemanticErrorKind::TypeMismatch,
        "coalesce_fallback_type_mismatch",
        &[],
    );
}
