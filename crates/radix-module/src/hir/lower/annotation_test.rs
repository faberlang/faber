use super::annotation::try_parse_nondum_annotation;
use crate::hir::{HirAnnotation, HirAnnotationField, HirAnnotationValue, HirAvailability};
use crate::lexer::{Interner, Span};

#[test]
fn test_try_parse_nondum_annotation_positive() {
    let mut interner = Interner::new();
    let family = interner.intern("nondum");
    let name_target = interner.intern("target");
    let name_ratio = interner.intern("ratio");
    let target_sym = interner.intern("rs");
    let reason_str = interner.intern("not yet implemented");

    let annotation = HirAnnotation::with_fields(
        family,
        vec![
            HirAnnotationField {
                name: name_target,
                value: HirAnnotationValue::Symbol(target_sym),
                span: Span { start: 0, end: 0 },
            },
            HirAnnotationField {
                name: name_ratio,
                value: HirAnnotationValue::String(reason_str),
                span: Span { start: 0, end: 0 },
            },
        ],
        Span { start: 0, end: 0 },
    );

    let result = try_parse_nondum_annotation(&annotation, &interner);
    assert!(result.is_some());
    match result {
        Some(HirAvailability::Nondum { target, reason }) => {
            assert_eq!(target, Some(target_sym));
            assert_eq!(reason, Some(reason_str));
        }
        _ => panic!("expected Nondum variant"),
    }
}

#[test]
fn test_try_parse_nondum_annotation_wrong_family_returns_none() {
    let mut interner = Interner::new();
    let family = interner.intern("optio");

    let annotation = HirAnnotation::with_fields(family, vec![], Span { start: 0, end: 0 });

    let result = try_parse_nondum_annotation(&annotation, &interner);
    assert!(result.is_none());
}
