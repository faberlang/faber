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

#[cfg(all(
    test,
    feature = "runner",
    feature = "device-runtime",
    target_os = "macos"
))]
#[path = "auto_device_test.rs"]
mod auto_device_tests;

#[cfg(feature = "runner")]
use radix::module::diagnostics::Diagnostic;
#[cfg(feature = "runner")]
use radix::module::driver::Config;
#[cfg(feature = "runner")]
use std::fs;
#[cfg(feature = "runner")]
use std::path::Path;

#[cfg(feature = "device-runtime")]
use radix::program::mir::DeviceSelection;
#[cfg(feature = "runner")]
use radix::program::mir::mir_diag;

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
        bin_runner_writer: write_fmir_bin_runner,
        toolchain_version: env!("CARGO_PKG_VERSION"),
    }
}

/// The faber product build of the FMIR device-section builder seam
/// (ownership-freeze §5.1): faber's `device/` subtree builds the device
/// payload. `Ok(None)` when the package declares no `[device]` surface or
/// when this build compiles no `device-runtime`.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn device_section_builder(
    prepared: &radix::program::mir::PreparedPackageMir<'_>,
    lowered: &radix::module::mir::LoweredMirUnit<'_>,
    diagnostic_path: &Path,
) -> Result<Option<radix_mir_fmir::FmirDeviceSection>, Vec<Diagnostic>> {
    // DDPP1-U2 (C2 feature isolation): the device-section builder compiles
    // only under `device-runtime` (it consumes the faber `device/` subtree);
    // without it no package carries a device payload.
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
    let device_inputs = resolve_device_inputs(prepared, lowered, &program, diagnostic_path)?;
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

/// Resolve the `[device] inputs` map for `program`: fail closed on a declared
/// input buffer with no value. The TWO exemptions (S3-A5 + S5-U1): the
/// companion's reverse-AD upstream seed — a synthetic 1-element input whose
/// source param carries no source name — is provisioned with 1.0; and a
/// DECOMPOSED kernel's data-flow input that another kernel produces (a forward
/// intermediate) needs no external value.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn resolve_device_inputs(
    prepared: &radix::program::mir::PreparedPackageMir<'_>,
    lowered: &radix::module::mir::LoweredMirUnit<'_>,
    program: &radix_mir::device_program::DeviceProgram,
    diagnostic_path: &Path,
) -> Result<std::collections::BTreeMap<String, Vec<f32>>, Vec<Diagnostic>> {
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
        if radix_mir::abi::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            function,
            lowered.validated.validation(),
            &lowered.interner,
        )
        .is_err()
        {
            continue;
        }
        for program_name in kernel
            .resources
            .iter()
            .filter(|resource| resource.buffer.role == radix_mir::device_program::BufferRole::Input)
        {
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
    Ok(device_inputs)
}

/// Without `device-runtime` (DDPP1-U2, C2 feature isolation) no package
/// carries a device payload: the builder is the constant `Ok(None)`.
#[cfg(all(feature = "runner", not(feature = "device-runtime")))]
fn device_section_builder(
    _prepared: &radix::program::mir::PreparedPackageMir<'_>,
    _lowered: &radix::module::mir::LoweredMirUnit<'_>,
    _diagnostic_path: &Path,
) -> Result<Option<radix_mir_fmir::FmirDeviceSection>, Vec<Diagnostic>> {
    Ok(None)
}

/// Public package-MIR callback for external backend harnesses: forward to the
/// moved compiler driver with the faber seams.
///
/// # Errors
/// Diagnostics when the input or persisted artifact is invalid.
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
///
/// # Errors
/// Diagnostics when the input or persisted artifact is invalid.
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
///
/// # Errors
/// Diagnostics when the image bytes or the persisted artifact is invalid.
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
    run_package_on_device_session_with_stats(config, input, selection, registry, host, false)
        .map(|run| run.receipt)
}

/// The result of an in-process device run: the discovery receipt line that was
/// printed, and the host-side meter when one was requested.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
#[derive(Debug)]
pub struct DeviceSessionRun {
    pub receipt: String,
    /// Host rows only. Kernel dispatches run on the device and are not
    /// measured (gpu-reset C2-U6c, D4).
    pub stats: Option<radix_mir_runner::RunnerStats>,
}

/// [`run_package_on_device_session`] with the opt-in host meter
/// (`collect_stats`). Program arguments on `host` bind into the entry record
/// and the package's declared CLI exit code is applied on success, as on the
/// other package routes.
///
/// # Errors
/// As [`run_package_on_device_session`], plus argument-binding failures.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub fn run_package_on_device_session_with_stats<H: radix_mir_runner::Host + ?Sized>(
    config: &Config,
    input: &Path,
    selection: DeviceSelection,
    registry: &mut radix_device_core::registry::DeviceRegistry,
    host: &mut H,
    collect_stats: bool,
) -> Result<DeviceSessionRun, Vec<Diagnostic>> {
    // Admission first: an unavailable explicit device fails before lowering
    // work is spent on it.
    let admitted = admit_device_session(input, selection, registry)?;
    let receipt = admitted.receipt.clone();
    let stats = radix::program::mir::with_interpreted_bound_package_mir(
        config,
        input,
        host,
        &faber_mir_seams(),
        |lowered, validated, exit_code, host| {
            run_admitted_device_session(
                input,
                admitted,
                lowered,
                validated,
                exit_code,
                host,
                collect_stats,
            )
        },
    )?;
    Ok(DeviceSessionRun { receipt, stats })
}

/// [`run_package_on_device_session_with_stats`] for the `auto` selection: the
/// package is prepared once, and the lowered unit that answers "does it declare
/// a kernel" ([`lowered_declares_kernels`]) is the unit the device run
/// executes. `Ok(None)` means the package declares no kernel (nothing was
/// admitted or run; the CPU route runs it), exactly as
/// [`package_declares_kernels`] answers. Admission happens only once a kernel
/// is declared, so an `auto` run still fails closed with
/// `E_DEVICE_NONE_ADMITTED` and never picks the CPU for a kernel package.
///
/// A failure of the bound preparation itself (for example program arguments
/// that do not bind) carries no kernel answer; it falls back to the
/// stand-alone question so the outcome is the one the two-pass route gave.
///
/// # Errors
/// As [`run_package_on_device_session_with_stats`].
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub fn run_package_on_device_session_if_kernels<H: radix_mir_runner::Host + ?Sized>(
    config: &Config,
    input: &Path,
    registry: &mut radix_device_core::registry::DeviceRegistry,
    host: &mut H,
    collect_stats: bool,
) -> Result<Option<DeviceSessionRun>, Vec<Diagnostic>> {
    let prepared = radix::program::mir::with_interpreted_bound_package_mir(
        config,
        input,
        host,
        &faber_mir_seams(),
        |lowered, validated, exit_code, host| {
            if !lowered_declares_kernels(lowered) {
                return Ok(Ok(None));
            }
            Ok(
                admit_device_session(input, DeviceSelection::Auto, registry).and_then(|admitted| {
                    let receipt = admitted.receipt.clone();
                    run_admitted_device_session(
                        input,
                        admitted,
                        lowered,
                        validated,
                        exit_code,
                        host,
                        collect_stats,
                    )
                    .map(|stats| Some(DeviceSessionRun { receipt, stats }))
                }),
            )
        },
    );
    match prepared {
        Ok(outcome) => outcome,
        Err(_) if package_declares_kernels(config, input).unwrap_or(false) => {
            run_package_on_device_session_with_stats(
                config,
                input,
                DeviceSelection::Auto,
                registry,
                host,
                collect_stats,
            )
            .map(Some)
        }
        Err(_) => Ok(None),
    }
}

/// An admitted, opened device session: the discovery receipt line has been
/// printed, and the session waits for the lowered unit to dispatch.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
struct AdmittedDeviceSession {
    session: radix_device_core::session::DeviceSession,
    backend: super::device::DeviceImageBackend,
    receipt: String,
}

/// Open the selected device, print the discovery receipt, and pick the
/// per-kernel image backend.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn admit_device_session(
    input: &Path,
    selection: DeviceSelection,
    registry: &mut radix_device_core::registry::DeviceRegistry,
) -> Result<AdmittedDeviceSession, Vec<Diagnostic>> {
    use super::device::DeviceImageBackend;
    use super::host_factory::{discovery_receipt, host_error_diagnostic, open_device_selection};
    use radix_device_core::backend::{DeviceBackend, DeviceClass};
    use radix_device_core::partition::MemoryBudget;
    use radix_device_core::session::DeviceSession;

    let opened = open_device_selection(
        registry,
        selection,
        true,
        MemoryBudget::new(DEVICE_RUN_MEMORY_BUDGET_BYTES),
    )
    .map_err(|diagnostic| vec![diagnostic])?
    .ok_or_else(|| {
        vec![mir_diag(
            input,
            "no device was admitted for the device route",
        )]
    })?;
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
    let session =
        DeviceSession::from_opened(opened).map_err(|error| vec![host_error_diagnostic(&error)])?;
    receipt.print();
    Ok(AdmittedDeviceSession {
        session,
        backend,
        receipt: receipt.render(),
    })
}

/// Build the device image from the lowered unit and run the argument-bound
/// entry on the MIR runner under kernel dispatch through the admitted session.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn run_admitted_device_session<H: radix_mir_runner::Host + ?Sized>(
    input: &Path,
    admitted: AdmittedDeviceSession,
    lowered: &radix::module::mir::LoweredMirUnit<'_>,
    validated: &radix::module::mir::ValidatedMir<'_>,
    exit_code: Option<i32>,
    host: &mut H,
    collect_stats: bool,
) -> Result<Option<radix_mir_runner::RunnerStats>, Vec<Diagnostic>> {
    use super::device::{SessionKernelDispatcher, device_image_for_lowered};
    use super::host_factory::host_error_diagnostic;

    let AdmittedDeviceSession {
        session, backend, ..
    } = admitted;
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
    let interner = validated
        .validation()
        .interner
        .as_interner()
        .ok_or_else(|| vec![mir_diag(input, "device run requires interner context")])?;
    let names = radix_mir::names::MirNames::new(
        validated.program(),
        validated.validation().types.as_table(),
        interner,
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
                .map_or_else(|| id.0.to_string(), |s| interner.resolve(s).to_owned());
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
    let extents = image.extents;
    let genus_layouts = image.genus_layouts;
    let fault_sites = image.fault_sites;
    let units = image.kernels.into_iter().map(|(_, unit)| unit).collect();
    let dispatcher = SessionKernelDispatcher::new(
        session,
        image.artifact,
        units,
        validated.validation().types.as_table(),
    )
    .map_err(|error| vec![host_error_diagnostic(&error)])?;
    dispatcher.borrow_mut().set_omissions(omissions);
    dispatcher.borrow_mut().set_extents(extents);
    super::device::install_fault_sites(&fault_sites, |entry, sites| {
        dispatcher.borrow_mut().set_fault_sites(entry, sites);
    });
    dispatcher
        .borrow_mut()
        .set_genus_layouts(genus_layouts, interner.clone());
    let hook: radix_mir_runner::kernel_dispatch::ResidencyHook = dispatcher.clone();
    let runner_diagnostics = |errors: Vec<radix_mir_runner::RunnerError>| {
        errors
            .into_iter()
            .map(|error| {
                mir_issue_diag(input, "device_run_failed", error.message)
                    .with_arg("code", error.issue)
            })
            .collect::<Vec<_>>()
    };
    let collected = if collect_stats {
        let outcome = radix_mir_runner::run_function_with_kernel_dispatcher_and_stats(
            validated, entry, host, &hook,
        )
        .map_err(runner_diagnostics)?;
        outcome.result.map_err(runner_diagnostics)?;
        Some(outcome.stats)
    } else {
        radix_mir_runner::run_function_with_kernel_dispatcher(validated, entry, host, &hook)
            .map_err(runner_diagnostics)?;
        None
    };
    if let Some(code) = exit_code {
        host.exit(code);
    }
    Ok(collected)
}

/// Whether the package lowers at least one `@ kernel` function: the `auto`
/// selection routes a package that declares kernels through the device
/// session (admitting a GPU or failing closed with `E_DEVICE_NONE_ADMITTED`),
/// and a package without kernels runs on the CPU route unchanged.
///
/// # Errors
/// Structured diagnostics when the package does not lower.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub fn package_declares_kernels(config: &Config, input: &Path) -> Result<bool, Vec<Diagnostic>> {
    with_interpreted_lowered_package_mir(config, input, lowered_declares_kernels)
}

/// Whether a lowered unit contains a compute-shader function. The lowered
/// `shader_stage` is the one authority for "declares a kernel"; no attribute
/// scan answers it.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn lowered_declares_kernels(lowered: &radix::module::mir::LoweredMirUnit<'_>) -> bool {
    lowered
        .validated
        .program()
        .functions
        .iter()
        .any(|function| function.shader_stage == Some(radix_mir::MirKernelShaderStage::Compute))
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
