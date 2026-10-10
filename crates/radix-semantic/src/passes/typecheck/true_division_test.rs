//! `÷` true division (d11-6 ruling 15) and cross-family comparison
//! (ruling 22) typecheck probes.

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

const FAMILY_CROSSING: &str = "numeric_family_crossing_requires_conversio";
const EXACT_EQ: &str = "equality_exact_types_required";

#[test]
fn constant_true_division_is_f64() {
    assert_accepts("functio p() → f64 { redde 7 ÷ 2 }");
    assert_rejects_with_issue(
        "functio p() → numerus { redde 7 ÷ 2 }",
        "return_type_mismatch",
    );
}

// Ruling 23 amends ruling 15: the float width follows the operand widths.
#[test]
fn integer_true_division_width_follows_the_operands() {
    // 8- and 16-bit operands: f32.
    assert_accepts("functio p(u8 a, u8 b) → f32 { redde a ÷ b }");
    assert_accepts("functio p(i16 a, u8 b) → f32 { redde a ÷ b }");
    // 32/64-bit or default `int`: f64; mixed widths use the wider.
    assert_accepts("functio p(i16 a, i32 b) → f64 { redde a ÷ b }");
    assert_accepts("functio p(i32 a, u16 b) → f64 { redde a ÷ b }");
    assert_accepts("functio p(numerus a) → f64 { redde a ÷ 3 }");
    // A constant joins by value beside a typed operand, defaults to `int` alone.
    assert_accepts("functio p(u8 a) → f32 { redde a ÷ 2 }");
    assert_accepts("functio p(u8 a) → f32 { redde 1000 ÷ a }");
    assert_accepts("functio p(u8 a) → f64 { redde a ÷ 100000 }");
    assert_accepts("functio p() → f64 { redde 7 ÷ 2 }");
    // A sub-expression keys on its range type: `u8 + u8` is `u16`.
    assert_accepts("functio p(u8 a, u8 b, u8 c) → f32 { redde (a + b) ÷ c }");
    assert_accepts("functio p(u8 a, u8 b, u8 c) → f64 { redde (a * b * c) ÷ c }");
    assert_rejects_with_issue(
        "functio p(u8 a, u8 b) → u8 { redde a ÷ b }",
        "return_type_mismatch",
    );
    // The reverse: an `f64` quotient does not narrow into `f32`.
    assert_rejects_with_issue(
        "functio p(numerus a, numerus b) → f32 { redde a ÷ b }",
        "return_type_mismatch",
    );
}

#[test]
fn word_true_division_follows_the_word_width() {
    assert_accepts("functio p(modulus<u8> a, modulus<u8> b) → f32 { redde a ÷ b }");
    assert_accepts("functio p(modulus<u32> a, modulus<u32> b) → f64 { redde a ÷ b }");
    assert_accepts("functio p(saturatus<i16> a) → f32 { redde a ÷ 2 }");
}

#[test]
fn float_true_division_keeps_the_operand_width() {
    assert_accepts("functio p(f32 a, f32 b) → f32 { redde a ÷ b }");
    assert_rejects_with_issue(
        "functio p(f64 a, f64 b) → f32 { redde a ÷ b }",
        "return_type_mismatch",
    );
}

#[test]
fn float_beside_integer_constant_is_accepted() {
    assert_accepts("functio p(f64 a) → f64 { redde a ÷ 2 }");
    assert_accepts("functio p(f64 a) → f64 { redde 2 ÷ a }");
}

#[test]
fn integer_beside_float_variable_is_rejected() {
    assert_rejects_with_issue(
        "functio p(numerus a, f64 b) → f64 { redde a ÷ b }",
        FAMILY_CROSSING,
    );
    assert_rejects_with_issue(
        "functio p(f64 a, u8 b) → f64 { redde a ÷ b }",
        FAMILY_CROSSING,
    );
}

#[test]
fn decimal_true_division_follows_slash() {
    assert_accepts("functio p(d64 a, d64 b) → d64 { redde a ÷ b }");
}

#[test]
fn cross_family_comparison_is_accepted() {
    for (lhs, rhs) in [
        ("numerus", "f64"),
        ("u8", "f32"),
        ("d64", "f64"),
        ("d64", "numerus"),
        ("f32", "i64"),
        ("f64", "d64"),
    ] {
        for glyph in ["≺", "≻", "≤", "≥", "≅", "≇", "≈", "≉"] {
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
        "functio p(numerus a, f64 b) → bivalens { redde a ≡ b }",
        EXACT_EQ,
    );
    assert_rejects_with_issue(
        "functio p(d64 a, f64 b) → bivalens { redde a ≢ b }",
        EXACT_EQ,
    );
}
