//! In-place mutating intrinsics against the binding modifier (tcs1).
//!
//! A mutating intrinsic (`is_mutating_receiver` in the registry) rewrites its
//! receiver's contents in place. On a direct path to an immutable (`fixum`)
//! binding it is rejected like a reassignment (SEM020,
//! `imperativus_operator_method_requires_varia_receiver`); WARN013 agrees, so
//! a `varia` that only feeds mutating intrinsics stays necessary. A field or
//! element reached through a `fixum` local is rejected too (`const` is deep,
//! need: SEM020 `mutating_receiver_through_const`); a `varia`
//! object's field contents stay mutable.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::test_support;
use crate::{SemanticErrorKind, WarningKind};

const ISSUE: &str = "imperativus_operator_method_requires_varia_receiver";

fn rejects_with_mutating_receiver_issue(source: &str) {
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ImmutableAssignment)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM020 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

fn warns_unnecessary_varia(source: &str) -> bool {
    let result = test_support::analyze_source(source);
    result.diagnostics.iter().any(|diag| {
        matches!(
            diag.kind,
            SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria)
        )
    })
}

#[test]
fn fixum_list_ordina_is_rejected() {
    rejects_with_mutating_receiver_issue(
        "incipit {\n    fixum lista<numerus> xs ← [3, 1, 2]\n    xs.ordina()\n}",
    );
}

#[test]
fn fixum_list_appende_is_rejected() {
    rejects_with_mutating_receiver_issue(
        "incipit {\n    fixum lista<numerus> xs ← [3, 1, 2]\n    xs.appende(4)\n}",
    );
}

#[test]
fn fixum_numerus_adde_still_rejected() {
    rejects_with_mutating_receiver_issue(
        "incipit {\n    fixum numerus n ← 1\n    n.adde(2)\n    nota n\n}",
    );
}

#[test]
fn varia_list_only_sorted_still_needs_varia() {
    let source =
        "incipit {\n    varia lista<numerus> ys ← [3, 1, 2]\n    ys.ordina()\n    nota ys\n}";
    let result = test_support::analyze_source(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    assert!(
        !warns_unnecessary_varia(source),
        "in-place ordina is a mutation; WARN013 must stay silent: {:?}",
        result.diagnostics
    );
}

#[test]
fn varia_list_only_appended_still_needs_varia() {
    let source =
        "incipit {\n    varia lista<numerus> ys ← [3, 1, 2]\n    ys.appende(4)\n    nota ys\n}";
    let result = test_support::analyze_source(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    assert!(
        !warns_unnecessary_varia(source),
        "in-place appende is a mutation; WARN013 must stay silent: {:?}",
        result.diagnostics
    );
}

#[test]
fn varia_list_never_mutated_still_warns() {
    let source = "incipit {\n    varia lista<numerus> zs ← [3, 1, 2]\n    nota zs\n}";
    assert!(warns_unnecessary_varia(source));
}

#[test]
fn fixum_list_value_returning_twin_stays_accepted() {
    // `addita` is the Perfectum twin (AppendImmutable): it yields a new list
    // and leaves the receiver alone, so a `fixum` receiver stays legal.
    test_support::assert_accepts(
        "incipit {\n    fixum lista<numerus> xs ← [3, 1, 2]\n    nota xs.addita(4)\n}",
    );
}

#[test]
fn fixum_field_list_contents_stay_mutable() {
    // A `varia` object's field contents stay mutable. (A `fixum` object's do
    // not: see `const_object_field_list_append_is_rejected`.)
    test_support::assert_accepts(
        "genus Punctum {\n    fixum lista<numerus> xs\n}\n\n\
         incipit {\n    varia Punctum p ← Punctum { xs = [] }\n    p.xs.appende(3)\n    nota p.xs\n}",
    );
}

const CONST_ISSUE: &str = "mutating_receiver_through_const";

fn rejects_through_const(source: &str) {
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ImmutableAssignment)
            && test_support::issue(err) == Some(CONST_ISSUE)),
        "expected SEM020 {CONST_ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn const_object_field_list_append_is_rejected() {
    // `const` is deep: a field of a const object is not
    // reachable for in-place mutation.
    rejects_through_const(
        "genus Punctum {\n    fixum lista<numerus> xs\n}\n\n\
         incipit {\n    fixum Punctum p ← Punctum { xs = [] }\n    p.xs.appende(3)\n}",
    );
}

#[test]
fn const_object_nested_field_list_append_is_rejected() {
    rejects_through_const(
        "genus Punctum {\n    fixum lista<numerus> xs\n}\n\n\
         genus Cista {\n    fixum Punctum p\n}\n\n\
         incipit {\n    fixum Cista c ← Cista { p = Punctum { xs = [] } }\n    c.p.xs.appende(3)\n}",
    );
}

#[test]
fn const_list_direct_append_keeps_its_own_issue() {
    rejects_with_mutating_receiver_issue(
        "incipit {\n    fixum lista<numerus> xs ← [3, 1, 2]\n    xs.appende(4)\n}",
    );
}

#[test]
fn const_object_field_list_read_stays_accepted() {
    test_support::assert_accepts(
        "genus Punctum {\n    fixum lista<numerus> xs\n}\n\n\
         incipit {\n    fixum Punctum p ← Punctum { xs = [] }\n    nota p.xs.longitudo()\n}",
    );
}

#[test]
fn ref_parameter_mutating_receiver_reports_only_the_ref_issue() {
    // A-200: the ref-view gate used to fall through to the binding-modifier
    // gate and report one fault twice (SEM056 then SEM020).
    let result = test_support::analyze_source(
        "functio f(de lista<numerus> xs) → vacuum {\n    xs.appende(1)\n}\nincipit { }",
    );
    let issues: Vec<&str> = result
        .diagnostics
        .iter()
        .filter(|err| err.is_error())
        .filter_map(test_support::issue)
        .collect();
    assert_eq!(
        issues
            .iter()
            .filter(|issue| **issue == "mutating_receiver_through_ref")
            .count(),
        1,
        "expected one mutating_receiver_through_ref, got {issues:?}"
    );
    assert!(
        !issues.contains(&ISSUE),
        "ref gate must not fall through to {ISSUE}, got {issues:?}"
    );
}
