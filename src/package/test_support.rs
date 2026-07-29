//! Shared test helpers for package integration tests.
//!
//! Extracted from `package_test.rs` so that sibling test modules (e.g.
//! `frontmatter_integration_test.rs`) can reuse them without duplication.

use radix::diagnostics::{Diagnostic, DiagnosticArg};
use tempfile::TempDir;

pub fn diagnostic_has_issue(diag: &Diagnostic, issue: &str) -> bool {
    diag.args.contains(&DiagnosticArg::new("issue", issue))
}

pub fn diagnostic_has_arg(diag: &Diagnostic, name: &'static str, value: impl Into<String>) -> bool {
    diag.args.contains(&DiagnosticArg::new(name, value))
}

pub fn test_temp_dir(label: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(&format!("faber-{label}-"))
        .tempdir()
        .expect("create temp dir")
}
