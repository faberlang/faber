//! Build-time file embed `insere "path"` (DECISIONS D8.10, campaign F8).
//!
//! Contract: `insere` (en `embed`) is a contextual keyword. It is claimed
//! only as the head of a `generis` initializer directly followed by a string
//! literal, where it parses to `ExprKind::Insere` carrying the authored path.
//! Everywhere else the spelling stays an ordinary identifier.

use crate::test_support::{parse_ok, symbol_name};
use radix_syntax::{ClassMemberKind, ExprKind, StmtKind};

#[test]
fn top_level_static_initializer_claims_insere() {
    let result = parse_ok("fixum textus SCHEMA = insere \"schema.sql\"");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected static var decl");
    };
    let init = decl.init.as_ref().expect("initializer");
    let ExprKind::Insere(insere) = &init.kind else {
        panic!("expected insere initializer, got {:?}", init.kind);
    };
    assert_eq!(symbol_name(&result, insere.path), "schema.sql");
}

#[test]
fn genus_static_field_initializer_claims_insere() {
    let result = parse_ok("genus Icon {\n    generis octeti LOGO = insere \"logo.png\"\n}\n");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected genus");
    };
    let ClassMemberKind::Field(field) = &class.members[0].kind else {
        panic!("expected field");
    };
    let init = field.init.as_ref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::Insere(_)),
        "got {:?}",
        init.kind
    );
}

#[test]
fn insere_stays_an_identifier_outside_the_static_initializer() {
    let result = parse_ok(
        "fixum numerus insere = 3\nfixum numerus ALIUS = insere\nfunctio f() → numerus {\n    fixum numerus insere ← 4\n    redde insere\n}\n",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[1].kind else {
        panic!("expected static var decl");
    };
    let init = decl.init.as_ref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::Ident(_)),
        "got {:?}",
        init.kind
    );
}

#[test]
fn instance_field_default_does_not_claim_insere() {
    // Only a `generis` initializer is a compile-time fact; an instance field
    // default keeps `insere` as an identifier (so `insere "x"` is not an
    // embed there and fails as ordinary syntax).
    let result =
        crate::test_support::parse_program("genus G {\n    fixum textus s = insere \"x.txt\"\n}\n");
    assert!(!result.success(), "instance field default must not embed");
}
