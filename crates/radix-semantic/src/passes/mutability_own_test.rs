//! MU-3 `own` + move pins.
//!
//! Bind-site transfers, source-dead SEM050, and the D-6 own-bind rejection
//! enumeration. Unmarked code stays on the MU-1 baseline tests.

use crate::test_support::{analyze_ok, analyze_source, assert_rejects_with_issue, issue};
use crate::{
    ISSUE_FIELD_INTO_OWN, ISSUE_INDEX_INTO_OWN, ISSUE_MUT_INTO_OWN, ISSUE_REF_INTO_OWN,
    ISSUE_SHARED_INTO_OWN, ISSUE_UNMARKED_INTO_OWN, SUGGESTION_COPY,
};

fn assert_own_bind_rejects(source: &str, expected: &str) {
    let result = analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected own-bind rejection {expected:?}, got: {:?}",
        result.diagnostics
    );
    let err = result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && issue(err) == Some(expected))
        .unwrap_or_else(|| panic!("expected issue {expected:?}, got: {:?}", result.diagnostics));
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "suggestion" && arg.value == SUGGESTION_COPY),
        "illegal own-bind must suggest `copy`, got: {:?}",
        err.args
    );
}

#[test]
fn own_fresh_literal_is_legal() {
    let _ = analyze_ok(
        r"
incipit {
    fixum lista<numerus> xs ← own [1, 2, 3]
    nota xs.longitudo()
}
",
    );
}

#[test]
fn own_transfer_without_later_use_is_legal() {
    let _ = analyze_ok(
        r"
incipit {
    fixum lista<numerus> a ← own [1, 2, 3]
    fixum lista<numerus> b ← own a
    nota b.longitudo()
}
",
    );
}

#[test]
fn own_transfer_then_use_is_sem050() {
    assert_rejects_with_issue(
        r"
incipit {
    fixum lista<numerus> a ← own [1, 2, 3]
    fixum lista<numerus> b ← own a
    nota a.longitudo()
    nota b.longitudo()
}
",
        "use_after_move",
    );
}

#[test]
fn own_param_call_then_use_is_sem050() {
    assert_rejects_with_issue(
        r"
functio take(own lista<numerus> xs) → vacuum {
    nota xs.longitudo()
}
incipit {
    fixum lista<numerus> a ← own [1, 2, 3]
    take(a)
    nota a.longitudo()
}
",
        "use_after_move",
    );
}

#[test]
fn unmarked_dead_into_own_is_legal() {
    let _ = analyze_ok(
        r"
incipit {
    fixum lista<numerus> a ← [1, 2, 3]
    fixum lista<numerus> b ← own a
    nota b.longitudo()
}
",
    );
}

#[test]
fn unmarked_live_into_own_rejects_with_copy_suggestion() {
    assert_own_bind_rejects(
        r"
incipit {
    fixum lista<numerus> a ← [1, 2, 3]
    fixum lista<numerus> b ← own a
    nota a.longitudo()
    nota b.longitudo()
}
",
        ISSUE_UNMARKED_INTO_OWN,
    );
}

#[test]
fn ref_into_own_rejects_with_copy_suggestion() {
    assert_own_bind_rejects(
        r"
incipit {
    fixum de lista<numerus> a ← [1, 2, 3]
    fixum lista<numerus> b ← own a
}
",
        ISSUE_REF_INTO_OWN,
    );
}

#[test]
fn mut_into_own_rejects_with_copy_suggestion() {
    assert_own_bind_rejects(
        r"
incipit {
    varia in lista<numerus> a ← [1, 2, 3]
    fixum lista<numerus> b ← own a
}
",
        ISSUE_MUT_INTO_OWN,
    );
}

#[test]
fn mut_view_into_own_param_rejects() {
    assert_rejects_with_issue(
        r"
functio take(own lista<numerus> xs) → vacuum {
    nota xs.longitudo()
}
incipit {
    fixum lista<numerus> a ← [1, 2, 3]
    varia in lista<numerus> b ← a
    take(b)
}
",
        ISSUE_MUT_INTO_OWN,
    );
}

#[test]
fn field_into_own_rejects_with_copy_suggestion() {
    assert_own_bind_rejects(
        r"
genus Persona {
    varia lista<numerus> amici
}
incipit {
    fixum Persona p ← Persona { amici = [1, 2] }
    fixum lista<numerus> b ← own p.amici
}
",
        ISSUE_FIELD_INTO_OWN,
    );
}

#[test]
fn index_into_own_rejects_with_copy_suggestion() {
    assert_own_bind_rejects(
        r"
incipit {
    fixum lista<lista<numerus>> xs ← [[1], [2]]
    fixum lista<numerus> b ← own xs[0]
}
",
        ISSUE_INDEX_INTO_OWN,
    );
}

#[test]
fn shared_branch_into_own_rejects_with_copy_suggestion() {
    assert_own_bind_rejects(
        r"
incipit {
    fixum lista<numerus> a ← [1]
    fixum lista<numerus> b ← [2]
    fixum lista<numerus> c ← own verum ✓ a ✗ b
}
",
        ISSUE_SHARED_INTO_OWN,
    );
}

#[test]
fn copy_source_transfers_like_own() {
    let _ = analyze_ok(
        r"
incipit {
    fixum lista<numerus> a ← copy [1, 2, 3]
    fixum lista<numerus> b ← own a
    nota b.longitudo()
}
",
    );
}

#[test]
fn copy_source_then_use_is_sem050() {
    assert_rejects_with_issue(
        r"
incipit {
    fixum lista<numerus> a ← copy [1, 2, 3]
    fixum lista<numerus> b ← own a
    nota a.longitudo()
}
",
        "use_after_move",
    );
}

#[test]
fn unmarked_alias_on_bind_stays_legal() {
    let _ = analyze_ok(
        r"
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    varia lista<numerus> b ← a
    b.appende(4)
    nota a.longitudo()
}
",
    );
}
