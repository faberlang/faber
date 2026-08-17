//! Pack-aware lexer tests that need `LocalePack` (monolith residual).
//!
//! Non-pack lexer unit tests live in `radix-lexer`. These residual tests prove
//! the `KeywordPack` impl for reader locales still wires through `radix_module::lexer`.

use crate::lexer::{lex_with_locale_pack, TokenKind};
use crate::locale::LocalePack;

#[test]
fn locale_pack_keywords_lex_to_current_tokens() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
incipit = { canonical = "入口", aliases = ["开始", "incipit"] }

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");

    let result = lex_with_locale_pack("入口 salve() { }", &pack);
    assert!(result.errors.is_empty());
    // Keywords lex as identifiers; the pack spelling resolves to the latin
    // identity through the recorded key map.
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
    assert_eq!(
        result
            .reader_keyword_latin_keys
            .get("入口")
            .map(String::as_str),
        Some("incipit")
    );

    let alias = lex_with_locale_pack("开始 salve() { }", &pack);
    assert!(alias.errors.is_empty());
    assert!(matches!(alias.tokens[0].kind, TokenKind::Ident(_)));
    assert_eq!(
        alias
            .reader_keyword_latin_keys
            .get("开始")
            .map(String::as_str),
        Some("incipit")
    );
}

#[test]
fn locale_pack_contextual_keyword_and_type_lex_to_latin_identities() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "en"
fallback = ["la"]

[keywords]
fiet = { canonical = "async" }

[types]
numerus = { canonical = "int" }

[llm]
system_prompt_snippet = "emit LLM-oriented Faber"
exemplars = ["./exemplars/salve-munde.llm.fab"]
"#,
    )
    .expect("pack should validate");

    let result = lex_with_locale_pack("async int", &pack);
    assert!(result.errors.is_empty());

    let TokenKind::Ident(async_sym) = result.tokens[0].kind else {
        panic!("async should lex as source-local contextual identifier");
    };
    let TokenKind::Ident(int_sym) = result.tokens[1].kind else {
        panic!("int should lex as source-local type identifier");
    };

    assert_eq!(result.interner.resolve(async_sym), "async");
    assert_eq!(result.interner.resolve(int_sym), "int");
    assert_eq!(
        result
            .reader_keyword_latin_keys
            .get("async")
            .map(String::as_str),
        Some("fiet")
    );
    assert_eq!(result.reader_type_latin_keys.get("int").map(String::as_str), Some("numerus"));
}

#[test]
fn undeclared_latin_spelling_lexes_as_plain_identifier_under_pack() {
    // S2-U2 lexer proof: under a non-`la` pack the Latin spelling `functio`
    // (undeclared) lexes as a plain identifier with NO recorded latin key,
    // while the declared spelling `函数` lexes as an identifier with its
    // pack-declared key recorded. The surface-mode signal also reaches the
    // result so the parser can gate claiming.
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");

    let result = lex_with_locale_pack("函数 functio", &pack);
    assert!(result.errors.is_empty());
    assert_eq!(result.keyword_surface, crate::lexer::KeywordSurfaceMode::PackAuthoritative);
    assert!(matches!(result.tokens[0].kind, TokenKind::Ident(_)));
    assert!(matches!(result.tokens[1].kind, TokenKind::Ident(_)));
    assert_eq!(
        result
            .reader_keyword_latin_keys
            .get("函数")
            .map(String::as_str),
        Some("functio"),
        "declared pack spelling must record its latin key"
    );
    assert!(
        !result.reader_keyword_latin_keys.contains_key("functio"),
        "undeclared Latin spelling must not record a latin key under the pack"
    );
}

#[test]
fn la_and_no_pack_lex_registry_direct_surface() {
    // S2-U2 surface proof: no-pack (`lex`) and the `la` pack both report the
    // registry-direct surface, so the parser keeps raw-source claiming there.
    let no_pack = crate::lexer::lex("functio salve() { }");
    assert_eq!(no_pack.keyword_surface, crate::lexer::KeywordSurfaceMode::RegistryDirect);
    assert_eq!(
        no_pack
            .reader_keyword_latin_keys
            .get("functio")
            .map(String::as_str),
        Some("functio"),
        "no-pack records identity keys for registered keywords"
    );

    let la_pack = crate::locale::latin_locale_pack();
    let la = lex_with_locale_pack("functio salve() { }", &la_pack);
    assert_eq!(la.keyword_surface, crate::lexer::KeywordSurfaceMode::RegistryDirect);
    assert_eq!(
        la.reader_keyword_latin_keys
            .get("functio")
            .map(String::as_str),
        Some("functio"),
        "la identity rows record identity keys"
    );
}
