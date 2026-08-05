//! Package-MIR artifact root, manifest writing, and manifest helpers.

use super::*;

pub(super) fn package_artifact_root(input: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    super::super::discover_build_layout(input)
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

/// Read the package's `[device]` surface for the S1-6 device payload:
/// `[device] inputs` (typed f32 host inputs), `[device] backend` (the
/// selection request recorded in the image), and `[device] steps` (the
/// S5-U5b declared training step count). Absent manifest → no device
/// declaration.
pub(super) fn manifest_device_config(
    input: &Path,
) -> Result<
    (
        BTreeMap<String, Vec<f32>>,
        Option<faber::device::DeviceSelection>,
        Option<u32>,
        bool,
    ),
    Vec<Diagnostic>,
> {
    let layout = super::super::discover_build_layout(input).map_err(|diagnostic| vec![*diagnostic])?;
    if !layout.manifest_path.exists() {
        return Ok((BTreeMap::new(), None, None, false));
    }
    let manifest =
        super::super::read_manifest(&layout.manifest_path).map_err(|diagnostic| vec![*diagnostic])?;
    let inputs = super::super::manifest_device_inputs(&manifest.device.inputs);
    let backend = super::super::manifest_backend_selection(
        manifest.device.backend.as_deref(),
        &layout.manifest_path,
    )
    .map_err(|diagnostic| vec![*diagnostic])?;
    // S5-U5b: a zero declared step count is a contradiction (a RepeatingStep
    // program must drive at least one step) — fail closed at the point of
    // use, never treated as "absent".
    let steps = manifest.device.steps;
    if let Some(0) = steps {
        return Err(vec![crate::package_diagnostic_error(
            "faber.toml device.steps must be at least 1",
        )
        .with_file(layout.manifest_path.display().to_string())
        .with_arg("issue", "package_device_steps_zero")]);
    }
    let declared = !inputs.is_empty() || backend.is_some();
    Ok((inputs, backend, steps, declared))
}

pub(super) fn package_mir_manifest(prepared: &PreparedPackageMir<'_>, package_root: &Path) -> String {
    let entry = escape_manifest_value(&relative_or_display(package_root, &prepared.entry_path));
    let mut manifest = format!(
        "version = {}\ntarget = \"{}\"\nentry = \"{}\"\nentry_function = \"run_entry\"\n\n[runtime]\n",
        PACKAGE_MIR_ARTIFACT_VERSION, PACKAGE_MIR_TARGET_NAME, entry
    );
    for requirement in &prepared.runtime_requirements {
        manifest.push_str(&format!(
            "requirement = \"{}\"\n",
            escape_manifest_value(requirement)
        ));
    }
    manifest.push_str("\n[sources]\n");
    let mut sources = prepared
        .source_paths
        .iter()
        .map(|source| relative_or_display(package_root, source))
        .collect::<Vec<_>>();
    sources.sort();
    for source in sources {
        manifest.push_str(&format!("file = \"{}\"\n", escape_manifest_value(&source)));
    }
    manifest
}

pub(super) fn relative_or_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// TOML basic-string escaping (backslash + double quote), shared by the
/// manifest-value and path writers.
pub(super) fn escape_toml_basic_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(super) fn escape_manifest_value(value: &str) -> String {
    escape_toml_basic_string(value)
}

pub(super) fn validate_package_mir_manifest(manifest: &str, path: &Path) -> Result<(), Vec<Diagnostic>> {
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

