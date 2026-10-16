//! Copy-with-changes construction source (DECISIONS D15.1–D15.2).
//!
//! `Genus { field = value } ex source` parses as one construction literal whose
//! last object field is the source (`ObjectKey::Spread`). Exactly one source is
//! legal; a second `ex` on the same line is `construction_source_repeated`. An
//! `ex` that starts the next line stays an extraction statement.

use radix_syntax::{ExprKind, ObjectKey, StmtKind};

use crate::ParseErrorKind;
use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok};

fn construction_fields(source: &str) -> Vec<&'static str> {
    let result = parse_ok(source);
    let program = result.program.expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected a binding statement");
    };
    let init = var.init.as_ref().expect("initializer");
    let ExprKind::Verte(verte) = &init.kind else {
        panic!("expected a construction literal, got {:?}", init.kind);
    };
    let ExprKind::Object(object) = &verte.expr.kind else {
        panic!("expected construction fields");
    };
    object
        .fields
        .iter()
        .map(|field| match &field.key {
            ObjectKey::Ident(_) => "field",
            ObjectKey::Spread(_) => "source",
            ObjectKey::String(_) | ObjectKey::Computed(_) => "other",
        })
        .collect()
}

#[test]
fn construction_source_is_the_last_field() {
    assert_eq!(
        construction_fields("sit _ q ← Punctum { x = 1 } ex p"),
        ["field", "source"]
    );
}

#[test]
fn construction_source_accepts_postfix_expressions() {
    assert_eq!(
        construction_fields("sit _ q ← Punctum { x = 1, y = 2 } ex fons.punctum()"),
        ["field", "field", "source"]
    );
}

#[test]
fn qualified_construction_takes_a_source() {
    assert_eq!(
        construction_fields("sit _ q ← geo.Punctum { x = 1 } ex p"),
        ["field", "source"]
    );
}

#[test]
fn construction_without_source_is_unchanged() {
    assert_eq!(
        construction_fields("sit _ q ← Punctum { x = 1 }"),
        ["field"]
    );
}

#[test]
fn second_construction_source_is_rejected() {
    assert_parse_error_matches(
        "sit _ q ← Punctum { x = 1 } ex p ex r",
        ParseExpectation::Args(&[("issue", "construction_source_repeated")]),
    );
    assert_parse_error_matches(
        "sit _ q ← Punctum { x = 1 } ex p ex r",
        ParseExpectation::Kind(ParseErrorKind::Unexpected),
    );
}

#[test]
fn next_line_ex_stays_an_extraction_statement() {
    let result = parse_ok("sit _ q ← Punctum { x = 1 }\nex q sit x\n");
    let program = result.program.expect("program");
    assert_eq!(program.statements.len(), 2);
    assert!(matches!(program.statements[1].kind, StmtKind::Ex(_)));
}
