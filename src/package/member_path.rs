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

#[cfg(test)]
mod tests {
    use super::resolve_package_member;
    use std::error::Error;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> Result<PathBuf, Box<dyn Error>> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(Box::<dyn Error>::from)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!("faber-member-path-{label}-{nonce}"));
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn has_arg(diagnostic: &radix::diagnostics::Diagnostic, name: &str, value: &str) -> bool {
        diagnostic
            .args
            .iter()
            .any(|arg| arg.name == name && arg.value == value)
    }

    #[test]
    fn resolve_package_member_rejects_parent_escape() -> Result<(), Box<dyn Error>> {
        let root = temp_dir("parent-escape")?;
        let package_root = root.join("pkg");
        fs::create_dir_all(package_root.join("src"))?;
        let anchor = package_root.join("faber.toml");
        fs::write(&anchor, "")?;

        let diagnostic = resolve_package_member(&package_root, "../outside.fab", &anchor)
            .expect_err("parent escape should be rejected");

        assert!(
            has_arg(&diagnostic, "issue", "package_member_parent_escape"),
            "expected parent escape issue, got {diagnostic:?}"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn resolve_package_member_rejects_symlink_escape() -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::symlink;

        let root = temp_dir("symlink-escape")?;
        let package_root = root.join("pkg");
        let outside = root.join("outside");
        fs::create_dir_all(package_root.join("src"))?;
        fs::create_dir_all(&outside)?;
        fs::write(outside.join("escaped.fab"), "nota \"outside\"\n")?;
        symlink(&outside, package_root.join("src").join("linked"))?;
        let anchor = package_root.join("faber.toml");
        fs::write(&anchor, "")?;

        let diagnostic = resolve_package_member(&package_root, "src/linked/escaped.fab", &anchor)
            .expect_err("symlink escape should be rejected");

        assert!(
            has_arg(&diagnostic, "issue", "package_member_symlink_escape"),
            "expected symlink escape issue, got {diagnostic:?}"
        );
        Ok(())
    }
}
