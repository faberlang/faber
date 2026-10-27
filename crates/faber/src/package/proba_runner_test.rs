//! Runner-backed proba runner tests (shipped `proba` leaf + package analysis).
//!
//! Proves `faber test` definition: MIR interpretation, not Cargo/Rust harness.

#![cfg(feature = "runner")]

use super::test_support::test_temp_dir;
use super::{
    MANIFEST_LESS_TEST_MESSAGE, TestInputRoute, discover_package, library_resolver_from_config,
    test_input_route,
};
use radix::module::driver::{Config, Session};
use radix::module::locale::LocalePack;
use radix::program::analyze::{
    AnalysisSeams, analyze_package as analyze_package_direct,
    analyze_package_for_tests as analyze_package_for_tests_direct,
};
use radix::program::discovery::PackageSpec;
use radix::program::library::LibraryResolver;

fn seams() -> AnalysisSeams {
    crate::package::faber_analysis_seams()
}

fn analyze_with_faber_seams(
    config: &Config,
    input: &Path,
) -> Result<radix::program::analyze::AnalyzedProgram, Vec<radix::module::diagnostics::Diagnostic>> {
    analyze_package_direct(config, input, &seams())
}

fn analyze_tests_with_faber_seams(
    config: &Config,
    input: &Path,
    filter: Option<&radix::program::test_source_filter::TestSourceFilter>,
) -> Result<radix::program::analyze::AnalyzedProgram, Vec<radix::module::diagnostics::Diagnostic>> {
    analyze_package_for_tests_direct(config, input, filter, &seams())
}

fn load_with_faber_seams(
    spec: &PackageSpec,
    resolver: &LibraryResolver,
    locale_pack: Option<&LocalePack>,
    include_proba: bool,
    filter: Option<&radix::program::test_source_filter::TestSourceFilter>,
) -> Result<Vec<radix::program::PackageFile>, Vec<radix::module::diagnostics::Diagnostic>> {
    let config = locale_pack
        .cloned()
        .map(|pack| Config::default().with_resolved_locale_pack(pack))
        .unwrap_or_default();
    radix::program::analyze::load_package_with_config(
        spec,
        resolver,
        &config,
        include_proba,
        filter,
        &seams(),
    )
}
use proba::{
    CaseOutcome, TestSelection, run_proba_on_analyzed, run_proba_on_validated_with_function_ids,
    run_proba_source,
};
use std::fs;
use std::path::{Path, PathBuf};

/// The product-default English code pack (en = authority). The loader-level
/// and runner-level tests below call `load_package_with_config` /
/// `run_proba_source` directly, so they must pass the same default pack the
/// `compile_package*`/`analyze_package` routes apply — not the pack-less
/// (registry-direct) lexing `None` / `Config::default()` would select.
fn english_pack() -> LocalePack {
    let path = radix::module::locale::workspace_locale_pack_path("en");
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
        r"fn identity(int n) → int {
    return n
}
",
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
        None,
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
            .any(|u| u.path.extension().and_then(|e| e.to_str()) == Some("proba")),
        "test analysis must include .proba units"
    );

    let production = analyze_with_faber_seams(&config, dir.path()).expect("analyze production");
    assert!(
        production
            .nodes
            .values()
            .all(|u| u.path.extension().and_then(|e| e.to_str()) != Some("proba")),
        "production analysis must exclude .proba"
    );

    // load_package path (build) also omits .proba — pass the product-default
    // en pack explicitly (a `None` pack would select registry-direct lexing).
    let spec = discover_package(dir.path()).expect("discover");
    let resolver = library_resolver_from_config(&config);
    let files =
        load_with_faber_seams(&spec, &resolver, Some(&english_pack()), false, None).expect("load");
    assert!(
        files
            .iter()
            .all(|f| f.path.extension().and_then(|e| e.to_str()) != Some("proba"))
    );
    let test_files = load_with_faber_seams(&spec, &resolver, Some(&english_pack()), true, None)
        .expect("load tests");
    assert!(
        test_files
            .iter()
            .any(|f| f.path.extension().and_then(|e| e.to_str()) == Some("proba"))
    );
}

#[test]
fn analyze_package_for_tests_named_proba_does_not_load_sibling() {
    let dir = test_temp_dir("named-proba-filter");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/cache.proba"),
        r#"
test "cache" {
    assert 1 ≡ 1
}
"#,
    )
    .expect("cache.proba");
    fs::write(
        dir.join("src/decode.proba"),
        r#"
test "decode" {
    assert 1 ≡ 1
}
"#,
    )
    .expect("decode.proba");

    let package =
        analyze_tests_with_faber_seams(&Config::default(), &dir.join("src/cache.proba"), None)
            .expect("analyze named .proba");
    let proba_names: Vec<_> = package
        .nodes
        .values()
        .filter_map(|unit| {
            (unit.path.extension().and_then(|ext| ext.to_str()) == Some("proba"))
                .then(|| unit.path.file_name()?.to_str().map(str::to_owned))
                .flatten()
        })
        .collect();
    assert_eq!(
        proba_names,
        vec!["cache.proba".to_owned()],
        "named .proba must not pull sibling test sources: {proba_names:?}"
    );
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
            None,
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
        r"@ public
fn double(int n) → int {
    return n + n
}
",
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
    let mut functions = Vec::new();
    let mut test_units = Vec::new();
    for unit in package.nodes.values() {
        let cases = proba::inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
        if cases.is_empty() {
            continue;
        }
        functions.extend(cases.iter().map(|case| (unit.path.clone(), case.def_id)));
        test_units.push((unit.path.clone(), cases));
    }
    let scope = radix::program::mir::TestPackageScope::for_program(&package, functions);
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
                        None,
                        false,
                        proba::CaseKernels::Runner(radix_mir_runner::RunnerKernelPolicy::Refuse),
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
        &Session::new(Config::default().with_latin_locale()),
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
        None,
        false,
    )
    .expect("run");
    assert_eq!(by_name.results.len(), 1);
    assert_eq!(by_name.results[0].name, "beta");

    let by_suite = run_proba_source(
        &Session::new(Config::default().with_latin_locale()),
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
        None,
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
        r"fn identity(int n) → int {
    return n
}
",
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
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            None,
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
        absolute = crate::package::absolutize_path(dir.path());
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
    let unit_names = |pkg: &radix::program::analyze::AnalyzedProgram| {
        let mut names = pkg
            .nodes
            .values()
            .map(|u| u.path.file_name().unwrap_or_default().to_os_string())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    assert_eq!(unit_names(&pkg_relative), unit_names(&pkg_absolute));
}

fn write_in_package_import_fixture(dir: &Path) -> PathBuf {
    write_minimal_lib_package(dir);
    fs::write(
        dir.join("src/mathesis.fab"),
        r"@ public
fn PI() → float {
    return 3.14159
}
",
    )
    .expect("mathesis.fab");
    let proba = dir.join("src/mathesis.proba");
    fs::write(
        &proba,
        r#"import from "./mathesis" mathesis

test "PI is positive" {
    assert mathesis.PI() ≻ 0.0
}
"#,
    )
    .expect("mathesis.proba");
    proba
}

fn linked_package_report(package: radix::program::analyze::AnalyzedProgram) -> proba::ProbaReport {
    let mut functions = Vec::new();
    let mut test_units = Vec::new();
    for unit in package.nodes.values() {
        let cases = proba::inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
        if cases.is_empty() {
            continue;
        }
        functions.extend(cases.iter().map(|case| (unit.path.clone(), case.def_id)));
        test_units.push((unit.path.clone(), cases));
    }
    let scope = radix::program::mir::TestPackageScope::for_program(&package, functions);
    crate::package::mir::with_linked_test_package_mir(
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
                        None,
                        false,
                        proba::CaseKernels::Runner(radix_mir_runner::RunnerKernelPolicy::Refuse),
                    )
                    .results,
                );
            }
            report
        },
    )
    .expect("link package test roots")
}

#[test]
fn test_route_sends_in_package_source_file_through_package_analysis() {
    let dir = test_temp_dir("route-pkg-file");
    let proba = write_in_package_import_fixture(dir.path());
    assert_eq!(
        test_input_route(&proba, false),
        TestInputRoute::Package,
        "a .proba under faber.toml must use package analysis"
    );
    assert_eq!(
        test_input_route(&dir.join("src/mathesis.fab"), false),
        TestInputRoute::Package,
        "a .fab under faber.toml must use package analysis"
    );
    assert_eq!(
        test_input_route(dir.path(), false),
        TestInputRoute::Package,
        "the package root itself stays on the package route"
    );
}

#[test]
fn test_route_no_package_keeps_bare_single_file_route() {
    let dir = test_temp_dir("route-no-pkg");
    let proba = write_in_package_import_fixture(dir.path());
    assert_eq!(
        test_input_route(&proba, true),
        TestInputRoute::SingleFile,
        "--no-package must preserve the bare single-file route"
    );
}

#[test]
fn test_route_bare_file_outside_package_stays_single_file() {
    let dir = test_temp_dir("route-bare-file");
    let path = dir.join("alone.proba");
    fs::write(&path, r#"test "ok" { assert 1 ≡ 1 }"#).expect("write");
    assert_eq!(test_input_route(&path, false), TestInputRoute::SingleFile);
    assert_eq!(test_input_route(&path, true), TestInputRoute::SingleFile);
}

#[test]
fn test_route_manifest_less_directory_is_guiding_diagnostic() {
    let dir = test_temp_dir("route-no-toml");
    fs::write(dir.join("src.fab"), "fn n() → int { return 1 }\n").expect("src");
    assert_eq!(
        test_input_route(dir.path(), false),
        TestInputRoute::ManifestLessDirectory
    );
    assert_eq!(
        MANIFEST_LESS_TEST_MESSAGE,
        "no faber.toml found; testing a library requires a package root"
    );
}

#[test]
fn single_proba_file_under_package_root_analyzes_and_links_local_import() {
    let dir = test_temp_dir("route-file-link");
    let proba = write_in_package_import_fixture(dir.path());

    assert_eq!(test_input_route(&proba, false), TestInputRoute::Package);

    let package = analyze_tests_with_faber_seams(&Config::default(), &proba, None)
        .expect("analyze single .proba through package discovery");
    assert!(
        package.spec.manifest_backed,
        "file input must resolve the enclosing package"
    );
    assert!(
        package.diagnostics.iter().all(|d| !d.is_error()),
        "{:?}",
        package.diagnostics
    );

    let report = linked_package_report(package);
    assert_eq!(report.passed(), 1, "{report:?}");
    assert_eq!(report.failed(), 0, "{report:?}");
    for result in &report.results {
        let message = match &result.outcome {
            CaseOutcome::Failed { message } => message.as_str(),
            _ => continue,
        };
        assert!(
            !message.contains("provider package:"),
            "in-package import must not stay a provider identity: {message}"
        );
    }
}

#[test]
fn no_package_single_file_route_cannot_resolve_in_package_import() {
    let dir = test_temp_dir("route-bare-import");
    let proba = write_in_package_import_fixture(dir.path());
    assert_eq!(test_input_route(&proba, true), TestInputRoute::SingleFile);

    let source = fs::read_to_string(&proba).expect("read");
    let result = run_proba_source(
        &english_session(),
        proba.to_str().unwrap(),
        &source,
        &TestSelection::default(),
        None,
        false,
        false,
        None,
        false,
    );
    if let Ok(report) = result {
        assert!(
            !report.success(),
            "bare --no-package route must not resolve in-package imports: {report:?}"
        );
    } else {
        // Frontend/MIR error on the import is also the bare-route failure.
    }
}

/// One `@ kernel` function called from a `.proba` case, run through the linked
/// package route under the given lowering config and runner policy.
fn run_kernel_proba_package(
    config: &Config,
    kernel_policy: radix_mir_runner::RunnerKernelPolicy,
) -> proba::ProbaReport {
    let dir = test_temp_dir("runner-kernel-proba");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/k.fab"),
        r"@ kernel
@ public
fn bump(f32 x) → f32 {
    return x + 1.0
}
",
    )
    .expect("k.fab");
    fs::write(
        dir.join("src/k.proba"),
        r#"import from "./k" k

test "cpu" {
    assert k.bump(1.0) ≡ (2.0 ∷ f32)
}
"#,
    )
    .expect("k.proba");

    let package =
        analyze_tests_with_faber_seams(config, dir.path(), None).expect("analyze kernel package");
    let mut functions = Vec::new();
    let mut test_units = Vec::new();
    for unit in package.nodes.values() {
        let cases = proba::inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
        if cases.is_empty() {
            continue;
        }
        functions.extend(cases.iter().map(|case| (unit.path.clone(), case.def_id)));
        test_units.push((unit.path.clone(), cases));
    }
    let scope = radix::program::mir::TestPackageScope::for_program(&package, functions);
    crate::package::mir::with_linked_test_package_mir(
        package,
        &scope,
        config,
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
                        None,
                        false,
                        proba::CaseKernels::Runner(kernel_policy),
                    )
                    .results,
                );
            }
            report
        },
    )
    .expect("link kernel package test roots")
}

#[test]
fn proba_case_calling_a_kernel_runs_under_host_kernels_and_execute() {
    let report = run_kernel_proba_package(
        &Config::default().with_host_kernels(),
        radix_mir_runner::RunnerKernelPolicy::Execute,
    );
    assert_eq!(report.passed(), 1, "{report:?}");
    assert_eq!(report.failed(), 0, "{report:?}");
    assert_eq!(report.blocked(), 0, "{report:?}");
}

#[test]
fn proba_case_calling_a_kernel_is_refused_on_the_default_route() {
    let report = run_kernel_proba_package(
        &Config::default(),
        radix_mir_runner::RunnerKernelPolicy::Refuse,
    );
    assert_eq!(report.results.len(), 1, "{report:?}");
    assert!(
        matches!(report.results[0].outcome, CaseOutcome::Failed { .. }),
        "{report:?}"
    );
    assert_eq!(report.passed(), 0, "{report:?}");
    assert_eq!(report.blocked(), 0, "{report:?}");
}
