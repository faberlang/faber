//! Completeness test for the en-locale intrinsic catalog (L2).
//!
//! Proves that `locale/en/pack.toml` maps **every** canonical
//! intrinsic name in the compiler registry (`INTRINSICS`) to an English
//! surface spelling, pins the four named example mappings from the delivery,
//! and proves that every installed non-la reader pack owns a translated row for
//! every registry canonical (only named technical identifiers stay Latin).
//!
//! The en pack is the complete catalog: the registry and the pack may only
//! drift together. A registry row added without an en row, or an en row whose
//! canonical key was renamed/removed from the registry, fails here.

use super::LocalePack;
use crate::intrinsics::{INTRINSICS, IntrinsicReceiver};
use radix_intrinsics::{SERMO_VIEW_METHODS, SermoViewKind};
use std::collections::BTreeSet;

/// Load a pack's **owned** rows from the raw file (no `inherits` merge).
///
/// WHY: the en pack inherits `la`; comparing merged packs would pass
/// vacuously for rows that belong to the parent. The completeness gate is
/// about the rows the en pack itself owns.
fn owned_installed_pack(locale: &str) -> LocalePack {
    let path = crate::locale::workspace_locale_pack_path(locale);
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
    let registry: BTreeSet<&str> = INTRINSICS
        .iter()
        .map(|entry| entry.faber_name)
        .chain(SERMO_VIEW_METHODS.iter().map(|entry| entry.faber_name))
        .collect();
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

/// Technical names en keeps as identity rows and the translated packs keep
/// too (algorithm and shader-domain identifiers with no settled native term).
/// Every other canonical must carry a translated surface in every non-la pack.
const KEPT_IDENTITY: &[&str] = &[
    "rms_norm",
    "rope_norm",
    "rope_half",
    "gelu",
    "silu",
    "formata",
    "swizzle",
    // Math built-ins keep the industry name (operator ruling 2026-10-27).
    "rank",
    "norm",
];

/// Every installed non-la reader pack owns a row for every registry canonical,
/// and only the named technical identifiers may keep the Latin spelling.
///
/// WHY: intrinsic lookup is pack-first for non-la packs, so a missing row makes
/// the member unusable in that locale (`SEM004 unknown_method`) and an identity
/// row leaks the Latin spelling into a native-language source (web5).
#[test]
fn locale_pack_completeness_intrinsics_cover_registry_in_every_pack() {
    let registry: BTreeSet<&str> = INTRINSICS.iter().map(|entry| entry.faber_name).collect();
    for locale in ["en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        let pack = owned_installed_pack(locale);
        let missing: Vec<&str> = registry
            .iter()
            .copied()
            .filter(|name| !pack.intrinsics.contains_key(*name))
            .collect();
        assert!(
            missing.is_empty(),
            "{locale} is missing [intrinsics] rows for {} registry canonicals: {missing:?}",
            missing.len()
        );
        if locale == "en" {
            continue;
        }
        let leaked: Vec<&str> = registry
            .iter()
            .copied()
            .filter(|name| pack.intrinsic_surface(name) == *name && !KEPT_IDENTITY.contains(name))
            .collect();
        assert!(
            leaked.is_empty(),
            "{locale} keeps the Latin spelling for {} canonicals: {leaked:?}",
            leaked.len()
        );
    }
}

/// Every registry row resolves from its pack surface back to its canonical in
/// the owning receiver family, in every non-la pack: no row is shadowed by
/// another canonical that shares the surface inside one family.
#[test]
fn locale_pack_completeness_intrinsic_surfaces_resolve_back_per_family() {
    for locale in ["en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        let pack = owned_installed_pack(locale);
        for entry in INTRINSICS {
            let surface = pack.intrinsic_surface(entry.faber_name);
            assert_eq!(
                pack.intrinsic_canonical(entry.receiver, surface),
                Some(entry.faber_name),
                "{locale}: `{surface}` must resolve to `{}` on {:?}",
                entry.faber_name,
                entry.receiver
            );
        }
    }
}

/// Every intrinsic surface is one identifier token after NFKC normalization,
/// as the lexer reads it; a spelling that is not would never match a call.
#[test]
fn locale_pack_completeness_intrinsic_surfaces_are_nfkc_identifiers() {
    use unicode_normalization::UnicodeNormalization;
    for locale in ["en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        let pack = owned_installed_pack(locale);
        for binding in pack.intrinsics.values() {
            for surface in std::iter::once(&binding.canonical).chain(binding.aliases.iter()) {
                let normalized: String = surface.nfkc().collect();
                assert_eq!(&normalized, surface, "{locale}: `{surface}` must be stored NFKC-normalized");
                let mut chars = surface.chars();
                let first = chars.next().expect("non-empty surface");
                assert!(
                    (unicode_ident::is_xid_start(first) || first == '_')
                        && chars.all(|c| unicode_ident::is_xid_continue(c) || c == '_'),
                    "{locale}: `{surface}` must lex as a single identifier"
                );
            }
        }
    }
}

/// The queue/stack method vocabulary is available in every installed reader
/// pack: en and the translated packs own localized rows; la is identity by
/// absence.
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
            let surface = pack.intrinsic_surface(method);
            match locale {
                "la" => assert_eq!(surface, method, "la keeps the canonical spelling"),
                "en" => assert_eq!(surface, english_surface, "en must expose {method}"),
                _ => {
                    assert!(pack.intrinsics.contains_key(method), "{locale} must own a row for {method}");
                    assert_ne!(surface, method, "{locale} must translate queue/stack method {method}");
                }
            }
        }
    }

    // la is identity-by-absence: forward projection keeps the canonical
    // spelling while reverse lookup has no explicit pack-owned row.
    let la = owned_installed_pack("la");
    assert_eq!(la.intrinsic_canonical(IntrinsicReceiver::Queue, "appende"), None);
    assert_eq!(la.intrinsic_canonical(IntrinsicReceiver::Stack, "detrahe"), None);
}

/// The directional sermo view methods (`SERMO_VIEW_METHODS`) are spelled by
/// every non-la pack, each spelling resolves back to its canonical on its
/// view, and no pack keeps the Latin spelling (en: `t.get()` / `t.finish()`).
#[test]
fn sermo_view_method_rows_are_owned_and_resolve_in_every_pack() {
    for locale in ["en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        let pack = owned_installed_pack(locale);
        for entry in SERMO_VIEW_METHODS {
            let surface = pack.intrinsic_surface(entry.faber_name);
            assert!(
                pack.intrinsics.contains_key(entry.faber_name),
                "{locale}: missing [intrinsics] row for view method `{}`",
                entry.faber_name
            );
            // `cursor` is an English word already, so en keeps the identity row.
            if !(locale == "en" && entry.faber_name == "cursor") {
                assert_ne!(surface, entry.faber_name, "{locale}: `{surface}` keeps the Latin spelling");
            }
            assert_eq!(
                pack.sermo_view_method_canonical(entry.view, surface),
                Some(entry.faber_name),
                "{locale}: `{surface}` must resolve to `{}` on {:?}",
                entry.faber_name,
                entry.view
            );
        }
        // The inbound view's four methods are pairwise distinct spellings.
        let tuus: BTreeSet<&str> = SERMO_VIEW_METHODS
            .iter()
            .filter(|entry| entry.view == SermoViewKind::Tuus)
            .map(|entry| pack.intrinsic_surface(entry.faber_name))
            .collect();
        assert_eq!(tuus.len(), 4, "{locale}: tuus view surfaces collide: {tuus:?}");
    }
}
