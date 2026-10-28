//! A statement that ends the whole `filum` loop from inside one lane.
//!
//! Run in order, `rumpe` and `redde` stop every later iteration; run in
//! parallel only the lane that reached them stops. Placement would change the
//! result, so both are refused by name. `perge` skips one lane and stays
//! legal, as do `rumpe`/`perge` aimed at a sequential loop inside the lane and
//! a `redde` inside a closure or a reduction term body (both return a value of
//! their own, not from the enclosing function).

use crate::{SemanticError, SemanticErrorKind};
use radix_hir::visit::{HirVisitor, walk_block, walk_expr, walk_stmt};
use radix_hir::{HirBlock, HirExpression, HirExpressionKind, HirStatement, HirStatementKind};

pub(super) const BREAK_ISSUE: &str = "break_in_thread_body";
pub(super) const RETURN_ISSUE: &str = "return_in_thread_body";

/// What a `rumpe` at this point would leave.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Thread,
    Sequential,
}

/// Walks the body of one `filum` loop.
pub(super) struct Ends {
    targets: Vec<Target>,
    pub(super) errors: Vec<SemanticError>,
}

impl Ends {
    /// Starts inside the body of the thread loop being checked.
    pub(super) fn new() -> Self {
        Self {
            targets: vec![Target::Thread],
            errors: Vec::new(),
        }
    }

    fn refuse(&mut self, issue: &'static str, stmt: &HirStatement) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::TypeMismatch, stmt.span)
                .with_arg("issue", issue),
        );
    }
}

impl HirVisitor for Ends {
    fn visit_block(&mut self, block: &HirBlock) {
        let scope = block
            .breakable
            .is_some_and(radix_hir::HirBreakableKind::is_scope);
        if scope {
            self.targets.push(Target::Sequential);
        }
        walk_block(self, block);
        if scope {
            self.targets.pop();
        }
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Rumpe if self.targets.last() == Some(&Target::Thread) => {
                self.refuse(BREAK_ISSUE, stmt);
            }
            HirStatementKind::Redde(_) => self.refuse(RETURN_ISSUE, stmt),
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            // A closure or a term body returns its own value.
            HirExpressionKind::Clausura(..)
            | HirExpressionKind::Reduction { .. }
            | HirExpressionKind::Scan { .. }
            | HirExpressionKind::Sort { .. } => {}
            HirExpressionKind::Itera {
                filum: Some(_),
                iter,
                body,
                ..
            } => {
                self.visit_expr(iter);
                self.targets.push(Target::Thread);
                self.visit_block(body);
                self.targets.pop();
            }
            HirExpressionKind::Itera { .. }
            | HirExpressionKind::IteraLockstep { .. }
            | HirExpressionKind::IteraRange { .. }
            | HirExpressionKind::Loop(_)
            | HirExpressionKind::Dum(..) => {
                self.targets.push(Target::Sequential);
                walk_expr(self, expr);
                self.targets.pop();
            }
            _ => walk_expr(self, expr),
        }
    }
}
