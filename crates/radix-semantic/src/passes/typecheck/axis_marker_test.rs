//! Echo rule for the `axis` marker: a parameter declared
//! `axis int a` is called `axis <expr>`; a bare argument is refused; `axis`
//! before an argument of a plain parameter is refused; inside the function
//! the parameter is an ordinary int and passing it on writes the word again.
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::test_support::{analyze_ok, assert_rejects_with_issue};

const DEFS: &str = "functio rows(axis numerus a) → numerus { redde a + 1 }\n\
functio plain(numerus a) → numerus { redde a }\n\
functio pass(axis numerus a) → numerus { redde rows(axis a) }\n";

fn call(body: &str) -> String {
    format!("{DEFS}incipit {{\n    {body}\n}}")
}

#[test]
fn axis_marker_marked_call_is_accepted() {
    let source = "functio rows(axis numerus a) → numerus { redde a + 1 }\n\
functio plain(numerus a) → numerus { redde a }\n\
functio pass(axis numerus a) → numerus { redde rows(axis a) }\n\
incipit {\n    sit numerus x ← rows(axis 1)\n    sit numerus y ← plain(2)\n    sit numerus z ← pass(axis 3)\n}";
    let _ = analyze_ok(source);
}

#[test]
fn axis_marker_bare_argument_to_axis_parameter_is_required() {
    assert_rejects_with_issue(&call("sit numerus x ← rows(1)"), "axis_marker_required");
}

#[test]
fn axis_marker_on_plain_parameter_is_unexpected() {
    assert_rejects_with_issue(
        &call("sit numerus x ← plain(axis 1)"),
        "axis_marker_unexpected",
    );
}

#[test]
fn axis_marker_pass_through_needs_the_word_again() {
    let source = "functio rows(axis numerus a) → numerus { redde a + 1 }\n\
functio bare_pass(axis numerus a) → numerus { redde rows(a) }\n\
incipit { }";
    assert_rejects_with_issue(source, "axis_marker_required");
}

#[test]
fn axis_builtin_rms_norm_axis_marker_accepted() {
    let source = r"functio norm(tf32[2, 8] x, tf32[8] gamma) → tf32[2, 8] {
    redde x.rms_norm(axis 1, 1e-5, gamma)
}";
    let _ = analyze_ok(source);
}

#[test]
fn axis_builtin_laminatio_axis_marker_accepted() {
    let source = r"functio norm(tf32[2, 8] x, tf32[8] gamma, tf32[8] beta) → tf32[2, 8] {
    redde x.laminatio(axis 1, 1e-5, gamma, beta)
}";
    let _ = analyze_ok(source);
}

#[test]
fn axis_builtin_misplaced_marker_refused() {
    let source = r"functio norm(tf32[2, 8] x, tf32[8] gamma) → tf32[2, 8] {
    redde x.rms_norm(axis 1, axis 1e-5, gamma)
}";
    assert_rejects_with_issue(source, "axis_marker_unexpected");
}

#[test]
fn axis_builtin_bare_axis_is_required() {
    let rms_norm = r"functio norm(tf32[2, 8] x, tf32[8] gamma) → tf32[2, 8] {
    redde x.rms_norm(1, 1e-5, gamma)
}";
    assert_rejects_with_issue(rms_norm, "axis_marker_required");
    let laminatio = r"functio norm(tf32[2, 8] x, tf32[8] gamma, tf32[8] beta) → tf32[2, 8] {
    redde x.laminatio(1, 1e-5, gamma, beta)
}";
    assert_rejects_with_issue(laminatio, "axis_marker_required");
}

#[test]
fn axis_marker_parameter_is_a_plain_int_inside() {
    let source = "functio rows(axis numerus a) → numerus {\n    sit numerus b ← a * 2\n    redde b + a\n}\nincipit {\n    sit numerus x ← rows(axis 2)\n}";
    let _ = analyze_ok(source);
}

// --- SOFTAXIS: `softmax(axis n)` names the last axis only ---

/// Every method call's surface name and argument count in analyzed HIR.
struct MethodCallCollector {
    calls: Vec<(radix_lexer::Symbol, usize)>,
}

impl radix_hir::visit::HirVisitor for MethodCallCollector {
    fn visit_expr(&mut self, expr: &radix_hir::HirExpression) {
        if let radix_hir::HirExpressionKind::MethodCall(_, name, _, args, _) = &expr.kind {
            self.calls.push((*name, args.len()));
        }
        radix_hir::visit::walk_expr(self, expr);
    }
}

#[test]
fn softmax_axis_last_is_accepted_and_cleared() {
    let source = r"functio rows(tf32[2, 8] x) → tf32[2, 8] {
    redde x.activatio_softmax(axis 1)
}";
    let fixture = analyze_ok(source);
    let mut collector = MethodCallCollector { calls: Vec::new() };
    radix_hir::visit::walk_program(&mut collector, fixture.hir.as_ref().expect("hir"));
    assert!(
        collector.calls.iter().any(|(name, arity)| {
            fixture.interner.resolve(*name) == "activatio_softmax" && *arity == 0
        }),
        "the explicit last axis clears to the bare call: {:?}",
        collector.calls
    );
}

#[test]
fn softmax_axis_not_last_is_refused() {
    let source = r"functio rows(tf32[2, 8] x) → tf32[2, 8] {
    redde x.activatio_softmax(axis 0)
}";
    assert_rejects_with_issue(source, "softmax_axis_not_last");
}

#[test]
fn softmax_axis_out_of_range_is_refused() {
    let source = r"functio rows(tf32[2, 8] x) → tf32[2, 8] {
    redde x.activatio_softmax(axis 2)
}";
    assert_rejects_with_issue(source, "axis_reduction_out_of_range");
}

#[test]
fn softmax_axis_bare_call_stays_accepted() {
    let source = r"functio rows(tf32[2, 8] x) → tf32[2, 8] {
    redde x.activatio_softmax()
}";
    let _ = analyze_ok(source);
}

#[test]
fn softmax_axis_rank_one_last_is_accepted() {
    let source = r"functio vector(tf32[8] x) → tf32[8] {
    redde x.activatio_softmax(axis 0)
}";
    let _ = analyze_ok(source);
}
