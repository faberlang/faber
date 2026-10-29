//! Statement lowering into MIR block bodies and terminators.
//!
//! Statements are where expression lowering meets CFG construction. This file
//! owns the HIR visitor hooks that keep the current MIR block well-formed:
//! ordinary statements append work to the open block, `redde` seals it with a
//! return terminator, and loop-control statements delegate to the active loop
//! context. Once a terminator is emitted, later HIR in the same block is treated
//! as a lowering error instead of being appended after a sealed block.
//!
//! INVARIANTS
//! ==========
//! - Every emitted MIR statement belongs to an open block.
//! - Statement-level unsupported forms become diagnostics, not partial MIR.
//! - Assignment expressions are routed through place-aware lowering so the LHS
//!   remains addressable instead of becoming a value-only operand.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use radix_hir::visit::HirVisitor;

impl HirVisitor for FunctionBuilder<'_> {
    /// Lower a HIR block in source order until a MIR terminator seals the
    /// current block.
    fn visit_block(&mut self, block: &HirBlock) {
        if block.breakable.is_some() {
            self.lower_breakable_block(block, block.span);
            return;
        }

        for stmt in &block.statements {
            if !self.current_is_open() {
                break;
            }
            self.visit_stmt(stmt);
        }

        if let Some(expr) = &block.expr
            && self.current_is_open()
        {
            self.visit_expr(expr);
        }
    }

    /// Lower one HIR statement into the current MIR block.
    ///
    /// The visitor only accepts statement forms whose MIR contracts are already
    /// represented. Provider blocks still fail closed because they require later
    /// effect lowering policy.
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if !self.current_is_open() {
            self.errors
                .push(MirError::unsupported(stmt.span, "statement after a MIR terminator"));
            return;
        }

        match &stmt.kind {
            HirStatementKind::Local(local) => {
                self.lower_local(local, stmt.span);
            }
            HirStatementKind::Expr(expr) => self.visit_expr(expr),
            HirStatementKind::Redde(Some(expr)) => {
                // Inside a `reducta via summa … filum` captured term body (SFR-5),
                // `redde` yields the term value: store it into the capture
                // local and exit the body — the statements belong to the
                // filum kernel-plan entry, never the inline kernel flow.
                if let Some(capture) = self.filum_capture {
                    let Some(value) = self.lower_expr_value(expr) else {
                        return;
                    };
                    self.assign(MirPlace::local(capture.term_result), value, capture.ty, stmt.span);
                    match self
                        .breakables
                        .last()
                        .and_then(|context| context.perge_target)
                    {
                        Some(exit_target) => {
                            self.terminate_current(MirTerminatorKind::Goto(exit_target), stmt.span);
                        }
                        None => self
                            .errors
                            .push(MirError::unsupported(stmt.span, "filum redde outside its captured term body")),
                    }
                    return;
                }
                if self.reduction_fold.is_some() {
                    let Some(value) = self.lower_expr_value(expr) else {
                        return;
                    };
                    self.fold_reduction_value(value, stmt.span);
                    if let Some(target) = self.breakables.last().and_then(|c| c.perge_target) {
                        self.terminate_open_current(MirTerminatorKind::Goto(target), stmt.span);
                    }
                    return;
                }
                if let Some(value) = self.lower_return_expr(expr) {
                    self.emit_function_return_shape_check(expr.id.0, value.clone(), stmt.span);
                    self.emit_device_exits_to(0, stmt.span);
                    self.terminate_current(MirTerminatorKind::Return(Some(value)), stmt.span);
                }
            }
            HirStatementKind::Redde(None) => {
                if self.filum_capture.is_some() {
                    self.errors
                        .push(MirError::unsupported(stmt.span, "filum reduction redde without a term value"));
                    return;
                }
                self.emit_device_exits_to(0, stmt.span);
                self.terminate_current(MirTerminatorKind::Return(None), stmt.span);
            }
            HirStatementKind::ReddeFilum(_) => self.errors.push(MirError {
                message: "return thread lowering is pending".to_owned(),
                issue: "return_thread_lowering_pending".to_owned(),
                span: stmt.span,
                definition_span: None,
            }),
            HirStatementKind::Rumpe => self.lower_rumpe(stmt.span),
            HirStatementKind::Perge => self.lower_perge(stmt.span),
            HirStatementKind::Tacet => {}
            HirStatementKind::IncDec(inc_dec) => self.lower_inc_dec_stmt(stmt.span, inc_dec),
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    if !self.current_is_open() {
                        break;
                    }
                    self.lower_si_stmt(&clause.cond, &clause.body, None, None, clause.span);
                }
            }
        }
        if !matches!(stmt.kind, HirStatementKind::Tacet) {
            self.deliver_after_statement(stmt.span);
        }
    }

    /// Lower a value-position expression used as a statement.
    ///
    /// Assignments are special because their left-hand side must remain a
    /// `MirPlace`; every other expression can be lowered for its value and then
    /// discarded if the caller does not need the operand.
    fn visit_expr(&mut self, expr: &HirExpression) {
        match expr.kind {
            HirExpressionKind::Assign(_, _) => {
                self.lower_assignment_expr(expr);
            }
            HirExpressionKind::ConversioAssign { .. } => {
                self.lower_conversio_assign_expr(expr);
            }
            _ => {
                self.lower_expr_value(expr);
            }
        }
    }
}
