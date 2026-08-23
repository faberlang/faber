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

// U1 red drivers (goal `bits-f16-pair`, unit U1): the `u16 ↔ f16` `Bits` row
// is admitted in U2; until then both directions reject `bits_pair_width_mismatch`.
// The accept tests flip green when U2 extends `bits_pair_issue`; the reject
// tests are expected to pass today.
//
// Expected accept values fixed in advance for the runner pin (U3):
//   u16 0x3C00 ↔ 1.0, 0x3E00 ↔ 1.5, 0xBC00 ↔ -1.0, 0x7BFF ↔ 65504,
//   subnormal 0x0001, NaN payload 0x7E00 round-trips to the original bits.

#[test]
fn u16_to_f16_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(numerus<u16> n) → fractus<f16> { redde n ↦ fractus<f16, Bits> }",
    );
}

#[test]
fn f16_to_u16_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(fractus<f16> x) → numerus<u16> { redde x ↦ numerus<u16, Bits> }",
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
fn u16_to_f16_bits_rejects_today() {
    test_support::assert_rejects_with_issue(
        "functio bits(numerus<u16> n) → fractus<f16> { redde n ↦ fractus<f16, Bits> }",
        "bits_pair_width_mismatch",
    );
}

#[test]
fn f16_to_u16_bits_rejects_today() {
    test_support::assert_rejects_with_issue(
        "functio bits(fractus<f16> x) → numerus<u16> { redde x ↦ numerus<u16, Bits> }",
        "bits_pair_width_mismatch",
    );
}

#[test]
fn f16_to_u32_bits_rejects() {
    test_support::assert_rejects_with_issue(
        "functio bits(fractus<f16> x) → numerus<u32> { redde x ↦ numerus<u32, Bits> }",
        "bits_pair_width_mismatch",
    );
}

// OQ3 default: `Bits` never co-occurs with `Hex`/`Bin`/`Oct` in one hint
// list. Red today (co-occurrence is silently accepted); the issue code is
// deliberately not pinned until U2 settles the diagnostic shape.
fn assert_rejects(source: &str) {
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()),
        "expected rejection, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn bits_with_hex_hint_rejects() {
    assert_rejects(
        "functio bits(numerus<u32> n) → fractus<f32> { redde n ↦ fractus<f32, Bits, Hex> }",
    );
}

#[test]
fn bits_with_bin_hint_rejects() {
    assert_rejects(
        "functio bits(numerus<u32> n) → fractus<f32> { redde n ↦ fractus<f32, Bits, Bin> }",
    );
}

#[test]
fn bits_with_oct_hint_rejects() {
    assert_rejects(
        "functio bits(numerus<u32> n) → fractus<f32> { redde n ↦ fractus<f32, Bits, Oct> }",
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
    test_support::assert_accepts("incipit { fixum _ n ← \"ff\" ↦ numerus<u32, Hex> }");
}
