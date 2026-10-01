//! Truth table for the block termination predicate.

use super::*;
use radix_hir::{
    HirBreakableKind, HirCustodi, HirCustodiClause, HirExpression, HirId, HirLiteral, HirStatement,
};
use radix_lexer::Span;

fn span() -> Span {
    Span::new(0, 0)
}

fn expr(kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: HirId(1),
        kind,
        ty: None,
        span: span(),
    }
}

fn int() -> HirExpression {
    expr(HirExpressionKind::Literal(HirLiteral::Int(0)))
}

fn stmt(kind: HirStatementKind) -> HirStatement {
    HirStatement {
        id: HirId(2),
        kind,
        span: span(),
    }
}

fn block(statements: Vec<HirStatement>) -> HirBlock {
    HirBlock {
        statements,
        expr: None,
        span: span(),
        breakable: None,
        end: None,
    }
}

fn redde() -> HirStatement {
    stmt(HirStatementKind::Redde(Some(int())))
}

fn expr_stmt(kind: HirExpressionKind) -> HirStatement {
    stmt(HirStatementKind::Expr(expr(kind)))
}

fn iace() -> HirExpression {
    expr(HirExpressionKind::Throw(Box::new(int())))
}

fn mori() -> HirExpression {
    expr(HirExpressionKind::Panic(Box::new(int())))
}

fn terminating() -> HirBlock {
    block(vec![redde()])
}

fn falling() -> HirBlock {
    block(vec![stmt(HirStatementKind::Tacet)])
}

fn si(then_block: HirBlock, else_block: Option<HirBlock>) -> HirExpressionKind {
    HirExpressionKind::Si {
        cond: Box::new(expr(HirExpressionKind::Literal(HirLiteral::Bool(true)))),
        then_block,
        then_catch: None,
        else_block,
    }
}

#[test]
fn redde_terminates() {
    assert!(block_always_terminates(&block(vec![redde()])));
    assert!(block_always_terminates(&block(vec![
        stmt(HirStatementKind::Tacet),
        stmt(HirStatementKind::Redde(None)),
    ])));
}

#[test]
fn iace_and_mori_terminate() {
    assert!(block_always_terminates(&block(vec![stmt(
        HirStatementKind::Expr(iace())
    )])));
    assert!(block_always_terminates(&block(vec![stmt(
        HirStatementKind::Expr(mori())
    )])));
}

#[test]
fn tail_expression_throw_terminates() {
    let mut b = block(vec![]);
    b.expr = Some(Box::new(iace()));
    assert!(block_always_terminates(&b));
}

#[test]
fn nested_terminating_block_terminates() {
    let inner = expr_stmt(HirExpressionKind::Block(terminating()));
    assert!(block_always_terminates(&block(vec![inner])));
}

#[test]
fn nested_falling_block_does_not_terminate() {
    let inner = expr_stmt(HirExpressionKind::Block(falling()));
    assert!(!block_always_terminates(&block(vec![inner])));
}

#[test]
fn si_with_both_arms_terminating_terminates() {
    let s = expr_stmt(si(terminating(), Some(terminating())));
    assert!(block_always_terminates(&block(vec![s])));
}

#[test]
fn si_chain_terminates_only_when_every_arm_does() {
    let tail_else = |inner: HirBlock| {
        let mut b = block(vec![]);
        b.expr = Some(Box::new(expr(si(inner, Some(terminating())))));
        b
    };
    let all = expr_stmt(si(terminating(), Some(tail_else(terminating()))));
    assert!(block_always_terminates(&block(vec![all])));
    let one_open = expr_stmt(si(terminating(), Some(tail_else(falling()))));
    assert!(!block_always_terminates(&block(vec![one_open])));
}

#[test]
fn si_without_else_does_not_terminate() {
    let s = expr_stmt(si(terminating(), None));
    assert!(!block_always_terminates(&block(vec![s])));
}

#[test]
fn si_with_one_open_arm_does_not_terminate() {
    let then_open = expr_stmt(si(falling(), Some(terminating())));
    assert!(!block_always_terminates(&block(vec![then_open])));
    let else_open = expr_stmt(si(terminating(), Some(falling())));
    assert!(!block_always_terminates(&block(vec![else_open])));
}

#[test]
fn si_with_cape_handler_does_not_terminate() {
    let HirExpressionKind::Si {
        cond,
        then_block,
        else_block,
        ..
    } = si(terminating(), Some(terminating()))
    else {
        unreachable!()
    };
    let handler = radix_hir::HirCape {
        binding_def_id: radix_hir::DefId(0),
        binding_name: radix_lexer::Symbol(0),
        binding_ty: None,
        body: falling(),
        span: span(),
    };
    let s = expr_stmt(HirExpressionKind::Si {
        cond,
        then_block,
        then_catch: Some(Box::new(handler)),
        else_block,
    });
    assert!(!block_always_terminates(&block(vec![s])));
}

#[test]
fn empty_and_plain_blocks_do_not_terminate() {
    assert!(!block_always_terminates(&block(vec![])));
    assert!(!block_always_terminates(&falling()));
    assert!(!block_always_terminates(&block(vec![stmt(
        HirStatementKind::Rumpe
    ),])));
    assert!(!block_always_terminates(&block(vec![stmt(
        HirStatementKind::Perge
    ),])));
}

#[test]
fn loop_with_rumpe_does_not_terminate() {
    let looped = block(vec![stmt(HirStatementKind::Rumpe)]);
    let s = expr_stmt(HirExpressionKind::Loop(looped));
    assert!(!block_always_terminates(&block(vec![s])));
}

#[test]
fn loop_without_rumpe_is_still_not_terminating() {
    let s = expr_stmt(HirExpressionKind::Loop(falling()));
    assert!(!block_always_terminates(&block(vec![s])));
}

#[test]
fn custodi_does_not_terminate() {
    let clause = HirCustodiClause {
        cond: int(),
        body: terminating(),
        span: span(),
    };
    let s = stmt(HirStatementKind::Custodi(HirCustodi {
        clauses: vec![clause],
    }));
    assert!(!block_always_terminates(&block(vec![s])));
}

#[test]
fn breakable_block_with_rumpe_does_not_terminate() {
    let mut b = block(vec![
        expr_stmt(si(block(vec![stmt(HirStatementKind::Rumpe)]), None)),
        redde(),
    ]);
    b.breakable = Some(HirBreakableKind::Fac);
    assert!(!block_always_terminates(&b));
}

#[test]
fn breakable_block_without_rumpe_follows_last_statement() {
    let mut b = block(vec![redde()]);
    b.breakable = Some(HirBreakableKind::Fac);
    assert!(block_always_terminates(&b));
}
