//! Typecheck coverage for scalar `textus.longitudo()` loops.
//!
//! Rust lhs `MethodCall` ordering parenthesization (`out.longitudo() < n`) is
//! asserted in `codegen/rust/tests/comparison_test.rs`.

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
}

fn accepts(result: &crate::SemanticResult) {
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn scalar_longitudo_sectio_loop_typechecks_for_non_ascii_text() {
    accepts(&compile(
        r#"incipit {
    sit textus s ← "café"
    varia numerus i ← 0
    varia textus out ← ""
    dum i ≺ s.longitudo() {
        out ← out + s.sectio(i, i + 1)
        i ← i + 1
    }
    nota out
}"#,
    ));
}
