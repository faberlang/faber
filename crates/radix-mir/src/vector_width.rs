//! Literal vector width resolution for MIR.
//!
//! Resolves concrete width from a literal `IndexExpr` in the type table.
//! Returns `None` when width is not statically known. Used by MIR layout and
//! the runner.

use radix_types::{IndexExpr, IndexId, TypeTable};

#[must_use]
pub fn literal_vector_width(index_id: IndexId, types: &TypeTable) -> Option<u8> {
    match types.get_index(index_id) {
        IndexExpr::Literal(_) | IndexExpr::Affine { .. } | IndexExpr::Divide { .. } => {
            index_literal_u8(index_id, types)
        }
        IndexExpr::Tuple(dims) if dims.len() == 1 => index_literal_u8(dims[0], types),
        IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified
        | IndexExpr::Tuple(_) => None,
    }
}

fn index_literal_u8(index_id: IndexId, types: &TypeTable) -> Option<u8> {
    let value = radix_types::evaluate_index_dimension(index_id, types).ok()??;
    u8::try_from(value).ok()
}
