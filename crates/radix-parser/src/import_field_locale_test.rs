//! Reader-surface sealing for the braced import record's field names.
//!
//! The five field names (`ex`, `visibilitas`, `nomen`, `ut`, `omnia`) are
//! keyword identities resolved through the reader pack: an en file writes
//! `from`, `visibility`, `name`, `as`, `all`, and a canonical Latin field word
//! in it is a named wrong-locale error rather than a silent acceptance.

use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};

/// `(en surface, canonical identity)` rows for the words these tests write.
const EN_ROWS: &[(&str, &str)] = &[
    ("import", "importa"),
    ("from", "ex"),
    ("visibility", "visibilitas"),
    ("name", "nomen"),
    ("as", "ut"),
    ("all", "omnia"),
    ("public", "publica"),
];

struct EnPack;

impl KeywordPack for EnPack {
    fn pack_id(&self) -> &'static str {
        "en"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        EN_ROWS
            .iter()
            .find(|(surface, _)| *surface == text)
            .map(|(_, latin)| *latin)
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        EN_ROWS
            .iter()
            .find(|(_, latin)| *latin == latin_key)
            .map(|(surface, _)| *surface)
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

/// The Latin pack is the identity surface: no rows, every spelling is its own
/// canonical key.
struct LaPack;

impl KeywordPack for LaPack {
    fn pack_id(&self) -> &'static str {
        "la"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
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

fn issue_args<'a>(
    result: &'a ParseResult,
    issue: &str,
) -> Option<&'a [radix_diagnostics::DiagnosticArg]> {
    result
        .errors
        .iter()
        .find(|error| {
            error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
        })
        .map(|error| error.args.as_slice())
}

fn arg<'a>(args: &'a [radix_diagnostics::DiagnosticArg], name: &str) -> Option<&'a str> {
    args.iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

#[test]
fn en_record_accepts_the_en_field_words() {
    let result =
        parse_en(r#"import { from = "norma:text", visibility = public, name = text, as = t }"#);
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn en_record_accepts_the_wildcard_field_word() {
    let result = parse_en(r#"import { from = "norma:text", all = t }"#);
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn la_record_accepts_the_latin_field_words() {
    let result =
        parse_la(r#"importa { ex = "norma:text", visibilitas = publica, nomen = text, ut = t }"#);
    assert!(result.success(), "{:?}", result.errors);
    let result = parse_la(r#"importa { ex = "norma:text", omnia = t }"#);
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn en_record_refuses_each_latin_field_word_by_name() {
    for (latin, source) in [
        ("ex", r#"import { ex = "norma:text" }"#),
        (
            "visibilitas",
            r#"import { from = "norma:text", visibilitas = public }"#,
        ),
        ("nomen", r#"import { from = "norma:text", nomen = text }"#),
        ("ut", r#"import { from = "norma:text", ut = t }"#),
        ("omnia", r#"import { from = "norma:text", omnia = t }"#),
    ] {
        let result = parse_en(source);
        let args = issue_args(&result, "wrong_locale_import_field")
            .unwrap_or_else(|| panic!("{latin}: expected wrong-locale error: {:?}", result.errors));
        assert_eq!(arg(args, "field"), Some(latin));
        let expected = EN_ROWS
            .iter()
            .find(|(_, identity)| *identity == latin)
            .map(|(surface, _)| *surface);
        assert_eq!(arg(args, "expected"), expected, "{latin}");
    }
}

#[test]
fn en_record_still_reports_a_word_that_is_no_field_as_unknown() {
    let result = parse_en(r#"import { from = "norma:text", alienum = 5 }"#);
    assert!(
        issue_args(&result, "unknown_import_field").is_some(),
        "{:?}",
        result.errors
    );
    assert!(issue_args(&result, "wrong_locale_import_field").is_none());
}

#[test]
fn la_record_refuses_the_en_field_words_as_unknown() {
    let result = parse_la(r#"importa { from = "norma:text" }"#);
    assert!(
        issue_args(&result, "unknown_import_field").is_some(),
        "{:?}",
        result.errors
    );
    let result = parse_la(r#"importa { ex = "norma:text", name = text }"#);
    assert!(
        issue_args(&result, "unknown_import_field").is_some(),
        "{:?}",
        result.errors
    );
}

#[test]
fn en_record_reports_a_missing_source_after_en_field_words() {
    let result = parse_en(r#"import { name = text }"#);
    assert!(
        issue_args(&result, "missing_import_source").is_some(),
        "{:?}",
        result.errors
    );
}
