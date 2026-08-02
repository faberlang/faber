//! Shared semantic policy for `lista ↦ tensor` conversio.
//!
//! `↦` may construct a tensor from a flat list when the target type witnesses
//! the tensor element type and, when available, a literal `Figura`. Runtime
//! routes still own element-count checking through the same `structa` /
//! `TensorFromFlat` path used by explicit tensor construction.

use radix_hir::{HirArrayElement, HirExpression, HirExpressionKind};

use crate::figura::literal_index_i64s;
use radix_types::{Primitive, Type, TypeId, TypeTable};

/// Outcome-ladder label when `structa` rejects a flat buffer against the target `Figura`.
pub const LISTA_TO_TENSOR_STRUCTA_COUNT_MSG: &str =
    "tensor structa element count does not match shape";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListaToTensorShape {
    Literal(Vec<i64>),
    InferOneD,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListaToTensorError {
    NotListaToTensor,
    NestedLista,
    ElementMismatch,
}

#[derive(Debug)]
pub struct RectangularListaLiteral<'a> {
    pub leaf_elem: TypeId,
    pub dims: Vec<i64>,
    pub flat_leaves: Vec<&'a HirExpression>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RectangularListaLiteralError {
    NotNestedLiteral,
    NonLiteralTarget,
    Ragged,
    RowLengthMismatch,
    ShapeMismatch,
    Spread,
    NonLiteralLeaf,
    MissingLeafType,
    ElementMismatch,
}

#[must_use]
pub fn resolve_lista_to_tensor_shape(
    target: TypeId,
    types: &TypeTable,
) -> Option<ListaToTensorShape> {
    let Type::Tensor(_, shape_id) = normalized_type(target, types) else {
        return None;
    };
    Some(match literal_index_i64s(*shape_id, types) {
        Some(dims) => ListaToTensorShape::Literal(dims),
        None => ListaToTensorShape::InferOneD,
    })
}

/// # Errors
/// Returns an error when validation or type resolution fails. See [`ListaToTensorError`].
pub fn validate_lista_to_tensor_source(
    source: TypeId,
    target: TypeId,
    types: &TypeTable,
) -> Result<(), ListaToTensorError> {
    let (source_elem, target_elem) = match (
        normalized_type(source, types),
        normalized_type(target, types),
    ) {
        (Type::Array(source_elem), Type::Tensor(target_elem, _)) => (*source_elem, *target_elem),
        _ => return Err(ListaToTensorError::NotListaToTensor),
    };

    if matches!(normalized_type(source_elem, types), Type::Array(_)) {
        return Err(ListaToTensorError::NestedLista);
    }

    if !types.equals(source_elem, target_elem) {
        return Err(ListaToTensorError::ElementMismatch);
    }

    Ok(())
}

/// # Errors
/// Returns an error when validation or type resolution fails. See the error type for details.
pub fn analyze_rectangular_lista_literal<'a>(
    expr: &'a HirExpression,
    target: TypeId,
    types: &TypeTable,
) -> Result<RectangularListaLiteral<'a>, RectangularListaLiteralError> {
    let literal_target = rank_two_literal_target(target, types)?;
    let HirExpressionKind::Array(rows) = &expr.kind else {
        return Err(RectangularListaLiteralError::NotNestedLiteral);
    };
    if rows.is_empty() {
        return Err(RectangularListaLiteralError::Ragged);
    }
    if rows.len() != literal_target.rows {
        return Err(RectangularListaLiteralError::ShapeMismatch);
    }

    let mut flat_leaves =
        Vec::with_capacity(literal_target.rows.saturating_mul(literal_target.cols));
    for row in rows {
        collect_literal_row_leaves(
            row,
            literal_target.cols,
            literal_target.elem,
            types,
            &mut flat_leaves,
        )?;
    }

    Ok(RectangularListaLiteral {
        leaf_elem: literal_target.elem,
        dims: literal_target.dims,
        flat_leaves,
    })
}

struct RankTwoLiteralTarget {
    elem: TypeId,
    rows: usize,
    cols: usize,
    dims: Vec<i64>,
}

fn rank_two_literal_target(
    target: TypeId,
    types: &TypeTable,
) -> Result<RankTwoLiteralTarget, RectangularListaLiteralError> {
    if !matches!(normalized_type(target, types), Type::Tensor(_, _)) {
        return Err(RectangularListaLiteralError::NotNestedLiteral);
    }
    let ListaToTensorShape::Literal(dims) = resolve_lista_to_tensor_shape(target, types)
        .ok_or(RectangularListaLiteralError::NonLiteralTarget)?
    else {
        return Err(RectangularListaLiteralError::NonLiteralTarget);
    };
    if dims.len() != 2 {
        return Err(RectangularListaLiteralError::NonLiteralTarget);
    }
    let rows = dim_to_usize(dims[0])?;
    let cols = dim_to_usize(dims[1])?;
    Ok(RankTwoLiteralTarget {
        elem: tensor_element(target, types)?,
        rows,
        cols,
        dims,
    })
}

fn tensor_element(
    target: TypeId,
    types: &TypeTable,
) -> Result<TypeId, RectangularListaLiteralError> {
    match normalized_type(target, types) {
        Type::Tensor(elem, _) => Ok(*elem),
        _ => Err(RectangularListaLiteralError::NotNestedLiteral),
    }
}

fn collect_literal_row_leaves<'a>(
    row: &'a HirArrayElement,
    expected_cols: usize,
    target_elem: TypeId,
    types: &TypeTable,
    flat_leaves: &mut Vec<&'a HirExpression>,
) -> Result<(), RectangularListaLiteralError> {
    let HirArrayElement::Expr(row_expr) = row else {
        return Err(RectangularListaLiteralError::Spread);
    };
    let HirExpressionKind::Array(cells) = &row_expr.kind else {
        return Err(RectangularListaLiteralError::NotNestedLiteral);
    };
    if cells.is_empty() {
        return Err(RectangularListaLiteralError::Ragged);
    }
    if cells.len() != expected_cols {
        return Err(RectangularListaLiteralError::RowLengthMismatch);
    }

    for cell in cells {
        let HirArrayElement::Expr(leaf) = cell else {
            return Err(RectangularListaLiteralError::Spread);
        };
        if !matches!(leaf.kind, HirExpressionKind::Literal(_)) {
            return Err(RectangularListaLiteralError::NonLiteralLeaf);
        }
        let leaf_ty = leaf
            .ty
            .ok_or(RectangularListaLiteralError::MissingLeafType)?;
        if !types.equals(leaf_ty, target_elem) {
            return Err(RectangularListaLiteralError::ElementMismatch);
        }
        flat_leaves.push(leaf);
    }
    Ok(())
}

fn dim_to_usize(dim: i64) -> Result<usize, RectangularListaLiteralError> {
    usize::try_from(dim).map_err(|_| RectangularListaLiteralError::ShapeMismatch)
}

#[must_use]
pub fn lista_numerus_type(types: &TypeTable) -> Option<TypeId> {
    types.find_array(types.primitive(Primitive::Numerus))
}

pub fn ensure_lista_numerus_type(types: &mut TypeTable) -> TypeId {
    let numerus = types.primitive(Primitive::Numerus);
    types.ensure_array(numerus)
}

fn normalized_type(mut ty: TypeId, types: &TypeTable) -> &Type {
    loop {
        match types.get(ty) {
            Type::Alias(_, inner) => ty = *inner,
            resolved => return resolved,
        }
    }
}
