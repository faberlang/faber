// Sibling + root items: explicit `use super` lists carry the seams the mir/
// split routes through `use super::*` (wildcard imports are denied).
// `host_factory` items are reached through `super::super::host_factory::…`
// paths (sibling of the package `device` module), so no import is needed.
use super::{
    admit_device_program_section, artifact_for_backend, descriptor_for_backend,
    inputs_by_buffer_id, wire_buffer_name, BTreeMap, DeviceBackend, DeviceDescriptor,
    DeviceSelection, Diagnostic, FmirDeviceSection, HostDeviceProgramLifetime, ProgramSession,
    WireBufferLifetime, WireBufferRole, WireDeviceProgram, WireObservationCadence,
    WireProgramLifetime, DEFAULT_TRAINING_STEPS,
};

/// The ordered step-run result of a program session (S5-U5): the per-step
/// observed values (the loss trace) and the convergence verdict.
pub(crate) struct StepRunReport {
    /// How many ordered launches / training steps executed.
    pub(crate) step_count: u32,
    /// The per-execution observed readbacks, in execution order (the loss
    /// trace for a `RepeatingStep` run).
    pub(crate) loss_trace: Vec<BTreeMap<u32, Vec<f32>>>,
    /// The first observed value of the first execution (initial loss).
    pub(crate) initial_loss: Option<f32>,
    /// The first observed value of the last execution (final loss).
    pub(crate) final_loss: Option<f32>,
    /// Whether the training run converged: `final_loss < 0.1 * initial_loss`
    /// (the Stage 5 gate) — or, when the initial loss is not positive, the
    /// final loss strictly decreased.
    pub(crate) converged: bool,
}

/// The per-step loss observation value of one execution's readbacks: the
/// DECLARED per-step observation buffer (the wire's `PerStep` result row —
/// the loss). Never `outputs.iter().next()` (deterministic BTreeMap order
/// by buffer id would pick the first forward tensor of a multi-observation
/// receipt — the U8 G1 divergence) and never a shape/name selection: the
/// declared cadence is the sole authority for which readback is the loss.
fn loss_observed(outputs: &BTreeMap<u32, Vec<f32>>, per_step_observation: u32) -> Option<f32> {
    outputs
        .get(&per_step_observation)
        .and_then(|values| values.first())
        .copied()
}

/// Reduce an ordered execution receipt list to the step-run report (S5-U5):
/// the loss trace (every observed readback per execution), the initial/final
/// loss of the DECLARED per-step observation buffer, and the convergence
/// verdict. Pure — the route prints it; the tests assert on it.
#[must_use]
pub(crate) fn step_run_report(
    receipts: &[faber_host_macos_arm64::composite_host::DeviceExecutionReceipt],
    per_step_observation: u32,
) -> StepRunReport {
    let loss_trace: Vec<BTreeMap<u32, Vec<f32>>> = receipts
        .iter()
        .map(|receipt| receipt.outputs.clone())
        .collect();
    let initial_loss = loss_trace
        .first()
        .and_then(|observed| loss_observed(observed, per_step_observation));
    let final_loss = loss_trace
        .last()
        .and_then(|observed| loss_observed(observed, per_step_observation));
    let converged = match (initial_loss, final_loss) {
        (Some(initial), Some(last)) if initial > 0.0 => last < 0.1 * initial,
        (Some(initial), Some(last)) => last < initial,
        _ => false,
    };
    StepRunReport {
        step_count: u32::try_from(receipts.len()).unwrap_or(u32::MAX),
        loss_trace,
        initial_loss,
        final_loss,
        converged,
    }
}

/// Execute a program session under its declared lifetime (S5-U5): a
/// `RepeatingStep` session once-inits its HostProvided params at session
/// creation (`init_params` — copied in exactly once, never re-copied on
/// later steps) and executes `steps` training steps, reading back the
/// declared observations per step (the loss trace). A `SingleRun` session
/// executes its ordered launches (the S2-8 repeat surface). Returns every
/// execution receipt in order.
///
/// # Errors
/// Fail-closed host diagnostics (a `RepeatingStep` session refuses
/// `execute`; a `SingleRun` session refuses `init_params`/`execute_step`).
pub(crate) fn execute_session_receipts(
    session: &mut ProgramSession,
    descriptor: &DeviceDescriptor,
    inputs: &BTreeMap<u32, Vec<f32>>,
    steps: u32,
) -> Result<Vec<faber_host_macos_arm64::composite_host::DeviceExecutionReceipt>, Vec<Diagnostic>> {
    match descriptor.program_lifetime {
        HostDeviceProgramLifetime::RepeatingStep => {
            session
                .init_params(inputs)
                .map_err(|error| vec![super::super::host_factory::host_error_diagnostic(&error)])?;
            let mut receipts = Vec::with_capacity(steps as usize);
            for index in 0..steps {
                // U8/U9 repair: the FINAL step runs through
                // `execute_final_step`, which keeps the declared end-of-run
                // PerStep buffers (final forward, final gradients) live past
                // the step boundary so the one-shot end-of-run readback
                // (`read_end_of_run`, driven by the route) observes them at
                // the declared completion boundary. Ordinary steps recycle
                // every per-step buffer exactly as before.
                let receipt = if index + 1 == steps {
                    session.execute_final_step()
                } else {
                    session.execute_step()
                };
                receipts.push(receipt.map_err(|error| {
                    vec![super::super::host_factory::host_error_diagnostic(&error)]
                })?);
            }
            Ok(receipts)
        }
        HostDeviceProgramLifetime::SingleRun => {
            let repeat_count = device_repeat_count()?;
            let mut receipts = Vec::with_capacity(repeat_count);
            for _ in 0..repeat_count {
                receipts.push(session.execute(inputs).map_err(|error| {
                    vec![super::super::host_factory::host_error_diagnostic(&error)]
                })?);
            }
            Ok(receipts)
        }
    }
}

/// The `FABER_DEVICE_STEPS` env-var override (S5-U5b): when set, the value
/// must agree with the image's **declared** `RepeatingStep` step count
/// (recovered from the wire) — a contradiction fails closed, never a silent
/// override. When absent, the image's declared count is the authority; the
/// env var is never the sole authority for an image-loaded route.
pub(crate) fn device_step_count(declared: u32) -> Result<u32, Vec<Diagnostic>> {
    match std::env::var("FABER_DEVICE_STEPS") {
        Ok(value) => {
            let parsed = value.parse::<u32>().map_err(|error| {
                vec![Diagnostic::error(format!(
                    "FABER_DEVICE_STEPS must be a non-negative integer, got `{value}`: {error}"
                ))]
            })?;
            if parsed != declared {
                return Err(vec![Diagnostic::error(format!(
                    "FABER_DEVICE_STEPS={parsed} contradicts the image's declared RepeatingStep step count {declared}; the route's override must agree with the device image"
                ))]);
            }
            Ok(parsed)
        }
        Err(std::env::VarError::NotPresent) => Ok(declared),
        Err(error) => Err(vec![Diagnostic::error(format!(
            "FABER_DEVICE_STEPS could not be read: {error}"
        ))]),
    }
}

/// Execute a device-bearing FMIR image's device route through the composite
/// host and print the A9/A10 receipt (S2-8) or the training report (S5-U5).
///
/// The ordinary-command launch seam (S1-6): constructs the composite host
/// under the one host-construction policy, builds the typed descriptor from
/// the image's canonical payload + declared artifact blob, executes the
/// full lifecycle (load → allocate → copy-in → launch → sync → readback →
/// release), and prints the A9 observed events (selected hardware, module
/// hash, allocations, launches, syncs, transfers, readbacks, releases), the
/// A10 declared logical resource graph (buffer identities, roles, lifetimes,
/// versions, data-flow edges), and the repeated-execution leak proof.
///
/// A `RepeatingStep` program (S5-U5, the training-loop route) once-inits its
/// HostProvided params at session creation, executes the image's DECLARED
/// step count (recovered from the wire, S5-U5b — `FABER_DEVICE_STEPS`, when
/// set, must agree) on ONE session, prints the per-step loss trace, and runs
/// the convergence check. `FABER_DEVICE_REPEAT` (default 1) runs a
/// `SingleRun` program's ordered launch sequence N times before teardown —
/// the S2-8 leak-proof surface.
///
/// # Errors
/// Fail-closed diagnostics; never a silent CPU fallback.
pub(crate) fn execute_device_route(
    device: &FmirDeviceSection,
    backend: DeviceBackend,
    source_hashes: &[String],
) -> Result<(), Vec<Diagnostic>> {
    // Fail-closed wire admission (S3-A4): the typed-section wire version is
    // gated before any field-level interpretation (old v2 payloads fail
    // closed with the structured `payload_version` diagnostic).
    admit_device_program_section(&device.device_program)?;
    let artifact = artifact_for_backend(&device.artifacts.artifact, backend).ok_or_else(|| {
        vec![super::super::host_factory::missing_backend_artifact(
            backend,
        )]
    })?;
    // A9 discovery receipt: selected device + declared artifact hash.
    let discovery =
        super::super::host_factory::discovery_receipt(backend, &device.artifacts.artifact)
            .ok_or_else(|| {
                vec![super::super::host_factory::missing_backend_artifact(
                    backend,
                )]
            })?;
    discovery.print();
    // The host consumes the WIRE: the descriptor is derived exclusively from
    // the carried program facts (never a thinned slot list).
    let descriptor = descriptor_for_backend(device, backend, artifact.blob.as_bytes())?;
    // Fail-before-launch: the descriptor is validated by the composite host
    // before any kernel is dispatched.
    let selection = match backend {
        DeviceBackend::Metal => DeviceSelection::Metal,
        DeviceBackend::Cuda => DeviceSelection::Cuda,
    };
    let mut host = super::super::host_factory::construct_composite_host(selection, true)
        .map_err(|diagnostic| vec![diagnostic])?;
    let inputs = inputs_by_buffer_id(device);
    // The explicit observation facts, already validated by descriptor
    // construction, are the sole authority for host readback selection (F6):
    // the host projects results from the descriptor's carried result rows.

    // A10 identity over the COMPLETE program (S3-A4): the canonical bytes of
    // the typed wire (semantics-only — CUDA symbols and declared inputs are
    // absent by construction), hashed with the source identities. Both image
    // routes carry the identical wire, so the identity is route-independent.
    let source_refs = source_hashes.iter().map(String::as_str).collect::<Vec<_>>();
    let canonical = radix_mir_fmir::canonical_program_bytes(&device.device_program.program);
    let identity = radix_mir_fmir::device_identity_hash(&source_refs, &canonical);
    println!("device: identity {identity} (A10, complete program)");

    // The step count a RepeatingStep route drives (S5-U5b): the image's
    // DECLARED count is recovered from the wire — the route never falls back
    // to an env-var default. `FABER_DEVICE_STEPS`, when set, must agree
    // (fail-closed). SingleRun routes keep the S2-8 repeat surface.
    let steps = match device.device_program.program.lifetime {
        WireProgramLifetime::RepeatingStep(declared) => device_step_count(declared)?,
        WireProgramLifetime::SingleRun => DEFAULT_TRAINING_STEPS,
    };

    let mut session = super::super::host_factory::create_program_session(&mut host, &descriptor)
        .map_err(|diagnostic| vec![diagnostic])?;
    // S5A-U1: the host session carries the DECLARED cadence sets directly
    // from the descriptor — the wire's `PerStep` result rows (read back
    // every step) and the wire's `EndOfRun` result rows (the final forward,
    // final gradients, final params — read back once after the step loop).
    // The descriptor is validated fail-closed before any launch; there is no
    // route-side derivation and no runtime declaration seam. The final step
    // keeps the declared EndOfRun PerStep buffers live and the route reads
    // the whole set back ONCE at the declared completion boundary; per-step
    // readbacks stay loss-only.
    let receipts = execute_session_receipts(&mut session, &descriptor, &inputs, steps)?;
    let receipt = receipts.last().ok_or_else(|| {
        vec![Diagnostic::error(
            "device route executed zero iterations (FABER_DEVICE_STEPS / FABER_DEVICE_REPEAT must be >= 1)",
        )]
    })?;
    // U8/U9 repair: the declared end-of-run set is read back ONCE at the
    // declared completion boundary — after the step loop, before teardown —
    // so the route prints the final forward / final gradients / final params
    // VALUES instead of a declaration. Within a step the only readback was
    // the loss observation.
    let end_of_run_readback: Option<faber_host_macos_arm64::composite_host::EndOfRunReadback> =
        if descriptor.program_lifetime == HostDeviceProgramLifetime::RepeatingStep {
            Some(
                session.read_end_of_run().map_err(|error| {
                    vec![super::super::host_factory::host_error_diagnostic(&error)]
                })?,
            )
        } else {
            None
        };
    session
        .teardown()
        .map_err(|error| vec![super::super::host_factory::host_error_diagnostic(&error)])?;

    // A9 observed lifecycle events of the last execution (R9): real
    // synchronization operations, the exact readback count, and the
    // completion boundary the receipt states.
    println!(
        "device: module hash fnv64:{:016x} semantic graph hash fnv64:{:016x} launches {} syncs {} transfers {} readbacks {} releases {} allocated {}",
        receipt.module_hash,
        receipt.semantic_graph_hash,
        receipt.launches,
        receipt.syncs,
        receipt.transfers,
        receipt.readbacks,
        receipt.releases,
        receipt.allocated_buffers.len()
    );
    println!("device: {}", receipt.completion_boundary.spelling());
    println!("{}", host_receipt_launch_order_line(&descriptor));

    // A10 declared logical resource graph: render the host's receipt facts
    // verbatim. Faber must not print a duplicate graph derived from the wire
    // after execution, because the host receipt is the observable seam.
    for line in host_receipt_graph_lines(&receipt.resource_graph, &receipt.data_flow_edges) {
        println!("{line}");
    }

    for (buffer_id, values) in &receipt.outputs {
        let name = wire_buffer_name(device, *buffer_id);
        println!(
            "device: output buffer {} `{}` = [{}]",
            buffer_id,
            name,
            values
                .iter()
                .map(|value| format!("{value}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // S5-U5 training report: a RepeatingStep run prints the per-step loss
    // trace and the convergence verdict (the done-when surface). The loss is
    // the DECLARED per-step observation buffer (the wire's PerStep result
    // row) — never a shape/name selection.
    if descriptor.program_lifetime == HostDeviceProgramLifetime::RepeatingStep {
        let per_step = declared_per_step_observations(&device.device_program.program);
        let per_step_observation = per_step.first().copied().ok_or_else(|| {
            vec![Diagnostic::error(
                "a RepeatingStep program must declare a per-step observation (the loss); the wire declares no PerStep result row",
            )]
        })?;
        let report = step_run_report(&receipts, per_step_observation);
        print_training_report(&report);
        // S5A-U1: the end-of-run observations — read back once at the
        // declared completion boundary — print the real VALUES (final
        // forward, final gradients, final params) from the DECLARED
        // EndOfRun result rows, not a declaration.
        if let Some(readback) = &end_of_run_readback {
            print_end_of_run_values(readback, &device.device_program.program);
        }
    }

    // Repeated-execution leak proof (S2-8 done-when): after N runs + teardown
    // the live handle count is 0 and the driver counters are at baseline. On
    // real drivers the counters surface reports all-zero by design (the leak
    // evidence is the handle-registry live count); the fake drivers track
    // cumulative loads/releases so tests prove the cache policy at the driver
    // boundary.
    let live = host
        .device()
        .map(|runtime| runtime.live_handle_count())
        .unwrap_or(0);
    let counters = host.device().map(|runtime| runtime.driver_counters());
    match counters {
        Some(counters) => println!(
            "device: leak proof: {} run(s) then teardown -> live_handle_count()={live}, driver counters at baseline (module loads {} releases {} buffer allocs {} releases {})",
            receipts.len(),
            counters.module_loads,
            counters.module_releases,
            counters.buffer_allocs,
            counters.buffer_releases
        ),
        None => println!(
            "device: leak proof: {} run(s) then teardown -> live_handle_count()={live}, no device session after teardown",
            receipts.len()
        ),
    }
    Ok(())
}

/// Print the S5-U5 training report: the per-step loss trace and the
/// convergence verdict.
fn print_training_report(report: &StepRunReport) {
    println!(
        "device: training: {} step(s) on ONE session; per-step observation (loss) trace:",
        report.step_count
    );
    for (index, observed) in report.loss_trace.iter().enumerate() {
        let values = observed
            .values()
            .map(|values| {
                values
                    .iter()
                    .map(|value| format!("{value}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .collect::<Vec<_>>()
            .join(" | ");
        println!("device:   step {index}: [{values}]");
    }
    match (report.initial_loss, report.final_loss) {
        (Some(initial), Some(last)) => println!(
            "device: training: initial loss {initial}, final loss {last}, converged: {} (final < 0.1 * initial)",
            report.converged
        ),
        _ => println!(
            "device: training: no loss observation read back; convergence not checked"
        ),
    }
}

/// The DECLARED end-of-run observation set of a `RepeatingStep` training
/// program (S5A-U1): the buffers whose FINAL values the exit-gate rows
/// observe once at the end — the final forward outputs, the final gradient
/// buffers, and the trainable params. The SET is declared on the wire (the
/// `EndOfRun` result rows); the route consumes it verbatim — there is no
/// derivation. Within each step the only readback is the loss observation;
/// the end-of-run set is read back once after the last step.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct EndOfRunObservationSet {
    /// The final forward outputs: the declared `EndOfRun` rows on `PerStep`
    /// Output-role buffers (the decomposition's forward tensors).
    pub(crate) forward: Vec<(u32, String)>,
    /// The final gradient buffers: the declared `EndOfRun` rows on `PerStep`
    /// InOut buffers (written by a device-resident companion kernel).
    pub(crate) gradients: Vec<(u32, String)>,
    /// The trainable params: the declared `EndOfRun` rows on `PerProgram`
    /// InOut buffers (device-resident across all steps).
    pub(crate) params: Vec<(u32, String)>,
}

/// Consume the DECLARED end-of-run observation set from the wire (S5A-U1):
/// exactly the result rows whose observation cadence is `EndOfRun`. The set
/// is the constructor's declared fact — this function only groups it for the
/// receipt, reusing each row's declared buffer facts (role + lifetime); it
/// never scans kernels or derives which buffers are observed.
#[must_use]
pub(crate) fn declared_end_of_run_observations(wire: &WireDeviceProgram) -> EndOfRunObservationSet {
    let mut forward: Vec<(u32, String)> = Vec::new();
    let mut gradients: Vec<(u32, String)> = Vec::new();
    let mut params: Vec<(u32, String)> = Vec::new();
    for result in &wire.results {
        if result.observation.cadence != WireObservationCadence::EndOfRun {
            continue;
        }
        let id = result.buffer.id;
        let name = result.buffer.name.clone();
        match (result.buffer.role, result.buffer.lifetime) {
            // The trainable params: PerProgram InOut buffers, read back once
            // at the end (HostProvided once-init persistence).
            (WireBufferRole::InOut, WireBufferLifetime::PerProgram) => params.push((id, name)),
            // The final gradient buffers: PerStep InOut, written by a
            // device-resident companion, never per-step observations.
            (WireBufferRole::InOut, WireBufferLifetime::PerStep) => gradients.push((id, name)),
            // The final forward outputs: PerStep Output finals.
            (WireBufferRole::Output, WireBufferLifetime::PerStep) => forward.push((id, name)),
            _ => {}
        }
    }
    forward.sort_by_key(|(id, _)| *id);
    gradients.sort_by_key(|(id, _)| *id);
    params.sort_by_key(|(id, _)| *id);
    EndOfRunObservationSet {
        forward,
        gradients,
        params,
    }
}

/// The DECLARED per-step observation set of a program (S5A-U1): the result
/// rows whose observation cadence is `PerStep` — for a `RepeatingStep`
/// training program the loss, read back within every step. The route consumes
/// this declared set; it never re-derives which buffer is read back per step.
#[must_use]
pub(crate) fn declared_per_step_observations(wire: &WireDeviceProgram) -> Vec<u32> {
    wire.results
        .iter()
        .filter(|result| result.observation.cadence == WireObservationCadence::PerStep)
        .map(|result| result.buffer.id)
        .collect()
}

/// Print the READ-BACK end-of-run observation VALUES: the final forward, the
/// final gradients, and the final params as observed once at the declared
/// completion boundary — the exit-gate rows show the real device values, not
/// a declaration. The values came from the host's one-shot end-of-run
/// readback; the names ride the wire's DECLARED `EndOfRun` rows.
fn print_end_of_run_values(
    readback: &faber_host_macos_arm64::composite_host::EndOfRunReadback,
    wire: &WireDeviceProgram,
) {
    let declared = declared_end_of_run_observations(wire);
    let name_by_id: BTreeMap<u32, String> = declared
        .forward
        .iter()
        .chain(&declared.gradients)
        .chain(&declared.params)
        .map(|(id, name)| (*id, name.clone()))
        .collect();
    println!("device: end-of-run observations (final forward, final gradients, final params):");
    for (label, category) in [
        ("final forward", &declared.forward),
        ("final gradients", &declared.gradients),
        ("final params", &declared.params),
    ] {
        let rows = category
            .iter()
            .map(|(id, _)| {
                let name = name_by_id
                    .get(id)
                    .map(String::as_str)
                    .unwrap_or("<unnamed>");
                match readback.values.get(id) {
                    Some(values) => format!(
                        "{id} `{name}` = [{}]",
                        values
                            .iter()
                            .map(|value| format!("{value}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    None => format!("{id} `{name}` = <not read back>"),
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("device:   {label}: {rows}");
    }
}

/// Render the exact ordered launch records that the host will execute.
///
/// The descriptor's launch sequence, not the kernel declaration order or the
/// aggregate receipt count, is the observable program order. A kernel index
/// may therefore repeat or appear out of declaration order.
pub(crate) fn host_receipt_launch_order_line(descriptor: &DeviceDescriptor) -> String {
    let launches = descriptor
        .launches
        .iter()
        .enumerate()
        .map(|(position, launch)| {
            let backend_entry = descriptor
                .kernels
                .get(launch.kernel_index as usize)
                .map(|kernel| kernel.entry.as_str())
                .unwrap_or("<invalid>");
            format!(
                "#{} id={} kernel_index={} backend_entry=`{}`",
                position, launch.id, launch.kernel_index, backend_entry
            )
        })
        .collect::<Vec<_>>();
    format!("device: launch order: [{}]", launches.join(", "))
}

pub(crate) fn host_receipt_graph_lines(
    resource_graph: &[faber_host_macos_arm64::composite_host::ReceiptBuffer],
    data_flow_edges: &[faber_host_macos_arm64::composite_host::DataFlowEdge],
) -> Vec<String> {
    let mut lines = vec!["device: declared resource graph (A10, host receipt):".to_owned()];
    for buffer in resource_graph {
        lines.push(format!(
            "device:   buffer {} `{}` {} {} version {} ({}[{}])",
            buffer.id,
            buffer.name,
            buffer.role.spelling(),
            buffer.lifetime.spelling(),
            buffer.version,
            buffer.element_ty.spelling(),
            buffer.element_count
        ));
    }
    if data_flow_edges.is_empty() {
        lines.push("device:   data-flow edges: none".to_owned());
    } else {
        for edge in data_flow_edges {
            lines.push(format!(
                "device:   data-flow {} -> {} via buffer {} version {}",
                edge.producer, edge.consumer, edge.buffer_id, edge.version
            ));
        }
    }
    lines
}

/// The `FABER_DEVICE_REPEAT` env-var hook for the S2-8 repeated-execution
/// leak proof: how many times to run the ordered launch sequence on one
/// session before teardown. Defaults to 1; a non-numeric value fails closed
/// (never a silent fallback to 1).
pub(crate) fn device_repeat_count() -> Result<usize, Vec<Diagnostic>> {
    match std::env::var("FABER_DEVICE_REPEAT") {
        Ok(value) => value.parse::<usize>().map_err(|error| {
            vec![Diagnostic::error(format!(
                "FABER_DEVICE_REPEAT must be a non-negative integer, got `{value}`: {error}"
            ))]
        }),
        Err(std::env::VarError::NotPresent) => Ok(1),
        Err(error) => Err(vec![Diagnostic::error(format!(
            "FABER_DEVICE_REPEAT could not be read: {error}"
        ))]),
    }
}
