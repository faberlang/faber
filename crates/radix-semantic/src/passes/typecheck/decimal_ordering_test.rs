//! Decimal ordering admission tests (OX P1).
//!
//! Decimal carriers join losslessly for ordering: the d64 scale joins
//! decimal/integer pairs use the same scaled join. Decimal/fractus is
//! accepted (d11-6 ruling 22: cross-family comparison).

use crate::test_support;

#[test]
fn decimal_integer_ordering_accepts() {
    test_support::assert_accepts(
        "functio maior(numerus<d64> a, numerus n) → bivalens { redde a > n }",
    );
}

#[test]
fn decimal_fractus_ordering_accepts_cross_family() {
    test_support::assert_accepts(
        "functio maior(numerus<d64> a, fractus f) → bivalens { redde a > f }",
    );
}

#[test]
fn same_width_decimal_ordering_stays_admitted() {
    test_support::assert_accepts(
        "functio maior(numerus<d64> a, numerus<d64> b) → bivalens { redde a > b }",
    );
}
