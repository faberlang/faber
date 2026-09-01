//! Completeness test for grammar keyword rows in every installed reader pack.
//!
//! The lexer/parser own the `summa` expression's contextual heads by grammar
//! slot, so those identities are not yet part of the token registry. Keep
//! them in this gate alongside the registry rows until their lexer plumbing
//! is promoted to the shared keyword registry.

use super::LocalePack;
use crate::lexer::{keyword_specs, lex_with_locale_pack};
use crate::parser::{ParseErrorKind, parse};
use std::collections::BTreeSet;

const INSTALLED_LOCALES: &[&str] = &["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"];
const CONTEXTUAL_SUMMA_KEYWORDS: &[&str] = &["summa", "filum"];

/// The conversion annotation has one sealed source spelling per settled
/// reader surface. The registry retains `conversion` as an identity alias so
/// other packs and tooling can name the family, but these two installed packs
/// must not declare that alias as source syntax.
const SEALED_CONVERSIO_ALIAS_OMISSIONS: &[(&str, &str)] = &[("en", "conversion"), ("la", "conversion")];

fn is_sealed_conversio_alias_omission(locale: &str, key: &str) -> bool {
    SEALED_CONVERSIO_ALIAS_OMISSIONS
        .iter()
        .any(|&(sealed_locale, omitted_key)| sealed_locale == locale && omitted_key == key)
}

/// Load a pack's **owned** rows from the raw file (no `inherits` merge).
fn owned_installed_pack(locale: &str) -> LocalePack {
    let path = crate::locale::workspace_locale_pack_path(&locale);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read installed {locale} pack: {err:?}"));
    LocalePack::from_toml_str(&source)
        .unwrap_or_else(|err| panic!("installed {locale} pack should parse owned rows: {err:?}"))
}

/// Every parser-owned grammar keyword identity has a row in every installed
/// pack. This is deliberately checked on owned rows so inheritance cannot
/// hide a missing localized surface.
#[test]
fn every_installed_pack_covers_grammar_keyword_rows() {
    let required: BTreeSet<&str> = keyword_specs()
        .iter()
        .map(|spec| spec.text)
        .chain(CONTEXTUAL_SUMMA_KEYWORDS.iter().copied())
        .collect();
    let mut missing = Vec::new();

    for &locale in INSTALLED_LOCALES {
        let pack = owned_installed_pack(locale);
        for &key in &required {
            if is_sealed_conversio_alias_omission(locale, key) {
                continue;
            }
            if !pack.keywords.contains_key(key) {
                missing.push(format!("{locale}:{key}"));
            }
        }
    }

    assert!(missing.is_empty(), "installed packs are missing [keywords] rows: {missing:?}");
}

#[test]
fn english_convert_probe_is_rejected_by_the_installed_pack() {
    let pack = owned_installed_pack("en");
    let result = parse(lex_with_locale_pack("union Dest { @ convert Source {} Variant }", &pack));
    assert!(
        !result.success(),
        "English @ convert must not register a third Conversio arm: {:?}",
        result.errors
    );
}

#[test]
fn latin_conversion_probe_is_parse018_with_the_installed_pack() {
    let pack = owned_installed_pack("la");
    // RED (F1): d2dd1c871 declared the English alias in the Latin pack.
    assert!(
        !pack.keywords.contains_key("conversion"),
        "la must not declare the cross-locale `conversion` alias"
    );
    let result = parse(lex_with_locale_pack("discretio Dest { @ conversion Source {} Variant }", &pack));
    assert!(
        result.errors.iter().any(|error| {
            error.kind == ParseErrorKind::AnnotationInvalid
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "wrong_locale_annotation")
        }),
        "Latin @ conversion must produce PARSE018 facts: {:?}",
        result.errors
    );
}

#[test]
fn conversion_annotation_rows_follow_the_sealed_locale_surface() {
    for &(locale, expected_key, expected_surface) in
        &[("en", "conversio", "conversion"), ("la", "conversio", "conversio")]
    {
        let pack = owned_installed_pack(locale);
        assert_eq!(
            pack.keywords
                .get(expected_key)
                .map(|binding| binding.canonical.as_str()),
            Some(expected_surface),
            "{locale} must retain its sealed Conversio spelling {expected_surface:?}"
        );
        assert!(
            !pack.keywords.contains_key("conversion"),
            "{locale} must not declare the cross-locale `conversion` alias"
        );
    }
}
