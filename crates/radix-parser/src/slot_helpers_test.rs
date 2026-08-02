//! Unit tests for the dual-mode grammar-slot keyword helpers.
//!
//! A grammar slot accepts a keyword in either the legacy reserved-token form
//! (`TokenKind::Iuncta`) or the tokenless surface form (`Ident` matching the
//! Latin key, or a reader-pack surface spelling resolving to it). These tests
//! pin both modes directly on the helpers, independent of any dispatch site,
//! so the token→spelling migration cannot silently break slot recognition.

use super::{ParseOptions, Parser};
use radix_lexer::{
    lex, lex_with_reader_pack, KeywordPack, LatinKeywordFallback, LexResult, TokenKind,
};

/// Minimal reader pack: localizes `iuncta` to `本地` and `tacebit` to `await`
/// as tokenless keywords (no token emission), with no Latin fallback declared.
struct SlotTestPack;

impl KeywordPack for SlotTestPack {
    fn pack_id(&self) -> &'static str {
        "slot-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "本地" => Some("iuncta"),
            "await" => Some("tacebit"),
            _ => None,
        }
    }

    fn latin_keyword_fallback(&self, _text: &str) -> Option<LatinKeywordFallback> {
        None
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

/// Build a parser over `source` on the plain Latin surface (no reader pack).
fn parser_over(source: &str) -> Parser {
    let lexed = lex(source);
    Parser::new(lexed.tokens, lexed.interner, ParseOptions::default())
}

/// Build a parser over `source`, keeping the reader-pack identity maps the
/// lexer recorded (the same wiring `parse_with_options` uses).
fn parser_over_with_pack(source: &str) -> Parser {
    let LexResult {
        tokens,
        reader_keyword_latin_keys,
        reader_type_latin_keys,
        interner,
        ..
    } = lex_with_reader_pack(source, &SlotTestPack);
    Parser::new_with_reader_identities(
        tokens,
        interner,
        ParseOptions::default(),
        reader_keyword_latin_keys,
        reader_type_latin_keys,
    )
}

#[test]
fn slot_helper_matches_legacy_reserved_token_form() {
    let mut parser = parser_over("iuncta");
    assert!(parser.check_keyword_in_slot("iuncta"));
    assert!(!parser.check_keyword_in_slot("modulus"));
    assert!(parser.eat_keyword_in_slot("iuncta"));
    assert!(
        !parser.check_keyword_in_slot("iuncta"),
        "consumed token must not re-match"
    );
}

#[test]
fn slot_helper_matches_tokenless_ident_by_latin_text() {
    // The pack does not localize `iuncta`, so it lexes as an `Ident` — the
    // Latin-fallback shape a migrated keyword takes under a partial pack.
    let mut parser = parser_over_with_pack("iuncta");
    assert!(parser.check_keyword_in_slot("iuncta"));
    assert!(parser.eat_keyword_in_slot("iuncta"));
}

#[test]
fn slot_helper_matches_localized_pack_spelling() {
    let mut parser = parser_over_with_pack("本地");
    assert!(parser.check_keyword_in_slot("iuncta"));
    assert!(!parser.check_keyword_in_slot("modulus"));
    assert!(parser.eat_keyword_in_slot("iuncta"));
}

#[test]
fn slot_helper_matches_tokenless_precedent_via_pack_key() {
    // Existing tokenless precedent: `await` → `tacebit` through the pack map.
    let mut parser = parser_over_with_pack("await");
    assert!(parser.check_keyword_in_slot("tacebit"));
    assert!(!parser.check_keyword_in_slot("iuncta"));
    assert!(parser.eat_keyword_in_slot("tacebit"));
}

#[test]
fn slot_helper_rejects_ordinary_identifier() {
    let mut parser = parser_over("nomine");
    assert!(!parser.check_keyword_in_slot("iuncta"));
    assert!(!parser.eat_keyword_in_slot("iuncta"));
}

#[test]
fn slot_helper_expect_errors_when_absent() {
    let mut parser = parser_over("nomine");
    let result = parser.expect_keyword_in_slot("iuncta", "expected 'iuncta'");
    assert!(
        result.is_err(),
        "missing slot keyword must produce an error"
    );
}
