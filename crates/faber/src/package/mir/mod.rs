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

/// The CLI-surface-aware interpreted probe used by `check --lower`.
#[cfg(feature = "runner")]
pub(crate) fn with_check_lowered_package_mir<R>(
    config: &Config,
    input: &Path,
    run: impl for<'a> FnOnce(&radix::module::mir::LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    radix::program::mir::with_check_lowered_package_mir(config, input, &faber_mir_seams(), run)
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

/// How much of a device run's GPU time the runtime could measure.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceGpuTiming {
    /// Every launch carried a measured GPU duration.
    PerLaunch,
    /// Some launches carried a measured GPU duration and some did not.
    Partial,
    /// No launch carried a measured GPU duration.
    Unavailable,
}

#[cfg(all(feature = "runner", feature = "device-runtime"))]
impl DeviceGpuTiming {
    /// The spelling a stats artifact prints.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PerLaunch => "per-launch",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One device run's phase receipt: the wall time of each phase the run
/// passes (admission, preparation, image build, module compile, execution),
/// the session runtime counters, the host-side transfer traffic and the host
/// clock spent encoding launches.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRunReceipt {
    /// Time spent opening and admitting the device session, in nanoseconds.
    pub admit_ns: u64,
    /// Time spent analysing and lowering the package before the device work,
    /// in nanoseconds.
    pub prepare_ns: u64,
    /// Time spent building the per-kernel device image, in nanoseconds.
    pub image_build_ns: u64,
    /// Time spent compiling the image's modules onto the session, in
    /// nanoseconds.
    pub module_compile_ns: u64,
    /// Time spent running the entry under kernel dispatch, in nanoseconds.
    pub execution_ns: u64,
    /// The session's runtime counters.
    pub runtime: radix_device_core::runtime::RuntimeStats,
    /// Host-to-device buffer writes (first uploads and re-uploads).
    pub uploads: u64,
    /// Device-to-host range reads.
    pub downloads: u64,
    /// Bytes host-to-device across every upload.
    pub uploaded_bytes: u64,
    /// Bytes device-to-host across every read.
    pub downloaded_bytes: u64,
    /// Host time spent encoding launches, in nanoseconds.
    pub host_encode_ns: u64,
    /// How much of the launch time the runtime could measure.
    pub gpu_timing: DeviceGpuTiming,
}

/// The result of an in-process device run: the discovery receipt line that was
/// printed, the host-side meter when one was requested, and the run's phase
/// receipt (device time and traffic at each phase boundary).
#[cfg(all(feature = "runner", feature = "device-runtime"))]
#[derive(Debug)]
pub struct DeviceSessionRun {
    pub receipt: String,
    /// Host rows only. Kernel dispatches run on the device, so their time is
    /// reported by [`device`](Self::device) rather than as host runner rows
    /// (gpu-reset C2-U6c, D4).
    pub stats: Option<radix_mir_runner::RunnerStats>,
    /// The inner phase clocks used to report cold-start stages separately.
    pub phases: DeviceRunPhases,
    /// The per-phase device receipt: admission, preparation, image build,
    /// module compile and execution clocks, runtime counters and transfer
    /// traffic.
    pub device: DeviceRunReceipt,
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
    // WHY: taken here, before the preparation call, so the closure can read
    // the analysis/lowering time before any device work starts.
    let route_start = std::time::Instant::now();
    let (stats, phases) = radix::program::mir::with_interpreted_bound_package_mir(
        config,
        input,
        host,
        &faber_mir_seams(),
        |lowered, validated, exit_code, host| {
            let prepare_ns = route_start.elapsed().as_nanos() as u64;
            run_admitted_device_session(
                input,
                admitted,
                lowered,
                validated,
                exit_code,
                host,
                collect_stats,
                prepare_ns,
            )
        },
    )?;
    Ok(DeviceSessionRun {
        receipt,
        stats,
        device: phases.into_receipt(),
        phases,
    })
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
    // WHY: taken here, before the preparation call, so the closure can read
    // the analysis/lowering time before any device work starts.
    let route_start = std::time::Instant::now();
    let prepared = radix::program::mir::with_interpreted_bound_package_mir(
        config,
        input,
        host,
        &faber_mir_seams(),
        |lowered, validated, exit_code, host| {
            let prepare_ns = route_start.elapsed().as_nanos() as u64;
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
                        prepare_ns,
                    )
                    .map(|(stats, phases)| {
                        Some(DeviceSessionRun {
                            receipt,
                            stats,
                            device: phases.into_receipt(),
                            phases,
                        })
                    })
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
    /// Time spent opening and admitting the session, in nanoseconds.
    admit_ns: u64,
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
    use super::host_factory::{
        DEVICE_MEMORY_BUDGET_ENV, budget_request_from_override, discovery_receipt,
        host_error_diagnostic, open_device_selection,
    };
    use radix_device_core::backend::{DeviceBackend, DeviceClass};
    use radix_device_core::session::DeviceSession;
    use std::time::Instant;

    let admit_start = Instant::now();
    let request =
        budget_request_from_override(std::env::var(DEVICE_MEMORY_BUDGET_ENV).ok().as_deref())
            .map_err(|diagnostic| vec![diagnostic])?;
    let opened = open_device_selection(registry, selection, true, request)
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
        DeviceClass::Backend(DeviceBackend::Cuda) => DeviceImageBackend::Cuda,
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
    let admit_ns = admit_start.elapsed().as_nanos() as u64;
    receipt.print();
    Ok(AdmittedDeviceSession {
        session,
        backend,
        receipt: receipt.render(),
        admit_ns,
    })
}

/// The inner phases of one admitted device session, with the dispatcher's own
/// receipt (observed before the dispatcher `Rc` drops at return).
#[cfg(all(feature = "runner", feature = "device-runtime"))]
#[derive(Debug)]
pub struct DeviceRunPhases {
    admit_ns: u64,
    prepare_ns: u64,
    /// Host time spent building the per-kernel device image, in nanoseconds.
    pub image_build_ns: u64,
    /// Host time of the image build spent emitting the backend artifact, in
    /// nanoseconds.
    pub image_emission_ns: u64,
    /// Host time of the image build spent compiling the emitted NVVM module to
    /// PTX, in nanoseconds (0 for the Metal backend).
    pub image_ptx_compile_ns: u64,
    /// Host time spent compiling device modules, in nanoseconds.
    pub module_compile_ns: u64,
    /// Wall time spent executing the entry, in nanoseconds.
    pub execution_ns: u64,
    dispatcher: super::device::DispatcherReceipt,
}

#[cfg(all(feature = "runner", feature = "device-runtime"))]
impl DeviceRunPhases {
    /// The public phase receipt for this run.
    fn into_receipt(&self) -> DeviceRunReceipt {
        let dispatcher = self.dispatcher.clone();
        let runtime = dispatcher.runtime;
        let gpu_timing = if runtime.timed_launch_count == 0 {
            DeviceGpuTiming::Unavailable
        } else if runtime.untimed_launch_count == 0 {
            DeviceGpuTiming::PerLaunch
        } else {
            DeviceGpuTiming::Partial
        };
        DeviceRunReceipt {
            admit_ns: self.admit_ns,
            prepare_ns: self.prepare_ns,
            image_build_ns: self.image_build_ns,
            module_compile_ns: self.module_compile_ns,
            execution_ns: self.execution_ns,
            runtime,
            uploads: dispatcher.transfers.uploads,
            downloads: dispatcher.transfers.downloads,
            uploaded_bytes: dispatcher.transfers.uploaded_bytes,
            downloaded_bytes: dispatcher.transfers.downloaded_bytes,
            host_encode_ns: dispatcher.host_encode_ns,
            gpu_timing,
        }
    }
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
    prepare_ns: u64,
) -> Result<(Option<radix_mir_runner::RunnerStats>, DeviceRunPhases), Vec<Diagnostic>> {
    use super::device::{SessionKernelDispatcher, device_image_for_lowered};
    use super::host_factory::host_error_diagnostic;
    use std::time::Instant;

    let AdmittedDeviceSession {
        session,
        backend,
        admit_ns,
        ..
    } = admitted;
    let image_start = Instant::now();
    let (mut image, omitted, image_timing) = device_image_for_lowered(lowered, backend)?;
    let image_build_ns = image_start.elapsed().as_nanos() as u64;
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
        .ok_or_else(|| vec![mir_diag(input, radix_mir::names::NO_ENTRY_FUNCTION)])?;
    let extents = image.extents;
    let genus_layouts = image.genus_layouts;
    let fault_sites = image.fault_sites;
    let backward = std::mem::take(&mut image.backward);
    let units = image.kernels.into_iter().map(|(_, unit)| unit).collect();
    let dispatcher_start = Instant::now();
    let dispatcher = SessionKernelDispatcher::new(
        session,
        image.artifact,
        units,
        validated.validation().types.as_table(),
    )
    .map_err(|error| vec![host_error_diagnostic(&error)])?;
    let module_compile_ns = dispatcher_start.elapsed().as_nanos() as u64;
    dispatcher.borrow_mut().set_omissions(omissions);
    dispatcher
        .borrow_mut()
        .set_scalars_by_buffer(backend == super::device::DeviceImageBackend::Cuda);
    dispatcher.borrow_mut().set_extents(extents);
    dispatcher.borrow_mut().set_backward_functions(backward);
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
    // The dispatcher outcome carries the entry's evaluated return: the MIR
    // entry pass lowers an `exit <expr>` tail as an int-returning entry, so a
    // `numerus` return is the process status and the planned constant is only
    // the fallback for a vacuum entry (the same rule as the FMIR image and the
    // plain runner route). Reading the return on the unmetered arm too keeps a
    // dynamic exit from being silently dropped now that plan time admits it.
    let execution_start = Instant::now();
    let outcome = radix_mir_runner::run_function_with_kernel_dispatcher_and_stats(
        validated, entry, host, &hook,
    )
    .map_err(runner_diagnostics)?;
    let execution_ns = execution_start.elapsed().as_nanos() as u64;
    outcome.result.map_err(runner_diagnostics)?;
    // WHY: taken here, before the exit handling (`host.exit` panics in tests)
    // and while the dispatcher `Rc` is still alive; the receipt is dropped with
    // the dispatcher at return otherwise.
    let dispatcher_receipt = dispatcher.borrow().receipt();
    if let Some(radix_mir_runner::Value::Int(code)) = outcome.return_value {
        // WHY: a process exit status is `i32` while the Faber `numerus`
        // carrier is `i64`; truncate exactly as the trap path does.
        #[allow(clippy::cast_possible_truncation)]
        let exit_code = code as i32;
        host.exit(exit_code);
    } else if let Some(code) = exit_code {
        host.exit(code);
    }
    Ok((
        collect_stats.then_some(outcome.stats),
        DeviceRunPhases {
            admit_ns,
            prepare_ns,
            image_build_ns,
            image_emission_ns: image_timing.emission_ns,
            image_ptx_compile_ns: image_timing.ptx_compile_ns,
            module_compile_ns,
            execution_ns,
            dispatcher: dispatcher_receipt,
        },
    ))
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

/// Register the system device runtimes this machine provides: Metal on
/// macOS when a Metal device opens, and CUDA wherever the CUDA driver
/// library (`libcuda`, loaded at run time) is present. A machine without a
/// driver registers no CUDA runtime, so `--device cuda` fails closed with
/// `E_DEVICE_UNAVAILABLE`; with a driver but no usable device, admission fails
/// closed with `E_DEVICE_NONE_ADMITTED`. CUDA discovery (`cuInit`) runs only
/// when a device is opened, so a run without kernels does not pay for it.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub fn system_device_registry() -> radix_device_core::registry::DeviceRegistry {
    let mut registry = radix_device_core::registry::DeviceRegistry::new();
    if radix_device_cuda::driver_library_present() {
        registry.register(Box::new(radix_device_cuda::system_runtime()));
    }
    #[cfg(target_os = "macos")]
    if let Some(info) = radix_device_metal::system_devices().into_iter().next()
        && let Ok(driver) = radix_device_metal::SystemMetalDriver::open(info.registry_id)
    {
        registry.register(Box::new(radix_device_metal::MetalRuntime::new(driver)));
    }
    registry
}
