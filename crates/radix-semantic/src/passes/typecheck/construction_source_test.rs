//! Copy-with-changes construction typing (DECISIONS D15.1–D15.3, D16.3).
//!
//! `Genus { field = value } ex source` builds a new `Genus`: the braced fields
//! override and every other field is copied from `source`. The source must be
//! the same genus type (`construction_source_type_mismatch`); required fields
//! may be omitted because the source supplies them; `fixum` fields can be set
//! because this is a construction literal.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use radix_diagnostics::DiagnosticArg;

use crate::test_support;

const TYPES: &str = "\
genus Punctum {
    fixum numerus x
    varia numerus y
    fixum lista<numerus> xs
}

genus Alius {
    fixum numerus x
}

";

fn with_types(body: &str) -> String {
    format!(
        "{TYPES}incipit {{\n    sit Punctum p ← Punctum {{ x = 1, y = 2, xs = [] }}\n{body}\n}}"
    )
}

#[test]
fn override_with_source_omits_required_fields() {
    test_support::assert_accepts(&with_types(
        "    sit Punctum q ← Punctum { y = 9 } ex p\n    nota q.y",
    ));
}

#[test]
fn empty_override_copies_every_field() {
    test_support::assert_accepts(&with_types(
        "    sit Punctum q ← Punctum {} ex p\n    nota q.x",
    ));
}

#[test]
fn fixum_field_can_be_set_in_copy_with_changes() {
    test_support::assert_accepts(&with_types(
        "    sit Punctum q ← Punctum { x = 5, xs = [1] } ex p\n    nota q.x",
    ));
}

#[test]
fn source_of_another_genus_is_rejected() {
    test_support::assert_rejects_with_args(
        &with_types(
            "    sit Alius a ← Alius { x = 3 }\n    sit Punctum q ← Punctum { y = 9 } ex a",
        ),
        &[DiagnosticArg::new(
            "issue",
            "construction_source_type_mismatch",
        )],
    );
}

#[test]
fn scalar_source_is_rejected() {
    test_support::assert_rejects_with_issue(
        &with_types("    sit Punctum q ← Punctum { y = 9 } ex 3"),
        "construction_source_type_mismatch",
    );
}

#[test]
fn override_value_type_is_still_checked() {
    test_support::assert_rejects_with_issue(
        &with_types("    sit Punctum q ← Punctum { y = \"nine\" } ex p"),
        "field_initializer_type_mismatch",
    );
}

#[test]
fn unknown_override_field_is_still_rejected() {
    test_support::assert_rejects_with_issue(
        &with_types("    sit Punctum q ← Punctum { z = 9 } ex p"),
        "unknown_field",
    );
}

#[test]
fn missing_required_field_without_source_is_still_rejected() {
    test_support::assert_rejects_with_issue(
        &with_types("    sit Punctum q ← Punctum { y = 9 }"),
        "missing_required_struct_field",
    );
}
