//! Lexical path normalization for package graph keys.

use std::path::{Component, Path, PathBuf};

/// Normalize lexical path components without consulting the filesystem.
///
/// Package compilation uses normalized paths as stable graph keys, but must not
/// require `std::fs::canonicalize` because missing files should become compiler
/// diagnostics rather than path-resolution panics.
#[must_use]
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}
