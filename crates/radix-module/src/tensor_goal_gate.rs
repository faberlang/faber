//! Verification-plan gates for sized numerics and tensor type system invariants.
//!
//! This module owns runnable assertions that prove the type system behaves
//! correctly at the compiler level: numeric width assignability, tensor
//! construction, sized conversio, and family-width mismatch detection.
//! Callers invoke these gates from driver-level verification plans; the module
//! has no phase position in the semantic pipeline.

use crate::diagnostics::DiagnosticArg;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::semantic::{NumericWidth, Primitive, Type, TypeTable};

pub fn run_types_gate() {
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    assert!(types.is_assignable(i32, i64));
    assert!(!types.is_assignable(i64, i32));
    assert_eq!(NumericWidth::I32.rust_type(), "i32");

    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let rank0 = types.rank0_shape();
    let tensor = types.tensor_with_shape(elem, rank0);
    assert!(matches!(types.get(tensor), Type::Tensor(inner, _) if *inner == elem));

    let rank0 = types.rank0_shape();
    let _tensor_id = types.tensor_with_shape(elem, rank0);
    // Tensor rejection now lives in the Go/TS type-spelling layer (lazy smoke
    // test) rather than an arena-wide pre-scan. The gate keeps verifying the
    // tensor intern shape; spelling-layer rejection is covered by codegen tests.

    let session = Session::new(Config::default());
    let result = analyze_source(
        &session,
        "gate-tensor-decl.fab",
        "functio hold(tensor<f32, _> value) → vacuum {}",
        AnalyzeRequest::default(),
    );
    assert!(result.is_ok(), "tensor decl should typecheck: {:?}", result.err());
}

pub fn run_convert_gate() {
    let session = Session::new(Config::default());

    let i32_ok = analyze_source(
        &session,
        "sized-i32.fab",
        r#"incipit {
  fixum i32 n ← "42" ↦ i32
  nota n
}"#,
        AnalyzeRequest::default(),
    );
    assert!(i32_ok.is_ok(), "expected sized i32 conversio to typecheck: {:?}", i32_ok.err());

    let f32_ok = analyze_source(
        &session,
        "sized-f32.fab",
        r#"incipit {
  fixum f32 f ← "1.5" ↦ f32
  nota f
}"#,
        AnalyzeRequest::default(),
    );
    assert!(f32_ok.is_ok(), "expected sized f32 conversio to typecheck: {:?}", f32_ok.err());

    let family_err = analyze_source(
        &session,
        "family-error.fab",
        r"incipit {
  fixum numerus<f32> invalid ← 1.0 ∷ f32
  nota invalid
}",
        AnalyzeRequest::default(),
    );
    assert!(
        family_err.is_err(),
        "expected family error diagnostic from float_width_on_numerus case"
    );
    if let Err(diags) = family_err {
        assert!(
            diags.iter().any(|d| d.args
                == vec![
                    DiagnosticArg::new("issue", "float_width_on_numerus"),
                    DiagnosticArg::new("primitive", "numerus")
                ]),
            "expected family diagnostic, got: {diags:?}"
        );
    }
}

#[cfg(test)]
#[path = "tensor_goal_gate_test.rs"]
mod tests;
