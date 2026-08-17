//! Completeness test for the en-locale intrinsic catalog (L2).
//!
//! Proves that `stdlib/locale/en/pack.toml` maps **every** canonical
//! intrinsic name in the compiler registry (`INTRINSICS`) to an English
//! surface spelling, pins the four named example mappings from the delivery,
//! and keeps the `la` pack identity-by-absence (no `[intrinsics]` rows).
//!
//! The en pack is the complete catalog: the registry and the pack may only
//! drift together. A registry row added without an en row, or an en row whose
//! canonical key was renamed/removed from the registry, fails here.

use super::LocalePack;
use crate::intrinsics::{IntrinsicReceiver, INTRINSICS};
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
    assert_eq!(en.intrinsic_surface("continet"), Some("contains"));
    assert_eq!(en.intrinsic_surface("longitudo"), Some("length"));
    assert_eq!(en.intrinsic_surface("sectio"), Some("slice"));
    assert_eq!(en.intrinsic_surface("appende"), Some("append"));
}

/// The la pack is untouched: still identity-by-absence for intrinsics.
#[test]
fn la_pack_stays_identity_by_absence() {
    let la = owned_installed_pack("la");
    assert!(la.intrinsics.is_empty(), "la pack must not own [intrinsics] rows");
    assert!(la.intrinsic_surface("continet").is_none());
    assert_eq!(la.intrinsic_canonical(IntrinsicReceiver::Lista, "contains"), None);
}
