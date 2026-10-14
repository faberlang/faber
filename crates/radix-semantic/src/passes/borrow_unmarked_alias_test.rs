//! G20: an unmarked parameter is shared read-only for every aggregate type, so
//! an unmarked `←` alias of it (directly or through another alias) cannot be
//! written either. Each negative fixture has a positive twin that differs only
//! by `mut` on the parameter.
//!
//! The `_via_reassignment` twins declare the alias from a fresh value and then
//! reassign it from the parameter (`var ys ← []` then `ys ← xs`). SEM065 only
//! guards a `var` declaration, so this route reaches the borrow pass and
//! exercises its alias tracking; the direct `var ys ← xs` fixtures stop at
//! typecheck with SEM065 before the borrow pass runs.

use crate::SemanticErrorKind;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

const SECOND: &str = "immutable_second_assignment";
const METHOD: &str = "imperativus_operator_method_requires_varia_receiver";
const SHARE: &str = "const_shared_into_var";

fn assert_rejects_share(source: &str) {
    let result = analyze_en(source);
    assert!(
        result.diagnostics.iter().any(|d| d.is_error()
            && d.kind == SemanticErrorKind::ConstSharedIntoVar
            && issue(d) == Some(SHARE)),
        "expected SEM065 {SHARE}, got: {:?}",
        result.diagnostics
    );
}

fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

fn assert_accepts(source: &str) {
    let result = analyze_en(source);
    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "expected no error, got: {:?}",
        result.diagnostics
    );
}

fn assert_rejects(source: &str, expected: &str) {
    let result = analyze_en(source);
    assert!(
        result.diagnostics.iter().any(|d| d.is_error()
            && d.kind == SemanticErrorKind::ImmutableAssignment
            && issue(d) == Some(expected)),
        "expected SEM020 {expected}, got: {:?}",
        result.diagnostics
    );
}

const V3: &str = "class V3 {\n    var f64 x = 0.0\n    fn bump() → void {\n        self.x ← self.x + 1.0\n    }\n}\n";

fn list_fn(mode: &str, body: &str) -> String {
    format!(
        "fn f({mode}list<int> xs) → void {{\n{body}\n}}\nmain {{\n    var list<int> a ← [1, 2]\n    f({mode}a)\n}}\n"
    )
}

fn obj_fn(mode: &str, body: &str) -> String {
    format!(
        "{V3}fn f({mode}V3 v) → void {{\n{body}\n}}\nmain {{\n    var V3 a ← V3 {{ x = 1.0 }}\n    f({mode}a)\n}}\n"
    )
}

#[test]
fn list_alias_index_store_is_rejected() {
    let body = "    var list<int> ys ← xs\n    ys[0] ← 9";
    assert_rejects_share(&list_fn("", body));
    assert_accepts(&list_fn("mut ", body));
}

#[test]
fn list_alias_of_alias_index_store_is_rejected() {
    let body = "    var list<int> ys ← xs\n    var list<int> zs ← ys\n    zs[0] ← 9";
    assert_rejects_share(&list_fn("", body));
    assert_accepts(&list_fn("mut ", body));
}

#[test]
fn list_alias_mutating_method_is_rejected() {
    for call in ["append(3)", "reverse()"] {
        let body = format!("    var list<int> ys ← xs\n    ys.{call}");
        assert_rejects_share(&list_fn("", &body));
        assert_accepts(&list_fn("mut ", &body));
    }
}

#[test]
fn passing_a_list_alias_to_a_mut_param_is_rejected() {
    let helper = "fn g(mut list<int> zs) → void {\n    zs[0] ← 9\n}\n";
    let src = |mode: &str, arg: &str| {
        format!(
            "{helper}fn f({mode}list<int> xs) → void {{\n    var list<int> ys ← xs\n    g({arg})\n}}\nmain {{\n    var list<int> a ← [1, 2]\n    f({mode}a)\n}}\n"
        )
    };
    assert_rejects_share(&src("", "mut ys"));
    assert_accepts(&src("mut ", "mut ys"));
}

#[test]
fn passing_an_unmarked_list_param_to_a_mut_param_is_rejected() {
    let src = |mode: &str| {
        format!(
            "fn g(mut list<int> zs) → void {{\n    zs[0] ← 9\n}}\nfn f({mode}list<int> xs) → void {{\n    g(mut xs)\n}}\nmain {{\n    var list<int> a ← [1, 2]\n    f({mode}a)\n}}\n"
        )
    };
    assert_rejects(&src(""), SECOND);
    assert_accepts(&src("mut "));
}

#[test]
fn map_alias_index_store_is_rejected() {
    let src = |mode: &str| {
        format!(
            "fn f({mode}map<string, int> m) → void {{\n    var map<string, int> n ← m\n    n[\"a\"] ← 9\n}}\nmain {{\n    var map<string, int> a ← {{ \"a\": 1 }}\n    f({mode}a)\n}}\n"
        )
    };
    assert_rejects_share(&src(""));
    assert_accepts(&src("mut "));
}

#[test]
fn object_alias_field_store_is_rejected() {
    let body = "    var V3 w ← v\n    w.x ← 5.0";
    assert_rejects_share(&obj_fn("", body));
    assert_accepts(&obj_fn("mut ", body));
}

#[test]
fn object_alias_of_alias_field_store_is_rejected() {
    let body = "    var V3 w ← v\n    var V3 u ← w\n    u.x ← 5.0";
    assert_rejects_share(&obj_fn("", body));
    assert_accepts(&obj_fn("mut ", body));
}

#[test]
fn object_alias_mutating_method_is_rejected() {
    let body = "    var V3 w ← v\n    w.bump()";
    assert_rejects_share(&obj_fn("", body));
    assert_accepts(&obj_fn("mut ", body));
}

#[test]
fn object_field_alias_store_is_rejected() {
    let src = |mode: &str| {
        format!(
            "class Inner {{\n    var int n = 0\n}}\nclass Outer {{\n    var Inner inner = Inner {{}}\n}}\nfn f({mode}Outer o) → void {{\n    var Inner i ← o.inner\n    i.n ← 3\n}}\nmain {{\n    var Outer a ← Outer {{}}\n    f({mode}a)\n}}\n"
        )
    };
    assert_rejects_share(&src(""));
    assert_accepts(&src("mut "));
}

#[test]
fn reading_through_an_alias_is_accepted() {
    let body = "    const list<int> ys ← xs\n    print ys[0]";
    assert_accepts(&list_fn("", body));
}

#[test]
fn list_alias_index_store_is_rejected_via_reassignment() {
    let body = "    var list<int> ys ← []\n    ys ← xs\n    ys[0] ← 9";
    assert_rejects(&list_fn("", body), SECOND);
    assert_accepts(&list_fn("mut ", body));
}

#[test]
fn list_alias_of_alias_index_store_is_rejected_via_reassignment() {
    let body = "    var list<int> ys ← []\n    ys ← xs\n    var list<int> zs ← ys\n    zs[0] ← 9";
    assert_rejects(&list_fn("", body), SECOND);
    assert_accepts(&list_fn("mut ", body));
}

#[test]
fn list_alias_mutating_method_is_rejected_via_reassignment() {
    for call in ["append(3)", "reverse()"] {
        let body = format!("    var list<int> ys ← []\n    ys ← xs\n    ys.{call}");
        assert_rejects(&list_fn("", &body), METHOD);
        assert_accepts(&list_fn("mut ", &body));
    }
}

#[test]
fn passing_a_list_alias_to_a_mut_param_is_rejected_via_reassignment() {
    let helper = "fn g(mut list<int> zs) → void {\n    zs[0] ← 9\n}\n";
    let src = |mode: &str, arg: &str| {
        format!(
            "{helper}fn f({mode}list<int> xs) → void {{\n    var list<int> ys ← []\n    ys ← xs\n    g({arg})\n}}\nmain {{\n    var list<int> a ← [1, 2]\n    f({mode}a)\n}}\n"
        )
    };
    assert_rejects(&src("", "mut ys"), SECOND);
    assert_accepts(&src("mut ", "mut ys"));
}

#[test]
fn map_alias_index_store_is_rejected_via_reassignment() {
    let src = |mode: &str| {
        format!(
            "fn f({mode}map<string, int> m) → void {{\n    var map<string, int> n ← {{}}\n    n ← m\n    n[\"a\"] ← 9\n}}\nmain {{\n    var map<string, int> a ← {{ \"a\": 1 }}\n    f({mode}a)\n}}\n"
        )
    };
    assert_rejects(&src(""), SECOND);
    assert_accepts(&src("mut "));
}

#[test]
fn object_alias_field_store_is_rejected_via_reassignment() {
    let body = "    var V3 w ← V3 { x = 0.0 }\n    w ← v\n    w.x ← 5.0";
    assert_rejects(&obj_fn("", body), SECOND);
    assert_accepts(&obj_fn("mut ", body));
}

#[test]
fn object_alias_of_alias_field_store_is_rejected_via_reassignment() {
    let body = "    var V3 w ← V3 { x = 0.0 }\n    w ← v\n    var V3 u ← w\n    u.x ← 5.0";
    assert_rejects(&obj_fn("", body), SECOND);
    assert_accepts(&obj_fn("mut ", body));
}

#[test]
fn object_alias_mutating_method_is_rejected_via_reassignment() {
    let body = "    var V3 w ← V3 { x = 0.0 }\n    w ← v\n    w.bump()";
    assert_rejects(&obj_fn("", body), METHOD);
    assert_accepts(&obj_fn("mut ", body));
}

#[test]
fn object_field_alias_store_is_rejected_via_reassignment() {
    let src = |mode: &str| {
        format!(
            "class Inner {{\n    var int n = 0\n}}\nclass Outer {{\n    var Inner inner = Inner {{}}\n}}\nfn f({mode}Outer o) → void {{\n    var Inner i ← Inner {{}}\n    i ← o.inner\n    i.n ← 3\n}}\nmain {{\n    var Outer a ← Outer {{}}\n    f({mode}a)\n}}\n"
        )
    };
    assert_rejects(&src(""), SECOND);
    assert_accepts(&src("mut "));
}
