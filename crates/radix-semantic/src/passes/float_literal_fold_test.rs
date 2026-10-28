//! F9 ruling 31 (inf-U7, lit1): literal-only float expressions are exact
//! rationals rounded once at the slot; typed operands stay IEEE per
//! operation; the front end emits one literal. Exact float comparisons are the
//! point of these tests.
#![allow(clippy::float_cmp, clippy::unreadable_literal)]

use radix_hir::{
    HirBinOp, HirExpression, HirExpressionKind, HirLiteral, HirStatementKind, HirUnOp,
};
use radix_target::Target;

use crate::PassConfig;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config_la};

fn analyze(body: &str, config: PassConfig) -> AnalyzedFixture {
    let result = analyze_source_with_config_la(&format!("incipit {{\n{body}\n}}\n"), config);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    result
}

/// Initializer expressions of the `incipit` locals, in source order.
fn inits_with(body: &str, config: PassConfig) -> Vec<HirExpression> {
    let result = analyze(body, config);
    let hir = result.hir.as_ref().expect("hir");
    let entry = hir.entry.as_ref().expect("incipit");
    entry
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => local.init.clone(),
            _ => None,
        })
        .collect()
}

fn inits(body: &str) -> Vec<HirExpression> {
    inits_with(body, PassConfig::for_target(Target::HirRust))
}

/// The last local's initializer.
fn last_init(body: &str) -> HirExpression {
    inits(body).pop().expect("a local")
}

/// The value of an expression that is one folded literal (`-x` is `Neg` over
/// the literal, the parser's shape for a negative literal); `None` otherwise.
fn literal_value(expr: &HirExpression) -> Option<f64> {
    match &expr.kind {
        HirExpressionKind::Literal(
            HirLiteral::FloatText { value, .. } | HirLiteral::Float(value),
        ) => Some(*value),
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => literal_value(inner).map(|v| -v),
        _ => None,
    }
}

fn folded(body: &str) -> f64 {
    let expr = last_init(body);
    literal_value(&expr).unwrap_or_else(|| panic!("not folded: {:?}", expr.kind))
}

fn is_binary(expr: &HirExpression) -> bool {
    matches!(expr.kind, HirExpressionKind::Binary(..))
}

#[test]
fn point_one_plus_point_two_folds_to_the_f64_nearest_three_tenths() {
    let value = folded("sit fractus c ← 0.1 + 0.2");
    assert_eq!(value.to_bits(), 0.3_f64.to_bits());
    assert_ne!(value.to_bits(), (0.1_f64 + 0.2_f64).to_bits());
}

#[test]
fn the_folded_expression_is_one_float_literal() {
    let expr = last_init("sit fractus c ← 0.1 + 0.2");
    assert!(
        matches!(expr.kind, HirExpressionKind::Literal(HirLiteral::Float(_))),
        "{:?}",
        expr.kind
    );
}

#[test]
fn an_f32_slot_rounds_once_from_the_exact_value() {
    // 3/10 once into f32; an f64 sum narrowed would also give this here, so the
    // discriminating case below sits just above an f32 midpoint.
    assert_eq!(folded("sit f32 x ← 0.1 + 0.2"), f64::from(0.3_f32));
    let above = 1.0_f32 + f32::EPSILON;
    let value = folded("sit f32 x ← 1.00000005960464477539062501 + 0.0");
    assert_eq!(value, f64::from(above));
    // Through f64 first, the same exact value ties to even and lands on 1.0.
    let through_f64: f64 = "1.00000005960464477539062501".parse().unwrap();
    assert_eq!(f64_to_f32(through_f64), 1.0_f32);
}

#[allow(clippy::cast_possible_truncation)] // the point is the narrowing
fn f64_to_f32(value: f64) -> f32 {
    value as f32
}

#[test]
fn f16_and_bf16_slots_round_once_from_the_exact_value() {
    assert_eq!(folded("sit f16 x ← 0.1 + 0.2"), 0.300_048_828_125);
    assert_eq!(folded("sit bf16 x ← 0.1 + 0.2"), 0.300_781_25);
}

#[test]
fn one_third_is_one_exact_rational() {
    assert_eq!(
        folded("sit fractus t ← 1.0 / 3.0").to_bits(),
        (1.0_f64 / 3.0).to_bits()
    );
    assert_eq!(folded("sit fractus t ← 1.0 / 3.0 * 3.0"), 1.0);
    assert_eq!(
        folded("sit f32 t ← 1.0 / 3.0").to_bits(),
        f64::from(1.0_f32 / 3.0).to_bits()
    );
}

#[test]
fn parenthesised_sum_times_ten_is_exactly_three() {
    assert_eq!(folded("sit fractus t ← (0.1 + 0.2) * 10.0"), 3.0);
    assert_eq!(folded("sit fractus t ← 10.0 * (0.1 + 0.2)"), 3.0);
    assert_eq!(folded("sit fractus t ← 0.7 + 0.1"), 0.8);
    assert_eq!(folded("sit fractus t ← 0.1 * 3.0"), 0.3);
}

#[test]
fn remainder_and_true_division_fold_exactly() {
    assert_eq!(folded("sit fractus t ← 7.5 % 2.0"), 1.5);
    assert_eq!(folded("sit fractus t ← -7.5 % 2.0"), 0.5);
    assert_eq!(folded("sit fractus t ← 7.5 % -2.0"), -0.5);
    assert_eq!(folded("sit fractus t ← 0.3 % 0.1"), 0.0);
    assert_eq!(folded("sit fractus t ← 1.0 ÷ 4.0"), 0.25);
}

#[test]
fn negative_results_keep_the_negative_literal_shape() {
    let expr = last_init("sit fractus t ← 0.1 - 0.3");
    let HirExpressionKind::Unary(HirUnOp::Neg, inner) = &expr.kind else {
        panic!("{:?}", expr.kind)
    };
    assert!(matches!(
        inner.kind,
        HirExpressionKind::Literal(HirLiteral::Float(_))
    ));
    assert_eq!(literal_value(&expr), Some(-0.2));
    assert_eq!(folded("sit fractus t ← -(0.1 + 0.2)"), -0.3);
}

#[test]
fn large_and_small_exponents_keep_the_exact_intermediate() {
    // The product overflows f64 (and underflows, below) but the exact value does not.
    assert_eq!(folded("sit fractus t ← 1e300 * 1e300 / 1e300"), 1e300);
    assert_eq!(folded("sit fractus t ← 1e-300 * 1e-300 / 1e-300"), 1e-300);
    assert_eq!(folded("sit fractus t ← 1.5e3 + 2.5e-3"), 1500.0025);
}

#[test]
fn a_result_that_overflows_the_slot_is_not_folded() {
    assert!(is_binary(&last_init("sit fractus t ← 1e300 * 1e300")));
    assert!(is_binary(&last_init("sit f32 t ← 1e30 * 1e30")));
}

#[test]
fn a_zero_divisor_is_left_to_run_time() {
    assert!(is_binary(&last_init("sit fractus t ← 1.0 / 0.0")));
    assert!(is_binary(&last_init("sit fractus t ← 1.0 % 0.0")));
    assert!(is_binary(&last_init("sit fractus t ← 0.5 + 1.0 / 0.0")));
}

#[test]
fn typed_operands_stay_ieee_per_operation() {
    let exprs = inits("sit fractus d ← 0.1\nsit fractus e ← 0.2\nsit fractus s ← d + e");
    let sum = exprs.last().unwrap();
    let HirExpressionKind::Binary(HirBinOp::Add, lhs, rhs) = &sum.kind else {
        panic!("{:?}", sum.kind)
    };
    assert!(matches!(lhs.kind, HirExpressionKind::Path(_)));
    assert!(matches!(rhs.kind, HirExpressionKind::Path(_)));
}

#[test]
fn a_variable_operand_stops_folding() {
    let exprs = inits(
        "sit fractus a ← 0.1\nsit fractus left ← a + 0.1 + 0.2\nsit fractus right ← 0.1 + 0.2 + a",
    );
    // `(a + 0.1) + 0.2`: nothing is literal-only, nothing reassociates.
    let HirExpressionKind::Binary(HirBinOp::Add, inner, constant) = &exprs[1].kind else {
        panic!("{:?}", exprs[1].kind)
    };
    assert!(is_binary(inner));
    assert_eq!(literal_value(constant), Some(0.2));
    // `(0.1 + 0.2) + a`: the literal-only left operand folds, the join stays.
    let HirExpressionKind::Binary(HirBinOp::Add, folded_left, variable) = &exprs[2].kind else {
        panic!("{:?}", exprs[2].kind)
    };
    assert_eq!(literal_value(folded_left), Some(0.3));
    assert!(matches!(variable.kind, HirExpressionKind::Path(_)));
}

#[test]
fn a_parenthesised_constant_beside_a_variable_folds_at_its_own_join() {
    let exprs = inits("sit fractus a ← 0.1\nsit fractus s ← a + (0.1 + 0.2)");
    let HirExpressionKind::Binary(HirBinOp::Add, _, constant) = &exprs[1].kind else {
        panic!("{:?}", exprs[1].kind)
    };
    assert_eq!(literal_value(constant), Some(0.3));
}

#[test]
fn a_method_call_on_a_literal_is_not_constant() {
    let expr = last_init("sit fractus p ← (0.1 + 0.2).potentia(2.0)");
    let HirExpressionKind::MethodCall(receiver, ..) = &expr.kind else {
        panic!("{:?}", expr.kind)
    };
    // The receiver is a constant expression; the call itself is not.
    assert_eq!(literal_value(receiver), Some(0.3));
}

#[test]
fn an_integer_typed_leaf_is_never_folded_as_an_exact_quotient() {
    // `7 / 2` is integer floor division; its leaves are integer-typed, so the
    // fold leaves the expression to today's evaluation.
    assert!(is_binary(&last_init("sit fractus t ← 7 / 2 * 0.5")));
    assert!(is_binary(&last_init("sit fractus t ← 1 / 3 * 3.0")));
}

#[test]
fn an_integer_constant_expression_in_a_float_slot_folds_to_a_float_literal() {
    // Ruling 20: typecheck leaves the integer literals integer-typed under the
    // float-typed root. Without the fold the runner kept the integer in the
    // float slot and printed `300`.
    let expr = last_init("sit fractus c ← 200 + 100");
    assert!(
        matches!(expr.kind, HirExpressionKind::Literal(HirLiteral::Float(value)) if value.to_bits() == 300.0_f64.to_bits()),
        "{:?}",
        expr.kind
    );
    assert_eq!(folded("sit f32 x ← 2 * 3"), 6.0);
    // The integer operators run first: `/` and `%` floor, then one conversion.
    assert_eq!(folded("sit fractus t ← 7 / 2"), 3.0);
    assert_eq!(folded("sit fractus t ← (200 + 100) / 2"), 150.0);
    assert_eq!(folded("sit fractus t ← 7 % 4"), 3.0);
    assert_eq!(folded("sit fractus t ← 1 ⇐ 4"), 16.0);
    assert_eq!(folded("sit fractus t ← -(2 + 3)"), -5.0);
}

#[test]
fn an_integer_constant_expression_with_a_zero_divisor_is_left_to_run_time() {
    assert!(is_binary(&last_init("sit fractus t ← 7 / 0")));
}

#[test]
fn a_decimal_slot_is_not_rewritten() {
    // Typecheck stores the decimal sum through an implicit conversion; the
    // literal-only sum inside it is decimal-typed and keeps its two literals.
    let expr = last_init("sit d64 e ← 0.1 + 0.2");
    let HirExpressionKind::Conversio { source, .. } = &expr.kind else {
        panic!("{:?}", expr.kind)
    };
    assert!(is_binary(source), "{:?}", source.kind);
}

#[test]
fn a_lone_literal_and_a_negated_literal_are_untouched() {
    let exprs = inits("sit fractus a ← 0.1\nsit fractus b ← -0.1");
    assert!(matches!(exprs[0].kind, HirExpressionKind::Literal(_)));
    let HirExpressionKind::Unary(HirUnOp::Neg, inner) = &exprs[1].kind else {
        panic!("{:?}", exprs[1].kind)
    };
    assert!(matches!(inner.kind, HirExpressionKind::Literal(_)));
}

#[test]
fn source_preserving_targets_and_the_opt_out_keep_the_authors_expression() {
    let body = "sit fractus c ← 0.1 + 0.2";
    assert!(is_binary(
        &inits_with(body, PassConfig::for_target(Target::HirFaber))
            .pop()
            .unwrap()
    ));
    assert!(is_binary(
        &inits_with(body, PassConfig::for_target(Target::HirFhir))
            .pop()
            .unwrap()
    ));
    let mut opted_out = PassConfig::for_target(Target::HirRust);
    opted_out.fold_literal_floats = false;
    assert!(is_binary(&inits_with(body, opted_out).pop().unwrap()));
    assert!(!is_binary(
        &inits_with(body, PassConfig::for_target(Target::HirRust))
            .pop()
            .unwrap()
    ));
}
