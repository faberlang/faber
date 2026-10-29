//! Grouped max/min fallback lowering pins.

use super::*;
use radix_hir::{HirArrayElement, HirExpression, HirId, HirLiteral, HirReductionOp};
use radix_types::IndexExpr;

const SOURCE: DefId = DefId(1);
const SOURCE_NAME: Symbol = Symbol(1);
const COORDINATE: DefId = DefId(2);
const COORDINATE_NAME: Symbol = Symbol(2);

#[test]
fn grouped_max_lowers_the_empty_bucket_identity() {
    let mut types = TypeTable::new();
    let float = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let integer = types.primitive(Primitive::Numerus);
    types.ensure_array(float);
    let key_list = types.ensure_array(integer);
    let source_extent = types.intern_index(IndexExpr::Literal(4));
    let source_shape = types.intern_index(IndexExpr::Tuple(vec![source_extent]));
    let result_extent = types.intern_index(IndexExpr::Literal(3));
    let result_shape = types.intern_index(IndexExpr::Tuple(vec![result_extent]));
    let source_ty = types.tensor_with_shape(float, source_shape);
    let result_ty = types.tensor_with_shape(float, result_shape);

    let mut builder = FunctionBuilder::new(&types);
    builder.add_param(SOURCE, SOURCE_NAME, source_ty, HirParamMode::Owned, Span::default());
    let entry = builder.fresh_block(Span::default());
    builder.switch_to(entry);

    let source = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Path(SOURCE),
        ty: Some(source_ty),
        span: Span::default(),
    };
    let key = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Literal(HirLiteral::Int(0)),
        ty: Some(integer),
        span: Span::default(),
    };
    let group = HirExpression {
        id: HirId(3),
        kind: HirExpressionKind::Array(vec![HirArrayElement::Expr(key)]),
        ty: Some(key_list),
        span: Span::default(),
    };
    let identity = HirExpression {
        id: HirId(4),
        kind: HirExpressionKind::Literal(HirLiteral::Float(0.5)),
        ty: Some(float),
        span: Span::default(),
    };
    let expression =
        HirExpression { id: HirId(5), kind: HirExpressionKind::Vacua, ty: Some(result_ty), span: Span::default() };
    let coordinates = [(COORDINATE, COORDINATE_NAME)];

    let result = builder.lower_reduction_expr(
        HirReductionOp::Max,
        &source,
        &coordinates,
        Some(&group),
        None,
        None,
        None,
        Some(&identity),
        &expression,
    );

    assert!(result.is_some(), "grouped max lowering failed: {:?}", builder.errors);
    assert!(
        builder
            .blocks
            .iter()
            .flat_map(|block| &block.statements)
            .any(|statement| {
                matches!(
                    &statement.kind,
                    MirStatementKind::Assign {
                        value: MirValue {
                            kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(value))),
                            ..
                        },
                        ..
                    } if *value == 0.5
                )
            }),
        "the supplied fallback is assigned on the empty-bucket edge"
    );
}
