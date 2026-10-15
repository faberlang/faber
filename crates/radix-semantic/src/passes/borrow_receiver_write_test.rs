//! audit1-a: a method call writes its receiver when the intrinsic registry
//! says so (`is_mutating_receiver`), not only when the call returns vacuum.
//!
//! Value-returning in-place mutators (`detrahe`/`decapita` return the removed
//! element, `dele` returns a bool) must count as receiver writes for the
//! `de` (ref) alias gate and for the `in` (mut) parameter lint (WARN008).
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::{SemanticErrorKind, WarningKind};

struct Findings {
    errors: Vec<String>,
    unused_mut_params: usize,
}

fn analyze(body: &str) -> Findings {
    let source = format!("{body}\nincipit {{ }}\n");
    let lex = radix_lexer::lex(&source);
    let mut parsed = radix_parser::parse(lex);
    assert!(parsed.success(), "parse: {:?}", parsed.errors);
    let program = parsed.program.take().expect("program");
    let config = crate::PassConfig::base();
    let result = crate::analyze(&program, &config, &mut parsed.interner);
    let errors = result
        .diagnostics
        .iter()
        .filter(|err| err.is_error())
        .map(|err| {
            err.args
                .iter()
                .find(|a| a.name == "issue")
                .map(|a| a.value.to_string())
                .unwrap_or_else(|| format!("{:?}", err.kind))
        })
        .collect();
    let unused_mut_params = result
        .diagnostics
        .iter()
        .filter(|err| {
            matches!(
                err.kind,
                SemanticErrorKind::Warning(WarningKind::UnusedMutRefParam)
            )
        })
        .count();
    Findings {
        errors,
        unused_mut_params,
    }
}

fn assert_alias_write_rejected(body: &str) {
    let found = analyze(body);
    assert!(
        found.errors.iter().any(|i| i == "assign_to_de_parameter"),
        "{:?}",
        found.errors
    );
}

#[test]
fn alias_of_de_list_rejects_detrahe() {
    assert_alias_write_rejected(
        "functio f(de lista<numerus> xs) → vacuum { sit lista<numerus> ys ← xs\n ys.detrahe() }",
    );
}

#[test]
fn alias_of_de_list_rejects_decapita() {
    assert_alias_write_rejected(
        "functio f(de lista<numerus> xs) → vacuum { sit lista<numerus> ys ← xs\n ys.decapita() }",
    );
}

#[test]
fn alias_of_de_tabula_rejects_bool_returning_dele() {
    assert_alias_write_rejected(
        "functio f(de tabula<textus, numerus> m) → vacuum { sit tabula<textus, numerus> n ← m\n n.dele(\"k\") }",
    );
}

#[test]
fn alias_of_de_list_vacuum_mutator_still_rejected() {
    assert_alias_write_rejected(
        "functio f(de lista<numerus> xs) → vacuum { sit lista<numerus> ys ← xs\n ys.appende(1) }",
    );
}

#[test]
fn alias_of_de_list_read_methods_stay_legal() {
    let found = analyze(
        "functio f(de lista<numerus> xs) → numerus { sit lista<numerus> ys ← xs\n redde ys.longitudo() }",
    );
    assert!(found.errors.is_empty(), "{:?}", found.errors);
}

#[test]
fn in_param_only_detrahe_does_not_suggest_ref() {
    let found = analyze("functio f(in lista<numerus> xs) → vacuum { xs.detrahe() }");
    assert!(found.errors.is_empty(), "{:?}", found.errors);
    assert_eq!(found.unused_mut_params, 0, "pop-only `in` param is a write");
}

#[test]
fn in_param_only_decapita_does_not_suggest_ref() {
    let found = analyze("functio f(in lista<numerus> xs) → vacuum { xs.decapita() }");
    assert!(found.errors.is_empty(), "{:?}", found.errors);
    assert_eq!(found.unused_mut_params, 0, "pop-only `in` param is a write");
}

#[test]
fn in_param_only_dele_does_not_suggest_ref() {
    let found = analyze("functio f(in tabula<textus, numerus> m) → vacuum { m.dele(\"k\") }");
    assert!(found.errors.is_empty(), "{:?}", found.errors);
    assert_eq!(
        found.unused_mut_params, 0,
        "dele-only `in` param is a write"
    );
}

#[test]
fn in_param_only_read_still_suggests_ref() {
    let found = analyze("functio f(in lista<numerus> xs) → numerus { redde xs.longitudo() }");
    assert_eq!(found.unused_mut_params, 1, "{:?}", found.errors);
}
