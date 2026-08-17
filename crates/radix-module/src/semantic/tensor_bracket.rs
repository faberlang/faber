//! Shared policy for tensor bracket access (`tensor[`idx`]`) — DEFER-063 sugar for
//! `accipe` / `ponde`.
//!
//! Bracket reads return `T ∪ nihil`; writes accept plain `T`. Index operands may
//! be scalar integers (rank-1 tensors only) or `lista<integer>` index vectors.

use crate::hir::{HirExpression, HirExpressionKind};

use super::{Type, TypeId, TypeTable};

pub const TENSOR_SCALAR_INDEX_RANK_MSG: &str = "tensor scalar index requires rank-1; use a lista<integer> for rank-N";
pub const TENSOR_BRACKET_INDEX_MSG: &str = "tensor bracket index requires an integer or lista<integer>";

/// Whether `ty` is a tensor type (including through type aliases).
#[must_use]
pub fn is_tensor_type(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Tensor(_, _) => true,
        Type::Alias(_, inner) => is_tensor_type(types, *inner),
        _ => false,
    }
}

/// Whether `ty` is a list-shaped bracket index (`lista<…>`).
#[must_use]
pub fn is_bracket_index_list_type(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Array(_) => true,
        Type::Alias(_, inner) => is_bracket_index_list_type(types, *inner),
        _ => false,
    }
}

/// Whether an index expression is list-shaped at the bracket boundary.
///
/// Inline `[r, c]` literals and typed `lista<…>` values take the list path;
/// scalar integers are wrapped into a one-element list at lowering boundaries.
#[must_use]
pub fn is_bracket_index_list_expr(expr: &HirExpression, types: &TypeTable) -> bool {
    matches!(expr.kind, HirExpressionKind::Array(_))
        || expr
            .ty
            .is_some_and(|ty| is_bracket_index_list_type(types, ty))
}

#[cfg(test)]
#[path = "tensor_bracket_test.rs"]
mod tests;
