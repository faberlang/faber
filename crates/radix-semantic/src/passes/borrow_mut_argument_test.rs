//! An argument passed to a `mut` parameter must be a writable place: a `var`
//! binding, a `mut` parameter of the caller, or a field/element path rooted at
//! one. A `const` binding, an unmarked parameter, a temporary and a field of a
//! `const` object are rejected, for every type.

use crate::SemanticErrorKind;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

const NOT_WRITABLE: &str = "mut_argument_not_writable";

const DECLS: &str = "class Counter {\n    mut int n = 0\n}\nclass Holder {\n    mut Counter c = Counter {}\n    mut list<int> xs = []\n}\nfn bl(mut list<int> xs) → void {\n    xs[0] ← 9\n}\nfn bt(mut tf32[2] t) → void {\n    t[0] ← 9.0\n}\nfn bc(mut Counter c) → void {\n    c.n ← 9\n}\nfn bi(mut int x) → void {\n    x ← 9\n}\n";

fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

fn program(fns: &str, main: &str) -> String {
    format!("{DECLS}{fns}\nmain {{\n{main}\n}}\n")
}

fn assert_accepts(source: &str) {
    let result = analyze_en(source);
    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "expected no error, got: {:?}",
        result.diagnostics
    );
}

fn assert_rejects(source: &str) {
    let result = analyze_en(source);
    assert!(
        result.diagnostics.iter().any(|d| d.is_error()
            && d.kind == SemanticErrorKind::ImmutableAssignment
            && issue(d) == Some(NOT_WRITABLE)),
        "expected SEM020 {NOT_WRITABLE}, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn const_list_into_mut_param_is_rejected() {
    assert_rejects(&program(
        "",
        "    let list<int> a ← [1, 2]\n    bl(mut a)\n    print a",
    ));
    assert_accepts(&program(
        "",
        "    mut list<int> a ← [1, 2]\n    bl(mut a)\n    print a",
    ));
}

#[test]
fn const_tensor_into_mut_param_is_rejected() {
    assert_rejects(&program(
        "",
        "    let tf32[2] a ← tf32[2] {}\n    bt(mut a)\n    print a[0]",
    ));
    assert_accepts(&program(
        "",
        "    mut tf32[2] a ← tf32[2] {}\n    bt(mut a)\n    print a[0]",
    ));
}

#[test]
fn const_object_into_mut_param_is_rejected() {
    assert_rejects(&program(
        "",
        "    let Counter a ← Counter {}\n    bc(mut a)\n    print a.n",
    ));
    assert_accepts(&program(
        "",
        "    mut Counter a ← Counter {}\n    bc(mut a)\n    print a.n",
    ));
}

#[test]
fn const_scalar_into_mut_param_is_rejected() {
    assert_rejects(&program(
        "",
        "    let int a ← 1\n    bi(mut a)\n    print a",
    ));
    assert_accepts(&program(
        "",
        "    mut int a ← 1\n    bi(mut a)\n    print a",
    ));
}

#[test]
fn field_of_const_object_into_mut_param_is_rejected() {
    let main = "    let Holder h ← Holder {}\n    bc(mut h.c)\n    bl(mut h.xs)";
    assert_rejects(&program("", main));
    let main = "    mut Holder h ← Holder {}\n    bc(mut h.c)\n    bl(mut h.xs)";
    assert_accepts(&program("", main));
}

#[test]
fn element_of_const_list_into_mut_param_is_rejected() {
    let main = "    let list<int> a ← [1, 2]\n    bi(mut a[0])";
    assert_rejects(&program("", main));
    let main = "    mut list<int> a ← [1, 2]\n    bi(mut a[0])";
    assert_accepts(&program("", main));
}

#[test]
fn unmarked_param_forwarded_to_mut_param_is_rejected() {
    // A read-only list, object or tensor parameter keeps the alias rules'
    // diagnostics (SEM020 second assignment, SEM056 for tensors); a scalar one
    // gets `mut_argument_not_writable`.
    for (ty, callee, value, kind, expected) in [
        (
            "list<int>",
            "bl",
            "[1, 2]",
            SemanticErrorKind::ImmutableAssignment,
            "immutable_second_assignment",
        ),
        (
            "tf32[2]",
            "bt",
            "tf32[2] {}",
            SemanticErrorKind::AssignToImmutableBorrow,
            "assign_to_nonmut_tensor_param",
        ),
        (
            "Counter",
            "bc",
            "Counter {}",
            SemanticErrorKind::ImmutableAssignment,
            "immutable_second_assignment",
        ),
        (
            "int",
            "bi",
            "1",
            SemanticErrorKind::ImmutableAssignment,
            NOT_WRITABLE,
        ),
    ] {
        let fns = format!("fn fwd({ty} p) → void {{\n    {callee}(mut p)\n}}\n");
        let main = format!("    mut {ty} a ← {value}\n    fwd(a)");
        let result = analyze_en(&program(&fns, &main));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.is_error() && d.kind == kind && issue(d) == Some(expected)),
            "{ty}: expected {expected}, got: {:?}",
            result.diagnostics
        );
        let fns = format!("fn fwd(mut {ty} p) → void {{\n    {callee}(mut p)\n}}\n");
        let main = format!("    mut {ty} a ← {value}\n    fwd(mut a)");
        assert_accepts(&program(&fns, &main));
    }
}

#[test]
fn mut_param_field_and_element_are_writable() {
    let fns = "fn fwd(mut Holder h) → void {\n    bc(mut h.c)\n    bl(mut h.xs)\n}\n";
    assert_accepts(&program(
        fns,
        "    mut Holder h ← Holder {}\n    fwd(mut h)",
    ));
}

#[test]
fn temporary_into_mut_param_is_rejected() {
    assert_rejects(&program("", "    bl(mut [1, 2])"));
    assert_rejects(&program("", "    bi(mut 3)"));
    assert_rejects(&program("", "    bc(mut Counter {})"));
}
