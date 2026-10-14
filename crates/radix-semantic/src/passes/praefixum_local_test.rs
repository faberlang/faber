//! Local `praefixum { … }` constants: lowering, captures, type parameters and
//! evaluation order (st1 R6–R9).
//!
//! Contract:
//! - `fixum T x = praefixum { … }` (and the `_` form) is legal as a
//!   block-level constant inside any body; a runtime `←` store stays
//!   SEM064 `praefixum_outside_constant`;
//! - the body stands alone: reading an enclosing parameter or local —
//!   including an earlier compile-time local, or a method's `ego` — is SEM064
//!   `praefixum_captures_local`, a type parameter of the enclosing function or
//!   genus is SEM064 `praefixum_type_parameter`;
//! - module constants and the body's own bindings are free to read;
//! - effects are still forbidden (`praefixum_forbidden`) and a site reached
//!   back through its own readers is SEM007 `constant_cycle`;
//! - ordering: a caller of a function that holds a local site waits for the
//!   site, and a nested site runs before the site that contains it.

use super::praefixum::{PraefixumSiteKind, evaluation_order, sites};
use crate::SemanticError;
use crate::test_support::{analyze_source_la, assert_rejects_with_issue_la, issue};

fn arg<'a>(err: &'a SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn rejection(source: &str, expected: &str) -> SemanticError {
    let result = analyze_source_la(source);
    result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && issue(err) == Some(expected))
        .cloned()
        .unwrap_or_else(|| panic!("expected {expected:?}, got: {:?}", result.diagnostics))
}

fn assert_clean(source: &str) {
    let result = analyze_source_la(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn block_level_constant_takes_a_praefixum_body() {
    assert_clean(
        "incipit {\n    fixum numerus n = praefixum {\n        redde 7\n    }\n    nota n\n}\n",
    );
}

#[test]
fn inferred_slot_takes_its_type_from_the_body() {
    assert_clean("incipit {\n    fixum _ n = praefixum {\n        redde 7\n    }\n    nota n\n}\n");
}

#[test]
fn local_site_works_in_a_function_a_method_and_a_nested_body() {
    assert_clean(
        "functio f() → numerus {\n    fixum numerus a = praefixum {\n        fixum numerus b = praefixum {\n            redde 2\n        }\n        redde b + 1\n    }\n    redde a\n}\n\
         genus Cista {\n    varia numerus n\n    functio ter() → numerus {\n        fixum numerus t = praefixum {\n            redde 3\n        }\n        redde ego.n * t\n    }\n}\n\
         incipit {\n    nota f()\n}\n",
    );
}

#[test]
fn runtime_store_stays_outside_a_constant() {
    assert_rejects_with_issue_la(
        "incipit {\n    fixum numerus n ← praefixum {\n        redde 1\n    }\n}\n",
        "praefixum_outside_constant",
    );
    assert_rejects_with_issue_la(
        "incipit {\n    varia numerus n ← 0\n    n ← praefixum {\n        redde 1\n    }\n}\n",
        "praefixum_outside_constant",
    );
}

#[test]
fn praefixum_as_an_operand_stays_outside_a_constant() {
    assert_rejects_with_issue_la(
        "incipit {\n    fixum numerus n = 1 + praefixum {\n        redde 1\n    }\n}\n",
        "praefixum_outside_constant",
    );
}

#[test]
fn module_constants_and_own_bindings_are_not_captures() {
    assert_clean(
        "fixum numerus BASIS = 5\n\
         functio f() → numerus {\n    fixum numerus s = praefixum {\n        fixum numerus propria = 2\n        redde propria * BASIS\n    }\n    redde s\n}\n\
         incipit {\n    nota f()\n}\n",
    );
}

#[test]
fn reading_a_parameter_is_a_capture() {
    let err = rejection(
        "functio f(numerus p) → numerus {\n    fixum numerus c = praefixum {\n        redde p\n    }\n    redde c\n}\n",
        "praefixum_captures_local",
    );
    assert_eq!(arg(&err, "name"), Some("c"));
    assert_eq!(arg(&err, "captured"), Some("p"));
}

#[test]
fn reading_a_runtime_local_is_a_capture() {
    let err = rejection(
        "incipit {\n    varia numerus v ← 4\n    fixum numerus c = praefixum {\n        redde v\n    }\n    nota c\n}\n",
        "praefixum_captures_local",
    );
    assert_eq!(arg(&err, "captured"), Some("v"));
}

#[test]
fn reading_an_earlier_compile_time_local_is_a_capture() {
    let err = rejection(
        "incipit {\n    fixum numerus a = 3\n    fixum numerus c = praefixum {\n        redde a + 1\n    }\n    nota c\n}\n",
        "praefixum_captures_local",
    );
    assert_eq!(arg(&err, "captured"), Some("a"));
}

#[test]
fn reading_the_receiver_is_a_capture() {
    let err = rejection(
        "genus Cista {\n    varia numerus n\n    functio ter() → numerus {\n        fixum numerus c = praefixum {\n            redde ego.n\n        }\n        redde c\n    }\n}\n",
        "praefixum_captures_local",
    );
    assert_eq!(arg(&err, "name"), Some("c"));
    assert_eq!(arg(&err, "captured"), Some("ego"));
}

#[test]
fn a_method_body_that_ignores_the_receiver_is_accepted() {
    assert_clean(
        "genus Cista {\n    varia numerus n\n    functio ter() → numerus {\n        fixum numerus c = praefixum {\n            redde 3\n        }\n        redde ego.n * c\n    }\n}\n",
    );
}

#[test]
fn a_parameter_is_named_before_the_receiver() {
    let err = rejection(
        "genus Cista {\n    varia numerus n\n    functio ter(numerus p) → numerus {\n        fixum numerus c = praefixum {\n            redde ego.n + p\n        }\n        redde c\n    }\n}\n",
        "praefixum_captures_local",
    );
    assert_eq!(arg(&err, "captured"), Some("p"));
}

#[test]
fn a_nested_site_may_not_read_the_enclosing_body() {
    let err = rejection(
        "incipit {\n    fixum numerus c = praefixum {\n        fixum numerus a = 3\n        fixum numerus d = praefixum {\n            redde a\n        }\n        redde d\n    }\n    nota c\n}\n",
        "praefixum_captures_local",
    );
    assert_eq!(arg(&err, "name"), Some("d"));
    assert_eq!(arg(&err, "captured"), Some("a"));
}

#[test]
fn a_type_parameter_of_a_function_is_rejected() {
    let err = rejection(
        "functio g<T>(T x) → numerus {\n    fixum numerus n = praefixum {\n        varia lista<T> ys ← vacua\n        redde 1\n    }\n    redde n\n}\n",
        "praefixum_type_parameter",
    );
    assert_eq!(arg(&err, "name"), Some("n"));
    assert_eq!(arg(&err, "param"), Some("T"));
}

#[test]
fn a_body_that_ignores_the_type_parameter_is_accepted() {
    assert_clean(
        "functio g<T>(T x) → numerus {\n    fixum numerus n = praefixum {\n        redde 1\n    }\n    redde n\n}\n\
         incipit {\n    nota g(1)\n}\n",
    );
}

#[test]
fn calling_a_generic_function_is_not_a_dependence() {
    assert_clean(
        "functio id<T>(T x) → T {\n    redde x\n}\n\
         functio g<T>(T x) → numerus {\n    fixum numerus n = praefixum {\n        redde id(3)\n    }\n    redde n\n}\n\
         incipit {\n    nota g(1)\n}\n",
    );
}

#[test]
fn a_generic_genus_field_default_may_not_depend_on_its_parameter() {
    let err = rejection(
        "genus Cista<T> {\n    fixum lista<T> cellae = praefixum {\n        varia lista<T> r ← vacua\n        redde r\n    }\n}\n",
        "praefixum_type_parameter",
    );
    assert_eq!(arg(&err, "name"), Some("cellae"));
    assert_eq!(arg(&err, "param"), Some("T"));
}

#[test]
fn a_generic_genus_field_default_without_the_parameter_is_accepted() {
    assert_clean(
        "genus Cista<T> {\n    fixum numerus cap = praefixum {\n        redde 4\n    }\n    fixum T item\n}\n",
    );
}

#[test]
fn a_local_body_may_not_open_ad() {
    let err = rejection(
        "incipit {\n    fixum textus t = praefixum {\n        fixum textus r ← ad 'runtime:echo' (\"salve\") ↦ textus\n        redde r\n    }\n    nota t\n}\n",
        "praefixum_forbidden",
    );
    assert_eq!(arg(&err, "effect"), Some("ad"));
    assert_eq!(arg(&err, "chain"), Some("t"));
}

#[test]
fn local_sites_follow_the_module_sites_and_nested_run_first() {
    let fixture = analyze_source_la(
        "fixum numerus M = praefixum {\n    redde 1\n}\n\
         functio f() → numerus {\n    fixum numerus a = praefixum {\n        fixum numerus b = praefixum {\n            redde 2\n        }\n        redde b + 1\n    }\n    redde a\n}\n\
         incipit {\n    nota f()\n}\n",
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let listed: Vec<(&str, PraefixumSiteKind)> = sites(hir)
        .iter()
        .map(|site| (fixture.interner.resolve(site.name), site.kind))
        .collect();
    assert_eq!(
        listed,
        [
            ("M", PraefixumSiteKind::Constant),
            ("b", PraefixumSiteKind::Local),
            ("a", PraefixumSiteKind::Local),
        ]
    );
}

#[test]
fn a_caller_waits_for_the_local_site_inside_its_callee() {
    let fixture = analyze_source_la(
        "fixum numerus SUMMA = praefixum {\n    redde ima()\n}\n\
         functio ima() → numerus {\n    fixum numerus k = praefixum {\n        redde 4\n    }\n    redde k * 2\n}\n\
         incipit {\n    nota SUMMA\n}\n",
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let order = evaluation_order(hir, &fixture.result.types).expect("no cycle");
    let names: Vec<&str> = order
        .iter()
        .map(|site| fixture.interner.resolve(site.name))
        .collect();
    assert_eq!(names, ["k", "SUMMA"]);
}

#[test]
fn a_local_site_that_reads_its_own_caller_is_a_cycle() {
    assert_rejects_with_issue_la(
        "fixum numerus SUMMA = praefixum {\n    redde ima()\n}\n\
         functio ima() → numerus {\n    fixum numerus k = praefixum {\n        redde SUMMA\n    }\n    redde k\n}\n",
        "constant_cycle",
    );
}
