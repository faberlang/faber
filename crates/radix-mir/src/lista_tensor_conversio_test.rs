use super::*;
use radix_hir::{HirArrayElement, HirExpression, HirExpressionKind, HirId, HirLiteral};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth};

fn bounded_lista(types: &mut TypeTable, elem: TypeId, n: u64) -> TypeId {
    let capacity = types.intern_index(IndexExpr::Literal(n));
    types.intern(Type::BoundedArray {
        element: elem,
        capacity,
    })
}

#[test]
fn bounded_numeric_source_admits_padded_snapshot() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let lista = bounded_lista(&mut types, f32, 8);
    let shape = types.intern_index(IndexExpr::Literal(8));
    let tensor = types.intern(Type::Tensor(f32, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Ok(())
    );
    assert_eq!(
        resolve_bounded_lista_padded_shape(lista, tensor, &types),
        Some((f32, 8))
    );
}

#[test]
fn bounded_non_numeric_source_rejects() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let lista = bounded_lista(&mut types, textus, 8);
    let shape = types.intern_index(IndexExpr::Literal(8));
    let tensor = types.intern(Type::Tensor(textus, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Err(ListaToTensorError::NonNumericElement)
    );
}

#[test]
fn bounded_capacity_mismatch_rejects() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let lista = bounded_lista(&mut types, f32, 8);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor = types.intern(Type::Tensor(f32, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Err(ListaToTensorError::CapacityMismatch)
    );
}

#[test]
fn unbounded_source_stays_on_flat_path() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let lista = types.array(f32);
    let shape = types.intern_index(IndexExpr::Literal(8));
    let tensor = types.intern(Type::Tensor(f32, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Ok(())
    );
    assert_eq!(
        resolve_bounded_lista_padded_shape(lista, tensor, &types),
        None
    );
}

#[test]
fn rectangular_literal_analysis_sizes_from_walked_leaves_not_declared_dims() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    // The declared col is a valid i64 dim, so it passes `dim_to_usize`; sizing
    // the buffer from it aborts before the per-row mismatch is ever reported.
    let target = tensor_type(&mut types, numerus, &[1, 9_223_372_036_854_775_800]);
    let expr = nested_int_literal(&mut types, numerus, &[&[1]]);

    assert!(matches!(
        analyze_rectangular_lista_literal(&expr, target, &types),
        Err(RectangularListaLiteralError::RowLengthMismatch)
    ));
}

fn tensor_type(types: &mut TypeTable, elem: TypeId, dims: &[u64]) -> TypeId {
    let mut dim_ids = Vec::with_capacity(dims.len());
    for dim in dims {
        dim_ids.push(types.intern_index(IndexExpr::Literal(*dim)));
    }
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    types.intern(Type::Tensor(elem, shape))
}

fn nested_int_literal(types: &mut TypeTable, elem: TypeId, rows: &[&[u64]]) -> HirExpression {
    let row_ty = types.array(elem);
    let outer_ty = types.array(row_ty);
    HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Array(
            rows.iter()
                .enumerate()
                .map(|(row_index, row)| {
                    HirArrayElement::Expr(HirExpression {
                        id: HirId(u32::try_from(row_index + 1).expect("fixture index fits u32")),
                        kind: HirExpressionKind::Array(
                            row.iter()
                                .enumerate()
                                .map(|(col_index, value)| {
                                    HirArrayElement::Expr(HirExpression {
                                        id: HirId(
                                            u32::try_from(10 + row_index * 10 + col_index)
                                                .expect("fixture index fits u32"),
                                        ),
                                        kind: HirExpressionKind::Literal(HirLiteral::Int(*value)),
                                        ty: Some(elem),
                                        span: Span::default(),
                                    })
                                })
                                .collect(),
                        ),
                        ty: Some(row_ty),
                        span: Span::default(),
                    })
                })
                .collect(),
        ),
        ty: Some(outer_ty),
        span: Span::default(),
    }
}

#[test]
fn bounded_source_rejects_rank_two_target_whose_leading_dim_matches_capacity() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let lista = bounded_lista(&mut types, f32, 8);
    // `tensor<f32, [8, 3]>`: the leading dim equals the capacity, but the
    // padded snapshot only fills a rank-1 `[N]` tensor.
    let tensor = tensor_type(&mut types, f32, &[8, 3]);

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Err(ListaToTensorError::CapacityMismatch)
    );
    assert_eq!(
        resolve_bounded_lista_padded_shape(lista, tensor, &types),
        None
    );
}
