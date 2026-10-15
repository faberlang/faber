//! Typecheck pins for the first-class `fractus<bf16>` conversion rows.
//!
//! The semantic layer admits the value-conversion routes here. Exact bf16
//! widening, RNE narrowing, and the f16 bridge are representation contracts
//! for later lowering; this crate must preserve their explicit source/target
//! widths and must not turn bf16 into an implicit f32 alias.

use crate::test_support;

#[test]
fn bf16_to_f32_value_conversion_is_admitted() {
    test_support::assert_accepts_la("functio widen(bf16 x) → f32 { redde x ↦ f32 }");
}

#[test]
fn f32_to_bf16_value_conversion_is_admitted() {
    test_support::assert_accepts_la("functio narrow(f32 x) → bf16 { redde x ↦ bf16 }");
}

#[test]
fn f16_and_bf16_value_conversions_are_admitted_through_f32() {
    test_support::assert_accepts_la("functio to_bf16(f16 x) → bf16 { redde x ↦ bf16 }");
    test_support::assert_accepts_la("functio to_f16(bf16 x) → f16 { redde x ↦ f16 }");
}

#[test]
fn bf16_literal_and_print_forms_keep_the_whole_marker() {
    test_support::assert_accepts_la(
        r"
incipit {
    sit bf16 value ← 1.0
    nota value
}
",
    );
    test_support::assert_accepts_la("functio keep(bf16 x) → bf16 { redde x }");
}

#[test]
fn bf16_does_not_silently_widen_to_f32() {
    test_support::assert_rejects_with_issue_la(
        r"
incipit {
    sit bf16 value ← 1.0
    sit f32 widened ← value
}
",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn u16_to_bf16_pattern_accepts() {
    test_support::assert_accepts_la("functio bits(u16 n) → bf16 { redde n ↦ b16 ↦ bf16 }");
}

#[test]
fn bf16_to_u16_pattern_accepts() {
    test_support::assert_accepts_la("functio bits(bf16 x) → u16 { redde x ↦ b16 ↦ u16 }");
}
