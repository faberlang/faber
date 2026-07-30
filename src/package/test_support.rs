//! Shared test helpers for package integration tests.
//!
//! Extracted from `package_test.rs` so that sibling test modules (e.g.
//! `frontmatter_integration_test.rs`) can reuse them without duplication.

use radix::diagnostics::{Diagnostic, DiagnosticArg};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub fn diagnostic_has_issue(diag: &Diagnostic, issue: &str) -> bool {
    diag.args.contains(&DiagnosticArg::new("issue", issue))
}

pub fn diagnostic_has_arg(diag: &Diagnostic, name: &'static str, value: impl Into<String>) -> bool {
    diag.args.contains(&DiagnosticArg::new(name, value))
}

/// Temp directory that `Deref`s to [`Path`] so call sites can keep using
/// `dir.join(...)` after tempfile 3.26+ dropped `TempDir: Deref<Path>`.
pub struct TestDir {
    inner: TempDir,
}

impl TestDir {
    pub fn path(&self) -> &Path {
        self.inner.path()
    }
}

impl Deref for TestDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.inner.path()
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        self.inner.path()
    }
}

impl From<&TestDir> for PathBuf {
    fn from(dir: &TestDir) -> PathBuf {
        dir.path().to_path_buf()
    }
}

impl PartialEq<PathBuf> for TestDir {
    fn eq(&self, other: &PathBuf) -> bool {
        self.path() == other.as_path()
    }
}

impl PartialEq<TestDir> for PathBuf {
    fn eq(&self, other: &TestDir) -> bool {
        self.as_path() == other.path()
    }
}

impl std::fmt::Debug for TestDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("TestDir").field(&self.path()).finish()
    }
}

pub fn test_temp_dir(label: &str) -> TestDir {
    let inner = tempfile::Builder::new()
        .prefix(&format!("faber-{label}-"))
        .tempdir()
        .expect("create temp dir");
    TestDir { inner }
}

/// Convenience for callers that need an owned path without keeping the temp dir.
#[allow(dead_code)]
pub fn test_temp_path(label: &str) -> PathBuf {
    test_temp_dir(label).path().to_path_buf()
}
