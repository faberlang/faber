//! Tests for TLA+ module emission from HIR aspect records.
//! Proves: recognized discerne-omnia records render valid TLA+; not-proven
//! records are skipped.

use super::emit_tla_modules;
use crate::aspect::{HirAspectRecordKind, report_hir_aspects};
use crate::hir::{
    DefId, HirBlock, HirCasuArm, HirEnum, HirExpression, HirExpressionKind, HirId, HirItem, HirItemKind, HirLiteral,
    HirModule, HirPattern, HirStatement, HirStatementKind, HirVariant,
};
use crate::lexer::Span;
use crate::semantic::{Type, TypeId, TypeTable};
use crate::test_support::test_span_between;

fn lit_expr(id: u32, span: Span) -> HirExpression {
    HirExpression { id: HirId(id), kind: HirExpressionKind::Literal(HirLiteral::Int(1)), ty: None, span }
}

fn variant_pattern(def_id: u32) -> HirPattern {
    HirPattern::Variant(DefId(def_id), Vec::new())
}

fn discerne_omnia_expr(id: u32, span: Span, scrutinee_ty: TypeId, patterns: Vec<HirPattern>) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Discerne {
            scrutinees: vec![HirExpression {
                id: HirId(id + 1),
                kind: HirExpressionKind::Path(DefId(100)),
                ty: Some(scrutinee_ty),
                span,
            }],
            arms: vec![HirCasuArm { patterns, body: lit_expr(id + 20, span), span }],
            exhaustive: true,
        },
        ty: None,
        span,
    }
}

fn enum_program(expr: HirExpression) -> HirModule {
    HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(200),
            def_id: DefId(1),
            kind: HirItemKind::Enum(HirEnum {
                name: crate::lexer::Symbol(1),
                type_params: Vec::new(),
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
        }],
        entry: Some(HirBlock {
            statements: vec![HirStatement { id: HirId(100), span: expr.span, kind: HirStatementKind::Expr(expr) }],
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
fn hir_aspect_tla_emit_renders_recognized_discerne_omnia_module() {
    let mut types = TypeTable::new();
    let enum_ty = types.intern(Type::Enum(DefId(1)));
    let program = enum_program(discerne_omnia_expr(
        7,
        test_span_between(8, 20),
        enum_ty,
        vec![variant_pattern(2), variant_pattern(3)],
    ));
    let report = report_hir_aspects(&program, &types);

    let modules = emit_tla_modules(&report);

    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "FaberAspectDiscerneOmnia_Hir7");
    assert_eq!(
        modules[0].text,
        "---- MODULE FaberAspectDiscerneOmnia_Hir7 ----\n\
EXTENDS TLC\n\
\n\
\\* aspect: discerne omnia\n\
\\* hir_id: 7\n\
\\* source_span: 8..20\n\
\n\
Variant_2 == \"Variant_2\"\n\
Variant_3 == \"Variant_3\"\n\
\n\
Variants == {Variant_2, Variant_3}\n\
Covered == {Variant_2, Variant_3}\n\
\n\
VARIABLE checker_state\n\
Init == checker_state = \"init\"\n\
Next == UNCHANGED checker_state\n\
Spec == Init /\\ [][Next]_checker_state\n\
\n\
Totality == Variants \\subseteq Covered\n\
=============================================================================\n"
    );
}

#[test]
fn hir_aspect_tla_emit_skips_not_proven_records() {
    let mut types = TypeTable::new();
    let enum_ty = types.intern(Type::Enum(DefId(1)));
    let program = enum_program(discerne_omnia_expr(
        7,
        test_span_between(8, 20),
        enum_ty,
        vec![variant_pattern(2)],
    ));
    let report = report_hir_aspects(&program, &types);

    assert_eq!(report.records()[0].record_kind, HirAspectRecordKind::NotProven);
    assert!(emit_tla_modules(&report).is_empty());
}
