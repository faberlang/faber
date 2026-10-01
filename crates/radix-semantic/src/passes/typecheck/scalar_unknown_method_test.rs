//! Scalar numeric receivers reject unknown methods in semantic analysis.
//!
//! A method call on an `f32` / `i32` receiver that matches no registered
//! intrinsic must fail at check time with `unknown_method`, not survive
//! as an inferred result and fail later in codegen. The semantic result
//! carries only semantic errors, so rejection here means no codegen cascade.

use radix_diagnostics::DiagnosticArg;

fn analyze(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source(source)
}

fn rejects_unknown_method(source: &str) {
    let result = analyze(source);
    assert!(
        !result.is_ok(),
        "expected unknown_method rejection, but source was accepted: {source}"
    );
    let diagnostics = &result.diagnostics;
    assert!(
        diagnostics
            .iter()
            .any(|d| d.args == vec![DiagnosticArg::new("issue", "unknown_method")]),
        "expected a diagnostic with issue `unknown_method`, got: {diagnostics:?}"
    );
}

fn function_source(ty: &str, call: &str) -> String {
    format!("functio probe({ty} x) → {ty} {{\n    redde {call}\n}}\n")
}

#[test]
fn scalar_unknown_method_is_rejected_in_semantics() {
    for (ty, zero) in [("f32", "0.0"), ("i32", "0")] {
        rejects_unknown_method(&function_source(ty, &format!("x.max({zero})")));
        rejects_unknown_method(&function_source(ty, &format!("x.foo({zero})")));
    }

    let accepted = analyze(&function_source("f32", "x.maximus(0.0)"));
    assert!(
        accepted.is_ok(),
        "registered scalar method must check clean: {:?}",
        accepted.diagnostics
    );
}
