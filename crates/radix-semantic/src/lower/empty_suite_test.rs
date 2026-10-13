//! A suite that holds no test case lowers to no items; it is not an
//! executable statement, so it must not report `unhandled_statement_kind`.

use crate::test_support::{analyze_and_get_diagnostics, assert_accepts};

fn issues(source: &str) -> Vec<String> {
    analyze_and_get_diagnostics(source)
        .iter()
        .filter_map(|diag| crate::test_support::issue(diag).map(str::to_owned))
        .collect()
}

#[test]
fn empty_probandum_suite_lowers_without_diagnostics() {
    let found = issues(r#"probandum "vacua" { }"#);
    assert!(
        found.is_empty(),
        "empty suite reported {found:?}, expected no diagnostics"
    );
}

#[test]
fn probandum_suite_holding_only_an_empty_nested_suite_is_accepted() {
    assert_accepts(
        r#"probandum "exterior" {
    probandum "interior" { }
}"#,
    );
}
