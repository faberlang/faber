//! Tests for figura expression lowering and formatting.
//!
//! These tests prove that figura (shape) expressions are correctly lowered
//! from syntax to index expressions and that format_figura produces
//! expected surface syntax.

use super::{format_figura, lower_figura_expr};
use crate::figura::{
    FIGURA_ARITHMETIC_DIVIDE_BY_ZERO_MSG, FIGURA_ARITHMETIC_NEGATIVE_MSG,
    FIGURA_ARITHMETIC_NON_INTEGRAL_MSG,
};
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

#[test]
fn lower_figura_arithmetic_checks_static_naturals_and_division() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();

    let negative = lower_figura_expr(
        &FiguraExpr::Subtract(Box::new(FiguraExpr::Literal(0)), 1),
        &resolver,
        &mut types,
    );
    assert_eq!(negative, Err(FIGURA_ARITHMETIC_NEGATIVE_MSG));

    let non_integral = lower_figura_expr(
        &FiguraExpr::Divide(Box::new(FiguraExpr::Literal(5)), 2),
        &resolver,
        &mut types,
    );
    assert_eq!(non_integral, Err(FIGURA_ARITHMETIC_NON_INTEGRAL_MSG));

    let zero_divisor = lower_figura_expr(
        &FiguraExpr::Divide(Box::new(FiguraExpr::Literal(5)), 0),
        &resolver,
        &mut types,
    );
    assert_eq!(zero_divisor, Err(FIGURA_ARITHMETIC_DIVIDE_BY_ZERO_MSG));

    let exact = lower_figura_expr(
        &FiguraExpr::Divide(Box::new(FiguraExpr::Literal(6)), 2),
        &resolver,
        &mut types,
    )
    .expect("exact static division");
    assert!(matches!(types.get_index(exact), IndexExpr::Literal(3)));

    let canceled_subtraction = FiguraExpr::Add(
        Box::new(FiguraExpr::Subtract(Box::new(FiguraExpr::Literal(0)), 1)),
        1,
    );
    let zero = lower_figura_expr(&canceled_subtraction, &resolver, &mut types)
        .expect("the complete expression is natural");
    assert!(matches!(types.get_index(zero), IndexExpr::Literal(0)));

    let canceled_division = FiguraExpr::Add(
        Box::new(FiguraExpr::Subtract(
            Box::new(FiguraExpr::Divide(Box::new(FiguraExpr::Literal(5)), 2)),
            1,
        )),
        1,
    );
    assert_eq!(
        lower_figura_expr(&canceled_division, &resolver, &mut types),
        Err(FIGURA_ARITHMETIC_NON_INTEGRAL_MSG),
        "affine cancellation must not erase an exact-division obligation"
    );
}

#[test]
fn format_figura_keeps_arithmetic_precedence() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let n = types.intern_index(IndexExpr::Param(interner.intern("N")));
    let minus_one = radix_types::add_index_constant(&mut types, n, -1).unwrap();
    let divide = radix_types::divide_index_exact(&mut types, minus_one, 2).unwrap();
    let divided_n = radix_types::divide_index_exact(&mut types, n, 2).unwrap();
    let plus_three = radix_types::add_index_constant(&mut types, divided_n, 3).unwrap();

    assert_eq!(format_figura(divide, &types, &interner), "(N - 1) / 2");
    assert_eq!(format_figura(plus_three, &types, &interner), "N / 2 + 3");
}

#[test]
fn affine_zero_offset_prints_as_its_base_without_parens() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let n = types.intern_index(IndexExpr::Param(interner.intern("N")));

    let zero_affine = types.intern_index(IndexExpr::Affine { base: n, offset: 0 });
    let divide_zero = types.intern_index(IndexExpr::Divide {
        numerator: zero_affine,
        divisor: 2,
    });
    assert_eq!(format_figura(divide_zero, &types, &interner), "N / 2");

    let plus_one = types.intern_index(IndexExpr::Affine { base: n, offset: 1 });
    let divide_one = types.intern_index(IndexExpr::Divide {
        numerator: plus_one,
        divisor: 2,
    });
    assert_eq!(format_figura(divide_one, &types, &interner), "(N + 1) / 2");
}
