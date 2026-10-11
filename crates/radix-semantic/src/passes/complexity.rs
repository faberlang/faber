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
use radix_hir::visit::{HirVisitor, walk_block, walk_expr, walk_stmt};
use radix_hir::{
    HirBinOp, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirModule,
    HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Span};

const ISSUE_COMPLEXITY_BUDGET_EXCEEDED: &str = "complexity_budget_exceeded";

/// Baked per-function complexity budget, fixed by the CC-U4 census (CC-U5):
/// `docs/factory/complexity-diagnostics/census-report.md` recommends 12 —
/// the nearest candidate to the measured pooled p95 = 11, firing on the top
/// 4.4% of the 2393 scored functions.
pub const DEFAULT_COMPLEXITY_BUDGET: u32 = 12;

/// Baked complexity budget for `@ nucleum` kernel entries
/// ([`HirFunction::is_kernel`]), fixed by the CC-U4 census kernel subset:
/// `docs/factory/complexity-diagnostics/census-report.md` recommends 2 —
/// all 84 measured kernel functions score ≤ 2 (six at 2), so the budget
/// fires on nothing today and trips any future kernel scoring 3+.
/// Advisory-only: it warns, it never gates kernel admission.
pub const DEFAULT_KERNEL_COMPLEXITY_BUDGET: u32 = 2;

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
            // Match/value dispatch (+1 per match, never per arm), loops,
            // short-circuit/coalesce operators, and error-handling forms: +1 each.
            HirExpressionKind::Discerne { .. }
            | HirExpressionKind::Loop(_)
            | HirExpressionKind::Dum(..)
            | HirExpressionKind::Itera { .. }
            | HirExpressionKind::IteraLockstep { .. }
            | HirExpressionKind::IteraRange { .. }
            | HirExpressionKind::Handled { .. }
            | HirExpressionKind::Trap(_)
            | HirExpressionKind::CallDefault { .. }
            | HirExpressionKind::FirstMatch { .. } => self.0 += 1,
            HirExpressionKind::Binary(HirBinOp::And | HirBinOp::Or | HirBinOp::Coalesce, _, _) => {
                self.0 += 1;
            }
            // Body-carrying folds/selections: +1 each.
            HirExpressionKind::Reduction { body, identity, .. } => {
                self.0 += u32::from(body.is_some()) + u32::from(identity.is_some());
            }
            // Conversion recovery tails: +1 only when the `⊥` fallback is present.
            HirExpressionKind::ConversioAssign { recovery, .. }
            | HirExpressionKind::Conversio { recovery, .. } => {
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

/// Warn once per declared function whose score exceeds its applicable
/// budget: `budget` for ordinary functions, `kernel_budget` for
/// `@ nucleum` kernel entries ([`HirFunction::is_kernel`]). The kernel
/// budget is advisory-only (warn, never admission); its findings carry a
/// `kernel` named arg and report the kernel budget in `budget`.
///
/// Scores top-level functions and struct methods (both carry a
/// [`HirFunction`]); bodyless interface methods and compiler-synthesized
/// items (HIR-synthetic def ids, ≥ [`radix_types::HIR_SYNTHETIC_DEF_ID_BASE`],
/// e.g. lifted AIR closures) are skipped.
#[must_use]
pub fn complexity(
    hir: &HirModule,
    interner: &Interner,
    budget: u32,
    kernel_budget: u32,
) -> Vec<SemanticError> {
    let mut findings = Vec::new();
    let mut consider = |func: &HirFunction, span: Span| {
        if let Some(score) = function_complexity(func) {
            let applicable = if func.is_kernel {
                kernel_budget
            } else {
                budget
            };
            if score > applicable {
                let mut finding = SemanticError::structured(
                    SemanticErrorKind::Warning(WarningKind::ComplexityBudgetExceeded),
                    span,
                )
                .with_arg("issue", ISSUE_COMPLEXITY_BUDGET_EXCEEDED)
                .with_arg("function", interner.resolve(func.name))
                .with_arg("score", score.to_string())
                .with_arg("budget", applicable.to_string());
                if func.is_kernel {
                    finding = finding.with_arg("kernel", "true");
                }
                findings.push(finding);
            }
        }
    };

    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(func) if !item.def_id.is_hir_synthetic() => {
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
