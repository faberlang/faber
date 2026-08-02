//! Literal matrix shape resolution for MIR.
//!
//! Resolves concrete `(rows, columns)` from a literal `IndexExpr::Tuple` in
//! the type table. Returns `None` when shape dimensions are not statically
//! known. Used by MIR validation and the stepper.

use radix_types::{IndexExpr, IndexId, TypeTable};

#[must_use]
pub fn literal_matrix_shape(index_id: IndexId, types: &TypeTable) -> Option<(u8, u8)> {
    let IndexExpr::Tuple(dims) = types.get_index(index_id) else {
        return None;
    };
    let [rows, columns] = dims.as_slice() else {
        return None;
    };
    Some((
        index_literal_u8(*rows, types)?,
        index_literal_u8(*columns, types)?,
    ))
}

fn index_literal_u8(index_id: IndexId, types: &TypeTable) -> Option<u8> {
    let IndexExpr::Literal(value) = types.get_index(index_id) else {
        return None;
    };
    u8::try_from(*value).ok()
}
