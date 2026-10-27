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
