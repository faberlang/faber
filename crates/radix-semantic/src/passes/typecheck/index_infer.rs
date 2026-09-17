//! Index inference integration on [`TypeChecker`].
//!
//! Constraint hooks for tensor shape inference during typechecking.

use crate::index::IndexExpr;
use crate::index::IndexId;
use crate::{IndexConstraint, SemanticErrorKind};
use radix_lexer::Span;

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

const ISSUE_INCOMPATIBLE_TENSOR_INDEX: &str = "incompatible_tensor_index";

impl TypeChecker<'_> {
    pub fn fresh_index_hole(&mut self) -> IndexId {
        self.index_infer.fresh_hole(self.types)
    }

    pub fn resolve_index(&mut self, id: IndexId) -> IndexId {
        self.index_infer.resolve(self.types, id)
    }

    pub fn canonicalize_index(&mut self, id: IndexId) -> IndexId {
        self.index_infer.canonicalize(self.types, id)
    }

    pub fn is_unresolved_index_hole(&mut self, id: IndexId) -> bool {
        self.index_infer.is_unresolved_hole(self.types, id)
    }

    pub fn index_contains_unresolved_hole(&mut self, id: IndexId) -> bool {
        self.index_infer.contains_unresolved_hole(self.types, id)
    }

    pub fn solve_index_constraints(&mut self, constraints: &[IndexConstraint]) -> bool {
        self.index_infer.solve_constraints(self.types, constraints)
    }

    pub fn constrain_index_equal(&mut self, a: IndexId, b: IndexId, span: Span) {
        if !self.solve_index_constraints(&[IndexConstraint::Equal(a, b)]) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_INCOMPATIBLE_TENSOR_INDEX,
                span,
            );
        }
    }

    pub fn constrain_index_broadcast(
        &mut self,
        a: IndexId,
        b: IndexId,
        span: Span,
    ) -> Option<IndexId> {
        let mut unifier = self.index_infer.unifier(self.types);
        if let Ok(result) = unifier.unify_broadcast(a, b) {
            Some(result)
        } else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_INCOMPATIBLE_TENSOR_INDEX,
                span,
            );
            None
        }
    }

    pub fn tensor_shape_has_unresolved_hole(&mut self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        let capacity = match self.types.get(resolved) {
            Type::Tensor(_, shape) => *shape,
            Type::BoundedAscii { capacity }
            | Type::BoundedTextus { capacity }
            | Type::BoundedOcteti { capacity } => *capacity,
            Type::BoundedArray { capacity, .. } => *capacity,
            _ => return false,
        };
        let capacity = self.resolve_index(capacity);
        if matches!(self.types.get_index(capacity), IndexExpr::Unspecified) {
            return true;
        }
        self.index_contains_unresolved_hole(capacity)
    }
}
