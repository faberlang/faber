//! Runner-backed proba runner tests (shipped `proba` leaf + package analysis).
//!
//! Proves `faber test` definition: MIR interpretation, not Cargo/Rust harness.

#![cfg(feature = "runner")]

use super::test_support::test_temp_dir;
use super::{discover_package, library_resolver_from_config};
use radix_package::analyze::{analyze_package as analyze_package_direct, analyze_package_for_tests as analyze_package_for_tests_direct, AnalysisSeams};
use radix_package::discovery::PackageSpec;
use radix_package::library::LibraryResolver;
use radix::driver::{Config, Session};
use radix::locale::LocalePack;

fn seams() -> AnalysisSeams {
    crate::package::faber_analysis_seams()
}

fn analyze_with_faber_seams(
    config: &Config,
    input: &Path,
) -> Result<radix_package::analyze::AnalyzedPackage, Vec<radix::diagnostics::Diagnostic>> {
    analyze_package_direct(config, input, &seams())
}

fn analyze_tests_with_faber_seams(
    config: &Config,
    input: &Path,
    filter: Option<&radix_package::test_source_filter::TestSourceFilter>,
) -> Result<radix_package::analyze::AnalyzedPackage, Vec<radix::diagnostics::Diagnostic>> {
    analyze_package_for_tests_direct(config, input, filter, &seams())
}

fn load_with_faber_seams(
    spec: &PackageSpec,
    resolver: &LibraryResolver,
    locale_pack: Option<&LocalePack>,
    include_proba: bool,
    filter: Option<&radix_package::test_source_filter::TestSourceFilter>,
) -> Result<Vec<radix_package::PackageFile>, Vec<radix::diagnostics::Diagnostic>> {
    radix_package::analyze::load_package_with_locale_pack(
        spec,
        resolver,
        locale_pack,
        include_proba,
        filter,
        &seams(),
    )
}
use proba::{run_proba_on_analyzed, run_proba_source, CaseOutcome, TestSelection};
use std::fs;
use std::path::{Path, PathBuf};

/// The product-default English code pack (en = authority). The loader-level
/// and runner-level tests below call `load_package_with_locale_pack` /
/// `run_proba_source` directly, so they must pass the same default pack the
/// `compile_package*`/`analyze_package` routes apply — not the pack-less
/// (registry-direct) lexing `None` / `Config::default()` would select.
fn english_pack() -> LocalePack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml");
    LocalePack::from_toml_path(path).expect("load English locale pack")
}

fn english_session() -> Session {
    Session::new(Config::default().with_locale_pack(english_pack()))
}

fn write_minimal_lib_package(dir: &Path) {
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "runner_proba_fixture"
version = "0.1.0"
edition = "2026"

[library]
provider = "runner_proba_fixture"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    fs::write(
        dir.join("src/lib.fab"),
        r#"fn identity(int n) → int {
    return n
}
"#,
    )
    .expect("lib.fab");
}

#[test]
fn runner_pass_fail_skip_on_inline_fixture() {
    let dir = test_temp_dir("runner-pfs");
    let path = dir.join("cases.fab");
    fs::write(
        &path,
        r#"
test "pass" {
    assert 1 ≡ 1
}
test "fail" {
    assert 1 ≡ 2
}
test "skip" skip "later" {
    assert 0 ≡ 1
}
"#,
    )
    .expect("write");
    let source = fs::read_to_string(&path).expect("read");
    let report = run_proba_source(
        &english_session(),
        path.to_str().unwrap(),
        &source,
        &TestSelection::default(),
        None,
        false,
        false,
    )
    .expect("run");
    assert_eq!(report.passed(), 1);
    assert_eq!(report.failed(), 1);
    assert_eq!(report.skipped(), 1);
    assert!(!report.success());
    let fail = report
        .results
        .iter()
        .find(|r| r.name == "fail")
        .expect("fail case");
    assert!(matches!(fail.outcome, CaseOutcome::Failed { .. }));
}

#[test]
fn analyze_package_for_tests_includes_proba_build_excludes() {
    let dir = test_temp_dir("runner-include");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/math.proba"),
        r#"
test "math ok" {
    assert 2 ≡ 2
}
"#,
    )
    .expect("proba");

    let config = Config::default();
    let for_tests = analyze_tests_with_faber_seams(&config, dir.path(), None).expect("analyze tests");
    assert!(
        for_tests
            .units
            .iter()
            .any(|u| u.path.extension().and_then(|e| e.to_str()) == Some("proba")),
        "test analysis must include .proba units"
    );

    let production = analyze_with_faber_seams(&config, dir.path()).expect("analyze production");
    assert!(
        production
            .units
            .iter()
            .all(|u| u.path.extension().and_then(|e| e.to_str()) != Some("proba")),
        "production analysis must exclude .proba"
    );

    // load_package path (build) also omits .proba — pass the product-default
    // en pack explicitly (a `None` pack would select registry-direct lexing).
    let spec = discover_package(dir.path()).expect("discover");
    let resolver = library_resolver_from_config(&config);
    let files = load_with_faber_seams(&spec, &resolver, Some(&english_pack()), false, None)
        .expect("load");
    assert!(files
        .iter()
        .all(|f| f.path.extension().and_then(|e| e.to_str()) != Some("proba")));
    let test_files =
        load_with_faber_seams(&spec, &resolver, Some(&english_pack()), true, None)
            .expect("load tests");
    assert!(test_files
        .iter()
        .any(|f| f.path.extension().and_then(|e| e.to_str()) == Some("proba")));
}

#[test]
fn package_inline_proba_runs_on_runner_after_analyze() {
    let dir = test_temp_dir("runner-pkg-inline");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/lib.fab"),
        r#"
fn identity(int n) → int {
    return n
}

test "identity holds" {
    assert identity(4) ≡ 4
}
"#,
    )
    .expect("lib with proba");

    let mut package =
        analyze_tests_with_faber_seams(&Config::default(), dir.path(), None).expect("analyze");
    assert!(
        package.diagnostics.iter().all(|d| !d.is_error()),
        "{:?}",
        package.diagnostics
    );

    let mut any = false;
    for unit in &mut package.units {
        let report = run_proba_on_analyzed(
            &mut unit.analysis,
            &TestSelection::default(),
            None,
            false,
            false,
        )
        .expect("run unit");
        if !report.results.is_empty() {
            any = true;
            assert!(report.success(), "{report:?}");
            assert_eq!(report.passed(), 1);
        }
    }
    assert!(any, "expected at least one proba case in package");
}

#[test]
fn name_and_suite_selection_on_runner() {
    let source = r#"
probandum "suite_a" {
    proba "alpha" { adfirma 1 ≡ 1 }
}
probandum "suite_b" {
    proba "beta" { adfirma 1 ≡ 1 }
}
"#;
    let by_name = run_proba_source(
        &Session::new(Config::default()),
        "sel.fab",
        source,
        &TestSelection {
            name: Some("beta".to_owned()),
            suite: None,
            tag: None,
        },
        None,
        false,
        false,
    )
    .expect("run");
    assert_eq!(by_name.results.len(), 1);
    assert_eq!(by_name.results[0].name, "beta");

    let by_suite = run_proba_source(
        &Session::new(Config::default()),
        "sel.fab",
        source,
        &TestSelection {
            name: None,
            suite: Some("suite_a".to_owned()),
            tag: None,
        },
        None,
        false,
        false,
    )
    .expect("run");
    assert_eq!(by_suite.results.len(), 1);
    assert_eq!(by_suite.results[0].name, "alpha");
}
