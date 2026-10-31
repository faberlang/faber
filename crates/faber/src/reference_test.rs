use crate::explain::{Lookup, Registry, render_plain};
use crate::reference::{ReferencePack, ResolvedTerm};
use crate::reference_parse::entry_from_exempla;

const LEGACY_NONE: &str = "redirects = []\n";

fn exemplum(term: &str, explains: &str) -> String {
    format!(
        "+++\nterm = \"{term}\"\n{explains}kind = \"keyword\"\ncategory = \"testing\"\n\
         summary = \"A term.\"\nsyntax = \"{term}\"\n+++\n# {term}\n\nnota \"x\"\n"
    )
}

#[test]
fn files_without_explains_contribute_no_terms() {
    let with = exemplum("ping", "explains = [\"ping\", \"pong\"]\n");
    let without = exemplum("quiet", "");
    let pack = ReferencePack::from_sources(
        &[("ping/ping.fab", &with), ("quiet/quiet.fab", &without)],
        LEGACY_NONE,
    )
    .expect("pack");
    let terms = pack
        .terms()
        .iter()
        .map(|record| (record.term.as_str(), record.primary.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        terms,
        [("ping", "ping/ping.fab"), ("pong", "ping/ping.fab")]
    );
    let registry = pack.build_registry().expect("registry");
    assert!(matches!(registry.lookup("pong"), Some(Lookup::Exact(entry)) if entry.term == "pong"));
    assert!(registry.lookup("quiet").is_none());
}

#[test]
fn a_term_explained_by_two_files_is_rejected() {
    let first = exemplum("ping", "explains = [\"ping\"]\n");
    let second = exemplum("other", "explains = [\"ping\"]\n");
    let err =
        ReferencePack::from_sources(&[("a/a.fab", &first), ("b/b.fab", &second)], LEGACY_NONE)
            .expect_err("duplicate term");
    assert!(err.message.contains("a/a.fab") && err.message.contains("b/b.fab"));
}

#[test]
fn a_redirect_to_an_unexplained_term_is_rejected() {
    let source = exemplum("ping", "explains = [\"ping\"]\n");
    let legacy = "[[redirects]]\nterm = \"old\"\ncanonical = \"gone\"\nmessage = \"Retired.\"\n";
    let err = ReferencePack::from_sources(&[("ping/ping.fab", &source)], legacy)
        .expect_err("dangling redirect");
    assert!(err.message.contains("\"gone\""));
}

#[test]
fn embedded_registry_loads_canonical_and_legacy_entries() {
    let pack = ReferencePack::embedded().expect("embedded reference");
    let registry = pack.build_registry().expect("build registry");
    assert_eq!(
        registry.entries().len(),
        pack.term_count() + pack.legacy_redirects().len()
    );
    // 203 explained terms (including `⇜`, the four ranged store policy terms
    // and the list size range) plus 14 legacy redirects (`generis` is retired).
    assert_eq!(registry.entries().len(), 218);
    assert!(registry.lookup("functio").is_some());
    assert!(matches!(registry.lookup("=="), Some(Lookup::Legacy { .. })));
}

#[test]
fn audit_finding_topics_resolve_with_current_behavior() {
    let registry = Registry::load().expect("embedded registry");

    // ⇇ copy-into: alias and term both reach the same canonical entry, and the
    // body carries the EBNF [158]/[159] precedence rows.
    assert!(matches!(registry.lookup("⇇"), Some(Lookup::Exact(_))));
    assert!(matches!(
        registry.lookup("transfer"),
        Some(Lookup::Alias { .. })
    ));
    let transfer = render_plain(&registry.lookup("⇇").expect("transfer resolves"));
    assert!(
        transfer.contains("[158]"),
        "transfer entry cites EBNF row 158"
    );
    assert!(
        transfer.contains("[159]"),
        "transfer entry cites EBNF row 159"
    );
    assert!(
        transfer.contains("vacuum"),
        "copy-into statement result is vacuum"
    );

    // Selective imports cite the [067]/[071] rows and the fixum member rule.
    let selective = render_plain(&registry.lookup("selective_import").expect("selective"));
    assert!(selective.contains("[071]"));
    assert!(selective.contains("fixum"));

    // The first reduction documents first-match semantics.
    let first = render_plain(&registry.lookup("reducta-first").expect("reducta-first"));
    assert!(first.contains("first live"));

    // filum is the distributed `reducta via summa ex` reduction, not a list op.
    let filum = render_plain(&registry.lookup("filum").expect("filum"));
    assert!(filum.contains("reducta via summa ex"));
    assert!(filum.contains("@ nucleum"));

    // Bits names its retirement and the bN replacement.
    let bits = render_plain(&registry.lookup("Bits").expect("Bits"));
    assert!(bits.contains("retired"));
    assert!(bits.contains("b16"));

    // finite teaches the bit-field read gate, never x ≢ x.
    let finite = render_plain(&registry.lookup("finite").expect("finite"));
    assert!(finite.contains("exponent"));
    assert!(finite.contains("≢ x"));

    // summa explains the grammar reduction, not the list `.summa()` method.
    let summa_lookup = registry.lookup("summa").expect("summa resolves");
    let summa = render_plain(&summa_lookup);
    assert!(
        summa.contains("reducta via summa ex"),
        "summa entry is the reduce grammar"
    );
    assert!(!summa.contains(".sum()"));
}

#[test]
fn parse_exempla_entry_for_functio_keyword() {
    let source = include_str!("../../../corpus/functio/functio.fab");
    let entry = entry_from_exempla("functio/functio.fab", source, "functio").expect("parse entry");
    assert_eq!(entry.term, "functio");
    assert!(entry.body.contains("```fab"));
}

#[test]
fn parse_exempla_entry_for_equivalence_operator() {
    let source = include_str!("../../../corpus/operatores/comparatio.fab");
    let entry = entry_from_exempla("operatores/comparatio.fab", source, "≡").expect("parse entry");
    assert_eq!(entry.term, "≡");
    assert!(entry.body.contains("```fab"));
}

#[test]
fn parse_exempla_entry_for_manifest_concept() {
    let source = include_str!("../../../corpus/meta/manifest.fab");
    let entry = entry_from_exempla("meta/manifest.fab", source, "manifest").expect("parse entry");
    assert_eq!(entry.term, "manifest");
    assert!(entry.body.contains("```fab"));
}

#[test]
fn render_plain_includes_short_contract_for_functio() {
    let registry = Registry::load().expect("embedded registry");
    let lookup = registry.lookup("functio").expect("functio");
    let rendered = render_plain(&lookup);
    assert!(rendered.contains("NAME"));
    assert!(rendered.contains("SYNTAX"));
    assert!(rendered.contains("functio <name>"));
}

#[test]
fn legacy_redirect_resolves_to_canonical_term() {
    let pack = ReferencePack::embedded().expect("embedded reference");
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

#[test]
fn nihil_entry_states_type_value_rule() {
    let registry = Registry::load().expect("embedded registry");
    let nihil = render_plain(&registry.lookup("nihil").expect("nihil resolves"));
    assert!(nihil.contains("is none"), "names the none-type check");
    assert!(nihil.contains("null"), "names the null value");
    assert!(nihil.contains("nihil"), "keeps the la gloss");
    assert!(!nihil.contains("used for null/none values"));
}
