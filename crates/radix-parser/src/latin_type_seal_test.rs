//! Locale sealing for type names: Latin is a locale pack like every other, so
//! a non-Latin source may not spell a type with its canonical Latin word.
//!
//! The pack here mimics the `en` surface (pack-authoritative, no Latin
//! fallback). The same words stay legal in a `la` (registry-direct) file, and
//! a user type that shares a Latin type word is an ordinary name.

use super::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex, lex_with_locale_pack};

struct EnLikePack;

impl KeywordPack for EnLikePack {
    fn pack_id(&self) -> &'static str {
        "en-like"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "fn" => Some("functio"),
            "genus" => Some("genus"),
            "is" => Some("est"),
            "match" => Some("discerne"),
            "case" => Some("casu"),
            "return" => Some("redde"),
            "mut" => Some("varia"),
            "print" => Some("nota"),
            _ => None,
        }
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "int" => Some("numerus"),
            "list" => Some("lista"),
            "none" => Some("nihil"),
            _ => None,
        }
    }

    fn type_surfaces(&self) -> Vec<(String, String)> {
        [("numerus", "int"), ("lista", "list"), ("nihil", "none")]
            .into_iter()
            .map(|(latin, surface)| (latin.to_owned(), surface.to_owned()))
            .collect()
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_en(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnLikePack))
}

fn wrong_locale_types(result: &ParseResult) -> Vec<String> {
    result
        .errors
        .iter()
        .filter(|error| {
            error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "wrong_locale_type")
        })
        .map(|error| {
            error
                .args
                .iter()
                .find(|arg| arg.name == "type")
                .map(|arg| arg.value.clone())
                .unwrap_or_default()
        })
        .collect()
}

fn assert_sealed(source: &str, word: &str) {
    let result = parse_en(source);
    assert_eq!(
        wrong_locale_types(&result),
        vec![word.to_owned()],
        "en file `{source}` must reject Latin `{word}`: {:?}",
        result.errors
    );
}

fn assert_latin_accepts(source: &str) {
    let result = parse(lex(source));
    assert!(
        result.success(),
        "la file `{source}` must accept Latin type words: {:?}",
        result.errors
    );
}

#[test]
fn en_file_rejects_latin_primitive_type_name() {
    assert_sealed("const numerus x = 1", "numerus");
}

#[test]
fn en_file_rejects_latin_collection_type_name() {
    assert_sealed("fn f(lista<int> xs) { }", "lista");
}

#[test]
fn en_file_rejects_latin_null_type_in_union() {
    assert_sealed("fn f(int ∪ nihil v) { }", "nihil");
}

#[test]
fn en_file_rejects_latin_iuncta_type() {
    assert_sealed("fn f(iuncta<int, int> p) { }", "iuncta");
}

#[test]
fn en_file_names_its_own_spelling_in_the_diagnostic() {
    let result = parse_en("const numerus x = 1");
    let expected = result
        .errors
        .iter()
        .flat_map(|error| error.args.iter())
        .find(|arg| arg.name == "expected")
        .map(|arg| arg.value.clone());
    assert_eq!(expected.as_deref(), Some("int"));
}

#[test]
fn en_file_accepts_its_own_type_spellings() {
    let result = parse_en("const int x = 1\nfn f(list<int> xs, int ∪ none v) { }");
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn la_file_accepts_latin_type_words() {
    assert_latin_accepts("fixum numerus x = 1");
    assert_latin_accepts("functio f(lista<numerus> xs) { }");
    assert_latin_accepts("functio f(numerus ∪ nihil v) { }");
    assert_latin_accepts("functio f(iuncta<numerus, numerus> p) { }");
}

#[test]
fn user_type_named_like_a_latin_type_word_is_not_rejected() {
    let result = parse_en("genus copia { }\nfn f(copia c) { }");
    assert!(
        wrong_locale_types(&result).is_empty(),
        "declared type `copia` must stay an ordinary name: {:?}",
        result.errors
    );
}

fn expected_of(result: &ParseResult) -> Option<String> {
    result
        .errors
        .iter()
        .flat_map(|error| error.args.iter())
        .find(|arg| arg.name == "expected")
        .map(|arg| arg.value.clone())
}

#[test]
fn en_file_rejects_latin_null_type_in_union_est_and_casu_forms() {
    let forms = [
        "fn f(int ∪ nihil v) { }",
        "fn f(int ∪ none v) { return v is nihil }",
        "fn f(int ∪ none v) { match v { case nihil { } } }",
    ];
    for source in forms {
        let result = parse_en(source);
        assert_eq!(
            wrong_locale_types(&result),
            vec!["nihil".to_owned()],
            "en file `{source}` must reject Latin `nihil`: {:?}",
            result.errors
        );
        assert_eq!(
            expected_of(&result).as_deref(),
            Some("none"),
            "`{source}` must point at `none`"
        );
    }
}

#[test]
fn la_file_accepts_latin_null_type_in_union_est_and_casu_forms() {
    assert_latin_accepts("functio f(numerus ∪ nihil v) { }");
    assert_latin_accepts("functio f(numerus ∪ nihil v) { redde v est nihil }");
    assert_latin_accepts("functio f(numerus ∪ nihil v) { discerne v { casu nihil { } } }");
}

#[test]
fn en_file_keeps_latin_type_words_as_expression_names() {
    for word in ["ratio", "valor", "lista", "numerus"] {
        let source = format!(
            "mut int {word} ← 1\nprint {word}\n{word} ← 2\nconst int y = {word} + 1\nf({word})\nprint {word}\n\n{word} ← 3\nprint {word}"
        );
        let result = parse_en(&source);
        assert!(
            wrong_locale_types(&result).is_empty(),
            "en file `{source}` must read `{word}` as a name: {:?}",
            result.errors
        );
    }
}

#[test]
fn en_file_still_rejects_the_same_words_in_a_type_slot() {
    for word in ["valor", "lista", "numerus"] {
        let source = format!("mut int {word} ← 1\nprint {word}\n{word} ← 2\nconst {word} y = 1");
        let result = parse_en(&source);
        assert_eq!(
            wrong_locale_types(&result),
            vec![word.to_owned()],
            "en file `{source}` must reject `{word}` in a type slot: {:?}",
            result.errors
        );
    }
}

#[test]
fn en_file_rejects_latin_type_pattern_and_la_file_accepts_it() {
    assert_sealed("fn f(int v) { match v { case numerus { } } }", "numerus");
    assert_latin_accepts("functio f(numerus v) { discerne v { casu numerus { } } }");
}
