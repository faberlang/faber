//! Genus field modifiers (DECISIONS D16.1, D16.2).
//!
//! A `genus` field may lead with exactly one of `fixum`, `varia` or
//! `generis`. The modifier is recorded on the field. `generis fixum` and
//! `generis varia` (and any other pairing) are rejected. A missing modifier
//! is a named error (D5c tightening); `discretio` shared fields keep today's
//! form (fork F7 held).

use crate::ParseErrorKind;
use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok};
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
        "genus Punctum {\n    sit numerus x\n    varia numerus y\n    generis numerus Z = 3\n}\n",
    );
    assert_eq!(
        modifiers,
        vec![
            Some(FieldModifier::Fixum),
            Some(FieldModifier::Varia),
            Some(FieldModifier::Generis),
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
fn generis_with_fixum_or_varia_is_rejected() {
    for source in [
        "genus Punctum {\n    generis fixum numerus Z = 3\n}\n",
        "genus Punctum {\n    generis varia numerus Z = 3\n}\n",
        "genus Punctum {\n    sit varia numerus x\n}\n",
        "genus Punctum {\n    varia generis numerus x\n}\n",
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
