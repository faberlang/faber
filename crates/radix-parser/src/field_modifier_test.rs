//! Genus field modifiers (DECISIONS D16.1, D16.2).
//!
//! A `genus` field may lead with exactly one of `sit` (read-only per
//! instance), `varia` (writable per instance) or `fixum` (en `const`, the
//! type-level compile-time constant). The modifier is recorded on the field.
//! `fixum sit` and `fixum varia` (and any other pairing) are rejected. A
//! missing modifier is a named error (D5c tightening); `discretio` shared
//! fields keep today's form (fork F7 held). The retired `generis` (en
//! `static`) word is `static_member_retired`; a constant without `=` or with
//! the run-time arrow is named too.

use crate::ParseErrorKind;
use crate::test_support::{
    ParseExpectation, assert_parse_error_matches, has_arg, parse_ok, parse_program,
};
use radix_syntax::{ClassMemberKind, FieldModifier, StmtKind};

fn field_modifiers(source: &str) -> Vec<Option<FieldModifier>> {
    let result = parse_ok(source);
    let program = result.program.expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected class declaration");
    };
    class
        .members
        .iter()
        .filter_map(|member| match &member.kind {
            ClassMemberKind::Field(field) => Some(field.modifier),
            ClassMemberKind::Method(_) => None,
        })
        .collect()
}

#[test]
fn field_records_each_modifier() {
    let modifiers = field_modifiers(
        "genus Punctum {\n    sit numerus x\n    varia numerus y\n    fixum numerus Z = 3\n}\n",
    );
    assert_eq!(
        modifiers,
        vec![
            Some(FieldModifier::Fixum),
            Some(FieldModifier::Varia),
            Some(FieldModifier::Const),
        ]
    );
}

#[test]
fn modifier_keeps_sponte_and_default() {
    let modifiers = field_modifiers(
        "genus Punctum {\n    varia numerus x sponte\n    sit textus nomen = \"a\"\n}\n",
    );
    assert_eq!(
        modifiers,
        vec![Some(FieldModifier::Varia), Some(FieldModifier::Fixum)]
    );
}

#[test]
fn fixum_with_another_modifier_is_rejected() {
    for source in [
        "genus Punctum {\n    fixum sit numerus Z = 3\n}\n",
        "genus Punctum {\n    fixum varia numerus Z = 3\n}\n",
        "genus Punctum {\n    sit varia numerus x\n}\n",
        "genus Punctum {\n    varia fixum numerus x = 1\n}\n",
    ] {
        assert_parse_error_matches(
            source,
            ParseExpectation::Args(&[("issue", "field_modifier_repeated")]),
        );
        assert_parse_error_matches(
            source,
            ParseExpectation::Kind(ParseErrorKind::DeclarationInvalid),
        );
    }
}

#[test]
fn unmarked_genus_field_is_rejected() {
    for source in [
        "genus Punctum {\n    numerus x\n}\n",
        "genus Punctum {\n    varia numerus x\n    numerus y = 0\n}\n",
        "genus Punctum {\n    @ privata\n    textus nomen sponte\n}\n",
    ] {
        assert_parse_error_matches(
            source,
            ParseExpectation::Args(&[("issue", "field_modifier_missing")]),
        );
        assert_parse_error_matches(
            source,
            ParseExpectation::Kind(ParseErrorKind::DeclarationInvalid),
        );
    }
}

#[test]
fn discretio_shared_field_stays_unmarked() {
    // Fork F7 held: union shared fields keep today's form.
    let result = parse_ok(
        "discretio Forma {\n    @ publica\n    textus nomen\n    Circulus { numerus r }\n}\n",
    );
    assert!(result.program.is_some());
}

fn issue_count(source: &str, issue: &str) -> usize {
    parse_program(source)
        .errors
        .iter()
        .filter(|err| has_arg(err, "issue", issue))
        .count()
}

#[test]
fn const_member_without_initializer_is_named() {
    let source = "genus Punctum {\n    fixum numerus Z\n    varia numerus y\n}\n";
    assert_eq!(issue_count(source, "const_member_requires_initializer"), 1);
    assert_eq!(parse_program(source).errors.len(), 1);
}

#[test]
fn const_member_with_the_run_time_arrow_is_named() {
    let source = "genus Punctum {\n    fixum numerus Z ← 3\n    varia numerus y\n}\n";
    assert_eq!(issue_count(source, "const_member_assign_initializer"), 1);
    assert_eq!(issue_count(source, "const_member_requires_initializer"), 0);
    assert_eq!(parse_program(source).errors.len(), 1);
}

#[test]
fn retired_static_member_is_named_and_read_as_the_constant() {
    let source = "genus Punctum {\n    generis numerus Z = 3\n    varia numerus y\n}\n";
    let result = parse_program(source);
    assert_eq!(issue_count(source, "static_member_retired"), 1);
    assert!(
        result
            .errors
            .iter()
            .any(|err| has_arg(err, "issue", "static_member_retired")
                && has_arg(err, "keyword", "generis")),
        "{:?}",
        result.errors
    );
    // Read as the constant: no further complaint about the member itself.
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
    let program = result.program.expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected class declaration");
    };
    let ClassMemberKind::Field(field) = &class.members[0].kind else {
        panic!("expected a field");
    };
    assert_eq!(field.modifier, Some(FieldModifier::Const));
}

#[test]
fn const_member_with_a_default_expression_is_accepted() {
    let _ = parse_ok("genus Punctum {\n    fixum numerus Z = 3 * 4\n    varia numerus y\n}\n");
}
