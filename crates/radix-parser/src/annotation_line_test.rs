//! Parser tests for the annotation line law (EBNF [004]+[043]).
//!
//! A bare `annotation_sugar` is NEWLINE-terminated, so the declaration it
//! annotates must start on the next physical line; same-line attachment has
//! no derivation because the payload would swallow the declaration head.
//! Payload sugar keeps its tokens on the annotation's own line ([043]
//! `NON_NEWLINE_TOKEN*`), and braced annotations end at `}` ([040]) — both
//! are exempt from the line law.

use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok, symbol_name};
use radix_syntax::{AnnotationKind, StmtKind};

#[test]
fn rejects_same_line_bare_annotation_before_functio() {
    assert_parse_error_matches(
        "@ futura functio f() → vacuum { }",
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ futura"),
        ]),
    );
}

#[test]
fn rejects_same_line_marker_annotation_before_functio() {
    assert_parse_error_matches(
        "@ publica functio f() → vacuum { }",
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ publica"),
        ]),
    );
}

#[test]
fn rejects_same_line_conversio_marker_before_functio() {
    assert_parse_error_matches(
        "@ conversio functio f(Source err) → Dest { redde finge From(err) }",
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ conversio"),
        ]),
    );
}

#[test]
fn accepts_two_line_annotation_before_declaration() {
    let result = parse_ok("@ futura\nfunctio f() → vacuum { }");
    let program = result.program.as_ref().expect("program");
    assert!(matches!(&program.statements[0].kind, StmtKind::Func(_)));
    assert!(matches!(
        program.statements[0].annotations.as_slice(),
        [annotation] if matches!(
            annotation.kind,
            AnnotationKind::Statement(ref stmt) if stmt.args.is_empty()
        )
    ));
}

#[test]
fn statement_payload_sugar_keeps_same_line_block() {
    let result = parse_ok("@ memento Source err { redde 1 }");
    let program = result.program.as_ref().expect("program");
    assert!(matches!(&program.statements[0].kind, StmtKind::Block(_)));
    match program.statements[0].annotations.as_slice() {
        [annotation] => match &annotation.kind {
            AnnotationKind::Statement(stmt) => {
                assert_eq!(symbol_name(&result, stmt.name.name), "memento");
                assert_eq!(
                    stmt.args.len(),
                    2,
                    "payload tokens belong to the annotation"
                );
            }
            other => panic!("expected statement annotation, got {other:?}"),
        },
        other => panic!("expected one annotation, got {other:?}"),
    }
}

#[test]
fn braced_annotation_is_exempt_from_the_line_law() {
    parse_ok("@ futura { } functio f() → vacuum { }");
}
