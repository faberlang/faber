//! Package-contained manifest path resolution.
//!
//! This module owns the security boundary between manifest-relative paths and
//! the host filesystem. Package graph normalization remains in `paths.rs`.

use std::fs;
use std::path::{Component, Path, PathBuf};

use radix::diagnostics::Diagnostic;

#[allow(clippy::result_large_err)]
pub(crate) fn resolve_package_member(
    package_root: &Path,
    relative: &str,
    anchor: &Path,
) -> Result<PathBuf, Diagnostic> {
    let normalized = normalize_member_path(relative, anchor)?;
    let canonical_root = fs::canonicalize(package_root).map_err(|error| {
        Diagnostic::io_error(package_root, error)
            .with_arg("issue", "package_root_canonicalize_failed")
    })?;
    let resolved = package_root.join(&normalized);
    let candidate = canonical_root.join(normalized);
    let existing = nearest_existing_ancestor(&candidate).ok_or_else(|| {
        path_diagnostic(
            anchor,
            relative,
            "package member has no existing ancestor",
            "package_member_ancestor_missing",
        )
    })?;
    let canonical_existing = fs::canonicalize(existing).map_err(|error| {
        Diagnostic::io_error(existing, error)
            .with_arg("issue", "package_member_canonicalize_failed")
            .with_arg("path", relative.to_owned())
    })?;

    if !canonical_existing.starts_with(&canonical_root) {
        return Err(path_diagnostic(
            anchor,
            relative,
            "package member resolves outside the package root",
            "package_member_symlink_escape",
        ));
    }

    Ok(resolved)
}

#[allow(clippy::result_large_err)]
fn normalize_member_path(relative: &str, anchor: &Path) -> Result<PathBuf, Diagnostic> {
    if relative.trim().is_empty() {
        return Err(path_diagnostic(
            anchor,
            relative,
            "package member path must not be empty",
            "package_member_empty",
        ));
    }

    let path = Path::new(relative);
    if path.is_absolute() {
        return Err(path_diagnostic(
            anchor,
            relative,
            "package member path must be relative",
            "package_member_absolute",
        ));
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => normalized.push(segment),
            Component::ParentDir if normalized.pop() => {}
            Component::ParentDir => {
                return Err(path_diagnostic(
                    anchor,
                    relative,
                    "package member path escapes through its parent",
                    "package_member_parent_escape",
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(path_diagnostic(
                    anchor,
                    relative,
                    "package member path must be relative",
                    "package_member_absolute",
                ));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(path_diagnostic(
            anchor,
            relative,
            "package member path must name a file or directory",
            "package_member_empty",
        ));
    }
    Ok(normalized)
}

fn nearest_existing_ancestor(path: &Path) -> Option<&Path> {
    path.ancestors().find(|ancestor| ancestor.exists())
}

fn path_diagnostic(
    anchor: &Path,
    path: &str,
    message: &'static str,
    issue: &'static str,
) -> Diagnostic {
    Diagnostic::error(format!("{message}: {path}"))
        .with_file(anchor.display().to_string())
        .with_arg("issue", issue)
        .with_arg("path", path.to_owned())
}
