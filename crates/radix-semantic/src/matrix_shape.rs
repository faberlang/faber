//! Lowering and validation for `matrix<T, [R, C]>` shape syntax.
//!
//! Matrices reuse the shared [`IndexId`] arena, but the surface shape must be
//! exactly two dimensions so register matrices do not collapse into tensors.

use super::index::{IndexExpr, IndexId};
use super::{Resolver, TypeTable};
use radix_syntax::FiguraExpr;

pub const MATRIX_INCOMPLETE_MSG: &str =
    "matrix requires element type and shape; use matrix<T, [R, C]>";
pub const MATRIX_MISSING_FIGURA_MSG: &str =
    "matrix requires a two-dimensional shape; use matrix<T, [R, C]>";
pub const MATRIX_SHAPE_TWO_DIM_MSG: &str = "matrix shape must be exactly two dimensions";

/// Lower surface matrix shape syntax into the shared index arena.
///
/// # Errors
///
/// Returns a static error message when the shape is not a two-element tuple
/// of index expressions.
pub fn lower_matrix_shape_expr(
    shape: &FiguraExpr,
    resolver: &Resolver,
    types: &mut TypeTable,
) -> Result<IndexId, &'static str> {
    let FiguraExpr::Tuple(dims) = shape else {
        return Err(MATRIX_SHAPE_TWO_DIM_MSG);
    };
    if dims.len() != 2 {
        return Err(MATRIX_SHAPE_TWO_DIM_MSG);
    }
    let rows = crate::lower_figura_expr(&dims[0], resolver, types)?;
    let columns = crate::lower_figura_expr(&dims[1], resolver, types)?;
    Ok(types.intern_index(IndexExpr::Tuple(vec![rows, columns])))
}

/// Extract compile-time literal matrix dimensions when both axes are literals.
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
