//! Unit tests for proba inventory and MIR-runner case runner.

use super::*;
use radix::module::{Config, Session};
use radix::program::analyze_prepared_program_name;
use std::path::PathBuf;

fn session() -> Session {
    Session::new(Config::default())
}

fn run(source: &str, selection: TestSelection) -> ProbaReport {
    run_proba_source(
        &session(),
        "proba_test.fab",
        source,
        &selection,
        None,
        false,
        false,
    )
    .unwrap_or_else(|err| panic!("proba run should succeed setup: {err:?}"))
}

#[test]
fn passing_adfirma_reports_pass() {
    let report = run(
        r#"
proba "one equals one" {
    adfirma 1 ≡ 1
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.passed(), 1);
    assert_eq!(report.failed(), 0);
    assert_eq!(report.blocked(), 0);
    assert_eq!(report.skipped(), 0);
    assert!(report.success());
    assert_eq!(report.results[0].name, "one equals one");
    assert!(matches!(report.results[0].outcome, CaseOutcome::Passed));
}

#[test]
fn captures_case_stdout_and_stderr() {
    let report = run(
        r#"
proba "output" {
    nota "stdout line"
    mone "stderr line"
    adfirma 1 ≡ 1
}
"#,
        TestSelection::default(),
    );
    let case = &report.results[0];
    assert_eq!(case.stdout, vec!["stdout line"]);
    assert_eq!(case.stderr, vec!["stderr line"]);
}

#[test]
fn report_attributes_case_to_source_unit() {
    let report = run_proba_source(
        &session(),
        "rel/path.proba",
        r#"
probandum "suite" {
    proba "name" {
        adfirma 1 ≡ 1
    }
}
"#,
        &TestSelection::default(),
        None,
        false,
        false,
    )
    .expect("run");

    let case = &report.results[0];
    assert_eq!(case.unit_path, PathBuf::from("rel/path.proba"));
    assert_eq!(case.display_path(), "suite/name");
    assert_eq!(
        format!("{}::{}", case.unit_path.display(), case.display_path()),
        "rel/path.proba::suite/name"
    );
}

#[test]
fn failing_adfirma_reports_fail_with_case_identity() {
    let report = run(
        r#"
proba "broken equality" {
    adfirma 1 ≡ 2
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.passed(), 0);
    assert_eq!(report.failed(), 1);
    assert_eq!(report.blocked(), 0);
    assert!(!report.success());
    assert_eq!(report.results[0].name, "broken equality");
    match &report.results[0].outcome {
        CaseOutcome::Failed { message } => {
            assert!(
                !message.is_empty(),
                "failure message should be non-empty: {message:?}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn omitte_case_is_skipped_not_executed() {
    let report = run(
        r#"
proba "blocked" omitte "not ready" {
    adfirma 1 ≡ 2
}
proba "ok" {
    adfirma 1 ≡ 1
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.passed(), 1);
    assert_eq!(report.failed(), 0);
    assert_eq!(report.skipped(), 1);
    assert!(report.success());
    let skip = report
        .results
        .iter()
        .find(|r| r.name == "blocked")
        .expect("skipped case");
    match &skip.outcome {
        CaseOutcome::Skipped { reason } => {
            assert!(
                reason.contains("omitte"),
                "skip reason should mention omitte: {reason}"
            );
        }
        other => panic!("expected Skipped, got {other:?}"),
    }
}

#[test]
fn futurum_case_is_skipped() {
    let report = run(
        r#"
proba "later" futurum "v2" {
    adfirma 0 ≡ 1
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.skipped(), 1);
    assert_eq!(report.failed(), 0);
    assert!(report.success());
}

#[test]
fn nested_suite_path_and_name_filter() {
    let source = r#"
probandum "outer" {
    probandum "inner" {
        proba "nested case" {
            adfirma 2 ≡ 2
        }
    }
    proba "sibling" {
        adfirma 3 ≡ 3
    }
}
"#;
    let all = run(source, TestSelection::default());
    assert_eq!(all.results.len(), 2);
    let nested = all
        .results
        .iter()
        .find(|r| r.name == "nested case")
        .expect("nested");
    assert_eq!(nested.suite_path, "outer/inner");
    assert_eq!(nested.display_path(), "outer/inner/nested case");

    let filtered = run(
        source,
        TestSelection {
            name: Some("nested case".to_owned()),
            suite: None,
            tag: None,
        },
    );
    assert_eq!(filtered.results.len(), 1);
    assert_eq!(filtered.results[0].name, "nested case");
    assert!(filtered.success());

    let suite_filtered = run(
        source,
        TestSelection {
            name: None,
            suite: Some("outer/inner".to_owned()),
            tag: None,
        },
    );
    assert_eq!(suite_filtered.results.len(), 1);
    assert_eq!(suite_filtered.results[0].name, "nested case");
}

#[test]
fn tag_selection_filters_cases() {
    let report = run(
        r#"
proba "tagged" tag "focus" {
    adfirma 1 ≡ 1
}
proba "other" {
    adfirma 1 ≡ 1
}
"#,
        TestSelection {
            name: None,
            suite: None,
            tag: Some("focus".to_owned()),
        },
    );
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].name, "tagged");
}

#[test]
fn inventory_lists_cases_without_execution() {
    let analysis = analyze_prepared_program_name(
        &session(),
        "inv.fab",
        r#"
proba "a" omitte "x" { adfirma 1 ≡ 1 }
probandum "s" {
    proba "b" { adfirma 1 ≡ 1 }
}
"#,
    )
    .expect("analyze");
    let cases = inventory_cases(&analysis.hir, &analysis.interner);
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].name, "a");
    assert!(matches!(
        cases[0].modifiers[0],
        ProbaModifierView::Omitte(_)
    ));
    assert_eq!(cases[1].name, "b");
    assert_eq!(cases[1].suite_path, "s");
}

#[test]
fn harness_filter_substring_selects_by_path() {
    let report = run_proba_source(
        &session(),
        "filter.fab",
        r#"
probandum "math" {
    proba "add" { adfirma 1 ≡ 1 }
    proba "mul" { adfirma 2 ≡ 2 }
}
"#,
        &TestSelection::default(),
        Some("math/add"),
        false,
        false,
    )
    .expect("run");
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].name, "add");
}

fn report_with_missing_mir_function() -> ProbaReport {
    let mut analysis = analyze_prepared_program_name(
        &session(),
        "blocked.fab",
        r#"
proba "ok" {
    adfirma 1 ≡ 1
}
"#,
    )
    .expect("analyze");
    let cases = inventory_cases(&analysis.hir, &analysis.interner);
    let lowered = super::lower_analyzed_unit_with_context(&mut analysis).expect("lower");
    let mut ghost = cases[0].clone();
    ghost.def_id = super::DefId(u32::MAX);
    ghost.name = "ghost".to_owned();
    run_proba_on_validated(
        &lowered.validated,
        &[ghost],
        &TestSelection::default(),
        None,
        false,
        false,
    )
}

#[test]
fn missing_mir_function_is_blocked_not_failed() {
    let report = report_with_missing_mir_function();
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.passed(), 0);
    assert_eq!(report.failed(), 0);
    assert_eq!(report.blocked(), 1);
    assert_eq!(report.skipped(), 0);
    assert!(!report.success());
    match &report.results[0].outcome {
        CaseOutcome::Blocked { reason } => {
            assert!(
                reason.contains("no lowered MIR function"),
                "blocked reason should name the missing MIR function: {reason}"
            );
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
}

#[test]
fn blocked_counts_separately_from_failed_and_summary_does_not_claim_fail() {
    let failed = run(
        r#"
proba "broken equality" {
    adfirma 1 ≡ 2
}
"#,
        TestSelection::default(),
    );
    let mut report = report_with_missing_mir_function();
    report.results.extend(failed.results);

    assert_eq!(report.blocked(), 1);
    assert_eq!(report.failed(), 1);
    assert_eq!(report.passed(), 0);
    assert!(!report.success());
    assert!(
        !matches!(report.results[0].outcome, CaseOutcome::Failed { .. }),
        "blocked case must not render as Failed: {:?}",
        report.results[0].outcome
    );

    let mixed = format_summary(&report);
    assert!(
        mixed.contains("1 failed"),
        "mixed summary should count the real failure: {mixed}"
    );
    assert!(
        mixed.contains("1 blocked"),
        "mixed summary should count blocked separately: {mixed}"
    );

    let blocked_only = format_summary(&report_with_missing_mir_function());
    assert!(
        blocked_only.starts_with("test result: BLOCKED."),
        "blocked-only summary must not claim FAILED: {blocked_only}"
    );
    assert!(
        blocked_only.contains("0 failed"),
        "blocked-only summary must report zero failures: {blocked_only}"
    );
    assert!(
        blocked_only.contains("1 blocked"),
        "blocked-only summary must count the blocked case: {blocked_only}"
    );
    assert!(
        !blocked_only.contains("FAILED"),
        "a blocked case that never ran must not render as a failed test: {blocked_only}"
    );
}
