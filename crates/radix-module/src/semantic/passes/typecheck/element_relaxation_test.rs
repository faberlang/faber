//! Regression coverage for tensor element relaxation — non-numeric element types in tensor construction and arithmetic.
//!
//! These tests prove that tensor<textus> construction via `structa`/`accipe` is
//! accepted while elementwise arithmetic on non-numeric tensor elements is
//! rejected with a targeted diagnostic.

use crate::{Compiler, Config, Target};

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust)).compile_str("element_relaxation.fab", source)
}

#[cfg(feature = "hir-rust")]
fn accepts(result: &crate::CompileResult) {
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

fn rejects_with_issue(result: &crate::CompileResult, issue: &str) {
    assert!(!result.success(), "expected failure, got: {:?}", result.diagnostics);
    assert!(
        result.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn tensor_textus_construction_and_accipe_typecheck() {
    accepts(&compile(
        r#"incipit {
  fixum lista<textus> flat ← ["a", "b", "c"]
  fixum tensor<textus, _> seed ← vacua
  fixum tensor<textus, [3]> grid ← seed.strue(flat, [3])
  fixum textus cell ← grid.accipe([0]) vel ""
  nota cell
}"#,
    ));
}

#[test]
fn tensor_textus_addita_rejects() {
    rejects_with_issue(
        &compile(
            r#"incipit {
  fixum lista<textus> flat ← ["a", "b", "c"]
  fixum tensor<textus, _> seed ← vacua
  fixum tensor<textus, [3]> left ← seed.strue(flat, [3])
  fixum tensor<textus, [3]> right ← seed.strue(flat, [3])
  fixum _ sum ← left.addita(right)
}"#,
        ),
        "tensor_arithmetic_numeric_element_required",
    );
}
