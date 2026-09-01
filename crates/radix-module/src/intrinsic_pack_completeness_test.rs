//! Completeness test for the en-locale intrinsic catalog (L2).
//!
//! Proves that `stdlib/locale/en/pack.toml` maps **every** canonical
//! intrinsic name in the compiler registry (`INTRINSICS`) to an English
//! surface spelling, pins the four named example mappings from the delivery,
//! and proves that every installed reader pack exposes the queue/stack method
//! names, either through a localized row or the canonical identity fallback.
//!
//! The en pack is the complete catalog: the registry and the pack may only
//! drift together. A registry row added without an en row, or an en row whose
//! canonical key was renamed/removed from the registry, fails here.

use super::LocalePack;
use crate::intrinsics::{INTRINSICS, IntrinsicReceiver};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Load a pack's **owned** rows from the raw file (no `inherits` merge).
///
/// WHY: the en pack inherits `la`; comparing merged packs would pass
/// vacuously for rows that belong to the parent. The completeness gate is
/// about the rows the en pack itself owns.
fn owned_installed_pack(locale: &str) -> LocalePack {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push(format!("../../stdlib/locale/{locale}/pack.toml"));
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read installed {locale} pack: {err:?}"));
    LocalePack::from_toml_str(&source)
        .unwrap_or_else(|err| panic!("installed {locale} pack should parse owned rows: {err:?}"))
}

/// Every canonical `faber_name` in the registry has an en row. Identity rows
/// (surface == canonical) are allowed where the English spelling equals the
/// canonical; the requirement is presence, not a changed spelling.
#[test]
fn en_pack_covers_every_registry_intrinsic() {
    let en = owned_installed_pack("en");
    let missing: Vec<&str> = INTRINSICS
        .iter()
        .filter(|entry| !en.intrinsics.contains_key(entry.faber_name))
        .map(|entry| entry.faber_name)
        .collect();
    assert!(
        missing.is_empty(),
        "en pack is missing [intrinsics] rows for {} registry canonicals: {missing:?}",
        missing.len()
    );
}

/// The en `[intrinsics]` keys and the registry canonical set are exactly the
/// same set. This is stronger than a zero-missing check: it also fails when
/// an en row names a canonical the registry no longer owns (a stale row that
/// would silently keep an orphan surface after a rename).
#[test]
fn en_pack_intrinsic_keys_match_registry_exactly() {
    let en = owned_installed_pack("en");
    let registry: BTreeSet<&str> = INTRINSICS.iter().map(|entry| entry.faber_name).collect();
    let pack_keys: BTreeSet<&str> = en.intrinsics.keys().map(String::as_str).collect();
    assert_eq!(
        pack_keys, registry,
        "en [intrinsics] keys must equal the registry canonical set"
    );
}

/// The four named example mappings from the delivery spec map exactly.
#[test]
fn en_pack_maps_the_four_named_examples() {
    let en = owned_installed_pack("en");
    assert_eq!(en.intrinsic_surface("continet"), "contains");
    assert_eq!(en.intrinsic_surface("longitudo"), "length");
    assert_eq!(en.intrinsic_surface("sectio"), "slice");
    assert_eq!(en.intrinsic_surface("appende"), "append");
}

/// The queue/stack method vocabulary is available in every installed reader
/// pack. English uses localized rows; the other packs retain the canonical
/// Latin spelling, either through an explicit identity row or by absence.
#[test]
fn queue_stack_method_rows_are_owned_by_all_installed_packs() {
    for locale in ["la", "en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        let pack = owned_installed_pack(locale);
        for (method, english_surface) in [
            ("appende", "append"),
            ("decapita", "remove_first"),
            ("detrahe", "remove_last"),
            ("longitudo", "length"),
        ] {
            let expected = if locale == "en" { english_surface } else { method };
            assert_eq!(
                pack.intrinsic_surface(method),
                expected,
                "{locale} must expose queue/stack method surface {method}"
            );
        }
    }

    // la is identity-by-absence: forward projection keeps the canonical
    // spelling while reverse lookup has no explicit pack-owned row.
    let la = owned_installed_pack("la");
    assert_eq!(la.intrinsic_canonical(IntrinsicReceiver::Queue, "appende"), None);
    assert_eq!(la.intrinsic_canonical(IntrinsicReceiver::Stack, "detrahe"), None);
}
