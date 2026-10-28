//! `fac omnia` (en `do all`) marker on the `fac` construct.
//!
//! The marker is surface-only: host semantics stay those of plain `fac`. It
//! parses between `fac` and the body block and combines with `cape` and with
//! the trailing `dum` condition (`fac omnia { … } cape err { … } dum cond`).

use crate::parse;
use crate::test_support::{assert_parse_error_issue_la, parse_ok};
use radix_lexer::KeywordPack;
use radix_syntax::{FacModifier, StmtKind};

#[test]
fn parses_fac_omnia_with_cape() {
    let result = parse_ok("fac omnia { tacet } cape err { tacet }");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Fac(fac_stmt) = &program.statements[0].kind else {
        panic!("expected fac statement");
    };
    assert_eq!(fac_stmt.modifier, FacModifier::Omnia);
    assert!(fac_stmt.catch.is_some());
    assert!(fac_stmt.while_.is_none());
}

#[test]
fn parses_plain_fac_without_omnia() {
    let result = parse_ok("fac { tacet }");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Fac(fac_stmt) = &program.statements[0].kind else {
        panic!("expected fac statement");
    };
    assert_eq!(fac_stmt.modifier, FacModifier::None);
}

#[test]
fn fac_omnia_dum_parses_with_marker_and_condition() {
    let result = parse_ok("fac omnia { tacet } cape err { tacet } dum verum");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Fac(fac_stmt) = &program.statements[0].kind else {
        panic!("expected fac statement");
    };
    assert_eq!(fac_stmt.modifier, FacModifier::Omnia);
    assert!(fac_stmt.catch.is_some(), "handler must be recorded");
    assert!(fac_stmt.while_.is_some(), "dum condition must be recorded");
}

/// Minimal en surface carrying the `do all` spellings (`fac`/`omnia`/`cape`).
struct EnFacOmniaPack;

impl KeywordPack for EnFacOmniaPack {
    fn pack_id(&self) -> &'static str {
        "en-fac-omnia"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<radix_lexer::TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "do" => Some("fac"),
            "all" => Some("omnia"),
            "device" => Some("machina"),
            "catch" => Some("cape"),
            "while" => Some("dum"),
            "pass" => Some("tacet"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

#[test]
fn english_do_all_parses_through_the_same_flag() {
    let source = "do all { pass } catch err { pass }";
    let lexed = radix_lexer::lex_with_locale_pack(source, &EnFacOmniaPack);
    let result = parse(lexed);
    assert!(
        result.success(),
        "en `do all … catch` must parse, got: {:?}",
        result.errors
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Fac(fac_stmt) = &program.statements[0].kind else {
        panic!("expected fac statement");
    };
    assert_eq!(fac_stmt.modifier, FacModifier::Omnia);
    assert!(fac_stmt.catch.is_some());
}

#[test]
fn english_do_all_while_parses_through_the_same_flag() {
    let source = "do all { pass } catch err { pass } while true";
    let lexed = radix_lexer::lex_with_locale_pack(source, &EnFacOmniaPack);
    let result = parse(lexed);
    assert!(
        result.success(),
        "en `do all … catch … while` must parse, got: {:?}",
        result.errors
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Fac(fac_stmt) = &program.statements[0].kind else {
        panic!("expected fac statement");
    };
    assert_eq!(fac_stmt.modifier, FacModifier::Omnia);
    assert!(fac_stmt.while_.is_some());
}

#[test]
fn parses_fac_machina_with_cape_and_dum() {
    let result = parse_ok("fac machina { tacet } cape err { tacet } dum verum");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Fac(fac_stmt) = &program.statements[0].kind else {
        panic!("expected fac statement");
    };
    assert_eq!(fac_stmt.modifier, FacModifier::Machina);
    assert!(fac_stmt.catch.is_some() && fac_stmt.while_.is_some());
}

#[test]
fn machina_stays_an_ordinary_identifier_outside_the_fac_slot() {
    let result = parse_ok("sit numerus machina ← 1");
    assert!(result.program.is_some());
}

#[test]
fn two_fac_modifiers_are_refused_by_name_in_either_order() {
    for source in [
        "fac omnia machina { tacet }",
        "fac machina omnia { tacet }",
        "fac machina machina { tacet }",
    ] {
        assert_parse_error_issue_la(source, "fac_modifier_redundant");
    }
}

#[test]
fn english_do_device_parses_as_machina() {
    let source = "do device { pass } catch err { pass } while true";
    let lexed = radix_lexer::lex_with_locale_pack(source, &EnFacOmniaPack);
    let result = parse(lexed);
    assert!(result.success(), "{:?}", result.errors);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Fac(fac_stmt) = &program.statements[0].kind else {
        panic!("expected fac statement");
    };
    assert_eq!(fac_stmt.modifier, FacModifier::Machina);
    assert!(fac_stmt.catch.is_some() && fac_stmt.while_.is_some());
}
