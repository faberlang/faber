use super::*;
use crate::IndexExpr;
use radix_lexer::Symbol;

fn make_types() -> TypeTable {
    TypeTable::new()
}

fn lit(types: &mut TypeTable, n: u64) -> IndexId {
    types.intern_index(IndexExpr::Literal(n))
}

fn param(types: &mut TypeTable, id: u32) -> IndexId {
    types.intern_index(IndexExpr::Param(Symbol(id)))
}

fn tuple(types: &mut TypeTable, dims: &[IndexId]) -> IndexId {
    types.intern_index(IndexExpr::Tuple(dims.to_vec()))
}

fn rank0(types: &mut TypeTable) -> IndexId {
    types.rank0_shape()
}

fn n_dim(types: &mut TypeTable) -> IndexId {
    param(types, 1)
}

fn m_dim(types: &mut TypeTable) -> IndexId {
    param(types, 2)
}

fn one_dim(types: &mut TypeTable) -> IndexId {
    lit(types, 1)
}

fn five_dim(types: &mut TypeTable) -> IndexId {
    lit(types, 5)
}

#[test]
fn identical_shapes() {
    let mut types = make_types();
    let n_val = n_dim(&mut types);
    let shape_a = tuple(&mut types, &[n_val]);
    let shape_b = tuple(&mut types, &[n_val]);
    let axes = compute_cotangent_reduction_axes(shape_a, shape_b, &types);
    assert_eq!(axes, Some(vec![]));
}

#[test]
fn scalar_broadcast() {
    let mut types = make_types();
    let one_val = one_dim(&mut types);
    let n_val = n_dim(&mut types);
    let op = tuple(&mut types, &[one_val]);
    let res = tuple(&mut types, &[n_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, Some(vec![0]));
}

#[test]
fn padded_broadcast() {
    let mut types = make_types();
    let n_val = n_dim(&mut types);
    let one_val = one_dim(&mut types);
    let m_val = m_dim(&mut types);
    let op = tuple(&mut types, &[n_val, one_val]);
    let res = tuple(&mut types, &[n_val, m_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, Some(vec![1]));
}

#[test]
fn left_padded_broadcast() {
    let mut types = make_types();
    let m_val = m_dim(&mut types);
    let n_val = n_dim(&mut types);
    let op = tuple(&mut types, &[m_val]);
    let res = tuple(&mut types, &[n_val, m_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, Some(vec![]));
}

#[test]
fn both_operands_broadcast() {
    let mut types = make_types();
    let n_val = n_dim(&mut types);
    let one_val = one_dim(&mut types);
    let m_val = m_dim(&mut types);
    let op = tuple(&mut types, &[n_val, one_val]);
    let res = tuple(&mut types, &[n_val, m_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, Some(vec![1]));
}

#[test]
fn incompatible_shapes_test() {
    let mut types = make_types();
    let five_val = five_dim(&mut types);
    let one_val = one_dim(&mut types);
    let op = tuple(&mut types, &[five_val]);
    let res = tuple(&mut types, &[one_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, None);
}

#[test]
fn incompatible_different_literals() {
    let mut types = make_types();
    let five_val = five_dim(&mut types);
    let three_val = lit(&mut types, 3);
    let op = tuple(&mut types, &[five_val]);
    let res = tuple(&mut types, &[three_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, None);
}

#[test]
fn empty_shapes() {
    let mut types = make_types();
    let op = rank0(&mut types);
    let res = rank0(&mut types);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, Some(vec![]));
}

#[test]
fn scalar_to_1d() {
    let mut types = make_types();
    let op = rank0(&mut types);
    let n_val = n_dim(&mut types);
    let res = tuple(&mut types, &[n_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, Some(vec![]));
}

#[test]
fn both_stretch_same_dimension() {
    let mut types = make_types();
    let one_val = one_dim(&mut types);
    let m_val = m_dim(&mut types);
    let n_val = n_dim(&mut types);
    let op = tuple(&mut types, &[one_val, m_val]);
    let res = tuple(&mut types, &[n_val, m_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, Some(vec![0]));
}

#[test]
fn multi_axis_reduction() {
    let mut types = make_types();
    let one_val = one_dim(&mut types);
    let another_one = lit(&mut types, 1);
    let n_val = n_dim(&mut types);
    let m_val = m_dim(&mut types);
    let op = tuple(&mut types, &[one_val, another_one]);
    let res = tuple(&mut types, &[n_val, m_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, Some(vec![0, 1]));
}

#[test]
fn operand_larger_rank_result() {
    let mut types = make_types();
    let n_val = n_dim(&mut types);
    let m_val = m_dim(&mut types);
    let op = tuple(&mut types, &[n_val, m_val]);
    let res = tuple(&mut types, &[n_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, None);
}

#[test]
fn same_shape_no_reduction() {
    let mut types = make_types();
    let n_val = n_dim(&mut types);
    let m_val = m_dim(&mut types);
    let op = tuple(&mut types, &[n_val, m_val]);
    let res = tuple(&mut types, &[n_val, m_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, Some(vec![]));
}

#[test]
fn single_element_broadcast_no_reduction() {
    let mut types = make_types();
    let one_val = one_dim(&mut types);
    let op = tuple(&mut types, &[one_val]);
    let res = tuple(&mut types, &[one_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, Some(vec![]));
}

#[test]
fn operand_larger_than_result_is_not_broadcast_compatible() {
    let mut types = make_types();
    let n_val = n_dim(&mut types);
    let m_val = m_dim(&mut types);
    let op = tuple(&mut types, &[n_val, m_val]);
    let res = tuple(&mut types, &[m_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, None);
}

#[test]
fn non_one_operand_dim_against_one_result_dim_is_incompatible() {
    let mut types = make_types();
    let n_val = n_dim(&mut types);
    let one_val = one_dim(&mut types);
    let op = tuple(&mut types, &[n_val]);
    let res = tuple(&mut types, &[one_val]);
    let axes = compute_cotangent_reduction_axes(op, res, &types);
    assert_eq!(axes, None);
}

#[test]
fn non_tuple_index_expr() {
    let mut types = make_types();
    let lit_id = lit(&mut types, 5);
    let n_val = n_dim(&mut types);
    let tuple_id = tuple(&mut types, &[n_val]);
    let axes = compute_cotangent_reduction_axes(lit_id, tuple_id, &types);
    assert_eq!(axes, None);
}
