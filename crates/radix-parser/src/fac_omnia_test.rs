//! `fac omnia` (en `do all`) marker on the `fac` construct.
//!
//! The marker is surface-only: host semantics stay those of plain `fac`. It
//! parses between `fac` and the body block, combines with `cape`, and is
//! rejected ahead of `dum` because the do-while form lowers to a plain loop
//! with no marker slot in HIR — accepting it would silently drop the marker
//! on re-emission.

use crate::test_support::{parse_ok, parse_program};
use crate::{ParseErrorKind, parse};
use radix_lexer::KeywordPack;
use radix_syntax::StmtKind;

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
    assert!(fac_stmt.omnia, "`omnia` marker must parse onto the fac");
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
    assert!(!fac_stmt.omnia, "plain `fac` must not gain the marker");
}

#[test]
fn fac_omnia_dum_rejected_at_dum() {
    // The do-while form lowers to a plain loop with no marker carrier; the
    // error lands on the `dum` token so the grammar gap is explicit.
    let source = "fac omnia { tacet } dum verum";
    let result = parse_program(source);
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::StatementInvalid
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "fac_omnia_dum")
        })
        .unwrap_or_else(|| panic!("expected fac_omnia_dum, got {:?}", result.errors));
    let dum_at = u32::try_from(source.find("dum").expect("dum token")).expect("offset fits");
    assert_eq!(
        err.span.start, dum_at,
        "error must point at the dum token, got {:?}",
        err.span
    );
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
            "catch" => Some("cape"),
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
    assert!(fac_stmt.omnia, "en `all` must set the same omnia flag");
    assert!(fac_stmt.catch.is_some());
}
