//! `ref` locals and the re-pointable bare parameter.
//!
//! A `ref` local names a place: the name may be re-pointed, a write through it
//! is one `assign_through_ref` diagnostic, and a scalar type takes no marker
//! (`scalar_access_marker`). A bare parameter is a `ref`: re-pointing the name
//! is legal, a write through it is one `assign_to_de_parameter` diagnostic.
//! Tests assert issue slugs only.

use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

const CELL: &str = "class Cell {\n    mut int value\n}\n";

fn error_issues(source: &str) -> Vec<String> {
    analyze_en(source)
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| issue(d).unwrap_or("<no issue>").to_owned())
        .collect()
}

#[test]
fn ref_local_accepts_repointing() {
    let errors = error_issues(&format!(
        "{CELL}fn f(ref Cell a, ref Cell b) → int {{\n    ref Cell cur ← a\n    cur ← b\n    return cur.value\n}}\nmain {{}}\n"
    ));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn write_through_a_ref_local_is_one_diagnostic() {
    let errors = error_issues(&format!(
        "{CELL}main {{\n    mut Cell c ← Cell {{ value = 1 }}\n    ref Cell cur ← c\n    cur.value ← 5\n    print c.value\n}}\n"
    ));
    assert_eq!(errors, ["assign_through_ref"], "{errors:?}");
}

#[test]
fn a_scalar_ref_local_is_refused() {
    let errors = error_issues("main {\n    let int n ← 3\n    ref int m ← n\n    print m\n}\n");
    assert!(
        errors.iter().any(|e| e == "scalar_access_marker"),
        "{errors:?}"
    );
}

#[test]
fn bare_parameter_may_be_repointed() {
    let errors = error_issues(&format!(
        "{CELL}fn f(Cell a, Cell b) → int {{\n    a ← b\n    return a.value\n}}\nmain {{}}\n"
    ));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn write_through_a_bare_parameter_is_one_diagnostic() {
    let errors = error_issues(&format!(
        "{CELL}fn f(Cell a) → void {{\n    a.value ← 5\n}}\nmain {{}}\n"
    ));
    assert_eq!(errors, ["assign_to_de_parameter"], "{errors:?}");
}
