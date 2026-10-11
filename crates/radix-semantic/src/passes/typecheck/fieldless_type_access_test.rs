//! Field access on a built-in type reports a named checker diagnostic.
//!
//! `field_type_from_type` used to fall through to the silent error type, so
//! `1.nonsense`, `"a".length` and `xs.length` passed `faber check` and only
//! failed in MIR and in emitted Rust. Built-in types have methods, not fields;
//! `length` is a method (`length()`). Two exceptions are hard-coded in the same
//! function and pinned here by name: `.nuntius` on `textus`/`ignotum` (the
//! caught-error message) and arbitrary dot-key lookup on map types.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use radix_diagnostics::DiagnosticArg;

use crate::test_support;

#[test]
fn unknown_field_on_numerus_is_named() {
    test_support::assert_rejects_with_args(
        "functio f(numerus n) → numerus { redde n.nonsense }",
        &[
            DiagnosticArg::new("issue", "type_has_no_field"),
            DiagnosticArg::new("member", "nonsense"),
            DiagnosticArg::new("type", "numerus"),
        ],
    );
}

#[test]
fn length_field_on_textus_points_to_the_method() {
    test_support::assert_rejects_with_issue(
        "functio f(textus s) → numerus { redde s.length }",
        "length_is_method",
    );
}

#[test]
fn latin_longitudo_field_points_to_the_method() {
    test_support::assert_rejects_with_issue(
        "functio f(lista<numerus> xs) → numerus { redde xs.longitudo }",
        "length_is_method",
    );
    test_support::assert_accepts("functio f(lista<numerus> xs) → numerus { redde xs.longitudo() }");
}

#[test]
fn length_field_on_lista_points_to_the_method() {
    test_support::assert_rejects_with_issue(
        "functio f(lista<numerus> xs) → numerus { redde xs.length }",
        "length_is_method",
    );
}

#[test]
fn length_field_on_numerus_is_a_plain_unknown_field() {
    test_support::assert_rejects_with_issue(
        "functio f(numerus n) → numerus { redde n.length }",
        "type_has_no_field",
    );
}

#[test]
fn other_field_on_textus_is_unknown_field() {
    test_support::assert_rejects_with_issue(
        "functio f(textus s) → numerus { redde s.size }",
        "type_has_no_field",
    );
}

#[test]
fn length_method_call_is_accepted() {
    test_support::assert_accepts("functio f(textus s) → numerus { redde s.longitudo() }");
    test_support::assert_accepts("functio f(lista<numerus> xs) → numerus { redde xs.longitudo() }");
}

/// PINNED EXCEPTION: `.nuntius` on `textus` is the caught-error message
/// (`corpus/fac/fac-cape.fab` reads `err.nuntius`). It stays accepted until a
/// separate ruling retires it.
#[test]
fn pinned_exception_nuntius_on_textus_is_accepted() {
    test_support::assert_accepts("functio f(textus e) → textus { redde e.nuntius }");
}

/// PINNED EXCEPTION: `.nuntius` on `ignotum` (the caught-error value) is the
/// same message hack and stays accepted.
#[test]
fn pinned_exception_nuntius_on_ignotum_is_accepted() {
    test_support::assert_accepts("functio f(ignotum e) → textus { redde e.nuntius }");
}

/// PINNED EXCEPTION: arbitrary dot-key lookup on a map type returns the value
/// type for any name. It stays accepted until a separate ruling retires it.
#[test]
fn pinned_exception_map_dot_key_lookup_is_accepted() {
    test_support::assert_accepts(
        "functio f(tabula<textus, numerus> m) → numerus { redde m.anything }",
    );
}

#[test]
fn nuntius_exception_does_not_cover_other_names_on_textus() {
    test_support::assert_rejects_with_issue(
        "functio f(textus e) → textus { redde e.message }",
        "type_has_no_field",
    );
}
