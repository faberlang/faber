//! Product half of the package-MIR artifact manifest surface (RP5).
//!
//! RP5: the compiler half of this module (artifact root/dir helpers, the
//! `image.toml` writer, manifest validation, TOML escaping) moved to
//! `radix-program/src/mir/manifest.rs`. The `[device]` manifest projection
//! stays faber-side (ownership-freeze §4/§5.2): the product view reads the
//! `[device]` table through faber's manifest helpers, and the compiler subset
//! receives the resolved config through [`radix::program::mir::MirSeams`].

use std::path::Path;

use radix::module::diagnostics::Diagnostic;

use radix::program::mir::DeviceManifestConfig;

/// Read the package's `[device]` surface for the S1-6 device payload:
/// `[device] inputs` (typed f32 host inputs), `[device] backend` (the
/// selection request recorded in the image), and `[device] steps` (the
/// S5-U5b declared training step count). Absent manifest → no device
/// declaration.
pub(super) fn manifest_device_config(
    input: &Path,
) -> Result<DeviceManifestConfig, Vec<Diagnostic>> {
    let layout =
        super::super::discover_build_layout(input).map_err(|diagnostic| vec![*diagnostic])?;
    if !layout.manifest_path.exists() {
        return Ok(DeviceManifestConfig::default());
    }
    let manifest = super::super::read_manifest(&layout.manifest_path)
        .map_err(|diagnostic| vec![*diagnostic])?;
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
    Ok(DeviceManifestConfig {
        inputs,
        backend,
        steps,
        declared,
    })
}
