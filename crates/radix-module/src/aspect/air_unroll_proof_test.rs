//! Focused proof-source and recognizer tests for APA-L1.

use super::{UNROLL_PROOF_PRELUDE, UnrollRoute, emit_unroll_equivalence_theorem, unroll_route_facts};
use crate::hir::{HirArrayElement, HirExpression, HirExpressionKind, HirId, HirIteraMode, HirLiteral};
use crate::lexer::Span;
use crate::semantic::{IndexExpr, Primitive, TypeTable};

fn expr(id: u32, kind: HirExpressionKind) -> HirExpression {
    HirExpression { id: HirId(id), kind, ty: None, span: Span::new(0, 1) }
}

fn int_expr(id: u32, value: u64) -> HirExpression {
    expr(id, HirExpressionKind::Literal(HirLiteral::Int(value)))
}

fn interval(id: u32, start: i64, end: i64, step: Option<i64>, inclusive: bool) -> HirExpression {
    let neg = |inner: u64, sub: u32| -> Box<HirExpression> {
        Box::new(expr(
            id + sub,
            HirExpressionKind::Unary(crate::hir::HirUnOp::Neg, Box::new(int_expr(id + sub + 1, inner))),
        ))
    };
    let start_expr = if start < 0 {
        neg(start.unsigned_abs(), 0)
    } else {
        Box::new(int_expr(id + 1, start.unsigned_abs()))
    };
    let end_expr = if end < 0 {
        neg(end.unsigned_abs(), 5)
    } else {
        Box::new(int_expr(id + 6, end.unsigned_abs()))
    };
    let step_expr = step.map(|step| {
        if step < 0 {
            neg(step.unsigned_abs(), 10)
        } else {
            Box::new(int_expr(id + 11, step.unsigned_abs()))
        }
    });
    expr(
        id,
        HirExpressionKind::Intervallum {
            start: start_expr,
            end: end_expr,
            step: step_expr,
            kind: if inclusive {
                crate::hir::HirRangeKind::Inclusive
            } else {
                crate::hir::HirRangeKind::Exclusive
            },
        },
    )
}

fn array_source(id: u32, elements: u64, element_ty: crate::semantic::TypeId) -> HirExpression {
    let mut source = expr(
        id,
        HirExpressionKind::Array(
            (0..elements)
                .map(|_| HirArrayElement::Expr(int_expr(id + 1, 0)))
                .collect(),
        ),
    );
    source.ty = Some(element_ty);
    source
}

#[test]
fn route_facts_enumerate_literal_ab_ranges_in_source_order() {
    let types = TypeTable::new();
    let source = interval(1, 0, 3, None, false);

    let facts = unroll_route_facts(HirIteraMode::Ab, 0, &source, &types);

    assert_eq!(
        facts,
        Some(super::UnrollRouteFacts { route: UnrollRoute::Ab, lanes: vec![0, 1, 2] })
    );
}

#[test]
fn route_facts_cover_inclusive_negative_and_stepped_ranges() {
    let types = TypeTable::new();
    let inclusive = unroll_route_facts(HirIteraMode::Ab, 0, &interval(1, 0, 2, None, true), &types);
    assert_eq!(inclusive.map(|facts| facts.lanes), Some(vec![0, 1, 2]));

    let stepped = unroll_route_facts(HirIteraMode::Ab, 0, &interval(40, 0, 6, Some(2), false), &types);
    assert_eq!(stepped.map(|facts| facts.lanes), Some(vec![0, 2, 4]));
}

#[test]
fn route_facts_take_range_direction_from_the_bounds() {
    let types = TypeTable::new();

    // The pass counts down for `3‥0` with no step: the step is a positive
    // stride and the bounds pick the direction.
    let descending = unroll_route_facts(HirIteraMode::Ab, 0, &interval(20, 3, 0, None, false), &types);
    assert_eq!(descending.map(|facts| facts.lanes), Some(vec![3, 2, 1]));

    let descending_inclusive = unroll_route_facts(HirIteraMode::Ab, 0, &interval(20, 3, 0, None, true), &types);
    assert_eq!(descending_inclusive.map(|facts| facts.lanes), Some(vec![3, 2, 1, 0]));

    let descending_stepped = unroll_route_facts(HirIteraMode::Ab, 0, &interval(40, 6, 0, Some(2), false), &types);
    assert_eq!(descending_stepped.map(|facts| facts.lanes), Some(vec![6, 4, 2]));
}

#[test]
fn route_facts_decline_non_positive_explicit_steps() {
    let types = TypeTable::new();

    // The pass declines a step that is not a positive stride, so the
    // recognizer must not hand the proof line a lane sequence for it.
    for (id, start, end, step) in [(20, 3, 0, -1), (40, 0, 3, -1), (60, 0, 3, 0)] {
        let source = interval(id, start, end, Some(step), false);
        assert!(
            unroll_route_facts(HirIteraMode::Ab, 0, &source, &types).is_none(),
            "step {step} over {start}..{end} must decline"
        );
    }
}

#[test]
fn route_facts_decline_non_literal_and_de_mode_sources() {
    let types = TypeTable::new();
    let path_source = expr(1, HirExpressionKind::Path(crate::hir::DefId(7)));
    assert!(unroll_route_facts(HirIteraMode::Ab, 0, &path_source, &types).is_none());
    assert!(unroll_route_facts(HirIteraMode::De, 0, &interval(2, 0, 3, None, false), &types).is_none());
}

#[test]
fn route_facts_decline_over_budget_ranges() {
    let types = TypeTable::new();
    let source = interval(1, 0, 100, None, false);

    assert!(unroll_route_facts(HirIteraMode::Ab, 0, &source, &types).is_none());
}

#[test]
fn route_facts_admit_bounded_array_literal_sources() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let capacity = types.intern_index(IndexExpr::Literal(3));
    let bounded = types.bounded_array(numerus, capacity);
    let source = array_source(1, 3, bounded);

    let facts = unroll_route_facts(HirIteraMode::Ex, 0, &source, &types);

    assert_eq!(
        facts,
        Some(super::UnrollRouteFacts { route: UnrollRoute::Ex, lanes: vec![0, 1, 2] })
    );
}

#[test]
fn route_facts_decline_bounded_array_sources_without_proven_length() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let capacity = types.intern_index(IndexExpr::Literal(3));
    let bounded = types.bounded_array(numerus, capacity);
    let short = array_source(1, 2, bounded);
    let untyped = {
        let mut source = array_source(2, 3, bounded);
        source.ty = None;
        source
    };

    assert!(unroll_route_facts(HirIteraMode::Ex, 0, &short, &types).is_none());
    assert!(unroll_route_facts(HirIteraMode::Ex, 0, &untyped, &types).is_none());
}

#[test]
fn route_facts_admit_rank_one_tensor_and_coordinate_routes() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let shape = types.intern_index(IndexExpr::Literal(3));
    let tensor = types.tensor_with_shape(numerus, shape);
    let mut source = expr(1, HirExpressionKind::Path(crate::hir::DefId(9)));
    source.ty = Some(tensor);

    let rank_one = unroll_route_facts(HirIteraMode::Ex, 0, &source, &types);
    assert_eq!(rank_one.map(|facts| facts.lanes), Some(vec![0, 1, 2]));

    // Route M: coordinate arity == rank; lanes are the product of dims.
    let outer = types.intern_index(IndexExpr::Literal(2));
    let inner = types.intern_index(IndexExpr::Literal(3));
    let dims = types.intern_index(IndexExpr::Tuple(vec![outer, inner]));
    let matrix = types.tensor_with_shape(numerus, dims);
    let mut matrix_source = expr(2, HirExpressionKind::Path(crate::hir::DefId(10)));
    matrix_source.ty = Some(matrix);

    let route_m = unroll_route_facts(HirIteraMode::Ex, 2, &matrix_source, &types);
    assert_eq!(route_m.map(|facts| facts.lanes), Some((0..6).collect::<Vec<_>>()));

    // Coordinate-free multi-dim sources defer (no obligation).
    assert!(unroll_route_facts(HirIteraMode::Ex, 0, &matrix_source, &types).is_none());
    // Coordinate arity must match the rank.
    assert!(unroll_route_facts(HirIteraMode::Ex, 1, &matrix_source, &types).is_none());
}

#[test]
fn route_facts_decline_unknown_shapes() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let param_shape = types.intern_index(IndexExpr::Param(crate::lexer::Symbol(1)));
    let tensor = types.tensor_with_shape(numerus, param_shape);
    let mut source = expr(1, HirExpressionKind::Path(crate::hir::DefId(9)));
    source.ty = Some(tensor);

    assert!(unroll_route_facts(HirIteraMode::Ex, 0, &source, &types).is_none());
}

#[test]
fn generated_unroll_proof_is_structural_and_float_free() {
    let source = format!(
        "{}\n{}",
        UNROLL_PROOF_PRELUDE,
        emit_unroll_equivalence_theorem("aspect_unroll_equivalence_7", &[0, 1, 2])
    );

    assert!(source.contains("unrolledLanesMatchSource"));
    assert!(source.contains("unrollEvaluationPreservesSourceOrder"));
    assert!(source.contains("unrollFailureBehaviorPreserved"));
    assert!(source.contains("unrolledStraightLine [0, 1, 2]"));
    assert!(!source.contains(": True := trivial"));
    assert!(!source.contains("f32"));
    assert!(!source.contains("f64"));
    assert!(!source.contains("Float"));
    assert_eq!(
        crate::aspect::LeanAspectChecker::scan_for_sorry(&source),
        crate::aspect::SorryVerdict::Clean
    );
}

#[test]
fn generated_unroll_obligation_reuses_master_theorems_for_negative_lanes() {
    let source = emit_unroll_equivalence_theorem("aspect_unroll_equivalence_9", &[-2, -1]);
    assert!(source.contains("unrolledStraightLine [-2, -1]"));
    assert!(source.contains("unrollEvaluationPreservesSourceOrder step [-2, -1] 0"));
    assert!(source.contains("isAbort (sourceLaneLoop step [-2, -1] 0)"));
}

#[test]
fn generated_unroll_prelude_carries_no_float_types() {
    let lowered = UNROLL_PROOF_PRELUDE.to_lowercase();
    assert!(!lowered.contains("f32"));
    assert!(!lowered.contains("f64"));
    assert!(!lowered.contains(": float"));
    assert!(!lowered.contains("float _"));
}
