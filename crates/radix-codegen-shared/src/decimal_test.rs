//! Decimal carrier math pins for codegen tiers (decimal-widths D5).

use super::{
    carrier_in_width, decimal_binop_width, decimal_expr_width, decimal_leaf_scale,
    decimal_literal_payload, decimal_scale_factor, decimal_source_width, decimal_text_to_carrier,
    round_half_even_quotient,
};
use radix_hir::{
    DefId, HirBinOp, HirBlock, HirCasuArm, HirExpression, HirExpressionKind, HirId, HirPattern,
};
use radix_lexer::Span;
use radix_types::{NumericWidth, Primitive, SemanticMutability, Type, TypeId, TypeTable};

#[test]
fn decimal_scale_factors_match_the_cto_ruling() {
    assert_eq!(decimal_scale_factor(NumericWidth::D64), Some(100_000_000));
    assert_eq!(decimal_scale_factor(NumericWidth::I32), None);
}

#[test]
fn round_half_even_quotient_pins() {
    assert_eq!(round_half_even_quotient(5, 2), Some(2));
    assert_eq!(round_half_even_quotient(7, 2), Some(4));
    assert_eq!(round_half_even_quotient(-5, 2), Some(-2));
    assert_eq!(round_half_even_quotient(-7, 2), Some(-4));
    assert_eq!(round_half_even_quotient(6, 2), Some(3));
    assert_eq!(round_half_even_quotient(1, 0), None);
}

#[test]
fn decimal_text_to_carrier_rounds_half_even_at_the_width_scale() {
    // d64 scale = 8 fractional digits: 2.000000005 → 200000000 (even tie
    // down), 2.000000015 → 200000002 (odd tie up).
    assert_eq!(
        decimal_text_to_carrier("2.000000005", NumericWidth::D64),
        Some(200_000_000)
    );
    assert_eq!(
        decimal_text_to_carrier("2.000000015", NumericWidth::D64),
        Some(200_000_002)
    );
    assert_eq!(
        decimal_text_to_carrier("4.2", NumericWidth::D64),
        Some(420_000_000)
    );
    assert_eq!(
        decimal_text_to_carrier("42", NumericWidth::D64),
        Some(4_200_000_000)
    );
}

#[test]
fn decimal_width_helpers_agree_on_decimal_and_non_decimal_types() {
    let mut types = TypeTable::new();
    let d64 = types.sized_numeric(Primitive::Numerus, NumericWidth::D64);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);

    assert_eq!(
        decimal_expr_width(&path(1, d64), &types),
        Some(NumericWidth::D64)
    );
    assert_eq!(decimal_expr_width(&path(1, i64), &types), None);

    let literal = HirExpressionKind::Literal(radix_hir::HirLiteral::FloatText {
        value: 4.2,
        text: radix_lexer::Symbol(0),
    });
    assert_eq!(
        decimal_literal_payload(&typed(1, d64, literal.clone()), &types),
        Some((radix_lexer::Symbol(0), NumericWidth::D64))
    );
    // A non-decimal expression type hides the literal payload.
    assert_eq!(
        decimal_literal_payload(&typed(1, i64, literal), &types),
        None
    );

    // A path recovers its declared binding type; the binding wins over the
    // expression type, and a non-decimal binding stays non-decimal.
    assert_eq!(
        decimal_source_width(&path(1, d64), &types, |_| None),
        Some(NumericWidth::D64)
    );
    assert_eq!(
        decimal_source_width(&path(1, i64), &types, |_| Some(d64)),
        Some(NumericWidth::D64)
    );
    assert_eq!(decimal_source_width(&path(1, i64), &types, |_| None), None);
}

#[test]
fn alias_of_a_decimal_reports_the_decimal_width() {
    let mut types = TypeTable::new();
    let d64 = types.sized_numeric(Primitive::Numerus, NumericWidth::D64);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let alias = types.intern(Type::Alias(radix_types::DefId(9_001), d64));
    let alias_of_alias = types.intern(Type::Alias(radix_types::DefId(9_002), alias));
    let int_alias = types.intern(Type::Alias(radix_types::DefId(9_003), i64));

    assert_eq!(
        decimal_expr_width(&path(1, alias), &types),
        Some(NumericWidth::D64)
    );
    assert_eq!(
        decimal_expr_width(&path(1, alias_of_alias), &types),
        Some(NumericWidth::D64)
    );
    assert_eq!(decimal_expr_width(&path(1, int_alias), &types), None);

    // The leaf scale follows the width: an alias of `d64` is a scale-8 leaf.
    assert_eq!(decimal_leaf_scale(&path(1, alias), &types), 8);
    assert_eq!(decimal_leaf_scale(&path(1, int_alias), &types), 0);

    // An alias of `d64` joins decimal arithmetic as a decimal operand.
    assert_eq!(
        decimal_binop_width(HirBinOp::Add, &path(1, alias), &path(2, alias), &types),
        Some(NumericWidth::D64)
    );

    // A `ref` access marker over an aliased decimal is still the decimal.
    let referenced = types.reference(SemanticMutability::Immutable, alias);
    assert_eq!(
        decimal_expr_width(&path(1, referenced), &types),
        Some(NumericWidth::D64)
    );
}

#[test]
fn carrier_in_width_rejects_out_of_range_values() {
    assert_eq!(
        carrier_in_width(i128::from(i64::MAX) + 1, NumericWidth::D64),
        None
    );
    assert_eq!(
        carrier_in_width(i128::from(i64::MAX), NumericWidth::D64),
        Some(i64::MAX)
    );
    assert_eq!(carrier_in_width(-1, NumericWidth::I32), None);
}

fn typed(id: u32, ty: TypeId, kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind,
        ty: Some(ty),
        span: Span::default(),
    }
}

fn path(id: u32, ty: TypeId) -> HirExpression {
    typed(id, ty, HirExpressionKind::Path(DefId(id)))
}

fn mul(id: u32, ty: TypeId) -> HirExpression {
    typed(
        id,
        ty,
        HirExpressionKind::Binary(
            HirBinOp::Mul,
            Box::new(path(id + 100, ty)),
            Box::new(path(id + 200, ty)),
        ),
    )
}

fn block_of(tail: HirExpression) -> HirBlock {
    HirBlock::simple(Vec::new(), Some(Box::new(tail)), Span::default())
}

fn si_of(ty: TypeId, then_tail: HirExpression, else_tail: HirExpression) -> HirExpression {
    typed(
        1,
        ty,
        HirExpressionKind::Si {
            cond: Box::new(path(900, ty)),
            then_block: block_of(then_tail),
            then_catch: None,
            else_block: Some(block_of(else_tail)),
        },
    )
}

#[test]
fn branching_construct_yields_exact_decimal_when_a_tail_is_arithmetic() {
    use super::yields_exact_decimal_tail;
    let mut types = TypeTable::new();
    let d64 = types.sized_numeric(Primitive::Numerus, NumericWidth::D64);

    // Either tail being decimal arithmetic is enough.
    assert!(yields_exact_decimal_tail(
        &si_of(d64, mul(10, d64), path(11, d64)),
        &types
    ));
    assert!(yields_exact_decimal_tail(
        &si_of(d64, path(11, d64), mul(10, d64)),
        &types
    ));
    // Stored leaves in every tail stay stored.
    assert!(!yields_exact_decimal_tail(
        &si_of(d64, path(10, d64), path(11, d64)),
        &types
    ));
    // Nested branching tails are followed.
    let nested = typed(
        2,
        d64,
        HirExpressionKind::Block(block_of(si_of(d64, mul(10, d64), path(11, d64)))),
    );
    assert!(yields_exact_decimal_tail(&nested, &types));
    // A `discerne` arm body counts.
    let discerne = typed(
        3,
        d64,
        HirExpressionKind::Discerne {
            scrutinees: vec![path(900, d64)],
            arms: vec![
                HirCasuArm {
                    patterns: vec![HirPattern::Wildcard],
                    body: path(11, d64),
                    span: Span::default(),
                },
                HirCasuArm {
                    patterns: vec![HirPattern::Wildcard],
                    body: mul(10, d64),
                    span: Span::default(),
                },
            ],
            exhaustive: true,
        },
    );
    assert!(yields_exact_decimal_tail(&discerne, &types));
}

#[test]
fn branching_construct_of_other_type_or_plain_arithmetic_is_not_a_decimal_tail() {
    use super::yields_exact_decimal_tail;
    let mut types = TypeTable::new();
    let d64 = types.sized_numeric(Primitive::Numerus, NumericWidth::D64);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    // Not itself decimal-typed.
    assert!(!yields_exact_decimal_tail(
        &si_of(i64, mul(10, d64), path(11, d64)),
        &types
    ));
    // Plain decimal arithmetic is not a branching construct.
    assert!(!yields_exact_decimal_tail(&mul(10, d64), &types));
}
