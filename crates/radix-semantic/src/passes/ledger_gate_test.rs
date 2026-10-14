//! Ledger type resolution and the operation refusal (need, unit L1).

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
fn ledger_resolves_as_a_parameter_and_a_return_type() {
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) → tabulae<textus, numerus> {\n    redde l\n}\n",
    ));
}

#[test]
fn ledger_resolves_as_a_field_type() {
    accepts(&compile(
        "genus Libri {\n    varia tabulae<textus, numerus> entries\n}\nfunctio f(Libri b) {\n}\n",
    ));
}

#[test]
fn ledger_key_follows_the_map_hashable_rule() {
    rejects_with_issue(
        &compile("functio f(tabulae<fractus, numerus> l) {\n}\n"),
        "tabula_key_not_hashable",
    );
}

#[test]
fn ledger_index_and_method_are_refused_by_name() {
    rejects_with_issue(
        &compile("functio f(tabulae<textus, numerus> l) → numerus {\n    redde l[\"a\"]\n}\n"),
        "ledger_operation_not_implemented",
    );
    rejects_with_issue(
        &compile("functio f(tabulae<textus, numerus> l) → numerus {\n    redde l.longitudo()\n}\n"),
        "ledger_operation_not_implemented",
    );
}

#[test]
fn ledger_loop_and_print_are_refused_by_name() {
    rejects_with_issue(
        &compile(
            "functio f(tabulae<textus, numerus> l) {\n    itera ex l fixum v {\n        nota v\n    }\n}\n",
        ),
        "ledger_operation_not_implemented",
    );
    rejects_with_issue(
        &compile("functio f(tabulae<textus, numerus> l) {\n    nota l\n}\n"),
        "ledger_operation_not_implemented",
    );
}

#[test]
fn ledger_literal_brace_and_empty_typecheck() {
    accepts(&compile(
        "functio f() {\n    varia tabulae<textus, numerus> x ← { \"b\": 2, \"a\": 1 }\n}\n",
    ));
    accepts(&compile(
        "functio f() {\n    varia tabulae<textus, numerus> x ← {}\n}\n",
    ));
    accepts(&compile(
        "functio f() {\n    varia tabulae<textus, numerus> x ← vacua\n}\n",
    ));
}

#[test]
fn ledger_literal_assignment_typechecks() {
    accepts(&compile(
        "functio f() {\n    varia tabulae<textus, numerus> x ← vacua\n    x ← { \"a\": 1 }\n}\n",
    ));
    accepts(&compile(
        "functio f() {\n    varia tabulae<textus, numerus> x ← { \"a\": 1 }\n    x ← vacua\n}\n",
    ));
}

#[test]
fn ledger_literal_map_to_ledger_stays_a_mismatch() {
    rejects_with_issue(
        &compile(
            "functio f() {\n    varia tabula<textus, numerus> m ← { \"a\": 1 }\n    varia tabulae<textus, numerus> x ← m\n}\n",
        ),
        "initializer_annotation_mismatch",
    );
}

#[test]
fn ledger_literal_index_still_refused() {
    rejects_with_issue(
        &compile("functio f(tabulae<textus, numerus> l) → numerus {\n    redde l[\"a\"]\n}\n"),
        "ledger_operation_not_implemented",
    );
}
