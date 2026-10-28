//! Tests for index expression interning and variable identity.
//!
//! These tests prove that `IndexExpr` values are interned as distinct `IndexIds`,
//! that `IndexVars` track identity correctly, and that tuple and literal
//! expressions round-trip through the `TypeTable`.

use super::{IndexExpr, IndexVar};
use crate::{Primitive, Type, TypeTable};
use radix_lexer::Symbol;

#[test]
fn intern_index_returns_stable_ids() {
    let mut types = TypeTable::new();
    let a = types.intern_index(IndexExpr::Literal(4));
    let b = types.intern_index(IndexExpr::Literal(4));
    assert_ne!(a, b);
    assert_eq!(types.get_index(a), &IndexExpr::Literal(4));
}

#[test]
fn index_equals_on_literals_params_and_tuples() {
    let mut types = TypeTable::new();
    let lit_a = types.intern_index(IndexExpr::Literal(3));
    let lit_b = types.intern_index(IndexExpr::Literal(3));
    let lit_c = types.intern_index(IndexExpr::Literal(4));
    assert!(types.index_equals(lit_a, lit_b));
    assert!(!types.index_equals(lit_a, lit_c));

    let n = types.intern_index(IndexExpr::Param(Symbol(1)));
    let m = types.intern_index(IndexExpr::Param(Symbol(1)));
    let k = types.intern_index(IndexExpr::Param(Symbol(2)));
    assert!(types.index_equals(n, m));
    assert!(!types.index_equals(n, k));

    let d0 = types.intern_index(IndexExpr::Literal(2));
    let d1 = types.intern_index(IndexExpr::Literal(3));
    let tuple = types.intern_index(IndexExpr::Tuple(vec![d0, d1]));
    let tuple_copy = types.intern_index(IndexExpr::Tuple(vec![d0, d1]));
    let tuple_rev = types.intern_index(IndexExpr::Tuple(vec![d1, d0]));
    assert!(types.index_equals(tuple, tuple_copy));
    assert!(!types.index_equals(tuple, tuple_rev));
}

#[test]
fn rank0_shape_is_empty_tuple() {
    let mut types = TypeTable::new();
    let shape = types.rank0_shape();
    assert!(matches!(types.get_index(shape), IndexExpr::Tuple(dims) if dims.is_empty()));
}

#[test]
fn unspecified_shape_is_singleton_per_table() {
    let mut types = TypeTable::new();
    let a = types.unspecified_shape();
    let b = types.unspecified_shape();
    assert_eq!(a, b);
    assert!(types.get_index(a).is_unspecified());
}

#[test]
fn tensor_equality_requires_element_and_shape() {
    let mut types = TypeTable::new();
    let elem = types.primitive(Primitive::Fractus);
    let shape_a = types.intern_index(IndexExpr::Literal(2));
    let shape_b = types.intern_index(IndexExpr::Literal(3));
    let t1 = types.tensor_with_shape(elem, shape_a);
    let t2 = types.tensor_with_shape(elem, shape_a);
    let t3 = types.tensor_with_shape(elem, shape_b);
    assert!(types.equals(t1, t2));
    assert!(!types.equals(t1, t3));
    assert!(matches!(types.get(t1), Type::Tensor(e, s) if *e == elem && *s == shape_a));
}

#[test]
fn infer_hole_predicate_matches_infer_nodes() {
    let mut types = TypeTable::new();
    let hole = types.intern_index(IndexExpr::Infer(IndexVar(0)));
    let rank0 = types.rank0_shape();
    assert!(types.get_index(hole).is_infer());
    assert!(!types.get_index(rank0).is_infer());
}

#[test]
fn infer_index_vars_equal_by_id() {
    let mut types = TypeTable::new();
    let v0a = types.intern_index(IndexExpr::Infer(IndexVar(0)));
    let v0b = types.intern_index(IndexExpr::Infer(IndexVar(0)));
    let v1 = types.intern_index(IndexExpr::Infer(IndexVar(1)));
    assert!(types.index_equals(v0a, v0b));
    assert!(!types.index_equals(v0a, v1));
}
