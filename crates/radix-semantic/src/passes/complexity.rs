//! Per-function cyclomatic-complexity analysis over typed HIR.
//!
//! Score = 1 (base) + decision points, per declared function with a body.
//! The counting rules are the contract in
//! `docs/factory/complexity-diagnostics/goal.md` §Proposal; the row-per-row
//! tests in `complexity_test.rs` pin every row. Closures are not declared
//! functions: their bodies fold into the enclosing function's score (the
//! HIR walk does not stop at `Clausura` boundaries).
//!
//! The pass is target-independent (reads only HIR) and warn-only: findings
//! are [`SemanticErrorKind::Warning`] and cannot fail compilation unless the
//! caller's deny policy promotes them.

use crate::{SemanticError, SemanticErrorKind, WarningKind};
use radix_hir::visit::{walk_block, walk_expr, walk_stmt, HirVisitor};
use radix_hir::{
    HirBinOp, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirModule,
    HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Span};

const ISSUE_COMPLEXITY_BUDGET_EXCEEDED: &str = "complexity_budget_exceeded";

/// Baked per-function complexity budget. Provisional (order of 10 per the
/// operator need); the CC-U4 census fixes the final value in CC-U5.
pub const DEFAULT_COMPLEXITY_BUDGET: u32 = 10;

/// Cyclomatic score of one declared function, or `None` when it has no body
/// (bodyless interface methods are skipped, not scored).
#[must_use]
pub fn function_complexity(func: &HirFunction) -> Option<u32> {
    func.body
        .as_ref()
        .map(|body| 1 + block_decision_points(body))
}

fn block_decision_points(block: &HirBlock) -> u32 {
    let mut counter = DecisionPointCounter(0);
    walk_block(&mut counter, block);
    counter.0
}

/// Decision-point counter over one function body.
struct DecisionPointCounter(u32);

impl HirVisitor for DecisionPointCounter {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            // Conditional: +1 each; an attached error-channel catch (`then_catch`)
            // is its own decision point on the same node.
            HirExpressionKind::Si { then_catch, .. } => {
                self.0 += 1 + u32::from(then_catch.is_some());
            }
            // Match/value dispatch: +1 per match, never per arm.
            HirExpressionKind::Discerne { .. } => self.0 += 1,
            HirExpressionKind::Loop(_)
            | HirExpressionKind::Dum(..)
            | HirExpressionKind::Itera(..)
            | HirExpressionKind::IteraLockstep { .. }
            | HirExpressionKind::IteraRange { .. } => self.0 += 1,
            HirExpressionKind::Binary(
                HirBinOp::And | HirBinOp::Or | HirBinOp::Coalesce,
                _,
                _,
            ) => self.0 += 1,
            HirExpressionKind::Handled { .. } => self.0 += 1,
            HirExpressionKind::Trap(_) => self.0 += 1,
            // Body-carrying folds/selections: +1 each.
            HirExpressionKind::Summa { .. } | HirExpressionKind::FirstMatch { .. } => self.0 += 1,
            // No-body fold (`Extremum`): 0; only its `vel` coalesce tail decides.
            HirExpressionKind::Extremum { coalesce, .. } => self.0 += u32::from(coalesce.is_some()),
            // Conversion recovery tails: +1 only when the `⇥` fallback is present.
            HirExpressionKind::ConversioAssign { recovery, .. } => {
                self.0 += u32::from(recovery.is_some());
            }
            HirExpressionKind::Conversio { recovery, .. } => {
                self.0 += u32::from(recovery.is_some());
            }
            _ => {}
        }
        walk_expr(self, expr);
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        // Guard clauses: +1 per `custodi` clause.
        if let HirStatementKind::Custodi(custodi) = &stmt.kind {
            self.0 += u32::try_from(custodi.clauses.len()).unwrap_or(u32::MAX);
        }
        walk_stmt(self, stmt);
    }
}

/// Warn once per declared function whose score exceeds `budget`.
///
/// Scores top-level functions and struct methods (both carry a
/// [`HirFunction`]); bodyless interface methods and compiler-synthesized
/// items (def ids ≥ 1_000_000, e.g. lifted AIR closures) are skipped.
#[must_use]
pub fn complexity(hir: &HirModule, interner: &Interner, budget: u32) -> Vec<SemanticError> {
    let mut findings = Vec::new();
    let mut consider = |func: &HirFunction, span: Span| {
        if let Some(score) = function_complexity(func) {
            if score > budget {
                findings.push(
                    SemanticError::structured(
                        SemanticErrorKind::Warning(WarningKind::ComplexityBudgetExceeded),
                        span,
                    )
                    .with_arg("issue", ISSUE_COMPLEXITY_BUDGET_EXCEEDED)
                    .with_arg("function", interner.resolve(func.name))
                    .with_arg("score", score.to_string())
                    .with_arg("budget", budget.to_string()),
                );
            }
        }
    };

    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(func) if item.def_id.0 < 1_000_000 => {
                consider(func, item.span);
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    consider(&method.func, method.span);
                }
            }
            _ => {}
        }
    }

    findings
}
