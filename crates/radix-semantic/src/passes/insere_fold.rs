//! Build-time file embeds fold to literals right after typecheck (st1 R5).
//!
//! Typecheck types every `insere "path"` node from the slot it fills
//! (`textus` or `octeti`). This pass replaces each node with the file's
//! contents as a literal of that type, reading the bytes the driver resolved
//! into [`InsereFiles`]. After it no `Insere` node survives, so statica
//! folding, the praefixum runner, and every backend only ever see literals.
//! The expression id is kept: the authored path rides the presentation
//! sidecar under that id, which is how Faber re-emission prints the embed
//! instead of the bytes.
//!
//! Every rejection is SEM061 with a named issue; the failing node becomes an
//! `Error` expression so nothing downstream reads a half-folded embed.

use crate::insere::{InsereFiles, InsereSource, octeti_literal_payload, textus_literal_payload};
use crate::{SemanticError, SemanticErrorKind};
use radix_hir::visit::{HirVisitorMut, walk_expr_mut};
use radix_hir::{HirExpression, HirExpressionKind, HirLiteral, HirModule};
use radix_lexer::{Interner, Symbol};
use radix_types::{Primitive, Type, TypeId, TypeTable};

const ISSUE_TYPE_REQUIRED: &str = "insere_type_required";
const ISSUE_INVALID_UTF8: &str = "insere_invalid_utf8";

/// Replace every embed in `hir` with its file's literal.
#[must_use]
pub fn fold(
    hir: &mut HirModule,
    files: &InsereFiles,
    types: &TypeTable,
    interner: &mut Interner,
) -> Vec<SemanticError> {
    let mut pass = Fold {
        files,
        types,
        interner,
        errors: Vec::new(),
    };
    pass.visit_program_mut(hir);
    pass.errors
}

struct Fold<'a> {
    files: &'a InsereFiles,
    types: &'a TypeTable,
    interner: &'a mut Interner,
    errors: Vec<SemanticError>,
}

impl Fold<'_> {
    /// The embed's literal kind, or the issue naming why it cannot be built.
    fn literal_for(
        &mut self,
        path: Symbol,
        ty: Option<TypeId>,
    ) -> Result<HirLiteral, &'static str> {
        let primitive = ty.and_then(|ty| self.slot_primitive(ty));
        let authored = self.interner.resolve(path).to_owned();
        let bytes = match self.files.get(&authored) {
            InsereSource::Bytes(bytes) => bytes,
            InsereSource::Rejected(rejection) => return Err(rejection.issue()),
        };
        match primitive {
            Some(Primitive::Textus) => {
                let text = std::str::from_utf8(bytes).map_err(|_| ISSUE_INVALID_UTF8)?;
                let payload = self.interner.intern(&textus_literal_payload(text));
                Ok(HirLiteral::String(payload))
            }
            Some(Primitive::Octeti) => {
                let payload = self.interner.intern(&octeti_literal_payload(bytes));
                Ok(HirLiteral::Octeti(payload))
            }
            _ => Err(ISSUE_TYPE_REQUIRED),
        }
    }

    fn slot_primitive(&self, mut ty: TypeId) -> Option<Primitive> {
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::Primitive(primitive) => return Some(*primitive),
                // `lista<b8>` takes the raw bytes (ruling B9, need).
                Type::Array(element)
                    if matches!(
                        self.types.get(*element),
                        Type::Bits(radix_types::BitsWidth::B8)
                    ) =>
                {
                    return Some(Primitive::Octeti);
                }
                _ => return None,
            }
        }
    }

    fn reject(&mut self, issue: &'static str, path: Symbol, span: radix_lexer::Span) {
        let path = self.interner.resolve(path).to_owned();
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::InsereInvalid, span)
                .with_arg("issue", issue)
                .with_arg("path", path),
        );
    }
}

impl HirVisitorMut for Fold<'_> {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        walk_expr_mut(self, expr);
        let HirExpressionKind::Insere(path) = expr.kind else {
            return;
        };
        match self.literal_for(path, expr.ty) {
            Ok(literal) => expr.kind = HirExpressionKind::Literal(literal),
            Err(issue) => {
                self.reject(issue, path, expr.span);
                expr.kind = HirExpressionKind::Error;
            }
        }
    }
}
