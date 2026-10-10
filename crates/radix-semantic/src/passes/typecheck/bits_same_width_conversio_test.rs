//! Typecheck pins for same-width `bN ↔ uN` conversions (need, unit
//! 1.4a, C2 and C3): accepted without a `via` clause, scalars and lists, at
//! the same width as `uN`, `iN` and the IEEE floats; every other pairing is a
//! named refusal.

use crate::test_support;

#[test]
fn same_width_scalar_conversions_accept_both_directions() {
    test_support::assert_accepts("functio f(b8 a) → u8 { redde a ↦ u8 }");
    test_support::assert_accepts("functio f(u8 a) → b8 { redde a ↦ b8 }");
    test_support::assert_accepts("functio f(b16 a) → u16 { redde a ↦ u16 }");
    test_support::assert_accepts("functio f(u32 a) → b32 { redde a ↦ b32 }");
    test_support::assert_accepts("functio f(b64 a) → u64 { redde a ↦ u64 }");
}

#[test]
fn same_width_list_conversions_accept_both_directions() {
    test_support::assert_accepts("functio f(lista<b16> a) → lista<u16> { redde a ↦ lista<u16> }");
    test_support::assert_accepts("functio f(lista<u8> a) → lista<b8> { redde a ↦ lista<b8> }");
}

#[test]
fn width_change_is_a_named_refusal() {
    for source in [
        "functio f(b8 a) → u16 { redde a ↦ u16 }",
        "functio f(b16 a) → u8 { redde a ↦ u8 }",
        "functio f(u32 a) → b64 { redde a ↦ b64 }",
        "functio f(b8 a) → numerus { redde a ↦ numerus }",
        "functio f(b8 a) → b16 { redde a ↦ b16 }",
        "functio f(lista<b8> a) → lista<numerus> { redde a ↦ lista<numerus> }",
    ] {
        test_support::assert_rejects_with_issue(source, "bits_conversion_width_mismatch");
    }
}

#[test]
fn same_width_signed_conversions_accept_both_directions() {
    test_support::assert_accepts("functio f(b8 a) → i8 { redde a ↦ i8 }");
    test_support::assert_accepts("functio f(i8 a) → b8 { redde a ↦ b8 }");
    test_support::assert_accepts("functio f(b16 a) → i16 { redde a ↦ i16 }");
    test_support::assert_accepts("functio f(i32 a) → b32 { redde a ↦ b32 }");
    test_support::assert_accepts("functio f(b64 a) → i64 { redde a ↦ i64 }");
}

#[test]
fn same_width_float_conversions_accept_both_directions() {
    test_support::assert_accepts("functio f(b16 a) → f16 { redde a ↦ f16 }");
    test_support::assert_accepts("functio f(b16 a) → bf16 { redde a ↦ bf16 }");
    test_support::assert_accepts("functio f(b32 a) → f32 { redde a ↦ f32 }");
    test_support::assert_accepts("functio f(b64 a) → f64 { redde a ↦ f64 }");
    test_support::assert_accepts("functio f(f16 a) → b16 { redde a ↦ b16 }");
    test_support::assert_accepts("functio f(bf16 a) → b16 { redde a ↦ b16 }");
    test_support::assert_accepts("functio f(f32 a) → b32 { redde a ↦ b32 }");
    test_support::assert_accepts("functio f(f64 a) → b64 { redde a ↦ b64 }");
}

#[test]
fn signed_and_float_width_mismatch_is_a_named_refusal() {
    for source in [
        "functio f(b8 a) → i16 { redde a ↦ i16 }",
        "functio f(i32 a) → b64 { redde a ↦ b64 }",
        "functio f(b16 a) → f32 { redde a ↦ f32 }",
        "functio f(b32 a) → f64 { redde a ↦ f64 }",
        "functio f(b8 a) → f16 { redde a ↦ f16 }",
        "functio f(f64 a) → b32 { redde a ↦ b32 }",
        "functio f(f16 a) → b8 { redde a ↦ b8 }",
    ] {
        test_support::assert_rejects_with_issue(source, "bits_conversion_width_mismatch");
    }
}

#[test]
fn signed_and_float_lists_convert_element_wise_at_the_same_width() {
    test_support::assert_accepts("functio f(lista<b8> a) → lista<i8> { redde a ↦ lista<i8> }");
    test_support::assert_accepts("functio f(lista<i16> a) → lista<b16> { redde a ↦ lista<b16> }");
    test_support::assert_accepts("functio f(lista<b32> a) → lista<f32> { redde a ↦ lista<f32> }");
    test_support::assert_rejects_with_issue(
        "functio f(lista<b8> a) → lista<f32> { redde a ↦ lista<f32> }",
        "bits_via_clause_required",
    );
}

#[test]
fn number_to_float_is_still_a_value_conversion() {
    test_support::assert_accepts("functio f(u16 a) → f16 { redde a ↦ f16 }");
    test_support::assert_accepts("functio f(i32 a) → f32 { redde a ↦ f32 }");
}

#[test]
fn signed_and_float_conversions_refuse_a_via_clause() {
    test_support::assert_rejects_with_issue(
        "functio f(b8 a) → i8 { redde a ↦ i8 via Le }",
        "bits_via_clause_not_allowed",
    );
    test_support::assert_rejects_with_issue(
        "functio f(b32 a) → f32 { redde a ↦ f32 via Be }",
        "bits_via_clause_not_allowed",
    );
}

#[test]
fn via_clause_on_a_same_width_conversion_is_a_named_refusal() {
    test_support::assert_rejects_with_issue(
        "functio f(b8 a) → u8 { redde a ↦ u8 via Le }",
        "bits_via_clause_not_allowed",
    );
    test_support::assert_rejects_with_issue(
        "functio f(u16 a) → b16 { redde a ↦ b16 via Be }",
        "bits_via_clause_not_allowed",
    );
    test_support::assert_rejects_with_issue(
        "functio f(lista<u8> a) → lista<b8> { redde a ↦ lista<b8> via Le }",
        "bits_via_clause_not_allowed",
    );
}

#[test]
fn list_width_change_without_a_clause_is_a_named_refusal() {
    for source in [
        "functio f(lista<b8> a) → lista<b16> { redde a ↦ lista<b16> }",
        "functio f(lista<b8> a) → lista<u16> { redde a ↦ lista<u16> }",
        "functio f(lista<b16> a) → lista<b8> { redde a ↦ lista<b8> }",
        "functio f(lista<u32> a) → lista<b8> { redde a ↦ lista<b8> }",
    ] {
        test_support::assert_rejects_with_issue(source, "bits_via_clause_required");
    }
}

#[test]
fn list_width_change_with_a_byte_order_clause_is_accepted() {
    for source in [
        "functio f(lista<b8> a) → lista<u16> { redde a ↦ lista<u16> via Le }",
        "functio f(lista<b8> a) → lista<i32> { redde a ↦ lista<i32> via Be }",
        "functio f(lista<b8> a) → lista<f32> { redde a ↦ lista<f32> via Le }",
        "functio f(lista<b8> a) → lista<b16> { redde a ↦ lista<b16> via Be }",
        "functio f(lista<b16> a) → lista<b8> { redde a ↦ lista<b8> via Le }",
        "functio f(lista<u16> a) → lista<b8> { redde a ↦ lista<b8> via Le }",
    ] {
        test_support::assert_accepts(source);
    }
}

#[test]
fn list_clause_on_an_eight_bit_element_is_refused() {
    for source in [
        "functio f(lista<b8> a) → lista<u8> { redde a ↦ lista<u8> via Le }",
        "functio f(lista<b8> a) → lista<i8> { redde a ↦ lista<i8> via Be }",
        "functio f(lista<u8> a) → lista<b8> { redde a ↦ lista<b8> via Be }",
        "functio f(lista<b16> a) → lista<u16> { redde a ↦ lista<u16> via Le }",
    ] {
        test_support::assert_rejects_with_issue(source, "bits_via_clause_not_allowed");
    }
}

#[test]
fn list_clause_without_a_bits_list_is_refused() {
    test_support::assert_rejects_with_issue(
        "functio f(lista<u8> a) → lista<u16> { redde a ↦ lista<u16> via Le }",
        "conversio_via_target_takes_no_hint",
    );
}
