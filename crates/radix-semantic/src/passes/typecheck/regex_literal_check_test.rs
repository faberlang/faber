//! Inline regex literal contents are not validated at check time
//! (operator ruling d381ae56, amended 2026-10-14).
//!
//! `regex /pattern/` types as `regex`; its raw body is passed to the target's
//! native engine unchecked, exactly as when a pattern was a plain `textus`.
//! Only the delimiters are the lexer's concern. The failable `↦ regex`
//! conversion for dynamic patterns is unchanged. Assertions are structured
//! (acceptance), never message text.

use crate::test_support::{assert_accepts, assert_rejects_with_issue};

#[test]
fn unbalanced_pattern_literal_is_accepted() {
    assert_accepts(r"incipit { fixum regex r ← regex /(/ }");
}

#[test]
fn valid_literal_pattern_passes_check() {
    assert_accepts(r"incipit { fixum regex r ← regex /a+(b|c)?/ }");
}

#[test]
fn inline_flag_literal_pattern_passes_check() {
    assert_accepts(r"incipit { fixum regex r ← regex /(?i)\d+/ }");
}

#[test]
fn engine_specific_patterns_pass_check_unchecked() {
    // Lookahead and a backreference: legal in some target engines, rejected
    // by the Rust engine; the checker does not inspect the body.
    assert_accepts(r"incipit { fixum regex a ← regex /(?=a)b/ }");
    assert_accepts(r"incipit { fixum regex b ← regex /(a)\1/ }");
}

#[test]
fn dynamic_conversion_stays_failable_not_check_time() {
    assert_accepts(r#"incipit { fixum regex r ← "(" ↦ regex }"#);
}

// `_ /pat/` is an inferred-constructor hole: it takes its type from the
// expected type and is named when no expected type exists (BUG-01). The
// explicit `regex /pat/` form stays self-typed.

#[test]
fn inferred_regex_double_hole_is_named_error() {
    assert_rejects_with_issue(
        r"incipit { fixum _ r ← _ /a/ }",
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn inferred_regex_context_free_is_named_error() {
    assert_rejects_with_issue(
        r"incipit { nota _ /a/ }",
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn inferred_regex_typed_binding_accepts() {
    assert_accepts(r"incipit { fixum regex r ← _ /a/ }");
}

#[test]
fn inferred_regex_argument_context_accepts() {
    assert_accepts(r"functio f(regex r) → numerus { redde 0 } incipit { nota f(_ /a+/) }");
}

#[test]
fn inferred_regex_return_context_accepts() {
    assert_accepts(r"functio g() → regex { redde _ /a+/ } incipit { nota g() }");
}

#[test]
fn inferred_regex_optional_slot_accepts() {
    assert_accepts(r"incipit { fixum regex ∪ nihil r ← _ /a/ }");
}

#[test]
fn explicit_regex_literal_stays_self_typed() {
    assert_accepts(r"incipit { fixum _ r ← regex /a/ }");
}
