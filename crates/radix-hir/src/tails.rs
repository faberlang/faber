//! Structural branch-tail enumeration.
//!
//! One place names which expression forms yield a value from a branch: a
//! block's tail, an `si`'s then and else tails, and every `discerne` arm
//! body. Typecheck (where a decimal store rounds) and codegen (where an
//! exact decimal stays exact) both ask this enumerator, so the two decisions
//! cannot drift apart.

use crate::{HirExpression, HirExpressionKind};

#[cfg(test)]
#[path = "tails_test.rs"]
mod tests;

/// The expressions that supply the value of a branching `expr`.
///
/// A [`HirExpressionKind::Block`] yields its tail expression, if any. An
/// [`HirExpressionKind::Si`] yields its then tail, then its else tail when an
/// `else` block exists. A [`HirExpressionKind::Discerne`] yields every arm
/// body. Every other expression yields nothing. The enumeration is one level
/// deep: a tail that is itself branching is returned as that tail, and the
/// caller recurses.
#[must_use]
pub fn branch_tails(expr: &HirExpression) -> Vec<&HirExpression> {
    match &expr.kind {
        HirExpressionKind::Block(block) => block.expr.as_deref().into_iter().collect(),
        HirExpressionKind::Si {
            then_block,
            else_block,
            ..
        } => then_block
            .expr
            .as_deref()
            .into_iter()
            .chain(else_block.iter().filter_map(|block| block.expr.as_deref()))
            .collect(),
        HirExpressionKind::Discerne { arms, .. } => arms.iter().map(|arm| &arm.body).collect(),
        _ => Vec::new(),
    }
}
