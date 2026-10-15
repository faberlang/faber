//! Build-time file embed `insere "path"` (DECISIONS D8.10, campaign F8; st1 U4).
//!
//! Contract: `insere` (en `embed`) is a contextual keyword. It is claimed in
//! expression-primary position directly followed by a string literal, where it
//! parses to `ExprKind::Insere` carrying the authored path. Whether an embed
//! is legal at that position is lowering's decision, not the parser's.
//! Without a following string the spelling stays an ordinary identifier.

use crate::test_support::{parse_ok_la, parse_program_la, symbol_name};
use radix_syntax::{ClassMemberKind, ExprKind, StmtKind};

fn first_class_field_init_kind(source: &str) -> String {
    let result = parse_ok_la(source);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected genus");
    };
    let ClassMemberKind::Field(field) = &class.members[0].kind else {
        panic!("expected field");
    };
    format!("{:?}", field.init.as_deref().expect("initializer").kind)
}

#[test]
fn module_constant_initializer_claims_insere() {
    let result = parse_ok_la("fixum textus SCHEMA = insere \"schema.sql\"");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected constant var decl");
    };
    let init = decl.init.as_ref().expect("initializer");
    let ExprKind::Insere(insere) = &init.kind else {
        panic!("expected insere initializer, got {:?}", init.kind);
    };
    assert_eq!(symbol_name(&result, insere.path), "schema.sql");
}

#[test]
fn genus_static_field_initializer_claims_insere() {
    let init = first_class_field_init_kind(
        "genus Icon {\n    generis octeti LOGO = insere \"logo.png\"\n}\n",
    );
    assert!(init.starts_with("Insere("), "got {init}");
}

#[test]
fn every_field_modifier_default_claims_insere() {
    for modifier in ["fixum", "varia", "generis"] {
        let source = format!("genus G {{\n    {modifier} textus s = insere \"x.txt\"\n}}\n");
        let init = first_class_field_init_kind(&source);
        assert!(
            init.starts_with("Insere("),
            "modifier `{modifier}`: got {init}"
        );
    }
}

#[test]
fn block_level_constant_claims_insere() {
    let result = parse_ok_la("incipit {\n    fixum textus x = insere \"a.txt\"\n}\n");
    assert!(format!("{:?}", result.program).contains("Insere("));
}

#[test]
fn insere_is_claimed_in_any_expression_position() {
    // Lowering rejects an embed outside a constant initializer; the parser
    // only recognizes the shape (keyword directly followed by a string).
    let result =
        parse_ok_la("incipit {\n    varia textus x ← \"a\"\n    x ← insere \"a.txt\"\n}\n");
    assert!(format!("{:?}", result.program).contains("Insere("));
}

#[test]
fn insere_stays_an_identifier_without_a_following_string() {
    let result = parse_ok_la(
        "fixum numerus insere = 3\nfixum numerus ALIUS = insere\nfunctio f() → numerus {\n    sit numerus insere ← 4\n    redde insere\n}\n",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[1].kind else {
        panic!("expected constant var decl");
    };
    let init = decl.init.as_ref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::Ident(_)),
        "got {:?}",
        init.kind
    );
}

#[test]
fn insere_call_stays_a_call() {
    let result = parse_ok_la("fixum numerus N = insere(3)");
    assert!(!format!("{:?}", result.program).contains("Insere("));
}

#[test]
fn insere_without_a_string_path_is_not_an_embed_anywhere() {
    // A field default `insere x` is no embed; it fails as ordinary syntax.
    let result = parse_program_la("genus G {\n    fixum textus s = insere x\n}\n");
    assert!(!result.success());
}
