//! Termination predicate for HIR blocks.
//!
//! One definition of "this block never falls through", shared by every
//! backend that drops dead trailing code after a block that already returned
//! or threw. The predicate reads HIR only and is deliberately conservative: a
//! false negative keeps the code that is there today, a false positive would
//! delete live code.
//!
//! A block terminates when its last element does: `redde`, an `iace`/`mori`
//! expression, a nested plain block that terminates, or an `si` with an
//! `else` whose two arms both terminate. Loops, `discerne`, and `custodi` are
//! never treated as terminating, and a breakable block that contains `rumpe`
//! can leave early and so does not terminate.

use crate::breakable::block_contains_rumpe;
use radix_hir::{HirBlock, HirExpression, HirExpressionKind, HirStatementKind};

#[cfg(test)]
#[path = "terminates_test.rs"]
mod tests;

/// Whether control can never reach the end of `block`.
#[must_use]
pub fn block_always_terminates(block: &HirBlock) -> bool {
    if block.breakable.is_some() && block_contains_rumpe(block) {
        return false;
    }
    if let Some(tail) = &block.expr {
        return expression_always_terminates(tail);
    }
    match block.statements.last() {
        Some(stmt) => match &stmt.kind {
            HirStatementKind::Redde(_) => true,
            HirStatementKind::Expr(expr) => expression_always_terminates(expr),
            _ => false,
        },
        None => false,
    }
}

fn expression_always_terminates(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Throw(_) | HirExpressionKind::Panic(_) => true,
        HirExpressionKind::Block(block) => block_always_terminates(block),
        HirExpressionKind::Si {
            then_block,
            then_catch: None,
            else_block: Some(else_block),
            ..
        } => block_always_terminates(then_block) && block_always_terminates(else_block),
        _ => false,
    }
}
