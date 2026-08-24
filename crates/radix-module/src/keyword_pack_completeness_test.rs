//! Completeness test for grammar keyword rows in every installed reader pack.
//!
//! The lexer/parser own the `summa` expression's contextual heads by grammar
//! slot, so those identities are not yet part of the token registry. Keep
//! them in this gate alongside the registry rows until their lexer plumbing
//! is promoted to the shared keyword registry.

use super::LocalePack;
use crate::lexer::keyword_specs;
use std::collections::BTreeSet;
use std::path::PathBuf;

const INSTALLED_LOCALES: &[&str] = &["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"];
const CONTEXTUAL_SUMMA_KEYWORDS: &[&str] = &["summa", "filum"];

/// Load a pack's **owned** rows from the raw file (no `inherits` merge).
fn owned_installed_pack(locale: &str) -> LocalePack {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push(format!("../../stdlib/locale/{locale}/pack.toml"));
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
            if !pack.keywords.contains_key(key) {
                missing.push(format!("{locale}:{key}"));
            }
        }
    }

    assert!(missing.is_empty(), "installed packs are missing [keywords] rows: {missing:?}");
}
