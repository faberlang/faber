//! Tests for HIR aspect report generation (discerne omnia recognition).
//! Proves: empty-program determinism; span ordering; complete/missing/
//! catchall/catchall-as-variant detection; non-enum and multi-scrutinee
//! skipping.

use super::{
    report_hir_aspects, HirAspectKind, HirAspectNoSpanReason, HirAspectPrecondition, HirAspectReason, HirAspectRecord,
    HirAspectRecordKind, HirAspectSpan,
};
use crate::hir::{
    DefId, HirBinOp, HirBlock, HirCallArg, HirCasuArm, HirEnum, HirExpression, HirExpressionKind, HirId, HirItem,
    HirItemKind, HirLiteral, HirModule, HirPattern, HirStatement, HirStatementKind, HirVariant,
};
use crate::lexer::{Interner, Span};
use crate::semantic::{Primitive, Type, TypeId, TypeTable};
use crate::test_support::test_span_between;

fn lit_expr(id: u32, span: Span, ty: Option<TypeId>) -> HirExpression {
    HirExpression { id: HirId(id), kind: HirExpressionKind::Literal(HirLiteral::Int(1)), ty, span }
}

fn variant_pattern(def_id: u32) -> HirPattern {
    HirPattern::Variant(DefId(def_id), Vec::new())
}

fn discerne_omnia_expr(id: u32, span: Span, scrutinee_ty: Option<TypeId>, patterns: Vec<HirPattern>) -> HirExpression {
    discerne_expr(id, span, true, vec![scrutinee_ty], patterns)
}

fn discerne_expr(
    id: u32,
    span: Span,
    exhaustive: bool,
    scrutinee_tys: Vec<Option<TypeId>>,
    patterns: Vec<HirPattern>,
) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Discerne {
            scrutinees: scrutinee_tys
                .into_iter()
                .enumerate()
                .map(|(index, ty)| HirExpression {
                    id: HirId(id + 1 + u32::try_from(index).expect("fixture index fits u32")),
                    kind: HirExpressionKind::Path(DefId(100 + u32::try_from(index).expect("fixture index fits u32"))),
                    ty,
                    span,
                })
                .collect(),
            arms: vec![HirCasuArm { patterns, guard: None, body: lit_expr(id + 20, span, None), span }],
            exhaustive,
        },
        ty: None,
        span,
    }
}

fn enum_program(exprs: Vec<HirExpression>) -> HirModule {
    let mut program = expr_program(exprs);
    program.items.push(HirItem {
        id: HirId(200),
        def_id: DefId(1),
        kind: HirItemKind::Enum(HirEnum {
            name: crate::lexer::Symbol(1),
            type_params: Vec::new(),
            annotations: Vec::new(),
            commune_fields: Vec::new(),
            variants: vec![
                HirVariant {
                    def_id: DefId(2),
                    name: crate::lexer::Symbol(2),
                    fields: Vec::new(),
                    span: test_span_between(1, 2),
                },
                HirVariant {
                    def_id: DefId(3),
                    name: crate::lexer::Symbol(3),
                    fields: Vec::new(),
                    span: test_span_between(3, 4),
                },
            ],
        }),
        span: test_span_between(1, 4),
    });
    program
}

fn expr_program(exprs: Vec<HirExpression>) -> HirModule {
    HirModule {
        items: Vec::new(),
        entry: Some(HirBlock {
            statements: exprs
                .into_iter()
                .enumerate()
                .map(|(index, expr)| HirStatement {
                    id: HirId(100 + u32::try_from(index).expect("fixture index fits u32")),
                    span: expr.span,
                    kind: HirStatementKind::Expr(expr),
                })
                .collect(),
            expr: None,
            span: test_span_between(0, 0),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

#[test]
fn hir_aspect_report_empty_program_is_deterministic() {
    let program = HirModule {
        items: Vec::new(),
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let types = TypeTable::new();

    let first = report_hir_aspects(&program, &types);
    let second = report_hir_aspects(&program, &types);

    assert_eq!(first, second);
    assert_eq!(first.precondition, HirAspectPrecondition::CheckedHir);
    assert!(first.records().is_empty());
}

#[test]
fn hir_aspect_report_skips_non_exhaustive_discerne() {
    let types = TypeTable::new();
    let program = expr_program(vec![discerne_expr(
        1,
        test_span_between(8, 20),
        false,
        vec![None],
        vec![HirPattern::Wildcard],
    )]);

    let report = report_hir_aspects(&program, &types);

    assert_eq!(report.records().len(), 1);
    assert_eq!(report.records()[0].record_kind, HirAspectRecordKind::SkippedCandidate);
    assert_eq!(report.records()[0].reason, HirAspectReason::NonOmniaDiscerne);
}

#[test]
fn hir_aspect_report_records_discerne_omnia_as_skipped_candidate() {
    let types = TypeTable::new();
    let program = expr_program(vec![discerne_omnia_expr(
        1,
        test_span_between(8, 20),
        None,
        vec![HirPattern::Wildcard],
    )]);

    let report = report_hir_aspects(&program, &types);

    assert_eq!(report.records().len(), 1);
    assert_eq!(report.records()[0].aspect, HirAspectKind::DiscerneOmnia);
    assert_eq!(report.records()[0].record_kind, HirAspectRecordKind::SkippedCandidate);
    assert_eq!(report.records()[0].span, HirAspectSpan::Source(test_span_between(8, 20)));
    assert_eq!(report.records()[0].node_id, Some(HirId(1)));
    assert_eq!(report.records()[0].reason, HirAspectReason::MissingScrutineeType);
}

#[test]
fn hir_aspect_report_orders_records_by_source_span() {
    let mut types = TypeTable::new();
    let enum_ty = types.intern(Type::Enum(DefId(1)));
    let program = enum_program(vec![
        discerne_omnia_expr(
            10,
            test_span_between(30, 40),
            Some(enum_ty),
            vec![variant_pattern(2), variant_pattern(3)],
        ),
        discerne_omnia_expr(
            20,
            test_span_between(5, 12),
            Some(enum_ty),
            vec![variant_pattern(2), variant_pattern(3)],
        ),
    ]);

    let first = report_hir_aspects(&program, &types);
    let second = report_hir_aspects(&program, &types);

    assert_eq!(first, second);
    assert_eq!(
        first
            .records()
            .iter()
            .map(|record| record.node_id)
            .collect::<Vec<_>>(),
        vec![Some(HirId(20)), Some(HirId(10))]
    );
}

#[test]
fn hir_aspect_discerne_omnia_recognizes_complete_enum_coverage() {
    let mut types = TypeTable::new();
    let enum_ty = types.intern(Type::Enum(DefId(1)));
    let program = enum_program(vec![discerne_omnia_expr(
        1,
        test_span_between(8, 20),
        Some(enum_ty),
        vec![variant_pattern(2), variant_pattern(3)],
    )]);

    let report = report_hir_aspects(&program, &types);

    assert_eq!(report.records().len(), 1);
    assert_eq!(report.records()[0].record_kind, HirAspectRecordKind::Recognized);
    assert_eq!(report.records()[0].reason, HirAspectReason::Recognized);
}

#[test]
fn hir_aspect_discerne_omnia_reports_missing_variant_as_not_proven() {
    let mut types = TypeTable::new();
    let enum_ty = types.intern(Type::Enum(DefId(1)));
    let program = enum_program(vec![discerne_omnia_expr(
        1,
        test_span_between(8, 20),
        Some(enum_ty),
        vec![variant_pattern(2)],
    )]);

    let report = report_hir_aspects(&program, &types);

    assert_eq!(report.records().len(), 1);
    assert_eq!(report.records()[0].record_kind, HirAspectRecordKind::NotProven);
    assert_eq!(report.records()[0].reason, HirAspectReason::MissingExplicitVariants);
}

#[test]
fn hir_aspect_discerne_omnia_reports_catchall_as_not_proven() {
    let mut types = TypeTable::new();
    let enum_ty = types.intern(Type::Enum(DefId(1)));
    let program = enum_program(vec![discerne_omnia_expr(
        1,
        test_span_between(8, 20),
        Some(enum_ty),
        vec![HirPattern::Wildcard],
    )]);

    let report = report_hir_aspects(&program, &types);

    assert_eq!(report.records().len(), 1);
    assert_eq!(report.records()[0].record_kind, HirAspectRecordKind::NotProven);
    assert_eq!(report.records()[0].reason, HirAspectReason::NonVariantPattern);
}

#[test]
fn hir_aspect_discerne_omnia_skips_non_enum_scrutinee() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let program = expr_program(vec![discerne_omnia_expr(
        1,
        test_span_between(8, 20),
        Some(numerus),
        vec![variant_pattern(2)],
    )]);

    let report = report_hir_aspects(&program, &types);

    assert_eq!(report.records().len(), 1);
    assert_eq!(report.records()[0].record_kind, HirAspectRecordKind::SkippedCandidate);
    assert_eq!(report.records()[0].reason, HirAspectReason::NonEnumScrutinee);
}

#[test]
fn hir_aspect_discerne_omnia_skips_multi_scrutinee() {
    let mut types = TypeTable::new();
    let enum_ty = types.intern(Type::Enum(DefId(1)));
    let program = enum_program(vec![discerne_expr(
        1,
        test_span_between(8, 20),
        true,
        vec![Some(enum_ty), Some(enum_ty)],
        vec![variant_pattern(2), variant_pattern(3)],
    )]);

    let report = report_hir_aspects(&program, &types);

    assert_eq!(report.records().len(), 1);
    assert_eq!(report.records()[0].record_kind, HirAspectRecordKind::SkippedCandidate);
    assert_eq!(report.records()[0].reason, HirAspectReason::MultipleScrutinees);
}

#[test]
fn hir_aspect_report_model_represents_recognized_and_no_span_records() {
    let record = HirAspectRecord::recognized(
        HirAspectKind::DiscerneOmnia,
        HirAspectSpan::NoSpan(HirAspectNoSpanReason::ProgramSummary),
        None,
    );

    assert_eq!(record.record_kind, HirAspectRecordKind::Recognized);
    assert_eq!(record.reason, HirAspectReason::Recognized);
    assert_eq!(record.span, HirAspectSpan::NoSpan(HirAspectNoSpanReason::ProgramSummary));
}

// ---------------------------------------------------------------------------
// APA-1 pins: AIR aspects are unregistered pre-delivery.
// Goal: docs/factory/air-proof-aspect. Registration is APA-2; the three proof
// lines are APA-L1/L2/L3. These pins hold only until those units land.
// ---------------------------------------------------------------------------

/// AIR-shaped HIR sample: plain calls whose shapes evoke the unroll route,
/// the `summa` zero-fill mask combine, and the purity gate. The report
/// builder has no AIR recognizer, so none of these may produce records.
fn air_call_expr(id: u32, span: Span, callee: DefId) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression { id: HirId(id + 1), kind: HirExpressionKind::Path(callee), ty: None, span }),
            Vec::new(),
            Vec::new(),
        ),
        ty: None,
        span,
    }
}

fn air_shaped_program() -> HirModule {
    let span = test_span_between(10, 20);
    expr_program(vec![
        air_call_expr(1, span, DefId(300)),
        air_call_expr(10, span, DefId(301)),
        air_call_expr(20, span, DefId(302)),
    ])
}

/// Exhaustive-dispatch pin: this function stops compiling whenever a
/// `HirAspectKind` variant changes, forcing registration units (APA-2 and
/// later lines) to carry the label pins for their kinds instead of silently
/// widening this test.
fn only_registered_aspect_kind_label(kind: HirAspectKind) -> &'static str {
    match kind {
        HirAspectKind::DiscerneOmnia => "discerne_omnia",
        HirAspectKind::UnrollEquivalence => "unroll_equivalence",
        HirAspectKind::MaskNeutrality => "mask_neutrality",
        HirAspectKind::GatePurity => "gate_purity",
    }
}

#[test]
fn air_aspect_kinds_are_absent_from_the_current_kind_set() {
    let report = report_hir_aspects(&air_shaped_program(), &TypeTable::new());

    for record in report.records() {
        // No AIR recognizer exists yet: any record from an AIR-shaped
        // program can only be a pre-existing DiscerneOmnia record.
        assert_eq!(only_registered_aspect_kind_label(record.aspect), "discerne_omnia");
    }
}

// ---------------------------------------------------------------------------
// APA-2 registration: the three AIR kinds are representable, distinguishable,
// and carry deterministic obligation identity. No recognizer yet (APA-L1/2/3).
// ---------------------------------------------------------------------------

#[test]
fn air_aspect_kinds_are_representable_and_distinguishable_in_records() {
    let span = HirAspectSpan::Source(test_span_between(8, 20));
    let kinds = [
        HirAspectKind::DiscerneOmnia,
        HirAspectKind::UnrollEquivalence,
        HirAspectKind::MaskNeutrality,
        HirAspectKind::GatePurity,
    ];

    let records: Vec<_> = kinds
        .iter()
        .map(|&kind| HirAspectRecord::recognized(kind, span, Some(HirId(7))))
        .collect();

    for (record, &kind) in records.iter().zip(kinds.iter()) {
        assert_eq!(record.aspect, kind);
    }
    // Pairwise distinguishable: no two kinds collapse in the record model.
    for i in 0..records.len() {
        for j in (i + 1)..records.len() {
            assert_ne!(records[i], records[j]);
        }
    }
    // Deterministic labels are pairwise distinct.
    let mut labels: Vec<_> = kinds
        .iter()
        .map(|&k| only_registered_aspect_kind_label(k))
        .collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), kinds.len());
}

#[test]
fn air_aspect_kinds_carry_deterministic_distinct_obligation_identity() {
    use crate::aspect::HirAspectObligationId;

    let kinds = [
        HirAspectKind::DiscerneOmnia,
        HirAspectKind::UnrollEquivalence,
        HirAspectKind::MaskNeutrality,
        HirAspectKind::GatePurity,
    ];

    // Same node and ordinal: identity differs only through the kind.
    let ids: Vec<_> = kinds
        .iter()
        .map(|&aspect| HirAspectObligationId { aspect, node_id: Some(HirId(7)), ordinal: 0 })
        .collect();
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(ids[i], ids[j]);
        }
    }

    // Same kind: node and ordinal keep multiple obligations distinguishable.
    assert_ne!(
        HirAspectObligationId { aspect: HirAspectKind::UnrollEquivalence, node_id: Some(HirId(7)), ordinal: 0 },
        HirAspectObligationId { aspect: HirAspectKind::UnrollEquivalence, node_id: Some(HirId(7)), ordinal: 1 }
    );
    assert_ne!(
        HirAspectObligationId { aspect: HirAspectKind::MaskNeutrality, node_id: Some(HirId(7)), ordinal: 0 },
        HirAspectObligationId { aspect: HirAspectKind::MaskNeutrality, node_id: Some(HirId(8)), ordinal: 0 }
    );
}

#[test]
fn air_shaped_program_produces_no_aspect_records_before_registration() {
    let types = TypeTable::new();
    let program = air_shaped_program();

    let first = report_hir_aspects(&program, &types);
    let second = report_hir_aspects(&program, &types);

    assert_eq!(first, second);
    assert!(
        first.records().is_empty(),
        "AIR-shaped HIR must produce no aspect records until APA-L1/L2/L3 land recognizers (APA-2 registers kinds only)"
    );
}

#[test]
#[ignore = "APA-1 future fixture: UnrollEquivalence (evidence class machine-checked-structural) is unregistered until APA-2 lands registration and APA-L1 lands the real obligation after AEU-2; un-ignored only by its owner"]
fn future_fixture_air_unroll_equivalence_record_and_verdict_shape() {
    // Owning line APA-L1 (after AEU-2): the unroll-route fixture must become a
    // Recognized UnrollEquivalence record whose Lean checker verdict is Proven
    // only through real `lake build` + sorry scan + allowlisted transitive
    // axiom audit (machine-checked-structural, never semantic preservation).
    // No float theorem, no sorry, no unallowlisted axiom, no execution claim.
    // Pre-registration absence pin (holds today):
    let report = report_hir_aspects(&air_shaped_program(), &TypeTable::new());
    assert!(report.records().is_empty());
}

#[test]
#[ignore = "APA-1 future fixture: MaskNeutrality (evidence class machine-checked-structural) is unregistered until APA-2 lands registration and APA-L2 lands the real obligation after ARC-3 and ARC-4; un-ignored only by its owner"]
fn future_fixture_air_mask_neutrality_record_and_verdict_shape() {
    // Owning line APA-L2 (after ARC-3/ARC-4): the caller-built-mask zero-fill
    // fixture must become a Recognized MaskNeutrality record with a live-prefix
    // / dead-suffix structural theorem — machine-checked-structural only, with
    // the caller-built-mask boundary retained. No float algebra, no identity
    // inference, no in-region mask admission.
    // Pre-registration absence pin (holds today):
    let report = report_hir_aspects(&air_shaped_program(), &TypeTable::new());
    assert!(report.records().is_empty());
}

#[test]
#[ignore = "APA-1 future fixture: GatePurity (evidence class machine-checked-structural) is unregistered until APA-2 lands registration and APA-L3 lands the real obligation; un-ignored only by its owner"]
fn future_fixture_air_gate_purity_record_and_verdict_shape() {
    // Owning line APA-L3: the scalar/let/call admitted fixture must become a
    // Recognized GatePurity record proving inhabitation in the pure type
    // fragment with no IO/monadic effects; a gate-admitted-but-impure form
    // surfaces as a named gate-hole finding, never a suppressed failure.
    // Machine-checked-structural, not semantic preservation.
    // Pre-registration absence pin (holds today):
    let report = report_hir_aspects(&air_shaped_program(), &TypeTable::new());
    assert!(report.records().is_empty());
}

// ---------------------------------------------------------------------------
// APA-L2: additive-combine reduction recognizer.
// ---------------------------------------------------------------------------

fn additive_closure(id: u32, span: Span) -> HirExpression {
    let left = HirExpression { id: HirId(id + 1), kind: HirExpressionKind::Path(DefId(701)), ty: None, span };
    let right = HirExpression { id: HirId(id + 2), kind: HirExpressionKind::Path(DefId(702)), ty: None, span };
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Clausura(
            Vec::new(),
            None,
            None,
            Box::new(HirExpression {
                id: HirId(id + 3),
                kind: HirExpressionKind::Binary(HirBinOp::Add, Box::new(left), Box::new(right)),
                ty: None,
                span,
            }),
            true,
        ),
        ty: None,
        span,
    }
}

fn reduction_call(
    id: u32,
    span: Span,
    method: crate::lexer::Symbol,
    combine: HirExpression,
    seed: HirExpression,
) -> HirExpression {
    let arg = |expr: HirExpression| HirCallArg { name: None, spread: false, span: expr.span, expr };
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression { id: HirId(id + 1), kind: HirExpressionKind::Path(DefId(700)), ty: None, span }),
            method,
            Vec::new(),
            vec![arg(combine), arg(seed)],
        ),
        ty: None,
        span,
    }
}

#[test]
fn hir_aspect_report_recognizes_additive_reducta_and_cumulata() {
    let span = test_span_between(40, 60);
    let mut interner = Interner::new();
    let reducta = interner.intern("reducta");
    let cumulata = interner.intern("cumulata");
    let program = expr_program(vec![
        reduction_call(
            40,
            span,
            reducta,
            additive_closure(41, span),
            expr(HirExpressionKind::Literal(HirLiteral::Int(0)), 42, span),
        ),
        reduction_call(
            50,
            span,
            cumulata,
            additive_closure(51, span),
            expr(HirExpressionKind::Literal(HirLiteral::Int(0)), 52, span),
        ),
    ]);
    let report = super::report_hir_aspects_with_interner(&program, &TypeTable::new(), &interner);

    assert_eq!(report.records().len(), 2);
    assert!(report
        .records()
        .iter()
        .all(|record| record.record_kind == HirAspectRecordKind::Recognized));
    assert_eq!(
        report
            .records()
            .iter()
            .map(|record| match &record.detail {
                super::HirAspectDetail::MaskNeutrality { family } => *family,
                _ => panic!("missing mask-neutrality detail"),
            })
            .collect::<Vec<_>>(),
        vec![super::MaskNeutralityFamily::Reduce, super::MaskNeutralityFamily::Scan]
    );
}

#[test]
fn hir_aspect_report_does_not_admit_non_neutral_or_non_additive_reductions() {
    let span = test_span_between(70, 90);
    let mut interner = Interner::new();
    let reducta = interner.intern("reducta");
    let non_additive = expr(HirExpressionKind::Literal(HirLiteral::Int(1)), 61, span);
    let non_zero = expr(HirExpressionKind::Literal(HirLiteral::Int(2)), 62, span);
    let program = expr_program(vec![
        reduction_call(60, span, reducta, non_additive, lit_expr(63, span, None)),
        reduction_call(70, span, reducta, additive_closure(71, span), non_zero),
        reduction_call(
            80,
            span,
            reducta,
            additive_closure(81, span),
            expr(HirExpressionKind::Literal(HirLiteral::Float(0.0)), 82, span),
        ),
    ]);
    let report = super::report_hir_aspects_with_interner(&program, &TypeTable::new(), &interner);

    assert_eq!(report.records().len(), 3);
    assert!(report
        .records()
        .iter()
        .all(|record| record.record_kind == HirAspectRecordKind::NotProven));
    assert_eq!(report.records()[0].reason, HirAspectReason::NonAdditiveCombine);
    assert_eq!(report.records()[1].reason, HirAspectReason::NonNeutralSeed);
    assert_eq!(report.records()[2].reason, HirAspectReason::NonNeutralSeed);
}

fn expr(kind: HirExpressionKind, id: u32, span: Span) -> HirExpression {
    HirExpression { id: HirId(id), kind, ty: None, span }
}
