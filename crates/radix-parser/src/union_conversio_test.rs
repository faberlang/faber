//! Parser contract for the `@ conversio` standalone-function marker.

use crate::{parse, ParseResult};
use radix_lexer::{lex, lex_with_locale_pack, KeywordPack, TokenKind};
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
            "union" => Some("discretio"),
            "conversion" => Some("conversio"),
            "functio" => Some("functio"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "discretio" => Some("union"),
            "conversio" => Some("conversion"),
            "functio" => Some("functio"),
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
            "discretio" => Some("discretio"),
            "conversio" => Some("conversio"),
            "functio" => Some("functio"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "discretio" => Some("discretio"),
            "conversio" => Some("conversio"),
            "functio" => Some("functio"),
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

#[test]
fn standalone_conversio_marker_parses_in_latin_and_english_surfaces() {
    let result = parse_la("@ conversio functio f(Source err) → Dest { redde finge From(err) }");
    assert!(result.success(), "parse errors: {:?}", result.errors);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    assert_eq!(result.interner.resolve(func.name.name), "f");
    assert!(matches!(
        program.statements[0].annotations.as_slice(),
        [annotation] if matches!(annotation.kind, AnnotationKind::Conversio)
    ));

    let english = parse_en("@ conversion functio f(Source err) → Dest { redde finge From(err) }");
    assert!(english.success(), "parse errors: {:?}", english.errors);
    let StmtKind::Func(func) = &english.program.as_ref().unwrap().statements[0].kind else {
        panic!("expected function declaration");
    };
    assert_eq!(english.interner.resolve(func.name.name), "f");
    assert!(matches!(
        english.program.as_ref().unwrap().statements[0]
            .annotations
            .as_slice(),
        [annotation] if matches!(annotation.kind, AnnotationKind::Conversio)
    ));
}

#[test]
fn conversio_marker_stacks_with_a_preceding_annotation() {
    let result = parse(lex(
        "@ kernel @ conversio functio f(Source err) → Dest { redde finge From(err) }",
    ));
    assert!(result.success(), "parse errors: {:?}", result.errors);
    let program = result.program.as_ref().expect("program");
    assert!(matches!(&program.statements[0].kind, StmtKind::Func(_)));
    assert_eq!(program.statements[0].annotations.len(), 2);
    assert!(matches!(
        program.statements[0].annotations[1].kind,
        AnnotationKind::Conversio
    ));
}

#[test]
fn retired_union_arm_form_is_rejected() {
    let latin = parse_la("discretio Dest { @ conversio Source err {} Variant }");
    assert!(!latin.success());
    let english = parse_en("union Dest { @ conversion Source err {} Variant }");
    assert!(!english.success());
    // The retracted after-variant latch diagnostic is gone with the arm form.
    let after_variant = parse(lex("discretio Dest { First, @ conversio Source {} }"));
    assert!(!after_variant.success());
    assert!(!after_variant.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "union_conversio_after_variant")
    }));
}

#[test]
fn wrong_locale_conversio_spellings_are_parse018() {
    let english = parse_en("@ conversio functio f(Source err) → Dest { redde finge From(err) }");
    assert!(english.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "wrong_locale_annotation")
    }));
    let latin = parse_la("@ conversion functio f(Source err) → Dest { redde finge From(err) }");
    assert!(latin.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "wrong_locale_annotation")
    }));
}

#[test]
fn union_parses_as_pure_shape() {
    let result = parse(lex(
        "discretio Dest {\n    @ commune\n    textus prefix\n    First,\n    Source { textus message }\n}",
    ));
    assert!(result.success(), "parse errors: {:?}", result.errors);
    let StmtKind::Union(union) = &result.program.as_ref().unwrap().statements[0].kind else {
        panic!("expected union declaration");
    };
    assert_eq!(union.shared_fields.len(), 1);
    assert_eq!(union.variants.len(), 2);
    assert_eq!(
        result.interner.resolve(union.variants[1].name.name),
        "Source"
    );
}
