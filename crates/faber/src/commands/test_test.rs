//! Companion tests for `faber test` (runner proba runner).
//!
//! WHY: the hygiene ratchet bans inline `#[cfg(test)] mod tests` in production
//! sources; tests live in dedicated `*_test.rs` files.

#![cfg(feature = "runner")]

use proba::{TestSelection, run_proba_source};
use radix::module::driver::{Config, Session};
use std::fs;
use std::path::PathBuf;

fn scratch_dir() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-runner-faber");
    let _ = fs::create_dir_all(&base);
    base
}

#[test]
fn runner_runner_passes_simple_adfirma_fixture() {
    let dir = scratch_dir();
    let path = dir.join("pass.fab");
    fs::write(
        &path,
        r#"
proba "pass case" {
    adfirma 1 ≡ 1
}
"#,
    )
    .expect("write");
    let session = Session::new(Config::default().with_latin_locale());
    let source = fs::read_to_string(&path).expect("read");
    let report = run_proba_source(
        &session,
        &path.to_string_lossy(),
        &source,
        &TestSelection::default(),
        None,
        false,
        false,
        None,
        false,
    )
    .expect("run");
    assert!(report.success(), "{report:?}");
    assert_eq!(report.passed(), 1);
}

#[test]
fn runner_runner_fails_named_case() {
    let dir = scratch_dir();
    let path = dir.join("fail.fab");
    fs::write(
        &path,
        r#"
proba "the broken one" {
    adfirma 1 ≡ 2
}
"#,
    )
    .expect("write");
    let session = Session::new(Config::default().with_latin_locale());
    let source = fs::read_to_string(&path).expect("read");
    let report = run_proba_source(
        &session,
        &path.to_string_lossy(),
        &source,
        &TestSelection::default(),
        None,
        false,
        false,
        None,
        false,
    )
    .expect("run");
    assert!(!report.success());
    assert_eq!(report.results[0].name, "the broken one");
}

#[test]
fn cmd_test_source_has_no_cargo_or_rust_executor() {
    // Guard: reintroducing Cargo / Target::HirRust as the test executor must fail CI.
    // Inspect only production code (exclude any test module, which names banned APIs).
    let source = include_str!("test.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    assert!(
        !production.contains("invoke_cargo_test"),
        "cmd_test must not call invoke_cargo_test"
    );
    assert!(
        !production.contains("Target::HirRust"),
        "cmd_test must not select Target::HirRust for execution"
    );
    assert!(
        !production.contains("emit_generated_crate"),
        "cmd_test must not emit a generated test crate"
    );
    assert!(
        production.contains("run_proba_on_validated"),
        "cmd_test must drive the runner proba runner"
    );
}

#[test]
fn package_analysis_failures_render_file_line_and_message() {
    use radix::module::diagnostics::{Diagnostic, Span};

    let error = Diagnostic::error("unknown identifier")
        .with_code("SEM001")
        .with_arg("issue", "unknown_identifier")
        .with_file("src/gradus.fab")
        .with_span(Span::new(42, 50))
        .with_source_line("    nomen missing");
    let check_line = radix::module::diagnostics::render_plain(&error);
    let rendered = super::render_package_analysis_diagnostics(
        std::slice::from_ref(&error),
        radix::services::DiagnosticMode::Normal,
        None,
    );

    assert_eq!(rendered, check_line);
    assert!(
        rendered.contains("src/gradus.fab"),
        "analysis errors must name the file the way faber check does:\n{rendered}"
    );
    assert!(
        rendered.contains("42"),
        "analysis errors must carry the source span:\n{rendered}"
    );
    assert!(
        rendered.contains("nomen missing"),
        "analysis errors must quote the source line:\n{rendered}"
    );
}

#[test]
fn package_analysis_repeated_warning_codes_collapse_to_a_count() {
    use radix::module::diagnostics::{Diagnostic, Span};

    let error = Diagnostic::error("undefined name")
        .with_code("SEM008")
        .with_arg("issue", "undefined_name")
        .with_file("src/lib.fab")
        .with_span(Span::new(12, 20))
        .with_source_line("    nomen x");
    let unique = Diagnostic::warning("one-off locale hint")
        .with_code("LOCALE001")
        .with_file("src/unique.fab")
        .with_span(Span::new(1, 4));
    let diagnostics = vec![
        Diagnostic::warning("spelling")
            .with_code("LOCALE002")
            .with_file("src/a.fab"),
        Diagnostic::warning("spelling")
            .with_code("LOCALE002")
            .with_file("src/b.fab"),
        Diagnostic::warning("spelling")
            .with_code("LOCALE002")
            .with_file("src/c.fab"),
        unique.clone(),
        error.clone(),
    ];
    let rendered = super::render_package_analysis_diagnostics(
        &diagnostics,
        radix::services::DiagnosticMode::Normal,
        None,
    );

    assert!(
        rendered.contains(&radix::module::diagnostics::render_plain(&error)),
        "errors must keep the faber-check plain renderer:\n{rendered}"
    );
    assert!(
        rendered.contains(&radix::module::diagnostics::render_plain(&unique)),
        "a unique warning still renders in full:\n{rendered}"
    );
    assert!(
        rendered.contains("3 occurrences"),
        "repeated warning codes collapse to a counted summary:\n{rendered}"
    );
    assert_eq!(
        rendered.matches("LOCALE002").count(),
        1,
        "do not dump every raw LOCALE002 line:\n{rendered}"
    );
    assert!(
        !rendered.contains("src/a.fab") && !rendered.contains("src/b.fab"),
        "collapsed warnings must not keep per-file lines:\n{rendered}"
    );
}

#[test]
fn cmd_test_routes_through_package_discovery() {
    let source = include_str!("test.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    assert!(
        production.contains("test_input_route"),
        "cmd_test must select the package vs single-file route via discovery"
    );
    assert!(
        production.contains("MANIFEST_LESS_TEST_MESSAGE"),
        "cmd_test must emit the guiding no-faber.toml diagnostic"
    );
    assert!(
        production.contains("no_package"),
        "cmd_test must honor --no-package as the bare-file escape"
    );
}

#[test]
fn empty_run_is_nonzero_without_allow_no_tests() {
    assert_eq!(super::empty_run_exit_code(false), 1);
    assert_eq!(super::empty_run_exit_code(true), 0);
}

#[test]
fn empty_run_messages_name_the_filter_vs_discovery_case() {
    assert_eq!(
        super::empty_run_detail(true),
        "--include matched no *.proba sources"
    );
    assert_eq!(super::empty_run_detail(false), "no proba cases discovered");
}

#[test]
fn unmatched_include_is_error_when_only_inline_fab_units_are_present() {
    let units = [PathBuf::from("src/main.fab")];
    assert!(super::unmatched_include_filter(
        &["zzz-no-such-file".to_owned()],
        &units
    ));
}

#[test]
fn include_that_loaded_a_proba_unit_is_not_unmatched() {
    let units = [
        PathBuf::from("src/main.fab"),
        PathBuf::from("src/math.proba"),
    ];
    assert!(!super::unmatched_include_filter(
        &["math".to_owned()],
        &units
    ));
}

#[test]
fn empty_include_is_never_an_unmatched_include() {
    let units = [PathBuf::from("src/main.fab")];
    assert!(!super::unmatched_include_filter(&[], &units));
}

#[test]
fn cli_parses_allow_no_tests() {
    use clap::Parser;
    let cli = crate::cli::Cli::try_parse_from(["faber", "test", ".", "--allow-no-tests"])
        .expect("parse --allow-no-tests");
    let Some(crate::cli::Command::Test(args)) = cli.command else {
        panic!("expected test subcommand");
    };
    assert!(args.allow_no_tests);
}

#[test]
fn cli_parses_focused_flag() {
    use clap::Parser;
    let cli = crate::cli::Cli::try_parse_from(["faber", "test", ".", "--focused"])
        .expect("parse --focused");
    let Some(crate::cli::Command::Test(args)) = cli.command else {
        panic!("expected test subcommand");
    };
    assert!(args.focused);
}

#[test]
fn package_solum_focus_applies_to_every_test_unit() {
    let root = scratch_dir().join("u8-package-wide-solum");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("fixture src");
    fs::write(
        root.join("faber.toml"),
        r#"[package]
name = "u8_package_wide_solum"
version = "0.1.0"
edition = "2026"

[library]
provider = "u8_package_wide_solum"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("fixture manifest");
    fs::write(
        root.join("src/a.proba"),
        "test \"focused\" only {\n    assert 1 ≡ 1\n}\n",
    )
    .expect("focused unit");
    fs::write(
        root.join("src/b.proba"),
        "test \"plain\" {\n    assert 1 ≡ 2\n}\n",
    )
    .expect("plain unit");

    let (report, unmatched_include, _metadata) = super::run_package_with_options(
        &Config::default(),
        &root,
        &super::TestRunOptions::default(),
        None,
        false,
        false,
    )
    .expect("package analysis succeeds");

    assert!(!unmatched_include);
    assert_eq!(
        report.results.len(),
        1,
        "only focused case should run: {report:?}"
    );
    assert_eq!(report.passed(), 1, "focused case should pass: {report:?}");
    assert_eq!(report.failed(), 0, "plain unit must not run: {report:?}");
    assert_eq!(report.blocked(), 0, "plain unit must not link: {report:?}");
    assert_eq!(report.results[0].name, "focused");
}

#[test]
fn package_analysis_failure_returns_err_instead_of_exiting() {
    let root = scratch_dir().join("sb442-analysis-failure");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("fixture src");
    fs::write(
        root.join("faber.toml"),
        r#"[package]
name = "sb442_analysis_failure"
version = "0.1.0"
edition = "2026"

[library]
provider = "sb442_analysis_failure"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("fixture manifest");
    fs::write(
        root.join("src/broken.proba"),
        "test \"broken\" {\n    assert undefined_name ≡ 1\n}\n",
    )
    .expect("analysis-failing unit");

    let result = super::run_package_with_options(
        &Config::default(),
        &root,
        &super::TestRunOptions::default(),
        None,
        false,
        false,
    );

    let message = result.expect_err("analysis failure must be an Err, not a process exit");
    assert!(
        message.rendered.ends_with("error: package analysis failed"),
        "summary line closes the message: {message}"
    );
}

#[test]
fn package_test_degrades_per_unit_when_one_unit_cannot_lower() {
    let root = scratch_dir().join("e10-two-unit");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("fixture src");
    fs::write(
        root.join("faber.toml"),
        r#"[package]
name = "e10_two_unit"
version = "0.1.0"
edition = "2026"

[library]
provider = "e10_two_unit"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("fixture manifest");
    fs::write(
        root.join("src/good.proba"),
        "test \"green\" {\n    assert 1 ≡ 1\n}\n",
    )
    .expect("green unit");
    fs::write(
        root.join("src/bad.proba"),
        "fn unsupported(matrix<f32, [2, 3]> m) → any {\n    return m.invert()\n}\n\ntest \"blocked\" {\n    assert 1 ≡ 1\n}\n",
    )
    .expect("unsupported unit");

    let (report, unmatched_include, _metadata) = super::run_package_with_options(
        &Config::default(),
        &root,
        &super::TestRunOptions::default(),
        None,
        false,
        false,
    )
    .expect("package analysis succeeds");

    assert!(!unmatched_include);
    assert_eq!(
        report.passed(),
        1,
        "green unit must still execute: {report:?}"
    );
    assert_eq!(
        report.failed(),
        0,
        "link degradation is not a test failure: {report:?}"
    );
    assert_eq!(
        report.blocked(),
        1,
        "only the unsupported unit is blocked: {report:?}"
    );
    assert!(report.results.iter().any(|result| {
        result.name == "green" && matches!(result.outcome, proba::CaseOutcome::Passed)
    }));
    assert!(report.results.iter().any(|result| {
        result.name == "blocked" && matches!(result.outcome, proba::CaseOutcome::Blocked { .. })
    }));
    let blocked = report
        .results
        .iter()
        .find(|result| result.name == "blocked")
        .expect("blocked case");
    let proba::CaseOutcome::Blocked { reason } = &blocked.outcome else {
        panic!("expected blocked outcome: {blocked:?}");
    };
    let gap = super::blocking_gap(reason);
    assert_ne!(
        gap, "unit failed to link/lower",
        "unit-link block must name the MIR gap, not the generic fallback: {reason}"
    );
    let json = super::json_report_value(&report, &super::RunMetadata::default(), false);
    assert_eq!(json["census"]["blocked"].as_array().unwrap().len(), 1);
    assert_eq!(json["census"]["blocked"][0]["name"], "blocked");
    assert_eq!(json["census"]["blocked"][0]["gap"], gap);
    assert_ne!(
        json["census"]["gaps"].as_array().unwrap(),
        &[] as &[serde_json::Value]
    );
}

#[test]
fn cmd_test_applies_empty_run_law_and_does_not_widen_unmatched_include() {
    let source = include_str!("test.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    assert!(
        production.contains("allow_no_tests"),
        "cmd_test must honor --allow-no-tests"
    );
    assert!(
        production.contains("unmatched_include_filter"),
        "cmd_test must detect a no-match --include before running leftover .fab cases"
    );
    assert!(
        production.contains("empty_run_detail"),
        "cmd_test must emit a distinct empty-run message"
    );
    assert!(
        production.contains("report.results.is_empty()"),
        "cmd_test must fail closed on zero selected cases"
    );
}

#[test]
fn cmd_test_json_report_emits_blocked_census() {
    let source = include_str!("test.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    assert!(
        production.contains("json_census_value"),
        "JSON report must include a blocked census worklist"
    );
    assert!(
        production.contains("blocking_gap"),
        "JSON blocked cases must name the blocking MIR gap"
    );
}

#[test]
fn empty_selection_json_is_not_success_without_changing_exit_policy() {
    let report = run_proba_source(
        &Session::new(Config::default().with_latin_locale()),
        "empty_selection.fab",
        r#"proba "one equals one" { adfirma 1 ≡ 1 }"#,
        &TestSelection {
            name: Some("not selected".to_owned()),
            ..TestSelection::default()
        },
        None,
        false,
        false,
        None,
        false,
    )
    .expect("analyze and run fixture");
    let json = super::json_report_value(&report, &super::RunMetadata::default(), false);
    println!("empty selection JSON: {json}");
    println!(
        "empty exit policy: default={} allow_no_tests={}",
        super::empty_run_exit_code(false),
        super::empty_run_exit_code(true)
    );
    assert_eq!(report.results, [] as [proba::CaseResult; 0]);
    assert_eq!(json["success"], false);
    assert_eq!(super::empty_run_exit_code(false), 1);
    assert_eq!(super::empty_run_exit_code(true), 0);
}
