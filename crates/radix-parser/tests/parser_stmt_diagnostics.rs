//! Issue-identity proof for statement-parser token contracts.
//!
//! Each case parses the smallest source that breaks one punctuation contract
//! and asserts the `PARSE001` diagnostic names its stable issue instead of
//! carrying English prose. Rendering belongs to the reader packs; the pack
//! check here only proves every locale ships a row for the issue.

use radix_lexer::lex;
use radix_parser::{ParseError, ParseErrorKind, parse};
use std::fs;
use std::path::PathBuf;

const LOCALES: [&str; 8] = ["en", "la", "ar", "hi", "vi", "th-TH", "zh-Hans", "zh-Hant"];

/// Minimal broken source per statement-level token contract.
const STMT_CASES: [(&str, &str); 8] = [
    ("apud_coordinates_start", "itera ex m apud sit cell { }"),
    ("apud_coordinates_end", "itera ex m apud [i sit cell { }"),
    ("elige_body_start", "elige x"),
    ("elige_body_end", "elige x { casu 1 { }"),
    ("discerne_body_start", "discerne x"),
    ("discerne_body_end", "discerne x { ceterum { }"),
    ("custodi_body_start", "custodi"),
    ("custodi_body_end", "custodi { si 1 { }"),
];

fn issue_of(error: &ParseError) -> Option<&str> {
    error
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.as_str())
}

fn pack_issue_headers(locale: &str) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../locale")
        .join(locale)
        .join("pack.toml");
    let text = fs::read_to_string(&path).expect("reader pack is readable");
    text.lines()
        .filter(|line| line.starts_with("[diagnostics.PARSE001.issues."))
        .map(str::to_owned)
        .collect()
}

#[test]
fn statement_token_contracts_name_their_issue() {
    for (slug, source) in STMT_CASES {
        let result = parse(lex(source));
        let error = result
            .errors
            .iter()
            .find(|error| issue_of(error) == Some(slug))
            .unwrap_or_else(|| {
                panic!(
                    "{slug}: expected a PARSE001 issue for {source:?}, got {:?}",
                    result.errors
                )
            });
        assert_eq!(error.kind, ParseErrorKind::Expected, "{slug}");
        assert!(
            error.args.iter().all(|arg| arg.name != "expected"),
            "{slug}: the issue must not also carry an expected-prose argument"
        );
        assert!(
            error.args.iter().all(|arg| arg.name != "legacy_message"),
            "{slug}: the issue must not carry a legacy message"
        );
    }
}

#[test]
fn statement_issue_slugs_are_unique() {
    let mut slugs: Vec<&str> = STMT_CASES.iter().map(|(slug, _)| *slug).collect();
    slugs.sort_unstable();
    slugs.dedup();
    assert_eq!(slugs.len(), STMT_CASES.len());
}

#[test]
fn statement_issue_rows_exist_in_all_locale_packs() {
    for locale in LOCALES {
        let headers = pack_issue_headers(locale);
        for (slug, _) in STMT_CASES {
            let header = format!("[diagnostics.PARSE001.issues.{slug}]");
            assert!(
                headers.contains(&header),
                "{locale}.{slug} must have an explicit PARSE001 issue row"
            );
        }
    }
}
