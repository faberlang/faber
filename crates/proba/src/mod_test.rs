//! Unit tests for proba inventory and MIR-runner case runner.

use super::*;
use radix::mir::RunnerError;
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
fn declared_error_conversion_propagates_through_mir_runner() {
    let report = run(
        r#"
discretio Sourceum { Failed }
discretio Destinatio { @ conversio Sourceum err { redde finge Converted {} ∷ Destinatio } Converted }
functio source() → vacuum ⇥ Sourceum { iace finge Failed {} ∷ Sourceum }
functio target() → vacuum ⇥ Destinatio { source() }
proba "conversion" {
    fac { target() }
    cape err { adfirma err ≡ finge Converted {} ∷ Destinatio }
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.passed(), 1, "report: {report:?}");
    assert!(report.success(), "report: {report:?}");
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
    let lowered = super::lower_analyzed_unit(&mut analysis, Default::default()).expect("lower");
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

#[test]
fn temporis_runaway_loop_exhausts_step_budget() {
    // Without the budget this `dum verum` case hangs the runner forever.
    let report = run(
        r#"
proba "runaway" temporis 64 {
    dum verum {
        tacet
    }
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.passed(), 0);
    assert_eq!(report.failed(), 1);
    assert_eq!(report.blocked(), 0);
    match &report.results[0].outcome {
        CaseOutcome::Failed { message } => {
            assert!(
                message.contains("temporis"),
                "budget exhaustion should name temporis: {message}"
            );
            assert!(
                message.contains("step budget exhausted"),
                "budget exhaustion should name the step budget: {message}"
            );
            assert!(
                message.contains("64"),
                "budget exhaustion should name the limit: {message}"
            );
        }
        other => panic!("expected Failed budget exhaustion, got {other:?}"),
    }
}

#[test]
fn temporis_sufficient_budget_still_passes() {
    let report = run(
        r#"
proba "bounded" temporis 10000 {
    adfirma 1 ≡ 1
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.passed(), 1);
    assert_eq!(report.failed(), 0);
    assert!(report.success());
}

#[test]
fn repete_runs_and_passes() {
    let report = run(
        r#"
proba "repeat me" repete 3 {
    adfirma 1 ≡ 1
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.passed(), 1);
    assert_eq!(report.failed(), 0);
    assert!(report.success());
}

#[test]
fn repete_still_fails_on_assertion() {
    let report = run(
        r#"
proba "always broken" repete 3 {
    adfirma 1 ≡ 2
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.passed(), 0);
    assert_eq!(report.failed(), 1);
    assert!(!report.success());
}

#[test]
fn fragilis_is_rejected_and_body_is_not_executed() {
    let report = run(
        r#"
proba "flaky" fragilis 3 {
    adfirma 1 ≡ 2
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.failed(), 1);
    assert_eq!(report.passed(), 0);
    match &report.results[0].outcome {
        CaseOutcome::Failed { message } => {
            assert!(
                message.contains("fragilis is rejected"),
                "fragilis must fail closed instead of silently running: {message}"
            );
            assert!(
                !message.contains("1 ≡ 2") && !message.contains("adfirma"),
                "rejected fragilis must not execute the assertion: {message}"
            );
        }
        other => panic!("expected Failed rejection, got {other:?}"),
    }
}

#[test]
fn metior_is_rejected() {
    let report = run(
        r#"
proba "bench" metior {
    adfirma 1 ≡ 1
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.failed(), 1);
    match &report.results[0].outcome {
        CaseOutcome::Failed { message } => {
            assert!(
                message.contains("metior is rejected"),
                "metior must fail closed instead of silently running: {message}"
            );
        }
        other => panic!("expected Failed rejection, got {other:?}"),
    }
}

#[test]
fn solum_in_skips_when_environment_does_not_match() {
    let report = run(
        r#"
proba "gated" solum_in "proba-u10-absent-env" {
    adfirma 1 ≡ 2
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.skipped(), 1);
    assert_eq!(report.failed(), 0);
    assert!(report.success());
    match &report.results[0].outcome {
        CaseOutcome::Skipped { reason } => {
            assert!(
                reason.contains("solum_in"),
                "skip reason should name solum_in: {reason}"
            );
            assert!(
                reason.contains("proba-u10-absent-env"),
                "skip reason should name the required environment: {reason}"
            );
        }
        other => panic!("expected Skipped, got {other:?}"),
    }
}

#[test]
fn solum_in_match_helper_covers_env_os_and_var() {
    assert!(super::solum_in_matches_in(
        "ci",
        Some("ci"),
        "linux",
        |_| None
    ));
    assert!(!super::solum_in_matches_in("ci", None, "linux", |_| None));
    assert!(super::solum_in_matches_in("linux", None, "linux", |_| None));
    assert!(super::solum_in_matches_in("ci", None, "macos", |name| {
        (name == "CI").then(|| "true".to_owned())
    }));
    assert!(super::solum_in_matches_in("CI", None, "macos", |name| {
        (name == "CI").then(|| "1".to_owned())
    }));
}

#[test]
fn erratur_clean_pass_fails_strictly() {
    let report = run(
        r#"
proba "unexpected pass" erratur {
    adfirma 1 ≡ 1
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.passed(), 0);
    assert_eq!(report.failed(), 1);
    match &report.results[0].outcome {
        CaseOutcome::Failed { message } => assert!(message.contains("erratur")),
        other => panic!("expected strict erratur failure, got {other:?}"),
    }
}

#[test]
fn erratur_is_carried_into_inventory() {
    let analysis = analyze_prepared_program_name(
        &session(),
        "erratur_inventory.fab",
        r#"
proba "expected" erratur {
    adfirma 1 ≡ 1
}
"#,
    )
    .expect("analyze");
    let cases = inventory_cases(&analysis.hir, &analysis.interner);
    assert!(matches!(
        cases[0].modifiers.as_slice(),
        [ProbaModifierView::Erratur]
    ));
}

#[test]
fn erratur_does_not_flip_fragilis_rejection() {
    let report = run(
        r#"
proba "rejected flaky" erratur fragilis 1 {
    adfirma 1 ≡ 1
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.failed(), 1);
    assert!(matches!(
        report.results[0].outcome,
        CaseOutcome::Failed { .. }
    ));
    assert!(!report.success());
}

#[test]
fn erratur_does_not_flip_metior_rejection() {
    let report = run(
        r#"
proba "rejected benchmark" erratur metior {
    adfirma 1 ≡ 1
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.failed(), 1);
    assert!(matches!(
        report.results[0].outcome,
        CaseOutcome::Failed { .. }
    ));
    assert!(!report.success());
}

#[test]
fn erratur_does_not_flip_temporis_budget_exhaustion() {
    let report = run(
        r#"
proba "budget" erratur temporis 64 {
    dum verum {
        tacet
    }
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.failed(), 1);
    match &report.results[0].outcome {
        CaseOutcome::Failed { message } => assert!(message.contains("temporis")),
        other => panic!("expected budget failure, got {other:?}"),
    }
}

#[test]
fn erratur_does_not_flip_assertion_abort() {
    let report = run(
        r#"
proba "assertion" erratur {
    adfirma 1 ≡ 2
}
"#,
        TestSelection::default(),
    );
    assert_eq!(report.failed(), 1);
    assert!(matches!(
        report.results[0].outcome,
        CaseOutcome::Failed { .. }
    ));
    assert!(!report.success());
}

#[test]
fn erratur_flips_only_uncaught_error_channel_escapes() {
    let escaped = RunnerError::internal(
        radix::lexer::Span::default(),
        "uncaught alternate return from call",
    );
    assert!(matches!(
        outcome_from_run_result(Err(vec![escaped]), true),
        CaseOutcome::Passed
    ));

    let unrelated = RunnerError::internal(radix::lexer::Span::default(), "host failed");
    assert!(matches!(
        outcome_from_run_result(Err(vec![unrelated]), true),
        CaseOutcome::Failed { .. }
    ));
}

#[test]
fn erratur_flips_before_repete_short_circuit() {
    let mut executions = 0;
    let execution = execute_repeated(3, || {
        executions += 1;
        CaseExecution::without_output(outcome_from_run_result(
            Err(vec![RunnerError::internal(
                radix::lexer::Span::default(),
                "uncaught alternate return from call",
            )]),
            true,
        ))
    });

    assert_eq!(executions, 3);
    assert!(matches!(execution.outcome, CaseOutcome::Passed));
}
