//! Reject unsupported live `tuus.cursor()` value escapes before codegen.
//!
//! Boundary: post-typecheck HIR pass. Walks HIR to prohibit live cursor
//! values from escaping their declaring scope via return, call arguments,
//! struct fields, aggregate literals, conversion, copy, or reassignment.
//! The only permitted expression use is `itera ex <cursor>`. Collect
//! produces the live-cursor set; this pass enforces its confinement.

use crate::live_tuus_cursor::{collect_live_tuus_cursor_locals, tuus_cursor_method_payload};
use crate::{Resolver, SemanticError, SemanticErrorKind, TypeId, TypeTable};
use handroll::FxHashMap;
use radix_hir::visit::{HirVisitor, walk_expr, walk_stmt};
use radix_hir::{
    DefId, HirArrayElement, HirBlock, HirConversioTarget, HirExpression, HirExpressionKind,
    HirLocal, HirModule, HirObjectField, HirStatement, HirStatementKind,
};
use radix_lexer::Interner;

const ISSUE_CALL_ARG: &str = "live_tuus_cursor_call_arg";
const ISSUE_RETURN: &str = "live_tuus_cursor_return";
const ISSUE_STRUCT: &str = "live_tuus_cursor_struct";
const ISSUE_REASSIGN: &str = "live_tuus_cursor_reassign";
const ISSUE_COPY: &str = "live_tuus_cursor_copy";
const ISSUE_CONVERSIO: &str = "live_tuus_cursor_conversio";
const ISSUE_DIRECT_INITIALIZER: &str = "live_tuus_cursor_direct_initializer";
const ISSUE_VALUE: &str = "live_tuus_cursor_value";

/// Check that live `tuus` cursors do not escape their declaring scope.
///
/// # Errors
///
/// Returns [`SemanticError`]s when a live cursor escapes via direct
/// initializer, value extraction, or unsupported conversion.
pub fn analyze(
    hir: &HirModule,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
) -> Result<(), Vec<SemanticError>> {
    let tuus_def_id = crate::builtins::frame_types::tuus_def_id(resolver);
    let live_locals = collect_live_tuus_cursor_locals(
        hir,
        tuus_def_id,
        &|symbol| interner.resolve(symbol),
        types,
    );

    let mut checker = LiveTuusCursorEscapeChecker {
        tuus_def_id,
        live_locals,
        interner,
        types,
        errors: Vec::new(),
        context_stack: vec![EscapeContext::Value],
    };
    checker.visit_program(hir);
    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

struct LiveTuusCursorEscapeChecker<'a> {
    tuus_def_id: Option<DefId>,
    live_locals: FxHashMap<DefId, TypeId>,
    interner: &'a Interner,
    types: &'a TypeTable,
    errors: Vec<SemanticError>,
    context_stack: Vec<EscapeContext>,
}

impl LiveTuusCursorEscapeChecker<'_> {
    fn resolve_symbol(&self, symbol: radix_lexer::Symbol) -> &str {
        self.interner.resolve(symbol)
    }

    fn current_context(&self) -> EscapeContext {
        *self.context_stack.last().unwrap_or(&EscapeContext::Value)
    }

    fn push_context(&mut self, context: EscapeContext) {
        self.context_stack.push(context);
    }

    fn pop_context(&mut self) {
        self.context_stack.pop();
    }

    fn with_context<F>(&mut self, context: EscapeContext, visit: F)
    where
        F: FnOnce(&mut Self),
    {
        self.push_context(context);
        visit(self);
        self.pop_context();
    }

    fn is_tuus_cursor_method(&self, expr: &HirExpression) -> bool {
        tuus_cursor_method_payload(
            expr,
            self.tuus_def_id,
            |symbol| self.resolve_symbol(symbol),
            self.types,
        )
        .is_some()
    }

    fn is_live_local_path(&self, expr: &HirExpression) -> bool {
        let HirExpressionKind::Path(def_id) = &expr.kind else {
            return false;
        };
        self.live_locals.contains_key(def_id)
    }

    fn check_expr_escape(&mut self, expr: &HirExpression) {
        let context = self.current_context();

        if self.is_tuus_cursor_method(expr) {
            if !matches!(
                context,
                EscapeContext::LocalInit | EscapeContext::IteraSource
            ) {
                self.error(Self::issue_for_tuus_cursor_escape(context), expr.span);
            }
            return;
        }

        if self.is_live_local_path(expr) && !matches!(context, EscapeContext::IteraSource) {
            self.error(Self::issue_for_bound_cursor_escape(context), expr.span);
        }
    }

    fn issue_for_tuus_cursor_escape(context: EscapeContext) -> &'static str {
        match context {
            EscapeContext::CallArg => ISSUE_CALL_ARG,
            EscapeContext::Return => ISSUE_RETURN,
            EscapeContext::StructField => ISSUE_STRUCT,
            EscapeContext::Conversio => ISSUE_CONVERSIO,
            _ => ISSUE_DIRECT_INITIALIZER,
        }
    }

    fn issue_for_bound_cursor_escape(context: EscapeContext) -> &'static str {
        match context {
            EscapeContext::CallArg => ISSUE_CALL_ARG,
            EscapeContext::Return => ISSUE_RETURN,
            EscapeContext::StructField | EscapeContext::AggregateElement => ISSUE_STRUCT,
            EscapeContext::Conversio => ISSUE_CONVERSIO,
            EscapeContext::AssignTarget => ISSUE_REASSIGN,
            _ => ISSUE_VALUE,
        }
    }

    fn error(&mut self, issue: &str, span: radix_lexer::Span) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                .with_arg("issue", issue),
        );
    }
}

impl HirVisitor for LiveTuusCursorEscapeChecker<'_> {
    fn visit_block(&mut self, block: &HirBlock) {
        for stmt in &block.statements {
            self.visit_stmt(stmt);
        }
        if let Some(expr) = &block.expr {
            self.with_context(EscapeContext::Value, |checker| checker.visit_expr(expr));
        }
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Local(local) => self.visit_local(local),
            HirStatementKind::Expr(expr) => {
                self.with_context(EscapeContext::Stmt, |checker| checker.visit_expr(expr));
            }
            HirStatementKind::Redde(expr) => {
                if let Some(value) = expr {
                    self.with_context(EscapeContext::Return, |checker| checker.visit_expr(value));
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_local(&mut self, local: &HirLocal) {
        if let Some(init) = &local.init {
            if self.is_live_local_path(init) {
                self.error(ISSUE_COPY, init.span);
            }
            self.with_context(EscapeContext::LocalInit, |checker| checker.visit_expr(init));
        }
    }

    fn visit_object_field(&mut self, field: &HirObjectField) {
        if let Some(value) = &field.value {
            self.with_context(EscapeContext::StructField, |checker| {
                checker.visit_expr(value);
            });
        }
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        self.check_expr_escape(expr);

        match &expr.kind {
            HirExpressionKind::Assign(target, value) => {
                if self.is_live_local_path(target) {
                    self.error(ISSUE_REASSIGN, expr.span);
                }
                if self.is_live_local_path(value) || self.is_tuus_cursor_method(value) {
                    if matches!(target.kind, HirExpressionKind::Path(_))
                        && !self.is_live_local_path(target)
                    {
                        self.error(ISSUE_COPY, expr.span);
                    } else if !matches!(target.kind, HirExpressionKind::Path(_)) {
                        self.error(ISSUE_STRUCT, expr.span);
                    }
                }
                self.with_context(EscapeContext::AssignTarget, |checker| {
                    checker.visit_expr(target);
                });
                self.with_context(EscapeContext::Value, |checker| checker.visit_expr(value));
            }
            // `↤` stores through the target place like ordinary assignment: a
            // live-cursor destination is a reassignment, and a live-cursor or
            // `tuus.cursor()` source copied into a non-path place is an escape.
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                ..
            } => {
                if self.is_live_local_path(target) {
                    self.error(ISSUE_REASSIGN, expr.span);
                }
                if self.is_live_local_path(source) || self.is_tuus_cursor_method(source) {
                    if matches!(target.kind, HirExpressionKind::Path(_))
                        && !self.is_live_local_path(target)
                    {
                        self.error(ISSUE_COPY, expr.span);
                    } else if !matches!(target.kind, HirExpressionKind::Path(_)) {
                        self.error(ISSUE_STRUCT, expr.span);
                    }
                }
                self.with_context(EscapeContext::AssignTarget, |checker| {
                    checker.visit_expr(target);
                });
                self.with_context(EscapeContext::Value, |checker| checker.visit_expr(source));
                if let Some(recovery) = recovery {
                    self.with_context(EscapeContext::Value, |checker| checker.visit_expr(recovery));
                }
            }
            HirExpressionKind::Call(callee, _, args, _) => {
                self.visit_expr(callee);
                for arg in args {
                    self.with_context(EscapeContext::CallArg, |checker| {
                        checker.visit_expr(&arg.expr);
                    });
                }
            }
            HirExpressionKind::MethodCall(receiver, _, _, args, _) => {
                self.visit_expr(receiver);
                for arg in args {
                    self.with_context(EscapeContext::CallArg, |checker| {
                        checker.visit_expr(&arg.expr);
                    });
                }
            }
            HirExpressionKind::Itera {
                binding: def_id,
                binding_name: name,
                iter,
                body: block,
                ..
            } => {
                self.visit_def(*def_id, *name);
                self.with_context(EscapeContext::IteraSource, |checker| {
                    checker.visit_expr(iter);
                });
                self.visit_block(block);
            }
            HirExpressionKind::Conversio {
                source,
                target,
                recovery,
                ..
            } => {
                self.with_context(EscapeContext::Conversio, |checker| {
                    checker.visit_expr(source);
                });
                if let HirConversioTarget::Intervallum(operand) = target {
                    self.visit_expr(operand);
                }
                if let Some(recovery) = recovery {
                    self.visit_expr(recovery);
                }
            }
            HirExpressionKind::Struct(_, fields) => {
                for (_, value) in fields {
                    self.with_context(EscapeContext::StructField, |checker| {
                        checker.visit_expr(value);
                    });
                }
            }
            HirExpressionKind::Verte {
                source, entries, ..
            } => {
                self.visit_expr(source);
                if let Some(entries) = entries {
                    for field in entries {
                        self.visit_object_field(field);
                    }
                }
            }
            HirExpressionKind::Array(elements) => {
                for element in elements {
                    match element {
                        HirArrayElement::Expr(element) | HirArrayElement::Spread(element) => {
                            self.with_context(EscapeContext::AggregateElement, |checker| {
                                checker.visit_expr(element);
                            });
                        }
                        HirArrayElement::Infer(_) => {}
                    }
                }
            }
            HirExpressionKind::Tuple(elements, _) => {
                for element in elements {
                    self.with_context(EscapeContext::AggregateElement, |checker| {
                        checker.visit_expr(element);
                    });
                }
            }
            HirExpressionKind::Scribe(_, elements) | HirExpressionKind::Scriptum(_, elements) => {
                let child_context = match self.current_context() {
                    EscapeContext::Stmt => EscapeContext::Stmt,
                    _ => EscapeContext::Value,
                };
                for element in elements {
                    self.with_context(child_context, |checker| checker.visit_expr(element));
                }
            }
            _ => walk_expr(self, expr),
        }
    }
}

#[derive(Clone, Copy)]
enum EscapeContext {
    Value,
    Stmt,
    Return,
    CallArg,
    AssignTarget,
    StructField,
    Conversio,
    /// `itera ex <live cursor>` — the only allowed expression use of a bound cursor.
    IteraSource,
    /// `sit cursor<scrinium<T>> c ← t.cursor()` — the only allowed direct `tuus.cursor()` site.
    LocalInit,
    /// Array/tuple/scribe literal elements that would store a live cursor value.
    AggregateElement,
}
