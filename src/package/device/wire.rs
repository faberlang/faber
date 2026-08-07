// Sibling + root items: explicit `use super` lists carry the seams the mir/
// split routes through `use super::*` (wildcard imports are denied).
use super::{
    BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, CollectionKernelPlan,
    DeviceProgram, DeviceProgramLifetime, DeviceResource, DeviceSemantics, Diagnostic,
    FmirDeviceProgramSection, FmirDeviceSection, InitializationPolicy, LaunchId,
    LosslessMirCompanionEntry, MirCompanionDerivativeKind, MirKernelResourceAccess,
    MirTensorStorageLayout, MirType, SemanticValueOrigin, WireBarrierPhase, WireBarrierPoint,
    WireBinding, WireBufferIdentity, WireBufferLifetime, WireBufferRole, WireBufferVersion,
    WireCollectionKernelPlan, WireCompanionDerivativeKind, WireCompanionRelation,
    WireCompanionSelectedInput, WireCompanionSelectedOutput, WireDependencyEdge, WireDeviceProgram,
    WireDeviceResource, WireDispatchSize, WireInitializationPolicy, WireKernelLaunchPlan,
    WireKernelUnit, WireLaunchUnit, WireMatMulPlan, WireMatMulSharedMemory, WireObservationCadence,
    WireObservationFact, WireOobPaddingPolicy, WireProgramLifetime, WireReduceOp,
    WireReductionPlan, WireResourceAccess, WireResultBuffer, WireSemanticValue,
    WireSemanticValueOrigin, WireSharedMemoryLayout, WireStorageLayout, WireTransposePlan,
    WireWorkgroupCount, WireWorkgroupSize,
};
// S6-C2 wire-surface imports: the new plan wire mirrors and the carried
// broadcast-fact mirror live on the radix-mir-fmir schema (not on the faber
// mir-split seam), so they import directly from the schema crate.
use radix_mir::abi::{MirBroadcastDeclaration, MirRankExtensionBroadcast};
use radix_mir_fmir::schema::{
    WireAxisReductionPlan, WireBroadcastDeclaration, WireBroadcastFact,
    WireCausalMaskedSoftmaxPlan, WireGatherPlan, WireInputUpdateCadence, WireInvocationMode,
    WireKvCacheDtype, WireLayerNormalizationPlan, WireReducedProjection, WireRmsNormalizationPlan,
    WireRopePlan, WireRowSoftmaxPlan, WireSessionObservationCadence, WIRE_SESSION_SECTION_VERSION,
};
// Doc-link surface: the carried generation type appears only in an
// intra-doc link here; the import keeps the link resolvable from this module.
#[allow(unused_imports)]
use super::ValueGeneration;

// ---------------------------------------------------------------------------
// Typed device-program wire (S3-A4 / N3.3, the operator-authorized seam break)
// ---------------------------------------------------------------------------

/// Wire/codec version. Bump only with a documented representation change
/// (the wire is the identity substrate of the same-package hash).
///
/// **v2 → v3 (S3-A4, the faber-owned codec bump in lockstep with the FMIR
/// 4 → 5 ratchet):** the thinned `DeviceRunPlan` slot list is REPLACED by
/// the serialized COMPLETE program — the typed
/// [`FmirDeviceProgramSection`] wire (kernels, launches, results,
/// per-resource access + version). CUDA symbols and host input values are
/// no longer part of the canonical program bytes: they ride the per-artifact
/// symbol mapping and the declared-inputs section. Admission is fail-closed:
/// [`admit_device_program_section`] checks the wire version before any
/// field-level interpretation, so an old wire payload fails with the
/// structured `payload_version` diagnostic.
///
/// **v3 → v4 (Stage 3R U1, the F1–F7 frozen-contract clean break, S1):**
/// the wire gains the carried semantic-value identities (F1), explicit
/// value generations per slot (F2), roots + producer/consumer dependencies
/// (F3), and the independent initialization / observation axes (F5/F6).
/// Bumped in lockstep with [`radix_mir_fmir::WIRE_DEVICE_PROGRAM_VERSION`];
/// the radix decode boundary rejects any other value before field-level
/// interpretation (S8/F7).
///
/// **v4 → v5 (S5-U5b, the declared-step-count clean break):** the
/// `RepeatingStep` lifetime variant carries the declared training step
/// count (a count the wire admits fail-closed at decode — zero is
/// rejected). Bumped in lockstep with
/// [`radix_mir_fmir::WIRE_DEVICE_PROGRAM_VERSION`].
///
/// **v5 → v6 (S5A-U1, the observation-cadence clean break):** every result
/// row's explicit observation fact gains the **observation cadence**
/// discriminator (`PerStep` / `EndOfRun`) — when the declared observation is
/// read back. The canonical wire bytes now change when the readback cadence
/// changes (A7). Bumped in lockstep with
/// [`radix_mir_fmir::WIRE_DEVICE_PROGRAM_VERSION`].
///
/// **v6 → v7 (S6-C2, the broadcast-fact + recipe-mirror clean break):** the
/// kernel plan gains the broadcast-carrying
/// [`WireCollectionKernelPlan::RankExtensionAdd`] variant (the typed
/// [`WireBroadcastFact`] of the `addita_bias` rank-extension lowering) and
/// the S6-N1-frozen recipe variants gain real wire mirrors
/// (`AxisReduction` / `RowSoftmax` / `LayerNormalization`). The carried
/// broadcast facts are admitted fail-closed here (a shape mismatch rejects)
/// and at the radix decode boundary (A7). Bumped in lockstep with
/// [`radix_mir_fmir::WIRE_DEVICE_PROGRAM_VERSION`].
///
/// **v7 → v8 (council-7/council-8 CB-2/CB-3, the GI3-1-visible +
/// reduced-projection clean break):** the wire records the GI3-1 shape
/// changes that landed inside wire-7 without a ratchet (`WireRopePlan.per_row`
/// / `rows`, the `MirUnOp::F16Round` unary — wire-7 was an unpublished clean
/// break, no pre-GI3-1 wire-7 artifact escaped the repo), and every
/// `AxisReduction` plan + reduced buffer version carries the
/// producer-defined **reduced-resource projection** (`axis_extent` +
/// `inner_stride`) so keep-dims consumers never reconstruct the reduced
/// buffer mapping from element count. Bumped in lockstep with
/// [`radix_mir_fmir::WIRE_DEVICE_PROGRAM_VERSION`].
pub(crate) const DEVICE_RUN_PLAN_VERSION: u32 = 8;

/// Fail-closed wire admission (S3-A4): the wire version is read FIRST,
/// before any field-level interpretation, so an old (or unknown) codec
/// version fails with the structured `payload_version` diagnostic — never a
/// silent default and never a generic field-level error that hides the
/// version gate. After the version gate, the S5-U5b declared step count is
/// admitted: a `RepeatingStep` program must declare at least one step (a
/// count the route could never drive fails closed here and at the radix
/// decode boundary). The S6-C2 carried broadcast facts are then admitted:
/// every `RankExtensionAdd` plan payload must be internally consistent (a
/// shape mismatch rejects — the fact is carried, never inferred, A3).
///
/// # Errors
/// Fail-closed when the wire carries an unsupported version, a
/// `RepeatingStep` program declares a zero step count, or a carried
/// rank-extension broadcast fact is internally inconsistent.
pub(crate) fn admit_device_program_section(
    section: &FmirDeviceProgramSection,
) -> Result<(), Vec<Diagnostic>> {
    if section.v != DEVICE_RUN_PLAN_VERSION {
        return Err(vec![Diagnostic::error(format!(
            "device section wire version {} is not supported (expected {})",
            section.v, DEVICE_RUN_PLAN_VERSION
        ))
        .with_arg("issue", "E_DEVICE_DESCRIPTOR")
        .with_arg("payload_version", section.v.to_string())]);
    }
    // S5-U5b: the declared RepeatingStep count is admitted fail-closed at
    // the faber boundary too — a zero count is a contradiction, never a
    // silently-defaulted step loop.
    if let WireProgramLifetime::RepeatingStep(declared) = section.program.lifetime {
        if declared == 0 {
            return Err(vec![Diagnostic::error(
                "device image declares a RepeatingStep program with step count 0; a training program must declare at least one step",
            )]);
        }
    }
    // S6-C2 (A7): every carried rank-extension broadcast fact (the
    // `RankExtensionAdd` plan payload) must be internally consistent — the
    // higher-rank operand is exactly one rank above the lower-rank operand,
    // the lower-rank dims equal the higher-rank trailing dims, and the
    // result is the higher-rank shape. A shape mismatch fails closed here,
    // the same rule the radix decode boundary runs.
    for kernel in &section.program.kernels {
        let WireCollectionKernelPlan::RankExtensionAdd(fact) = &kernel.plan else {
            continue;
        };
        let (higher, lower) = if fact.lhs_shape.len() >= fact.rhs_shape.len() {
            (&fact.lhs_shape, &fact.rhs_shape)
        } else {
            (&fact.rhs_shape, &fact.lhs_shape)
        };
        if fact.declaration != WireBroadcastDeclaration::RankExtension {
            return Err(vec![Diagnostic::error(format!(
                "kernel '{}' carries the unsupported broadcast declaration {fact:?}; only rank-extension is admitted (S6-C2 A7)",
                kernel.entry
            ))]);
        }
        if higher.len() != lower.len() + 1 || lower != &higher[1..] {
            return Err(vec![Diagnostic::error(format!(
                "kernel '{}' carries a mismatched rank-extension broadcast fact (lhs {:?}, rhs {:?}); the lower-rank dims must equal the higher-rank trailing dims (S6-C2 A7)",
                kernel.entry, fact.lhs_shape, fact.rhs_shape
            ))]);
        }
        if fact.result_shape != *higher {
            return Err(vec![Diagnostic::error(format!(
                "kernel '{}' carries a rank-extension broadcast fact whose result shape {:?} does not equal the higher-rank operand shape {:?} (S6-C2 A7)",
                kernel.entry, fact.result_shape, *higher
            ))]);
        }
    }
    Ok(())
}

/// GI4-2: the faber boundary admission of the optional cadence/session
/// section (the codec arm for the session surface). The session-section
/// version ratchet is gated BEFORE any field-level interpretation (the same
/// pattern as [`admit_device_program_section`]), then the carried session
/// facts are validated fail-closed against the carried single-device program
/// — the same rule set the radix decode boundary runs (the equivalence-gate
/// discipline: admission inspects the actually-delivered session facts,
/// never a self-declared hash).
///
/// Absent for packages without an inference-session surface (`None` — the
/// single-device default; single-device packages do not require the section,
/// the MD-A15 precedent). The session section carries its OWN version ratchet
/// on the accepted wire version — no `DEVICE_RUN_PLAN_VERSION` /
/// `WIRE_DEVICE_PROGRAM_VERSION` bump (the MD2-W1 sibling-field precedent).
///
/// # Errors
/// Fail-closed when the section carries an unsupported version, a zero KV
/// layout dimension, a dtype outside the closed `f32` surface, a missing or
/// duplicate workload mode, an unresolvable or duplicate session-input slot,
/// a cadence that contradicts the carried program's buffer facts, or an
/// observation cadence that contradicts a decode invocation mode.
pub(crate) fn admit_session_section(device: &FmirDeviceSection) -> Result<(), Vec<Diagnostic>> {
    let Some(section) = &device.session else {
        return Ok(());
    };
    if section.v != WIRE_SESSION_SECTION_VERSION {
        return Err(vec![Diagnostic::error(format!(
            "session section version {} is not supported (expected {})",
            section.v, WIRE_SESSION_SECTION_VERSION
        ))
        .with_arg("issue", "E_DEVICE_SESSION_SECTION")
        .with_arg("session_version", section.v.to_string())]);
    }
    // The typed KV-layout carriage: every dimension positive (a zero
    // slot/context/layer/head layout is not a layout — it would silently
    // produce a zero-byte KV, the `KvCacheLayout::new` mirror).
    let kv = &section.kv_layout;
    if kv.slots == 0 {
        return Err(session_section_invalid(
            "kv layout declares `slots` 0; a zero-slot layout is not a layout",
        ));
    }
    if kv.context_length == 0 {
        return Err(session_section_invalid(
            "kv layout declares `context_length` 0; a zero-context layout is not a layout",
        ));
    }
    if kv.layer_count == 0 {
        return Err(session_section_invalid(
            "kv layout declares `layer_count` 0; a zero-layer layout is not a layout",
        ));
    }
    if kv.kv_head_count == 0 {
        return Err(session_section_invalid(
            "kv layout declares `kv_head_count` 0; a zero-head layout is not a layout",
        ));
    }
    if kv.head_dim == 0 {
        return Err(session_section_invalid(
            "kv layout declares `head_dim` 0; a zero-dim layout is not a layout",
        ));
    }
    // The dtype surface is closed (a dtype change is a contract revision).
    if !matches!(kv.dtype, WireKvCacheDtype::F32) {
        return Err(session_section_invalid(format!(
            "kv layout declares dtype {:?}; the dtype surface is closed to f32 (a dtype change is a contract revision, never a silent widening)",
            kv.dtype
        )));
    }
    // At least one workload mode, no duplicates (a session that executes no
    // workload mode is a contradiction).
    if section.invocation_modes.is_empty() {
        return Err(session_section_invalid(
            "session declares no invocation mode; a session that executes no workload mode is a contradiction",
        ));
    }
    for (index, mode) in section.invocation_modes.iter().enumerate() {
        if section.invocation_modes[index + 1..].contains(mode) {
            return Err(session_section_invalid(format!(
                "session declares invocation mode {} more than once",
                mode.spelling()
            )));
        }
    }
    // Session-input slots: unique + every slot resolves against the carried
    // program's buffers.
    for (index, input) in section.inputs.iter().enumerate() {
        if section.inputs[index + 1..]
            .iter()
            .any(|other| other.slot == input.slot)
        {
            return Err(session_section_invalid(format!(
                "session declares input slot {} more than once; one cadence per slot",
                input.slot
            )));
        }
        if session_buffer_fact(device, input.slot).is_none() {
            return Err(session_section_invalid(format!(
                "session input slot {} does not resolve against a buffer the carried single-device program declares",
                input.slot
            )));
        }
    }
    // Cadence vs the carried buffer facts: a resident input names a
    // PerProgram buffer, and a once-init (PerProgram + HostProvided) buffer
    // named as a session input is resident — a per-invocation cadence on a
    // once-init buffer is the SingleRun re-copy semantic the GI4-1 discovery
    // proved infeasible.
    for input in &section.inputs {
        let Some(fact) = session_buffer_fact(device, input.slot) else {
            continue;
        };
        match input.cadence {
            WireInputUpdateCadence::Resident
                if fact.lifetime != WireBufferLifetime::PerProgram =>
            {
                return Err(session_section_invalid(format!(
                    "session input slot {} (buffer '{}') declares cadence resident, but the buffer is {}; a resident input is uploaded once at session creation and lives for the session (per-program)",
                    input.slot,
                    fact.name,
                    fact.lifetime.spelling()
                )));
            }
            WireInputUpdateCadence::PerInvocation
                if fact.lifetime == WireBufferLifetime::PerProgram
                    && fact.initialization == WireInitializationPolicy::HostProvided =>
            {
                return Err(session_section_invalid(format!(
                    "session input slot {} (buffer '{}') declares cadence per_invocation, but the buffer is a once-init per-program host-provided buffer; a per-invocation cadence on a once-init buffer is the SingleRun re-copy semantic the GI4-1 discovery proved infeasible (weights would re-upload per invocation)",
                    input.slot,
                    fact.name
                )));
            }
            _ => {}
        }
    }
    // Observation cadence: a scalar-decode session observes per invocation
    // (the full-vocab logits are sampled host-side on every invocation).
    if section
        .invocation_modes
        .contains(&WireInvocationMode::ScalarDecode)
        && section.observation_cadence != WireSessionObservationCadence::PerInvocation
    {
        return Err(session_section_invalid(format!(
            "session executes {} (a one-token decode step) but declares observation cadence {}; per-token decode reads back its observation per token",
            WireInvocationMode::ScalarDecode.spelling(),
            section.observation_cadence.spelling()
        )));
    }
    Ok(())
}

/// The carried buffer facts of one wire buffer slot (identity, lifetime,
/// initialization) — what the session admission resolves the declared
/// session-input slots and cadence facts against.
struct SessionBufferFact<'a> {
    name: &'a str,
    lifetime: WireBufferLifetime,
    initialization: WireInitializationPolicy,
}

/// The first carried buffer fact for a slot id in the device section's wire
/// program (the equivalence-gate resolution surface for the session-input
/// slots).
fn session_buffer_fact(device: &FmirDeviceSection, slot: u32) -> Option<SessionBufferFact<'_>> {
    let wire = &device.device_program.program;
    wire.kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .find(|resource| resource.buffer.id == slot)
        .map(|resource| SessionBufferFact {
            name: &resource.buffer.name,
            lifetime: resource.buffer.lifetime,
            initialization: resource.initialization,
        })
}

/// A focused faber-boundary diagnostic for a session-section admission
/// failure (the session-surface issue code).
fn session_section_invalid(detail: impl Into<String>) -> Vec<Diagnostic> {
    vec![
        Diagnostic::error(format!("session section: {}", detail.into()))
            .with_arg("issue", "E_DEVICE_SESSION_SECTION"),
    ]
}

/// Build the typed complete-program wire from a constructed device program
/// and its carried semantic facts (S3-A4). Every program fact the constructor
/// materialized is carried field-for-field: kernels (function id, entry,
/// plan, typed resources with access + version, launch), the ordered launch
/// sequence, the program lifetime, and the explicit result buffers — plus
/// the frozen semantic contract (Stage 3R F1–F7): the semantic-value table
/// (F1), explicit value generations per slot (F2), carried roots +
/// producer/consumer dependencies (F3), the lossless primal/companion
/// relation (F4), per-buffer initialization policies and explicit
/// observation facts on every result row (F5/F6). CUDA symbols and host
/// input values are NOT program semantics — they never enter the canonical
/// bytes. A `RepeatingStep` program carries its declared training step count
/// (S5-U5b) in the `RepeatingStep(count)` lifetime variant.
///
/// S6-C2: the carried per-kernel rank-extension broadcast facts are threaded
/// through [`wire_program_for_program_with_broadcast_facts`]; the ordinary
/// producer (the current constructor does not resolve the facts yet — the
/// route is S6-P1) passes an empty set, so every kernel stays on its plan's
/// own wire mirror.
#[must_use]
pub(crate) fn wire_program_for_program(
    program: &DeviceProgram,
    semantics: &DeviceSemantics,
    repeating_steps: u32,
) -> WireDeviceProgram {
    wire_program_for_program_with_broadcast_facts(program, semantics, repeating_steps, &[])
}

/// The full S6-C2 producer: the same complete-program projection with the
/// carried per-kernel rank-extension broadcast facts (the `addita_bias`
/// device-ABI facts) threaded onto the wire. A kernel whose
/// [`MirRankExtensionBroadcast`] is carried at `kernel_index` emits
/// [`WireCollectionKernelPlan::RankExtensionAdd`] (the typed fact rides the
/// plan — a program fact, never dropped and never inferred, A3). The
/// `broadcast_facts` slice is indexed by kernel declaration order; a kernel
/// without a carried fact keeps its plan's own mirror.
#[must_use]
pub(crate) fn wire_program_for_program_with_broadcast_facts(
    program: &DeviceProgram,
    semantics: &DeviceSemantics,
    repeating_steps: u32,
    broadcast_facts: &[Option<&MirRankExtensionBroadcast>],
) -> WireDeviceProgram {
    WireDeviceProgram {
        kernels: program
            .kernels
            .iter()
            .enumerate()
            .map(|(kernel_index, kernel)| WireKernelUnit {
                function: kernel.function.0,
                entry: kernel.entry.clone(),
                plan: wire_plan(
                    &kernel.plan,
                    broadcast_facts.get(kernel_index).copied().flatten(),
                ),
                resources: kernel
                    .resources
                    .iter()
                    .map(|resource| WireDeviceResource {
                        buffer: wire_buffer_identity(&resource.buffer),
                        version: wire_buffer_version(&resource.version),
                        binding: WireBinding {
                            group: resource.binding.group,
                            binding: resource.binding.binding,
                        },
                        access: wire_access(resource.access),
                        // Explicit value generation (F2): the slot's carried
                        // generation fact — a write/read-write slot carries
                        // the generation its launch produces; a read slot
                        // carries the generation it consumes. Never a
                        // universal `1`.
                        generation: wire_slot_generation(
                            program,
                            semantics,
                            kernel_index,
                            resource,
                        ),
                        // Independent initialization axis (F5): how the
                        // buffer's storage is brought to its first defined
                        // state, decided from access facts, never from role.
                        initialization: wire_initialization_policy(semantics, resource.buffer.id),
                    })
                    .collect(),
                launch: WireKernelLaunchPlan {
                    workgroup: WireWorkgroupSize {
                        x: kernel.launch.workgroup.x,
                        y: kernel.launch.workgroup.y,
                        z: kernel.launch.workgroup.z,
                    },
                    dispatch_size: WireDispatchSize {
                        x: kernel.launch.dispatch_size.x,
                        y: kernel.launch.dispatch_size.y,
                        z: kernel.launch.dispatch_size.z,
                    },
                    workgroup_count: WireWorkgroupCount {
                        x: kernel.launch.workgroup_count.x,
                        y: kernel.launch.workgroup_count.y,
                        z: kernel.launch.workgroup_count.z,
                    },
                },
            })
            .collect(),
        launches: program
            .launches
            .iter()
            .map(|launch| WireLaunchUnit {
                id: launch.id.0,
                kernel_index: u32::try_from(launch.kernel_index).unwrap_or(u32::MAX),
            })
            .collect(),
        lifetime: match program.lifetime {
            DeviceProgramLifetime::SingleRun => WireProgramLifetime::SingleRun,
            DeviceProgramLifetime::RepeatingStep => {
                WireProgramLifetime::RepeatingStep(repeating_steps)
            }
        },
        results: program
            .results
            .iter()
            .map(|result| WireResultBuffer {
                buffer: wire_buffer_identity(&result.buffer),
                version: wire_buffer_version(&result.version),
                role: wire_role(result.role),
                produced_by: result.produced_by.0,
                // Explicit observation fact (F5/F6) + observation cadence
                // (S5A-U1): the result row IS a declared observation at the
                // producing launch's completion boundary, read back per step
                // (PerStep) or once after the step loop (EndOfRun). The
                // cadence is the constructor's declared fact — carried
                // verbatim, never re-derived from shapes.
                observation: WireObservationFact {
                    at_launch: result.produced_by.0,
                    cadence: match result.cadence {
                        radix_mir::device_program::ObservationCadence::PerStep => {
                            WireObservationCadence::PerStep
                        }
                        radix_mir::device_program::ObservationCadence::EndOfRun => {
                            WireObservationCadence::EndOfRun
                        }
                    },
                },
            })
            .collect(),
        // The carried semantic-value table (F1): every value minted from its
        // carried MIR/value origin.
        semantic_values: semantics
            .values
            .iter()
            .map(|value| WireSemanticValue {
                id: value.id.0,
                name: value.name.clone(),
                origin: wire_origin(&value.origin),
            })
            .collect(),
        // Declared legal execution roots (F3): the launches no dependency
        // edge feeds — the minimal set a host may start from.
        roots: semantics.roots.iter().map(|root| root.0).collect(),
        // Carried producer/consumer dependency edges (F3): hosts schedule the
        // validated graph from these facts, never from kernel declaration
        // order.
        dependencies: semantics
            .dependencies
            .iter()
            .map(|edge| WireDependencyEdge {
                producer: edge.producer.0,
                consumer: edge.consumer.0,
                buffer: edge.buffer.0,
                version: edge.version,
            })
            .collect(),
        // Lossless primal/companion relation rows (F4) projected from the
        // carried carrier.
        relations: semantics.relations.iter().map(wire_relation).collect(),
    }
}

/// The explicit value generation (F2) a kernel slot carries on the wire,
/// projected from the carried generation facts (the semantic
/// [`ValueGeneration`] list) — never reconstructed from declaration order or
/// a universal `1`. A write/read-write slot carries the generation its
/// launch produces; a read slot carries the generation its launch consumes
/// (the latest produced before it, or the buffer's initial state).
fn wire_slot_generation(
    program: &DeviceProgram,
    semantics: &DeviceSemantics,
    kernel_index: usize,
    resource: &DeviceResource,
) -> u32 {
    let value = semantics
        .bindings
        .iter()
        .find(|binding| binding.buffer == resource.buffer.id)
        .map(|binding| binding.value);
    let Some(value) = value else {
        return 1;
    };
    let Some(launch) = program
        .launches
        .iter()
        .find(|launch| launch.kernel_index == kernel_index)
    else {
        return 1;
    };
    match resource.access {
        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite => semantics
            .generations
            .iter()
            .find(|generation| generation.value == value && generation.produced_by == launch.id)
            .map(|generation| generation.generation)
            .unwrap_or(1),
        MirKernelResourceAccess::Read => {
            let position = launch_position(program, launch.id);
            semantics
                .generations
                .iter()
                .filter(|generation| {
                    generation.value == value
                        && launch_position(program, generation.produced_by) < position
                })
                .map(|generation| generation.generation)
                .max()
                .unwrap_or(1)
        }
    }
}

/// The position of a launch in the ordered execution sequence.
fn launch_position(program: &DeviceProgram, launch: LaunchId) -> usize {
    program
        .launches
        .iter()
        .position(|candidate| candidate.id == launch)
        .unwrap_or(usize::MAX)
}

/// Map a carried [`SemanticValueOrigin`] onto its typed wire mirror (F1).
fn wire_origin(origin: &SemanticValueOrigin) -> WireSemanticValueOrigin {
    match origin {
        SemanticValueOrigin::MirLocal { function, local } => WireSemanticValueOrigin::MirLocal {
            function: function.0,
            local: *local,
        },
        SemanticValueOrigin::HostInput => WireSemanticValueOrigin::HostInput,
        SemanticValueOrigin::Synthetic { label } => WireSemanticValueOrigin::Synthetic {
            label: label.clone(),
        },
    }
}

/// Map a carried lossless companion-relation row onto its typed wire mirror
/// (F4): the gradient-to-primal identity survives onto the serialized
/// package.
fn wire_relation(entry: &LosslessMirCompanionEntry) -> WireCompanionRelation {
    WireCompanionRelation {
        primal: entry.primal.0,
        companion: entry.companion.0,
        derivative: match entry.derivative {
            MirCompanionDerivativeKind::ReverseModeVjp => {
                WireCompanionDerivativeKind::ReverseModeVjp
            }
        },
        device_resident: entry.device_resident,
        selected_inputs: entry
            .selected_inputs
            .iter()
            .map(|selected| WireCompanionSelectedInput {
                param: selected.param.0,
                position: selected.position,
                ty: selected.ty.0,
                gradient_slot: selected.gradient_slot,
            })
            .collect(),
        selected_outputs: entry
            .selected_outputs
            .iter()
            .map(|selected| WireCompanionSelectedOutput {
                position: selected.position,
                ty: selected.ty.0,
                upstream_gradient_ty: selected.upstream_gradient_ty.0,
            })
            .collect(),
    }
}

/// The independent initialization axis (F5) of a buffer, projected onto the
/// wire from the carried facts: how its storage is brought to its first
/// defined state — decided from the buffer's aggregate access facts, never
/// from its role or lifetime. A read-only host-provided buffer is uploaded
/// (HostProvided); a buffer any slot writes in place is zero-filled at
/// allocation (ZeroFill — its first generation is defined at allocation); a
/// kernel-written buffer is fully defined by the kernel before any read
/// (KernelInitialized).
fn wire_initialization_policy(
    semantics: &DeviceSemantics,
    buffer: BufferId,
) -> WireInitializationPolicy {
    semantics
        .initializations
        .iter()
        .find(|fact| fact.buffer == buffer)
        .map(|fact| match fact.policy {
            InitializationPolicy::ZeroFill => WireInitializationPolicy::ZeroFill,
            InitializationPolicy::HostProvided => WireInitializationPolicy::HostProvided,
            InitializationPolicy::KernelInitialized => WireInitializationPolicy::KernelInitialized,
        })
        .unwrap_or(WireInitializationPolicy::KernelInitialized)
}

fn wire_buffer_identity(identity: &BufferIdentity) -> WireBufferIdentity {
    WireBufferIdentity {
        id: identity.id.0,
        name: identity.name.clone(),
        role: wire_role(identity.role),
        storage: match identity.storage {
            MirTensorStorageLayout::DeviceHandle => WireStorageLayout::DeviceHandle,
            MirTensorStorageLayout::HostOwned => WireStorageLayout::HostOwned,
        },
        lifetime: match identity.lifetime {
            BufferLifetime::PerProgram => WireBufferLifetime::PerProgram,
            BufferLifetime::PerStep => WireBufferLifetime::PerStep,
            BufferLifetime::ObservationPoint => WireBufferLifetime::ObservationPoint,
        },
        // The stable semantic value identity (F1): every buffer reference
        // carries the value it holds (first projection — one value per
        // buffer, minted above).
        semantic_value: identity.id.0,
    }
}

fn wire_buffer_version(version: &BufferVersion) -> WireBufferVersion {
    WireBufferVersion {
        version: version.version,
        element_ty: data_type_spelling(version.element_ty),
        element_count: version.element_count,
        // The producer-defined reduced-buffer projection (council-8 CB-3)
        // rides the wire version explicitly — never defaulted.
        reduced_projection: version.reduced_projection.map(|projection| {
            WireReducedProjection {
                axis_extent: projection.axis_extent,
                inner_stride: projection.inner_stride,
            }
        }),
    }
}

fn wire_role(role: BufferRole) -> WireBufferRole {
    match role {
        BufferRole::Input => WireBufferRole::Input,
        BufferRole::Output => WireBufferRole::Output,
        BufferRole::InOut => WireBufferRole::InOut,
    }
}

fn wire_access(access: MirKernelResourceAccess) -> WireResourceAccess {
    match access {
        MirKernelResourceAccess::Read => WireResourceAccess::Read,
        MirKernelResourceAccess::Write => WireResourceAccess::Write,
        MirKernelResourceAccess::ReadWrite => WireResourceAccess::ReadWrite,
    }
}

/// Map a radix [`CollectionKernelPlan`] onto its typed wire mirror (the
/// complete plan is a program fact — never dropped on the wire). An
/// elementwise kernel whose device ABI records a rank-extension broadcast
/// fact (S6-C2, the `addita_bias` lowering) carries
/// [`WireCollectionKernelPlan::RankExtensionAdd`] — the typed fact rides the
/// plan, never inferred (A3); a recipe plan never carries a broadcast fact.
fn wire_plan(
    plan: &CollectionKernelPlan,
    broadcast: Option<&MirRankExtensionBroadcast>,
) -> WireCollectionKernelPlan {
    match plan {
        CollectionKernelPlan::Elementwise => match broadcast {
            Some(fact) => WireCollectionKernelPlan::RankExtensionAdd(wire_broadcast_fact(fact)),
            None => WireCollectionKernelPlan::Elementwise,
        },
        CollectionKernelPlan::TiledMatMul(matmul) => {
            WireCollectionKernelPlan::TiledMatMul(WireMatMulPlan {
                m: matmul.m,
                k: matmul.k,
                n: matmul.n,
                tile: matmul.tile,
                workgroup_x: matmul.workgroup_x,
                workgroup_y: matmul.workgroup_y,
                shared_memory: WireMatMulSharedMemory {
                    shared_a: wire_shared_layout(&matmul.shared_memory.shared_a),
                    shared_b: wire_shared_layout(&matmul.shared_memory.shared_b),
                },
                barriers: matmul
                    .barriers
                    .iter()
                    .map(|barrier| WireBarrierPoint {
                        after: match barrier.after {
                            radix_mir::kernel_plan::BarrierPhase::SharedMemoryLoad => {
                                WireBarrierPhase::SharedMemoryLoad
                            }
                            radix_mir::kernel_plan::BarrierPhase::ReductionStep => {
                                WireBarrierPhase::ReductionStep
                            }
                            radix_mir::kernel_plan::BarrierPhase::InnerProductStep => {
                                WireBarrierPhase::InnerProductStep
                            }
                        },
                    })
                    .collect(),
                oob_padding: wire_oob_padding(matmul.oob_padding),
            })
        }
        CollectionKernelPlan::TreeReduction(reduction) => {
            WireCollectionKernelPlan::TreeReduction(WireReductionPlan {
                op: wire_reduce_op(reduction.op),
                length: reduction.length,
                workgroup_x: reduction.workgroup_x,
                partials: reduction.partials,
                shared_memory: wire_shared_layout(&reduction.shared_memory),
                barriers: reduction
                    .barriers
                    .iter()
                    .map(|barrier| WireBarrierPoint {
                        after: match barrier.after {
                            radix_mir::kernel_plan::BarrierPhase::SharedMemoryLoad => {
                                WireBarrierPhase::SharedMemoryLoad
                            }
                            radix_mir::kernel_plan::BarrierPhase::ReductionStep => {
                                WireBarrierPhase::ReductionStep
                            }
                            radix_mir::kernel_plan::BarrierPhase::InnerProductStep => {
                                WireBarrierPhase::InnerProductStep
                            }
                        },
                    })
                    .collect(),
                oob_padding: wire_oob_padding(reduction.oob_padding),
            })
        }
        // S5-U1: the rank-2 transpose recipe is a complete program fact —
        // never dropped on the wire (the mirror must carry it).
        CollectionKernelPlan::Transpose(transpose) => {
            WireCollectionKernelPlan::Transpose(WireTransposePlan {
                m: transpose.m,
                n: transpose.n,
                workgroup_x: transpose.workgroup_x,
                dispatch_x: transpose.dispatch_x,
            })
        }
        // S6-C2: the S6-N1-frozen recipe variants now carry their real wire
        // mirrors (the radix-mir-fmir schema pins the exact shapes at
        // radix-mir-fmir/src/schema/wire.rs) — the plans are program facts,
        // carried field-for-field. Recipe admission/resolution is S6-P1, but
        // the wire surface is the producer contract under wire 7.
        CollectionKernelPlan::AxisReduction(plan) => {
            WireCollectionKernelPlan::AxisReduction(WireAxisReductionPlan {
                op: wire_reduce_op(plan.op),
                axis: plan.axis,
                // The producer-defined reduced-resource projection
                // (council-8 CB-3) is carried field-for-field.
                projection: WireReducedProjection {
                    axis_extent: plan.projection.axis_extent,
                    inner_stride: plan.projection.inner_stride,
                },
            })
        }
        CollectionKernelPlan::RowSoftmax(plan) => {
            WireCollectionKernelPlan::RowSoftmax(WireRowSoftmaxPlan { axis: plan.axis })
        }
        CollectionKernelPlan::LayerNormalization(plan) => {
            WireCollectionKernelPlan::LayerNormalization(WireLayerNormalizationPlan {
                axis: plan.axis,
            })
        }
        // GI3-1: the pinned-row inference recipes gain their mechanical wire
        // mirrors (the F1 decision, audit 112dc81a; S6-C2 precedent) — the
        // plans are program facts carried field-for-field under wire 7, no
        // `FmirDeviceSection` field change, no version bump.
        CollectionKernelPlan::Gather(plan) => WireCollectionKernelPlan::Gather(WireGatherPlan {
            table_rows: plan.table_rows,
            table_cols: plan.table_cols,
            id_count: plan.id_count,
        }),
        CollectionKernelPlan::RmsNormalization(plan) => {
            WireCollectionKernelPlan::RmsNormalization(WireRmsNormalizationPlan {
                axis: plan.axis,
                epsilon_bits: plan.epsilon_bits,
                width: plan.width,
            })
        }
        CollectionKernelPlan::Rope(plan) => WireCollectionKernelPlan::Rope(WireRopePlan {
            pos: plan.pos,
            dim: plan.dim,
            width: plan.width,
            per_row: plan.per_row,
            rows: plan.rows,
        }),
        CollectionKernelPlan::CausalMaskedSoftmax(plan) => {
            WireCollectionKernelPlan::CausalMaskedSoftmax(WireCausalMaskedSoftmaxPlan {
                rows: plan.rows,
                cols: plan.cols,
            })
        }
    }
}

/// Map a radix reduction operator onto its typed wire mirror (the shared
/// operator surface of the tree-reduction and axis-reduction recipes).
fn wire_reduce_op(op: radix_mir::kernel_plan::ReduceOp) -> WireReduceOp {
    match op {
        radix_mir::kernel_plan::ReduceOp::Sum => WireReduceOp::Sum,
        radix_mir::kernel_plan::ReduceOp::Mean => WireReduceOp::Mean,
    }
}

/// Map a carried [`MirRankExtensionBroadcast`] (the S6-C2 device-ABI fact)
/// onto its typed wire mirror: the operand/result shapes and the broadcast
/// declaration ride the plan verbatim — the fact is a program fact, never
/// dropped on the wire and never inferred (A3).
fn wire_broadcast_fact(fact: &MirRankExtensionBroadcast) -> WireBroadcastFact {
    WireBroadcastFact {
        lhs_shape: fact.lhs_shape.clone(),
        rhs_shape: fact.rhs_shape.clone(),
        result_shape: fact.result_shape.clone(),
        declaration: match fact.declaration {
            MirBroadcastDeclaration::RankExtension => WireBroadcastDeclaration::RankExtension,
        },
    }
}

fn wire_shared_layout(
    layout: &radix_mir::kernel_plan::SharedMemoryLayout,
) -> WireSharedMemoryLayout {
    WireSharedMemoryLayout {
        element_byte_width: u32::try_from(layout.element_byte_width).unwrap_or(u32::MAX),
        slot_count: layout.slot_count,
        buffer_name: layout.buffer_name.clone(),
    }
}

fn wire_oob_padding(padding: radix_mir::kernel_plan::OobPaddingPolicy) -> WireOobPaddingPolicy {
    match padding {
        radix_mir::kernel_plan::OobPaddingPolicy::ZeroFill => WireOobPaddingPolicy::ZeroFill,
    }
}

fn data_type_spelling(_ty: MirType) -> String {
    // The S1-1 device-program schema pins f32 as the campaign dtype (the
    // numeric policy admits only f32 and integer dtypes; the vertical-slice
    // kernel surface is f32). The host descriptor's typed element type is
    // always F32 for the Stage 1 slice; a future dtype widens this mapping
    // with the schema that carries it — never inferred from emitted text.
    "f32".to_owned()
}
