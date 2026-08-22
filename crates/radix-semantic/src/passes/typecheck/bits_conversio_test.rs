//! Typecheck pins for the `Bits` bitcast hint (`numerus<u32|u64> ↔ fractus<f32|f64>`).

use crate::test_support;

#[test]
fn u32_to_f32_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(numerus<u32> n) → fractus<f32> { redde n ↦ fractus<f32, Bits> }",
    );
}

#[test]
fn f32_to_u32_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(fractus<f32> x) → numerus<u32> { redde x ↦ numerus<u32, Bits> }",
    );
}

#[test]
fn u64_to_f64_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(numerus<u64> n) → fractus<f64> { redde n ↦ fractus<f64, Bits> }",
    );
}

#[test]
fn f64_to_u64_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(fractus<f64> x) → numerus<u64> { redde x ↦ numerus<u64, Bits> }",
    );
}

#[test]
fn u16_to_f32_bits_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(numerus<u16> n) → fractus<f32> { redde n ↦ fractus<f32, Bits> }",
        "bits_pair_width_mismatch",
    );
}

#[test]
fn unsized_fractus_to_u32_bits_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(fractus x) → numerus<u32> { redde x ↦ numerus<u32, Bits> }",
        "bits_pair_width_mismatch",
    );
}

#[test]
fn u32_to_f64_bits_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(numerus<u32> n) → fractus<f64> { redde n ↦ fractus<f64, Bits> }",
        "bits_pair_width_mismatch",
    );
}

#[test]
fn without_bits_value_conversion_stays() {
    test_support::assert_accepts(
        "functio val(numerus<u32> n) → fractus<f64> { redde n ↦ fractus<f64> }",
    );
}

#[test]
fn radix_hint_accept_row_stays_green() {
    test_support::assert_accepts(
        "incipit { fixum _ n ← \"ff\" ↦ numerus<u32, Hex> }",
    );
}
