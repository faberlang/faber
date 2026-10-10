//! Typecheck pins for same-width `bN ↔ uN` conversions (need, unit
//! 1.4a): accepted without a `via` clause, scalars and lists; every other
//! number pairing is a named refusal.

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
        "functio f(lista<b8> a) → lista<b16> { redde a ↦ lista<b16> }",
        "functio f(lista<b8> a) → lista<numerus> { redde a ↦ lista<numerus> }",
    ] {
        test_support::assert_rejects_with_issue(source, "bits_conversion_width_mismatch");
    }
}

#[test]
fn signed_and_float_conversions_are_unsettled_refusals() {
    test_support::assert_rejects_with_issue(
        "functio f(b8 a) → i8 { redde a ↦ i8 }",
        "bits_signed_conversion_unsettled",
    );
    test_support::assert_rejects_with_issue(
        "functio f(i32 a) → b32 { redde a ↦ b32 }",
        "bits_signed_conversion_unsettled",
    );
    test_support::assert_rejects_with_issue(
        "functio f(b16 a) → f16 { redde a ↦ f16 }",
        "bits_float_conversion_unsettled",
    );
    test_support::assert_rejects_with_issue(
        "functio f(f64 a) → b64 { redde a ↦ b64 }",
        "bits_float_conversion_unsettled",
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
