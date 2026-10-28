//! Shared policy for tensor bracket access (`tensor[`idx`]`) — DEFER-063 sugar for
//! `accipe` / `ponde`.
//!
//! Bracket reads return `T ∪ nihil`; writes accept plain `T`. Index operands may
//! be scalar integers (rank-1 tensors only) or `lista<integer>` index vectors.

use radix_hir::{HirExpression, HirExpressionKind};

use super::{Type, TypeId, TypeTable};

pub const TENSOR_SCALAR_INDEX_RANK_MSG: &str =
    "tensor scalar index requires rank-1; use a lista<integer> for rank-N";
pub const TENSOR_BRACKET_INDEX_MSG: &str =
    "tensor bracket index requires an integer or lista<integer>";

/// Whether `ty` is a tensor type (including through type aliases and an access
/// marker: `copy`, `own`, `ref` and `mut` are binding facts, not a carrier).
#[must_use]
pub fn is_tensor_type(types: &TypeTable, ty: TypeId) -> bool {
    matches!(
        types.get(types.peel_access_marker(ty).1),
        Type::Tensor(_, _)
    )
}

/// Whether `ty` is a coordinate-bracket receiver: a tensor or a matrix.
///
/// The coordinate form (`m[[r, c]]`) is shared; `is_tensor_type` keeps its
/// tensor-only meaning for the kernel/filum/borrow passes.
#[must_use]
pub fn is_coordinate_bracket_receiver(types: &TypeTable, ty: TypeId) -> bool {
    matches!(
        types.get(types.peel_access_marker(ty).1),
        Type::Tensor(_, _) | Type::Matrix(_, _)
    )
}

/// Whether `ty` is a list-shaped bracket index (`lista<…>`).
#[must_use]
pub fn is_bracket_index_list_type(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. } => true,
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
