//! Builtin collection and text receivers reject unknown methods at check time
//!.
//!
//! A method call whose name is absent from the receiver's intrinsic table
//! must fail with `unknown_method`, not survive as an inferred result that
//! every emitter prints as a verbatim method call. Declared methods of user
//! genera and `ignotum` receivers stay permissive.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use radix_diagnostics::DiagnosticArg;

fn analyze(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source(source)
}

fn rejects_unknown_method(source: &str) {
    let result = analyze(source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.args == vec![DiagnosticArg::new("issue", "unknown_method")]),
        "expected an `unknown_method` diagnostic, got: {:?} for {source}",
        result.diagnostics
    );
}

fn accepts(source: &str) {
    let result = analyze(source);
    assert!(
        result.is_ok(),
        "expected clean check, got: {:?} for {source}",
        result.diagnostics
    );
}

#[test]
fn list_unknown_methods_are_rejected() {
    for call in [
        "prepend(0)",
        "insert(0)",
        "pop()",
        "clear()",
        "frobnicate()",
    ] {
        rejects_unknown_method(&format!(
            "incipit {{\n    varia lista<numerus> xs ← [1, 2, 3]\n    xs.{call}\n    nota xs\n}}"
        ));
    }
    accepts(
        "incipit {\n    varia lista<numerus> xs ← [1, 2, 3]\n    xs.appende(4)\n    nota xs.longitudo()\n}",
    );
}

#[test]
fn map_unknown_method_is_rejected() {
    rejects_unknown_method(
        "incipit {\n    varia tabula<textus, numerus> m ← {\"a\": 1}\n    m.frobnicate(\"a\")\n    nota m\n}",
    );
}

#[test]
fn set_unknown_method_is_rejected() {
    rejects_unknown_method(
        "incipit {\n    varia copia<numerus> s ← vacua\n    s.frobnicate(1)\n    nota s\n}",
    );
    accepts("incipit {\n    varia copia<numerus> s ← vacua\n    s.adde(1)\n    nota s\n}");
}

#[test]
fn text_unknown_method_is_rejected() {
    rejects_unknown_method("incipit {\n    fixum textus t ← \"abc\"\n    nota t.frobnicate()\n}");
}

#[test]
fn number_unknown_method_is_rejected() {
    rejects_unknown_method("incipit {\n    fixum numerus n ← 1\n    nota n.frobnicate()\n}");
}
