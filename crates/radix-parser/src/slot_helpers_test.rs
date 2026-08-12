//! Unit tests for the dual-mode grammar-slot keyword helpers.
//!
//! A grammar slot accepts a keyword in either the legacy reserved-token form
//! (`TokenKind::Iuncta`) or the tokenless surface form (`Ident` matching the
//! Latin key, or a reader-pack surface spelling resolving to it). These tests
//! pin both modes directly on the helpers, independent of any dispatch site,
//! so the token→spelling migration cannot silently break slot recognition.

use super::{ParseOptions, Parser};
use radix_lexer::{
    lex, lex_with_locale_pack, KeywordPack, LexResult, TokenKind,
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
            // Genus, statements, and literals let class-body regression tests
            // exercise localized type spellings and statement terminals end to
            // end.
            "genus" => Some("genus"),
            "fn" => Some("functio"),
            "var" => Some("varia"),
            "if" => Some("si"),
            "then" => Some("ergo"),
            "return" => Some("redde"),
            "true" => Some("verum"),
            "false" => Some("falsum"),
            _ => None,
        }
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "bool" => Some("bivalens"),
            "int" => Some("numerus"),
            "string" => Some("textus"),
            _ => None,
        }
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
        keyword_surface,
        interner,
        ..
    } = lex_with_locale_pack(source, &SlotTestPack);
    Parser::new_with_reader_identities(
        tokens,
        interner,
        ParseOptions::default(),
        reader_keyword_latin_keys,
        reader_type_latin_keys,
        keyword_surface,
    )
}

#[test]
fn slot_helper_matches_legacy_reserved_token_form() {
    let mut parser = parser_over("iuncta");
    assert!(parser.check_keyword_ident(TokenKind::Iuncta));
    assert!(!parser.check_keyword_ident(TokenKind::Modulus));
    assert!(parser.eat_keyword_ident(TokenKind::Iuncta));
    assert!(
        !parser.check_keyword_ident(TokenKind::Iuncta),
        "consumed token must not re-match"
    );
}

#[test]
fn slot_helper_leaves_undeclared_latin_spelling_an_identifier_under_pack() {
    // The pack does not localize `iuncta`, so it lexes as an `Ident` with no
    // recorded latin key. Under a non-`la` pack the parser is
    // pack-authoritative: an undeclared spelling stays an ordinary identifier
    // and cannot claim the `iuncta` grammar slot (S2-U2). The registry-direct
    // (`la`/no-pack) claim of the same spelling is pinned by
    // `slot_helper_matches_legacy_reserved_token_form`.
    let mut parser = parser_over_with_pack("iuncta");
    assert!(!parser.check_keyword_ident(TokenKind::Iuncta));
    assert!(!parser.eat_keyword_ident(TokenKind::Iuncta));
}

#[test]
fn slot_helper_matches_localized_pack_spelling() {
    let mut parser = parser_over_with_pack("本地");
    assert!(parser.check_keyword_ident(TokenKind::Iuncta));
    assert!(!parser.check_keyword_ident(TokenKind::Modulus));
    assert!(parser.eat_keyword_ident(TokenKind::Iuncta));
}

#[test]
fn slot_helper_matches_tokenless_precedent_via_pack_key() {
    // Existing tokenless precedent: `await` → `tacebit` through the pack map.
    let mut parser = parser_over_with_pack("await");
    assert!(parser.check_keyword_ident(TokenKind::Tacebit));
    assert!(!parser.check_keyword_ident(TokenKind::Iuncta));
    assert!(parser.eat_keyword_ident(TokenKind::Tacebit));
}

#[test]
fn slot_helper_rejects_ordinary_identifier() {
    let mut parser = parser_over("nomine");
    assert!(!parser.check_keyword_ident(TokenKind::Iuncta));
    assert!(!parser.eat_keyword_ident(TokenKind::Iuncta));
}

#[test]
fn slot_helper_expect_errors_when_absent() {
    let mut parser = parser_over("nomine");
    let result = parser.expect_keyword_ident(TokenKind::Iuncta, "expected 'iuncta'");
    assert!(
        result.is_err(),
        "missing slot keyword must produce an error"
    );
}

#[test]
fn localized_type_spelling_after_initialized_field_is_not_an_operand() {
    // Regression: the field-boundary guard matched type spellings against the
    // latin conversio-type list by raw source text. Under a pack that
    // localizes `textus` → `string`, the guard missed the second field's type
    // and the retired `true`-prefix predicate check fired (PARSE001) on
    // `bool visible = true` followed by `string name`.
    let mut parser =
        parser_over_with_pack("genus X {\n    bool visible = true\n    string name\n}\n");
    let result = parser.parse_program();
    assert!(
        result.errors.is_empty(),
        "localized type spelling after an initialized field must not fire the retired prefix predicate: {:?}",
        result.errors
    );
}

#[test]
fn statement_terminal_literal_is_not_a_retired_predicate() {
    // Regression: `return false` followed by the next statement's identifier
    // (`j ← j + 1`) read as the retired `falsum <operand>` prefix form and
    // fired PARSE001. A literal in operand position is a value; only a
    // statement-start literal followed by an operand is the retired form.
    let mut parser = parser_over_with_pack(
        "genus X {\n    fn f() → bool {\n        var int j ← 0\n        if j ≡ 1 then return false\n        j ← j + 1\n        return true\n    }\n}\n",
    );
    let result = parser.parse_program();
    assert!(
        result.errors.is_empty(),
        "statement-terminal literal must not fire the retired prefix predicate: {:?}",
        result.errors
    );

    // The retired standalone form must still be rejected.
    let mut parser =
        parser_over_with_pack("genus X {\n    fn f() → bool {\n        false j\n    }\n}\n");
    let result = parser.parse_program();
    assert!(
        result.errors.iter().any(|error| {
            matches!(error.kind, super::ParseErrorKind::Expected)
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "retired_prefix_predicate")
        }),
        "standalone `false <operand>` must still be rejected: {:?}",
        result.errors
    );
}
