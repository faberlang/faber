//! Faber package-MIR product surface (RP5 residual).
//!
//! RP5: the compiler-owned subset of this module moved to
//! `radix-program/src/mir/` (lowering/linking/image/routes/driver/cli/diag/
//! manifest writer + test support). This module keeps the faber product
//! surface:
//!
//! - [`bin_runner`] — the generated fmir-bin runner crate writer (writes the
//!   runner crate and invokes `cargo build`; external tool invocation stays
//!   faber-side, ownership-freeze §3).
//! - [`faber_mir_seams`] — the faber-supplied seam bundle (executed-lane
//!   target, library resolver/cache, `[device]` projection, device-section
//!   builder, host-route policy, runner writer, toolchain version) that the
//!   compiler subset receives.
//! - Faber product entry adapters that inject those seams into the typed
//!   `radix-program` MIR operations. These adapters are product boundaries,
//!   not compiler compatibility modules.
#![allow(dead_code)] // Binary and library targets exercise different package runner surfaces.

pub(crate) mod bin_runner;
mod manifest;

#[cfg(test)]
#[path = "mir_test.rs"]
mod tests;

use radix_module::diagnostics::Diagnostic;
use radix_module::driver::Config;
use std::fs;
use std::path::Path;

use radix_program::mir::{mir_diag, DeviceSelection};

// Faber's fmir-bin runner writer is the seam body radix-program's binary
// bundle builder receives (`MirSeams::bin_runner_writer`).
use bin_runner::write_fmir_bin_runner;

/// The faber-supplied package-MIR seams (RP5). Every faber product input the
/// moved compiler subset needs enters as an explicit function — no ambient
/// reads inside `radix-program`.
pub(crate) fn faber_mir_seams() -> radix_program::mir::MirSeams {
    radix_program::mir::MirSeams {
        analysis: super::faber_analysis_seams(),
        executed_lane_target: super::executed_lane_analysis_target,
        library_resolver_for_config: super::library_resolver_from_config,
        library_cache_for_config: super::library_cache_with_config,
        device_config_for_input: manifest::manifest_device_config,
        device_section_builder,
        host_route,
        bin_runner_writer: write_fmir_bin_runner,
        toolchain_version: env!("CARGO_PKG_VERSION"),
    }
}

/// The faber product build of the FMIR device-section builder seam
/// (ownership-freeze §5.1): faber's `device/` subtree builds the device
/// payload. `Ok(None)` when the package declares no `[device]` surface or
/// when this build compiles no `device-runtime`.
fn device_section_builder(
    prepared: &radix_program::mir::PreparedPackageMir<'_>,
    lowered: &radix_module::mir::LoweredMirUnit<'_>,
    diagnostic_path: &Path,
) -> Result<Option<radix_mir_fmir::FmirDeviceSection>, Vec<Diagnostic>> {
    // DDPP1-U2 (C2 feature isolation): the device-section builder compiles
    // only under `device-runtime` (it consumes the faber `device/` subtree);
    // without it no package carries a device payload.
    #[cfg(feature = "device-runtime")]
    {
        if !prepared.device_declared {
            return Ok(None);
        }
        // S5-U5b: the manifest `[device] steps` channel (or the portable
        // default) is the declared repeating step count the constructor
        // admits against the source loop bound and the wire carries.
        let declared_steps = prepared
            .device_steps
            .unwrap_or(super::device::DEFAULT_TRAINING_STEPS);
        let Some((program, semantics, step_count)) = super::device::device_program_for_lowered(
            &lowered.validated,
            &lowered.interner,
            &lowered.companions,
            declared_steps,
        )?
        else {
            // The selected entry lowers to no compute kernel: no device
            // payload. An explicit GPU request fails closed at run time
            // (N1.1); `auto` keeps the CPU route.
            return Ok(None);
        };
        // S5-U5b: `[device] steps` applies only to a RepeatingStep training
        // program. A declared count on a SingleRun program is a contradiction
        // — fail closed, never a silently dropped count.
        if prepared.device_steps.is_some()
            && program.lifetime != radix_mir::device_program::DeviceProgramLifetime::RepeatingStep
        {
            return Err(vec![crate::package_diagnostic_error(
                "faber.toml device.steps applies only to a training-loop package; this package's device program is not a RepeatingStep training program",
            )
            .with_file(diagnostic_path.display().to_string())
            .with_arg("issue", "package_device_steps_not_repeating")]);
        }
        // Fail closed when a declared input is missing for an input buffer.
        // The TWO exemptions (S3-A5 + S5-U1): the companion's reverse-AD
        // upstream seed — a synthetic 1-element input whose source param
        // carries no source name — is provisioned with 1.0; and a DECOMPOSED
        // kernel's data-flow input that another kernel produces (a forward
        // intermediate) needs no external value.
        let mut device_inputs = prepared.device_inputs.clone();
        let mut upstream_seeds: Vec<(String, Vec<f32>)> = Vec::new();
        let produced: std::collections::BTreeSet<String> = program
            .kernels
            .iter()
            .flat_map(|kernel| kernel.resources.iter())
            .filter(|resource| {
                matches!(
                    resource.buffer.role,
                    radix_mir::device_program::BufferRole::Output
                )
            })
            .map(|resource| resource.buffer.name.clone())
            .collect();
        for kernel in &program.kernels {
            let Some(function) = lowered
                .validated
                .program()
                .functions
                .iter()
                .find(|function| function.id == kernel.function)
            else {
                continue;
            };
            let has_anonymous_param = function.params.iter().any(|param| {
                param.name.is_none()
                    || param.name.is_some_and(|symbol| {
                        (symbol.0 as usize) >= lowered.interner.strings().len()
                    })
            });
            if has_anonymous_param {
                for program_name in kernel.resources.iter().filter(|resource| {
                    resource.buffer.role == radix_mir::device_program::BufferRole::Input
                }) {
                    if program_name.version.element_count == 1
                        && !device_inputs.contains_key(&program_name.buffer.name)
                        && !produced.contains(&program_name.buffer.name)
                    {
                        upstream_seeds.push((program_name.buffer.name.clone(), vec![1.0]));
                    }
                }
            }
            let Ok(_signature) =
                radix_mir::abi::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
                    function, lowered.validated.validation(), &lowered.interner,
                )
            else {
                continue;
            };
            for program_name in kernel.resources.iter().filter(|resource| {
                resource.buffer.role == radix_mir::device_program::BufferRole::Input
            }) {
                if has_anonymous_param && program_name.version.element_count == 1 {
                    continue;
                }
                if !device_inputs.contains_key(&program_name.buffer.name) {
                    if produced.contains(&program_name.buffer.name) {
                        continue;
                    }
                    return Err(vec![mir_issue_diag(
                        diagnostic_path,
                        "package_device_input_missing",
                        format!(
                            "[device] inputs has no value for kernel `{}` input buffer `{}`",
                            kernel.entry, program_name.buffer.name
                        ),
                    )
                    .with_arg("buffer", program_name.buffer.name.clone())]);
                }
            }
        }
        for (name, values) in upstream_seeds {
            device_inputs.insert(name, values);
        }
        let selection = prepared.device_backend.unwrap_or(DeviceSelection::Auto);
        let section = super::device::device_section_for_program(
            &program,
            &semantics,
            &lowered.validated,
            &lowered.interner,
            super::device::DeviceSectionBuild {
                selection,
                inputs: &device_inputs,
                ptx_target: S1_6_PTX_TARGET,
                repeating_steps: step_count,
            },
        )?;
        Ok(Some(section))
    }
    #[cfg(not(feature = "device-runtime"))]
    {
        let _ = (prepared, lowered, diagnostic_path);
        Ok(None)
    }
}

/// The NVPTX target the S1-6 device images compile for (faber-side with the
/// device-section builder seam).
#[cfg(feature = "device-runtime")]
const S1_6_PTX_TARGET: &str = "sm_90";

/// The faber host-construction policy (N1.1/N1.5, S1-6): decide whether a
/// loaded image runs on the CPU/FMIR runner or a device route, and execute
/// the device route when selected. `Ok(None)` runs the CPU route with the
/// caller's host; `Ok(Some(()))` means the device route already executed.
fn host_route(
    image: &radix_program::mir::FmirPackageImage,
    selection: DeviceSelection,
) -> Result<Option<()>, Vec<Diagnostic>> {
    let requires_device = image.device.is_some();
    // DDPP1-U2 (C2 feature isolation): the device route is compiled only under
    // `device-runtime`; without it an explicit backend request or a
    // device-bearing image fails closed (never silently runs CPU).
    #[cfg(feature = "device-runtime")]
    {
        // In-process `host_factory` is parked (no hosts crate). Packed Metal
        // closeout uses `host_spawn`. An explicit backend or a device-bearing
        // image still fails closed — never a silent CPU fallback.
        if selection != DeviceSelection::Auto || requires_device {
            return Err(vec![mir_diag(
                &image.diagnostic_path,
                "device section or explicit backend selection present, but this faber build parks the in-process host; packed closeout uses the spawn client",
            )]);
        }
        Ok(None)
    }
    #[cfg(not(feature = "device-runtime"))]
    {
        if selection != DeviceSelection::Auto || requires_device {
            return Err(vec![mir_diag(
                &image.diagnostic_path,
                "device section or explicit backend selection present, but this faber build compiles no `device-runtime`; rebuild with the `device-runtime` feature to execute device routes",
            )]);
        }
        Ok(None)
    }
}

/// Public package-MIR callback for external backend harnesses: forward to the
/// moved compiler driver with the faber seams.
pub fn with_lowered_package_mir<R>(
    config: &Config,
    input: &Path,
    run: impl for<'a> FnOnce(&radix_module::mir::LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    radix_program::mir::with_lowered_package_mir(config, input, &faber_mir_seams(), run)
}

/// The interpreted-consumer probe used by faber's package consumers.
#[cfg(feature = "runner")]
pub(crate) fn with_interpreted_lowered_package_mir<R>(
    config: &Config,
    input: &Path,
    run: impl for<'a> FnOnce(&radix_module::mir::LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    radix_program::mir::with_interpreted_lowered_package_mir(config, input, &faber_mir_seams(), run)
}

/// Link an already-analyzed package test scope once, then lend its validated
/// MIR and original test-function identity map to the runner consumer.
#[cfg(feature = "runner")]
pub(crate) fn with_linked_test_package_mir<R>(
    package: radix_program::analyze::AnalyzedProgram,
    scope: &radix_program::mir::TestPackageScope,
    config: &Config,
    run: impl for<'a> FnOnce(
        &radix_module::mir::ValidatedMir<'a>,
        &radix_program::mir::TestFunctionIds,
    ) -> R,
) -> Result<R, Vec<Diagnostic>> {
    radix_program::mir::with_linked_test_package_mir(
        package,
        scope,
        config,
        &faber_mir_seams(),
        run,
    )
}

/// Generated fmir-bin runner crates consume this public API (the stdio
/// entry). Forward to the moved route with the faber seams.
pub fn run_fmir_image_bytes_with_stdio(
    image_bytes: &[u8],
    diagnostic_path: &Path,
    argumenta: Vec<String>,
) -> Result<(), Vec<Diagnostic>> {
    radix_program::mir::run_fmir_image_bytes_with_stdio(
        image_bytes,
        diagnostic_path,
        argumenta,
        &faber_mir_seams(),
    )
}

// Runner-crate constants stay faber-side (the toolchain version is the
// product version recorded in the generated runner's Cargo manifest).
pub(crate) const PACKAGE_MIR_TOOLCHAIN_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const FMIR_BIN_RUNNER_CRATE_DIR: &str = "runner";
pub(crate) const FMIR_BIN_RUNNER_PACKAGE_NAME: &str = "faber-fmir-bin-runner";

/// Diagnostic constructors used by the faber seam bodies.
pub(crate) fn mir_issue_diag(
    path: &Path,
    issue: &'static str,
    message: impl Into<String>,
) -> Diagnostic {
    mir_diag(path, message).with_arg("issue", issue)
}
