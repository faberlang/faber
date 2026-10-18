//! Reader-locale handling of the annotation-contract family word.
//!
//! The family word is a locale-pack keyword (`annotatio` is the canonical
//! Latin key, en spells it `annotation`). An en file must not need the Latin
//! spelling, and the Latin spelling is refused there like any other
//! annotation-owned Latin word.

use crate::locale::LocalePack;
use crate::{PassConfig, test_support};
use radix_target::Target;
use unicode_normalization::UnicodeNormalization;

fn pack(id: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id))
        .expect("installed pack should load")
}

fn analyze_with(id: &str, source: &str) -> test_support::AnalyzedFixture {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(pack(id));
    test_support::analyze_source_with_config(source, config)
}

const EN_CONTRACT: &str = r#"
@ annotation { target = fn }
class Meta {
    let string selector
}

@ Meta { selector = "main" }
fn apply() → void {
}
"#;

const LA_CONTRACT: &str = r#"
@ annotatio { target = functio }
genus Meta {
    varia textus selector
}

@ Meta { selector = "main" }
functio apply() → vacuum {
}
"#;

fn has_issue(result: &test_support::AnalyzedFixture, issue: &str, expected: Option<&str>) -> bool {
    result.diagnostics.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)
            && expected.is_none_or(|word| {
                error
                    .args
                    .iter()
                    .any(|arg| arg.name == "expected" && arg.value == word)
            })
    })
}

#[test]
fn en_annotation_family_word_declares_a_contract() {
    let result = analyze_with("en", EN_CONTRACT);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    assert_eq!(result.annotation_contracts.registry.iter().count(), 1);
    assert_eq!(result.annotation_contracts.applications().count(), 1);
}

#[test]
fn en_refuses_the_latin_annotation_family_word() {
    let source = EN_CONTRACT.replace("@ annotation", "@ annotatio");
    let result = analyze_with("en", &source);
    assert!(
        has_issue(&result, "wrong_locale_annotation", Some("annotation")),
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn la_keeps_the_latin_annotation_family_word() {
    let result = analyze_with("la", LA_CONTRACT);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    assert_eq!(result.annotation_contracts.registry.iter().count(), 1);
}

#[test]
fn la_refuses_the_en_annotation_family_word() {
    let source = LA_CONTRACT.replace("@ annotatio", "@ annotation");
    let result = analyze_with("la", &source);
    assert!(
        result.annotation_contracts.registry.iter().count() == 0,
        "the en spelling must not declare a contract under la"
    );
}

#[test]
fn every_pack_resolves_its_annotation_family_spellings_to_the_latin_keys() {
    // (pack, annotatio, versio, imperia)
    for (id, words) in [
        ("en", ["annotation", "version", "commands"]),
        ("la", ["annotatio", "versio", "imperia"]),
        ("ar", ["تعليق_توضيحي", "إصدار", "أوامر"]),
        ("hi", ["टीका", "संस्करण", "आदेश_सूची"]),
        ("th-TH", ["คำอธิบายประกอบ", "เวอร์ชัน", "ชุดคำสั่ง"]),
        ("vi", ["chú_thích", "phiên_bản", "các_lệnh"]),
        ("zh-Hans", ["注解", "版本", "命令集"]),
        ("zh-Hant", ["註解", "版本", "命令集"]),
    ] {
        for (word, latin) in words.into_iter().zip(["annotatio", "versio", "imperia"]) {
            assert_eq!(
                pack(id).keyword_latin_key(&word.nfkc().collect::<String>()),
                Some(latin),
                "{id} must spell the annotation family word {word:?}"
            );
        }
    }
}
