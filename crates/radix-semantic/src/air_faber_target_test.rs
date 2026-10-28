//! The Faber target re-emits authored source: the AIR unroll and purity
//! passes run on clones there, so the unit keeps the authored loop while the
//! declines stay observable. Every other target keeps the in-place rewrite.

use crate::test_support::analyze_source_for_target;
use crate::{SemanticErrorKind, SemanticResult};
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{HirExpression, HirExpressionKind, HirModule};
use radix_target::Target;

const ADMIT: &str = r#"
@ radix lane "air"
functio tensor_sum(tensor<f32, [4]> x) → f32 {
    varia f32 total ← 0.0
    itera ex x sit value {
        total ← total + value
    }
    redde total
}
"#;

const DECLINE: &str = r#"
@ radix lane "air"
functio too_wide(tensor<f32, [65]> x) → f32 {
    itera ex x sit value { }
    redde 0.0
}
"#;

struct CountIteras(usize);
impl HirVisitor for CountIteras {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(expr.kind, HirExpressionKind::Itera { .. }) {
            self.0 += 1;
        }
        walk_expr(self, expr);
    }
}

fn itera_count(hir: &HirModule) -> usize {
    let mut counter = CountIteras(0);
    walk_program(&mut counter, hir);
    counter.0
}

fn analyze(source: &str, target: Target) -> SemanticResult {
    analyze_source_for_target(source, target).result
}

fn has_decline(result: &SemanticResult, causa: &str) -> bool {
    result.diagnostics.iter().any(|err| {
        err.kind == SemanticErrorKind::AirPurityViolation
            && err
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
            && err
                .args
                .iter()
                .any(|arg| arg.name == "causa" && arg.value == causa)
    })
}

#[test]
fn faber_target_keeps_the_authored_loop() {
    let result = analyze(ADMIT, Target::HirFaber);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(super::error::SemanticError::is_error),
        "admitted AIR loop must analyze clean: {:?}",
        result.diagnostics
    );
    assert_eq!(itera_count(result.hir.as_ref().expect("typed HIR")), 1);
}

#[test]
fn lowering_targets_still_unroll_in_place() {
    for target in [Target::MirWasm, Target::HirRust] {
        let result = analyze(ADMIT, target);
        assert_eq!(
            itera_count(result.hir.as_ref().expect("typed HIR")),
            0,
            "{target:?} must still unroll the admitted loop"
        );
    }
}

#[test]
fn faber_target_keeps_the_unroll_decline() {
    let result = analyze(DECLINE, Target::HirFaber);
    assert!(
        has_decline(&result, "budget"),
        "decline must survive the clone: {:?}",
        result.diagnostics
    );
    assert_eq!(itera_count(result.hir.as_ref().expect("typed HIR")), 1);
}
