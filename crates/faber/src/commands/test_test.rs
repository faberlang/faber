//! Companion tests for `faber test` (runner proba runner).
//!
//! WHY: the hygiene ratchet bans inline `#[cfg(test)] mod tests` in production
//! sources; tests live in dedicated `*_test.rs` files.

#![cfg(feature = "runner")]

use proba::{run_proba_source, TestSelection};
use radix_module::driver::{Config, Session};
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
    let session = Session::new(Config::default());
    let source = fs::read_to_string(&path).expect("read");
    let report = run_proba_source(
        &session,
        &path.to_string_lossy(),
        &source,
        &TestSelection::default(),
        None,
        false,
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
    let session = Session::new(Config::default());
    let source = fs::read_to_string(&path).expect("read");
    let report = run_proba_source(
        &session,
        &path.to_string_lossy(),
        &source,
        &TestSelection::default(),
        None,
        false,
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
        production.contains("run_proba_source") || production.contains("run_proba_on_analyzed"),
        "cmd_test must drive the runner proba runner"
    );
}

#[test]
fn package_analysis_failures_render_file_line_and_message() {
    use radix_module::diagnostics::{Diagnostic, Span};

    let error = Diagnostic::error("unknown identifier")
        .with_code("SEM001")
        .with_arg("issue", "unknown_identifier")
        .with_file("src/gradus.fab")
        .with_span(Span::new(42, 50))
        .with_source_line("    nomen missing");
    let check_line = radix_module::diagnostics::render_plain(&error);
    let rendered = super::render_package_analysis_diagnostics(std::slice::from_ref(&error));

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
    use radix_module::diagnostics::{Diagnostic, Span};

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
    let rendered = super::render_package_analysis_diagnostics(&diagnostics);

    assert!(
        rendered.contains(&radix_module::diagnostics::render_plain(&error)),
        "errors must keep the faber-check plain renderer:\n{rendered}"
    );
    assert!(
        rendered.contains(&radix_module::diagnostics::render_plain(&unique)),
        "a unique warning still renders in full:\n{rendered}"
    );
    assert!(
        rendered.contains("warning[LOCALE002]: 3 occurrences"),
        "repeated warning codes collapse to a counted summary:\n{rendered}"
    );
    assert_eq!(
        rendered.matches("warning[LOCALE002]").count(),
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
