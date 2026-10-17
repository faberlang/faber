//! Reader-surface sealing for registered annotation families.
//!
//! A non-Latin reader pack must not silently reinterpret a canonical Latin
//! annotation spelling as an extension annotation. The pack's own spelling
//! remains valid, while the same family in Latin is a structured parse error.

use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};

struct EnAnnotationPack;

impl KeywordPack for EnAnnotationPack {
    fn pack_id(&self) -> &'static str {
        "en"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "fn" => Some("functio"),
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "kernel" => Some("nucleum"),
            "annotation" => Some("annotatio"),
            "fragment" => Some("fragment"),
            "lane" => Some("lane"),
            "public" => Some("publica"),
            "true" => Some("verum"),
            "false" => Some("falsum"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "nucleum" => Some("kernel"),
            "annotatio" => Some("annotation"),
            "fragment" => Some("fragment"),
            "lane" => Some("lane"),
            "publica" => Some("public"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        (text == "int").then_some("numerus")
    }
}

fn parse_en(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnAnnotationPack))
}

fn assert_wrong_locale_annotation(source: &str, expected: &str) {
    let result = parse_en(source);
    let error = result
        .errors
        .iter()
        .find(|error| {
            error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "wrong_locale_annotation")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected wrong-locale annotation error, got {:?}",
                result.errors
            )
        });
    assert!(
        error
            .args
            .iter()
            .any(|arg| arg.name == "expected" && arg.value == expected),
        "expected reader spelling {expected:?}, got {:?}",
        error.args
    );
}

#[test]
fn en_annotation_spelling_is_sealed_in_both_nucleum_forms() {
    assert!(parse_en("@ kernel\nfn probe() { }\n").success());
    assert!(parse_en("@ kernel { fragment = true }\nfn probe() { }\n").success());
    assert_wrong_locale_annotation("@ nucleum\nfn probe() { }\n", "kernel");
    assert_wrong_locale_annotation("@ nucleum { fragment = verum }\nfn probe() { }\n", "kernel");
}

#[test]
fn en_removed_lane_spelling_gets_the_migration_issue_and_stays_sealed() {
    for source in [
        "@ kernel lane \"pos\"\nfn probe() { }\n",
        "@ kernel { lane = \"pos\" }\nfn probe() { }\n",
    ] {
        let result = parse_en(source);
        assert!(
            result.errors.iter().any(|error| error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "nucleum_lane_removed")),
            "expected nucleum_lane_removed for {source:?}, got {:?}",
            result.errors
        );
    }
    assert_wrong_locale_annotation("@ nucleum lane \"pos\"\nfn probe() { }\n", "kernel");
    assert_wrong_locale_annotation("@ nucleum { lane = \"pos\" }\nfn probe() { }\n", "kernel");
}

#[test]
fn en_annotation_visibility_spelling_is_sealed() {
    assert!(parse_en("@ public { }\nfn probe() { }\n").success());
    assert_wrong_locale_annotation("@ publica { }\nfn probe() { }\n", "public");
}

#[test]
fn en_kernel_annotation_identity_is_sealed() {
    // The `kernel` spelling owns the annotation position on the en pack,
    // while the Latin `nucleum` spelling stays sealed out of it.
    let result = parse_en("@ kernel\nfn probe() { }\n");
    assert!(
        result.success(),
        "en `@ kernel` annotation must keep parsing: {:?}",
        result.errors
    );
    assert_wrong_locale_annotation("@ nucleum\nfn probe() { }\n", "kernel");
}

#[test]
fn en_annotation_contract_family_spelling_is_sealed() {
    assert!(parse_en("@ annotation { target = fn }\nclass Meta { }\n").success());
    assert!(parse_en("@ annotation\nclass Meta { }\n").success());
    assert_wrong_locale_annotation(
        "@ annotatio { target = fn }\nclass Meta { }\n",
        "annotation",
    );
    assert_wrong_locale_annotation("@ annotatio\nclass Meta { }\n", "annotation");
}
