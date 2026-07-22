use crate::explain::{render_json, render_plain, render_search, Lookup, Registry};
use crate::reference::ReferencePack;
use crate::reference_pack_test_support::{env_lock, repo_exempla_root};
use serde::Deserialize;

fn disk_registry() -> Registry {
    let root = repo_exempla_root();
    ReferencePack::load_from(&root)
        .expect("load exempla pack")
        .build_registry()
        .expect("build registry")
}

#[test]
fn disk_registry_resolves_core_terms() {
    let registry = disk_registry();
    assert!(registry.lookup("≡").is_some());
    assert!(registry.lookup("→").is_some());
    assert!(registry.lookup("proba").is_some());
    assert!(registry.reference_root().is_some());
}

#[test]
fn lookup_resolves_terms_aliases_and_legacy() {
    let registry = disk_registry();
    assert!(matches!(registry.lookup("≡"), Some(Lookup::Exact(entry)) if entry.term == "≡"));
    assert!(
        matches!(registry.lookup("function"), Some(Lookup::Alias { entry, .. }) if entry.term == "functio")
    );
    assert!(
        matches!(registry.lookup("=="), Some(Lookup::Legacy { canonical, .. }) if canonical.term == "≡")
    );
    assert!(
        matches!(registry.lookup("==="), Some(Lookup::Legacy { canonical, .. }) if canonical.term == "est")
    );
    assert!(
        matches!(registry.lookup("!=="), Some(Lookup::Legacy { canonical, .. }) if canonical.term == "non est")
    );
}

#[test]
fn index_manifest_matches_registry() {
    let manifest = load_index_manifest();
    let registry = disk_registry();

    assert_eq!(
        registry.entries().len(),
        manifest.registry_terms as usize + manifest.legacy_terms.len()
    );

    for term in &manifest.canonical_terms {
        assert_canonical_term(&registry, term);
    }

    for (term, canonical_term) in &manifest.legacy_canonical {
        assert_legacy_term(&registry, term, canonical_term);
    }

    for term in &manifest.excluded_terms {
        assert!(
            registry.lookup(term).is_none(),
            "excluded term {term:?} unexpectedly has explain coverage"
        );
    }
}

#[test]
fn all_entries_validate() {
    let registry = disk_registry();
    assert_eq!(registry.entries().len(), 195);

    for entry in registry.entries() {
        assert!(!entry.term.is_empty());
        assert!(!entry.category.is_empty());
        assert!(!entry.syntax.is_empty());
        assert!(!entry.summary.is_empty());
        assert!(
            entry.body.contains("```fab"),
            "{} missing fab example",
            entry.term
        );
    }
}

#[test]
fn render_plain_includes_short_contract() {
    let registry = disk_registry();
    let lookup = registry.lookup("≡").expect("lookup equality");
    let rendered = render_plain(&lookup);
    assert!(rendered.contains("NAME"));
    assert!(rendered.contains("SYNTAX"));
    assert!(rendered.contains("DESCRIPTION"));
    assert!(rendered.contains("<expression> ≡ <expression>"));
    assert!(rendered.contains("operator / comparison"));
    assert!(rendered.contains("incipit"));
    assert!(rendered.contains("RELATED"));
}

#[test]
fn render_legacy_uses_distinct_layout() {
    let registry = disk_registry();
    let lookup = registry.lookup("===").expect("lookup legacy equality");
    let rendered = render_plain(&lookup);
    assert!(rendered.contains("STATUS"));
    assert!(rendered.contains("legacy"));
    assert!(rendered.contains("USE INSTEAD"));
    assert!(rendered.contains("est"));
    assert!(rendered.contains("not canonical Faber source"));
}

#[test]
fn search_returns_ranked_candidates() {
    let registry = disk_registry();
    let hits = registry.search("equality");
    assert!(!hits.is_empty());
    assert!(hits.iter().any(|hit| hit.entry.term == "≡"));
    assert!(hits.iter().any(|hit| hit.entry.term == "est"));

    let rendered = render_search("equality", &hits);
    assert!(rendered.starts_with("Search: equality"));
    assert!(rendered.contains("≡"));
    assert!(rendered.contains("est"));
}

#[test]
fn render_json_is_valid() {
    let registry = disk_registry();
    let lookup = registry.lookup("≡").expect("lookup equality");
    let rendered = render_json(&lookup).expect("json renders");
    let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid json");
    assert_eq!(value["term"], "≡");
    assert_eq!(value["canonical"], true);
}

#[test]
fn load_aliases_disk_registry_entry_point() {
    let _guard = env_lock();
    let previous = std::env::var(crate::reference::REFERENCE_ROOT_ENV).ok();
    std::env::remove_var(crate::reference::REFERENCE_ROOT_ENV);

    let registry = Registry::load().expect("load via disk entry point");
    assert_eq!(registry.entries().len(), 195);

    if let Some(value) = previous {
        std::env::set_var(crate::reference::REFERENCE_ROOT_ENV, value);
    }
}

#[derive(Debug, Deserialize)]
struct IndexManifest {
    registry_terms: u32,
    terms: Vec<IndexTerm>,
}

#[derive(Debug, Deserialize)]
struct IndexTerm {
    term: String,
}

#[derive(Debug, Deserialize)]
struct LegacyRedirectsFile {
    redirects: Vec<LegacyRedirectRow>,
}

#[derive(Debug, Deserialize)]
struct LegacyRedirectRow {
    term: String,
    canonical: String,
}

#[derive(Debug, Deserialize)]
struct ExcludedTermsFile {
    terms: Vec<String>,
}

fn load_index_manifest() -> IndexManifestView {
    let index: IndexManifest =
        toml::from_str(include_str!("../../examples/corpus/index.toml")).expect("index");
    let legacy: LegacyRedirectsFile =
        toml::from_str(include_str!("../../examples/corpus/legacy-redirects.toml"))
            .expect("legacy redirects");
    let excluded: ExcludedTermsFile =
        toml::from_str(include_str!("../../examples/corpus/excluded-terms.toml"))
            .expect("excluded terms");
    let legacy_canonical = legacy
        .redirects
        .iter()
        .map(|row| (row.term.clone(), row.canonical.clone()))
        .collect::<Vec<_>>();
    let legacy_terms = legacy_canonical
        .iter()
        .map(|(term, _)| term.clone())
        .collect();
    IndexManifestView {
        registry_terms: index.registry_terms,
        canonical_terms: index.terms.into_iter().map(|row| row.term).collect(),
        legacy_terms,
        legacy_canonical,
        excluded_terms: excluded.terms,
    }
}

struct IndexManifestView {
    registry_terms: u32,
    canonical_terms: Vec<String>,
    legacy_terms: Vec<String>,
    legacy_canonical: Vec<(String, String)>,
    excluded_terms: Vec<String>,
}

fn assert_canonical_term(registry: &Registry, term: &str) {
    match registry.lookup(term) {
        Some(Lookup::Exact(entry)) => assert_eq!(entry.term, term),
        other => panic!("expected canonical lookup for {term:?}, got {other:?}"),
    }
}

fn assert_legacy_term(registry: &Registry, term: &str, canonical_term: &str) {
    match registry.lookup(term) {
        Some(Lookup::Legacy {
            entry, canonical, ..
        }) => {
            assert_eq!(entry.term, term);
            assert!(!entry.canonical);
            assert_eq!(canonical.term, canonical_term);
        }
        other => panic!("expected legacy lookup for {term:?}, got {other:?}"),
    }
}
