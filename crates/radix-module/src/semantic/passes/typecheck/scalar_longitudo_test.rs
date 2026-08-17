//! Typecheck coverage for scalar `textus.longitudo()` loops.
//!
//! Rust lhs `MethodCall` ordering parenthesization (`out.longitudo() < n`) is
//! asserted in `codegen/rust/tests/comparison_test.rs`.

use crate::{Compiler, Config, Target};

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust)).compile_str("test.fab", source)
}

fn accepts(result: &crate::CompileResult) {
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn scalar_longitudo_sectio_loop_typechecks_for_non_ascii_text() {
    accepts(&compile(
        r#"incipit {
    fixum textus s ← "café"
    varia numerus i ← 0
    varia textus out ← ""
    dum i < s.longitudo() {
        out ← out + s.sectio(i, i + 1)
        i ← i + 1
    }
    nota out
}"#,
    ));
}
