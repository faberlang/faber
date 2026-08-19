//! Tests for tensor bracket index list recognition.
//!
//! These tests prove that bracket-indexed list expressions, bracket-indexed
//! list types, and tensor types are correctly identified by the surface-type
//! recognition helpers.

use super::tensor_bracket::{
    is_bracket_index_list_expr, is_bracket_index_list_type, is_tensor_type,
};
use crate::{Primitive, TypeTable};
use radix_hir::{HirExpression, HirExpressionKind, HirId, HirLiteral};
use radix_lexer::Span;

#[test]
fn is_tensor_type_recognizes_tensor_surface() {
    let mut types = TypeTable::new();
    let elem = types.sized_numeric(Primitive::Fractus, crate::NumericWidth::F32);
    let rank0 = types.rank0_shape();
    let tensor = types.tensor_with_shape(elem, rank0);

    assert!(is_tensor_type(&types, tensor));
    assert!(!is_tensor_type(&types, elem));
}

#[test]
fn is_bracket_index_list_expr_detects_literal_and_typed_list() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let list = types.array(numerus);

    let literal = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Array(vec![]),
        ty: Some(list),
        span: Span::default(),
    };
    assert!(is_bracket_index_list_expr(&literal, &types));

    let scalar = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Literal(HirLiteral::Int(0)),
        ty: Some(numerus),
        span: Span::default(),
    };
    assert!(!is_bracket_index_list_expr(&scalar, &types));
    assert!(!is_bracket_index_list_type(&types, numerus));
    assert!(is_bracket_index_list_type(&types, list));
}
