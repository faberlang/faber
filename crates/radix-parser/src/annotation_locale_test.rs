//! Reader-surface sealing for registered annotation families.
//!
//! A non-Latin reader pack must not silently reinterpret a canonical Latin
//! annotation spelling as an extension annotation. The pack's own spelling
//! remains valid, while the same family in Latin is a structured parse error.

use crate::{parse, ParseResult};
use radix_lexer::{lex_with_locale_pack, KeywordPack, TokenKind};

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
            "kernel" => Some("nucleum"),
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
fn en_nucleum_lane_modifier_is_sealed_in_both_directions() {
    assert!(parse_en("@ kernel lane \"pos\"\nfn probe() { }\n").success());
    assert!(parse_en("@ kernel { lane = \"pos\" }\nfn probe() { }\n").success());
    assert_wrong_locale_annotation("@ nucleum lane \"pos\"\nfn probe() { }\n", "kernel");
    assert_wrong_locale_annotation("@ nucleum { lane = \"pos\" }\nfn probe() { }\n", "kernel");
}

#[test]
fn en_annotation_visibility_spelling_is_sealed() {
    assert!(parse_en("@ public { }\nfn probe() { }\n").success());
    assert_wrong_locale_annotation("@ publica { }\nfn probe() { }\n", "public");
}

#[test]
fn en_closure_slot_sealing_leaves_annotation_identity_intact() {
    // KCM-02: the same `kernel` spelling owns both surfaces on the en pack —
    // the annotation position (already pinned above) and the closure-modifier
    // slot — while the Latin `nucleum` spelling stays sealed out of both.
    let result = parse_en("@ kernel\nfn probe() { }\n");
    assert!(
        result.success(),
        "en `@ kernel` annotation must keep parsing: {:?}",
        result.errors
    );

    let closure = parse_en("const _ probe ← (int value) kernel ∴ value\n");
    assert!(
        closure.success(),
        "en `kernel` closure modifier must parse: {:?}",
        closure.errors
    );
    let program = closure.program.as_ref().expect("program");
    let radix_syntax::StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let radix_syntax::ExprKind::Clausura(node) = &var.init.as_ref().expect("initializer").kind
    else {
        panic!("expected closure expression");
    };
    assert_eq!(
        node.modifier,
        radix_syntax::ClosureModifier::Nucleum,
        "en `kernel` must carry the nucleum identity in the closure slot"
    );

    let sealed = parse_en("const _ probe ← (int value) nucleum ∴ value\n");
    assert!(
        !sealed.success(),
        "Latin `nucleum` must stay sealed out of the en closure slot: {:?}",
        sealed.errors
    );
    assert_wrong_locale_annotation("@ nucleum\nfn probe() { }\n", "kernel");
}
