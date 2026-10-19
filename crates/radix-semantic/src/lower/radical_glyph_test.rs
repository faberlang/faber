//! Semantic lowering of the radical glyphs.
//!
//! Contract: `√ x` is sugar for `x.sqrt()` exactly where `.sqrt()` is legal,
//! `∛ x` for `x.cbrt()`, and `∜ x` for `x.sqrt().sqrt()`. Lowering emits the
//! ordinary method twin, so the intrinsic checker (not the glyph) owns
//! legality: an integer operand inherits the `.sqrt()` receiver error. The
//! glyph node ids are recorded for Faber re-emission; `∜` gets its own set
//! because its nested twin is otherwise indistinguishable from authored
//! `√ x.sqrt()`.

use crate::test_support::{AnalyzedFixture, analyze_ok, analyze_source, issue};
use radix_hir::{HirExpression, HirExpressionKind, HirItemKind, HirStatementKind};

/// The return expression of the first function item.
fn returned_expr(fixture: &AnalyzedFixture) -> HirExpression {
    let hir = fixture.result.hir.as_ref().expect("hir");
    let item = hir.items.first().expect("function item");
    let HirItemKind::Function(function) = &item.kind else {
        panic!("expected a function item, got {:?}", item.kind);
    };
    let body = function.body.as_ref().expect("function body");
    body.statements
        .iter()
        .rev()
        .find_map(|stmt| match &stmt.kind {
            HirStatementKind::Redde(Some(expr)) => Some(expr.clone()),
            _ => None,
        })
        .expect("return expression")
}

/// Assert `expr` is the canonical zero-argument method twin `canonical` and
/// return its receiver.
fn assert_method_twin<'a>(
    fixture: &AnalyzedFixture,
    expr: &'a HirExpression,
    canonical: &str,
) -> &'a HirExpression {
    let HirExpressionKind::MethodCall(receiver, name, type_args, args, _) = &expr.kind else {
        panic!("expected a method call, got {:?}", expr.kind);
    };
    assert_eq!(fixture.interner.resolve(*name), canonical);
    assert!(type_args.is_empty(), "the twin takes no type arguments");
    assert!(args.is_empty(), "the twin takes no arguments");
    receiver
}

/// First error diagnostic's `issue` argument.
fn first_issue(source: &str) -> Option<String> {
    analyze_source(source)
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .find_map(|diagnostic| issue(diagnostic).map(str::to_owned))
}

#[test]
fn radical_sqrt_lowers_to_method_twin() {
    let fixture = analyze_ok("functio radix_sqrt(f32 x) → f32 {\n    redde √ x\n}");
    let expr = returned_expr(&fixture);
    assert_method_twin(&fixture, &expr, "radix");
    let presentation = &fixture.result.presentation;
    assert!(
        presentation.glyph_ids.contains(&expr.id),
        "√ node id is recorded as a glyph"
    );
    assert!(
        !presentation.radical_quarta_ids.contains(&expr.id),
        "√ is not the fourth-root glyph"
    );
}

#[test]
fn radical_cbrt_lowers_to_cubica_twin() {
    let fixture = analyze_ok("functio radix_cbrt(f32 x) → f32 {\n    redde ∛ x\n}");
    let expr = returned_expr(&fixture);
    assert_method_twin(&fixture, &expr, "radix_cubica");
    let presentation = &fixture.result.presentation;
    assert!(
        presentation.glyph_ids.contains(&expr.id),
        "∛ node id is recorded as a glyph"
    );
    assert!(
        !presentation.radical_quarta_ids.contains(&expr.id),
        "∛ is not the fourth-root glyph"
    );
}

#[test]
fn radical_fourth_lowers_to_nested_sqrt() {
    let fixture = analyze_ok("functio radix_quarta(f32 x) → f32 {\n    redde ∜ x\n}");
    let outer = returned_expr(&fixture);
    let inner = assert_method_twin(&fixture, &outer, "radix");
    assert_method_twin(&fixture, inner, "radix");
    let presentation = &fixture.result.presentation;
    assert!(
        presentation.radical_quarta_ids.contains(&outer.id),
        "the outer ∜ id is recorded"
    );
    assert!(
        !presentation.radical_quarta_ids.contains(&inner.id),
        "the synthesized inner twin is not the authored glyph"
    );
    assert!(
        !presentation.glyph_ids.contains(&inner.id),
        "the synthesized inner twin carries no glyph id"
    );
}

#[test]
fn radical_int_operand_inherits_sqrt_error() {
    let glyph = first_issue("functio radix_int(numerus x) → f64 {\n    redde √ x\n}");
    let method = first_issue("functio radix_int(numerus x) → f64 {\n    redde x.radix()\n}");
    assert_eq!(
        glyph, method,
        "the glyph inherits the authored method's receiver error"
    );
    assert_eq!(
        glyph.as_deref(),
        Some("unknown_method"),
        "an integer receiver has no sqrt method"
    );
}
