//! Type-level index terms for indexed types (tensor shape is the first deployment).
//!
//! Gate 1 v1 surface: literals, declared size variables (`magnitudo N`), and tuple
//! shapes. Arithmetic expression forms are reserved for a later grammar expansion;
//! the arena and equality helpers are structured so unification can grow without
//! rewriting call sites.
//!
//! WHY: Indexed types treat container size as a static fact parallel to dtype.
//! `Figura` in `tensor<T, Figura>` lowers into interned [`IndexExpr`] nodes.

use radix_lexer::Symbol as LexSymbol;
use serde::{Deserialize, Serialize};

/// Arena index into a [`TypeTable`](crate::TypeTable) index store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndexId(pub u32);

/// Identifier for an index inference metavariable (Gate 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndexVar(pub u32);

/// A natural-number index term in the semantic type system.
///
/// Tuple nodes represent rank-n shapes (`[N, M]`). An empty tuple is rank-0
/// (`tensor<T, []>`). [`IndexExpr::Unspecified`] is an internal bridge for
/// legacy `tensor<T>` spellings until grammar supplies `Figura` (Stages 4–5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexExpr {
    /// Compile-time natural literal, including `0`.
    Literal(u64),
    /// Declared size parameter from `magnitudo N` in a generic parameter list.
    Param(LexSymbol),
    /// Rank-n compound shape.
    Tuple(Vec<IndexId>),
    /// Inference hole; solved by the index inference pass (Stage 3+).
    Infer(IndexVar),
    /// Internal sentinel: source omitted `Figura` (pre-grammar bridge).
    Unspecified,
}

impl IndexExpr {
    /// Whether this expression is the internal unspecified-shape bridge.
    #[must_use]
    pub fn is_unspecified(&self) -> bool {
        matches!(self, Self::Unspecified)
    }

    /// Whether this expression is an inference hole awaiting constraints.
    #[must_use]
    pub fn is_infer(&self) -> bool {
        matches!(self, Self::Infer(_))
    }
}

/// Extract compile-time literal dimensions when every term is a natural literal.
///
/// Returns `None` when any dimension is symbolic (`Param`, `Infer`, `Unspecified`).
#[must_use]
pub fn i64s_from_literal_index(index_id: IndexId, types: &crate::TypeTable) -> Option<Vec<i64>> {
    match types.get_index(index_id) {
        IndexExpr::Literal(value) => Some(vec![i64::try_from(*value).ok()?]),
        IndexExpr::Tuple(dims) => {
            let mut out = Vec::with_capacity(dims.len());
            for dim in dims {
                let IndexExpr::Literal(value) = types.get_index(*dim) else {
                    return None;
                };
                out.push(i64::try_from(*value).ok()?);
            }
            Some(out)
        }
        IndexExpr::Param(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => None,
    }
}

/// Compute which axes need sum-reduction when reducing a broadcast result
/// cotangent back to an operand's original shape.
///
/// Returns `None` if the shapes are not broadcast-compatible. Returns
/// `Some(axes)` where `axes` is a list of axis indices in operand-local
/// space (0-indexed from the operand's own rank, not the padded rank).
#[must_use]
pub fn compute_cotangent_reduction_axes(
    operand_shape: IndexId,
    result_shape: IndexId,
    types: &crate::TypeTable,
) -> Option<Vec<usize>> {
    let op_dims = dims_from_shape(operand_shape, types);
    let res_dims = dims_from_shape(result_shape, types);

    let op_rank = op_dims.len();
    let res_rank = res_dims.len();
    let rank = op_rank.max(res_rank);
    let op_pad = rank - op_rank;
    let res_pad = rank - res_rank;

    let mut axes = Vec::new();
    for pos in 0..rank {
        let op_dim_is_one = if pos < op_pad {
            true
        } else {
            is_literal_one(op_dims[pos - op_pad], types)
        };

        let res_dim_is_one = if pos < res_pad {
            true
        } else {
            is_literal_one(res_dims[pos - res_pad], types)
        };

        if op_dim_is_one && !res_dim_is_one {
            if pos < op_pad {
                continue;
            }
            axes.push(pos - op_pad);
        } else if !op_dim_is_one
            && !res_dim_is_one
            && !types.index_equals(op_dims[pos - op_pad], res_dims[pos - res_pad])
        {
            return None;
        }
    }

    Some(axes)
}

fn dims_from_shape(shape_id: IndexId, types: &crate::TypeTable) -> Vec<IndexId> {
    match types.get_index(shape_id) {
        IndexExpr::Tuple(dims) => dims.clone(),
        IndexExpr::Literal(_) | IndexExpr::Param(_) | IndexExpr::Infer(_) => vec![shape_id],
        IndexExpr::Unspecified => vec![],
    }
}

fn is_literal_one(id: IndexId, types: &crate::TypeTable) -> bool {
    matches!(types.get_index(id), IndexExpr::Literal(1))
}

/// Parse a NATURAL capacity spelling interned as a named type-argument ident.
///
/// BB-1R interns `8` in `ascii<8>` as the decimal ident `"8"`. Only a pure
/// ASCII-digit spelling is a capacity literal; other names are magnitudo
/// params or non-natural rejects at the resolve site.
#[must_use]
pub fn parse_natural_capacity(name: &str) -> Option<u64> {
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    name.parse().ok()
}
