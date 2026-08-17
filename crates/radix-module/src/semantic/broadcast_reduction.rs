//! Broadcast cotangent reduction query.
//!
//! When an elementwise tensor op broadcasts (e.g. `tensor<f32, [N]> +
//! tensor<f32, [1]>`), the forward pass stretches size-1 dimensions. The
//! backward pass must reduce the cotangent from the broadcast result shape
//! back to each operand's original shape. This module provides the shared
//! read-only query that computes which axes need sum-reduction.
//!
//! The algorithm mirrors [`super::IndexUnifier::unify_broadcast`] trailing
//! alignment: left-pad the shorter shape with literal `1` dims, then mark
//! every axis where the operand has literal `1` and the result does not.

use super::{IndexId, TypeTable};

/// Compute which axes need sum-reduction when reducing a broadcast result
/// cotangent back to an operand's original shape.
///
/// Re-exported from `radix-types` — see [`radix_types::compute_cotangent_reduction_axes`].
///
/// Returns `None` if the shapes are not broadcast-compatible (defense-in-
/// depth — the forward typecheck already guarantees compatibility).
/// Returns `Some(axes)` where `axes` is a (possibly empty) list of axis
/// indices in **operand-local** space (0-indexed from operand's own rank,
/// not the padded rank). An empty list means no reduction is needed
/// (same-shape or operand shape equals result shape after alignment).
#[must_use]
pub fn compute_cotangent_reduction_axes(
    operand_shape: IndexId,
    result_shape: IndexId,
    types: &TypeTable,
) -> Option<Vec<usize>> {
    radix_types::compute_cotangent_reduction_axes(operand_shape, result_shape, types)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Symbol;
    use crate::semantic::IndexExpr;

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

    fn _n(types: &mut TypeTable) -> IndexId {
        param(types, 1)
    }

    fn _m(types: &mut TypeTable) -> IndexId {
        param(types, 2)
    }

    fn _one(types: &mut TypeTable) -> IndexId {
        lit(types, 1)
    }

    fn _five(types: &mut TypeTable) -> IndexId {
        lit(types, 5)
    }

    // ── Tests ──────────────────────────────────────────────────────

    #[test]
    fn identical_shapes() {
        let mut types = make_types();
        let n_val = _n(&mut types);
        let shape_a = tuple(&mut types, &[n_val]);
        let shape_b = tuple(&mut types, &[n_val]);
        let axes = compute_cotangent_reduction_axes(shape_a, shape_b, &types);
        assert_eq!(axes, Some(vec![]));
    }

    #[test]
    fn scalar_broadcast() {
        // operand=[1], result=[N] → Some([0])
        let mut types = make_types();
        let one_val = _one(&mut types);
        let n_val = _n(&mut types);
        let op = tuple(&mut types, &[one_val]);
        let res = tuple(&mut types, &[n_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![0]));
    }

    #[test]
    fn padded_broadcast() {
        // operand=[N, 1], result=[N, M] → Some([1])
        let mut types = make_types();
        let n_val = _n(&mut types);
        let one_val = _one(&mut types);
        let m_val = _m(&mut types);
        let op = tuple(&mut types, &[n_val, one_val]);
        let res = tuple(&mut types, &[n_val, m_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![1]));
    }

    #[test]
    fn left_padded_broadcast() {
        // operand=[M], result=[N, M] → after pad: [1, M] vs [N, M]
        // axis 0: 1 vs N → stretched (padding, no operand-local axis)
        // axis 1: M vs M → same → no reduction
        // Result: empty axes — total Sum reduces everything (including padding axes)
        let mut types = make_types();
        let m_val = _m(&mut types);
        let n_val = _n(&mut types);
        let op = tuple(&mut types, &[m_val]);
        let res = tuple(&mut types, &[n_val, m_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![]));
    }

    #[test]
    fn both_operands_broadcast() {
        // operand=[N, 1], result=[N, M] — axis 1 reduces
        let mut types = make_types();
        let n_val = _n(&mut types);
        let one_val = _one(&mut types);
        let m_val = _m(&mut types);
        let op = tuple(&mut types, &[n_val, one_val]);
        let res = tuple(&mut types, &[n_val, m_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![1]));
    }

    #[test]
    fn incompatible_shapes_test() {
        // operand=[5], result=[1] — 5 vs 1: op not 1, res is 1 → no reduction
        let mut types = make_types();
        let five_val = _five(&mut types);
        let one_val = _one(&mut types);
        let op = tuple(&mut types, &[five_val]);
        let res = tuple(&mut types, &[one_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![]));
    }

    #[test]
    fn incompatible_different_literals() {
        // operand=[5], result=[3] where neither is 1 → None
        let mut types = make_types();
        let five_val = _five(&mut types);
        let three_val = lit(&mut types, 3);
        let op = tuple(&mut types, &[five_val]);
        let res = tuple(&mut types, &[three_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, None);
    }

    #[test]
    fn empty_shapes() {
        // operand=[], result=[] → Some([])
        let mut types = make_types();
        let op = rank0(&mut types);
        let res = rank0(&mut types);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![]));
    }

    #[test]
    fn scalar_to_1d() {
        // operand=[], result=[N] → Some([]) — scalar, total Sum handles it
        let mut types = make_types();
        let op = rank0(&mut types);
        let n_val = _n(&mut types);
        let res = tuple(&mut types, &[n_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![]));
    }

    #[test]
    fn both_stretch_same_dimension() {
        // operand=[1, M], result=[N, M] → Some([0])
        let mut types = make_types();
        let one_val = _one(&mut types);
        let m_val = _m(&mut types);
        let n_val = _n(&mut types);
        let op = tuple(&mut types, &[one_val, m_val]);
        let res = tuple(&mut types, &[n_val, m_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![0]));
    }

    #[test]
    fn multi_axis_reduction() {
        // operand=[1, 1], result=[N, M] → Some([0, 1])
        let mut types = make_types();
        let one_val = _one(&mut types);
        let another_one = lit(&mut types, 1);
        let n_val = _n(&mut types);
        let m_val = _m(&mut types);
        let op = tuple(&mut types, &[one_val, another_one]);
        let res = tuple(&mut types, &[n_val, m_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![0, 1]));
    }

    #[test]
    fn operand_larger_rank_result() {
        let mut types = make_types();
        let n_val = _n(&mut types);
        let m_val = _m(&mut types);
        let op = tuple(&mut types, &[n_val, m_val]);
        let res = tuple(&mut types, &[n_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, None);
    }

    #[test]
    fn same_shape_no_reduction() {
        let mut types = make_types();
        let n_val = _n(&mut types);
        let m_val = _m(&mut types);
        let op = tuple(&mut types, &[n_val, m_val]);
        let res = tuple(&mut types, &[n_val, m_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![]));
    }

    #[test]
    fn single_element_broadcast_no_reduction() {
        let mut types = make_types();
        let one_val = _one(&mut types);
        let op = tuple(&mut types, &[one_val]);
        let res = tuple(&mut types, &[one_val]);
        let axes = compute_cotangent_reduction_axes(op, res, &types);
        assert_eq!(axes, Some(vec![]));
    }

    #[test]
    fn non_tuple_index_expr() {
        let mut types = make_types();
        let lit_id = lit(&mut types, 5);
        let n_val = _n(&mut types);
        let tuple_id = tuple(&mut types, &[n_val]);
        let axes = compute_cotangent_reduction_axes(lit_id, tuple_id, &types);
        assert_eq!(axes, None);
    }
}
