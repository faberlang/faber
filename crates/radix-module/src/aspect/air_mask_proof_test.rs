//! Focused proof-source and recognizer tests for APA-L2.

use super::{
    MASK_NEUTRALITY_PRELUDE, MaskNeutralityFamily, emit_mask_neutrality_theorem, is_additive_combine,
    is_integer_zero_seed, reduction_family,
};
use crate::hir::{HirExpression, HirExpressionKind, HirId, HirLiteral};
use crate::lexer::Span;

fn expr(kind: HirExpressionKind) -> HirExpression {
    HirExpression { id: HirId(1), kind, ty: None, span: Span::new(0, 1) }
}

#[test]
fn reduction_family_is_limited_to_reduce_and_scan_consumers() {
    assert_eq!(reduction_family("reducta"), Some(MaskNeutralityFamily::Reduce));
    assert_eq!(reduction_family("cumulata"), Some(MaskNeutralityFamily::Scan));
    assert_eq!(reduction_family("summa"), None);
    assert_eq!(reduction_family("media"), None);
}

#[test]
fn recognizer_rejects_uninspected_paths_and_non_combine_shapes() {
    assert!(!is_additive_combine(&expr(HirExpressionKind::Path(crate::hir::DefId(7)))));
    assert!(!is_additive_combine(&expr(HirExpressionKind::Literal(HirLiteral::Int(1)))));
}

#[test]
fn recognizer_requires_explicit_integer_zero_seed() {
    assert!(is_integer_zero_seed(&expr(HirExpressionKind::Literal(HirLiteral::Int(0)))));
    assert!(!is_integer_zero_seed(&expr(HirExpressionKind::Literal(HirLiteral::Int(1)))));
    assert!(!is_integer_zero_seed(&expr(HirExpressionKind::Literal(HirLiteral::Float(0.0)))));
}

#[test]
fn generated_mask_proof_is_structural_and_float_free() {
    let source = format!(
        "{}\n{}",
        MASK_NEUTRALITY_PRELUDE,
        emit_mask_neutrality_theorem("mask_neutrality_reduce_7", MaskNeutralityFamily::Reduce)
    );

    assert!(source.contains("foldNeutralSuffix"));
    assert!(source.contains("additiveReduceMaskNeutrality"));
    assert!(source.contains("live : List Nat"));
    assert!(source.contains("dead : Nat"));
    assert!(!source.contains("Float"));
    assert!(!source.contains("f32"));
    assert!(!source.contains("f64"));
    assert!(!source.contains("sorry"));
}

#[test]
fn generated_scan_obligation_reuses_the_additive_lemma_family() {
    let source = emit_mask_neutrality_theorem("mask_neutrality_scan_8", MaskNeutralityFamily::Scan);
    assert!(source.contains("additiveScanMaskNeutrality"));
    assert!(source.contains("zeroFilledSuffix"));
    assert!(source.contains("dead : Nat"));
}
