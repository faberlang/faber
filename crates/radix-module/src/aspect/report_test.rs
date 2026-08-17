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
