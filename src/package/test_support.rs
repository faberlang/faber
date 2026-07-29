//! Shared test helpers for package integration tests.
//!
//! Extracted from `package_test.rs` so that sibling test modules (e.g.
//! `frontmatter_integration_test.rs`) can reuse them without duplication.

use radix::diagnostics::{Diagnostic, DiagnosticArg};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn diagnostic_has_issue(diag: &Diagnostic, issue: &str) -> bool {
    diag.args.contains(&DiagnosticArg::new("issue", issue))
}

pub fn diagnostic_has_arg(diag: &Diagnostic, name: &'static str, value: impl Into<String>) -> bool {
    diag.args.contains(&DiagnosticArg::new(name, value))
}

pub fn test_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("radix-project-{label}-{nanos}"));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
