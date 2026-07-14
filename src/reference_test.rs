use crate::explain::{render_list, render_plain, Lookup, Registry};
use crate::reference::{
    pack_version_skew, parse_release_version, resolve_reference_root, PackMetadata,
    ReferenceLayout, ReferencePack, ResolvedTerm, REFERENCE_ROOT_ENV,
};
use crate::reference_pack_test_support::{env_lock, repo_exempla_root};
use crate::reference_parse::entry_from_exempla;

#[test]
fn dev_fallback_loads_repo_exempla_index() {
    let _guard = env_lock();
    let previous = std::env::var(REFERENCE_ROOT_ENV).ok();
    std::env::remove_var(REFERENCE_ROOT_ENV);

    let pack = ReferencePack::load().expect("dev fallback loads repo exempla");
    assert_eq!(pack.term_count(), 174);
    assert_eq!(pack.metadata().registry_terms, 174);
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
    assert_eq!(pack.term_count(), 174);
    assert_eq!(pack.legacy_redirects().len(), 16);
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
    assert_eq!(registry.entries().len(), 190);
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
    assert_eq!(registry.entries().len(), 190);
    let list = render_list(&registry);
    assert!(list.contains("reference:"));

    if let Some(value) = previous {
        std::env::set_var(REFERENCE_ROOT_ENV, value);
    }
}

#[test]
fn parse_exempla_entry_for_spot_check_terms() {
    let root = repo_exempla_root();
    let cases = [
        ("functio/functio.fab", "functio", "keyword"),
        ("operatores/comparatio.fab", "≡", "operator-group"),
        ("meta/manifest.fab", "manifest", "concept"),
    ];
    for (rel, term, rule) in cases {
        let path = root.join(rel);
        let source = std::fs::read_to_string(&path).expect("read exempla");
        let entry = entry_from_exempla(rel, &source, term, rule).expect("parse entry");
        assert_eq!(entry.term, term);
        assert!(entry.body.contains("```fab"));
    }
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
fn release_version_parser_accepts_prerelease_and_build_metadata() {
    assert_eq!(parse_release_version("1.0.0-rc.1"), Some((1, 0, 0)));
    assert_eq!(parse_release_version("1.0.0+local"), Some((1, 0, 0)));
    assert_eq!(parse_release_version("1.0.0-rc.1+local"), Some((1, 0, 0)));
    assert_eq!(parse_release_version("ci"), None);
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
