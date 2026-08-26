//! Decimal ordering admission tests (OX P1).
//!
//! Decimal carriers join losslessly for ordering: d32/d64 use the d64 scale,
//! and decimal/integer pairs use the same scaled join. Decimal/fractus stays
//! rejected because the cross-family ordering carrier is not settled.

use crate::test_support;

#[test]
fn cross_width_decimal_ordering_accepts() {
    test_support::assert_accepts(
        "functio maior(numerus<d32> a, numerus<d64> b) → bivalens { redde a > b }",
    );
}

#[test]
fn decimal_integer_ordering_accepts() {
    test_support::assert_accepts(
        "functio maior(numerus<d32> a, numerus n) → bivalens { redde a > n }",
    );
}

#[test]
fn decimal_fractus_ordering_rejects_cross_family() {
    test_support::assert_rejects_with_issue(
        "functio maior(numerus<d32> a, fractus f) → bivalens { redde a > f }",
        "mixed_decimal_fractus_ordering",
    );
}

#[test]
fn same_width_decimal_ordering_stays_admitted() {
    test_support::assert_accepts(
        "functio maior(numerus<d32> a, numerus<d32> b) → bivalens { redde a > b }",
    );
}
