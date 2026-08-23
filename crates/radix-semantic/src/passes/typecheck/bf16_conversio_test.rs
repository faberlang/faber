//! Typecheck pins for the first-class `fractus<bf16>` conversion rows.
//!
//! The semantic layer admits the value-conversion routes here. Exact bf16
//! widening, RNE narrowing, and the f16 bridge are representation contracts
//! for later lowering; this crate must preserve their explicit source/target
//! widths and must not turn bf16 into an implicit f32 alias.

use crate::test_support;

#[test]
fn bf16_to_f32_value_conversion_is_admitted() {
    test_support::assert_accepts(
        "functio widen(fractus<bf16> x) → fractus<f32> { redde x ↦ fractus<f32> }",
    );
}

#[test]
fn f32_to_bf16_value_conversion_is_admitted() {
    test_support::assert_accepts(
        "functio narrow(fractus<f32> x) → fractus<bf16> { redde x ↦ fractus<bf16> }",
    );
}

#[test]
fn f16_and_bf16_value_conversions_are_admitted_through_f32() {
    test_support::assert_accepts(
        "functio to_bf16(fractus<f16> x) → fractus<bf16> { redde x ↦ fractus<bf16> }",
    );
    test_support::assert_accepts(
        "functio to_f16(fractus<bf16> x) → fractus<f16> { redde x ↦ fractus<f16> }",
    );
}

#[test]
fn bf16_literal_and_print_forms_keep_the_whole_marker() {
    test_support::assert_accepts(
        r#"
incipit {
    fixum fractus<bf16> value ← 1.0
    nota value
}
"#,
    );
    test_support::assert_accepts("functio keep(bf16 x) → bf16 { redde x }");
}

#[test]
fn bf16_does_not_silently_widen_to_f32() {
    test_support::assert_rejects_with_issue(
        r#"
incipit {
    fixum fractus<bf16> value ← 1.0
    fixum fractus<f32> widened ← value
}
"#,
        "initializer_annotation_mismatch",
    );
}

#[test]
fn u16_to_bf16_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(numerus<u16> n) → fractus<bf16> { redde n ↦ fractus<bf16, Bits> }",
    );
}

#[test]
fn bf16_to_u16_bits_accepts() {
    test_support::assert_accepts(
        "functio bits(fractus<bf16> x) → numerus<u16> { redde x ↦ numerus<u16, Bits> }",
    );
}

#[test]
fn bf16_and_f16_bits_are_not_a_pair() {
    test_support::assert_rejects_with_issue(
        "functio bits(fractus<bf16> x) → fractus<f16> { redde x ↦ fractus<f16, Bits> }",
        "bits_pair_width_mismatch",
    );
    test_support::assert_rejects_with_issue(
        "functio bits(fractus<f16> x) → fractus<bf16> { redde x ↦ fractus<bf16, Bits> }",
        "bits_pair_width_mismatch",
    );
}
