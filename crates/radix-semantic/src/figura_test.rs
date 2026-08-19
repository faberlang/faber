//! Tests for figura expression lowering and formatting.
//!
//! These tests prove that figura (shape) expressions are correctly lowered
//! from syntax to index expressions and that format_figura produces
//! expected surface syntax.

use super::{format_figura, lower_figura_expr};
use crate::index::{IndexExpr, IndexVar};
use crate::{Resolver, TypeTable};
use radix_lexer::Interner;
use radix_syntax::FiguraExpr;

#[test]
fn lower_figura_literal_tuple_and_infer() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();

    let lit = lower_figura_expr(&FiguraExpr::Literal(3), &resolver, &mut types).unwrap();
    assert!(matches!(types.get_index(lit), IndexExpr::Literal(3)));

    let hole = lower_figura_expr(&FiguraExpr::Infer, &resolver, &mut types).unwrap();
    assert!(matches!(
        types.get_index(hole),
        IndexExpr::Infer(IndexVar(0))
    ));

    let tuple = lower_figura_expr(
        &FiguraExpr::Tuple(vec![FiguraExpr::Literal(2), FiguraExpr::Literal(3)]),
        &resolver,
        &mut types,
    )
    .unwrap();
    let IndexExpr::Tuple(dims) = types.get_index(tuple) else {
        panic!("expected tuple");
    };
    assert_eq!(dims.len(), 2);
}

#[test]
fn lower_figura_infer_holes_are_distinct_across_calls() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let h0 = lower_figura_expr(&FiguraExpr::Infer, &resolver, &mut types).unwrap();
    let h1 = lower_figura_expr(&FiguraExpr::Infer, &resolver, &mut types).unwrap();
    assert_ne!(h0, h1);
    assert!(matches!(types.get_index(h0), IndexExpr::Infer(IndexVar(0))));
    assert!(matches!(types.get_index(h1), IndexExpr::Infer(IndexVar(1))));
}

#[test]
fn format_figura_renders_literals_tuple_and_holes() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let d2 = types.intern_index(IndexExpr::Literal(2));
    let d3 = types.intern_index(IndexExpr::Literal(3));
    let tuple = types.intern_index(IndexExpr::Tuple(vec![d2, d3]));
    let rank0 = types.rank0_shape();
    let hole = types.intern_index(IndexExpr::Infer(IndexVar(0)));

    assert_eq!(format_figura(tuple, &types, &interner), "[2, 3]");
    assert_eq!(format_figura(rank0, &types, &interner), "[]");
    assert_eq!(format_figura(hole, &types, &interner), "_");
    assert_eq!(
        format_figura(types.unspecified_shape(), &types, &interner),
        "_"
    );
}
