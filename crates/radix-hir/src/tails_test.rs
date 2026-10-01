//! Pins the branch-tail enumerator: the set it yields is the set the decimal
//! store decision (`carries_exact_decimal`) walks.

use super::branch_tails;
use crate::{
    HirBlock, HirCasuArm, HirExpression, HirExpressionKind, HirId, HirLiteral, HirPattern,
};
use radix_lexer::Span;

fn lit(id: u32) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(u64::from(id))),
        ty: None,
        span: Span::default(),
    }
}

fn expr(id: u32, kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind,
        ty: None,
        span: Span::default(),
    }
}

fn block(tail: Option<HirExpression>) -> HirBlock {
    HirBlock::simple(Vec::new(), tail.map(Box::new), Span::default())
}

fn si(id: u32, then_tail: u32, else_tail: Option<u32>) -> HirExpression {
    expr(
        id,
        HirExpressionKind::Si {
            cond: Box::new(lit(900)),
            then_block: block(Some(lit(then_tail))),
            then_catch: None,
            else_block: else_tail.map(|tail| block(Some(lit(tail)))),
        },
    )
}

fn ids(tails: &[&HirExpression]) -> Vec<u32> {
    tails.iter().map(|tail| tail.id.0).collect()
}

#[test]
fn block_yields_its_tail_only() {
    let with_tail = expr(1, HirExpressionKind::Block(block(Some(lit(2)))));
    assert_eq!(ids(&branch_tails(&with_tail)), vec![2]);
    let without_tail = expr(3, HirExpressionKind::Block(block(None)));
    assert!(branch_tails(&without_tail).is_empty());
}

#[test]
fn si_yields_then_and_else_tails() {
    assert_eq!(ids(&branch_tails(&si(1, 2, Some(3)))), vec![2, 3]);
}

#[test]
fn si_without_else_yields_only_the_then_tail() {
    assert_eq!(ids(&branch_tails(&si(1, 2, None))), vec![2]);
}

#[test]
fn discerne_yields_every_arm_body() {
    let arm = |id: u32| HirCasuArm {
        patterns: vec![HirPattern::Wildcard],
        body: lit(id),
        span: Span::default(),
    };
    let discerne = expr(
        1,
        HirExpressionKind::Discerne {
            scrutinees: vec![lit(900)],
            arms: vec![arm(2), arm(3), arm(4)],
            exhaustive: true,
        },
    );
    assert_eq!(ids(&branch_tails(&discerne)), vec![2, 3, 4]);
}

#[test]
fn nesting_is_one_level_deep() {
    let nested = expr(1, HirExpressionKind::Block(block(Some(si(2, 3, Some(4))))));
    let outer = branch_tails(&nested);
    assert_eq!(ids(&outer), vec![2]);
    assert_eq!(ids(&branch_tails(outer[0])), vec![3, 4]);
}

#[test]
fn non_branching_expressions_yield_nothing() {
    assert!(branch_tails(&lit(1)).is_empty());
}
