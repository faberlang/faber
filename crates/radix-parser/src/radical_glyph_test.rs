//! Prefix radical glyphs: `√` square root, `∛` cube root, `∜` fourth root
//!. They bind like unary `-`: `√ x + 1` is `(√ x) + 1`, and
//! `√ (a + b)` takes the parenthesized operand.

use super::ParseResult;
use crate::test_support::parse_program_la;
use radix_syntax::{BinOp, ExprKind, Literal, StmtKind, UnOp};

fn parse_source(source: &str) -> ParseResult {
    parse_program_la(source)
}

fn first_expr(result: &ParseResult) -> &ExprKind {
    let statement = &result.program.as_ref().expect("program").statements[0];
    let StmtKind::Expr(statement) = &statement.kind else {
        panic!("expected expression statement: {:?}", statement.kind);
    };
    &statement.expr.kind
}

#[test]
fn radical_prefix_parses_without_errors() {
    for source in [
        "incipit { sit _ a ← √ x }",
        "incipit { sit _ a ← ∛ x + 1 }",
        "incipit { sit _ a ← ∜ (x + y) }",
        "incipit { sit _ a ← √ √ x }",
    ] {
        let result = parse_source(source);
        assert!(result.errors.is_empty(), "{source}: {:?}", result.errors);
    }
}

#[test]
fn radical_binds_like_unary_minus() {
    let result = parse_source("√ x + 1");
    assert!(result.errors.is_empty(), "{:?}", result.errors);

    let ExprKind::Binary(binary) = first_expr(&result) else {
        panic!("expected a binary expression: {:?}", first_expr(&result));
    };
    assert_eq!(binary.op, BinOp::Add);
    assert!(
        matches!(&binary.lhs.kind, ExprKind::Unary(unary)
            if unary.op == UnOp::RadicalSqrt
                && matches!(unary.operand.kind, ExprKind::Ident(_))),
        "left operand must be `√ x`: {:?}",
        binary.lhs.kind
    );
    assert!(
        matches!(binary.rhs.kind, ExprKind::Literal(Literal::Int(1))),
        "right operand must be the literal 1: {:?}",
        binary.rhs.kind
    );
}
