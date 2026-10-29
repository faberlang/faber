//! Lowering and validation for `vector<T, size N>` width syntax.
//!
//! Vector width reuses the shared [`IndexId`] arena but must be a single
//! dimension — not a multi-axis `Figura` tuple.

use super::index::{IndexExpr, IndexId};
use super::{Resolver, TypeTable};
use radix_syntax::FiguraExpr;

pub const VECTOR_INCOMPLETE_MSG: &str =
    "vector requires element type and width; use vector<T, size N>";
pub const VECTOR_MISSING_WIDTH_MSG: &str =
    "vector requires a width; use vector<T, size 4> or vf32[4]";
pub const VECTOR_WIDTH_SINGLE_DIM_MSG: &str = "vector width must be a single dimension";
pub const VECTOR_WIDTH_SINGLE_DIM_ISSUE: &str = "vector_width_single_dim";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VectorWidthError {
    pub issue: &'static str,
    pub message: &'static str,
}

impl VectorWidthError {
    const fn new(issue: &'static str, message: &'static str) -> Self {
        Self { issue, message }
    }

    fn from_message(message: &'static str) -> Self {
        let issue = if crate::figura::figura_arithmetic_reason(message).is_some() {
            "figura_arithmetic_invalid"
        } else {
            match message {
                crate::FIGURA_NOT_INDEX_PARAM_MSG => "figura_not_index_param",
                crate::FIGURA_TYPE_PARAM_IN_INDEX_MSG => "figura_type_param_in_index",
                crate::FIGURA_UNKNOWN_INDEX_MSG => "figura_unknown_index",
                _ => "unknown_vector_width_error",
            }
        };
        Self { issue, message }
    }
}

/// Lower surface width syntax into the shared index arena.
///
/// # Errors
///
/// Returns [`VectorWidthError`] when the width expression is not a valid
/// vector dimension.
pub fn lower_vector_width_expr(
    width: &FiguraExpr,
    resolver: &Resolver,
    types: &mut TypeTable,
) -> Result<IndexId, VectorWidthError> {
    match width {
        FiguraExpr::Tuple(dims) if dims.len() == 1 => {
            crate::lower_figura_expr(&dims[0], resolver, types)
                .map_err(VectorWidthError::from_message)
        }
        FiguraExpr::Tuple(_) => Err(VectorWidthError::new(
            VECTOR_WIDTH_SINGLE_DIM_ISSUE,
            VECTOR_WIDTH_SINGLE_DIM_MSG,
        )),
        other => {
            crate::lower_figura_expr(other, resolver, types).map_err(VectorWidthError::from_message)
        }
    }
}

/// Extract a concrete vector width when the index expression is a natural literal.
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

/// Whether an index expression is a valid single-dimension vector width shape.
#[must_use]
pub fn is_vector_width_shape(index_id: IndexId, types: &TypeTable) -> bool {
    match types.get_index(index_id) {
        IndexExpr::Literal(_) | IndexExpr::Param(_) | IndexExpr::Infer(_) => true,
        IndexExpr::Affine { base, .. } => {
            radix_types::evaluate_index_dimension(index_id, types).is_ok()
                && is_static_width_term(*base, types)
        }
        IndexExpr::Divide { numerator, .. } => {
            radix_types::evaluate_index_dimension(index_id, types).is_ok()
                && is_static_width_term(*numerator, types)
        }
        IndexExpr::Tuple(dims) => dims.len() == 1,
        IndexExpr::Runtime(_) | IndexExpr::Unspecified => false,
    }
}

fn is_static_width_term(index_id: IndexId, types: &TypeTable) -> bool {
    match types.get_index(index_id) {
        IndexExpr::Literal(_) | IndexExpr::Param(_) => true,
        IndexExpr::Affine { base, .. } => {
            radix_types::evaluate_index_dimension(index_id, types).is_ok()
                && is_static_width_term(*base, types)
        }
        IndexExpr::Divide { numerator, .. } => {
            radix_types::evaluate_index_dimension(index_id, types).is_ok()
                && is_static_width_term(*numerator, types)
        }
        IndexExpr::Tuple(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified => false,
    }
}
