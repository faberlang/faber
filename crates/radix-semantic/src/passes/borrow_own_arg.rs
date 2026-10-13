//! `own` call arguments: what may be given away, and what is dead afterwards.
//!
//! Operator rulings 2026-10-08 (R-05, R-06):
//!
//! - Only a **whole local binding** may be passed as `own`. A field or element
//!   (`eat(own a.xs)`), an unmarked parameter, a module constant and an
//!   `itera` binder are named check errors; the caller writes `copy`. A fresh
//!   value and an `own`/`copy` parameter are fine.
//! - After `eat(own a)` the name `a` is dead. An exclusive (`own`/`copy`) source
//!   is moved, so a later read is SEM050 `use_after_move` at the read. An
//!   unmarked source is tentatively given away, exactly like `const T b ← own a`
//!   (`unmarked_own_binds`): a later use blames the **call argument** with
//!   `use_after_own` and a `copy` suggestion, not the later read.
//! - A move inside a loop body from a binding declared outside the loop moves
//!   the value once per iteration (`moved_in_loop`), unless the body can leave
//!   the loop.
//! - A closure that captured the name before the move and is called after it
//!   reads a dead value (`moved_name_captured_by_closure`).
//!
//! Branches (`si`, `discerne`) are merged: a move in one branch does not kill
//! the name in its sibling, and survives after the join unless the branch
//! diverges.

use super::BorrowChecker;
use crate::{SUGGESTION_COPY, SemanticError, SemanticErrorKind};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{
    DefId, HirArgMarker, HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirStatementKind,
};
use rustc_hash::{FxHashMap, FxHashSet};

pub(super) const ISSUE_OWN_ARG_NOT_LOCAL_BINDING: &str = "own_argument_not_local_binding";
pub(super) const ISSUE_OWN_ARG_UNMARKED_PARAMETER: &str = "own_argument_unmarked_parameter";
pub(super) const ISSUE_OWN_ARG_MODULE_CONSTANT: &str = "own_argument_module_constant";
pub(super) const ISSUE_OWN_ARG_LOOP_BINDER: &str = "own_argument_loop_binder";
const ISSUE_USE_AFTER_OWN: &str = "use_after_own";
const ISSUE_MOVED_IN_LOOP: &str = "moved_in_loop";
const ISSUE_MOVED_NAME_CAPTURED: &str = "moved_name_captured_by_closure";

/// State added by the `own` argument rules.
#[derive(Default)]
pub(super) struct OwnArgState {
    /// Module-level constants (survive per-function resets).
    module_consts: FxHashSet<DefId>,
    /// Bindings that may not be passed as `own`, with the issue naming why.
    unpassable: FxHashMap<DefId, &'static str>,
    /// Unmarked locals given away by `own`; a later use rejects the call.
    pending: FxHashMap<DefId, radix_lexer::Span>,
    /// Where each binding was last moved by an `own` argument or bind.
    move_spans: FxHashMap<DefId, radix_lexer::Span>,
    /// Outer names a closure local captured when it was created.
    closure_captures: FxHashMap<DefId, Vec<DefId>>,
}

impl OwnArgState {
    pub(super) fn reset(&mut self) {
        self.unpassable.clear();
        self.pending.clear();
        self.move_spans.clear();
        self.closure_captures.clear();
    }

    pub(super) fn note_module_const(&mut self, def_id: DefId) {
        self.module_consts.insert(def_id);
    }

    pub(super) fn note_unmarked_param(&mut self, def_id: DefId) {
        self.unpassable
            .insert(def_id, ISSUE_OWN_ARG_UNMARKED_PARAMETER);
    }

    pub(super) fn note_loop_binder(&mut self, def_id: DefId) {
        self.unpassable.insert(def_id, ISSUE_OWN_ARG_LOOP_BINDER);
    }
}

/// Dead-name facts at one program point, for branch merging.
pub(super) struct FlowMarks {
    moved: FxHashSet<DefId>,
    pending: FxHashMap<DefId, radix_lexer::Span>,
}

/// Loop-entry facts for the repeated-move check.
pub(super) struct LoopEntry {
    known: FxHashSet<DefId>,
    moved: FxHashSet<DefId>,
    pending: FxHashSet<DefId>,
}

impl BorrowChecker<'_> {
    /// Evaluate an argument marked `own`: validate the source and kill it.
    pub(super) fn own_argument(&mut self, arg: &HirCallArg) {
        let expr = &arg.expr;
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                let issue = if self.own.module_consts.contains(def_id) {
                    Some(ISSUE_OWN_ARG_MODULE_CONSTANT)
                } else {
                    self.own.unpassable.get(def_id).copied()
                };
                if let Some(issue) = issue {
                    self.error(SemanticErrorKind::ModeMismatch, issue, expr.span);
                    self.check_expr(expr);
                    return;
                }
                if self.arg_is_exclusive(expr) {
                    self.own.move_spans.insert(*def_id, expr.span);
                    self.move_from_expr(expr);
                } else {
                    self.check_expr(expr);
                    self.own.pending.insert(*def_id, expr.span);
                    self.own.move_spans.insert(*def_id, expr.span);
                }
            }
            HirExpressionKind::Field(..)
            | HirExpressionKind::Index(..)
            | HirExpressionKind::Deref(..)
            | HirExpressionKind::NonNull(..)
                if Self::root_def_id(expr).is_some() =>
            {
                self.error(
                    SemanticErrorKind::ModeMismatch,
                    ISSUE_OWN_ARG_NOT_LOCAL_BINDING,
                    expr.span,
                );
                self.check_expr(expr);
            }
            _ => self.check_expr(expr),
        }
    }

    /// Evaluate a call argument, applying the `own` rules when it is marked.
    pub(super) fn check_arg_expr(&mut self, arg: &HirCallArg) {
        if is_own_marked(arg) {
            self.own_argument(arg);
        } else {
            self.check_expr(&arg.expr);
        }
    }

    /// A use of a name an `own` argument gave away: blame the call argument.
    pub(super) fn note_live_own_arg(&mut self, def_id: DefId) {
        if let Some(span) = self.own.pending.remove(&def_id) {
            self.errors.push(
                SemanticError::structured(SemanticErrorKind::UseAfterMove, span)
                    .with_arg("issue", ISSUE_USE_AFTER_OWN)
                    .with_arg("suggestion", SUGGESTION_COPY),
            );
        }
    }

    /// Assigning a whole binding gives it a fresh value: it is live again.
    pub(super) fn revive_on_assign(&mut self, target: &HirExpression) {
        if let HirExpressionKind::Path(def_id) = &target.kind {
            self.own.pending.remove(def_id);
            self.own.move_spans.remove(def_id);
            if let Some(state) = self.states.get_mut(def_id) {
                state.moved = false;
            }
        }
    }

    pub(super) fn flow_marks(&self) -> FlowMarks {
        FlowMarks {
            moved: self
                .states
                .iter()
                .filter(|(_, state)| state.moved)
                .map(|(def_id, _)| *def_id)
                .collect(),
            pending: self.own.pending.clone(),
        }
    }

    pub(super) fn restore_marks(&mut self, marks: &FlowMarks) {
        for (def_id, state) in &mut self.states {
            state.moved = marks.moved.contains(def_id);
        }
        self.own.pending.clone_from(&marks.pending);
    }

    /// Union of the facts of every non-diverging branch.
    pub(super) fn join_marks(&mut self, branches: Vec<FlowMarks>) {
        let mut moved = FxHashSet::default();
        let mut pending = FxHashMap::default();
        for branch in branches {
            moved.extend(branch.moved);
            pending.extend(branch.pending);
        }
        self.restore_marks(&FlowMarks { moved, pending });
    }

    pub(super) fn loop_entry(&self) -> LoopEntry {
        LoopEntry {
            known: self.states.keys().copied().collect(),
            moved: self.flow_marks().moved,
            pending: self.own.pending.keys().copied().collect(),
        }
    }

    /// After one walk of a loop body: a name declared outside the loop that is
    /// dead at the end of the body is moved again on the next iteration.
    pub(super) fn finish_loop(&mut self, entry: &LoopEntry, body: &HirBlock) {
        if block_may_leave(body) {
            return;
        }
        let mut repeated: Vec<(DefId, radix_lexer::Span)> = Vec::new();
        for (def_id, state) in &self.states {
            if state.moved
                && !entry.moved.contains(def_id)
                && entry.known.contains(def_id)
                && let Some(span) = self.own.move_spans.get(def_id)
            {
                repeated.push((*def_id, *span));
            }
        }
        for (def_id, span) in &self.own.pending {
            if !entry.pending.contains(def_id) && entry.known.contains(def_id) {
                repeated.push((*def_id, *span));
            }
        }
        repeated.sort_by_key(|(_, span)| span.start);
        for (_, span) in repeated {
            self.error(SemanticErrorKind::UseAfterMove, ISSUE_MOVED_IN_LOOP, span);
        }
    }

    /// Record the outer names a closure bound to `closure` reads.
    pub(super) fn note_closure_captures(
        &mut self,
        closure: DefId,
        body: &HirExpression,
        known: &FxHashSet<DefId>,
    ) {
        let mut finder = PathCollector::default();
        finder.visit_expr(body);
        let captured: Vec<DefId> = finder
            .paths
            .into_iter()
            .filter(|def_id| known.contains(def_id) && *def_id != closure)
            .collect();
        if !captured.is_empty() {
            self.own.closure_captures.insert(closure, captured);
        }
    }

    /// Calling a closure after a name it captured was given away.
    pub(super) fn check_closure_call(&mut self, callee: &HirExpression, span: radix_lexer::Span) {
        let HirExpressionKind::Path(closure) = &callee.kind else {
            return;
        };
        let Some(captured) = self.own.closure_captures.get(closure) else {
            return;
        };
        let dead = captured.iter().any(|def_id| {
            self.own.pending.contains_key(def_id)
                || self.states.get(def_id).is_some_and(|state| state.moved)
        });
        if dead {
            self.error(
                SemanticErrorKind::UseAfterMove,
                ISSUE_MOVED_NAME_CAPTURED,
                span,
            );
        }
    }
}

/// The block ends in a statement that never falls through to the join.
pub(super) fn block_diverges(block: &HirBlock) -> bool {
    block.expr.is_none()
        && block.statements.last().is_some_and(|stmt| {
            matches!(
                stmt.kind,
                HirStatementKind::Redde(_) | HirStatementKind::Rumpe | HirStatementKind::Perge
            )
        })
}

/// The loop body contains a statement that can leave the loop or the function.
///
/// WHY: a `rumpe` inside a nested loop only leaves that inner loop, and a
/// `redde` inside a closure returns from the closure, so neither stops the
/// outer loop from running its body again.
fn block_may_leave(block: &HirBlock) -> bool {
    let mut finder = LeaveFinder {
        found: false,
        loop_depth: 0,
    };
    finder.visit_block(block);
    finder.found
}

struct LeaveFinder {
    found: bool,
    loop_depth: u32,
}

impl HirVisitor for LeaveFinder {
    fn visit_stmt(&mut self, stmt: &radix_hir::HirStatement) {
        let leaves = match stmt.kind {
            HirStatementKind::Redde(_) => true,
            HirStatementKind::Rumpe => self.loop_depth == 0,
            _ => false,
        };
        if leaves {
            self.found = true;
            return;
        }
        radix_hir::visit::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Clausura(..) => {}
            HirExpressionKind::Loop(_)
            | HirExpressionKind::Dum(..)
            | HirExpressionKind::Itera { .. }
            | HirExpressionKind::IteraLockstep { .. }
            | HirExpressionKind::IteraRange { .. } => {
                self.loop_depth += 1;
                walk_expr(self, expr);
                self.loop_depth -= 1;
            }
            _ => walk_expr(self, expr),
        }
    }
}

#[derive(Default)]
struct PathCollector {
    paths: Vec<DefId>,
}

impl HirVisitor for PathCollector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Path(def_id) = &expr.kind {
            self.paths.push(*def_id);
        }
        walk_expr(self, expr);
    }
}

/// Whether this call argument carries the `own` marker.
pub(super) fn is_own_marked(arg: &HirCallArg) -> bool {
    arg.marker == Some(HirArgMarker::Own)
}
