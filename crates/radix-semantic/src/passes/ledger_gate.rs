//! Refusal of every ledger operation that has not landed yet
//! (need, units L1, L2a and L2b).
//!
//! The ledger type `tabulae<K, V>` (en `ledger<K, V>`) is a built-in
//! insertion-ordered map. The type is admitted in every type slot a map
//! accepts; a brace literal, `empty`, or a declaration/assignment from
//! one at a ledger cell typechecks like its map twin, and so do the index
//! read/store, the map methods, membership and the three loop forms.
//! Equality, print/format, conversions, sort and the remaining wrapper
//! operations are still refused by one named issue,
//! `ledger_operation_not_implemented`, so nothing half-works.

use crate::{SemanticError, SemanticErrorKind, Type, TypeId, TypeTable};
use radix_hir::visit::{HirVisitor, walk_expr, walk_local, walk_program};
use radix_hir::{
    HirBinOp, HirConversioTarget, HirExpression, HirExpressionKind, HirLiteral, HirLocal, HirModule,
};

const ISSUE: &str = "ledger_operation_not_implemented";

/// Report every operation on a ledger value in `hir`.
///
/// Runs on the typed HIR even when typecheck failed: an operation typecheck
/// cannot type on a ledger (an index, a method, a loop) fails there with a
/// generic mismatch, and the gate replaces that cascade with the one named
/// refusal. See [`replace_cascades`].
#[must_use]
pub fn check(hir: &HirModule, types: &TypeTable) -> Vec<SemanticError> {
    let mut gate = Gate {
        types,
        errors: Vec::new(),
    };
    walk_program(&mut gate, hir);
    gate.errors
}

/// Drop every error in `diagnostics` that lies inside the span of a gated
/// operation (the typecheck cascade of the refusal), then append the refusals.
pub fn replace_cascades(diagnostics: &mut Vec<SemanticError>, refusals: Vec<SemanticError>) {
    if refusals.is_empty() {
        return;
    }
    diagnostics.retain(|diagnostic| {
        !diagnostic.is_error()
            || !refusals.iter().any(|refusal| {
                diagnostic.span.start >= refusal.span.start
                    && diagnostic.span.end <= refusal.span.end
            })
    });
    diagnostics.extend(refusals);
}

struct Gate<'a> {
    types: &'a TypeTable,
    errors: Vec<SemanticError>,
}

impl Gate<'_> {
    fn is_ledger(&self, ty: Option<TypeId>) -> bool {
        let mut current = ty;
        while let Some(id) = current {
            match self.types.get(id) {
                Type::Ledger(_, _) => return true,
                Type::Alias(_, inner) | Type::Ref(_, inner) => current = Some(*inner),
                _ => return false,
            }
        }
        false
    }

    /// A value built in place (literal, empty, ascription, conversion) rather
    /// than read from a name, a call or a field.
    fn constructs(expr: &HirExpression) -> bool {
        matches!(
            expr.kind,
            HirExpressionKind::Array(_)
                | HirExpressionKind::Struct(..)
                | HirExpressionKind::Tuple(..)
                | HirExpressionKind::Vacua
                | HirExpressionKind::Literal(_)
                | HirExpressionKind::Verte { .. }
                | HirExpressionKind::Conversio { .. }
        )
    }

    /// A brace literal or the empty collection: the constructors this unit
    /// admits at a ledger cell. Every other [`Self::constructs`] shape stays
    /// refused until the matching ledger operations land.
    fn literal_or_empty(expr: &HirExpression) -> bool {
        matches!(
            expr.kind,
            HirExpressionKind::Vacua | HirExpressionKind::Literal(HirLiteral::JsonValor(_))
        )
    }

    fn operand(&self, expr: &HirExpression) -> bool {
        self.is_ledger(expr.ty)
    }

    fn operates_on_ledger(&self, expr: &HirExpression) -> bool {
        match &expr.kind {
            // Index reads/stores, method calls, membership and the loop forms
            // typecheck on a ledger now (unit L2b), so only the operations
            // that stay refused are named here: equality on a `Binary` other
            // than `∈`/`∉`, print/format, conversions, sort and the remaining
            // wrappers.
            HirExpressionKind::Binary(op, left, right)
                if !matches!(
                    op,
                    HirBinOp::InRange
                        | HirBinOp::NotInRange
                        | HirBinOp::Between
                        | HirBinOp::NotBetween
                ) =>
            {
                self.operand(left) || self.operand(right)
            }
            HirExpressionKind::OptionalChain(object, _)
            | HirExpressionKind::NonNull(object, _)
            | HirExpressionKind::Unary(_, object)
            | HirExpressionKind::Format { value: object, .. } => self.operand(object),
            HirExpressionKind::Scribe(_, args) | HirExpressionKind::Scriptum(_, args) => {
                args.iter().any(|arg| self.operand(arg))
            }
            HirExpressionKind::Conversio { source, target, .. } => {
                self.operand(source)
                    || matches!(target, HirConversioTarget::Type(ty) if self.is_ledger(Some(*ty)))
            }
            HirExpressionKind::Verte { source, target, .. } => {
                self.operand(source) || self.is_ledger(Some(*target))
            }
            HirExpressionKind::Sort { source, .. } => self.operand(source),
            HirExpressionKind::Assign(target, value) => {
                self.operand(target) && Self::constructs(value) && !Self::literal_or_empty(value)
            }
            HirExpressionKind::Array(_)
            | HirExpressionKind::Struct(..)
            | HirExpressionKind::Tuple(..) => self.is_ledger(expr.ty),
            _ => false,
        }
    }
}

impl HirVisitor for Gate<'_> {
    fn visit_local(&mut self, local: &HirLocal) {
        if let Some(init) = &local.init
            && self.is_ledger(local.ty)
            && Self::constructs(init)
            && !Self::literal_or_empty(init)
        {
            self.errors.push(
                SemanticError::structured(SemanticErrorKind::TypeMismatch, init.span)
                    .with_arg("issue", ISSUE),
            );
            return;
        }
        walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if self.operates_on_ledger(expr) {
            self.errors.push(
                SemanticError::structured(SemanticErrorKind::TypeMismatch, expr.span)
                    .with_arg("issue", ISSUE),
            );
            return;
        }
        walk_expr(self, expr);
    }
}
