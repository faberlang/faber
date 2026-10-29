//! Focused tests for `faber test` selection and report parity.

#![cfg(feature = "runner")]

use super::*;
use clap::Parser;
use faber_cli::package::DeviceChoice;
use proba::{CaseOutcome, ProbaCase, ProbaModifierView};
use radix::module::hir::DefId;
use radix::module::lexer::Span;
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
    assert!(json["cases"][0]["gap"].is_null());
    assert_eq!(
        json["census"]["blocked"].as_array().unwrap(),
        &[] as &[serde_json::Value]
    );
    assert_eq!(
        json["census"]["gaps"].as_array().unwrap(),
        &[] as &[serde_json::Value]
    );
}

fn blocked_result(unit: &str, name: &str, suite: &str, reason: &str) -> CaseResult {
    CaseResult {
        unit_path: unit.into(),
        name: name.to_owned(),
        suite_path: suite.to_owned(),
        outcome: CaseOutcome::Blocked {
            reason: reason.to_owned(),
        },
        stdout: Vec::new(),
        stderr: Vec::new(),
        stats: None,
    }
}

#[test]
fn json_census_names_every_blocked_case_with_its_mir_gap() {
    let passed = CaseResult {
        outcome: CaseOutcome::Passed,
        ..blocked_result("src/math.proba", "ok", "math", "")
    };
    let method = blocked_result(
        "src/math.proba",
        "adds",
        "math",
        "unsupported MIR lowering: method call before runtime/provider MIR lowering (src/math.proba)",
    );
    let projection = blocked_result(
        "src/math.proba",
        "dot",
        "math",
        "unsupported MIR lowering: projection base that does not resolve to a local value (src/math.proba)",
    );
    let method_again = blocked_result(
        "src/vec.proba",
        "norm",
        "",
        "unsupported MIR lowering: method call before runtime/provider MIR lowering (src/vec.proba)",
    );
    let report = ProbaReport {
        results: vec![passed, method, projection, method_again],
    };
    let json = json_report_value(&report, &RunMetadata::default(), false);

    assert!(json["cases"][0]["gap"].is_null());
    assert_eq!(
        json["cases"][1]["gap"],
        "method call before runtime/provider MIR lowering"
    );
    assert_eq!(
        json["cases"][2]["gap"],
        "projection base that does not resolve to a local value"
    );
    assert_eq!(json["census"]["blocked"].as_array().unwrap().len(), 3);
    assert_eq!(json["census"]["blocked"][0]["name"], "adds");
    assert_eq!(
        json["census"]["blocked"][0]["gap"],
        "method call before runtime/provider MIR lowering"
    );

    let gaps = json["census"]["gaps"].as_array().unwrap();
    assert_eq!(gaps.len(), 2);
    let method_gap = gaps
        .iter()
        .find(|gap| gap["gap"] == "method call before runtime/provider MIR lowering")
        .expect("method-call gap");
    assert_eq!(method_gap["count"], 2);
    assert_eq!(method_gap["cases"].as_array().unwrap().len(), 2);
    let projection_gap = gaps
        .iter()
        .find(|gap| gap["gap"] == "projection base that does not resolve to a local value")
        .expect("projection gap");
    assert_eq!(projection_gap["count"], 1);
}

#[test]
fn blocking_gap_strips_prefixes_and_unit_path_suffix() {
    assert_eq!(
        blocking_gap(
            "unsupported MIR lowering: itera ex over tabula before map value iteration MIR lowering (src/t.proba)"
        ),
        "itera ex over tabula before map value iteration MIR lowering"
    );
    assert_eq!(
        blocking_gap("faberscript unsupported: provider package:./mathesis::mathesis::PI"),
        "faberscript unsupported: provider package:./mathesis::mathesis::PI"
    );
    assert_eq!(
        blocking_gap("missing type information for MIR lowering"),
        "missing type information for MIR lowering"
    );
    assert_eq!(
        blocking_gap("proba case \"x\" has no lowered MIR function (def #3)"),
        "no lowered MIR function"
    );
    assert_eq!(
        blocking_gap("unit failed to link/lower for runner proba execution (src/bad.proba)"),
        "unit failed to link/lower"
    );
    assert_eq!(
        blocking_gap("invalid MIR: named aggregate field type mismatch (src/material.proba)"),
        "named aggregate field type mismatch"
    );
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
        stats: None,
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

#[test]
fn cli_parses_device_choice_and_defaults_to_none() {
    let parse = |extra: &[&str]| {
        let mut argv = vec!["faber", "test", "."];
        argv.extend_from_slice(extra);
        let cli = crate::cli::Cli::try_parse_from(argv).expect("parse test command");
        let Some(crate::cli::Command::Test(args)) = cli.command else {
            panic!("expected test command");
        };
        args.device
    };
    assert_eq!(parse(&["--device", "cpu"]), Some(DeviceChoice::Cpu));
    assert_eq!(
        parse(&["--device", "reference"]),
        Some(DeviceChoice::Reference)
    );
    assert_eq!(parse(&["--device", "metal"]), Some(DeviceChoice::Metal));
    assert_eq!(parse(&[]), None);
}

#[test]
fn gpu_selection_refusal_covers_only_device_selections() {
    assert!(gpu_selection_refusal(DeviceSelection::Metal).is_some());
    assert!(gpu_selection_refusal(DeviceSelection::Cuda).is_some());
    assert!(gpu_selection_refusal(DeviceSelection::Cpu).is_none());
    assert!(gpu_selection_refusal(DeviceSelection::Reference).is_none());
    assert!(gpu_selection_refusal(DeviceSelection::Auto).is_none());
}

#[test]
fn only_the_cpu_and_reference_selections_execute_kernels() {
    assert_eq!(
        DeviceSelection::Cpu.kernel_policy(),
        RunnerKernelPolicy::Execute
    );
    assert_eq!(
        DeviceSelection::Reference.kernel_policy(),
        RunnerKernelPolicy::Execute
    );
    assert_eq!(
        DeviceSelection::Auto.kernel_policy(),
        RunnerKernelPolicy::Refuse
    );
    assert_eq!(
        TestRunOptions::default().kernel_policy,
        RunnerKernelPolicy::Refuse
    );
}
