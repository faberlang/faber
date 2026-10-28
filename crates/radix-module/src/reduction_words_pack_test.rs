//! Op-word pins for the `reducta via Op ex source` surface (K3-1).
//!
//! The ten op words are `[keywords]` rows whose value is the pack's method
//! spelling. A word that has an `[intrinsics]` row must read the same in both
//! sections; `maxima`, `minima`, `factum`, `numeratio` and `prima` have no
//! equal intrinsic row (the list method `prima` is `take`, a different word),
//! so their keyword rows are the only spelling. The
//! `locale_pack_completeness` prefix puts the test under
//! `scripta/check-reader-pack-completeness`.

use crate::locale::LocalePack;
use std::collections::BTreeMap;

const INSTALLED_LOCALES: &[&str] = &["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"];

/// The ten op words, by Latin key.
const OP_KEYS: &[&str] = &[
    "summa",
    "factum",
    "maxima",
    "minima",
    "argmaxima",
    "argminima",
    "omnia",
    "quilibet",
    "numeratio",
    "prima",
];

/// Keys whose `[keywords]` row must equal the `[intrinsics]` row in every
/// non-la pack.
const METHOD_EQUAL_KEYS: &[&str] = &["summa", "omnia", "quilibet", "argmaxima", "argminima", "reducta"];

/// Load a pack's owned rows from the raw file (no `inherits` merge).
fn owned_installed_pack(locale: &str) -> LocalePack {
    let path = crate::locale::workspace_locale_pack_path(locale);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read installed {locale} pack: {err:?}"));
    LocalePack::from_toml_str(&source)
        .unwrap_or_else(|err| panic!("installed {locale} pack should parse owned rows: {err:?}"))
}

fn keyword<'a>(pack: &'a LocalePack, locale: &str, key: &str) -> &'a str {
    pack.keywords
        .get(key)
        .unwrap_or_else(|| panic!("{locale} [keywords] is missing `{key}`"))
        .canonical
        .as_str()
}

#[test]
fn locale_pack_completeness_reduction_op_words_equal_method_spellings() {
    for &locale in INSTALLED_LOCALES.iter().filter(|&&locale| locale != "la") {
        let pack = owned_installed_pack(locale);
        for &key in METHOD_EQUAL_KEYS {
            let method = pack
                .intrinsics
                .get(key)
                .unwrap_or_else(|| panic!("{locale} [intrinsics] is missing `{key}`"))
                .canonical
                .as_str();
            assert_eq!(
                keyword(&pack, locale, key),
                method,
                "{locale}: [keywords] `{key}` must equal the method spelling"
            );
        }
    }
}

#[test]
fn locale_pack_completeness_reduction_op_words_are_pairwise_distinct() {
    for &locale in INSTALLED_LOCALES {
        let pack = owned_installed_pack(locale);
        let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
        for &key in OP_KEYS {
            let spelling = keyword(&pack, locale, key);
            if let Some(other) = seen.insert(spelling, key) {
                panic!("{locale}: op words `{other}` and `{key}` share the spelling `{spelling}`");
            }
        }
    }
}

#[test]
fn locale_pack_completeness_reduction_words_are_identity_in_la() {
    let pack = owned_installed_pack("la");
    for &key in OP_KEYS.iter().chain(["reducta", "pro"].iter()) {
        assert_eq!(keyword(&pack, "la", key), key, "la `{key}` must be its own spelling");
    }
}

#[test]
fn locale_pack_completeness_reduction_head_and_group_rows_exist_in_every_pack() {
    for &locale in INSTALLED_LOCALES {
        let pack = owned_installed_pack(locale);
        for key in ["reducta", "pro"] {
            keyword(&pack, locale, key);
        }
    }
}
