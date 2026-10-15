//! Typecheck laws for the sort expression (need, rulings O1 to O5).

use radix_diagnostics::DiagnosticArg;

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source(source)
}

fn accepts(result: &crate::SemanticResult) {
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

fn rejects_with_issue(result: &crate::SemanticResult, issue: &str) {
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.args == vec![DiagnosticArg::new("issue", issue)]),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn sort_from_list_set_map_and_ref_map_type_as_lists() {
    accepts(&compile(
        "functio f(lista<numerus> xs, copia<textus> s, tabula<textus, numerus> m) {\n    sit lista<numerus> a ← ordinata via ascendens ex xs\n    sit lista<textus> b ← ordinata via descendens ex s\n    sit lista<numerus> c ← ordinata via ascendens ex m\n    sit lista<textus> d ← ordinata via descendens de m\n}\n",
    ));
}

#[test]
fn sort_body_returns_the_key() {
    accepts(&compile(
        "functio f(lista<textus> xs) {\n    sit lista<textus> a ← ordinata via ascendens ex xs fixum p {\n        redde p.longitudo()\n    }\n}\n",
    ));
}

#[test]
fn sort_ref_on_a_list_or_a_set_is_refused_by_name() {
    rejects_with_issue(
        &compile(
            "functio f(lista<numerus> xs) {\n    sit lista<numerus> a ← ordinata via ascendens de xs\n}\n",
        ),
        "sort_ref_requires_map",
    );
    rejects_with_issue(
        &compile(
            "functio f(copia<numerus> s) {\n    sit lista<numerus> a ← ordinata via ascendens de s\n}\n",
        ),
        "sort_ref_requires_map",
    );
}

#[test]
fn sort_unordered_elements_and_keys_are_refused_by_name() {
    rejects_with_issue(
        &compile(
            "functio f(lista<lista<numerus>> xs) {\n    sit lista<lista<numerus>> a ← ordinata via ascendens ex xs\n}\n",
        ),
        "sort_source_unordered",
    );
    rejects_with_issue(
        &compile(
            "functio f(lista<numerus> xs) {\n    sit lista<numerus> a ← ordinata via ascendens ex xs fixum p {\n        redde [p]\n    }\n}\n",
        ),
        "sort_key_unordered",
    );
}
