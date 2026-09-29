//! `÷` true division (d11-6 ruling 15) and cross-family comparison
//! (ruling 22) typecheck probes.

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

const FAMILY_CROSSING: &str = "numeric_family_crossing_requires_conversio";
const EXACT_EQ: &str = "equality_exact_types_required";

#[test]
fn constant_true_division_is_f64() {
    assert_accepts("functio p() → fractus<f64> { redde 7 ÷ 2 }");
    assert_rejects_with_issue(
        "functio p() → numerus { redde 7 ÷ 2 }",
        "return_type_mismatch",
    );
}

#[test]
fn integer_true_division_is_f64_at_every_width() {
    assert_accepts("functio p(numerus<u8> a, numerus<u8> b) → fractus<f64> { redde a ÷ b }");
    assert_accepts("functio p(numerus<i32> a, numerus<u16> b) → fractus<f64> { redde a ÷ b }");
    assert_accepts("functio p(numerus a) → fractus<f64> { redde a ÷ 3 }");
    assert_rejects_with_issue(
        "functio p(numerus<u8> a, numerus<u8> b) → numerus<u8> { redde a ÷ b }",
        "return_type_mismatch",
    );
}

#[test]
fn float_true_division_keeps_the_operand_width() {
    assert_accepts("functio p(fractus<f32> a, fractus<f32> b) → fractus<f32> { redde a ÷ b }");
    assert_rejects_with_issue(
        "functio p(fractus<f64> a, fractus<f64> b) → fractus<f32> { redde a ÷ b }",
        "return_type_mismatch",
    );
}

#[test]
fn float_beside_integer_constant_is_accepted() {
    assert_accepts("functio p(fractus<f64> a) → fractus<f64> { redde a ÷ 2 }");
    assert_accepts("functio p(fractus<f64> a) → fractus<f64> { redde 2 ÷ a }");
}

#[test]
fn integer_beside_float_variable_is_rejected() {
    assert_rejects_with_issue(
        "functio p(numerus a, fractus<f64> b) → fractus<f64> { redde a ÷ b }",
        FAMILY_CROSSING,
    );
    assert_rejects_with_issue(
        "functio p(fractus<f64> a, numerus<u8> b) → fractus<f64> { redde a ÷ b }",
        FAMILY_CROSSING,
    );
}

#[test]
fn decimal_true_division_follows_slash() {
    assert_accepts("functio p(numerus<d64> a, numerus<d64> b) → numerus<d64> { redde a ÷ b }");
}

#[test]
fn cross_family_comparison_is_accepted() {
    for (lhs, rhs) in [
        ("numerus", "fractus<f64>"),
        ("numerus<u8>", "fractus<f32>"),
        ("numerus<d64>", "fractus<f64>"),
        ("numerus<d64>", "numerus"),
        ("fractus<f32>", "numerus<i64>"),
        ("fractus<f64>", "numerus<d32>"),
    ] {
        for glyph in ["<", ">", "≤", "≥", "≅", "≇", "≈", "≉"] {
            assert_accepts(&format!(
                "functio p({lhs} a, {rhs} b) → bivalens {{ redde a {glyph} b }}"
            ));
        }
    }
}

#[test]
fn strict_equality_still_rejects_cross_family() {
    assert_rejects_with_issue("functio p() → bivalens { redde 1 ≡ 1.0 }", EXACT_EQ);
    assert_rejects_with_issue(
        "functio p(numerus a, fractus<f64> b) → bivalens { redde a ≡ b }",
        EXACT_EQ,
    );
    assert_rejects_with_issue(
        "functio p(numerus<d64> a, fractus<f64> b) → bivalens { redde a ≠ b }",
        EXACT_EQ,
    );
}
