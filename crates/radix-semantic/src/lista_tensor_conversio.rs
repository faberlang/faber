//! Shared semantic policy for `lista ↦ tensor` conversio.
//!
//! `↦` may construct a tensor from a flat list when the target type witnesses
//! the tensor element type and, when available, a literal `Figura`. Runtime
//! routes still own element-count checking through the same `structa` /
//! `TensorFromFlat` path used by explicit tensor construction.

use radix_hir::{HirArrayElement, HirExpression, HirExpressionKind};

use super::{IndexExpr, IndexId, Primitive, Type, TypeId, TypeTable, i64s_from_literal_index};

/// Outcome-ladder label when `structa` rejects a flat buffer against the target `Figura`.
pub const LISTA_TO_TENSOR_STRUCTA_COUNT_MSG: &str =
    "tensor structa element count does not match shape";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListaToTensorShape {
    Literal(Vec<i64>),
    /// Declared shape whose every dim is a literal, generic expression, or
    /// runtime-origin extent: rank is known even when values are not.
    Symbolic {
        shape: IndexId,
        rank: usize,
    },
    InferOneD,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListaToTensorError {
    NonListaSource,
    NestedLista,
    ElementMismatch,
    /// Route C admits numeric element types only (AEU-5 fill contract).
    NonNumericElement,
    /// Route C requires the bounded capacity to equal the rank-1 tensor dim.
    CapacityMismatch,
}

#[derive(Debug)]
pub struct RectangularListaLiteral<'a> {
    pub leaf_elem: TypeId,
    pub dims: Vec<i64>,
    pub flat_leaves: Vec<&'a HirExpression>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RectangularListaLiteralError {
    NonNestedLiteral,
    NonLiteralTarget,
    Ragged,
    RowLengthMismatch,
    ShapeMismatch,
    Spread,
    NonLiteralLeaf,
    AbsentLeafType,
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
    Some(match i64s_from_literal_index(*shape_id, types) {
        Some(dims) => ListaToTensorShape::Literal(dims),
        None => match symbolic_shape_rank(*shape_id, types) {
            Some(rank) => ListaToTensorShape::Symbolic {
                shape: *shape_id,
                rank,
            },
            None => ListaToTensorShape::InferOneD,
        },
    })
}

/// Rank of a shape index whose every dim is declared — a literal or a
/// `magnitudo` param (`[T, D]`, `[3, D]`). Inference holes and unspecified
/// shapes declare no rank; they stay one-dimensional inferences.
fn symbolic_shape_rank(shape: IndexId, types: &TypeTable) -> Option<usize> {
    match types.get_index(shape) {
        IndexExpr::Literal(_)
        | IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Affine { .. }
        | IndexExpr::Divide { .. }
            if valid_declared_dimension(shape, types) =>
        {
            Some(1)
        }
        IndexExpr::Tuple(dims) => dims
            .iter()
            .all(|dim| valid_declared_dimension(*dim, types))
            .then_some(dims.len()),
        _ => None,
    }
}

fn valid_declared_dimension(index: IndexId, types: &TypeTable) -> bool {
    match types.get_index(index) {
        IndexExpr::Literal(_) | IndexExpr::Param(_) | IndexExpr::Runtime(_) => true,
        IndexExpr::Affine { base, .. } => {
            radix_types::evaluate_index_dimension(index, types).is_ok()
                && valid_declared_dimension(*base, types)
        }
        IndexExpr::Divide { numerator, .. } => {
            radix_types::evaluate_index_dimension(index, types).is_ok()
                && valid_declared_dimension(*numerator, types)
        }
        IndexExpr::Tuple(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => false,
    }
}

/// Validate that a source type is a valid list representation of a tensor.
///
/// # Errors
///
/// Returns [`ListaToTensorError`] when the source type is not a valid
/// nested-array representation of the target tensor type.
pub fn validate_lista_to_tensor_source(
    source: TypeId,
    target: TypeId,
    types: &TypeTable,
) -> Result<(), ListaToTensorError> {
    match (
        normalized_type(source, types),
        normalized_type(target, types),
    ) {
        (Type::Array(source_elem), Type::Tensor(target_elem, _)) => {
            validate_flat_source(*source_elem, *target_elem, types)
        }
        // Route C (AEU-5): bounded `lista<T, N>` snapshots into a padded
        // `tensor<T, [N]>`. Numeric `T` only; the capacity must equal the
        // rank-1 literal dim so every padded slot is a real fill value.
        (Type::BoundedArray { element, capacity }, Type::Tensor(target_elem, shape)) => {
            let elem = *element;
            validate_flat_source(elem, *target_elem, types)?;
            if !is_numeric_element(elem, types) {
                return Err(ListaToTensorError::NonNumericElement);
            }
            let dim = rank_one_literal_dim(*shape, types);
            let capacity = rank_one_literal_dim(*capacity, types);
            if dim.is_none() || dim != capacity {
                return Err(ListaToTensorError::CapacityMismatch);
            }
            Ok(())
        }
        _ => Err(ListaToTensorError::NonListaSource),
    }
}

/// The single literal extent of a rank-1 index; `None` for any other rank, so a
/// `[N, M]` target never passes for its leading dim alone.
fn rank_one_literal_dim(index: IndexId, types: &TypeTable) -> Option<i64> {
    match i64s_from_literal_index(index, types)?.as_slice() {
        [dim] => Some(*dim),
        _ => None,
    }
}

fn validate_flat_source(
    source_elem: TypeId,
    target_elem: TypeId,
    types: &TypeTable,
) -> Result<(), ListaToTensorError> {
    if matches!(
        normalized_type(source_elem, types),
        Type::Array(_) | Type::BoundedArray { .. }
    ) {
        return Err(ListaToTensorError::NestedLista);
    }

    if !types.equals(source_elem, target_elem) {
        return Err(ListaToTensorError::ElementMismatch);
    }

    Ok(())
}

/// Whether a type is a numeric scalar eligible as a Route C padded-tensor
/// element (fill contract: zero is a real value, not `MaybeUninit`).
#[must_use]
pub fn is_numeric_element(ty: TypeId, types: &TypeTable) -> bool {
    matches!(
        normalized_type(ty, types),
        Type::Primitive(Primitive::Numerus | Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
    )
}

/// Route C shape facts shared by the padded route and literal adoption: the
/// source element, the target tensor element, the bounded capacity, and the
/// literal bound `N`, when `source` is a numeric bounded lista whose capacity
/// matches the rank-1 literal dim of the tensor `target`.
fn padded_bound(
    source: TypeId,
    target: TypeId,
    types: &TypeTable,
) -> Option<(TypeId, TypeId, IndexId, i64)> {
    match (
        normalized_type(source, types),
        normalized_type(target, types),
    ) {
        (Type::BoundedArray { element, capacity }, Type::Tensor(target_elem, shape)) => {
            let elem = *element;
            if !is_numeric_element(elem, types) {
                return None;
            }
            let dim = rank_one_literal_dim(*shape, types)?;
            let bound = rank_one_literal_dim(*capacity, types)?;
            (dim == bound).then_some((elem, *target_elem, *capacity, dim))
        }
        _ => None,
    }
}

/// Route C padded snapshot facts: the element type and the literal bound `N`,
/// when `source` is a numeric bounded lista whose element type is the tensor
/// `target`'s element and whose capacity matches the rank-1 literal dim.
///
/// The element rule is the flat route's (`validate_flat_source`): a source
/// whose element differs is not a padded snapshot and takes the flat route's
/// `ElementMismatch` rejection.
#[must_use]
pub fn resolve_bounded_lista_padded_shape(
    source: TypeId,
    target: TypeId,
    types: &TypeTable,
) -> Option<(TypeId, i64)> {
    let (elem, target_elem, _, dim) = padded_bound(source, target, types)?;
    types.equals(elem, target_elem).then_some((elem, dim))
}

/// Route C facts for a list *literal* whose element type is not yet the
/// tensor's element: the source element, the tensor element, and the bound
/// `N`. A literal's constants adopt the tensor element by store rules, so the
/// caller admits it element by element instead of by type equality.
#[must_use]
pub fn padded_literal_facts(
    source: TypeId,
    target: TypeId,
    types: &TypeTable,
) -> Option<(TypeId, TypeId, i64)> {
    let (elem, target_elem, _, dim) = padded_bound(source, target, types)?;
    Some((elem, target_elem, dim))
}

/// Analyze a rectangular lista literal against a known tensor target type.
///
/// # Errors
///
/// Returns [`RectangularListaLiteralError`] when the literal is not a valid
/// rectangular representation of the target tensor type.
pub fn analyze_rectangular_lista_literal<'a>(
    expr: &'a HirExpression,
    target: TypeId,
    types: &TypeTable,
) -> Result<RectangularListaLiteral<'a>, RectangularListaLiteralError> {
    let literal_target = rank_two_literal_target(target, types)?;
    let HirExpressionKind::Array(rows) = &expr.kind else {
        return Err(RectangularListaLiteralError::NonNestedLiteral);
    };
    if rows.is_empty() {
        return Err(RectangularListaLiteralError::Ragged);
    }
    if rows.len() != literal_target.rows {
        return Err(RectangularListaLiteralError::ShapeMismatch);
    }

    // Capacity from the walked leaves only — declared dims are untrusted
    // input and must never size an allocation (SEC-A6).
    let mut flat_leaves = Vec::new();
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
    let Type::Tensor(elem, _) = normalized_type(target, types) else {
        return Err(RectangularListaLiteralError::NonNestedLiteral);
    };
    let elem = *elem;
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
        elem,
        rows,
        cols,
        dims,
    })
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
        return Err(RectangularListaLiteralError::NonNestedLiteral);
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
            .ok_or(RectangularListaLiteralError::AbsentLeafType)?;
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
