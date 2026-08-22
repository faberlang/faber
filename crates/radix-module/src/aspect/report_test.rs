//! Tests for HIR aspect report generation (discerne omnia recognition).
//! Proves: empty-program determinism; span ordering; complete/missing/
//! catchall/catchall-as-variant detection; non-enum and multi-scrutinee
//! skipping.

use super::{
    report_hir_aspects, HirAspectKind, HirAspectNoSpanReason, HirAspectPrecondition, HirAspectReason, HirAspectRecord,
    HirAspectRecordKind, HirAspectSpan,
};
use crate::hir::{
    DefId, HirBlock, HirCasuArm, HirEnum, HirExpression, HirExpressionKind, HirId, HirItem, HirItemKind, HirLiteral,
    HirModule, HirPattern, HirStatement, HirStatementKind, HirVariant,
};
use crate::lexer::Span;
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
            Box::new(HirExpression {
                id: HirId(id + 1),
                kind: HirExpressionKind::Path(callee),
                ty: None,
                span,
            }),
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

/// Exhaustive-dispatch pin: this function stops compiling the moment a second
/// `HirAspectKind` variant is added, forcing APA-2 to carry the registration
/// pins for the new kinds instead of silently widening this test.
fn only_registered_aspect_kind_label(kind: HirAspectKind) -> &'static str {
    match kind {
        HirAspectKind::DiscerneOmnia => "discerne_omnia",
    }
}

#[test]
fn air_aspect_kinds_are_absent_from_the_current_kind_set() {
    let report = report_hir_aspects(&air_shaped_program(), &TypeTable::new());

    for record in report.records() {
        // Exhaustive match: pre-APA-2, every record is DiscerneOmnia.
        assert_eq!(only_registered_aspect_kind_label(record.aspect), "discerne_omnia");
    }
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
        "AIR-shaped HIR must produce no aspect records before APA-2 registers the AIR kinds"
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
