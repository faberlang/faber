use crate::explain::{render_list, render_plain, Lookup, Registry};
use crate::reference::{
    pack_version_skew, parse_release_version, resolve_reference_root, resolve_reference_root_in,
    PackMetadata, ReferenceLayout, ReferencePack, ResolvedTerm, REFERENCE_ROOT_ENV,
};
use crate::reference_pack_test_support::{env_lock, repo_exempla_root};
use crate::reference_parse::entry_from_exempla;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

/// Unique scratch dir under the platform temp root (never inside a workspace).
fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let serial = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("{label}-{nonce}-{serial}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Minimal valid corpus index.toml that would satisfy the old dev walk-up.
const FAKE_INDEX: &str = "generated_on = \"2026-08-07\"\nfab_count = 0\nregistry_terms = 0\nterms = []\n";

#[test]
fn release_mode_fails_closed_beside_stray_sibling_checkout() {
    // E5 regression: a hermetic directory containing a stray `radix/corpus`
    // checkout must NOT satisfy an installed (release-shaped) binary.
    let hermetic = temp_dir("faber-e5-stray");
    let stray = hermetic.join("radix/corpus");
    fs::create_dir_all(&stray).expect("stray corpus dir");
    fs::write(stray.join("index.toml"), FAKE_INDEX).expect("stray index");
    let exe = hermetic.join("bin/faber");
    fs::create_dir_all(exe.parent().expect("bin dir")).expect("bin dir");

    let err = resolve_reference_root_in(Some(&exe), Some(&hermetic), false)
        .expect_err("installed binary must fail closed");
    assert!(err.message.contains("reference pack not found"), "{}", err.message);
    assert!(
        err.message.contains("share/faber/reference"),
        "error must name the install location: {}",
        err.message
    );
    let _ = fs::remove_dir_all(&hermetic);
}

#[test]
fn release_mode_fails_closed_in_hermetic_dir_without_packs() {
    let hermetic = temp_dir("faber-e5-empty");
    let exe = hermetic.join("bin/faber");
    fs::create_dir_all(exe.parent().expect("bin dir")).expect("bin dir");

    let err = resolve_reference_root_in(Some(&exe), Some(&hermetic), false)
        .expect_err("installed binary must fail closed");
    assert!(err.message.contains("reference pack not found"), "{}", err.message);
    let _ = fs::remove_dir_all(&hermetic);
}

#[test]
fn install_sibling_resolves_share_faber_reference() {
    let prefix = temp_dir("faber-install-share");
    let root = prefix.join("share/faber/reference");
    fs::create_dir_all(&root).expect("pack root");
    fs::write(root.join("index.toml"), FAKE_INDEX).expect("index");
    let exe = prefix.join("bin/faber");
    fs::create_dir_all(exe.parent().expect("bin dir")).expect("bin dir");

    let resolved = resolve_reference_root_in(Some(&exe), Some(&prefix), false)
        .expect("install-sibling resolution");
    assert_eq!(
        resolved,
        root.canonicalize().unwrap_or(root.clone()),
        "resolved must match the canonicalized pack root"
    );
    let _ = fs::remove_dir_all(&prefix);
}

#[test]
fn install_sibling_resolves_lib_faber_reference() {
    let prefix = temp_dir("faber-install-lib");
    let root = prefix.join("lib/faber/reference");
    fs::create_dir_all(&root).expect("pack root");
    fs::write(root.join("index.toml"), FAKE_INDEX).expect("index");
    let exe = prefix.join("bin/faber");
    fs::create_dir_all(exe.parent().expect("bin dir")).expect("bin dir");

    let resolved = resolve_reference_root_in(Some(&exe), Some(&prefix), false)
        .expect("install-sibling resolution");
    assert_eq!(
        resolved,
        root.canonicalize().unwrap_or(root.clone()),
        "resolved must match the canonicalized pack root"
    );
    let _ = fs::remove_dir_all(&prefix);
}

#[test]
fn dev_walkup_resolves_sibling_radix_tree() {
    // Development builds may walk up from the cwd to a sibling radix checkout.
    let work = temp_dir("faber-dev-tree");
    let faber_dir = work.join("faber");
    fs::create_dir_all(&faber_dir).expect("faber dir");
    let corpus = work.join("radix/corpus");
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(corpus.join("index.toml"), FAKE_INDEX).expect("index");

    let resolved = resolve_reference_root_in(None, Some(&faber_dir), true)
        .expect("dev walk-up resolves sibling radix");
    assert_eq!(resolved, corpus);
    let _ = fs::remove_dir_all(&work);
}

#[test]
fn dev_walkup_does_not_resolve_when_disabled() {
    let work = temp_dir("faber-dev-disabled");
    let faber_dir = work.join("faber");
    fs::create_dir_all(&faber_dir).expect("faber dir");
    let corpus = work.join("radix/corpus");
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(corpus.join("index.toml"), FAKE_INDEX).expect("index");

    // A release-shaped binary does not get the dev walk-up even when a sibling
    // checkout exists above the hermetic cwd (E5).
    let exe = work.join("faber/bin/faber");
    let err = resolve_reference_root_in(Some(&exe), Some(&faber_dir), false)
        .expect_err("no dev walk-up for installed binaries");
    assert!(err.message.contains("reference pack not found"), "{}", err.message);
    let _ = fs::remove_dir_all(&work);
}

#[test]
fn dev_fallback_loads_repo_exempla_index() {
    let _guard = env_lock();
    let previous = std::env::var(REFERENCE_ROOT_ENV).ok();
    std::env::remove_var(REFERENCE_ROOT_ENV);

    let pack = ReferencePack::load().expect("dev fallback loads repo exempla");
    assert_eq!(pack.term_count(), 185);
    assert_eq!(pack.metadata().registry_terms, 185);
    assert_eq!(pack.layout(), ReferenceLayout::Repo);
    assert!(pack.term("functio").is_some());
    assert!(pack.term("≡").is_some());

    if let Some(value) = previous {
        std::env::set_var(REFERENCE_ROOT_ENV, value);
    }
}

#[test]
fn load_from_repo_root_lists_canonical_terms() {
    let root = repo_exempla_root();
    let pack = ReferencePack::load_from(&root).expect("load repo exempla");
    assert_eq!(pack.term_count(), 185);
    assert_eq!(pack.legacy_redirects().len(), 14);
    assert!(pack.resolve_exempla_path("functio/functio.fab").is_file());
}

#[test]
fn env_override_wins_over_dev_fallback() {
    let _guard = env_lock();
    let repo = repo_exempla_root();
    let Some(workspace_root) = repo.ancestors().find(|dir| {
        dir.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "faber" || name == "faber-forma")
    }) else {
        eprintln!("skip env_override_wins_over_dev_fallback: workspace root not found");
        return;
    };
    let pack_root = workspace_root.join("target/faber-reference-ci");

    if !pack_root.join("index.toml").is_file() {
        eprintln!(
            "skip env_override_wins_over_dev_fallback: missing {}",
            pack_root.display()
        );
        return;
    }

    let previous = std::env::var(REFERENCE_ROOT_ENV).ok();
    std::env::set_var(REFERENCE_ROOT_ENV, &pack_root);

    let resolved = resolve_reference_root().expect("env override resolves");
    assert_eq!(
        resolved.canonicalize().unwrap_or(resolved),
        pack_root.canonicalize().unwrap_or(pack_root)
    );

    let pack = ReferencePack::load().expect("load overridden pack");
    assert_eq!(pack.layout(), ReferenceLayout::Pack);
    assert!(pack.resolve_exempla_path("functio/functio.fab").is_file());
    assert_eq!(pack.metadata().faber_version.as_deref(), Some("ci"));

    std::env::remove_var(REFERENCE_ROOT_ENV);
    if let Some(value) = previous {
        std::env::set_var(REFERENCE_ROOT_ENV, value);
    }
}

#[test]
fn missing_root_reports_actionable_error() {
    let _guard = env_lock();
    let previous = std::env::var(REFERENCE_ROOT_ENV).ok();
    std::env::set_var(REFERENCE_ROOT_ENV, "/tmp/faber-reference-pack-missing-test");

    let err = ReferencePack::load().expect_err("missing pack fails");
    assert!(err.message.contains("index.toml"));
    assert!(err.message.contains(REFERENCE_ROOT_ENV));

    std::env::remove_var(REFERENCE_ROOT_ENV);
    if let Some(value) = previous {
        std::env::set_var(REFERENCE_ROOT_ENV, value);
    }
}

#[test]
fn build_registry_loads_canonical_and_legacy_entries() {
    let pack = ReferencePack::load_from(repo_exempla_root()).expect("load pack");
    let registry = pack.build_registry().expect("build registry");
    assert_eq!(registry.entries().len(), 199);
    assert!(registry.reference_root().is_some());
    assert!(registry.lookup("functio").is_some());
    assert!(matches!(registry.lookup("=="), Some(Lookup::Legacy { .. })));
}

#[test]
fn load_from_disk_builds_explain_registry() {
    let _guard = env_lock();
    let previous = std::env::var(REFERENCE_ROOT_ENV).ok();
    std::env::remove_var(REFERENCE_ROOT_ENV);

    let registry = Registry::load_from_disk().expect("disk registry");
    assert_eq!(registry.entries().len(), 199);
    let list = render_list(&registry);
    assert!(list.contains("reference:"));

    if let Some(value) = previous {
        std::env::set_var(REFERENCE_ROOT_ENV, value);
    }
}

#[test]
fn parse_exempla_entry_for_functio_keyword() {
    let root = repo_exempla_root();
    let path = root.join("functio/functio.fab");
    let source = std::fs::read_to_string(&path).expect("read exempla");
    let entry = entry_from_exempla("functio/functio.fab", &source, "functio", "keyword")
        .expect("parse entry");
    assert_eq!(entry.term, "functio");
    assert!(entry.body.contains("```fab"));
}

#[test]
fn parse_exempla_entry_for_equivalence_operator() {
    let root = repo_exempla_root();
    let path = root.join("operatores/comparatio.fab");
    let source = std::fs::read_to_string(&path).expect("read exempla");
    let entry = entry_from_exempla("operatores/comparatio.fab", &source, "≡", "operator-group")
        .expect("parse entry");
    assert_eq!(entry.term, "≡");
    assert!(entry.body.contains("```fab"));
}

#[test]
fn parse_exempla_entry_for_manifest_concept() {
    let root = repo_exempla_root();
    let path = root.join("meta/manifest.fab");
    let source = std::fs::read_to_string(&path).expect("read exempla");
    let entry = entry_from_exempla("meta/manifest.fab", &source, "manifest", "concept")
        .expect("parse entry");
    assert_eq!(entry.term, "manifest");
    assert!(entry.body.contains("```fab"));
}

#[test]
fn disk_render_plain_includes_short_contract_for_functio() {
    let _guard = env_lock();
    let previous = std::env::var(REFERENCE_ROOT_ENV).ok();
    std::env::remove_var(REFERENCE_ROOT_ENV);

    let disk = Registry::load_from_disk().expect("disk");
    let lookup = disk.lookup("functio").expect("functio");
    let rendered = render_plain(&lookup);
    assert!(rendered.contains("NAME"));
    assert!(rendered.contains("SYNTAX"));
    assert!(rendered.contains("functio <name>"));

    if let Some(value) = previous {
        std::env::set_var(REFERENCE_ROOT_ENV, value);
    }
}

#[test]
fn pack_version_skew_errors_on_major_mismatch() {
    let (major, _minor, _patch) =
        parse_release_version(env!("CARGO_PKG_VERSION")).expect("release version");
    let pack_version = format!("{}.0.0", major + 1);
    let metadata = PackMetadata {
        faber_version: Some(pack_version),
        generated_on: None,
        fab_count: 0,
        registry_terms: 0,
        source_commit: None,
        index_generated_on: None,
    };
    let err = pack_version_skew(&metadata).expect_err("major mismatch");
    assert!(err.message.contains("major version mismatch"));
}

#[test]
fn pack_version_skew_warns_on_minor_patch_drift() {
    let (major, minor, _patch) =
        parse_release_version(env!("CARGO_PKG_VERSION")).expect("release version");
    let pack_version = format!("{major}.{}.0", minor + 1);
    let metadata = PackMetadata {
        faber_version: Some(pack_version.clone()),
        generated_on: None,
        fab_count: 0,
        registry_terms: 0,
        source_commit: None,
        index_generated_on: None,
    };
    let warning = pack_version_skew(&metadata)
        .expect("minor drift warns")
        .expect("warning message");
    assert!(warning.contains(&pack_version));
    assert!(warning.contains("differs"));
}

#[test]
fn pack_version_skew_ignores_non_release_pack_versions() {
    let metadata = PackMetadata {
        faber_version: Some("ci".to_owned()),
        generated_on: None,
        fab_count: 0,
        registry_terms: 0,
        source_commit: None,
        index_generated_on: None,
    };
    assert!(pack_version_skew(&metadata).expect("ci pack").is_none());
}

#[test]
fn pack_version_skew_accepts_matching_release_version() {
    let metadata = PackMetadata {
        faber_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        generated_on: None,
        fab_count: 0,
        registry_terms: 0,
        source_commit: None,
        index_generated_on: None,
    };
    assert!(pack_version_skew(&metadata)
        .expect("matching version")
        .is_none());
}

#[test]
fn pack_version_skew_no_version_is_ok() {
    let metadata = PackMetadata {
        faber_version: None,
        generated_on: None,
        fab_count: 0,
        registry_terms: 0,
        source_commit: None,
        index_generated_on: None,
    };
    assert!(pack_version_skew(&metadata).expect("no version").is_none());
}

#[test]
fn release_version_parser_accepts_prerelease() {
    assert_eq!(parse_release_version("1.0.0-rc.1"), Some((1, 0, 0)));
}

#[test]
fn release_version_parser_accepts_build_metadata() {
    assert_eq!(parse_release_version("1.0.0+local"), Some((1, 0, 0)));
}

#[test]
fn release_version_parser_accepts_prerelease_with_build_metadata() {
    assert_eq!(parse_release_version("1.0.0-rc.1+local"), Some((1, 0, 0)));
}

#[test]
fn release_version_parser_ci_is_none() {
    assert_eq!(parse_release_version("ci"), None);
}

#[test]
fn release_version_parser_rejects_malformed_versions() {
    assert_eq!(parse_release_version("abc"), None);
    assert_eq!(parse_release_version("1.0.0.0"), None);
    assert_eq!(parse_release_version(""), None);
}

#[test]
fn release_version_parser_defaults_missing_patch_to_zero() {
    assert_eq!(parse_release_version("1.0"), Some((1, 0, 0)));
}

#[test]
fn legacy_redirect_resolves_to_canonical_term() {
    let pack = ReferencePack::load_from(repo_exempla_root()).expect("load");
    let resolved = pack.resolve_term("==").expect("legacy redirect");
    match resolved {
        ResolvedTerm::Legacy {
            redirect,
            canonical,
        } => {
            assert_eq!(redirect.canonical, "≡");
            assert_eq!(canonical.term, "≡");
        }
        ResolvedTerm::Canonical(_) => panic!("expected legacy redirect"),
    }
}
