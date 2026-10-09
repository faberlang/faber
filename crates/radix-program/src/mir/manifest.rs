//! Package-MIR artifact root, manifest writing, and manifest helpers.

use super::{
    Diagnostic, PACKAGE_MIR_ARTIFACT_DIR, PACKAGE_MIR_ARTIFACT_VERSION, PACKAGE_MIR_TARGET_NAME,
    Path, PathBuf, PreparedPackageMir, fs, mir_diag, mir_issue_diag,
};

pub(super) fn package_artifact_root(input: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    crate::discovery::discover_build_layout(input)
        .map(|layout| layout.package_root)
        .map_err(|diagnostic| vec![*diagnostic])
}

/// `<package_root>/target/<faber-mir>/[<subdir>]` artifact directory for a
/// package build, created on demand.
pub(super) fn package_artifact_dir(
    package_root: &Path,
    diagnostic_path: &Path,
    subdir: &str,
) -> Result<PathBuf, Vec<Diagnostic>> {
    let artifact_root = package_root
        .join("target")
        .join(PACKAGE_MIR_ARTIFACT_DIR)
        .join(subdir);
    fs::create_dir_all(&artifact_root)
        .map_err(|error| vec![mir_diag(diagnostic_path, error.to_string())])?;
    Ok(artifact_root)
}

/// Write a package FMIR image artifact under `artifact_root` and return its
/// path.
pub(super) fn write_package_image(
    artifact_root: &Path,
    diagnostic_path: &Path,
    file: &str,
    bytes: impl AsRef<[u8]>,
) -> Result<PathBuf, Vec<Diagnostic>> {
    let image_path = artifact_root.join(file);
    fs::write(&image_path, bytes)
        .map_err(|error| vec![mir_diag(diagnostic_path, error.to_string())])?;
    Ok(image_path)
}

pub(super) fn package_mir_manifest(
    prepared: &PreparedPackageMir<'_>,
    package_root: &Path,
) -> String {
    let entry = escape_manifest_value(&relative_or_display(package_root, &prepared.entry_path));
    let mut manifest = format!(
        "version = {PACKAGE_MIR_ARTIFACT_VERSION}\ntarget = \"{PACKAGE_MIR_TARGET_NAME}\"\nentry = \"{entry}\"\nentry_function = \"run_entry\"\n\n[runtime]\n"
    );
    for requirement in &prepared.runtime_requirements {
        manifest.push_str("requirement = \"");
        manifest.push_str(&escape_manifest_value(requirement));
        manifest.push_str("\"\n");
    }
    manifest.push_str("\n[sources]\n");
    let mut sources = prepared
        .source_paths
        .iter()
        .map(|source| relative_or_display(package_root, source))
        .collect::<Vec<_>>();
    sources.sort();
    for source in sources {
        manifest.push_str("file = \"");
        manifest.push_str(&escape_manifest_value(&source));
        manifest.push_str("\"\n");
    }
    manifest
}

#[must_use]
pub fn relative_or_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// TOML basic-string escaping (backslash + double quote), shared by the
/// manifest-value and path writers.
#[must_use]
pub fn escape_toml_basic_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(super) fn escape_manifest_value(value: &str) -> String {
    escape_toml_basic_string(value)
}

pub(super) fn validate_package_mir_manifest(
    manifest: &str,
    path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let has_version = manifest
        .lines()
        .any(|line| line.trim() == format!("version = {PACKAGE_MIR_ARTIFACT_VERSION}"));
    let has_target = manifest
        .lines()
        .any(|line| line.trim() == format!("target = \"{PACKAGE_MIR_TARGET_NAME}\""));
    let has_entry = manifest
        .lines()
        .any(|line| line.trim_start().starts_with("entry = \""));
    let has_runtime = manifest.lines().any(|line| line.trim() == "[runtime]");
    if has_version && has_target && has_entry && has_runtime {
        return Ok(());
    }
    Err(vec![mir_issue_diag(
        path,
        "package_mir_artifact_manifest_metadata_missing",
        "package MIR artifact manifest is missing required v1 metadata",
    )])
}
