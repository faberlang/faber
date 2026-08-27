//! Parser tests for structured `@ nucleum` / `@ nucleum fragment`.
//!
//! `@ nucleum fragment` is typed modifier data on the nucleum annotation, not
//! an untyped name/argument. Standalone `@ fragment` stays the graphics
//! annotation.

use crate::test_support::{assert_parse_error_matches, parse_ok, symbol_name, ParseExpectation};
use radix_syntax::AnnotationKind;

fn first_annotation_kind(source: &str) -> AnnotationKindSnapshot {
    let result = parse_ok(source);
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1, "expected one statement");
    assert_eq!(
        program.statements[0].annotations.len(),
        1,
        "expected one annotation"
    );
    match &program.statements[0].annotations[0].kind {
        AnnotationKind::Nucleum(nucleum) => AnnotationKindSnapshot::Nucleum {
            fragment: nucleum.is_fragment,
        },
        AnnotationKind::Statement(stmt) => AnnotationKindSnapshot::Statement {
            name: symbol_name(&result, stmt.name.name).to_owned(),
            arg_count: stmt.args.len(),
        },
        AnnotationKind::Braced(braced) => AnnotationKindSnapshot::Braced {
            name: symbol_name(&result, braced.name.name).to_owned(),
            field_count: braced.fields.len(),
        },
        other => panic!("unexpected annotation kind: {other:?}"),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AnnotationKindSnapshot {
    Nucleum { fragment: bool },
    Statement { name: String, arg_count: usize },
    Braced { name: String, field_count: usize },
}

#[test]
fn parses_bare_nucleum_sugar_as_structured_entry() {
    assert_eq!(
        first_annotation_kind("@ nucleum\nfunctio k() → vacuum { }\n"),
        AnnotationKindSnapshot::Nucleum { fragment: false }
    );
}

#[test]
fn parses_nucleum_fragment_sugar_as_structured_modifier() {
    assert_eq!(
        first_annotation_kind("@ nucleum fragment\nfunctio h() → vacuum { }\n"),
        AnnotationKindSnapshot::Nucleum { fragment: true }
    );
}

#[test]
fn nucleum_fragment_sugar_is_not_untyped_argument() {
    let result = parse_ok("@ nucleum fragment\nfunctio h() → vacuum { }\n");
    let program = result.program.as_ref().expect("program");
    match &program.statements[0].annotations[0].kind {
        AnnotationKind::Nucleum(nucleum) => assert!(nucleum.is_fragment),
        AnnotationKind::Statement(stmt) => panic!(
            "@ nucleum fragment must not be an untyped statement annotation (name={}, args={})",
            symbol_name(&result, stmt.name.name),
            stmt.args.len()
        ),
        other => panic!("expected structured nucleum, got {other:?}"),
    }
}

#[test]
fn rejects_same_line_nucleum_fragment_sugar_before_functio() {
    assert_parse_error_matches(
        "@ nucleum fragment functio h() → vacuum { }\n",
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ nucleum"),
        ]),
    );
}

#[test]
fn parses_bare_fragment_as_graphics_statement() {
    assert_eq!(
        first_annotation_kind("@ fragment\nfunctio shade() → vacuum { }\n"),
        AnnotationKindSnapshot::Statement {
            name: "fragment".to_owned(),
            arg_count: 0,
        }
    );
}

#[test]
fn parses_bare_fragment_braced_as_graphics_annotation() {
    assert_eq!(
        first_annotation_kind("@ fragment { }\nfunctio shade() → vacuum { }\n"),
        AnnotationKindSnapshot::Braced {
            name: "fragment".to_owned(),
            field_count: 0,
        }
    );
}

#[test]
fn parses_bare_nucleum_braced_as_structured_entry() {
    assert_eq!(
        first_annotation_kind("@ nucleum { }\nfunctio k() → vacuum { }\n"),
        AnnotationKindSnapshot::Nucleum { fragment: false }
    );
}

#[test]
fn parses_nucleum_braced_fragment_verum_as_structured_modifier() {
    assert_eq!(
        first_annotation_kind("@ nucleum { fragment = verum }\nfunctio h() → vacuum { }\n"),
        AnnotationKindSnapshot::Nucleum { fragment: true }
    );
}

#[test]
fn parses_nucleum_braced_fragment_falsum_as_structured_entry() {
    assert_eq!(
        first_annotation_kind("@ nucleum { fragment = falsum }\nfunctio k() → vacuum { }\n"),
        AnnotationKindSnapshot::Nucleum { fragment: false }
    );
}

#[test]
fn parses_named_lane_sugar_as_string_binding() {
    let result = parse_ok("@ nucleum lane \"position\"\nfunctio k() → vacuum { }\n");
    let AnnotationKind::Nucleum(nucleum) =
        &result.program.as_ref().expect("program").statements[0].annotations[0].kind
    else {
        panic!("expected structured nucleum");
    };
    assert_eq!(
        result.interner.resolve(nucleum.lane.expect("lane")),
        "position"
    );
    assert_eq!(nucleum.lane_values.len(), 1);
}

#[test]
fn parses_named_lane_beside_fragment_in_braced_form() {
    let result =
        parse_ok("@ nucleum { lane = \"position\", fragment = verum }\nfunctio k() → vacuum { }\n");
    let AnnotationKind::Nucleum(nucleum) =
        &result.program.as_ref().expect("program").statements[0].annotations[0].kind
    else {
        panic!("expected structured nucleum");
    };
    assert!(nucleum.is_fragment);
    assert_eq!(
        result.interner.resolve(nucleum.lane.expect("lane")),
        "position"
    );
}

#[test]
fn rejects_non_string_named_lane_value() {
    assert_parse_error_matches(
        "@ nucleum lane 1\nfunctio k() → vacuum { }\n",
        ParseExpectation::Issue("non_string_nucleum_lane_value"),
    );
}

#[test]
fn rejects_nucleum_fragment_combined_with_graphics_fragment() {
    assert_parse_error_matches(
        "@ nucleum fragment\n@ fragment\nfunctio h() → vacuum { }\n",
        ParseExpectation::Issue("nucleum_fragment_conflicts_with_fragment"),
    );
}

#[test]
fn rejects_braced_nucleum_fragment_combined_with_graphics_fragment() {
    assert_parse_error_matches(
        "@ nucleum { fragment = verum }\n@ fragment\nfunctio h() → vacuum { }\n",
        ParseExpectation::Issue("nucleum_fragment_conflicts_with_fragment"),
    );
}

#[test]
fn rejects_doubled_nucleum_sugar() {
    assert_parse_error_matches(
        "@ nucleum\n@ nucleum\nfunctio k() → vacuum { }\n",
        ParseExpectation::Issue("duplicate_nucleum"),
    );
}

#[test]
fn rejects_nucleum_plus_nucleum_fragment() {
    assert_parse_error_matches(
        "@ nucleum\n@ nucleum fragment\nfunctio k() → vacuum { }\n",
        ParseExpectation::Issue("duplicate_nucleum"),
    );
}

#[test]
fn rejects_doubled_nucleum_braced() {
    assert_parse_error_matches(
        "@ nucleum { }\n@ nucleum { fragment = verum }\nfunctio k() → vacuum { }\n",
        ParseExpectation::Issue("duplicate_nucleum"),
    );
}

#[test]
fn rejects_unexpected_token_after_nucleum_sugar() {
    assert_parse_error_matches(
        "@ nucleum ignotum\nfunctio k() → vacuum { }\n",
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ nucleum"),
        ]),
    );
}

#[test]
fn rejects_unexpected_token_after_nucleum_fragment_sugar() {
    assert_parse_error_matches(
        "@ nucleum fragment ignotum\nfunctio h() → vacuum { }\n",
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ nucleum"),
        ]),
    );
}
