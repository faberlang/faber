//! Runner-backed proba runner tests (shipped `proba` leaf + package analysis).
//!
//! Proves `faber test` definition: MIR interpretation, not Cargo/Rust harness.

#![cfg(feature = "runner")]

use super::test_support::test_temp_dir;
use super::{discover_package, library_resolver_from_config};
use radix::driver::{Config, Session};
use radix::locale::LocalePack;
use radix_package::analyze::{
    analyze_package as analyze_package_direct,
    analyze_package_for_tests as analyze_package_for_tests_direct, AnalysisSeams,
};
use radix_package::discovery::PackageSpec;
use radix_package::library::LibraryResolver;

fn seams() -> AnalysisSeams {
    crate::package::faber_analysis_seams()
}

fn analyze_with_faber_seams(
    config: &Config,
    input: &Path,
) -> Result<radix_package::analyze::AnalyzedProgram, Vec<radix::diagnostics::Diagnostic>> {
    analyze_package_direct(config, input, &seams())
}

fn analyze_tests_with_faber_seams(
    config: &Config,
    input: &Path,
    filter: Option<&radix_package::test_source_filter::TestSourceFilter>,
) -> Result<radix_package::analyze::AnalyzedProgram, Vec<radix::diagnostics::Diagnostic>> {
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
use proba::{
    run_proba_on_analyzed, run_proba_on_validated_with_function_ids, run_proba_source, CaseOutcome,
    TestSelection,
};
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
    let for_tests =
        analyze_tests_with_faber_seams(&config, dir.path(), None).expect("analyze tests");
    assert!(
        for_tests
            .nodes
            .values()
            .iter()
            .any(|u| u.path.extension().and_then(|e| e.to_str()) == Some("proba")),
        "test analysis must include .proba units"
    );

    let production = analyze_with_faber_seams(&config, dir.path()).expect("analyze production");
    assert!(
        production
            .nodes
            .values()
            .iter()
            .all(|u| u.path.extension().and_then(|e| e.to_str()) != Some("proba")),
        "production analysis must exclude .proba"
    );

    // load_package path (build) also omits .proba — pass the product-default
    // en pack explicitly (a `None` pack would select registry-direct lexing).
    let spec = discover_package(dir.path()).expect("discover");
    let resolver = library_resolver_from_config(&config);
    let files =
        load_with_faber_seams(&spec, &resolver, Some(&english_pack()), false, None).expect("load");
    assert!(files
        .iter()
        .all(|f| f.path.extension().and_then(|e| e.to_str()) != Some("proba")));
    let test_files = load_with_faber_seams(&spec, &resolver, Some(&english_pack()), true, None)
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
    for unit in package.nodes.values_mut() {
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
fn entryless_package_proba_roots_link_once_and_run_with_original_ids() {
    let dir = test_temp_dir("runner-linked-roots");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/lib.fab"),
        r#"@ public
fn double(int n) → int {
    return n + n
}
"#,
    )
    .expect("lib.fab");
    fs::write(
        dir.join("src/first.proba"),
        r#"import from "./lib" lib

test "double" {
    assert lib.double(3) ≡ 6
}
"#,
    )
    .expect("first.proba");
    fs::write(
        dir.join("src/second.proba"),
        r#"test "literal" {
    assert 2 ≡ 2
}
"#,
    )
    .expect("second.proba");

    let package = analyze_tests_with_faber_seams(&Config::default(), dir.path(), None)
        .expect("analyze entryless package tests");
    let mut roots = Vec::new();
    let mut functions = Vec::new();
    let mut test_units = Vec::new();
    for unit in package.nodes.values() {
        let cases = proba::inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
        if cases.is_empty() {
            continue;
        }
        roots.push(unit.path.clone());
        functions.extend(cases.iter().map(|case| (unit.path.clone(), case.def_id)));
        test_units.push((unit.path.clone(), cases));
    }
    let remap_anchor = roots.into_iter().next().expect("test units are non-empty");
    let scope = radix_package::mir::TestPackageScope {
        remap_anchor,
        functions,
    };
    let report = crate::package::mir::with_linked_test_package_mir(
        package,
        &scope,
        &Config::default(),
        |validated, function_ids| {
            let mut report = proba::ProbaReport::default();
            for (path, cases) in &test_units {
                report.results.extend(
                    run_proba_on_validated_with_function_ids(
                        validated,
                        path,
                        cases,
                        function_ids,
                        &TestSelection::default(),
                        None,
                        false,
                        false,
                    )
                    .results,
                );
            }
            report
        },
    )
    .expect("link entryless package test roots");
    assert_eq!(report.passed(), 2, "{report:?}");
    assert_eq!(report.failed(), 0, "{report:?}");
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

/// Mirrors [`write_minimal_lib_package`] and adds the Triga-shaped
/// `[reader] locale = "en"` manifest section so the rdx-s05-1 regressions also
/// prove manifest reader-locale selection on the package-test route.
fn write_reader_locale_lib_package(dir: &Path) {
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "runner_locale_fixture"
version = "0.1.0"
edition = "2026"

[library]
provider = "runner_locale_fixture"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]

[reader]
locale = "en"
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

/// Serializes the rdx-s05-1 chdir regressions: ambient CWD is process-global,
/// so parallel tests cannot each chdir to their own temp package.
static CWD_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

fn with_package_cwd(dir: &Path, f: impl FnOnce()) {
    let guard = CWD_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let original_cwd = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(dir).expect("chdir");
    f();
    std::env::set_current_dir(original_cwd).expect("restore current dir");
    drop(guard);
}

/// rdx-s05-1 regression: `faber test .` from a package root must resolve the
/// manifest-backed package. A raw `.` is what `TestArgs` defaults to and what
/// `cmd_test` now routes through the faber absolutization seam before package
/// analysis; previously the lexical `normalize_path` collapsed `.` to an
/// empty path and discovery reported `cannot read '.'`. The English probe is
/// parsed only because the manifest `[reader] locale` selects the en pack.
#[test]
fn dot_input_from_package_root_analyzes_and_runs_colocated_proba() {
    let dir = test_temp_dir("runner-dot-pkg");
    write_reader_locale_lib_package(dir.path());
    fs::write(
        dir.join("src/math.proba"),
        r#"
test "math ok" {
    assert 8 ≡ 8
}
"#,
    )
    .expect("math.proba");

    let mut dot_input = PathBuf::new();
    with_package_cwd(dir.path(), || {
        dot_input = crate::package::absolutize_path(Path::new("."));
    });

    assert!(
        dot_input.is_absolute(),
        "`.` must absolutize to the package root, got {}",
        dot_input.display()
    );
    let mut package = analyze_tests_with_faber_seams(&Config::default(), &dot_input, None)
        .expect("analyze `faber test .` package");
    assert!(
        package.spec.manifest_backed,
        "`.` must resolve the manifest-backed package"
    );
    assert!(
        package.diagnostics.iter().all(|d| !d.is_error()),
        "{:?}",
        package.diagnostics
    );

    let mut passed = 0;
    for unit in package.nodes.values_mut() {
        if proba::inventory_cases(&unit.analysis.hir, &unit.analysis.interner).is_empty() {
            continue;
        }
        let report = run_proba_on_analyzed(
            &mut unit.analysis,
            &TestSelection::default(),
            None,
            false,
            false,
        )
        .expect("run unit");
        passed += report.passed();
    }
    assert!(
        passed >= 1,
        "co-located proba cases must run from the package-root `.` input"
    );
}

/// rdx-s05-1 regression: `faber test triga` from a workspace parent and the
/// absolute package path must enter the same manifest-backed package. The two
/// spellings differ only before the shared faber absolutization seam.
#[test]
fn relative_and_absolute_package_paths_resolve_the_same_test_package() {
    let dir = test_temp_dir("runner-relabs-pkg");
    write_reader_locale_lib_package(dir.path());
    fs::write(
        dir.join("src/core.proba"),
        r#"
test "core ok" {
    assert 1 ≡ 1
}
"#,
    )
    .expect("core.proba");

    let package_name = dir
        .path()
        .file_name()
        .expect("temp package name")
        .to_owned();
    let parent = dir.path().parent().expect("temp parent").to_path_buf();

    let mut relative = PathBuf::new();
    let mut absolute = PathBuf::new();
    with_package_cwd(&parent, || {
        relative = crate::package::absolutize_path(Path::new(&package_name));
        absolute = crate::package::absolutize_path(&dir.path());
    });

    assert!(
        super::test_support::paths_equivalent(&relative, &absolute),
        "both spellings must absolutize to the same directory: {} vs {}",
        relative.display(),
        absolute.display()
    );

    let pkg_relative = analyze_tests_with_faber_seams(&Config::default(), &relative, None)
        .expect("relative package path must analyze");
    let pkg_absolute = analyze_tests_with_faber_seams(&Config::default(), &absolute, None)
        .expect("absolute package path must analyze");
    for pkg in [&pkg_relative, &pkg_absolute] {
        assert!(pkg.spec.manifest_backed, "package must be manifest-backed");
    }
    assert!(
        super::test_support::paths_equivalent(
            &pkg_relative.spec.package_root,
            &pkg_absolute.spec.package_root,
        ),
        "relative and absolute inputs must land on the same package root"
    );
    let unit_names = |pkg: &radix_package::analyze::AnalyzedProgram| {
        let mut names = pkg
            .nodes
            .values()
            .iter()
            .map(|u| u.path.file_name().unwrap_or_default().to_os_string())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    assert_eq!(unit_names(&pkg_relative), unit_names(&pkg_absolute));
}
