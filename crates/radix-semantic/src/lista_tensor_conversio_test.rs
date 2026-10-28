//! Tests for rectangular lista-to-tensor shape resolution.
//!
//! These tests prove that rectangular list literals resolve to tensor shapes
//! and that irregular or type-mismatched lists produce the expected
//! `ListaToTensorError` diagnostics.

use super::lista_tensor_conversio::{
    ListaToTensorError, ListaToTensorShape, RectangularListaLiteralError,
    analyze_rectangular_lista_literal, ensure_lista_numerus_type, padded_literal_facts,
    resolve_bounded_lista_padded_shape, resolve_lista_to_tensor_shape,
    validate_lista_to_tensor_source,
};
use crate::test_support::{assert_accepts_la, assert_rejects_with_issue_la};
use crate::{IndexExpr, Primitive, Type, TypeId, TypeTable};
use radix_hir::{HirArrayElement, HirExpression, HirExpressionKind, HirId, HirLiteral};
use radix_lexer::Span;

#[test]
fn lista_tensor_conversio_resolves_literal_rank_one_shape() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let shape = types.intern_index(IndexExpr::Literal(3));
    let target = types.intern(Type::Tensor(numerus, shape));

    assert_eq!(
        resolve_lista_to_tensor_shape(target, &types),
        Some(ListaToTensorShape::Literal(vec![3]))
    );
}

#[test]
fn lista_tensor_conversio_resolves_literal_rank_two_shape() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, crate::NumericWidth::F32);
    let two = types.intern_index(IndexExpr::Literal(2));
    let three = types.intern_index(IndexExpr::Literal(3));
    let shape = types.intern_index(IndexExpr::Tuple(vec![two, three]));
    let target = types.intern(Type::Tensor(f32, shape));

    assert_eq!(
        resolve_lista_to_tensor_shape(target, &types),
        Some(ListaToTensorShape::Literal(vec![2, 3]))
    );
}

#[test]
fn lista_tensor_conversio_uses_infer_one_d_for_non_literal_shape() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let shape = types.fresh_index_infer_var();
    let shape = types.intern_index(IndexExpr::Infer(shape));
    let target = types.intern(Type::Tensor(numerus, shape));

    assert_eq!(
        resolve_lista_to_tensor_shape(target, &types),
        Some(ListaToTensorShape::InferOneD)
    );
}

#[test]
fn lista_tensor_conversio_validates_flat_source() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let lista = types.array(numerus);
    let shape = types.intern_index(IndexExpr::Literal(3));
    let tensor = types.intern(Type::Tensor(numerus, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Ok(())
    );
}

#[test]
fn lista_tensor_conversio_rejects_nested_source() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let inner = types.array(numerus);
    let nested = types.array(inner);
    let shape = types.intern_index(IndexExpr::Literal(3));
    let tensor = types.intern(Type::Tensor(numerus, shape));

    assert_eq!(
        validate_lista_to_tensor_source(nested, tensor, &types),
        Err(ListaToTensorError::NestedLista)
    );
}

#[test]
fn lista_tensor_conversio_ensures_shape_list_type() {
    let mut types = TypeTable::new();
    let shape_ty = ensure_lista_numerus_type(&mut types);

    assert!(matches!(
        types.get(shape_ty),
        Type::Array(elem) if types.equals(*elem, types.primitive(Primitive::Numerus))
    ));
}

#[test]
fn rectangular_lista_literal_analyzer_flattens_rank_two_literal() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let target = tensor_type(&mut types, numerus, &[2, 2]);
    let expr = nested_int_literal(&mut types, numerus, &[&[1, 2], &[3, 4]]);

    let analyzed =
        analyze_rectangular_lista_literal(&expr, target, &types).expect("rectangular literal");

    assert_eq!(analyzed.leaf_elem, numerus);
    assert_eq!(analyzed.dims, vec![2, 2]);
    assert_eq!(analyzed.flat_leaves.len(), 4);
}

#[test]
fn rectangular_lista_literal_analyzer_rejects_ragged_rows() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let target = tensor_type(&mut types, numerus, &[2, 2]);
    let expr = nested_int_literal(&mut types, numerus, &[&[1, 2], &[3]]);

    assert!(matches!(
        analyze_rectangular_lista_literal(&expr, target, &types),
        Err(RectangularListaLiteralError::RowLengthMismatch)
    ));
}

#[test]
fn rectangular_lista_literal_analyzer_rejects_shape_mismatch() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let target = tensor_type(&mut types, numerus, &[3, 3]);
    let expr = nested_int_literal(&mut types, numerus, &[&[1, 2], &[3, 4]]);

    assert!(matches!(
        analyze_rectangular_lista_literal(&expr, target, &types),
        Err(RectangularListaLiteralError::ShapeMismatch)
    ));
}

#[test]
fn rectangular_lista_literal_analyzer_sizes_from_walked_leaves_not_declared_dims() {
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

fn tensor_type(types: &mut TypeTable, elem: crate::TypeId, dims: &[u64]) -> crate::TypeId {
    let mut dim_ids = Vec::with_capacity(dims.len());
    for dim in dims {
        dim_ids.push(types.intern_index(IndexExpr::Literal(*dim)));
    }
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    types.intern(Type::Tensor(elem, shape))
}

fn nested_int_literal(
    types: &mut TypeTable,
    elem: crate::TypeId,
    rows: &[&[u64]],
) -> HirExpression {
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

fn bounded_lista(types: &mut TypeTable, elem: TypeId, n: u64) -> TypeId {
    let capacity = types.intern_index(IndexExpr::Literal(n));
    types.intern(Type::BoundedArray {
        element: elem,
        capacity,
    })
}

#[test]
fn lista_tensor_conversio_admits_bounded_numeric_source() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, crate::NumericWidth::F32);
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
fn lista_tensor_conversio_rejects_bounded_non_numeric_source() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let lista = bounded_lista(&mut types, textus, 8);
    let shape = types.intern_index(IndexExpr::Literal(8));
    let tensor = types.intern(Type::Tensor(textus, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Err(ListaToTensorError::NonNumericElement)
    );
    assert_eq!(
        resolve_bounded_lista_padded_shape(lista, tensor, &types),
        None
    );
}

#[test]
fn lista_tensor_conversio_rejects_capacity_dim_mismatch() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, crate::NumericWidth::F32);
    let lista = bounded_lista(&mut types, f32, 8);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor = types.intern(Type::Tensor(f32, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Err(ListaToTensorError::CapacityMismatch)
    );
    assert_eq!(
        resolve_bounded_lista_padded_shape(lista, tensor, &types),
        None
    );
}

#[test]
fn lista_tensor_conversio_keeps_unbounded_source_on_flat_path() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, crate::NumericWidth::F32);
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

// Check-level coverage for the conversion target's `_` extents: the target
// type is authoritative, so one hole infers from the flat element count.

#[test]
fn lista_tensor_conversio_solves_single_target_hole_from_flat_count() {
    // 4 flat elements over the declared extent [2, _] pin the hole to 2.
    assert_accepts_la(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
  sit tensor<f32, _> shaped ← flat ↦ tensor<f32, [2, _]>
  nota shaped.longitudo()
}",
    );
}

#[test]
fn lista_tensor_conversio_names_indivisible_flat_count_for_target_hole() {
    // 4 flat elements do not divide over [3, _]; the index contradiction is
    // named at the conversion like every other tensor count mismatch.
    assert_rejects_with_issue_la(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
  sit tensor<f32, _> shaped ← flat ↦ tensor<f32, [3, _]>
  nota shaped.longitudo()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn lista_tensor_conversio_refuses_multi_hole_target() {
    // Two holes admit no unique division; the target stays unsolved and the
    // finalize hole check names it.
    assert_rejects_with_issue_la(
        r"incipit {
  sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
  sit tensor<f32, _> shaped ← flat ↦ tensor<f32, [_, _]>
  nota shaped.longitudo()
}",
        "cannot_infer_tensor_shape",
    );
}

#[test]
fn lista_tensor_conversio_rejects_bounded_source_with_mismatched_target_element() {
    // `lista<numerus, 8> ↦ tensor<textus, [8]>` must not take the padded
    // snapshot route: the target element type is part of the contract.
    assert_rejects_with_issue_la(
        r"functio selecta(lista<numerus, 8> src) → tensor<textus, [8]> {
  redde src ↦ tensor<textus, [8]>
}",
        "lista_to_tensor_element_mismatch",
    );
}

#[test]
fn lista_tensor_conversio_accepts_literal_list_for_narrow_float_tensor() {
    // A list literal's element type is not yet the tensor element: the flat
    // route accepts it, so the padded route must not reject it either.
    assert_accepts_la(
        r"incipit {
  sit tf32[3] row ← [1.0, 2.0, 3.0] ↦ tf32[3]
  nota row.longitudo()
}",
    );
}

#[test]
fn lista_tensor_conversio_accepts_literal_list_for_long_form_tensor() {
    assert_accepts_la(
        r"incipit {
  sit tensor<f32, [2]> pair ← [1.0, 2.0] ↦ tensor<f32, [2]>
  nota pair.longitudo()
}",
    );
}

#[test]
fn lista_tensor_conversio_padded_shape_requires_matching_target_element() {
    // The padded snapshot fills dead slots with source-element values, so a
    // target tensor of a different element type is not a padded snapshot.
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let lista = bounded_lista(&mut types, numerus, 8);
    let shape = types.intern_index(IndexExpr::Literal(8));
    let tensor = types.intern(Type::Tensor(textus, shape));

    assert_eq!(
        validate_lista_to_tensor_source(lista, tensor, &types),
        Err(ListaToTensorError::ElementMismatch)
    );
    assert_eq!(
        resolve_bounded_lista_padded_shape(lista, tensor, &types),
        None
    );
}

#[test]
fn lista_tensor_conversio_rejects_literal_list_for_text_tensor() {
    // A literal adopts the tensor element by store rules, so a numeric
    // literal cannot be stored into a `textus` element.
    let result = crate::test_support::analyze_source_la(
        r"incipit {
  sit tensor<textus, [3]> t ← [1, 2, 3] ↦ tensor<textus, [3]>
  nota t.longitudo()
}",
    );
    assert!(!result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn lista_tensor_conversio_rejects_rank_two_target_whose_leading_dim_matches_capacity() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, crate::NumericWidth::F32);
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
    assert_eq!(padded_literal_facts(lista, tensor, &types), None);
}

#[test]
fn lista_tensor_conversio_rejects_rank_two_target_for_bounded_source_at_check() {
    assert_rejects_with_issue_la(
        r"functio selecta(lista<f32, 8> src) → tensor<f32, [8, 3]> {
  redde src ↦ tensor<f32, [8, 3]>
}",
        "lista_to_tensor_capacity_mismatch",
    );
}
