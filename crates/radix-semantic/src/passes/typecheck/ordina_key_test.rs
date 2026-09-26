//! Typecheck probes for the `ordina` / `ordinata` key selector (D1.7).

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

const PERSONA: &str = r#"
genus Persona {
    textus nomen
    numerus aetas
    bivalens activa
}
"#;

fn with_persona(body: &str) -> String {
    format!(
        "{PERSONA}\nincipit {{\n    fixum lista<Persona> ps ← [Persona {{ nomen = \"b\", aetas = 2, activa = verum }}]\n{body}\n}}\n"
    )
}

#[test]
fn ordinata_text_key_typechecks_as_receiver_lista() {
    accepts(&compile(&with_persona(
        "    fixum lista<Persona> o ← ps.ordinata(Persona p ∴ p.nomen)\n    nota o.longitudo()",
    )));
}

#[test]
fn ordinata_numeric_key_typechecks() {
    accepts(&compile(&with_persona(
        "    fixum lista<Persona> o ← ps.ordinata(Persona p ∴ p.aetas)\n    nota o.longitudo()",
    )));
}

#[test]
fn ordina_key_sorts_varia_receiver_in_place() {
    accepts(&compile(&with_persona(
        "    varia lista<Persona> m ← ps\n    m.ordina(Persona p ∴ p.nomen)\n    nota m.longitudo()",
    )));
}

#[test]
fn ordinata_bivalens_key_rejects() {
    rejects_with_issue(
        &compile(&with_persona(
            "    fixum lista<Persona> o ← ps.ordinata(Persona p ∴ p.activa)\n    nota o.longitudo()",
        )),
        "ordina_key_not_ordered",
    );
}

#[test]
fn ordina_genus_key_rejects() {
    rejects_with_issue(
        &compile(&with_persona(
            "    varia lista<Persona> m ← ps\n    m.ordina(Persona p ∴ p)\n    nota m.longitudo()",
        )),
        "ordina_key_not_ordered",
    );
}

#[test]
fn ordinata_without_key_is_unchanged() {
    accepts(&compile(
        "incipit {\n    fixum lista<numerus> o ← [3, 1, 2].ordinata()\n    nota o\n}\n",
    ));
}

#[test]
fn ordinata_key_result_is_not_key_lista() {
    rejects_with_issue(
        &compile(&with_persona(
            "    fixum lista<textus> o ← ps.ordinata(Persona p ∴ p.nomen)\n    nota o",
        )),
        "initializer_annotation_mismatch",
    );
}
