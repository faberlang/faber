//! Stepper-backed proba runner tests (shipped `radix::proba` + package analysis).
//!
//! Proves `faber test` definition: MIR interpretation, not Cargo/Rust harness.

use super::test_support::test_temp_dir;
use super::{analyze_package, analyze_package_for_tests, load_package, load_package_with_reader_pack};
use super::{discover_package, library_resolver_from_config};
use radix::driver::{Config, Session};
use radix::proba::{run_proba_source, CaseOutcome, TestSelection};
use std::fs;
use std::path::Path;

fn write_minimal_lib_package(dir: &Path) {
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "stepper_proba_fixture"
version = "0.1.0"
edition = "2026"

[library]
provider = "stepper_proba_fixture"

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
        r#"functio identity(numerus n) → numerus {
    redde n
}
"#,
    )
    .expect("lib.fab");
}

#[test]
fn stepper_pass_fail_skip_on_inline_fixture() {
    let dir = test_temp_dir("stepper-pfs");
    let path = dir.join("cases.fab");
    fs::write(
        &path,
        r#"
proba "pass" {
    adfirma 1 ≡ 1
}
proba "fail" {
    adfirma 1 ≡ 2
}
proba "skip" omitte "later" {
    adfirma 0 ≡ 1
}
"#,
    )
    .expect("write");
    let source = fs::read_to_string(&path).expect("read");
    let report = run_proba_source(
        &Session::new(Config::default()),
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
    let dir = test_temp_dir("stepper-include");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/math.proba"),
        r#"
proba "math ok" {
    adfirma 2 ≡ 2
}
"#,
    )
    .expect("proba");

    let config = Config::default();
    let for_tests = analyze_package_for_tests(&config, dir.path(), None).expect("analyze tests");
    assert!(
        for_tests
            .units
            .iter()
            .any(|u| u.path.extension().and_then(|e| e.to_str()) == Some("proba")),
        "test analysis must include .proba units"
    );

    let production = analyze_package(&config, dir.path()).expect("analyze production");
    assert!(
        production
            .units
            .iter()
            .all(|u| u.path.extension().and_then(|e| e.to_str()) != Some("proba")),
        "production analysis must exclude .proba"
    );

    // load_package path (build) also omits .proba
    let spec = discover_package(dir.path()).expect("discover");
    let resolver = library_resolver_from_config(&config);
    let files = load_package(&spec, &resolver).expect("load");
    assert!(
        files
            .iter()
            .all(|f| f.path.extension().and_then(|e| e.to_str()) != Some("proba"))
    );
    let test_files =
        load_package_with_reader_pack(&spec, &resolver, None, true, None).expect("load tests");
    assert!(test_files
        .iter()
        .any(|f| f.path.extension().and_then(|e| e.to_str()) == Some("proba")));
}

#[test]
fn package_inline_proba_runs_on_stepper_after_analyze() {
    let dir = test_temp_dir("stepper-pkg-inline");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/lib.fab"),
        r#"
functio identity(numerus n) → numerus {
    redde n
}

proba "identity holds" {
    adfirma identity(4) ≡ 4
}
"#,
    )
    .expect("lib with proba");

    let mut package =
        analyze_package_for_tests(&Config::default(), dir.path(), None).expect("analyze");
    assert!(package.diagnostics.iter().all(|d| !d.is_error()), "{:?}", package.diagnostics);

    let mut any = false;
    for unit in &mut package.units {
        let report = radix::proba::run_proba_on_analyzed(
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
fn name_and_suite_selection_on_stepper() {
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
