//! A declared value binding named `nihil` reads as that binding.
//!
//! `nihil` in value position is the retired null spelling (`PARSE030
//! retired_null_value_spelling`). Keywords are contextual and a binding may
//! shadow them (operator ruling 2026-10-01), so once a variable or parameter
//! named `nihil` is declared, a read of it is an ordinary identifier read. A
//! bare `nihil` with no such binding keeps the named error. The check does not
//! read the locale; the `en` cases use a minimal pack, the `la` cases the
//! default Latin surface.

use crate::test_support::{assert_parse_error_issue, parse_ok};
use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};

struct EnPack;

impl KeywordPack for EnPack {
    fn pack_id(&self) -> &'static str {
        "en"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "fn" => Some("functio"),
            "mut" => Some("varia"),
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "print" => Some("scribe"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, _latin_key: &str) -> Option<&str> {
        None
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        (text == "int").then_some("numerus")
    }
}

fn parse_en(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnPack))
}

fn retired_issue_reported(result: &ParseResult) -> bool {
    result.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "retired_null_value_spelling")
    })
}

#[test]
fn la_declared_nihil_variable_reads_as_the_binding() {
    let _ = parse_ok("functio f() {\n    varia numerus nihil ← 1\n    scribe nihil\n}\n");
}

#[test]
fn la_bare_nihil_without_binding_stays_retired_null_spelling() {
    assert_parse_error_issue(
        "functio f() {\n    scribe nihil\n}\n",
        "retired_null_value_spelling",
    );
}

#[test]
fn en_declared_nihil_variable_reads_as_the_binding() {
    let result = parse_en("fn f() {\n    mut int nihil ← 1\n    print nihil\n}\n");
    assert!(
        result.success(),
        "declared `nihil` must read as a variable: {:?}",
        result.errors
    );
}

#[test]
fn en_bare_nihil_without_binding_stays_retired_null_spelling() {
    let result = parse_en("fn f() {\n    print nihil\n}\n");
    assert!(
        retired_issue_reported(&result),
        "bare `nihil` must report retired_null_value_spelling: {:?}",
        result.errors
    );
}
