//! Parsing contract for the locale-specific `return thread` / `redde filum` forms.

use crate::ParseResult;
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{Program, StmtKind};

struct EnglishThreadPack;

impl KeywordPack for EnglishThreadPack {
    fn pack_id(&self) -> &'static str {
        "en"
    }

    fn keyword_token_kind(&self, text: &str) -> Option<TokenKind> {
        (text == "return").then_some(TokenKind::Redde)
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "return" => Some("redde"),
            "thread" => Some("filum"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "redde" => Some("return"),
            "filum" => Some("thread"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse(source: &str, pack: &dyn KeywordPack) -> ParseResult {
    crate::parse(lex_with_locale_pack(source, pack))
}

fn thread_return(program: &Program) -> &radix_syntax::ReddeFilumStmt {
    let StmtKind::ReddeFilum(stmt) = &program.statements[0].kind else {
        panic!(
            "expected a distinct thread-return statement, got {:?}",
            program.statements[0].kind
        );
    };
    stmt
}

#[test]
fn parses_thread_return_on_english_surface() {
    let result = parse("return thread", &EnglishThreadPack);
    assert!(result.success(), "parse errors: {:?}", result.errors);
    assert!(
        thread_return(result.program.as_ref().expect("program"))
            .value
            .is_none()
    );
}

#[test]
fn parses_thread_return_on_latin_surface() {
    let result = crate::test_support::parse_program_la("redde filum");
    assert!(result.success(), "parse errors: {:?}", result.errors);
    assert!(
        thread_return(result.program.as_ref().expect("program"))
            .value
            .is_none()
    );
}

#[test]
fn preserves_a_forbidden_thread_return_value_for_semantic_diagnosis() {
    let result = parse("return thread 7", &EnglishThreadPack);
    assert!(result.success(), "parse errors: {:?}", result.errors);
    assert!(
        thread_return(result.program.as_ref().expect("program"))
            .value
            .is_some()
    );
}
