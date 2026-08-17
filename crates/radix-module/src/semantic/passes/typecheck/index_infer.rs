//! Index inference integration on [`TypeChecker`].
//!
//! Constraint hooks for tensor shape inference during typechecking.

use crate::lexer::Span;
use crate::semantic::index::IndexExpr;
use crate::semantic::index::IndexId;
use crate::semantic::{IndexConstraint, IndexInferError, SemanticErrorKind};

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

const ISSUE_INCOMPATIBLE_TENSOR_INDEX: &str = "incompatible_tensor_index";

impl TypeChecker<'_> {
    pub(super) fn fresh_index_hole(&mut self) -> IndexId {
        self.index_infer.fresh_hole(self.types)
    }

    pub(super) fn resolve_index(&mut self, id: IndexId) -> IndexId {
        self.index_infer.resolve(self.types, id)
    }

    pub(super) fn canonicalize_index(&mut self, id: IndexId) -> IndexId {
        self.index_infer.canonicalize(self.types, id)
    }

    pub(super) fn is_unresolved_index_hole(&mut self, id: IndexId) -> bool {
        self.index_infer.is_unresolved_hole(self.types, id)
    }

    pub(super) fn index_contains_unresolved_hole(&mut self, id: IndexId) -> bool {
        self.index_infer.contains_unresolved_hole(self.types, id)
    }

    pub(super) fn solve_index_constraints(&mut self, constraints: &[IndexConstraint]) -> bool {
        self.index_infer.solve_constraints(self.types, constraints)
    }

    pub(super) fn constrain_index_equal(&mut self, a: IndexId, b: IndexId, span: Span) {
        if !self.solve_index_constraints(&[IndexConstraint::Equal(a, b)]) {
            self.error_issue(SemanticErrorKind::TypeMismatch, ISSUE_INCOMPATIBLE_TENSOR_INDEX, span);
        }
    }

    pub(super) fn constrain_index_broadcast(&mut self, a: IndexId, b: IndexId, span: Span) -> Option<IndexId> {
        let mut unifier = self.index_infer.unifier(self.types);
        if let Ok(result) = unifier.unify_broadcast(a, b) {
            Some(result)
        } else {
            self.error_issue(SemanticErrorKind::TypeMismatch, ISSUE_INCOMPATIBLE_TENSOR_INDEX, span);
            None
        }
    }

    #[allow(dead_code)] // Wired when tensor finalize reports hole-specific diagnostics.
    pub(super) fn finalize_index(&mut self, id: IndexId, span: Span, issue: &'static str) -> IndexId {
        match self.index_infer.finalize(self.types, id) {
            Ok(canonical) => canonical,
            Err(IndexInferError::UnresolvedHole) => {
                self.missing_type_annotation(issue, span);
                id
            }
            Err(IndexInferError::Mismatch | IndexInferError::Occurs) => {
                self.error_issue(SemanticErrorKind::TypeMismatch, ISSUE_INCOMPATIBLE_TENSOR_INDEX, span);
                id
            }
        }
    }

    pub(super) fn tensor_shape_has_unresolved_hole(&mut self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        let Type::Tensor(_, shape) = self.types.get(resolved) else {
            return false;
        };
        let shape = self.resolve_index(*shape);
        if matches!(self.types.get_index(shape), IndexExpr::Unspecified) {
            return true;
        }
        self.index_contains_unresolved_hole(shape)
    }
}
