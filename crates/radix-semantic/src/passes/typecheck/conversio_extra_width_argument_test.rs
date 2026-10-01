//! cv1: a `numerus` / `fractus` conversion target takes no type arguments.
//!
//! Lowering once kept only the first type argument of a conversion target, so
//! `n ↦ numerus<u32, Foo>` passed while the declaration was a semantic error.
//! The wrapped numeric spelling is now retired whole (N7d): a declaration and
//! a conversion target reject every wrapped form, extra arguments and holes
//! included, with the same parse diagnostic `numeric_wrapper_retired`. A
//! convert hint is a `via` clause (D11.9), and stays accepted.

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

fn conversion(target: &str) -> String {
    format!("functio f(numerus n) → numerus {{\n    redde n ↦ {target} ⊥ 0\n}}\n")
}

fn declaration(target: &str) -> String {
    format!("functio g() → {target} {{\n    redde 0\n}}\n")
}

#[test]
fn every_extra_argument_wrapper_is_the_retired_wrapper_in_both_positions() {
    for target in [
        "numerus<u32, Foo>",
        "numerus<u32, Foo, Bar>",
        "fractus<f32, Foo>",
        "numerus<u32, u64>",
        "numerus<u32, _>",
        "fractus<f32, f64>",
    ] {
        assert_rejects_with_issue(&declaration(target), "numeric_wrapper_retired");
        assert_rejects_with_issue(&conversion(target), "numeric_wrapper_retired");
    }
}

#[test]
fn one_width_marker_and_a_via_clause_stay_accepted() {
    assert_accepts("functio f(numerus n) → u32 {\n    redde n ↦ u32 ⊥ 0\n}\n");
    assert_accepts("functio f(textus s) → u32 {\n    redde s ↦ u32 via Hex ⊥ 0\n}\n");
}
