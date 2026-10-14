//! A bare `{ field = value }` literal with no typed cell is a record (operator
//! ruling 2026-10-14).

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::analyze_source_with_config;
use radix_target::Target;

fn config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    config
}

fn check(source: &str) -> crate::test_support::AnalyzedFixture {
    analyze_source_with_config(source, config())
}

fn issues(source: &str) -> Vec<String> {
    check(source)
        .diagnostics
        .iter()
        .filter_map(|diag| {
            diag.args
                .iter()
                .find(|arg| arg.name == "issue")
                .map(|arg| arg.value.clone())
        })
        .collect()
}

fn accepts(source: &str) {
    let result = check(source);
    assert!(
        result.is_ok(),
        "expected green for {source:?}, got: {:?}",
        result.diagnostics
    );
}

fn refuses_with(source: &str, issue: &str) {
    let found = issues(source);
    assert!(
        found.iter().any(|found| found == issue),
        "expected {issue} for {source:?}, got: {found:?}"
    );
}

#[test]
fn bare_literal_without_cell_is_a_record() {
    accepts(
        "main {\n    const _ p ← { g = 1.0, w = 2 }\n    print p.g\n    const int n ← p.w\n    print n\n}\n",
    );
}

#[test]
fn underscore_hole_constructor_still_needs_a_cell() {
    refuses_with(
        "main {\n    const _ p ← _ { g = 1.0 }\n}\n",
        "inferred_constructor_no_expected_type",
    );
}
