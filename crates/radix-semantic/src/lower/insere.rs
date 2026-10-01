//! Lowering for compile-time initializers and build-time file embeds
//! (`insere "path"`, DECISIONS D8.10).
//!
//! One surface serves every `=` position: a module-level constant, a block
//! `=` local, and a genus field default. Lowering tracks whether it is inside
//! such an initializer tree ([`Lowerer::compile_time_depth`]); an embed is
//! legal only there. The embed lowers to a typeless
//! [`HirExpressionKind::Insere`] node: the slot it fills types it during
//! typecheck, and the `insere_fold` pass then replaces it with the file's
//! literal. The authored path rides the presentation sidecar so Faber
//! re-emission prints the embed, not the bytes.

use super::Lowerer;
use crate::SemanticErrorKind;
use radix_diagnostics::DiagnosticArg;
use radix_hir::{HirExpression, HirExpressionKind, HirId, HirInsere};
use radix_syntax::{Expr, ExprKind, InsereExpr};

impl Lowerer<'_> {
    /// Lower the initializer of a module constant, a block-level `=` constant,
    /// or a genus field default: the one initializer surface (st1 R4/R6).
    ///
    /// A `praefixum { … }` body lowers to `Praefixum(Block)` for the driver's
    /// build-time evaluation (D8.5); it is legal only as the whole initializer.
    /// Every other initializer lowers as an ordinary expression inside a
    /// compile-time tree, so an embed is admitted anywhere in it.
    pub(super) fn lower_compile_time_initializer(&mut self, init: &Expr) -> HirExpression {
        self.compile_time_depth += 1;
        let lowered = match &init.kind {
            ExprKind::Praefixum(praefixum) => {
                self.current_span = init.span;
                let id = self.next_hir_id();
                let kind = self.lower_praefixum(praefixum, init.span);
                HirExpression {
                    id,
                    kind,
                    ty: None,
                    span: init.span,
                }
            }
            _ => self.lower_expr(init),
        };
        self.compile_time_depth -= 1;
        lowered
    }

    /// Lower `insere "path"` once the parser has claimed it.
    pub(super) fn lower_insere(
        &mut self,
        insere: &InsereExpr,
        id: HirId,
        span: radix_lexer::Span,
    ) -> HirExpressionKind {
        if self.compile_time_depth == 0 {
            let path = self.interner.resolve(insere.path).to_owned();
            self.push_error(
                SemanticErrorKind::InsereInvalid,
                vec![
                    DiagnosticArg::new("issue", "insere_outside_constant"),
                    DiagnosticArg::new("path", path),
                ],
                span,
            );
            return HirExpressionKind::Error;
        }
        self.presentation.insere_exprs.push(HirInsere {
            expr: id,
            path: insere.path,
        });
        HirExpressionKind::Insere(insere.path)
    }
}
