//! Coverage for `praefixum { … }` static initializers (DECISIONS D8.4–D8.6).
//!
//! Contract:
//! - `praefixum` is legal only as a `generis` initializer; elsewhere it is
//!   SEM064 `praefixum_outside_static`;
//! - its body type-checks like a function body returning the declared type;
//! - a body that reaches `ad`, await, async, `externa`, input, or output —
//!   directly or through Faber callees — is SEM064 `praefixum_forbidden`,
//!   naming the effect and the call chain;
//! - a static that reads itself through a `praefixum` body is SEM007
//!   `static_cycle`;
//! - evaluation order puts every static a body reads before it.

use super::praefixum::evaluation_order;
use crate::SemanticError;
use crate::test_support::{analyze_source, assert_rejects_with_issue, issue};

fn arg<'a>(err: &'a SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

/// The single error diagnostic carrying `expected`.
fn rejection(source: &str, expected: &str) -> SemanticError {
    let result = analyze_source(source);
    result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && issue(err) == Some(expected))
        .cloned()
        .unwrap_or_else(|| panic!("expected {expected:?}, got: {:?}", result.diagnostics))
}

#[test]
fn loop_body_with_callee_is_accepted() {
    let result = analyze_source(
        "functio quadratum(numerus n) → numerus {\n    redde n * n\n}\n\
         fixum numerus SUMMA = praefixum {\n    varia numerus s ← 0\n    \
         itera ab 1‥4 fixum i {\n        s ← s + quadratum(i)\n    }\n    redde s\n}\n",
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn result_must_type_check_as_the_declared_type() {
    assert_rejects_with_issue(
        "fixum numerus N = praefixum {\n    redde \"non numerus\"\n}\n",
        "return_type_mismatch",
    );
}

#[test]
fn praefixum_outside_a_static_is_rejected() {
    assert_rejects_with_issue(
        "incipit {\n    fixum numerus n ← praefixum {\n        redde 1\n    }\n}\n",
        "praefixum_outside_static",
    );
}

#[test]
fn direct_ad_is_forbidden() {
    let err = rejection(
        "fixum textus T = praefixum {\n    \
         fixum textus t ← ad 'runtime:echo' (\"salve\") ↦ textus\n    redde t\n}\n",
        "praefixum_forbidden",
    );
    assert_eq!(arg(&err, "effect"), Some("ad"));
    assert_eq!(arg(&err, "chain"), Some("T"));
}

#[test]
fn transitive_ad_names_the_call_chain() {
    let err = rejection(
        "functio ima() → textus {\n    \
         fixum textus t ← ad 'runtime:echo' (\"salve\") ↦ textus\n    redde t\n}\n\
         functio media() → textus {\n    redde ima()\n}\n\
         fixum textus T = praefixum {\n    redde media()\n}\n",
        "praefixum_forbidden",
    );
    assert_eq!(arg(&err, "effect"), Some("ad"));
    assert_eq!(arg(&err, "chain"), Some("T → media → ima"));
}

#[test]
fn async_callee_is_forbidden() {
    let err = rejection(
        "functio lente() fiet → numerus {\n    redde 1\n}\n\
         fixum numerus N = praefixum {\n    fixum _ f ← lente()\n    redde 1\n}\n",
        "praefixum_forbidden",
    );
    assert_eq!(arg(&err, "effect"), Some("async"));
}

#[test]
fn output_is_forbidden() {
    let err = rejection(
        "fixum numerus N = praefixum {\n    nota \"x\"\n    redde 1\n}\n",
        "praefixum_forbidden",
    );
    assert_eq!(arg(&err, "effect"), Some("scribe"));
}

#[test]
fn self_reference_is_a_cycle() {
    assert_rejects_with_issue(
        "fixum numerus A = praefixum {\n    redde A + 1\n}\n",
        "static_cycle",
    );
}

#[test]
fn cycle_through_a_callee_is_rejected() {
    assert_rejects_with_issue(
        "functio lege_a() → numerus {\n    redde A\n}\n\
         fixum numerus A = praefixum {\n    redde lege_a() + 1\n}\n",
        "static_cycle",
    );
}

#[test]
fn evaluation_order_follows_static_reads() {
    let fixture = analyze_source(
        "fixum numerus B = praefixum {\n    redde A * 2\n}\n\
         fixum numerus C = A + 1\n\
         fixum numerus A = praefixum {\n    redde 3\n}\n",
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let order = evaluation_order(hir, &fixture.result.types).expect("no cycle");
    let names: Vec<&str> = order
        .iter()
        .map(|site| fixture.interner.resolve(site.name))
        .collect();
    assert_eq!(names, ["A", "B"]);
}
