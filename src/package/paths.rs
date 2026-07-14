#[cfg(test)]
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Normalize lexical path components without consulting the filesystem.
///
/// Package compilation uses normalized paths as stable graph keys, but must not
/// require `std::fs::canonicalize` because missing files should become compiler
/// diagnostics rather than path-resolution panics.
pub(crate) fn normalize_path(path: &Path) -> PathBuf {
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

/// Convert a possibly relative path into the normalized form used for package
/// graph keys.
///
/// This deliberately avoids filesystem canonicalization: package checks should
/// be able to report diagnostics for paths that do not exist yet without
/// requiring every parent directory to resolve through the OS.
pub(crate) fn absolutize_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return normalize_path(path);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    normalize_path(&cwd.join(path))
}

/// Compare paths after resolving filesystem aliases such as macOS `/var` →
/// `/private/var`, while retaining lexical normalization for missing paths.
#[cfg(test)]
pub(crate) fn paths_equivalent(left: &Path, right: &Path) -> bool {
    comparison_path(left) == comparison_path(right)
}

#[cfg(test)]
fn comparison_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| normalize_path(path))
}
