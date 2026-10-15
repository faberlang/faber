//! Clean-break contract for the removed `nexum` field modifier (DECISIONS D16.2).
//!
//! `nexum` (bound/reactive field) was removed. Where the word appears in its
//! old field-modifier position (`nexum <type> <name>`) the parser reports the
//! named migration diagnostic `nexum_removed`, not a generic parse error.
//! Elsewhere `nexum` is an ordinary identifier.

use crate::ParseErrorKind;
use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok};
use radix_syntax::{ClassMemberKind, StmtKind};

#[test]
fn nexum_field_reports_nexum_removed() {
    assert_parse_error_matches(
        "genus Animal {\n    nexum numerus aetas = 3\n}",
        ParseExpectation::Args(&[("issue", "nexum_removed")]),
    );
    assert_parse_error_matches(
        "genus Animal {\n    nexum numerus aetas = 3\n}",
        ParseExpectation::Kind(ParseErrorKind::Unexpected),
    );
}

#[test]
fn nexum_after_generis_reports_nexum_removed() {
    assert_parse_error_matches(
        "genus Animal {\n    generis nexum textus nomen = \"leo\"\n}",
        ParseExpectation::Args(&[("issue", "nexum_removed")]),
    );
}

#[test]
fn nexum_generic_field_reports_nexum_removed() {
    assert_parse_error_matches(
        "genus Grex {\n    nexum lista<numerus> aetates\n}",
        ParseExpectation::Args(&[("issue", "nexum_removed")]),
    );
}

#[test]
fn nexum_is_an_ordinary_identifier_elsewhere() {
    // A field of a user type spelled `nexum`, then a field on the next line.
    let result = parse_ok("genus Animal {\n    varia nexum vinculum\n    varia numerus aetas\n}\n");
    let program = result.program.expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected class declaration");
    };
    assert_eq!(class.members.len(), 2);
    assert!(matches!(class.members[0].kind, ClassMemberKind::Field(_)));
    let bindings = parse_ok("sit nexum ← 5\nsit alius ← nexum\n");
    assert_eq!(bindings.program.expect("program").statements.len(), 2);
}
