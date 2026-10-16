//! The format operator `value ¶ "spec"` (DECISIONS D2.1, D2.2, D2.5).
//!
//! `¶` sits on its own binary tier: looser than arithmetic (and the bitwise
//! and shift tiers), tighter than comparison. The right side must be a
//! string literal; a computed spec is rejected in pass 1. `¶` does not chain.

use crate::test_support::{parse_ok, parse_program};
use radix_syntax::{BinOp, Expr, ExprKind, Literal, StmtKind};

fn initializer(source: &str) -> ExprKind {
    let program = parse_ok(source).program.expect("program");
    let StmtKind::Var(decl) = program.statements.into_iter().next().expect("stmt").kind else {
        panic!("expected a binding");
    };
    let init: Expr = *decl.init.expect("initializer");
    init.kind
}

fn issue_span(source: &str, issue: &str) -> u32 {
    let result = parse_program(source);
    let err = result
        .errors
        .iter()
        .find(|err| crate::test_support::has_arg(err, "issue", issue))
        .unwrap_or_else(|| panic!("expected {issue}, got {:?}", result.errors));
    err.span.start
}

fn offset(source: &str, needle: &str) -> u32 {
    u32::try_from(source.rfind(needle).expect("needle in source")).expect("fits")
}

#[test]
fn pilcrow_parses_as_the_format_node() {
    let ExprKind::Format(format) = initializer("sit _ v ← pretium ¶ \".2\"") else {
        panic!("expected a format expression");
    };
    assert!(matches!(format.value.kind, ExprKind::Ident(_)));
}

#[test]
fn format_binds_looser_than_arithmetic() {
    let ExprKind::Format(format) = initializer("sit _ v ← a + b ¶ \".2\"") else {
        panic!("expected (a + b) ¶ spec");
    };
    assert!(matches!(
        format.value.kind,
        ExprKind::Binary(ref bin) if matches!(bin.op, BinOp::Add)
    ));
}

#[test]
fn format_binds_tighter_than_comparison() {
    let ExprKind::Binary(bin) = initializer("sit _ v ← a ≡ b ¶ \"5\"") else {
        panic!("expected a ≡ (b ¶ spec)");
    };
    assert!(matches!(bin.op, BinOp::Eq));
    assert!(matches!(bin.rhs.kind, ExprKind::Format(_)));
}

#[test]
fn parenthesized_format_is_a_plain_operand() {
    let ExprKind::Binary(bin) = initializer("sit _ v ← (a ¶ \".2\") + \"!\"") else {
        panic!("expected (a ¶ spec) + text");
    };
    let ExprKind::Paren(inner) = &bin.lhs.kind else {
        panic!("expected a parenthesized operand");
    };
    assert!(matches!(inner.kind, ExprKind::Format(_)));
    assert!(matches!(
        bin.rhs.kind,
        ExprKind::Literal(Literal::String(_))
    ));
}

#[test]
fn chained_format_is_rejected_at_the_second_pilcrow() {
    let source = "sit _ v ← x ¶ \"a\" ¶ \"b\"";
    assert_eq!(issue_span(source, "format_chained"), offset(source, "¶"));
}

#[test]
fn computed_spec_is_rejected() {
    let source = "sit _ v ← x ¶ spec";
    assert_eq!(
        issue_span(source, "format_spec_not_literal"),
        offset(source, "spec")
    );
}

#[test]
fn parenthesized_spec_is_rejected() {
    let source = "sit _ v ← x ¶ (\".2\")";
    assert_eq!(
        issue_span(source, "format_spec_not_literal"),
        offset(source, "(")
    );
}

#[test]
fn missing_spec_is_rejected() {
    assert!(!parse_program("sit _ v ← x ¶").success());
}
