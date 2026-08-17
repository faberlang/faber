//! Exhaustive lattice coverage for tensor element-width conversio.
//!
//! A single parameterised test pairs every distinct tensor element type with
//! every target width, verifying that allowed conversions typecheck and that
//! disallowed conversions produce `tensor_element_mismatch`.

use crate::semantic::tensor_element_lattice;
use crate::{Compiler, Config, Target};

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust)).compile_str("test.fab", source)
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

/// Exhaustive `tensor<A> ↦ tensor<B>` lattice: every distinct element type × every target.
#[test]
fn tensor_element_width_lattice_matrix_typecheck() {
    let mut types = crate::semantic::TypeTable::new();
    for from in tensor_element_lattice::SPECS {
        for to in tensor_element_lattice::SPECS {
            let case = format!("{}_to_{}", from.label, to.label);
            let source = tensor_element_lattice::conversio_source(from, to);
            let result = compile(&source);
            if tensor_element_lattice::conversio_allowed(&mut types, from, to) {
                assert!(result.success(), "{case}: {:?}", result.diagnostics);
            } else {
                rejects_with_issue(&result, "tensor_element_mismatch");
            }
        }
    }
}
