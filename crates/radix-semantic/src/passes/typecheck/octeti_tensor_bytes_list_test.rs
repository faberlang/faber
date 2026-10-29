//! Typecheck pins for the bulk byte decode `lista<b8> ↦ tensor<E, [shape]>
//! via Le|Be` (need, octeti retirement slice 2d, ruling B6): every
//! element wider than 8 bits requires the clause, an 8-bit element takes none
//! and refuses one.

use crate::test_support;

#[test]
fn wide_element_with_a_clause_is_accepted() {
    for source in [
        "functio f(lista<b8> a) → tensor<u16, [2]> ⇥ textus { redde a ↦ tensor<u16, [2]> via Le }",
        "functio f(lista<b8> a) → tensor<f32, [2]> ⇥ textus { redde a ↦ tensor<f32, [2]> via Be }",
    ] {
        test_support::assert_accepts(source);
    }
}

#[test]
fn wide_element_without_a_clause_is_refused() {
    for source in [
        "functio f(lista<b8> a) → tensor<u16, [2]> ⇥ textus { redde a ↦ tensor<u16, [2]> }",
        "functio f(lista<b8> a) → tensor<i32, [2]> ⇥ textus { redde a ↦ tensor<i32, [2]> }",
    ] {
        test_support::assert_rejects_with_issue(source, "list_b8_to_tensor_unspecified");
    }
}

#[test]
fn eight_bit_element_takes_no_clause() {
    for source in [
        "functio f(lista<b8> a) → tensor<u8, [4]> ⇥ textus { redde a ↦ tensor<u8, [4]> }",
        "functio f(lista<b8> a) → tensor<i8, [4]> ⇥ textus { redde a ↦ tensor<i8, [4]> }",
    ] {
        test_support::assert_accepts(source);
    }
}

#[test]
fn eight_bit_element_with_a_clause_is_refused() {
    for source in [
        "functio f(lista<b8> a) → tensor<u8, [4]> ⇥ textus { redde a ↦ tensor<u8, [4]> via Le }",
        "functio f(lista<b8> a) → tensor<i8, [4]> ⇥ textus { redde a ↦ tensor<i8, [4]> via Be }",
    ] {
        test_support::assert_rejects_with_issue(source, "bits_via_clause_not_allowed");
    }
}
