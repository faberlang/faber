//! Tests for the reader-locale intrinsic-name projection (L1).
//!
//! Proves: `[intrinsics]` section parsing (canonical → surface rows), the
//! surface → canonical reverse lookup scoped by receiver family, family-local
//! surface-collision rejection, unknown-canonical rejection, alias-row
//! participation, inheritance merging, and identity-by-absence for packs
//! without an `[intrinsics]` section.

use super::{
    latin_locale_pack, validate_locale_pack, IntrinsicBinding, LocalePack, LocalePackIssue, LocalePackLoadError,
};
use crate::intrinsics::IntrinsicReceiver;
use std::path::PathBuf;

fn owned_installed_pack(locale: &str) -> LocalePack {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push(format!("../../stdlib/locale/{locale}/pack.toml"));
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read installed {locale} pack: {err:?}"));
    LocalePack::from_toml_str(&source)
        .unwrap_or_else(|err| panic!("installed {locale} pack should parse owned rows: {err:?}"))
}

fn assert_validation_issue(err: LocalePackLoadError, predicate: impl Fn(&LocalePackIssue) -> bool) {
    let LocalePackLoadError::Validation(issues) = err else {
        panic!("expected validation errors, got {err:?}");
    };
    assert!(
        issues.iter().any(predicate),
        "expected a matching validation issue, got {issues:?}"
    );
}

/// A minimal pack that passes the full validation surface, ready for
/// `[intrinsics]` rows to be appended.
const PACK_PREAMBLE: &str = r#"
[pack]
id = "test"
fallback = ["la"]

[llm]
system_prompt_snippet = "emit test reader-locale Faber"
exemplars = ["salve-munde.test.fab"]
"#;

#[test]
fn toml_pack_parses_intrinsic_rows_and_resolves_both_directions() {
    let pack = LocalePack::from_toml_str(&format!(
        r#"
{PACK_PREAMBLE}
[intrinsics]
continet = "contains"
appende = "append"
accipe = "get"
"#
    ))
    .expect("intrinsic rows should validate");

    // Canonical → surface (emit direction).
    assert_eq!(
        pack.intrinsics["continet"],
        IntrinsicBinding { key: "continet".to_owned(), canonical: "contains".to_owned(), aliases: Vec::new() }
    );
    assert_eq!(pack.intrinsic_surface("continet"), Some("contains"));
    assert_eq!(pack.intrinsic_surface("appende"), Some("append"));
    assert_eq!(pack.intrinsic_surface("accipe"), Some("get"));

    // Surface → canonical, scoped by receiver family. `continet` is owned by
    // Lista/Textus/Intervallum; a family that does not own it has no mapping.
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Lista, "contains"), Some("continet"));
    assert_eq!(
        pack.intrinsic_canonical(IntrinsicReceiver::Textus, "contains"),
        Some("continet")
    );
    assert_eq!(
        pack.intrinsic_canonical(IntrinsicReceiver::Intervallum, "contains"),
        Some("continet")
    );
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Tabula, "contains"), None);
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Copia, "contains"), None);

    // `accipe` is owned by five families; Copia still has no mapping.
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Sparsa, "get"), Some("accipe"));
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Tensor, "get"), Some("accipe"));
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Copia, "get"), None);

    // A canonical with no row has no explicit surface (identity by absence).
    assert_eq!(pack.intrinsic_surface("longitudo"), None);
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Lista, "length"), None);
}

#[test]
fn duplicate_intrinsic_surfaces_within_family_are_rejected() {
    // `continet` and `summa` are both Lista canonicals; mapping both to
    // "contains" is ambiguous for Lista receivers and must reject the pack.
    let err = LocalePack::from_toml_str(&format!(
        r#"
{PACK_PREAMBLE}
[intrinsics]
continet = "contains"
summa = "contains"
"#
    ))
    .expect_err("duplicate family surface should fail validation");
    assert_validation_issue(err, |issue| {
        matches!(
            issue,
            LocalePackIssue::DuplicateIntrinsicSurface { receiver: IntrinsicReceiver::Lista, surface }
                if surface == "contains"
        )
    });
}

#[test]
fn same_surface_in_disjoint_families_is_allowed() {
    // `continet` (Lista/Textus/Intervallum) and `habet` (Tabula/Copia) share
    // no receiver family, so one surface spelling is unambiguous per family.
    let pack = LocalePack::from_toml_str(&format!(
        r#"
{PACK_PREAMBLE}
[intrinsics]
continet = "contains"
habet = "contains"
"#
    ))
    .expect("disjoint-family surfaces should validate");
    assert!(validate_locale_pack(&pack).is_empty());
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Lista, "contains"), Some("continet"));
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Tabula, "contains"), Some("habet"));
}

#[test]
fn unknown_intrinsic_canonical_keys_are_rejected() {
    let err = LocalePack::from_toml_str(&format!(
        r#"
{PACK_PREAMBLE}
[intrinsics]
continet = "contains"
longitodo = "length"
"#
    ))
    .expect_err("unknown canonical key should fail validation");
    assert_validation_issue(
        err,
        |issue| matches!(issue, LocalePackIssue::UnknownIntrinsic { key } if key == "longitodo"),
    );
}

#[test]
fn intrinsic_alias_rows_resolve_and_participate_in_collision_scope() {
    let pack = LocalePack::from_toml_str(&format!(
        r#"
{PACK_PREAMBLE}
[intrinsics]
sectio = {{ canonical = "slice", aliases = ["sliced"] }}
"#
    ))
    .expect("alias row should validate");

    assert_eq!(pack.intrinsics["sectio"].aliases, vec!["sliced"]);
    assert_eq!(pack.intrinsic_surface("sectio"), Some("slice"));
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Lista, "sliced"), Some("sectio"));

    // An alias colliding with another canonical's surface in a shared family
    // is rejected the same way a plain surface collision is.
    let err = LocalePack::from_toml_str(&format!(
        r#"
{PACK_PREAMBLE}
[intrinsics]
sectio = {{ canonical = "slice", aliases = ["cut"] }}
recide = "cut"
"#
    ))
    .expect_err("alias collision in shared family should fail validation");
    assert_validation_issue(err, |issue| {
        matches!(
            issue,
            LocalePackIssue::DuplicateIntrinsicSurface { receiver: IntrinsicReceiver::Textus, surface }
                if surface == "cut"
        )
    });
}

#[test]
fn packs_without_intrinsics_section_load_unchanged() {
    // The la pack (and every other installed pack in this wave) owns zero
    // `[intrinsics]` rows: the absent section must parse to an empty table,
    // validate, and expose no explicit surface mappings. (The en pack now
    // owns a full intrinsic catalog — its completeness is proven by
    // `intrinsic_pack_completeness_test.rs`, L2.)
    let la = owned_installed_pack("la");
    assert!(la.intrinsics.is_empty(), "installed la pack should own no [intrinsics] rows");
    assert!(validate_locale_pack(&la).is_empty(), "installed la pack should validate");
    assert_eq!(la.intrinsic_surface("continet"), None);
    assert_eq!(la.intrinsic_canonical(IntrinsicReceiver::Lista, "contains"), None);

    // The generated Latin pack is identity-by-absence for every canonical.
    let latin = latin_locale_pack();
    assert!(latin.intrinsics.is_empty());
    assert_eq!(latin.intrinsic_surface("longitudo"), None);
    assert_eq!(latin.intrinsic_canonical(IntrinsicReceiver::Lista, "length"), None);
}

#[test]
fn inherited_pack_merges_intrinsic_rows() {
    let dir = crate::test_support::test_temp_dir("intrinsic-inherit");
    let root = dir.path().to_path_buf();
    let parent_dir = root.join("parent");
    let child_dir = root.join("child");
    std::fs::create_dir_all(&parent_dir).expect("create parent");
    std::fs::create_dir_all(&child_dir).expect("create child");
    std::fs::write(
        parent_dir.join("pack.toml"),
        r#"
[pack]
id = "parent"
fallback = ["la"]

[intrinsics]
continet = "contains"

[llm]
system_prompt_snippet = "emit parent"
exemplars = ["./salve.fab"]
"#,
    )
    .expect("write parent");
    std::fs::write(parent_dir.join("salve.fab"), "parent salve {}").expect("write parent exemplar");
    std::fs::write(
        child_dir.join("pack.toml"),
        r#"
[pack]
id = "child"
inherits = "parent"
fallback = ["la"]

[intrinsics]
sectio = "slice"

[llm]
system_prompt_snippet = "emit child"
exemplars = ["./salve.fab"]
"#,
    )
    .expect("write child");
    std::fs::write(child_dir.join("salve.fab"), "child salve {}").expect("write child exemplar");

    let pack = LocalePack::from_toml_path(child_dir.join("pack.toml")).expect("load child pack");

    assert_eq!(pack.intrinsics["continet"].canonical, "contains");
    assert_eq!(pack.intrinsics["sectio"].canonical, "slice");
    assert_eq!(pack.intrinsic_surface("continet"), Some("contains"));
    assert_eq!(pack.intrinsic_canonical(IntrinsicReceiver::Lista, "slice"), Some("sectio"));
}
