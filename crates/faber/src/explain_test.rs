use crate::explain::{
    Lookup, Registry, render_json, render_list, render_list_json, render_plain, render_search,
};
use crate::reference::ReferencePack;
use serde::Deserialize;

fn disk_registry() -> Registry {
    Registry::load().expect("embedded registry")
}

#[test]
fn disk_registry_resolves_core_terms() {
    let registry = disk_registry();
    assert!(registry.lookup("≡").is_some());
    assert!(registry.lookup("→").is_some());
    assert!(registry.lookup("proba").is_some());
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
        matches!(registry.lookup("==="), Some(Lookup::Legacy { canonical, .. }) if canonical.term == "≡")
    );
    assert!(
        matches!(registry.lookup("!=="), Some(Lookup::Legacy { canonical, .. }) if canonical.term == "≢")
    );
}

#[test]
fn every_declared_term_resolves() {
    let manifest = load_index_manifest();
    let registry = disk_registry();

    assert_eq!(
        registry.entries().len(),
        manifest.canonical_terms.len() + manifest.legacy_terms.len()
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
    // 216 → 210 → 211 → 210 → 211: the registry follows the canonical term count
    // (197) plus the legacy redirects (13). The `nulla` redirect retired with
    // D23.4 (8c5cc3de4), so 197 + 13 = 210 (mg5a folds inter+intra into one
    // term; need adds `guillemet`); 211 = 198 + 13 (need
    // adds trapped); `generis` moved from an explained term to a legacy
    // redirect (the retired static member word): 197 + 14 = 211; 212 = 198 + 14
    // (the sink-into `⇜` exemplum adds one explained term); 216 = 202 + 14
    // (the ranged store policies add `saturating<W, lo‥hi>`,
    // `trapping<W, lo‥hi>`, `wrapping<W, lo‥hi>` and `whole-width range`);
    // 217 = 203 + 14 (the list size range exemplum adds `list<T, size lo‥hi>`).
    // See reference_test for history.
    assert_eq!(registry.entries().len(), 217);

    for entry in registry.entries() {
        assert_ne!(entry.term, "");
        assert_ne!(entry.category, "");
        assert_ne!(entry.syntax, "");
        assert_ne!(entry.summary, "");
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
    assert!(rendered.contains("main"));
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
    assert_ne!(hits, [] as [crate::explain::SearchHit<'_>; 0]);
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
fn render_json_carries_exemplum_grammar_ids() {
    let registry = disk_registry();
    let lookup = registry.lookup("ab").expect("lookup ab");
    let rendered = render_json(&lookup).expect("json renders");
    let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid json");
    let grammar = value["grammar"].as_array().expect("grammar array");
    let ids: Vec<&str> = grammar.iter().filter_map(|id| id.as_str()).collect();
    assert_eq!(ids.first(), Some(&"fab_file"));
    assert!(ids.contains(&"itera_stmt"));
    assert!(ids.contains(&"range_expr"));
}

#[test]
fn render_json_omits_grammar_when_entry_has_none() {
    let registry = disk_registry();
    // The legacy redirect entry has no exemplum, so it declares no grammar ids;
    // its canonical target still does.
    let lookup = registry.lookup("==").expect("lookup legacy equality");
    let rendered = render_json(&lookup).expect("json renders");
    let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid json");
    assert!(value["entry"].get("grammar").is_none());
    assert!(value["canonical"].get("grammar").is_some());
}

#[test]
fn render_list_json_matches_plain_list_rows() {
    let registry = disk_registry();
    let rendered = render_list_json(&registry).expect("list json renders");
    let rows: Vec<serde_json::Value> = serde_json::from_str(&rendered).expect("valid json array");
    let canonical = registry.entries().iter().filter(|e| e.canonical).count();
    assert_eq!(rows.len(), canonical);

    let plain = render_list(&registry);
    for row in &rows {
        for field in ["term", "kind", "category", "summary"] {
            assert!(row[field].is_string(), "row missing {field}: {row}");
        }
        let summary = row["summary"].as_str().expect("summary");
        assert!(plain.contains(summary), "plain list lacks {summary}");
    }
    // Plain list ignores --json: still the sectioned fixed-width text.
    assert!(plain.starts_with("KEYWORDS\n"));
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
    let pack = ReferencePack::embedded().expect("embedded reference");
    let legacy: LegacyRedirectsFile =
        toml::from_str(include_str!("../../../corpus/legacy-redirects.toml"))
            .expect("legacy redirects");
    let excluded: ExcludedTermsFile =
        toml::from_str(include_str!("../../../corpus/excluded-terms.toml"))
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
        canonical_terms: pack
            .terms()
            .iter()
            .map(|record| record.term.clone())
            .collect(),
        legacy_terms,
        legacy_canonical,
        excluded_terms: excluded.terms,
    }
}

struct IndexManifestView {
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

#[test]
fn search_empty_query_returns_empty() {
    let registry = disk_registry();
    let hits = registry.search("");
    assert_eq!(hits, [] as [crate::explain::SearchHit<'_>; 0]);
    let rendered = render_search("", &hits);
    assert!(rendered.starts_with("Search: "));
}

#[test]
fn lookup_nonexistent_term_returns_none() {
    let registry = disk_registry();
    assert!(registry.lookup("no_such_term_xyzzy").is_none());
    assert!(registry.lookup("").is_none());
}

#[test]
fn explain_is_alias_resolves_and_est_wording() {
    let registry = disk_registry();
    assert!(
        matches!(registry.lookup("is"), Some(Lookup::Alias { entry, .. }) if entry.term == "est")
    );
    let est = render_plain(&registry.lookup("est").expect("est resolves"));
    assert!(est.contains("is none"));
    assert!(!est.contains("est null"));
}
