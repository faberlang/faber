// Sibling + root items: explicit `use super` lists carry the seams the mir/
// split routes through `use super::*` (wildcard imports are denied).
use super::{
    BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, CollectionKernelPlan,
    DeviceProgram, DeviceProgramLifetime, DeviceResource, DeviceSemantics, Diagnostic,
    FmirDeviceProgramSection, InitializationPolicy, LaunchId, LosslessMirCompanionEntry,
    MirCompanionDerivativeKind, MirKernelResourceAccess, MirTensorStorageLayout, MirType,
    SemanticValueOrigin, WireBarrierPhase, WireBarrierPoint, WireBinding, WireBufferIdentity,
    WireBufferLifetime, WireBufferRole, WireBufferVersion, WireCollectionKernelPlan,
    WireCompanionDerivativeKind, WireCompanionRelation, WireCompanionSelectedInput,
    WireCompanionSelectedOutput, WireDependencyEdge, WireDeviceProgram, WireDeviceResource,
    WireDispatchSize, WireInitializationPolicy, WireKernelLaunchPlan, WireKernelUnit,
    WireLaunchUnit, WireMatMulPlan, WireMatMulSharedMemory, WireObservationCadence,
    WireObservationFact, WireOobPaddingPolicy, WireProgramLifetime, WireReduceOp,
    WireReductionPlan, WireResourceAccess, WireResultBuffer, WireSemanticValue,
    WireSemanticValueOrigin, WireSharedMemoryLayout, WireStorageLayout, WireTransposePlan,
    WireWorkgroupCount, WireWorkgroupSize,
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
pub(crate) const DEVICE_RUN_PLAN_VERSION: u32 = 6;

/// Fail-closed wire admission (S3-A4): the wire version is read FIRST,
/// before any field-level interpretation, so an old (or unknown) codec
/// version fails with the structured `payload_version` diagnostic — never a
/// silent default and never a generic field-level error that hides the
/// version gate. After the version gate, the S5-U5b declared step count is
/// admitted: a `RepeatingStep` program must declare at least one step (a
/// count the route could never drive fails closed here and at the radix
/// decode boundary).
///
/// # Errors
/// Fail-closed when the wire carries an unsupported version, or a
/// `RepeatingStep` program declares a zero step count.
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
    Ok(())
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
#[must_use]
pub(crate) fn wire_program_for_program(
    program: &DeviceProgram,
    semantics: &DeviceSemantics,
    repeating_steps: u32,
) -> WireDeviceProgram {
    WireDeviceProgram {
        kernels: program
            .kernels
            .iter()
            .enumerate()
            .map(|(kernel_index, kernel)| WireKernelUnit {
                function: kernel.function.0,
                entry: kernel.entry.clone(),
                plan: wire_plan(&kernel.plan),
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
/// complete plan is a program fact — never dropped on the wire).
fn wire_plan(plan: &CollectionKernelPlan) -> WireCollectionKernelPlan {
    match plan {
        CollectionKernelPlan::Elementwise => WireCollectionKernelPlan::Elementwise,
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
                op: match reduction.op {
                    radix_mir::kernel_plan::ReduceOp::Sum => WireReduceOp::Sum,
                    radix_mir::kernel_plan::ReduceOp::Mean => WireReduceOp::Mean,
                },
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
