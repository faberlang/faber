//! Callmark trial: call-site access markers must agree with the signature.
//!
//! A parameter declared `mut` (Latin `in`), `own` or `copy` needs the same
//! marker at the call; a read-only parameter is bare, with `ref` (`de`) as the
//! optional explicit default. Fixtures use latin spellings because the radix
//! unit-test harness compiles bare source without the en-locale lex pack.

use crate::test_support::{analyze_ok, analyze_source, assert_rejects_with_issue, issue};

const DEFS: &str = "functio bump(varia lista<numerus> xs) → vacuum { xs.appende(1) }\n\
functio eat(own lista<numerus> xs) → vacuum { nota xs.longitudo() }\n\
functio dup(copy lista<numerus> xs) → vacuum { nota xs.longitudo() }\n\
functio peek(lista<numerus> xs) → vacuum { nota xs.longitudo() }\n\
functio peek_ref(de lista<numerus> xs) → vacuum { nota xs.longitudo() }\n\
functio make() → lista<numerus> { redde [1, 2] }\n";

fn call(body: &str) -> String {
    format!("{DEFS}incipit {{\n    varia lista<numerus> a ← [1, 2, 3]\n    {body}\n}}")
}

#[test]
fn matching_markers_are_accepted() {
    let _ = analyze_ok(&call("bump(varia a)"));
    let _ = analyze_ok(&call("eat(own a)"));
    let _ = analyze_ok(&call("dup(copy a)"));
}

#[test]
fn read_only_stays_bare_and_ref_is_optional() {
    let _ = analyze_ok(&call("peek(a)"));
    let _ = analyze_ok(&call("peek(de a)"));
    let _ = analyze_ok(&call("peek_ref(a)"));
    let _ = analyze_ok(&call("peek_ref(de a)"));
}

#[test]
fn missing_marker_is_call_marker_missing() {
    assert_rejects_with_issue(&call("bump(a)"), "call_marker_missing");
    assert_rejects_with_issue(&call("eat(a)"), "call_marker_missing");
    assert_rejects_with_issue(&call("dup(a)"), "call_marker_missing");
}

#[test]
fn wrong_marker_is_call_marker_mismatch() {
    assert_rejects_with_issue(&call("bump(own a)"), "call_marker_mismatch");
    assert_rejects_with_issue(&call("eat(copy a)"), "call_marker_mismatch");
    assert_rejects_with_issue(&call("dup(varia a)"), "call_marker_mismatch");
    assert_rejects_with_issue(&call("bump(de a)"), "call_marker_mismatch");
}

#[test]
fn mutating_marker_on_read_only_param_is_rejected() {
    assert_rejects_with_issue(&call("peek(varia a)"), "call_marker_on_read_only");
    assert_rejects_with_issue(&call("peek_ref(own a)"), "call_marker_on_read_only");
    assert_rejects_with_issue(&call("peek(copy a)"), "call_marker_on_read_only");
}

#[test]
fn diagnostic_names_expected_and_found_marker() {
    let result = analyze_source(&call("bump(a)"));
    let err = result
        .diagnostics
        .iter()
        .find(|err| issue(err) == Some("call_marker_missing"))
        .expect("call_marker_missing diagnostic");
    let arg = |name: &str| {
        err.args
            .iter()
            .find(|arg| arg.name == name)
            .map(|arg| arg.value.clone())
    };
    assert_eq!(arg("expected").as_deref(), Some("mut"));
    assert_eq!(arg("found").as_deref(), Some("none"));
}

#[test]
fn temporary_to_mut_param_is_not_writable_whatever_the_marker() {
    // The write would be lost: the writable-place rule (SEM020) rejects it
    // even when the marker agrees.
    assert_rejects_with_issue(&call("bump(varia make())"), "mut_argument_not_writable");
    assert_rejects_with_issue(&call("bump(make())"), "call_marker_missing");
}

#[test]
fn temporary_with_matching_own_or_copy_marker_is_accepted() {
    let _ = analyze_ok(&call("eat(own make())"));
    let _ = analyze_ok(&call("dup(copy make())"));
    assert_rejects_with_issue(&call("eat(make())"), "call_marker_missing");
    assert_rejects_with_issue(&call("dup(own make())"), "call_marker_mismatch");
}

const GENUS: &str = "genus Box {\n    varia numerus n\n    \
functio fill(varia lista<numerus> ys) → vacuum { ys.appende(1) }\n    \
functio take(own lista<numerus> ys) → vacuum { nota ys.longitudo() }\n    \
functio look(lista<numerus> ys) → vacuum { nota ys.longitudo() }\n}\n";

fn method(body: &str) -> String {
    format!(
        "{GENUS}incipit {{\n    varia lista<numerus> a ← [1]\n    varia Box b ← Box {{ n = 1 }}\n    {body}\n}}"
    )
}

#[test]
fn method_arguments_follow_the_signature_modes() {
    let _ = analyze_ok(&method("b.fill(varia a)"));
    let _ = analyze_ok(&method("b.take(own a)"));
    let _ = analyze_ok(&method("b.look(a)"));
    let _ = analyze_ok(&method("b.look(de a)"));
    assert_rejects_with_issue(&method("b.fill(a)"), "call_marker_missing");
    assert_rejects_with_issue(&method("b.take(a)"), "call_marker_missing");
    assert_rejects_with_issue(&method("b.take(varia a)"), "call_marker_mismatch");
    assert_rejects_with_issue(&method("b.look(varia a)"), "call_marker_on_read_only");
}

#[test]
fn method_receiver_carries_no_marker_and_intrinsics_are_exempt() {
    let _ = analyze_ok(&method("a.appende(9)"));
    let _ = analyze_ok(&method("nota a.longitudo()"));
}

const APPLY: &str = "functio apply((varia lista<numerus>) → vacuum f, varia lista<numerus> xs) → vacuum {\n    f(varia xs)\n}\n";

#[test]
fn function_value_calls_use_the_function_type_modes() {
    let _ = analyze_ok(&format!(
        "{DEFS}{APPLY}incipit {{\n    varia lista<numerus> a ← [1]\n    apply(bump, varia a)\n}}"
    ));
    for (call, issue) in [
        ("f(xs)", "call_marker_missing"),
        ("f(own xs)", "call_marker_mismatch"),
    ] {
        let source = format!(
            "{DEFS}functio apply((varia lista<numerus>) → vacuum f, varia lista<numerus> xs) → vacuum {{\n    {call}\n}}\nincipit {{ }}"
        );
        assert_rejects_with_issue(&source, issue);
    }
    // The type declares no mode: any mutating marker is on a read-only slot.
    let source = format!(
        "{DEFS}functio apply((lista<numerus>) → vacuum f, varia lista<numerus> xs) → vacuum {{\n    f(varia xs)\n}}\nincipit {{ }}"
    );
    assert_rejects_with_issue(&source, "call_marker_on_read_only");
    let source = format!(
        "{DEFS}functio apply((lista<numerus>) → vacuum f, lista<numerus> xs) → vacuum {{\n    f(de xs)\n}}\nincipit {{ }}"
    );
    let _ = analyze_ok(&source);
}

#[test]
fn closure_values_use_their_declared_parameter_modes() {
    let ok = "incipit {\n    varia lista<numerus> a ← [1]\n    sit _ g ← (varia lista<numerus> ys) ∴ ys.appende(2)\n    g(varia a)\n}";
    let _ = analyze_ok(ok);
    let missing = "incipit {\n    varia lista<numerus> a ← [1]\n    sit _ g ← (varia lista<numerus> ys) ∴ ys.appende(2)\n    g(a)\n}";
    assert_rejects_with_issue(missing, "call_marker_missing");
}

#[test]
fn function_typed_field_calls_use_the_function_type_modes() {
    let src = |call: &str| {
        format!(
            "{DEFS}genus Hook {{\n    varia (varia lista<numerus>) → vacuum act\n}}\nincipit {{\n    varia lista<numerus> a ← [1]\n    varia Hook h ← Hook {{ act = bump }}\n    {call}\n}}"
        )
    };
    let _ = analyze_ok(&src("h.act(varia a)"));
    assert_rejects_with_issue(&src("h.act(a)"), "call_marker_missing");
}

fn analyze_en(source: &str) -> crate::test_support::AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    crate::test_support::analyze_source_with_config(source, config)
}

fn issues_en(call: &str) -> Vec<String> {
    let source = format!(
        "fn bump(mut list<int> xs) → void {{ xs[0] ← 9 }}\n\
fn eat(own list<int> xs) → void {{ print xs[0] }}\n\
fn dup(copy list<int> xs) → void {{ print xs[0] }}\n\
fn peek(list<int> xs) → void {{ print xs[0] }}\n\
main {{\n    mut list<int> a ← [1, 2]\n    {call}\n}}\n"
    );
    analyze_en(&source)
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .filter_map(|d| issue(d).map(str::to_owned))
        .collect()
}

#[test]
fn english_spellings_ref_mut_own_copy() {
    for ok in [
        "bump(mut a)",
        "eat(own a)",
        "dup(copy a)",
        "peek(a)",
        "peek(ref a)",
    ] {
        assert!(issues_en(ok).is_empty(), "{ok}: {:?}", issues_en(ok));
    }
    assert_eq!(issues_en("bump(a)"), ["call_marker_missing"]);
    assert_eq!(issues_en("eat(copy a)"), ["call_marker_mismatch"]);
    assert_eq!(issues_en("peek(mut a)"), ["call_marker_on_read_only"]);
}
