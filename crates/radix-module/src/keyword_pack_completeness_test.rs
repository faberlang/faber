//! Completeness test for grammar keyword rows in every installed reader pack.
//!
//! The lexer/parser own the `summa` expression's contextual heads by grammar
//! slot, so those identities are not yet part of the token registry. Keep
//! them in this gate alongside the registry rows until their lexer plumbing
//! is promoted to the shared keyword registry.

use super::LocalePack;
use crate::lexer::{keyword_specs, lex_with_locale_pack};
use crate::parser::{ParseErrorKind, parse};
use radix_lexer::KeywordScope;
use std::collections::BTreeSet;

const INSTALLED_LOCALES: &[&str] = &["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"];
const CONTEXTUAL_SUMMA_KEYWORDS: &[&str] = &["summa", "filum"];

// The registry retains `conversion` as an identity alias row
// (`KeywordScope::Alias`). The sealed English and Latin packs omit it, the six
// other packs localize it; `nihil` is the permanent `[types]` null-type word,
// so la omits it from `[keywords]` (D23.2). The sealed omissions are the one
// list in `locale_test.rs` (`SEALED_KEYWORD_ROW_OMISSIONS`), shared below.

/// Load a pack's **owned** rows from the raw file (no `inherits` merge).
fn owned_installed_pack(locale: &str) -> LocalePack {
    let path = crate::locale::workspace_locale_pack_path(locale);
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
        .filter(|spec| !matches!(spec.scope, KeywordScope::Alias { .. }))
        .map(|spec| spec.text)
        .chain(CONTEXTUAL_SUMMA_KEYWORDS.iter().copied())
        .collect();
    let mut missing = Vec::new();

    for &locale in INSTALLED_LOCALES {
        let pack = owned_installed_pack(locale);
        for &key in &required {
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

/// Per-section key-set drift between packs, naming both sets. `sections`
/// pairs a locale with the keys its pack owns for one table. A `[keywords]`
/// key a sealed pack intentionally omits is not drift.
fn key_set_drift(section: &str, sections: &[(&str, BTreeSet<String>)]) -> Vec<String> {
    let union: BTreeSet<&String> = sections.iter().flat_map(|(_, keys)| keys).collect();
    let mut drift = Vec::new();
    for (locale, keys) in sections {
        let missing: Vec<&&String> = union
            .iter()
            .filter(|key| !keys.contains(**key))
            .filter(|key| {
                !(section == "keywords"
                    && super::tests::is_sealed_keyword_row_omission(locale, "keyword", key.as_str()))
            })
            .collect();
        if !missing.is_empty() {
            drift.push(format!(
                "[{section}] {locale} lacks {missing:?} (carries {} of {} union keys)",
                keys.len(),
                union.len()
            ));
        }
    }
    drift
}

/// The eight installed packs carry one key set per table (modulo the sealed
/// keyword omissions), so the locale table
/// can join them on the Latin key. `[keywords]` and `[types]` are compared
/// across all eight; `[intrinsics]` across the seven non-la packs (la owns no
/// section: identity-by-absence, asserted by the intrinsic completeness gate).
/// Keys are read from the parsed binding maps, so a table-valued row such as
/// en `approximata = { canonical = "approx", .. }` counts like a string row.
#[test]
fn installed_packs_carry_one_key_set_per_section() {
    let packs: Vec<(&str, LocalePack)> = INSTALLED_LOCALES
        .iter()
        .map(|&locale| (locale, owned_installed_pack(locale)))
        .collect();
    let keys = |select: &dyn Fn(&LocalePack) -> BTreeSet<String>, skip_la: bool| {
        packs
            .iter()
            .filter(|(locale, _)| !(skip_la && *locale == "la"))
            .map(|(locale, pack)| (*locale, select(pack)))
            .collect::<Vec<_>>()
    };

    let mut drift = key_set_drift("keywords", &keys(&|p| p.keywords.keys().cloned().collect(), false));
    drift.extend(key_set_drift("types", &keys(&|p| p.types.keys().cloned().collect(), false)));
    drift.extend(key_set_drift(
        "intrinsics",
        &keys(&|p| p.intrinsics.keys().cloned().collect(), true),
    ));
    assert!(drift.is_empty(), "locale pack key-set drift: {drift:#?}");

    let en = &packs
        .iter()
        .find(|(locale, _)| *locale == "en")
        .expect("en pack")
        .1;
    assert!(
        en.intrinsics.contains_key("approximata"),
        "table-valued en `approximata` row must be counted by the parity check"
    );
}

#[test]
fn key_set_drift_names_the_pack_and_the_missing_keys() {
    let full: BTreeSet<String> = ["a", "b"].iter().map(|key| (*key).to_owned()).collect();
    let short: BTreeSet<String> = ["a"].iter().map(|key| (*key).to_owned()).collect();
    let drift = key_set_drift("keywords", &[("en", full), ("ar", short)]);
    assert_eq!(drift.len(), 1, "{drift:?}");
    assert!(drift[0].contains("ar lacks") && drift[0].contains("\"b\""), "{drift:?}");

    // Sealed omissions (en and la omit `conversion`) are not drift; a non-sealed
    // pack missing the same key still is.
    let with_conversion: BTreeSet<String> = ["a", "conversion"]
        .iter()
        .map(|key| (*key).to_owned())
        .collect();
    let without_conversion: BTreeSet<String> = ["a"].iter().map(|key| (*key).to_owned()).collect();
    let sealed = key_set_drift(
        "keywords",
        &[
            ("ar", with_conversion.clone()),
            ("en", without_conversion.clone()),
            ("la", without_conversion.clone()),
        ],
    );
    assert!(sealed.is_empty(), "{sealed:?}");
    let unsealed = key_set_drift("keywords", &[("ar", with_conversion), ("vi", without_conversion)]);
    assert_eq!(unsealed.len(), 1, "{unsealed:?}");
}
