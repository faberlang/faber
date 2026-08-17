//! HIR validation gate — canonical single definition consumed by all codegen targets.
//!
//! Every HIR-backed codegen target must reject programs that still carry error
//! recovery expressions from earlier compiler phases. This module provides the
//! one canonical implementation so changes to the shared logic sync across all
//! five backends (Rust, Faber, Swift, Go, TS) and the facade dispatch.

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_hir::visit::{walk_expr, HirVisitor};
use radix_hir::{HirExpressionKind, HirModule};
use radix_lexer::Span;

/// # Errors
///
/// Returns `CodegenError` if the HIR program contains error recovery expressions
/// that must not reach codegen.
pub fn reject_hir_errors(hir: &HirModule) -> Result<(), CodegenError> {
    if let Some(span) = find_error_expr_in_program(hir) {
        return Err(CodegenError {
            message: format!(
                "cannot generate code from HIR containing error expressions at span {}..{}",
                span.start, span.end
            ),
            args: vec![DiagnosticArg::new(
                "issue",
                "hir_error_expression_reached_codegen",
            )],
        });
    }
    Ok(())
}

/// Walk the HIR program and return the span of the first error expression found.
#[must_use]
pub fn find_error_expr_in_program(hir: &HirModule) -> Option<Span> {
    struct Finder {
        span: Option<Span>,
    }
    impl HirVisitor for Finder {
        fn visit_expr(&mut self, expr: &radix_hir::HirExpression) {
            if self.span.is_some() {
                return;
            }
            if matches!(expr.kind, HirExpressionKind::Error) {
                self.span = Some(expr.span);
                return;
            }
            walk_expr(self, expr);
        }
    }
    let mut f = Finder { span: None };
    radix_hir::visit::walk_program(&mut f, hir);
    f.span
}
