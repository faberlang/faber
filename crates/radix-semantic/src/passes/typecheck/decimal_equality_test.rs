//! Decimal equality admission tests (GT-2).
//!
//! The ≅/≈ equality tiers mirror the ordering gate: decimal carriers join
//! losslessly for equality, decimal/integer pairs use the same scaled join,
//! and decimal/fractus is accepted (d11-6 ruling 22).

use crate::test_support;

#[test]
fn cross_width_decimal_equality_accepts() {
    test_support::assert_accepts(
        "functio aequalis(numerus<d32> a, numerus<d64> b) → bivalens { redde a ≅ b }",
    );
}

#[test]
fn cross_width_decimal_fuzzy_equality_accepts() {
    test_support::assert_accepts(
        "functio aequalis(numerus<d32> a, numerus<d64> b) → bivalens { redde a ≈ b }",
    );
}

#[test]
fn decimal_integer_equality_accepts() {
    test_support::assert_accepts(
        "functio aequalis(numerus<d32> a, numerus n) → bivalens { redde a ≅ n }",
    );
}

#[test]
fn decimal_fractus_equality_accepts_cross_family() {
    test_support::assert_accepts(
        "functio aequalis(numerus<d32> a, fractus f) → bivalens { redde a ≅ f }",
    );
}

#[test]
fn decimal_fractus_fuzzy_equality_accepts_cross_family() {
    test_support::assert_accepts(
        "functio aequalis(numerus<d32> a, fractus f) → bivalens { redde a ≈ f }",
    );
}

#[test]
fn same_width_decimal_equality_stays_admitted() {
    test_support::assert_accepts(
        "functio aequalis(numerus<d32> a, numerus<d32> b) → bivalens { redde a ≅ b }",
    );
}
