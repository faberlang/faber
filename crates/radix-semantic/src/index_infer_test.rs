//! Tests for index inference state and constraint solving.
//!
//! These tests prove that `IndexInferState` correctly produces `IndexConstraint`
//! values and that incompatible index expressions produce expected
//! `IndexInferError` diagnostics.

use super::{IndexConstraint, IndexInferError, IndexInferState};
use crate::TypeTable;
use crate::index::{IndexExpr, IndexVar};

fn lit(types: &mut TypeTable, n: u64) -> crate::IndexId {
    types.intern_index(IndexExpr::Literal(n))
}

fn tuple(types: &mut TypeTable, dims: &[crate::IndexId]) -> crate::IndexId {
    types.intern_index(IndexExpr::Tuple(dims.to_vec()))
}

#[test]
fn fresh_hole_allocates_monotonic_vars() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let h0 = infer.fresh_hole(&mut types);
    let h1 = infer.fresh_hole(&mut types);
    assert_ne!(h0, h1);
    assert!(matches!(types.get_index(h0), IndexExpr::Infer(IndexVar(0))));
    assert!(matches!(types.get_index(h1), IndexExpr::Infer(IndexVar(1))));
}

#[test]
fn solve_equal_binds_hole_to_literal() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let hole = infer.fresh_hole(&mut types);
    let two = lit(&mut types, 2);
    assert!(infer.solve_constraints(&mut types, &[IndexConstraint::Equal(hole, two)]));
    let finalized = infer.finalize(&mut types, hole).expect("solved");
    assert!(matches!(types.get_index(finalized), IndexExpr::Literal(2)));
}

#[test]
fn solve_broadcast_unifies_rank_mismatch() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let d3a = lit(&mut types, 3);
    let d3b = lit(&mut types, 3);
    let one = lit(&mut types, 1);
    let left = tuple(&mut types, &[d3a]);
    let right = tuple(&mut types, &[d3b, one]);
    assert!(infer.solve_constraints(&mut types, &[IndexConstraint::Broadcast(left, right)]));
}

#[test]
fn finalize_rejects_unresolved_hole() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let hole = infer.fresh_hole(&mut types);
    assert_eq!(
        infer.finalize(&mut types, hole),
        Err(IndexInferError::UnboundHole)
    );
}

#[test]
fn finalize_rejects_tuple_with_unresolved_dim() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let hole = infer.fresh_hole(&mut types);
    let d3 = lit(&mut types, 3);
    let shape = tuple(&mut types, &[hole, d3]);

    assert_eq!(
        infer.finalize(&mut types, shape),
        Err(IndexInferError::UnboundHole)
    );
}

#[test]
fn canonicalize_rebuilds_tuple_with_solved_dims() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let hole = infer.fresh_hole(&mut types);
    let d3 = lit(&mut types, 3);
    let shape = tuple(&mut types, &[hole, d3]);
    let two = lit(&mut types, 2);
    assert!(infer.solve_constraints(&mut types, &[IndexConstraint::Equal(hole, two)]));

    let canonical = infer.canonicalize(&mut types, shape);
    let IndexExpr::Tuple(dims) = types.get_index(canonical) else {
        panic!("expected tuple");
    };
    assert!(matches!(types.get_index(dims[0]), IndexExpr::Literal(2)));
    assert!(matches!(types.get_index(dims[1]), IndexExpr::Literal(3)));
}

#[test]
fn chained_constraints_propagate_across_holes() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let h0 = infer.fresh_hole(&mut types);
    let h1 = infer.fresh_hole(&mut types);
    let two = lit(&mut types, 2);
    assert!(infer.solve_constraints(
        &mut types,
        &[
            IndexConstraint::Equal(h0, two),
            IndexConstraint::Equal(h1, h0)
        ],
    ));
    let finalized = infer.finalize(&mut types, h1).expect("chained");
    assert!(matches!(types.get_index(finalized), IndexExpr::Literal(2)));
}

#[test]
fn occurs_rejects_cyclic_constraint() {
    let mut types = TypeTable::new();
    let mut infer = IndexInferState::new();
    let hole = infer.fresh_hole(&mut types);
    let cyclic = tuple(&mut types, &[hole]);
    assert!(!infer.solve_constraints(&mut types, &[IndexConstraint::Equal(hole, cyclic)]));
}
