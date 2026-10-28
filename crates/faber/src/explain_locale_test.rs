use crate::explain::{Kind, Lookup, Registry};
use crate::explain_locale::{rewrite_prose, rewrite_words};
use crate::package::load_embedded_locale_pack;
use std::collections::BTreeMap;

fn registry_for(locale: &str) -> Registry {
    let pack = load_embedded_locale_pack(locale).expect("embedded pack");
    Registry::load_for_pack(&pack).expect("projected registry")
}

fn keyword_spelling(locale: &str, identity: &str) -> String {
    let pack = load_embedded_locale_pack(locale).expect("embedded pack");
    pack.keywords[identity].canonical.clone()
}

#[test]
fn en_registry_resolves_the_pack_spelling() {
    let registry = registry_for("en");
    for identity in ["functio", "si", "itera", "discerne"] {
        let spelling = keyword_spelling("en", identity);
        assert_ne!(spelling, identity, "{identity} should be renamed in en");
        assert!(
            matches!(registry.lookup(&spelling), Some(Lookup::Exact(entry)) if entry.term == spelling),
            "{spelling} (en spelling of {identity}) must resolve"
        );
    }
}

#[test]
fn en_registry_does_not_answer_to_the_latin_identity() {
    let registry = registry_for("en");
    assert!(registry.lookup("functio").is_none());
    assert_eq!(registry.spelling_of_identity("functio"), Some("fn"));
    assert_eq!(registry.spelling_of_identity("fn"), None);
}

#[test]
fn en_list_shows_no_renamed_identity_as_a_term() {
    let registry = registry_for("en");
    let pack = load_embedded_locale_pack("en").expect("embedded pack");
    for entry in registry.entries().iter().filter(|entry| entry.canonical) {
        let row = if entry.kind == Kind::Type {
            pack.types.get(&entry.term)
        } else {
            None
        };
        let keyword = pack.keywords.get(&entry.term);
        let renamed = row.is_some_and(|row| row.canonical != entry.term)
            || (row.is_none() && keyword.is_some_and(|row| row.canonical != entry.term));
        assert!(!renamed, "{} is a renamed Latin identity", entry.term);
    }
}

#[test]
fn en_list_is_sorted_by_the_spelling_shown() {
    let registry = registry_for("en");
    let terms = registry
        .entries()
        .iter()
        .map(|entry| entry.term.as_str())
        .collect::<Vec<_>>();
    let mut sorted = terms.clone();
    sorted.sort_unstable();
    assert_eq!(terms, sorted);
}

#[test]
fn la_registry_resolves_through_the_la_pack() {
    let identity = Registry::load().expect("identity registry");
    let la = registry_for("la");
    assert_eq!(identity.entries().len(), la.entries().len());
    assert!(matches!(
        la.lookup("functio"),
        Some(Lookup::Exact(entry)) if entry.term == "functio"
    ));
    assert!(la.lookup("fn").is_none());
    // Every identity answers to the spelling the la pack gives it, or the pack
    // names that spelling in the miss hint.
    for entry in identity
        .entries()
        .iter()
        .filter(|entry| entry.kind != Kind::Legacy)
    {
        assert!(
            la.lookup(&entry.term).is_some() || la.spelling_of_identity(&entry.term).is_some(),
            "{} must resolve in la",
            entry.term
        );
    }
}

#[test]
fn projection_keeps_every_entry_and_cross_reference() {
    let identity = Registry::load().expect("identity registry");
    let en = registry_for("en");
    assert_eq!(identity.entries().len(), en.entries().len());
    for entry in en.entries() {
        for related in &entry.related {
            assert!(
                en.lookup(related).is_some(),
                "{}: related {related} does not resolve in en",
                entry.term
            );
        }
    }
}

#[test]
fn shared_spelling_keeps_both_entries_reachable() {
    let registry = registry_for("en");
    // `varia` (binding) and `in` (parameter) are both `mut` in en.
    let bare = registry.lookup("mut").expect("bare mut");
    assert!(matches!(bare, Lookup::Exact(entry) if entry.category == "binding"));
    let qualified = registry.lookup("mut (function)").expect("qualified mut");
    assert!(matches!(qualified, Lookup::Exact(entry) if entry.category == "function"));
}

#[test]
fn explicit_locale_flag_selects_the_pack() {
    let la = Registry::load_for_locale(Some("la")).expect("la registry");
    assert!(la.lookup("functio").is_some());
    let en = Registry::load_for_locale(Some("en")).expect("en registry");
    assert!(en.lookup("fn").is_some());
    assert!(Registry::load_for_locale(Some("xx")).is_err());
    assert!(Registry::load_for_locale(Some("  ")).is_err());
}

#[test]
fn every_explain_example_in_the_help_resolves_in_en() {
    let registry = registry_for("en");
    let help = include_str!("../help/faber-after-help.md");
    let mut checked = 0;
    for line in help.lines() {
        let Some(rest) = line.trim().strip_prefix("faber explain ") else {
            continue;
        };
        let term = rest
            .split_whitespace()
            .filter(|word| !word.starts_with("--"))
            .last();
        let flags = rest.split_whitespace().next().unwrap_or("");
        if flags == "--list" || flags == "--category" || flags == "--search" {
            continue;
        }
        let Some(term) = term else { continue };
        assert!(
            registry.lookup(term).is_some(),
            "help example `{line}` does not resolve in en"
        );
        checked += 1;
    }
    assert!(checked >= 3, "help should carry explain examples");
}

fn words() -> BTreeMap<String, String> {
    [
        ("itera", "for"),
        ("in", "mut"),
        ("functio", "fn"),
        ("ab", "range"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

#[test]
fn prose_rewrites_backtick_spans_only() {
    let words = words();
    assert_eq!(
        rewrite_prose("Runs in an itera loop; `in` and `ab` mark it.", &words),
        "Runs in an itera loop; `mut` and `range` mark it."
    );
}

#[test]
fn prose_leaves_a_bare_english_word_that_is_an_identity_alone() {
    let mut words = words();
    words.insert("series".to_owned(), "row".to_owned());
    assert_eq!(
        rewrite_prose("A series of values; `series` is the identity.", &words),
        "A series of values; `row` is the identity."
    );
    // Every identity word in bare prose is untouched, whatever its length.
    assert_eq!(
        rewrite_prose("functio itera in ab series", &words),
        "functio itera in ab series"
    );
}

#[test]
fn prose_article_agrees_only_for_a_rewritten_span() {
    let words = words();
    assert_eq!(rewrite_prose("an `itera` loop", &words), "a `for` loop");
    assert_eq!(rewrite_prose("an itera loop", &words), "an itera loop");
}

#[test]
fn prose_with_an_unterminated_span_is_left_whole() {
    let words = words();
    assert_eq!(rewrite_prose("see `itera", &words), "see `itera");
}

#[test]
fn syntax_rewrites_every_word_on_word_boundaries() {
    let words = words();
    assert_eq!(
        rewrite_words("in <type> <name> functio_x functio", &words, 0),
        "mut <type> <name> functio_x fn"
    );
}

#[test]
fn article_follows_the_new_spelling() {
    let words = words();
    assert_eq!(
        rewrite_words("An itera and a ab", &words, 0),
        "A for and a range"
    );
    assert_eq!(rewrite_words("a in", &words, 0), "a mut");
    assert_eq!(rewrite_words("an ab", &words, 0), "a range");
}
