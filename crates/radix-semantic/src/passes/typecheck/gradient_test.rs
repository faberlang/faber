//! TENSOR-GLYPHS-2A fail-closed coverage for the postfix gradient-selection
//! node. Phase 2A ships the dedicated syntax/HIR node only: the operand
//! eligibility rule and selector resolution land with 2B. Until then every
//! `call(...) ∇ [places]` expression must fail closed with a structured
//! `gradient_expression_unsupported` fact — never a silent pass.

use radix_diagnostics::DiagnosticArg;

fn rejects_with_issue(source: &str, issue: &str) {
    let result = crate::test_support::analyze_source_la(source);
    assert!(
        !result.is_ok(),
        "expected semantic failure, but source was accepted"
    );
    let diagnostics = result.diagnostics.clone();
    let issue_arg = DiagnosticArg::new("issue", issue);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error() && d.args.contains(&issue_arg)),
        "expected a diagnostic with issue `{issue}`, got: {diagnostics:?}"
    );
}

const BARE_GRADIENT: &str = r"functio loss(f32 x, f32 y) → f32 {
    redde x * y
}
incipit {
    fixum f32 a ← 1.0
    fixum f32 b ← 2.0
    fixum _ g ← loss(a, b) ∇
}
";

const SELECTED_GRADIENT: &str = r"functio loss(f32 x, f32 y) → f32 {
    redde x * y
}
incipit {
    fixum f32 a ← 1.0
    fixum f32 b ← 2.0
    fixum _ g ← loss(a, b) ∇ [a, b]
}
";

#[test]
fn bare_gradient_fails_closed_in_typecheck() {
    rejects_with_issue(BARE_GRADIENT, "gradient_expression_unsupported");
}

#[test]
fn selected_gradient_fails_closed_in_typecheck() {
    rejects_with_issue(SELECTED_GRADIENT, "gradient_expression_unsupported");
}

#[test]
fn gradient_fails_closed_before_target_emission() {
    // The fail-closed diagnostic must surface during analysis, so a gradient
    // program never reaches a backend as an untyped silent pass.
    let result = crate::test_support::analyze_source_la(SELECTED_GRADIENT);
    assert!(result.is_err(), "gradient must not compile in Phase 2A");
}
