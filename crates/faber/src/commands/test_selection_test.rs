//! Focused tests for `faber test` selection and report parity.

#![cfg(feature = "runner")]

use super::*;
use clap::Parser;
use proba::{CaseOutcome, ProbaCase, ProbaModifierView};
use radix_module::hir::DefId;
use radix_module::lexer::Span;
use std::path::Path;

fn case(name: &str, suite: &str, modifiers: Vec<ProbaModifierView>) -> ProbaCase {
    ProbaCase {
        def_id: DefId(0),
        name: name.to_owned(),
        suite_path: suite.to_owned(),
        span: Span::new(0, 1),
        modifiers,
    }
}

#[test]
fn repeatable_filters_use_or_semantics_and_skip_is_negation() {
    let first = case("alpha", "smoke", vec![]);
    let second = case("beta", "slow", vec![]);
    let options = TestRunOptions {
        filters: vec!["alpha".to_owned(), "beta".to_owned()],
        skips: vec!["beta".to_owned()],
        ..TestRunOptions::default()
    };

    assert!(case_matches_options(
        Path::new("src/math.proba"),
        &first,
        &options
    ));
    assert!(!case_matches_options(
        Path::new("src/math.proba"),
        &second,
        &options
    ));
}

#[test]
fn file_scoped_filter_matches_relative_path_and_case_name() {
    let selected = case("adds", "math", vec![]);
    let other = case("adds", "math", vec![]);
    let query = "src/math.proba::math/adds";

    assert!(case_query_matches(
        query,
        Path::new("/workspace/pkg/src/math.proba"),
        &selected,
        false
    ));
    assert!(!case_query_matches(
        "src/other.proba::math/adds",
        Path::new("/workspace/pkg/src/math.proba"),
        &other,
        false
    ));
}

#[test]
fn repeatable_suite_and_tag_values_use_or_semantics() {
    let selected = case(
        "case",
        "nested/suite",
        vec![ProbaModifierView::Tag("gpu".to_owned())],
    );
    let options = TestRunOptions {
        suites: vec!["unit".to_owned(), "nested/suite".to_owned()],
        tags: vec!["slow".to_owned(), "gpu".to_owned()],
        ..TestRunOptions::default()
    };

    assert!(case_matches_options(
        Path::new("case.proba"),
        &selected,
        &options
    ));
}

#[test]
fn list_report_does_not_execute_and_json_contains_duration() {
    let cases = vec![case("listed", "", vec![])];
    let report = list_report(Path::new("src/cases.proba"), &cases);
    assert_eq!(report.results.len(), 1);
    assert!(matches!(
        report.results[0].outcome,
        CaseOutcome::Skipped { .. }
    ));

    let mut metadata = RunMetadata::default();
    metadata
        .durations_ms
        .insert(result_key(&report.results[0]), 17);
    let json = json_report_value(&report, &metadata, true);
    assert_eq!(json["cases"][0]["status"], "listed");
    assert_eq!(json["cases"][0]["duration_ms"], 17);
}

#[test]
fn fail_fast_stops_on_failed_or_blocked_outcomes() {
    let failed = CaseResult {
        unit_path: "case.proba".into(),
        name: "broken".to_owned(),
        suite_path: String::new(),
        outcome: CaseOutcome::Failed {
            message: "no".to_owned(),
        },
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    let blocked = CaseResult {
        outcome: CaseOutcome::Blocked {
            reason: "unsupported".to_owned(),
        },
        ..failed.clone()
    };
    assert!(result_is_failure(&failed));
    assert!(result_is_failure(&blocked));
}

#[test]
fn cli_parses_selection_and_reporting_flags() {
    let cli = crate::cli::Cli::try_parse_from([
        "faber",
        "test",
        ".",
        "--filter",
        "alpha",
        "--filter",
        "beta",
        "--skip",
        "slow",
        "--suite",
        "unit",
        "--suite",
        "integration",
        "--tag",
        "fast",
        "--tag",
        "gpu",
        "--list",
        "--format",
        "json",
        "--fail-fast",
    ])
    .expect("parse selection flags");
    let Some(crate::cli::Command::Test(args)) = cli.command else {
        panic!("expected test command");
    };
    assert_eq!(args.filter_flag, ["alpha", "beta"]);
    assert_eq!(args.skip, ["slow"]);
    assert_eq!(args.suite, ["unit", "integration"]);
    assert_eq!(args.tag, ["fast", "gpu"]);
    assert!(args.list);
    assert_eq!(args.format, crate::cli::TestOutputFormat::Json);
    assert!(args.fail_fast);
}
