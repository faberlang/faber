//! Path normalization helpers for the faber package surface.
//!
//! RP2b: lexical normalization is compiler-owned and lives in `radix-package`.
//! The ambient-CWD absolutization seam stays faber-side (CTO memo):
//! `radix-package` never reads the process working directory.

#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) use radix_package::paths::normalize_path;

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
