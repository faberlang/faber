use super::{
    analyze_package_for_tests, bounded_analyze_diagnostic, run_with_analyze_budget,
    test_route_descent, test_source_filter_for_input, TestRouteDescent,
    PACKAGE_TEST_ANALYZE_BUDGET,
};
use crate::package::test_support::{diagnostic_has_arg, diagnostic_has_issue, test_temp_dir};
use radix::module::driver::Config;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

fn write_named_proba_fixture(dir: &Path) {
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "route_filter_fixture"
version = "0.1.0"
edition = "2026"

[library]
provider = "route_filter_fixture"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fhir"]
"#,
    )
    .expect("manifest");
    fs::write(
        dir.join("src/lib.fab"),
        r#"
@ public
fn n() → int {
  return 1
}
"#,
    )
    .expect("lib.fab");
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
    fs::write(
        dir.join("src/speculative.proba"),
        r#"
test "speculative" {
    assert 1 ≡ 1
}
"#,
    )
    .expect("speculative.proba");
}

#[test]
fn explicit_proba_path_replaces_include_and_drops_siblings() {
    let dir = test_temp_dir("route-filter-include");
    write_named_proba_fixture(dir.path());
    let filter = test_source_filter_for_input(
        &dir.join("src/cache.proba"),
        vec!["decode".to_owned()],
        vec!["*skip*".to_owned()],
    )
    .expect("named .proba builds a filter");
    assert_eq!(filter.include, vec!["cache.proba".to_owned()]);
    assert_eq!(filter.exclude, vec!["*skip*".to_owned()]);
    assert!(filter.allows_relative("cache.proba"));
    assert!(!filter.allows_relative("decode.proba"));
    assert!(!filter.allows_relative("speculative.proba"));
}

#[test]
fn package_root_keeps_caller_include() {
    let dir = test_temp_dir("route-filter-root");
    write_named_proba_fixture(dir.path());
    let filter = test_source_filter_for_input(dir.path(), vec!["math".to_owned()], Vec::new())
        .expect("include on a package root stays");
    assert_eq!(filter.include, vec!["math".to_owned()]);
}

#[test]
fn empty_directory_input_has_no_filter() {
    let dir = test_temp_dir("route-filter-empty");
    write_named_proba_fixture(dir.path());
    assert!(test_source_filter_for_input(dir.path(), Vec::new(), Vec::new()).is_none());
}

#[test]
fn descent_selects_only_the_named_proba() {
    let dir = test_temp_dir("route-descent");
    write_named_proba_fixture(dir.path());
    let input = dir.join("src/cache.proba");
    let filter = test_source_filter_for_input(&input, Vec::new(), Vec::new());
    let descent = test_route_descent(&input, filter.as_ref());
    assert_eq!(descent.selected, vec!["cache.proba".to_owned()]);
    assert!(
        descent.discovered.iter().any(|name| name == "decode.proba"),
        "discovered must still name sibling suites: {:?}",
        descent.discovered
    );
    assert!(
        descent
            .discovered
            .iter()
            .any(|name| name == "speculative.proba"),
        "discovered must still name sibling suites: {:?}",
        descent.discovered
    );
}

#[test]
fn bounded_diagnostic_names_input_filter_and_descent() {
    let descent = TestRouteDescent {
        input: "/pkg/src/cache.proba".to_owned(),
        include: vec!["cache.proba".to_owned()],
        selected: vec!["cache.proba".to_owned()],
        discovered: vec![
            "cache.proba".to_owned(),
            "decode.proba".to_owned(),
            "speculative.proba".to_owned(),
        ],
    };
    let diagnostic = bounded_analyze_diagnostic(PACKAGE_TEST_ANALYZE_BUDGET, &descent);
    assert!(diagnostic_has_issue(
        &diagnostic,
        "package_test_analyze_budget_exceeded"
    ));
    assert!(diagnostic_has_arg(&diagnostic, "phase", "analyze"));
    assert!(diagnostic_has_arg(&diagnostic, "include", "cache.proba"));
    assert!(diagnostic_has_arg(&diagnostic, "selected", "cache.proba"));
    assert!(
        diagnostic.message.contains("cache.proba"),
        "{}",
        diagnostic.message
    );
    assert!(
        diagnostic.message.contains("decode.proba"),
        "{}",
        diagnostic.message
    );
    assert!(
        diagnostic.message.contains("speculative.proba"),
        "{}",
        diagnostic.message
    );
    assert!(
        diagnostic
            .message
            .contains(&PACKAGE_TEST_ANALYZE_BUDGET.as_millis().to_string()),
        "{}",
        diagnostic.message
    );
}

#[test]
fn analyze_budget_times_out_instead_of_hanging() {
    let started = Instant::now();
    let result = run_with_analyze_budget(Duration::from_millis(50), || {
        thread_sleep();
        "done"
    });
    assert!(
        result.is_err(),
        "budget must fail closed when work does not return"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "timeout must be bounded, took {:?}",
        started.elapsed()
    );
}

#[test]
fn analyze_budget_returns_completed_work() {
    let value = run_with_analyze_budget(Duration::from_secs(2), || 7_u8).expect("within budget");
    assert_eq!(value, 7);
}

#[test]
fn named_proba_analyze_does_not_load_sibling_suites() {
    let dir = test_temp_dir("route-analyze-named");
    write_named_proba_fixture(dir.path());
    let package = analyze_package_for_tests(&Config::default(), &dir.join("src/cache.proba"), None)
        .expect("analyze named .proba");
    let mut proba_names: Vec<_> = package
        .nodes
        .values()
        .filter_map(|unit| {
            (unit.path.extension().and_then(|ext| ext.to_str()) == Some("proba"))
                .then(|| unit.path.file_name()?.to_str().map(str::to_owned))
                .flatten()
        })
        .collect();
    proba_names.sort();
    assert_eq!(
        proba_names,
        vec!["cache.proba".to_owned()],
        "named .proba must not pull sibling test sources: {proba_names:?}"
    );
}

fn thread_sleep() {
    std::thread::sleep(Duration::from_secs(8));
}
