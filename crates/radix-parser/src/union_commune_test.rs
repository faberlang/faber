//! TEU1 parser proofs for annotated shared fields in tagged unions.

use crate::{ParseErrorKind, parse};
use radix_lexer::lex;
use radix_syntax::{AnnotationKind, StmtKind};

#[test]
fn parses_commune_region_before_union_variants() {
    let source = "discretio ShapeError {\n    @ commune\n    textus message\n\n    NegativeDimension,\n    Incompatible,\n    ElementMismatch { numerus expected numerus actual }\n}\n";
    let result = parse(lex(source));
    assert!(
        result.success(),
        "expected parse success: {:?}",
        result.errors
    );

    let program = result.program.as_ref().expect("program");
    let StmtKind::Union(union) = &program.statements[0].kind else {
        panic!("expected union declaration");
    };
    assert_eq!(union.shared_fields.len(), 1);
    assert_eq!(union.variants.len(), 3);
    assert_eq!(union.variants[2].fields.len(), 2);
    assert!(matches!(
        union.shared_fields[0].annotations[0].kind,
        AnnotationKind::Statement(ref annotation)
            if result.interner.resolve(annotation.name.name) == "commune"
                && annotation.args.is_empty()
    ));
}

#[test]
fn rejects_variant_redeclaration_of_commune_field() {
    let source = "discretio ShapeError {\n    @ commune\n    textus message\n\n    Incompatible { textus message }\n}\n";
    let result = parse(lex(source));
    assert!(!result.success(), "variant redeclaration must fail");
    assert!(
        result.errors.iter().any(|error| {
            error.kind == ParseErrorKind::Expected
                && error.args.iter().any(|arg| {
                    arg.name == "issue" && arg.value == "union_variant_redeclares_shared_field"
                })
        }),
        "expected shared-field redeclaration issue: {:?}",
        result.errors
    );
}

#[test]
fn parses_shared_alias_as_commune_annotation_marker() {
    let result = parse(lex(
        "discretio ShapeError {\n    @ shared\n    textus message\n    Incompatible\n}\n",
    ));
    assert!(
        result.success(),
        "expected parse success: {:?}",
        result.errors
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Union(union) = &program.statements[0].kind else {
        panic!("expected union declaration");
    };
    let AnnotationKind::Statement(annotation) = &union.shared_fields[0].annotations[0].kind else {
        panic!("expected statement annotation");
    };
    assert_eq!(result.interner.resolve(annotation.name.name), "commune");
    assert!(annotation.args.is_empty());
}
