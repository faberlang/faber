//! Unit tests for proba inventory and MIR-runner case runner.

use super::*;
use radix_module::driver::{Config, Session};

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
    assert_eq!(report.skipped(), 0);
    assert!(report.success());
    assert_eq!(report.results[0].name, "one equals one");
    assert!(matches!(report.results[0].outcome, CaseOutcome::Passed));
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
    let analysis = analyze_source(
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
