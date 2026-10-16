//! The value conditional `c ✓ a ✗ b` (DECISIONS D4.5, D4.6).
//!
//! `✓ ✗` sits on the conditional tier (looser than `aut`, tighter than `←`)
//! and reuses the `Ternary` node, so backends need no change. One level only:
//! a `✓ ✗` anywhere inside another — condition or either branch — is
//! rejected with `conditional_nested`, spanned at the inner conditional.

use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok, parse_program};
use radix_syntax::{BinOp, Expr, ExprKind, StmtKind};

fn initializer(source: &str) -> ExprKind {
    let program = parse_ok(source).program.expect("program");
    let StmtKind::Var(decl) = program.statements.into_iter().next().expect("stmt").kind else {
        panic!("expected a binding");
    };
    let init: Expr = *decl.init.expect("initializer");
    init.kind
}

fn nested_error_span(source: &str, inner: &str) {
    let result = parse_program(source);
    let err = result
        .errors
        .iter()
        .find(|err| crate::test_support::has_arg(err, "issue", "conditional_nested"))
        .unwrap_or_else(|| panic!("expected conditional_nested, got {:?}", result.errors));
    let start = u32::try_from(source.find(inner).expect("inner in source")).expect("fits");
    assert_eq!(
        err.span.start, start,
        "span must point at the inner conditional"
    );
}

#[test]
fn check_ballot_parses_as_the_conditional_node() {
    let ExprKind::Ternary(ternary) = initializer("sit _ v ← c ✓ 1 ✗ 2") else {
        panic!("expected a conditional");
    };
    assert!(matches!(ternary.cond.kind, ExprKind::Ident(_)));
    assert!(matches!(ternary.then.kind, ExprKind::Literal(_)));
    assert!(matches!(ternary.else_.kind, ExprKind::Literal(_)));
}

#[test]
fn check_ballot_condition_binds_looser_than_aut() {
    let ExprKind::Ternary(ternary) = initializer("sit _ v ← a aut b ✓ 1 ✗ 2") else {
        panic!("expected a conditional");
    };
    assert!(matches!(
        ternary.cond.kind,
        ExprKind::Binary(ref bin) if matches!(bin.op, BinOp::Or)
    ));
}

#[test]
fn check_without_ballot_is_rejected() {
    assert!(!parse_program("sit _ v ← c ✓ 1").success());
}

#[test]
fn sibling_conditionals_are_not_nested() {
    let _ = parse_ok("sit _ v ← f(c ✓ 1 ✗ 2, d ✓ 3 ✗ 4)");
    let _ = parse_ok("sit _ v ← c ✓ 1 ✗ 2\nsit _ w ← d ✓ 3 ✗ 4");
}

#[test]
fn conditional_in_else_branch_is_rejected() {
    assert_parse_error_matches(
        "sit _ v ← c ✓ 1 ✗ d ✓ 2 ✗ 3",
        ParseExpectation::Issue("conditional_nested"),
    );
}

#[test]
fn conditional_in_then_branch_is_rejected() {
    assert_parse_error_matches(
        "sit _ v ← c ✓ (d ✓ 1 ✗ 2) ✗ 3",
        ParseExpectation::Issue("conditional_nested"),
    );
    assert_parse_error_matches(
        "sit _ v ← c ✓ d ✓ 1 ✗ 2 ✗ 3",
        ParseExpectation::Issue("conditional_nested"),
    );
}

#[test]
fn conditional_deep_inside_a_branch_is_rejected() {
    assert_parse_error_matches(
        "sit _ v ← c ✓ f(d ✓ 1 ✗ 2) ✗ 3",
        ParseExpectation::Issue("conditional_nested"),
    );
}

#[test]
fn conditional_in_condition_is_rejected() {
    assert_parse_error_matches(
        "sit _ v ← (c ✓ a ✗ b) ✓ 1 ✗ 2",
        ParseExpectation::Issue("conditional_nested"),
    );
}

#[test]
fn nested_diagnostic_points_at_the_inner_conditional() {
    nested_error_span("sit _ v ← c ✓ 1 ✗ d ✓ 2 ✗ 3", "d ✓");
    nested_error_span("sit _ v ← c ✓ f(d ✓ 1 ✗ 2) ✗ 3", "d ✓");
}
