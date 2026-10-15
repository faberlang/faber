//! Membership glyphs `∈` / `∉` (membership-glyph unit 1b).
//!
//! Both sit in the comparison tier, left-associative with `≺ ≻ ≤ ≥`. The
//! parser only builds `BinOp::Member` / `BinOp::NotMember`; the checker picks
//! range or collection semantics from the right operand. The retired keyword
//! forms `intra` / `inter` are pinned in `membership_retired_test.rs`.

use crate::test_support::{parse_ok, parse_program};
use radix_syntax::{BinOp, Expr, ExprKind, StmtKind};

fn initializer(source: &str) -> ExprKind {
    let program = parse_ok(source).program.expect("program");
    let StmtKind::Var(decl) = program.statements.into_iter().next().expect("stmt").kind else {
        panic!("expected a binding");
    };
    let init: Expr = *decl.init.expect("initializer");
    init.kind
}

fn binary(source: &str) -> radix_syntax::BinaryExpr {
    let ExprKind::Binary(bin) = initializer(source) else {
        panic!("expected a binary expression for {source:?}");
    };
    bin
}

#[test]
fn element_of_parses_as_member() {
    let bin = binary("sit v ← x ∈ xs");
    assert!(matches!(bin.op, BinOp::Member));
    assert!(matches!(bin.lhs.kind, ExprKind::Ident(_)));
    assert!(matches!(bin.rhs.kind, ExprKind::Ident(_)));
}

#[test]
fn not_element_of_parses_as_not_member() {
    let bin = binary("sit v ← x ∉ xs");
    assert!(matches!(bin.op, BinOp::NotMember));
}

#[test]
fn element_of_accepts_a_range_right_operand() {
    let bin = binary("sit v ← n ∈ 0‥10");
    assert!(matches!(bin.op, BinOp::Member));
    assert!(matches!(bin.rhs.kind, ExprKind::Intervallum(_)));
}

#[test]
fn element_of_binds_looser_than_arithmetic() {
    let bin = binary("sit v ← a + 1 ∈ xs");
    assert!(matches!(bin.op, BinOp::Member));
    assert!(matches!(bin.lhs.kind, ExprKind::Binary(ref lhs) if matches!(lhs.op, BinOp::Add)));
}

#[test]
fn membership_chains_left_associative_with_comparison() {
    let bin = binary("sit v ← a ∈ xs ∉ ys");
    assert!(matches!(bin.op, BinOp::NotMember));
    assert!(matches!(bin.lhs.kind, ExprKind::Binary(ref lhs) if matches!(lhs.op, BinOp::Member)));
}

#[test]
fn membership_binds_tighter_than_logical_and() {
    let bin = binary("sit v ← a ∈ xs et b ∉ ys");
    assert!(matches!(bin.op, BinOp::And));
    assert!(matches!(bin.lhs.kind, ExprKind::Binary(ref lhs) if matches!(lhs.op, BinOp::Member)));
    assert!(
        matches!(bin.rhs.kind, ExprKind::Binary(ref rhs) if matches!(rhs.op, BinOp::NotMember))
    );
}

#[test]
fn element_of_without_a_right_operand_is_a_parse_error() {
    let result = parse_program("sit v ← x ∈");
    assert!(!result.errors.is_empty());
}
