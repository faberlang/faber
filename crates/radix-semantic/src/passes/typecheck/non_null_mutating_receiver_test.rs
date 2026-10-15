//! audit1-a: the tcs1 immutable-binding gate sees through `value!.method()`.
//!
//! For an optional binding the direct call is `optional_receiver_requires_chain`
//! and the non-null form `xs!.appende(…)` is the mandatory spelling, so the
//! gate must treat the non-null member boundary as the receiver place
//! (SEM020 on an immutable binding), and WARN013 must agree for a `varia`
//! that only feeds such calls. SEM020 names the binding and the intrinsic.
//! (A `ref` optional root cannot be authored: an access marker on `T ∪ nihil`
//! binds to the first member and is rejected as a nested marker.)
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::test_support;
use crate::{SemanticErrorKind, WarningKind};

const ISSUE: &str = "imperativus_operator_method_requires_varia_receiver";
const PRELUDE: &str = "functio forte() → lista<numerus> ∪ nihil { redde [1, 2] }\n";

fn arg<'a>(err: &'a crate::SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

#[test]
fn fixum_optional_list_non_null_appende_is_rejected() {
    let source = format!(
        "{PRELUDE}incipit {{\n    sit lista<numerus> ∪ nihil xs ← forte()\n    xs!.appende(1)\n}}"
    );
    let result = test_support::analyze_source(&source);
    let err = result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && test_support::issue(err) == Some(ISSUE))
        .unwrap_or_else(|| panic!("expected SEM020 {ISSUE}, got {:?}", result.diagnostics));
    assert!(matches!(err.kind, SemanticErrorKind::ImmutableAssignment));
}

#[test]
fn fixum_optional_list_non_null_detrahe_is_rejected() {
    // Value-returning in-place mutator: the registry predicate, not the
    // return type, decides.
    let source = format!(
        "{PRELUDE}incipit {{\n    sit lista<numerus> ∪ nihil xs ← forte()\n    xs!.detrahe()\n}}"
    );
    test_support::assert_rejects_with_issue(&source, ISSUE);
}

#[test]
fn varia_optional_list_non_null_appende_is_accepted() {
    let source = format!(
        "{PRELUDE}incipit {{\n    varia lista<numerus> ∪ nihil xs ← forte()\n    xs!.appende(1)\n    nota xs\n}}"
    );
    let result = test_support::analyze_source(&source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    assert!(
        !result.diagnostics.iter().any(|diag| matches!(
            diag.kind,
            SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria)
        )),
        "in-place appende through `!` is a mutation: {:?}",
        result.diagnostics
    );
}

#[test]
fn fixum_optional_list_non_null_read_is_accepted() {
    let source = format!(
        "{PRELUDE}incipit {{\n    sit lista<numerus> ∪ nihil xs ← forte()\n    nota xs!.longitudo()\n}}"
    );
    test_support::assert_accepts(&source);
}

#[test]
fn non_null_field_receiver_keeps_shallow_behavior() {
    // `p!.xs.appende(3)`: the non-null boundary is a field access on `p`, not
    // the method callee; the receiver is a field place (shallow D-1).
    test_support::assert_accepts(
        "genus Punctum {\n    fixum lista<numerus> xs\n}\n\
         functio da() → Punctum ∪ nihil { redde Punctum { xs = [] } }\n\
         incipit {\n    sit Punctum ∪ nihil p ← da()\n    p!.xs.appende(3)\n}",
    );
}

#[test]
fn sem020_names_binding_and_method_on_direct_receiver() {
    let result = test_support::analyze_source(
        "incipit {\n    sit lista<numerus> xs ← [3, 1, 2]\n    xs.appende(4)\n}",
    );
    let err = result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && test_support::issue(err) == Some(ISSUE))
        .unwrap_or_else(|| panic!("expected SEM020 {ISSUE}, got {:?}", result.diagnostics));
    assert_eq!(arg(err, "binding"), Some("xs"));
    assert_eq!(arg(err, "method"), Some("appende"));
}

#[test]
fn sem020_names_binding_and_method_through_non_null() {
    let source = format!(
        "{PRELUDE}incipit {{\n    sit lista<numerus> ∪ nihil ys ← forte()\n    ys!.appende(1)\n}}"
    );
    let result = test_support::analyze_source(&source);
    let err = result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && test_support::issue(err) == Some(ISSUE))
        .unwrap_or_else(|| panic!("expected SEM020 {ISSUE}, got {:?}", result.diagnostics));
    assert_eq!(arg(err, "binding"), Some("ys"));
    assert_eq!(arg(err, "method"), Some("appende"));
}
