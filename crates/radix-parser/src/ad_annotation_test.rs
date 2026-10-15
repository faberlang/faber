//! Parser contract for the compiler-owned `@ ad '<route>'` handler annotation.

use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{AnnotationKind, StmtKind};

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
            "call" => Some("ad"),
            "fn" => Some("functio"),
            "public" => Some("publica"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "ad" => Some("call"),
            "functio" => Some("fn"),
            "publica" => Some("public"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

struct LaPack;

impl KeywordPack for LaPack {
    fn pack_id(&self) -> &'static str {
        "la"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "ad" => Some("ad"),
            "functio" => Some("functio"),
            "publica" => Some("publica"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "ad" => Some("ad"),
            "functio" => Some("functio"),
            "publica" => Some("publica"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_en(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnPack))
}

fn parse_la(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &LaPack))
}

fn issues(result: &ParseResult) -> Vec<String> {
    result
        .errors
        .iter()
        .flat_map(|error| error.args.iter())
        .filter(|arg| arg.name == "issue")
        .map(|arg| arg.value.clone())
        .collect()
}

fn assert_issue(result: &ParseResult, issue: &str) {
    let found = issues(result);
    assert!(
        found.iter().any(|found| found == issue),
        "expected issue {issue}, got {found:?}"
    );
}

fn route_of(result: &ParseResult) -> String {
    assert!(result.success(), "parse errors: {:?}", result.errors);
    let program = result.program.as_ref().expect("program");
    let stmt = &program.statements[0];
    assert!(matches!(stmt.kind, StmtKind::Func(_)), "expected functio");
    let route = stmt
        .annotations
        .iter()
        .find_map(|annotation| match &annotation.kind {
            AnnotationKind::Ad(ad) => Some(ad.route),
            _ => None,
        })
        .expect("structured @ ad annotation");
    result.interner.resolve(route).to_owned()
}

#[test]
fn latin_ad_annotation_parses_to_structured_route() {
    let result = parse_la("@ ad 'orders:list'\nfunctio liste() { }");
    assert_eq!(route_of(&result), "orders:list");
}

#[test]
fn english_call_annotation_parses_to_structured_route() {
    let result = parse_en("@ call 'clock/now'\nfn nunc() { }");
    assert_eq!(route_of(&result), "clock/now");
}

#[test]
fn ad_annotation_stacks_with_visibility() {
    let result = parse_la("@ ad 'x:y'\n@ publica\nfunctio f() { }");
    assert_eq!(route_of(&result), "x:y");
    let before = parse_la("@ publica\n@ ad 'x:y'\nfunctio f() { }");
    assert_eq!(route_of(&before), "x:y");
}

#[test]
fn pattern_route_is_deferred() {
    assert_issue(
        &parse_la("@ ad sed \"orders:.*\"\nfunctio f() { }"),
        "ad_pattern_route_deferred",
    );
}

#[test]
fn missing_route_is_pointed() {
    assert_issue(&parse_la("@ ad\nfunctio f() { }"), "ad_annotation_route");
    assert_issue(
        &parse_la("@ ad \"x:y\"\nfunctio f() { }"),
        "ad_annotation_route",
    );
}

#[test]
fn trailing_token_after_route_is_rejected() {
    assert_issue(
        &parse_la("@ ad 'x:y' extra\nfunctio f() { }"),
        "unexpected_annotation_token",
    );
}

#[test]
fn ad_annotation_requires_a_functio() {
    assert_issue(
        &parse_la("@ ad 'x:y'\nsit numerus n ← 1"),
        "ad_annotation_requires_functio",
    );
}

#[test]
fn wrong_locale_ad_spellings_are_rejected() {
    assert_issue(
        &parse_en("@ ad 'x:y'\nfn f() { }"),
        "wrong_locale_annotation",
    );
}
