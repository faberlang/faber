//! MU-4 `mut` exclusive-borrow pins.
//!
//! Mutable-borrow-conflict family as mode-scoped language rules, plus the
//! OQ-3 default (`mut` locals parse; a WARN discourages). Unmarked code stays
//! on the MU-1 baseline tests.

use crate::test_support::{
    analyze_ok_la, analyze_source_la, analyze_source_with_config_la, assert_rejects_with_issue_la,
    issue,
};
use crate::{ISSUE_MUT_LOCAL, PassConfig};

fn assert_language_rejects(source: &str, expected: &str) {
    let result = analyze_source_with_config_la(source, PassConfig::base());
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

fn assert_language_accepts(source: &str) {
    let result = analyze_source_with_config_la(source, PassConfig::base());
    assert!(
        result.is_ok(),
        "language analysis expected acceptance, got: {:?}",
        result.diagnostics
    );
}

fn assert_has_issue(source: &str, expected: &str) {
    let result = analyze_source_la(source);
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
    let result = analyze_ok_la(
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
    let result = analyze_ok_la(
        r"
functio poke(in lista<numerus> xs) → vacuum {
    xs.appende(4)
}
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    poke(in a)
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
    assert_rejects_with_issue_la(
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
    assert_rejects_with_issue_la(
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
    assert_rejects_with_issue_la(
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
    assert_rejects_with_issue_la(
        r"
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    sit de lista<numerus> r ← a
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
    assert_rejects_with_issue_la(
        r"
functio swap(in lista<numerus> a, in lista<numerus> b) → vacuum {
    a.appende(1)
    b.appende(2)
}
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    swap(in xs, in xs)
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
    swap(in xs, in xs)
}
",
        "mutable_borrow_conflict",
    );
}

#[test]
fn sequential_mut_param_use_stays_legal() {
    let _ = analyze_ok_la(
        r"
functio poke(in lista<numerus> xs) → vacuum {
    xs.appende(4)
}
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    poke(in a)
    nota a.longitudo()
}
",
    );
}

#[test]
fn unmarked_alias_is_not_an_exclusive_borrow() {
    let _ = analyze_ok_la(
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
    let result = analyze_source_la(
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

const MUT_SCALAR_SWAP: &str = r"
functio swap(in numerus a, in numerus b) → vacuum {
    sit numerus held ← a
    a ← b
    b ← held
}
incipit {
    varia numerus x ← 1
    varia numerus y ← 2
    swap(in x, in y)
    nota x
    nota y
}
";

#[test]
fn scalar_assignment_between_mut_params_retains_no_borrow() {
    let _ = analyze_ok_la(MUT_SCALAR_SWAP);
    assert_language_accepts(MUT_SCALAR_SWAP);
    let source = r"
functio copy_then_set(in numerus a, in numerus b) → vacuum {
    a ← b
    b ← 9
}
incipit {
    varia numerus x ← 1
    varia numerus y ← 2
    copy_then_set(in x, in y)
}
";
    let _ = analyze_ok_la(source);
    assert_language_accepts(source);
}

#[test]
fn three_mut_scalar_param_rotation_retains_no_borrow() {
    let source = r"
functio rotate(in numerus a, in numerus b, in numerus c) → vacuum {
    sit numerus held ← a
    a ← b
    b ← c
    c ← held
}
incipit {
    varia numerus x ← 1
    varia numerus y ← 2
    varia numerus z ← 3
    rotate(in x, in y, in z)
}
";
    let _ = analyze_ok_la(source);
    assert_language_accepts(source);
}

#[test]
fn passing_one_scalar_variable_to_two_mut_params_still_conflicts() {
    let source = r"
functio swap(in numerus a, in numerus b) → vacuum {
    sit numerus held ← a
    a ← b
    b ← held
}
incipit {
    varia numerus x ← 1
    swap(in x, in x)
}
";
    assert_rejects_with_issue_la(source, "mutable_borrow_conflict");
    assert_language_rejects(source, "mutable_borrow_conflict");
}

#[test]
fn borrow_conflict_names_the_place_whose_borrow_is_live() {
    let result = analyze_source_la(
        r"
functio swap(in numerus a, in numerus b) → vacuum {
    a ← b
}
incipit {
    varia numerus x ← 1
    swap(in x, in x)
}
",
    );
    assert!(
        result.diagnostics.iter().any(|err| {
            issue(err) == Some("mutable_borrow_conflict")
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "name" && arg.value == "x")
        }),
        "expected the conflict to name `x`, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn narrower_cell_passed_to_wider_mut_param_is_rejected() {
    let source = r"
functio setit(in numerus x) → vacuum {
    x ← 300
}
incipit {
    varia u8 small ← 7
    setit(in small)
    nota small
}
";
    assert_rejects_with_issue_la(source, "mut_argument_type_mismatch");
}

#[test]
fn exact_width_cell_passed_to_mut_param_is_accepted() {
    let _ = analyze_ok_la(
        r"
functio setit(in u8 x) → vacuum {
    x ← 200
}
incipit {
    varia u8 small ← 7
    setit(in small)
    nota small
}
",
    );
    let _ = analyze_ok_la(
        r"
functio setit(in numerus x) → vacuum {
    x ← 300
}
incipit {
    varia numerus big ← 7
    setit(in big)
    nota big
}
",
    );
}
