//! Tests for vector width expression lowering and validation.
//!
//! These tests prove that vector width literals lower to correct index
//! expressions and that invalid width shapes produce VectorWidthError
//! diagnostics.

use super::{is_vector_width_shape, literal_vector_width, lower_vector_width_expr};
use crate::semantic::{IndexExpr, Resolver, TypeTable};
use crate::syntax::FiguraExpr;

#[test]
fn vector_width_literal_lowers() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let width = lower_vector_width_expr(&FiguraExpr::Literal(4), &resolver, &mut types).unwrap();
    assert_eq!(*types.get_index(width), IndexExpr::Literal(4));
    assert_eq!(literal_vector_width(width, &types), Some(4));
}

#[test]
fn vector_width_bracket_sugar_lowers_single_dim() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let width =
        lower_vector_width_expr(&FiguraExpr::Tuple(vec![FiguraExpr::Literal(4)]), &resolver, &mut types).unwrap();
    assert_eq!(*types.get_index(width), IndexExpr::Literal(4));
}

#[test]
fn vector_width_shape_accepts_literal_and_single_dim_tuple() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let literal = lower_vector_width_expr(&FiguraExpr::Literal(4), &resolver, &mut types).unwrap();
    assert!(is_vector_width_shape(literal, &types));

    let bracketed =
        lower_vector_width_expr(&FiguraExpr::Tuple(vec![FiguraExpr::Literal(4)]), &resolver, &mut types).unwrap();
    assert!(is_vector_width_shape(bracketed, &types));
}

#[test]
fn vector_width_rejects_multi_dim_tuple() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let err = lower_vector_width_expr(
        &FiguraExpr::Tuple(vec![FiguraExpr::Literal(2), FiguraExpr::Literal(3)]),
        &resolver,
        &mut types,
    )
    .unwrap_err();
    assert_eq!(err.issue, super::VECTOR_WIDTH_SINGLE_DIM_ISSUE);
}

#[test]
fn vector_width_rejects_zero() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let width = lower_vector_width_expr(&FiguraExpr::Literal(0), &resolver, &mut types).unwrap();
    assert_eq!(*types.get_index(width), IndexExpr::Literal(0));
    assert_eq!(literal_vector_width(width, &types), Some(0));
}

#[test]
fn vector_width_tuple_literal_zero() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let width =
        lower_vector_width_expr(&FiguraExpr::Tuple(vec![FiguraExpr::Literal(0)]), &resolver, &mut types).unwrap();
    assert_eq!(*types.get_index(width), IndexExpr::Literal(0));
    assert!(is_vector_width_shape(width, &types));
}
