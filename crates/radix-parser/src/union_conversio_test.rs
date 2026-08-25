//! Parser contract for declaration-owned `@ conversio` arms.

use crate::{parse, ParseResult};
use radix_lexer::{lex, lex_with_locale_pack, KeywordPack, TokenKind};
use radix_syntax::{AnnotationKind, StmtKind, TypeExprKind};

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
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "discretio" => Some("union"),
            "conversio" => Some("conversion"),
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
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "discretio" => Some("discretio"),
            "conversio" => Some("conversio"),
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

fn assert_arm(result: &ParseResult, source_name: &str, self_name: &str) {
    assert!(result.success(), "parse errors: {:?}", result.errors);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Union(union) = &program.statements[0].kind else {
        panic!("expected union declaration");
    };
    assert_eq!(union.arms.len(), 1);
    let arm = &union.arms[0];
    assert_eq!(
        result
            .interner
            .resolve(arm.self_ident.as_ref().expect("self binding").name),
        self_name
    );
    let TypeExprKind::Named(source, args) = &arm.source.kind else {
        panic!("expected named source type");
    };
    assert!(args.is_empty());
    assert_eq!(result.interner.resolve(source.name), source_name);
    assert!(arm.body.statements.is_empty());
}

#[test]
fn conversion_arm_parses_in_latin_and_english_surfaces() {
    assert_arm(
        &parse_la("discretio Dest { @ conversio Source err {} Variant }"),
        "Source",
        "err",
    );
    assert_arm(
        &parse_en("union Dest { @ conversion Source err {} Variant }"),
        "Source",
        "err",
    );
}

#[test]
fn annotation_first_keeps_bare_braced_variant_after_variants() {
    let result = parse(lex("discretio Dest { First, Source { textus message } }"));
    assert!(result.success(), "parse errors: {:?}", result.errors);
    let StmtKind::Union(union) = &result.program.as_ref().unwrap().statements[0].kind else {
        panic!("expected union declaration");
    };
    assert!(union.arms.is_empty());
    assert_eq!(union.variants.len(), 2);
    assert_eq!(
        result.interner.resolve(union.variants[1].name.name),
        "Source"
    );
}

#[test]
fn conversion_arm_after_first_variant_is_rejected_by_the_latch() {
    let result = parse(lex("discretio Dest { First, @ conversio Source {} }"));
    assert!(!result.success());
    assert!(result.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "union_conversio_after_variant")
    }));
}

#[test]
fn wrong_locale_conversion_spellings_are_parse018() {
    let english = parse_en("union Dest { @ conversio Source {} Variant }");
    assert!(english.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "wrong_locale_annotation")
    }));
    let latin = parse_la("discretio Dest { @ conversion Source {} Variant }");
    assert!(latin.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "wrong_locale_annotation")
    }));
}

#[test]
fn conversion_annotation_is_typed_before_union_member_classification() {
    let result = parse(lex(
        "discretio Dest { @ conversio Source err { redde err } Variant }",
    ));
    assert!(result.success(), "parse errors: {:?}", result.errors);
    let StmtKind::Union(union) = &result.program.as_ref().unwrap().statements[0].kind else {
        panic!("expected union declaration");
    };
    assert!(matches!(
        union.arms[0].body.statements[0].annotations.as_slice(),
        []
    ));
    assert!(matches!(
        union.arms[0].body.statements[0].kind,
        StmtKind::Redde(_)
    ));
    // The typed annotation is consumed by the union arm and does not leak into
    // the shared-field region.
    assert!(!union.shared_fields.iter().any(|field| {
        field
            .annotations
            .iter()
            .any(|annotation| matches!(&annotation.kind, AnnotationKind::Conversio(_)))
    }));
}
