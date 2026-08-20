//! MU-4 `mut` exclusive-borrow pins.
//!
//! Mutable-borrow-conflict family as mode-scoped language rules, plus the
//! OQ-3 default (`mut` locals parse; a WARN discourages). Unmarked code stays
//! on the MU-1 baseline tests.

use crate::test_support::{
    analyze_ok, analyze_source, analyze_source_with_config, assert_rejects_with_issue, issue,
};
use crate::{PassConfig, ISSUE_MUT_LOCAL};

fn assert_language_rejects(source: &str, expected: &str) {
    let result = analyze_source_with_config(source, PassConfig::base());
    assert!(
        !result.is_ok(),
        "language analysis expected {expected:?}, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && issue(err) == Some(expected)),
        "language analysis expected issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

fn assert_has_issue(source: &str, expected: &str) {
    let result = analyze_source(source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| issue(err) == Some(expected)),
        "expected issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn mut_local_parses_with_discouraged_warning() {
    let result = analyze_ok(
        r"
incipit {
    varia in lista<numerus> xs ← [1, 2, 3]
    nota xs.longitudo()
}
",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| !err.is_error() && issue(err) == Some(ISSUE_MUT_LOCAL)),
        "OQ-3: mut locals warn, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn mut_param_does_not_warn_as_a_local() {
    let result = analyze_ok(
        r"
functio poke(in lista<numerus> xs) → vacuum {
    xs.appende(4)
}
incipit {
    fixum lista<numerus> a ← [1, 2, 3]
    poke(a)
}
",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|err| issue(err) != Some(ISSUE_MUT_LOCAL)),
        "mut parameters are not discouraged locals, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn mut_local_then_use_source_is_sem052() {
    assert_rejects_with_issue(
        r"
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    varia in lista<numerus> b ← a
    nota a.longitudo()
    nota b.longitudo()
}
",
        "use_while_mutably_borrowed",
    );
    assert_language_rejects(
        r"
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    varia in lista<numerus> b ← a
    nota a.longitudo()
    nota b.longitudo()
}
",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn two_mut_locals_from_one_source_conflict() {
    assert_rejects_with_issue(
        r"
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    varia in lista<numerus> b ← a
    varia in lista<numerus> c ← a
    nota b.longitudo()
    nota c.longitudo()
}
",
        "mutable_borrow_conflict",
    );
    assert_language_rejects(
        r"
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    varia in lista<numerus> b ← a
    varia in lista<numerus> c ← a
    nota b.longitudo()
    nota c.longitudo()
}
",
        "mutable_borrow_conflict",
    );
}

#[test]
fn mut_local_then_write_source_is_sem052() {
    assert_rejects_with_issue(
        r"
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    varia in lista<numerus> b ← a
    a.appende(4)
    nota b.longitudo()
}
",
        "write_while_borrowed",
    );
}

#[test]
fn ref_local_then_mut_local_conflicts() {
    assert_rejects_with_issue(
        r"
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    fixum de lista<numerus> r ← a
    varia in lista<numerus> b ← a
    nota r.longitudo()
    nota b.longitudo()
}
",
        "mutable_borrow_conflict",
    );
}

#[test]
fn overlapping_mut_args_conflict() {
    assert_rejects_with_issue(
        r"
functio swap(in lista<numerus> a, in lista<numerus> b) → vacuum {
    a.appende(1)
    b.appende(2)
}
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    swap(xs, xs)
}
",
        "mutable_borrow_conflict",
    );
    assert_language_rejects(
        r"
functio swap(in lista<numerus> a, in lista<numerus> b) → vacuum {
    a.appende(1)
    b.appende(2)
}
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    swap(xs, xs)
}
",
        "mutable_borrow_conflict",
    );
}

#[test]
fn sequential_mut_param_use_stays_legal() {
    analyze_ok(
        r"
functio poke(in lista<numerus> xs) → vacuum {
    xs.appende(4)
}
incipit {
    fixum lista<numerus> a ← [1, 2, 3]
    poke(a)
    nota a.longitudo()
}
",
    );
}

#[test]
fn unmarked_alias_is_not_an_exclusive_borrow() {
    analyze_ok(
        r"
incipit {
    fixum lista<numerus> a ← [1, 2, 3]
    fixum lista<numerus> b ← a
    b.appende(4)
    nota a.longitudo()
}
",
    );
}

#[test]
fn mut_local_warning_is_warn019() {
    assert_has_issue(
        r"
incipit {
    varia in lista<numerus> xs ← [1, 2, 3]
    nota xs.longitudo()
}
",
        ISSUE_MUT_LOCAL,
    );
    let result = analyze_source(
        r"
incipit {
    varia in lista<numerus> xs ← [1, 2, 3]
    nota xs.longitudo()
}
",
    );
    let warning = result
        .diagnostics
        .iter()
        .find(|err| issue(err) == Some(ISSUE_MUT_LOCAL))
        .expect("mut_local warning");
    assert_eq!(crate::semantic_spec(warning.kind).code, "WARN019");
}
