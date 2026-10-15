//! audit1-a2: `value!.method()` is an ordinary `MethodCall` whose receiver is
//! the bare non-null assertion, so the borrow pass sees the receiver write.
//!
//! Before the normalisation the call was `Call(NonNull(value, Member(method)))`,
//! which the borrow pass did not visit as a method call: an in-place write
//! through an optional alias of a `de` parameter, or a pop through `!` on an
//! `in` parameter, was invisible to the de-alias gate and WARN008. (A `mut`
//! optional parameter cannot be authored: the marker is a nested access
//! marker, so the WARN008 case uses a pass-through `!` on a plain parameter.)
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::test_support;
use crate::{SemanticErrorKind, WarningKind};

fn unused_mut_params(source: &str) -> usize {
    test_support::analyze_source(source)
        .result
        .diagnostics
        .iter()
        .filter(|err| {
            matches!(
                err.kind,
                SemanticErrorKind::Warning(WarningKind::UnusedMutRefParam)
            )
        })
        .count()
}

#[test]
fn optional_alias_of_de_list_rejects_non_null_appende() {
    test_support::assert_rejects_with_issue(
        "functio f(de lista<numerus> xs) → vacuum {\n sit lista<numerus> ∪ nihil ys ← xs\n ys!.appende(1)\n}\nincipit { }",
        "assign_to_de_parameter",
    );
}

#[test]
fn optional_alias_of_de_list_rejects_non_null_value_returning_mutator() {
    test_support::assert_rejects_with_issue(
        "functio f(de lista<numerus> xs) → vacuum {\n sit lista<numerus> ∪ nihil ys ← xs\n ys!.detrahe()\n}\nincipit { }",
        "assign_to_de_parameter",
    );
}

#[test]
fn optional_alias_of_de_list_read_through_non_null_stays_legal() {
    test_support::assert_accepts(
        "functio f(de lista<numerus> xs) → numerus {\n sit lista<numerus> ∪ nihil ys ← xs\n redde ys!.longitudo()\n}\nincipit { }",
    );
}

#[test]
fn in_param_written_through_non_null_is_not_unused_mut() {
    // `!` on a non-optional receiver is a pass-through assertion; the pop is
    // still a write to the `in` parameter, so no WARN008 'change to ref'.
    let source = "functio f(varia lista<numerus> xs) → vacuum {\n xs!.detrahe()\n}\nincipit { }";
    assert_eq!(unused_mut_params(source), 0);
}

#[test]
fn in_param_only_read_through_non_null_still_suggests_ref() {
    let source =
        "functio f(varia lista<numerus> xs) → numerus {\n redde xs!.longitudo()\n}\nincipit { }";
    assert_eq!(unused_mut_params(source), 1);
}
