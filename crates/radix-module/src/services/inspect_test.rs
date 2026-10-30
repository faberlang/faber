//! Tests for the phase-inspection services (`faber lex` / `faber parse`).
//! Proves: inspection honors the source's frontmatter reader locale, and the
//! lex payload carries each identifier's spelling plus its Latin keyword
//! identity (the token stream the stage-5 Earley reference parser consumes).

use super::{hir_inspection_for_source, lex_inspection_for_source, parse_inspection_for_source};

const EN_SOURCE: &str = "+++\nlocale = \"en\"\n+++\nfn salve() {\n    if true then return\n}\n";

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("inspection JSON")
}

#[test]
fn parse_inspection_honors_frontmatter_locale() {
    let result = parse_inspection_for_source("en.fab", EN_SOURCE);

    assert!(result.success, "errors: {:?}", result.errors);
    let payload = json(&result.text);
    assert_eq!(payload["statements"], 1);
    assert_eq!(payload["ast"][0]["kind"], "Func");
}

#[test]
fn hir_inspection_honors_frontmatter_locale() {
    let source =
        "+++\nlocale = \"en\"\n+++\nfn twice(int n) → int {\n    return n * 2\n}\n\nmain {\n    print twice(4)\n}\n";
    let result = hir_inspection_for_source("en.fab", source);

    assert!(result.success, "errors: {:?}", result.errors);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let payload = json(&result.text);
    assert_eq!(payload["frontmatter"]["locale"], "en");
    assert_eq!(payload["items"], 1);
}

#[test]
fn lex_inspection_records_latin_keyword_identity_under_reader_locale() {
    let result = lex_inspection_for_source("en.fab", EN_SOURCE);
    let payload = json(&result.text);
    let idents: Vec<(String, Option<String>)> = payload["tokens"]
        .as_array()
        .expect("tokens")
        .iter()
        .filter(|token| {
            token["kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("Ident"))
        })
        .map(|token| {
            (
                token["text"].as_str().expect("ident text").to_owned(),
                token["keyword"].as_str().map(ToOwned::to_owned),
            )
        })
        .collect();

    assert_eq!(
        idents,
        vec![
            ("fn".to_owned(), Some("functio".to_owned())),
            ("salve".to_owned(), None),
            ("if".to_owned(), Some("si".to_owned())),
            ("true".to_owned(), Some("verum".to_owned())),
            ("then".to_owned(), Some("ergo".to_owned())),
            ("return".to_owned(), Some("redde".to_owned())),
        ]
    );
}

#[test]
fn lex_inspection_latin_surface_keys_keywords_by_their_own_spelling() {
    let result = lex_inspection_for_source("la.fab", "+++\nlocale = \"la\"\n+++\nfunctio salve() { }\n");
    let payload = json(&result.text);
    let first = &payload["tokens"][0];

    assert_eq!(first["text"], "functio");
    assert_eq!(first["keyword"], "functio");
    assert_eq!(payload["tokens"][1]["text"], "salve");
    assert!(payload["tokens"][1].get("keyword").is_none());
}

/// `faber hir` resolves imports as `faber check` does: a type imported from a
/// library module (`Orderable` from `norma:order`) is not an unknown type.
#[test]
fn hir_inspection_resolves_imported_library_types() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/implet/implet-orderable.fab");
    let source = std::fs::read_to_string(&path).expect("corpus file");
    let result = hir_inspection_for_source(&path.display().to_string(), &source);

    assert!(result.success, "errors: {:?}\n{}", result.errors, result.text);
    let payload = json(&result.text);
    assert_eq!(payload["success"], true);
    assert!(payload["errors"].as_array().expect("errors").is_empty());
    assert!(payload["items"].as_u64().expect("items") > 0);
}

// ── single-file inspection resolves frontmatter locales through the config ──
//
// The lex/parse/hir/cli-ir lanes used to resolve a declared `+++ locale ──`
// through a default config's discovered workspace root only:
// an installed binary with no `locale/` tree silently inspected the source in
// another surface. The `_with_config` entries take the caller's root/loader.

fn loader_only_config() -> crate::driver::Config {
    let en =
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en")).expect("en pack");
    let la =
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la")).expect("la pack");
    let mut config = crate::driver::Config::default();
    config.locale_root = None;
    config.with_locale_pack_loader(crate::driver::LocalePackLoader::new(move |locale_id| match locale_id {
        "en" => Ok(en.clone()),
        "la" => Ok(la.clone()),
        other => Err(format!("no pack for locale '{other}'")),
    }))
}

const LA_SOURCE: &str = "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio salve() { }\n";

#[test]
fn lex_inspection_resolves_frontmatter_locale_through_config_loader() {
    let result = super::lex_inspection_for_source_with_config("la.fab", LA_SOURCE, &loader_only_config());
    assert!(result.success, "errors: {:?}", result.errors);
    let payload = json(&result.text);
    // The Latin keyword claims its slot under the loader-resolved pack.
    assert!(
        payload["tokens"]
            .as_array()
            .expect("tokens")
            .iter()
            .any(|token| token["keyword"] == "functio"),
        "the Latin `functio` must lex as a keyword: {}",
        result.text
    );
}

#[test]
fn parse_inspection_resolves_frontmatter_locale_through_config_loader() {
    let result = super::parse_inspection_for_source_with_config("la.fab", LA_SOURCE, &loader_only_config());
    assert!(result.success, "errors: {:?}", result.errors);
    let payload = json(&result.text);
    assert_eq!(payload["statements"], 1);
    assert_eq!(payload["ast"][0]["kind"], "Func");
}

#[test]
fn hir_inspection_resolves_frontmatter_locale_through_config_loader() {
    let source = "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio bis(numerus i) → numerus {\n    redde i * 2\n}\n";
    let result = super::hir_inspection_for_source_with_config("la.fab", source, &loader_only_config());
    assert!(result.success, "errors: {:?}", result.errors);
    let payload = json(&result.text);
    assert_eq!(payload["items"], 1);
}

#[test]
fn cli_ir_inspection_resolves_frontmatter_locale_through_config_loader() {
    let source = "+++\nlocale = \"la\"\n+++\nincipit {\n    nota \"salve\"\n}\n";
    let result = super::cli_ir_inspection_for_source_with_config("la.fab", source, &loader_only_config());
    assert!(result.success, "errors: {:?}", result.errors);
}

#[test]
fn lex_inspection_fails_closed_on_unresolvable_frontmatter_locale() {
    let source = "+++\nlocale = \"zh-Hant\"\n+++\nfunctio salve() { }\n";
    let result = super::lex_inspection_for_source_with_config("la.fab", source, &loader_only_config());
    assert!(!result.success, "an unloadable declared locale must not inspect");
    assert!(
        result.errors.iter().any(|error| error.contains("zh-Hant")),
        "the failure must name the declared locale: {:?}",
        result.errors
    );
}
