//! Faber package-MIR product surface (RP5 residual).
//!
//! RP5: the compiler-owned subset of this module moved to
//! `radix-program/src/mir/` (lowering/linking/image/routes/driver/cli/diag/
//! manifest writer + test support). This module keeps the faber product
//! surface:
//!
//! - [`faber_mir_seams`] — the faber-supplied seam bundle (executed-lane
//!   target, library resolver/cache, `[device]` projection, device-section
//!   builder, host-route policy, runner writer, toolchain version) that the
//!   compiler subset receives.
//! - Faber product entry adapters that inject those seams into the typed
//!   `radix-program` MIR operations. These adapters are product boundaries,
//!   not compiler compatibility modules.
#![allow(dead_code)] // Binary and library targets exercise different package runner surfaces.

#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub mod cpu_proba;
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
use std::path::Path;

#[cfg(feature = "device-runtime")]
use radix::program::mir::DeviceSelection;
#[cfg(feature = "runner")]
use radix::program::mir::mir_diag;

#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn elapsed_nanoseconds(duration: std::time::Duration) -> Option<u64> {
    u64::try_from(duration.as_nanos()).ok()
}

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
    // The Metal artifact is always part of the section, so a kernel Metal's
    // requirement row refuses stops the build by name before any emission.
    let refused = super::device::metal_refused_kernels(lowered);
    if let Some((_, reason)) = refused
        .iter()
        .find(|(id, _)| program.kernels.iter().any(|kernel| kernel.function == *id))
    {
        return Err(vec![mir_issue_diag(
            diagnostic_path,
            super::device::TARGET_REQUIREMENTS_ISSUE,
            reason.clone(),
        )]);
    }
    let device_inputs = resolve_device_inputs(prepared, lowered, &program, diagnostic_path)?;
    let selection = prepared.device_selection.unwrap_or(DeviceSelection::Auto);
    let cuda_libdevice =
        super::resolve_cuda_libdevice(diagnostic_path).map_err(|diagnostic| vec![*diagnostic])?;
    let section = super::device::device_section_for_program(
        &program,
        &semantics,
        &lowered.validated,
        &lowered.interner,
        super::device::DeviceSectionBuild {
            selection,
            inputs: &device_inputs,
            ptx_target: super::device::ptx_target_for(None),
            cuda_libdevice: cuda_libdevice.as_deref(),
            repeating_steps: step_count,
            device_function_metadata: &lowered.device_function_metadata,
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
/// Returns the discovery receipt line reported to stderr.
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
    /// The session window: host interpretation of the entry plus device
    /// execution, in nanoseconds. Not GPU time; the dispatcher's
    /// [`RuntimeStats::gpu_busy_ns`] is the device-only figure.
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
    /// Host time spent encoding launches, in nanoseconds, excluding
    /// `host_blocked_ns`.
    pub host_encode_ns: u64,
    /// Host time blocked in commit-and-wait calls inside the launch-encode
    /// window, in nanoseconds; GPU wait, not host work. A subset of
    /// `runtime.blocking_wait_ns`.
    pub host_blocked_ns: u64,
    /// Cumulative host bytes fresh kernel-output twins materialized during
    /// the run: a launch allocates no host array for its output, and the
    /// first host read that needs real bytes materializes the twin once.
    pub host_fresh_output_bytes: u64,
    /// How much of the launch time the runtime could measure.
    pub gpu_timing: DeviceGpuTiming,
    /// The avoidable cross-partition syncs the spanned run's calls implied
    /// (the planner audit's classification; the counters are in
    /// `runtime.span`). Empty on an unspanned device.
    pub span_avoidable: Vec<SpanAvoidable>,
}

/// One planner-audit classification: the entry's syncs of `kind` are
/// avoidable for this reason. A dispatcher fact; the spanning runtime only
/// counts the syncs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanAvoidable {
    /// The launched entry.
    pub entry: String,
    /// The sync kind the class describes (`own-write-gather`,
    /// `partials-all-gather`, `first-broadcast`).
    pub kind: &'static str,
    /// The class: `unbounded-read`, `fusable-with-previous` or
    /// `first-fallback`.
    pub avoidable: &'static str,
    /// The class's data: the missing read set, the entry a reduction would
    /// fuse into, or the fallback reason.
    pub detail: Option<String>,
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

/// A failed in-process device run: the diagnostics, and, when the program had
/// started executing, what the run measured up to the failure (the discovery
/// receipt, the host meter when requested, the phase clocks and the device
/// counters). `run` is `None` for a failure before launch (admission,
/// preparation, image build), where nothing ran to measure.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
#[derive(Debug)]
pub struct DeviceSessionFailure {
    pub diagnostics: Vec<Diagnostic>,
    pub run: Option<DeviceSessionRun>,
}

#[cfg(all(feature = "runner", feature = "device-runtime"))]
impl From<Vec<Diagnostic>> for DeviceSessionFailure {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            diagnostics,
            run: None,
        }
    }
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
    run_package_on_device_session_shaped(
        config,
        input,
        selection,
        registry,
        host,
        collect_stats,
        CpuDeviceShape::default(),
    )
    .map_err(|failure| failure.diagnostics)
}

/// [`run_package_on_device_session_with_stats`] over a device shape:
/// `--partitions N` on an explicit Metal selection spans the GPU (rule 18).
/// A failure also carries what a run that had started executing measured
/// before it failed ([`DeviceSessionFailure::run`]).
///
/// # Errors
/// As [`run_package_on_device_session_with_stats`].
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub fn run_package_on_device_session_shaped<H: radix_mir_runner::Host + ?Sized>(
    config: &Config,
    input: &Path,
    selection: DeviceSelection,
    registry: &mut radix_device_core::registry::DeviceRegistry,
    host: &mut H,
    collect_stats: bool,
    shape: CpuDeviceShape,
) -> Result<DeviceSessionRun, DeviceSessionFailure> {
    // Admission first: an unavailable explicit device fails before lowering
    // work is spent on it.
    let admitted = admit_device_session(input, selection, registry, shape)?;
    let receipt = admitted.receipt.clone();
    // WHY: taken here, before the preparation call, so the closure can read
    // the analysis/lowering time before any device work starts.
    let route_start = std::time::Instant::now();
    let outcome = radix::program::mir::with_interpreted_bound_package_mir(
        config,
        input,
        host,
        &faber_mir_seams(),
        |lowered, validated, exit_code, host| {
            let prepare_ns = elapsed_nanoseconds(route_start.elapsed())
                .ok_or_else(|| vec![mir_diag(input, "elapsed duration exceeds u64 nanoseconds")])?;
            Ok(run_admitted_device_session(
                input,
                admitted,
                lowered,
                validated,
                exit_code,
                host,
                collect_stats,
                prepare_ns,
            ))
        },
    )?;
    outcome
        .map(|(stats, phases)| DeviceSessionRun::new(receipt.clone(), stats, phases))
        .map_err(|failure| failure.into_session_failure(receipt))
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
/// A failure of the device run itself is returned as is, carrying what the run
/// measured before it failed ([`DeviceSessionFailure::run`]); the package is
/// never run again.
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
) -> Result<Option<DeviceSessionRun>, DeviceSessionFailure> {
    // WHY: taken here, before the preparation call, so the closure can read
    // the analysis/lowering time before any device work starts.
    let route_start = std::time::Instant::now();
    let prepared = radix::program::mir::with_interpreted_bound_package_mir(
        config,
        input,
        host,
        &faber_mir_seams(),
        |lowered, validated, exit_code, host| {
            let prepare_ns = elapsed_nanoseconds(route_start.elapsed())
                .ok_or_else(|| vec![mir_diag(input, "elapsed duration exceeds u64 nanoseconds")])?;
            if !lowered_declares_kernels(lowered) {
                return Ok(Ok(None));
            }
            Ok(run_declared_kernels_on_device_session(
                input,
                DeviceSelection::Auto,
                registry,
                CpuDeviceShape::default(),
                lowered,
                validated,
                exit_code,
                host,
                collect_stats,
                prepare_ns,
            ))
        },
    );
    match prepared {
        Ok(outcome) => outcome,
        Err(_) if package_declares_kernels(config, input).unwrap_or(false) => {
            run_package_on_device_session_shaped(
                config,
                input,
                DeviceSelection::Auto,
                registry,
                host,
                collect_stats,
                CpuDeviceShape::default(),
            )
            .map(Some)
        }
        Err(_) => Ok(None),
    }
}

/// Admit `selection` over `registry` and run the lowered unit (which declares
/// kernels) on it: the shared body of the `auto` and `cpu` device routes.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
#[allow(clippy::too_many_arguments)]
fn run_declared_kernels_on_device_session<H: radix_mir_runner::Host + ?Sized>(
    input: &Path,
    selection: DeviceSelection,
    registry: &mut radix_device_core::registry::DeviceRegistry,
    shape: CpuDeviceShape,
    lowered: &radix::module::mir::LoweredMirUnit<'_>,
    validated: &radix::module::mir::ValidatedMir<'_>,
    exit_code: Option<i32>,
    host: &mut H,
    collect_stats: bool,
    prepare_ns: u64,
) -> Result<Option<DeviceSessionRun>, DeviceSessionFailure> {
    let admitted = admit_device_session(input, selection, registry, shape)?;
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
    .map(|(stats, phases)| Some(DeviceSessionRun::new(receipt.clone(), stats, phases)))
    .map_err(|failure| failure.into_session_failure(receipt))
}

/// An admitted, opened device session: the discovery receipt has been
/// reported to stderr, and the session waits for the lowered unit to dispatch.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub(crate) struct AdmittedDeviceSession {
    session: radix_device_core::session::DeviceSession,
    backend: super::device::DeviceImageBackend,
    pub(crate) receipt: String,
    /// Time spent opening and admitting the session, in nanoseconds.
    admit_ns: u64,
    /// Whether the run captures its kernel calls (the C3 comparator).
    pub(crate) trace_kernels: bool,
}

/// Open the selected device, report the discovery receipt to stderr, and pick the
/// per-kernel image backend.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub(crate) fn admit_device_session(
    input: &Path,
    selection: DeviceSelection,
    registry: &mut radix_device_core::registry::DeviceRegistry,
    cpu: CpuDeviceShape,
) -> Result<AdmittedDeviceSession, Vec<Diagnostic>> {
    let admitted = open_device_session(input, selection, registry, cpu)?;
    eprintln!("{}", admitted.receipt);
    Ok(admitted)
}

/// [`admit_device_session`] without reporting the discovery receipt: a proba
/// route opens one session per case and keeps its report clean.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn open_device_session(
    input: &Path,
    selection: DeviceSelection,
    registry: &mut radix_device_core::registry::DeviceRegistry,
    cpu: CpuDeviceShape,
) -> Result<AdmittedDeviceSession, Vec<Diagnostic>> {
    use super::device::DeviceImageBackend;
    use super::host_factory::{discovery_receipt, host_error_diagnostic, open_device_selection};
    use radix_device_core::backend::{DeviceBackend, DeviceClass};
    use radix_device_core::policy::BudgetRequest;
    use radix_device_core::session::DeviceSession;
    use std::time::Instant;

    let admit_start = Instant::now();
    let opened = open_device_selection(registry, selection, true, BudgetRequest::Derived, cpu)
        .map_err(|diagnostic| vec![diagnostic])?
        .ok_or_else(|| {
            vec![mir_diag(
                input,
                "no device was admitted for the device route",
            )]
        })?;
    let receipt = discovery_receipt(&opened);
    let backend = match receipt.class {
        // A Metal device spanning several partitions takes the lane-range
        // image (rule 18).
        DeviceClass::Backend(DeviceBackend::Metal) if receipt.partitions > 1 => {
            DeviceImageBackend::MetalSpanned
        }
        DeviceClass::Backend(DeviceBackend::Metal) => DeviceImageBackend::Metal,
        // A CUDA device spanning several partitions takes the lane-range
        // image too (rule 18).
        DeviceClass::Backend(DeviceBackend::Cuda) if receipt.partitions > 1 => {
            DeviceImageBackend::CudaSpanned
        }
        DeviceClass::Backend(DeviceBackend::Cuda) => DeviceImageBackend::Cuda,
        DeviceClass::Cpu => DeviceImageBackend::Cpu,
    };
    let session =
        DeviceSession::from_opened(opened).map_err(|error| vec![host_error_diagnostic(&error)])?;
    let admit_ns = elapsed_nanoseconds(admit_start.elapsed())
        .ok_or_else(|| vec![mir_diag(input, "elapsed duration exceeds u64 nanoseconds")])?;
    Ok(AdmittedDeviceSession {
        session,
        backend,
        receipt: receipt.render(),
        admit_ns,
        trace_kernels: false,
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
    /// The kernel calls the run captured, when it was a traced run.
    pub kernel_trace: Option<radix_mir_runner::kernel_trace::KernelTrace>,
}

#[cfg(all(feature = "runner", feature = "device-runtime"))]
impl DeviceRunPhases {
    /// The public phase receipt for this run.
    fn into_receipt(&self) -> DeviceRunReceipt {
        let runtime = self.dispatcher.runtime.clone();
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
            uploads: self.dispatcher.transfers.uploads,
            downloads: self.dispatcher.transfers.downloads,
            uploaded_bytes: self.dispatcher.transfers.uploaded_bytes,
            downloaded_bytes: self.dispatcher.transfers.downloaded_bytes,
            host_encode_ns: self.dispatcher.host_encode_ns,
            host_blocked_ns: self.dispatcher.host_blocked_ns,
            host_fresh_output_bytes: self.dispatcher.host_fresh_output_bytes,
            gpu_timing,
            span_avoidable: self
                .dispatcher
                .span_avoidable
                .iter()
                .map(|(entry, class)| SpanAvoidable {
                    entry: entry.clone(),
                    kind: class.kind().as_str(),
                    avoidable: class.name(),
                    detail: class.detail(),
                })
                .collect(),
        }
    }
}

/// A session kernel dispatcher built over one lowered unit, with the time its
/// image build and module compile took.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
struct SessionDispatcher {
    dispatcher: std::rc::Rc<std::cell::RefCell<super::device::SessionKernelDispatcher>>,
    image_build_ns: u64,
    image_timing: super::device::DeviceImageTiming,
    module_compile_ns: u64,
}

/// Build the device image from the lowered unit and the kernel dispatcher
/// that launches it through the admitted session. A unit with no kernel that
/// lowers to the device fails closed (`E_DEVICE_NONE_ADMITTED`).
#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn session_dispatcher(
    input: &Path,
    admitted: AdmittedDeviceSession,
    lowered: &radix::module::mir::LoweredMirUnit<'_>,
    validated: &radix::module::mir::ValidatedMir<'_>,
) -> Result<SessionDispatcher, Vec<Diagnostic>> {
    use super::device::{SessionKernelDispatcher, device_image_for_lowered_with};
    use super::host_factory::host_error_diagnostic;
    use std::time::Instant;

    /// The row layout of every record (fixed-layout genus) element type the
    /// kernel units bind as a tensor resource.
    fn record_layouts(
        units: &[radix_mir::KernelUnit],
        validation: &radix_mir::MirValidationContext<'_>,
    ) -> std::collections::HashMap<
        radix_types::TypeId,
        std::sync::Arc<radix_mir_runner::dense::RecordLayout>,
    > {
        let types = validation.types.as_table();
        let mut records = std::collections::HashMap::new();
        for resource in units.iter().flat_map(|unit| &unit.resources) {
            let ty = resource.version.element_ty.semantic_id();
            if matches!(types.get(ty), radix_types::Type::Struct(_))
                && !records.contains_key(&ty)
                && let Ok(layout) = radix_mir_runner::dense::RecordLayout::resolve(ty, validation)
            {
                records.insert(ty, std::sync::Arc::new(layout));
            }
        }
        records
    }

    let AdmittedDeviceSession {
        session, backend, ..
    } = admitted;
    let image_start = Instant::now();
    let cuda = matches!(
        backend,
        super::device::DeviceImageBackend::Cuda | super::device::DeviceImageBackend::CudaSpanned
    );
    let cuda_libdevice = if cuda {
        super::resolve_cuda_libdevice(input).map_err(|diagnostic| vec![*diagnostic])?
    } else {
        None
    };
    let (mut image, omitted, image_timing) =
        device_image_for_lowered_with(lowered, backend, cuda_libdevice.as_deref(), true)?;
    let image_build_ns = elapsed_nanoseconds(image_start.elapsed())
        .ok_or_else(|| vec![mir_diag(input, "elapsed duration exceeds u64 nanoseconds")])?;
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
    // Owned slabs are CUDA spanning only. The plan lays out only the roles the
    // spanning runtime can launch, so it must read the image before the
    // dispatcher takes its parts.
    let placement = matches!(backend, super::device::DeviceImageBackend::CudaSpanned).then(|| {
        super::device::placement_plan(lowered, &image, super::device::SlabRole::launchable)
    });
    let interner = validated
        .validation()
        .interner
        .as_interner()
        .ok_or_else(|| vec![mir_diag(input, "device run requires interner context")])?;
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
    // The CPU lane image must resolve before the session compiles its token.
    if let Some(cpu) = &image.cpu {
        super::device::CpuLaneEngine::shared().install(cpu);
    }
    let extents = image.extents;
    let empty_reductions = image.empty_reductions;
    let genus_layouts = image.genus_layouts;
    let fault_sites = image.fault_sites;
    let backward = std::mem::take(&mut image.backward);
    let param_buffers = std::mem::take(&mut image.param_buffers);
    let result_buffers = std::mem::take(&mut image.result_buffers);
    let refusals: Vec<(radix_mir::MirFunctionId, String)> = std::mem::take(&mut image.refused)
        .into_iter()
        .map(|(id, reason)| {
            let name = validated
                .program()
                .functions
                .iter()
                .find(|function| function.id == id)
                .and_then(|function| function.name)
                .map_or_else(|| id.0.to_string(), |s| interner.resolve(s).to_owned());
            (
                id,
                format!("kernel `{name}` cannot run as a unit sequence: {reason}"),
            )
        })
        .collect();
    let lane_ranges = std::mem::take(&mut image.lane_ranges);
    let slab_origins = std::mem::take(&mut image.slab_origins);
    let write_maps = std::mem::take(&mut image.write_maps);
    let fusions = std::mem::take(&mut image.fusions);
    let units: Vec<radix_mir::KernelUnit> =
        image.kernels.into_iter().map(|(_, unit)| unit).collect();
    let dispatcher_start = Instant::now();
    let fused_entries = fusions.iter().map(|fused| fused.entry.clone()).collect();
    // gpu-reset Q02: a tensor of fixed-layout records binds as typed row
    // bytes; each record element type the kernels name gets its row layout
    // from the layout law (a type with no layout stays unsupported by name).
    let records = record_layouts(&units, validated.validation());
    let dispatcher = SessionKernelDispatcher::with_fused_entries_and_records(
        session,
        image.artifact,
        units,
        fused_entries,
        validated.validation().types.as_table(),
        records,
    )
    .map_err(|error| vec![host_error_diagnostic(&error)])?;
    let module_compile_ns = elapsed_nanoseconds(dispatcher_start.elapsed())
        .ok_or_else(|| vec![mir_diag(input, "elapsed duration exceeds u64 nanoseconds")])?;
    dispatcher.borrow_mut().set_omissions(omissions);
    dispatcher.borrow_mut().set_refusals(refusals);
    dispatcher.borrow_mut().set_result_buffers(
        result_buffers
            .into_iter()
            .map(|(function, row)| (function, row.into_iter().map(|id| id.0).collect()))
            .collect(),
    );
    dispatcher.borrow_mut().set_param_buffers(
        param_buffers
            .into_iter()
            .map(|(function, row)| {
                (
                    function,
                    row.into_iter()
                        .map(|buffer| buffer.map(|id| id.0))
                        .collect(),
                )
            })
            .collect(),
    );
    dispatcher.borrow_mut().set_scalars_by_buffer(cuda);
    dispatcher.borrow_mut().set_extents(extents);
    dispatcher
        .borrow_mut()
        .set_empty_reductions(empty_reductions);
    dispatcher.borrow_mut().set_write_maps(write_maps);
    dispatcher.borrow_mut().set_reduce_fusions(fusions);
    if matches!(
        backend,
        super::device::DeviceImageBackend::MetalSpanned
            | super::device::DeviceImageBackend::CudaSpanned
    ) {
        dispatcher.borrow_mut().set_span(lane_ranges, slab_origins);
    }
    if let Some(plan) = placement {
        dispatcher.borrow_mut().set_placement(plan);
    }
    dispatcher.borrow_mut().set_backward_functions(backward);
    super::device::install_fault_sites(&fault_sites, |entry, sites| {
        dispatcher.borrow_mut().set_fault_sites(entry, sites);
    });
    dispatcher
        .borrow_mut()
        .set_genus_layouts(genus_layouts, interner.clone());
    Ok(SessionDispatcher {
        dispatcher,
        image_build_ns,
        image_timing,
        module_compile_ns,
    })
}

/// Build the device image from the lowered unit and run the argument-bound
/// entry on the MIR runner under kernel dispatch through the admitted session.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub(crate) fn run_admitted_device_session<H: radix_mir_runner::Host + ?Sized>(
    input: &Path,
    admitted: AdmittedDeviceSession,
    lowered: &radix::module::mir::LoweredMirUnit<'_>,
    validated: &radix::module::mir::ValidatedMir<'_>,
    exit_code: Option<i32>,
    host: &mut H,
    collect_stats: bool,
    prepare_ns: u64,
) -> Result<(Option<radix_mir_runner::RunnerStats>, DeviceRunPhases), DeviceRunFailure> {
    use std::time::Instant;

    let admit_ns = admitted.admit_ns;
    let trace_kernels = admitted.trace_kernels;
    let SessionDispatcher {
        dispatcher,
        image_build_ns,
        image_timing,
        module_compile_ns,
    } = session_dispatcher(input, admitted, lowered, validated)?;
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
    let entry = names
        .explicit_entry_function()
        .map(|function| function.id)
        .ok_or_else(|| vec![mir_diag(input, radix_mir::names::NO_ENTRY_FUNCTION)])?;
    let hook: radix_mir_runner::kernel_dispatch::ResidencyHook = dispatcher.clone();
    let runner_diagnostics = |errors: Vec<radix_mir_runner::RunnerError>| {
        errors
            .into_iter()
            .map(|error| {
                let exit_status = error.exit_status().to_string();
                // A device error keeps its stable `E_DEVICE_*` code as the
                // diagnostic issue, so the terminal prints `error: <code>: ..`.
                let issue = if error.issue.starts_with("E_DEVICE_") {
                    error.issue.clone()
                } else {
                    "device_run_failed".to_owned()
                };
                mir_diag(input, error.message)
                    .with_arg("issue", issue)
                    .with_arg("code", error.issue)
                    .with_arg("exit_status", exit_status)
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
    let outcome = if trace_kernels {
        radix_mir_runner::run_function_with_kernel_trace(
            validated,
            entry,
            host,
            radix_mir_runner::RunnerKernelPolicy::Dispatch,
            Some(&hook),
        )
    } else {
        radix_mir_runner::run_function_with_kernel_dispatcher_and_stats(
            validated, entry, host, &hook,
        )
    }
    .map_err(runner_diagnostics)?;
    let execution_ns = elapsed_nanoseconds(execution_start.elapsed())
        .ok_or_else(|| vec![mir_diag(input, "elapsed duration exceeds u64 nanoseconds")])?;
    // WHY: taken here, before the exit handling (`host.exit` panics in tests)
    // and while the dispatcher `Rc` is still alive; the receipt is dropped with
    // the dispatcher at return otherwise. A failed run keeps it too: the
    // counters up to the fault are the failure's receipt.
    let dispatcher_receipt = dispatcher.borrow().receipt();
    let phases = DeviceRunPhases {
        admit_ns,
        prepare_ns,
        image_build_ns,
        image_emission_ns: image_timing.emission_ns,
        image_ptx_compile_ns: image_timing.ptx_compile_ns,
        module_compile_ns,
        execution_ns,
        dispatcher: dispatcher_receipt,
        kernel_trace: outcome.kernel_trace.clone(),
    };
    if let Err(errors) = outcome.result {
        return Err(DeviceRunFailure {
            diagnostics: runner_diagnostics(errors),
            partial: Some((collect_stats.then_some(outcome.stats), phases)),
        });
    }
    // A traced run is a measurement: the entry's exit status is not applied.
    if trace_kernels {
        return Ok((None, phases));
    }
    if let Some(radix_mir_runner::Value::Int(code)) = outcome.return_value {
        // WHY: a process exit status is `i32` while the Faber `numerus`
        // carrier is `i64`; truncate exactly as the trap path does.
        #[allow(clippy::cast_possible_truncation)]
        let exit_code = code as i32;
        host.exit(exit_code);
    } else if let Some(code) = exit_code {
        host.exit(code);
    }
    Ok((collect_stats.then_some(outcome.stats), phases))
}

/// A failed [`run_admitted_device_session`]: the diagnostics, and the host
/// meter and phase clocks of a run that failed after it started executing.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub(crate) struct DeviceRunFailure {
    pub(crate) diagnostics: Vec<Diagnostic>,
    partial: Option<(Option<radix_mir_runner::RunnerStats>, DeviceRunPhases)>,
}

#[cfg(all(feature = "runner", feature = "device-runtime"))]
impl From<Vec<Diagnostic>> for DeviceRunFailure {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            diagnostics,
            partial: None,
        }
    }
}

#[cfg(all(feature = "runner", feature = "device-runtime"))]
impl DeviceRunFailure {
    fn into_session_failure(self, receipt: String) -> DeviceSessionFailure {
        DeviceSessionFailure {
            diagnostics: self.diagnostics,
            run: self
                .partial
                .map(|(stats, phases)| DeviceSessionRun::new(receipt, stats, phases)),
        }
    }
}

#[cfg(all(feature = "runner", feature = "device-runtime"))]
impl DeviceSessionRun {
    fn new(
        receipt: String,
        stats: Option<radix_mir_runner::RunnerStats>,
        phases: DeviceRunPhases,
    ) -> Self {
        Self {
            receipt,
            stats,
            device: phases.into_receipt(),
            phases,
        }
    }
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

/// The registry of the `--device cpu` route: one CPU runtime over the
/// process-wide lane engine ([`super::device::CpuLaneEngine`]). The worker
/// budget is shared by all logical partitions. `auto` never uses this registry.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub fn cpu_device_registry(
    workers: Option<std::num::NonZeroU32>,
) -> radix_device_core::registry::DeviceRegistry {
    let workers = workers
        .or_else(default_cpu_workers)
        .map_or(std::num::NonZeroUsize::MIN, |workers| {
            std::num::NonZeroUsize::try_from(workers).unwrap_or(std::num::NonZeroUsize::MAX)
        });
    let mut registry = radix_device_core::registry::DeviceRegistry::new();
    registry.register(Box::new(radix_device_cpu::CpuRuntime::new(
        super::device::CpuLaneEngine::shared(),
        workers,
    )));
    registry
}

/// Run `input` on the CPU device (gpu-reset rule 16.9): several worker
/// threads behind one logical device, through the same device session as
/// Metal and CUDA. `cpu.workers` is `--threads N`; absent, half the available
/// cores, at least one. `cpu.partitions` is `--partitions N`: the logical
/// device spans that many logical partitions sharing the worker pool (rule 18).
/// `Ok(None)` means the package declares no kernel: nothing ran,
/// and the reference route runs it, exactly as
/// [`run_package_on_device_session_if_kernels`] answers for `auto`.
///
/// # Errors
/// As [`run_package_on_device_session_with_stats`].
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub fn run_package_on_cpu_device_session<H: radix_mir_runner::Host + ?Sized>(
    config: &Config,
    input: &Path,
    cpu: CpuDeviceShape,
    host: &mut H,
    collect_stats: bool,
) -> Result<Option<DeviceSessionRun>, DeviceSessionFailure> {
    let cpu = CpuDeviceShape {
        workers: cpu.workers.or_else(default_cpu_workers),
        ..cpu
    };
    let mut registry = cpu_device_registry(cpu.workers);
    // WHY: taken here, before the preparation call, so the closure can read
    // the analysis/lowering time before any device work starts.
    let route_start = std::time::Instant::now();
    let prepared = radix::program::mir::with_interpreted_bound_package_mir(
        config,
        input,
        host,
        &faber_mir_seams(),
        |lowered, validated, exit_code, host| {
            let prepare_ns = elapsed_nanoseconds(route_start.elapsed())
                .ok_or_else(|| vec![mir_diag(input, "elapsed duration exceeds u64 nanoseconds")])?;
            if !lowered_declares_kernels(lowered) {
                return Ok(Ok(None));
            }
            Ok(run_declared_kernels_on_device_session(
                input,
                DeviceSelection::Cpu,
                &mut registry,
                cpu,
                lowered,
                validated,
                exit_code,
                host,
                collect_stats,
                prepare_ns,
            ))
        },
    );
    match prepared {
        Ok(outcome) => outcome,
        // A failed preparation of a package without kernels is reported by
        // the reference route, as for `auto`.
        Err(diagnostics) if package_declares_kernels(config, input).unwrap_or(false) => {
            Err(diagnostics.into())
        }
        Err(_) => Ok(None),
    }
}

/// The shape of the `--device cpu` logical device: the total worker count
/// (`--threads N`; `None` takes the route default) and the number of virtual
/// partitions sharing the workers (`--partitions N`, default one).
/// Other devices take the default shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuDeviceShape {
    /// Total CPU workers across every partition.
    pub workers: Option<std::num::NonZeroU32>,
    /// Virtual partitions of the one logical CPU device.
    pub partitions: std::num::NonZeroU32,
}

impl Default for CpuDeviceShape {
    fn default() -> Self {
        Self {
            workers: None,
            partitions: std::num::NonZeroU32::MIN,
        }
    }
}

/// The CPU device's default worker count: half the available cores, at
/// least one.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn default_cpu_workers() -> Option<std::num::NonZeroU32> {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    std::num::NonZeroU32::new(u32::try_from(cores / 2).unwrap_or(u32::MAX).max(1))
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
    system_device_registry_spanning(MetalSpan::Unspanned, 1)
}

/// How logical Metal partitions share the physical device's memory.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetalSpan {
    /// One ordinary Metal runtime without a spanning wrapper.
    Unspanned,
    /// One allocation owner and queue shared by all logical partitions.
    Shared,
    /// Separate stores and queues for transport tests on one physical GPU.
    PrivateCopies,
}

/// Register CUDA for `cuda_partitions` partitions: one is the plain runtime,
/// more span that many physical devices (the lowest healthy ordinals, each
/// with its own memory; fewer devices than partitions fails closed at open).
/// Register Metal with the requested partition-memory model. Shared mode
/// uses one allocation owner and queue; Replicate and First execute once.
/// Private copies keep separate stores and queues for transport tests.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
pub fn system_device_registry_spanning(
    metal_span: MetalSpan,
    cuda_partitions: u32,
) -> radix_device_core::registry::DeviceRegistry {
    let mut registry = radix_device_core::registry::DeviceRegistry::new();
    if radix_device_cuda::driver_library_present() {
        registry.register(radix_device_cuda::system_runtime_for_partitions(
            cuda_partitions,
        ));
    }
    #[cfg(target_os = "macos")]
    if let Some(info) = radix_device_metal::system_devices().into_iter().next()
        && let Ok(driver) = radix_device_metal::SystemMetalDriver::open(info.registry_id)
    {
        if metal_span == MetalSpan::PrivateCopies {
            let registry_id = info.registry_id;
            let mut first = Some(driver);
            let factory: radix_device_core::span::ChildFactory = Box::new(move || {
                let driver = match first.take() {
                    Some(driver) => driver,
                    None => radix_device_metal::SystemMetalDriver::open(registry_id).map_err(
                        |error| {
                            radix_device_core::HostError::invalid_args(format!(
                                "a Metal partition driver did not open: {error:?}"
                            ))
                        },
                    )?,
                };
                Ok(Box::new(radix_device_metal::MetalRuntime::new(driver))
                    as Box<dyn radix_device_core::runtime::DeviceRuntime>)
            });
            if let Ok(spanning) = radix_device_core::span::SpanningRuntime::new(factory) {
                registry.register(Box::new(spanning));
            }
        } else if metal_span == MetalSpan::Shared {
            registry.register(Box::new(radix_device_core::span::SpanningRuntime::shared(
                Box::new(radix_device_metal::MetalRuntime::new(driver)),
            )));
        } else {
            registry.register(Box::new(radix_device_metal::MetalRuntime::new(driver)));
        }
    }
    registry
}
