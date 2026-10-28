//! Tests for literal index extraction from figura expressions.
//!
//! These tests prove that `i64s_from_literal_index` correctly extracts rank-0
//! and tuple literal index values from interned `IndexExpr` entries.

use super::i64s_from_literal_index;
use crate::{TypeTable, index::IndexExpr};

#[test]
fn i64s_from_literal_index_extracts_rank0_and_tuple_literals() {
    let mut types = TypeTable::new();
    let rank0 = types.rank0_shape();
    assert_eq!(i64s_from_literal_index(rank0, &types), Some(vec![]));

    let d2 = types.intern_index(IndexExpr::Literal(2));
    let d3 = types.intern_index(IndexExpr::Literal(3));
    let tuple = types.intern_index(IndexExpr::Tuple(vec![d2, d3]));
    assert_eq!(i64s_from_literal_index(tuple, &types), Some(vec![2, 3]));

    let hole = types.intern_index(IndexExpr::Infer(crate::IndexVar(0)));
    assert_eq!(i64s_from_literal_index(hole, &types), None);
}

#[test]
fn single_element_tuple_extracts_correctly() {
    let mut types = TypeTable::new();
    let d5 = types.intern_index(IndexExpr::Literal(5));
    let tuple = types.intern_index(IndexExpr::Tuple(vec![d5]));
    assert_eq!(i64s_from_literal_index(tuple, &types), Some(vec![5]));
}

#[test]
fn empty_tuple_returns_empty_vec() {
    let mut types = TypeTable::new();
    let empty = types.intern_index(IndexExpr::Tuple(vec![]));
    assert_eq!(i64s_from_literal_index(empty, &types), Some(vec![]));
}

#[test]
fn unnamed_var_returns_none() {
    let mut types = TypeTable::new();
    let unspecified = types.intern_index(IndexExpr::Unspecified);
    assert_eq!(i64s_from_literal_index(unspecified, &types), None);
}
