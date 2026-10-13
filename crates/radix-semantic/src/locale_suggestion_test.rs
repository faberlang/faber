use super::*;
use std::sync::atomic::Ordering;

fn reader_pack(id: &str) -> LocalePack {
    LocalePack::from_toml_path(format!(
        "{}/../../locale/{id}/pack.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|err| panic!("load reader pack `{id}`: {err:?}"))
}

/// Exact keyword, near-miss typos, ASCII names, NFKC-foldable input,
/// accent-folded input, a one-char name, the reserved `inf` marker, and
/// unknown words (too short, in range but far, too long).
const SPREAD: &[&str] = &[
    "while",
    "dum",
    "whlie",
    "wihle",
    "dun",
    "retrun",
    "reddee",
    "\u{ff57}\u{ff48}\u{ff49}\u{ff4c}\u{ff45}",
    "d\u{fc}m",
    "w\u{e9}hile",
    "x",
    "ab",
    "inf",
    "zzzzzzzz",
    "quodlibetquodlibet",
    "caf\u{e9}_name",
    "\u{3b1}\u{3b2}\u{3b3}\u{3b4}",
    "counter",
    "",
];

fn assert_memo_matches_uncached(pack: &LocalePack) {
    for text in SPREAD {
        let reference = pack.compute_keyword_suggestion(text);
        // Cold call computes and stores; warm calls must hit the memo.
        assert_eq!(pack.keyword_suggestion(text), reference, "cold `{text}`");
        assert_eq!(pack.keyword_suggestion(text), reference, "warm `{text}`");
    }
}

#[test]
fn memoized_suggestion_equals_uncached_in_en_and_la() {
    for id in ["en", "la"] {
        let pack = reader_pack(id);
        assert_memo_matches_uncached(&pack);
    }
}

#[test]
fn memoized_suggestion_equals_uncached_for_latin_registry_pack() {
    assert_memo_matches_uncached(&latin_locale_pack());
}

#[test]
fn spread_exercises_suggestion_and_no_suggestion_outcomes() {
    let en = reader_pack("en");
    assert_eq!(en.keyword_suggestion("whlie").as_deref(), Some("while"));
    assert_eq!(
        en.keyword_suggestion("\u{ff57}\u{ff48}\u{ff49}\u{ff4c}\u{ff45}")
            .as_deref(),
        Some("while")
    );
    assert_eq!(en.keyword_suggestion("zzzzzzzz"), None);
    assert_eq!(en.keyword_suggestion("x"), None);
    assert_eq!(en.keyword_suggestion("inf"), None);
    let la = reader_pack("la");
    assert_eq!(la.keyword_suggestion("dun").as_deref(), Some("dum"));
}

#[test]
fn packs_with_different_spelling_tables_do_not_share_results() {
    let en = reader_pack("en");
    let la = reader_pack("la");
    // `while` is a keyword in en, a plain word far from every la keyword.
    assert_eq!(en.keyword_suggestion("whlie").as_deref(), Some("while"));
    assert_ne!(
        la.keyword_suggestion("whlie"),
        en.keyword_suggestion("whlie")
    );
    assert_eq!(
        la.keyword_suggestion("whlie"),
        la.compute_keyword_suggestion("whlie")
    );
}

#[test]
fn repeated_identifier_is_computed_once() {
    // A table no other test shares, so the memo (and its counter) is private.
    let mut pack = reader_pack("en");
    pack.keywords.insert(
        "kwsuggest_unique_probe".to_owned(),
        KeywordBinding {
            key: "kwsuggest_unique_probe".to_owned(),
            canonical: "kwsuggestuniqueprobe".to_owned(),
            aliases: Vec::new(),
        },
    );
    pack.rebuild_surfaces();
    let computed = || pack.suggestion_memo.0.computed.load(Ordering::Relaxed);
    assert_eq!(computed(), 0);

    for _ in 0..50 {
        assert_eq!(
            pack.keyword_suggestion("kwsuggestuniqueprobee").as_deref(),
            Some("kwsuggestuniqueprobe")
        );
    }
    assert_eq!(computed(), 1);

    // A no-suggestion result is memoized too.
    for _ in 0..50 {
        assert_eq!(pack.keyword_suggestion("zzzzzzzz"), None);
    }
    assert_eq!(computed(), 2);

    // A clone shares the memo: still no new computation for known text.
    let clone = pack.clone();
    assert_eq!(clone, pack);
    assert_eq!(clone.keyword_suggestion("zzzzzzzz"), None);
    assert_eq!(computed(), 2);
}

#[test]
fn suggestion_key_folds_width_and_accents() {
    assert_eq!(suggestion_key("d\u{fc}m"), "dum");
    assert_eq!(suggestion_key("w\u{e9}hile"), "wehile");
    assert_eq!(
        suggestion_key("\u{ff57}\u{ff48}\u{ff49}\u{ff4c}\u{ff45}"),
        "while"
    );
    assert_eq!(suggestion_key("caf\u{e9}_name"), "cafe_name");
    assert_eq!(suggestion_key("plain"), "plain");
}

#[test]
fn latin_scaffold_carries_word_type_rows() {
    let pack = latin_locale_pack();
    assert_eq!(pack.type_latin_key("modulus_t"), Some("modulus"));
    assert_eq!(pack.type_latin_key("saturatus"), Some("saturatus"));
    assert_eq!(pack.type_latin_key("exactus"), Some("exactus"));
    assert_eq!(pack.type_latin_key("octetus"), Some("octetus"));
    assert_eq!(pack.type_surface(TypeBindingKind::ModularWord), "modulus_t");
    assert_eq!(pack.type_surface(TypeBindingKind::SaturatingWord), "saturatus");
}

#[test]
fn duplicate_library_member_surface_is_rejected_in_owner_scope() {
    let err = LocalePack::from_toml_str(
        r#"
[pack]
id = "en"

[[library_members]]
provider = "package"
package = "demo"
module_path = ["math"]
kind = "function"
canonical = "summa"
surface = "sum"

[[library_members]]
provider = "package"
package = "demo"
module_path = ["math"]
kind = "function"
canonical = "differentia"
surface = "sum"

[llm]
system_prompt_snippet = "emit English reader-locale Faber"
exemplars = ["salve-munde.en.fab"]
"#,
    )
    .expect_err("duplicate scope surface should fail validation");
    let LocalePackLoadError::Validation(issues) = err else {
        panic!("expected validation errors");
    };
    assert!(issues.iter().any(|issue| matches!(
        issue,
        LocalePackIssue::DuplicateLibraryMemberSurface { owner: None, surface, .. }
            if surface == "sum"
    )));
}
