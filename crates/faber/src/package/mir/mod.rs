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

#[cfg(feature = "runner")]
pub(crate) mod bin_runner;
#[cfg(feature = "runner")]
mod manifest;

#[cfg(test)]
#[path = "mir_test.rs"]
mod tests;

#[cfg(feature = "runner")]
use radix::module::diagnostics::Diagnostic;
#[cfg(feature = "runner")]
use radix::module::driver::Config;
#[cfg(feature = "runner")]
use std::fs;
#[cfg(feature = "runner")]
use std::path::Path;

#[cfg(feature = "runner")]
use radix::program::mir::{DeviceSelection, mir_diag};

// Faber's fmir-bin runner writer is the seam body radix-program's binary
// bundle builder receives (`MirSeams::bin_runner_writer`).
#[cfg(feature = "runner")]
use bin_runner::write_fmir_bin_runner;

/// The faber-supplied package-MIR seams (RP5). Every faber product input the
/// moved compiler subset needs enters as an explicit function — no ambient
/// reads inside `radix-program`.
#[cfg(feature = "runner")]
pub fn faber_mir_seams() -> radix::program::mir::MirSeams {
    radix::program::mir::MirSeams {
        analysis: super::faber_analysis_seams(),
        executed_lane_target: super::executed_lane_analysis_target,
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
#[cfg(feature = "runner")]
fn device_section_builder(
    prepared: &radix::program::mir::PreparedPackageMir<'_>,
    lowered: &radix::module::mir::LoweredMirUnit<'_>,
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
        let Some((program, semantics, step_count)) =
            super::device::device_program_for_lowered_with_metadata(
                &lowered.validated,
                &lowered.interner,
                &lowered.companions,
                declared_steps,
                &lowered.device_function_metadata,
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
            let has_anonymous_param = function.params.iter().any(|param| param.name.is_none());
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
                    return Err(vec![
                        mir_issue_diag(
                            diagnostic_path,
                            "package_device_input_missing",
                            format!(
                                "[device] inputs has no value for kernel `{}` input buffer `{}`",
                                kernel.entry, program_name.buffer.name
                            ),
                        )
                        .with_arg("buffer", program_name.buffer.name.clone()),
                    ]);
                }
            }
        }
        for (name, values) in upstream_seeds {
            device_inputs.insert(name, values);
        }
        let selection = prepared.device_selection.unwrap_or(DeviceSelection::Auto);
        let section = super::device::device_section_for_program(
            &program,
            &semantics,
            &lowered.validated,
            &lowered.interner,
            super::device::DeviceSectionBuild {
                selection,
                inputs: &device_inputs,
                ptx_target: super::device::ptx_target_for(None),
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

/// The faber host-construction policy (N1.1/N1.5, S1-6): decide whether a
/// loaded image runs on the CPU/FMIR runner or a device route, and execute
/// the device route when selected. `Ok(None)` runs the CPU route with the
/// caller's host; `Ok(Some(()))` means the device route already executed.
/// `stats_requested` is the explicit CLI-originated fact transported by the
/// radix-program route (RSM-2); a stats request on a route that will not run
/// the interpreter — a device-bearing image (device route) or an explicit
/// device selection — refuses here before any device callback/launch
/// (RSM-4, fail closed; never silently empty numbers).
#[cfg(feature = "runner")]
fn host_route(
    image: &radix::program::mir::FmirPackageImage,
    selection: DeviceSelection,
    stats_requested: bool,
) -> Result<Option<()>, Vec<Diagnostic>> {
    // The cpu selection runs the program on the MIR runner: no device route is
    // taken even when the image carries a device section, and stats are the
    // ordinary runner stats.
    if selection == DeviceSelection::Cpu {
        return Ok(None);
    }
    if stats_requested && (image.device.is_some() || selection != DeviceSelection::Auto) {
        return Err(vec![mir_issue_diag(
            &image.diagnostic_path,
            "run_stats_device_route_unsupported",
            "run statistics (`--stats`/`--stats-file`) require the interpreted CPU route; this image executes a device route through the host driver that never runs the interpreter, so the stats request is refused",
        )]);
    }
    // DDPP1-U2 (C2 feature isolation): the device route is compiled only under
    // `device-runtime`; without it an explicit device request or a
    // device-bearing image fails closed (never silently runs CPU).
    #[cfg(feature = "device-runtime")]
    {
        if let Some(device) = &image.device {
            super::execute_device_route(device, selection, &image.source_hashes)?;
            return Ok(Some(()));
        }
        if selection != DeviceSelection::Auto {
            return Err(vec![mir_diag(
                &image.diagnostic_path,
                "explicit device selection requires a device image; CPU-only images never silently fall back",
            )]);
        }
        Ok(None)
    }
    #[cfg(not(feature = "device-runtime"))]
    {
        let requires_device = image.device.is_some();
        if selection != DeviceSelection::Auto || requires_device {
            return Err(vec![mir_diag(
                &image.diagnostic_path,
                "device section or explicit device selection present, but this faber build compiles no `device-runtime`; rebuild with the `device-runtime` feature to execute device routes",
            )]);
        }
        Ok(None)
    }
}

/// Public package-MIR callback for external backend harnesses: forward to the
/// moved compiler driver with the faber seams.
#[cfg(feature = "runner")]
pub fn with_lowered_package_mir<R>(
    config: &Config,
    input: &Path,
    run: impl for<'a> FnOnce(&radix::module::mir::LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    radix::program::mir::with_lowered_package_mir(config, input, &faber_mir_seams(), run)
}

/// The interpreted-consumer probe used by faber's package consumers.
#[cfg(feature = "runner")]
pub(crate) fn with_interpreted_lowered_package_mir<R>(
    config: &Config,
    input: &Path,
    run: impl for<'a> FnOnce(&radix::module::mir::LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    radix::program::mir::with_interpreted_lowered_package_mir(
        config,
        input,
        &faber_mir_seams(),
        run,
    )
}

/// Link an already-analyzed package test scope once, then lend its validated
/// MIR and original test-function identity map to the runner consumer.
#[cfg(feature = "runner")]
pub fn with_linked_test_package_mir<R>(
    package: radix::program::analyze::AnalyzedProgram,
    scope: &radix::program::mir::TestPackageScope,
    config: &Config,
    run: impl for<'a> FnOnce(
        &radix::module::mir::ValidatedMir<'a>,
        &radix::program::mir::TestFunctionIds,
    ) -> R,
) -> Result<R, Vec<Diagnostic>> {
    radix::program::mir::with_linked_test_package_mir(
        package,
        scope,
        config,
        &faber_mir_seams(),
        run,
    )
}

/// Generated fmir-bin runner crates consume this public API (the stdio
/// entry). Forward to the moved route with the faber seams.
#[cfg(feature = "runner")]
pub fn run_fmir_image_bytes_with_stdio(
    image_bytes: &[u8],
    diagnostic_path: &Path,
    argumenta: Vec<String>,
) -> Result<(), Vec<Diagnostic>> {
    radix::program::mir::run_fmir_image_bytes_with_stdio(
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
#[cfg(feature = "runner")]
pub(crate) fn mir_issue_diag(
    path: &Path,
    issue: &'static str,
    message: impl Into<String>,
) -> Diagnostic {
    mir_diag(path, message).with_arg("issue", issue)
}

/// The in-process device run (gpu-reset C2-U6b): lower the package to MIR,
/// open a [`DeviceSession`](radix_device_core::session::DeviceSession) through
/// the faber device seam, build the per-kernel device image, and run `main` on
/// the MIR runner under `RunnerKernelPolicy::Dispatch` through a
/// [`SessionKernelDispatcher`](super::device::SessionKernelDispatcher). No child
/// process is spawned and no `[device]` table is required (D1). A call to a
/// kernel that did not lower fails closed at the call, naming the function
/// (D3). An unavailable explicit device fails closed before anything lowers
/// onto it; there is no fallback.
///
/// Returns the discovery receipt line that was printed.
///
/// # Errors
/// Structured diagnostics: device admission (`E_DEVICE_UNAVAILABLE`,
/// `E_DEVICE_NONE_ADMITTED`, ...), image construction, or the runner's error.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub fn run_package_on_device_session<H: radix_mir_runner::Host + ?Sized>(
    config: &Config,
    input: &Path,
    selection: DeviceSelection,
    registry: &mut radix_device_core::registry::DeviceRegistry,
    host: &mut H,
) -> Result<String, Vec<Diagnostic>> {
    use super::device::{DeviceImageBackend, SessionKernelDispatcher, device_image_for_lowered};
    use super::host_factory::{discovery_receipt, host_error_diagnostic, open_device_selection};
    use radix_device_core::backend::{DeviceBackend, DeviceClass};
    use radix_device_core::partition::MemoryBudget;
    use radix_device_core::session::DeviceSession;

    // The device route binds no program arguments yet: fail closed rather
    // than run with operands silently unbound.
    let has_args = host.argumenta().map(|args| !args.is_empty()).unwrap_or(false);
    if has_args {
        return Err(vec![mir_issue_diag(
            input,
            "device_run_program_args_unsupported",
            "program arguments are not supported on the in-process device route yet",
        )]);
    }
    // Admission first: an unavailable explicit device fails before lowering
    // work is spent on it.
    let opened = open_device_selection(
        registry,
        selection,
        true,
        MemoryBudget::new(DEVICE_RUN_MEMORY_BUDGET_BYTES),
    )
    .map_err(|diagnostic| vec![diagnostic])?
    .ok_or_else(|| vec![mir_diag(input, "no device was admitted for the device route")])?;
    let receipt = discovery_receipt(&opened);
    let backend = match receipt.class {
        DeviceClass::Backend(DeviceBackend::Metal) => DeviceImageBackend::Metal,
        other => {
            return Err(vec![
                mir_issue_diag(
                    input,
                    "E_DEVICE_UNSUPPORTED",
                    format!(
                        "no per-kernel device image backend for `{}` yet",
                        other.spelling()
                    ),
                )
                .with_arg("code", "E_DEVICE_UNSUPPORTED".to_owned()),
            ]);
        }
    };
    let session = DeviceSession::from_opened(opened)
        .map_err(|error| vec![host_error_diagnostic(&error)])?;
    receipt.print();

    let mut session = Some(session);
    with_interpreted_lowered_package_mir(config, input, |lowered| {
        let (image, omitted) = device_image_for_lowered(lowered, backend)?;
        if image.kernels.is_empty() {
            let reasons = omitted
                .iter()
                .map(|(_, reason)| reason.clone())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(vec![
                mir_issue_diag(
                    input,
                    "E_DEVICE_NONE_ADMITTED",
                    format!("no kernel of this package lowers to the device: {reasons}"),
                )
                .with_arg("code", "E_DEVICE_NONE_ADMITTED".to_owned()),
            ]);
        }
        let validated = &lowered.validated;
        let names = radix_mir::names::MirNames::new(
            validated.program(),
            validated.validation().types.as_table(),
            &lowered.interner,
        );
        let omissions: Vec<(radix_mir::MirFunctionId, String)> = omitted
            .iter()
            .map(|(id, reason)| {
                let name = validated
                    .program()
                    .functions
                    .iter()
                    .find(|function| function.id == *id)
                    .and_then(|function| function.name)
                    .map_or_else(|| id.0.to_string(), |s| lowered.interner.resolve(s).to_owned());
                (
                    *id,
                    format!("kernel `{name}` has no device kernel: {reason}"),
                )
            })
            .collect();
        let entry = names
            .explicit_entry_function()
            .map(|function| function.id)
            .ok_or_else(|| vec![mir_diag(input, "no entry function in MIR program")])?;
        let units = image.kernels.into_iter().map(|(_, unit)| unit).collect();
        let dispatcher = SessionKernelDispatcher::new(
            session.take().expect("session is taken once"),
            image.artifact,
            units,
            validated.validation().types.as_table(),
        )
        .map_err(|error| vec![host_error_diagnostic(&error)])?;
        dispatcher.borrow_mut().set_omissions(omissions);
        let hook: radix_mir_runner::kernel_dispatch::ResidencyHook = dispatcher.clone();
        radix_mir_runner::run_function_with_kernel_dispatcher(validated, entry, host, &hook)
            .map_err(|errors| {
                errors
                    .into_iter()
                    .map(|error| {
                        mir_issue_diag(input, "device_run_failed", error.message)
                            .with_arg("code", error.issue)
                    })
                    .collect::<Vec<_>>()
            })
    })
    .and_then(|result| result)?;
    Ok(receipt.render())
}

/// Device-route memory budget declared at admission (the registry's policy
/// ceiling is the declared budget itself).
#[cfg(all(feature = "runner", feature = "device-runtime"))]
const DEVICE_RUN_MEMORY_BUDGET_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Register the system device runtimes this build and platform provide
/// (Metal on macOS). CUDA joins when its per-kernel image backend lands.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub fn system_device_registry() -> radix_device_core::registry::DeviceRegistry {
    #[allow(unused_mut)]
    let mut registry = radix_device_core::registry::DeviceRegistry::new();
    #[cfg(target_os = "macos")]
    if let Some(info) = radix_device_metal::system_devices().into_iter().next()
        && let Ok(driver) = radix_device_metal::SystemMetalDriver::open(info.registry_id)
    {
        registry.register(Box::new(radix_device_metal::MetalRuntime::new(driver)));
    }
    registry
}
