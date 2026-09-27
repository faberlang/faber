//! Lowering for build-time file embeds (`insere "path"`, DECISIONS D8.10).
//!
//! A `generis` initializer that embeds a file folds here, during lowering,
//! to the literal the declared type asks for: `textus` (valid UTF-8 only) or
//! `octeti` (raw bytes). The bytes come from the driver-resolved
//! [`InsereFiles`](crate::insere::InsereFiles); every rejection is SEM061 with
//! a named issue. The authored path rides the presentation sidecar so Faber
//! re-emission prints the embed, not the bytes.

use super::Lowerer;
use crate::insere::{InsereSource, octeti_literal_payload, textus_literal_payload};
use crate::{Primitive, SemanticErrorKind, Type, TypeId};
use radix_diagnostics::DiagnosticArg;
use radix_hir::{HirExpression, HirExpressionKind, HirInsere, HirLiteral};
use radix_syntax::{Expr, ExprKind, InsereExpr};

impl Lowerer<'_> {
    /// Lower a `generis` initializer, folding a build-time embed against the
    /// declared type. Every other initializer lowers as an ordinary expression.
    pub(super) fn lower_static_initializer(
        &mut self,
        init: &Expr,
        declared: Option<TypeId>,
    ) -> HirExpression {
        let ExprKind::Insere(insere) = &init.kind else {
            return self.lower_expr(init);
        };
        let id = self.next_hir_id();
        let kind = self.lower_insere(insere, declared, init.span);
        if !matches!(kind, HirExpressionKind::Error) {
            self.presentation.insere_exprs.push(HirInsere {
                expr: id,
                path: insere.path,
            });
        }
        HirExpression {
            id,
            kind,
            ty: None,
            span: init.span,
        }
    }

    fn lower_insere(
        &mut self,
        insere: &InsereExpr,
        declared: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> HirExpressionKind {
        let path = self.interner.resolve(insere.path).to_owned();
        let bytes = match self.insere_files.get(&path) {
            InsereSource::Bytes(bytes) => bytes.clone(),
            InsereSource::Rejected(rejection) => {
                return self.insere_error(rejection.issue(), &path, insere.path_span);
            }
        };
        let primitive = declared.and_then(|ty| match self.types.get(ty) {
            Type::Primitive(primitive) => Some(*primitive),
            _ => None,
        });
        match primitive {
            Some(Primitive::Textus) => match String::from_utf8(bytes) {
                Ok(text) => {
                    let payload = self.interner.intern(&textus_literal_payload(&text));
                    HirExpressionKind::Literal(HirLiteral::String(payload))
                }
                Err(_) => self.insere_error("insere_invalid_utf8", &path, insere.path_span),
            },
            Some(Primitive::Octeti) => {
                let payload = self.interner.intern(&octeti_literal_payload(&bytes));
                HirExpressionKind::Literal(HirLiteral::Octeti(payload))
            }
            _ => self.insere_error("insere_type_invalid", &path, span),
        }
    }

    fn insere_error(
        &mut self,
        issue: &'static str,
        path: &str,
        span: radix_lexer::Span,
    ) -> HirExpressionKind {
        self.push_error(
            SemanticErrorKind::InsereInvalid,
            vec![
                DiagnosticArg::new("issue", issue),
                DiagnosticArg::new("path", path),
            ],
            span,
        );
        HirExpressionKind::Error
    }
}
