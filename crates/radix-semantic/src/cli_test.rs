//! Mounted command modules report each CLI validation error once.

use super::{CliAnalysis, analyze_mounted_module};
use crate::test_support::issue;
use radix_lexer::lex;
use radix_parser::parse;

fn analyze_mounted(source: &str, prefix: &[&str]) -> CliAnalysis {
    let parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let prefix: Vec<String> = prefix.iter().map(|part| (*part).to_owned()).collect();
    analyze_mounted_module(
        parsed.program.as_ref().expect("program"),
        &parsed.interner,
        &prefix,
    )
}

fn issues(analysis: &CliAnalysis) -> Vec<&str> {
    analysis
        .errors
        .iter()
        .filter_map(|err| issue(err))
        .collect()
}

#[test]
fn mounted_duplicate_short_flag_is_reported_once() {
    let analysis = analyze_mounted(
        r#"
@ imperium "jobs"
@ optio left brevis "x"
@ optio right brevis "x"
functio jobs() argumenta args {}
"#,
        &["tool"],
    );

    let found = issues(&analysis);
    assert_eq!(
        found
            .iter()
            .filter(|name| **name == "duplicate_short_flag")
            .count(),
        1,
        "{found:?}"
    );
}

#[test]
fn mounted_duplicate_binding_is_reported_once() {
    let analysis = analyze_mounted(
        r#"
@ imperium "jobs"
@ optio same longum "left"
@ optio same longum "right"
functio jobs() argumenta args {}
"#,
        &["tool"],
    );

    let found = issues(&analysis);
    assert_eq!(
        found
            .iter()
            .filter(|name| **name == "duplicate_binding")
            .count(),
        1,
        "{found:?}"
    );
}

#[test]
fn mounted_alias_path_is_still_rejected_before_prefixing() {
    let analysis = analyze_mounted(
        r#"
@ imperium "jobs"
@ alias "a/b"
functio jobs() argumenta args {}
"#,
        &["tool"],
    );

    assert!(
        issues(&analysis).contains(&"invalid_command_alias"),
        "{:?}",
        issues(&analysis)
    );
}
