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

/// Any diagnostic naming `issue`, whatever its other args carry.
fn rejects_with_named_issue(result: &crate::SemanticResult, issue: &str) {
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value.as_str() == issue)),
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
fn ledger_ops_index_read_yields_the_value_type() {
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) → numerus {\n    redde l[\"a\"]\n}\n",
    ));
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) → vacuum {\n    fixum numerus v ← l[\"a\"]\n    nota v\n}\n",
    ));
}

#[test]
fn ledger_ops_index_store_accepted_on_var_ledger() {
    accepts(&compile(
        "functio f() → vacuum {\n    varia tabulae<textus, numerus> x ← {}\n    x[\"a\"] ← 3\n}\n",
    ));
}

#[test]
fn ledger_ops_methods_type_like_the_map_twin() {
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) → bivalens {\n    varia tabulae<textus, numerus> x ← vacua\n    x.pone(\"c\", 1)\n    fixum numerus ∪ nihil g ← x.accipe(\"a\")\n    fixum bivalens h ← x.habet(\"a\")\n    fixum bivalens d ← x.dele(\"a\")\n    fixum numerus n ← x.longitudo()\n    fixum bivalens e ← x.vacua()\n    fixum lista<textus> ks ← x.claves()\n    fixum lista<numerus> vs ← x.valores()\n    redde h aut d aut e\n}\n",
    ));
}

#[test]
fn ledger_ops_at_read_types_like_the_map_twin() {
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) → numerus {\n    redde l.apud(\"a\") ⊥ 0\n}\n",
    ));
}

#[test]
fn ledger_ops_membership_typechecks() {
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) → bivalens {\n    redde \"a\" ∈ l\n}\n",
    ));
}

#[test]
fn ledger_ops_three_loop_forms_typecheck() {
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) → vacuum {\n    itera ex l fixum v {\n        fixum numerus x ← v\n        nota x\n    }\n}\n",
    ));
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) → vacuum {\n    itera de l fixum k {\n        fixum textus x ← k\n        nota x\n    }\n}\n",
    ));
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) → vacuum {\n    itera ex l apud [k] fixum v {\n        fixum textus a ← k\n        fixum numerus b ← v\n        nota a, b\n    }\n}\n",
    ));
}

#[test]
fn ledger_ops_loop_shape_change_still_refused() {
    rejects_with_named_issue(
        &compile(
            "incipit {\n    varia tabulae<textus, numerus> m ← vacua\n    itera de m fixum k {\n        m.pone(k, 1)\n    }\n}\n",
        ),
        "loop_collection_shape_change",
    );
}

#[test]
fn ledger_equality_typechecks() {
    accepts(&compile(
        "functio f(tabulae<textus, numerus> a, tabulae<textus, numerus> b) → bivalens {\n    redde a ≡ b\n}\n",
    ));
    accepts(&compile(
        "functio f(tabulae<textus, numerus> a, tabulae<textus, numerus> b) → bivalens {\n    redde a ≢ b\n}\n",
    ));
}

#[test]
fn ledger_print_and_interpolation_typecheck() {
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) {\n    nota l\n}\n",
    ));
}

#[test]
fn ledger_equality_against_map_is_refused() {
    rejects_with_issue(
        &compile(
            "functio f(tabulae<textus, numerus> a, tabula<textus, numerus> b) → bivalens {\n    redde a ≡ b\n}\n",
        ),
        "equality_exact_types_required",
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
fn ledger_ops_literal_index_types_instead_of_refusing() {
    accepts(&compile(
        "functio f(tabulae<textus, numerus> l) → numerus {\n    redde l[\"a\"]\n}\n",
    ));
}
