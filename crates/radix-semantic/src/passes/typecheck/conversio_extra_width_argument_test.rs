//! cv1: a `numerus` / `fractus` conversion target takes exactly the type
//! arguments a declaration of the same type takes.
//!
//! Lowering once kept only the first type argument of a conversion target, so
//! `n ↦ numerus<u32, Foo>` passed while `numerus<u32, Foo>` in a declaration
//! is `SEM002:invalid_numerus_width_marker`. An unexplained extra argument is
//! now rejected with the declaration's diagnostic; a convert hint is a `via`
//! clause (D11.9), so it stays accepted. The parser-level hole
//! `numerus<u32, _>` (qp1c) still parses; it is now a semantic
//! `numerus_width_params` error, as in a declaration.

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

fn conversion(target: &str) -> String {
    format!("functio f(numerus n) → numerus {{\n    redde n ↦ {target} ⊥ 0\n}}\n")
}

fn declaration(target: &str) -> String {
    format!("functio g() → {target} {{\n    redde 0\n}}\n")
}

#[test]
fn an_unnamed_extra_argument_is_the_declarations_invalid_marker() {
    for (target, issue) in [
        ("numerus<u32, Foo>", "invalid_numerus_width_marker"),
        ("numerus<u32, Foo, Bar>", "invalid_numerus_width_marker"),
        ("fractus<f32, Foo>", "invalid_fractus_width_marker"),
    ] {
        assert_rejects_with_issue(&declaration(target), issue);
        assert_rejects_with_issue(&conversion(target), issue);
    }
}

#[test]
fn a_valid_looking_or_hole_extra_argument_is_the_declarations_params_error() {
    for (target, issue) in [
        ("numerus<u32, u64>", "numerus_width_params"),
        ("numerus<u32, _>", "numerus_width_params"),
        ("fractus<f32, f64>", "fractus_width_params"),
    ] {
        assert_rejects_with_issue(&declaration(target), issue);
        assert_rejects_with_issue(&conversion(target), issue);
    }
}

#[test]
fn one_width_marker_and_a_via_clause_stay_accepted() {
    assert_accepts("functio f(numerus n) → numerus<u32> {\n    redde n ↦ numerus<u32> ⊥ 0\n}\n");
    assert_accepts(
        "functio f(textus s) → numerus<u32> {\n    redde s ↦ numerus<u32> via Hex ⊥ 0\n}\n",
    );
}
