//! Typecheck probes for the `ordina` key selector (D1.7) and the retired keyed
//! `ordinata(key)` copy form.

use radix_diagnostics::DiagnosticArg;

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
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

const PERSONA: &str = r#"
genus Persona {
    varia textus nomen
    varia numerus aetas
    varia bivalens activa
}
"#;

fn with_persona(body: &str) -> String {
    format!(
        "{PERSONA}\nincipit {{\n    sit lista<Persona> ps ← [Persona {{ nomen = \"b\", aetas = 2, activa = verum }}]\n{body}\n}}\n"
    )
}

#[test]
fn ordinata_text_key_is_retired_by_name() {
    rejects_with_issue(
        &compile(&with_persona(
            "    sit lista<Persona> o ← ps.ordinata(Persona p ∴ p.nomen)\n    nota o.longitudo()",
        )),
        "sorted_key_retired",
    );
}

#[test]
fn ordinata_numeric_key_is_retired_by_name() {
    rejects_with_issue(
        &compile(&with_persona(
            "    sit lista<Persona> o ← ps.ordinata(Persona p ∴ p.aetas)\n    nota o.longitudo()",
        )),
        "sorted_key_retired",
    );
}

#[test]
fn ordinata_key_retirement_is_the_only_diagnostic() {
    let result = compile(&with_persona(
        "    sit lista<Persona> o ← ps.ordinata(Persona p ∴ p.aetas)\n    nota o.longitudo()",
    ));
    assert_eq!(
        result.diagnostics.len(),
        1,
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn ordina_key_sorts_varia_receiver_in_place() {
    accepts(&compile(&with_persona(
        "    varia lista<Persona> m ← copy ps\n    m.ordina(Persona p ∴ p.nomen)\n    nota m.longitudo()",
    )));
}

#[test]
fn ordina_genus_key_rejects() {
    rejects_with_issue(
        &compile(&with_persona(
            "    varia lista<Persona> m ← copy ps\n    m.ordina(Persona p ∴ p)\n    nota m.longitudo()",
        )),
        "ordina_key_not_ordered",
    );
}

#[test]
fn ordinata_without_key_is_unchanged() {
    accepts(&compile(
        "incipit {\n    sit lista<numerus> o ← [3, 1, 2].ordinata()\n    nota o\n}\n",
    ));
}
