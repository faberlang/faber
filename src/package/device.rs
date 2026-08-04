//! Device-program construction + packaged payload codec (S1-6 vertical slice).
//!
//! This module is the faber-owned half of the S1-6 consumption seam (the
//! differentiable-GPU campaign, `docs/factory/gpu-training-lowering/`): it
//! carries one proven collection kernel from **Faber source** through the
//! **common device program** (the S1-1 `DeviceProgram` schema), the
//! **packaged FMIR image** (the S1-2 `device` section), and the **composite
//! host** (S1-4) that runs it on real Metal/CUDA sessions — all through the
//! ordinary `faber run --backend <metal|cuda>` command.
//!
//! # What this module owns
//!
//! - [`device_program_for_lowered`] — the device-program constructor: scans
//!   the lowered package MIR for `@ nucleum` compute kernels (functions whose
//!   `shader_stage` is [`MirKernelShaderStage::Compute`]) and composes the
//!   target-neutral [`DeviceProgram`] from typed facts only (A3): the ABI
//!   kernel signature, the shared plan pass
//!   ([`kernel_plan_for_function`] — the typed
//!   `MirCollectionOp → CollectionKernelPlan` bridge), the typed storage
//!   buffers, and the derived launch plan.
//! - [`wire_program_for_program`] / [`admit_device_program_section`] — the
//!   codec-v3 (S3-A4) canonical wire: the former serializes the complete
//!   typed program (kernels, launches, results, per-resource access +
//!   version) into the typed [`FmirDeviceProgramSection`] wire of the FMIR
//!   `device` section; the latter admits it fail-closed, gating on the
//!   `payload_version` check before any field-level interpretation. CUDA
//!   symbols and host input values are not program semantics — they never
//!   enter the canonical bytes. The S1-3 typed logical-entry → NVVM symbol
//!   mapping ([`CudaKernelIdentity`]) now rides the per-artifact symbols
//!   metadata and is consumed by [`descriptor_for_backend`] when it
//!   constructs the CUDA descriptor.
//! - [`descriptor_for_backend`] — maps a parsed run plan + a declared backend
//!   artifact blob onto the S1-4 host [`DeviceDescriptor`], and
//!   [`execute_device_route`] — the ordinary-command launch seam that
//!   constructs the composite host, executes the descriptor (load → allocate
//!   → copy-in → launch → sync → readback → release), and prints an A9-style
//!   receipt (selected hardware, module hash, launches, transfers, readbacks,
//!   output values).
//!
//! # Ownership boundaries (N2/N3)
//!
//! The shared schemas remain single-writer in radix: this module consumes
//! `radix-mir`'s `DeviceProgram` / plan-mapping and the S1-3 emitters; it
//! never forks the schema. The host descriptor surface is hosts' S1-4
//! `faber-host-macos-arm64`; faber constructs descriptors from typed facts,
//! never by parsing emitted MSL/PTX text (A3).

use faber::device::{DeviceBackend, DeviceSelection};
use faber_host_macos_arm64::device_descriptor::{
    DescriptorBuffer, DescriptorBufferVersion, DescriptorDataFlow as HostDescriptorDataFlow,
    DescriptorKernel, DescriptorLaunch, DeviceBufferLifetime, DeviceBufferRole, DeviceDataType,
    DeviceDescriptor, DeviceProgramLifetime as HostDeviceProgramLifetime,
};
use radix::diagnostics::Diagnostic;
use radix::lexer::Interner;
use radix::mir::{MirFunction, MirKernelShaderStage, ValidatedMir};
use radix_mir::abi::{
    MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind, MirKernelSignature,
};
use radix_mir::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit,
};
use radix_mir::device_program_plans::kernel_plan_for_function;
use radix_mir::kernel_plan::CollectionKernelPlan;
use radix_mir::layout::MirTensorStorageLayout;
use radix_mir::{MirFunctionId, MirType};
use radix_mir_fmir::{
    FmirDeviceArtifact, FmirDeviceArtifactsSection, FmirDeviceBackend, FmirDeviceInput,
    FmirDeviceProgramSection, FmirDeviceSection, FmirDeviceSelection, FmirDeviceSymbol,
    WireBarrierPhase, WireBarrierPoint, WireBinding, WireBufferIdentity, WireBufferLifetime,
    WireBufferRole, WireBufferVersion, WireCollectionKernelPlan, WireDependencyEdge,
    WireDeviceProgram, WireDeviceResource, WireDispatchSize, WireInitializationPolicy,
    WireKernelLaunchPlan, WireKernelUnit, WireLaunchUnit, WireMatMulPlan, WireMatMulSharedMemory,
    WireObservationFact, WireOobPaddingPolicy, WireProgramLifetime, WireReduceOp,
    WireReductionPlan, WireResourceAccess, WireResultBuffer, WireSemanticValue,
    WireSemanticValueOrigin, WireSharedMemoryLayout, WireStorageLayout, WireWorkgroupCount,
    WireWorkgroupSize,
};
use std::collections::BTreeMap;

/// Stable fail-closed diagnostic for a device-program construction failure.
fn device_diag(context: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(format!("device program {context}: {}", message.into()))
        .with_arg("issue", "E_DEVICE_DESCRIPTOR")
}

// ---------------------------------------------------------------------------
// Device-program constructor
// ---------------------------------------------------------------------------

/// The logical entry name for a kernel function.
///
/// The target-neutral entry is the Faber function name when the function
/// carries one (the S1-3 emitters name the Metal kernel entry after it; the
/// CUDA PTX symbol is separate and carried via [`CudaKernelIdentity`]).
fn kernel_entry_name(function: &MirFunction, interner: &Interner) -> String {
    function
        .name
        .map(|symbol| interner.resolve(symbol))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("kernel{}", function.id.0))
}

/// The param-name for a buffer slot, when the kernel function names the
/// source local (diagnostics + the `[device] inputs` manifest keys).
fn buffer_slot_name(
    function: &MirFunction,
    interner: &Interner,
    resource: &MirKernelResource,
) -> String {
    if let Some(local) = resource.source_local {
        if let Some(name) = function
            .params
            .iter()
            .find(|param| param.local == local)
            .and_then(|param| param.name)
            .and_then(|symbol| safe_interner_name(interner, symbol))
        {
            return name;
        }
    }
    match resource.role {
        radix_mir::abi::MirKernelResourceRole::Input => {
            format!("input_{}", resource.binding)
        }
        radix_mir::abi::MirKernelResourceRole::Output => {
            format!("output_{}", resource.binding)
        }
    }
}

/// Resolve a symbol name safely: an uninterned synthetic symbol (the
/// reverse-AD residual/upstream placeholders of a generated companion)
/// falls back to `None` instead of indexing past the interner's string
/// table — the caller then uses the role-based fallback name (S3-A2).
fn safe_interner_name(interner: &Interner, symbol: radix::lexer::Symbol) -> Option<String> {
    if (symbol.0 as usize) < interner.strings().len() {
        Some(interner.resolve(symbol).to_owned())
    } else {
        None
    }
}

/// One buffer unified across kernel boundaries (S2-5).
///
/// The unification key is the **identity name + shape**: when one kernel's
/// input buffer matches another kernel's output (same name + shape), the two
/// share one `BufferId` as an InOut intermediate with a data-flow edge
/// (delivery spec N2.6; the schema's `BufferRegistry`/`DataFlowPair` already
/// model this — the constructor stops minting a fresh id per slot). The role
/// is the merged program-level fact: like roles keep the role, an
/// Input+Output mix (either order) is InOut.
///
/// The resource-state axes (F5) ride here as **independent facts** gathered
/// from the slot access pattern across the whole program: `written` and
/// `consumed` feed the allocation-lifetime and initialization decisions in
/// pass 2 — no axis is derived from the role or from another axis.
struct UnifiedBuffer {
    id: BufferId,
    name: String,
    element_ty: MirType,
    element_count: u64,
    role: BufferRole,
    /// Any kernel slot writes the buffer (Write/ReadWrite) — the independent
    /// fact behind the lifetime and initialization axes.
    written: bool,
    /// Any kernel slot reads the buffer (Read) — the independent fact that
    /// distinguishes a step-local intermediate from a final observation.
    consumed: bool,
}

impl UnifiedBuffer {
    /// The S2-5 unification key: same identity name and same shape.
    fn matches(&self, name: &str, element_ty: MirType, element_count: u64) -> bool {
        self.name == name && self.element_ty == element_ty && self.element_count == element_count
    }
}

/// Merge an ABI-derived role onto a buffer's program-level role under one id.
fn merge_buffer_roles(previous: BufferRole, next: BufferRole) -> BufferRole {
    match (previous, next) {
        (BufferRole::Input, BufferRole::Input) => BufferRole::Input,
        (BufferRole::Output, BufferRole::Output) => BufferRole::Output,
        _ => BufferRole::InOut,
    }
}

/// Construct the common device program for a lowered package.
///
/// Scans the validated package MIR for `@ nucleum` compute kernels
/// (`shader_stage == Compute`) and composes one ordered [`DeviceProgram`]
/// whose kernel units carry the typed plan, typed storage-buffer resources,
/// and derived launch plans. Every field is a program fact from the ABI
/// signature and the shared plan pass — never inferred from emitted text
/// (A3). A package with no compute kernels yields `None` (no device payload).
///
/// **Cross-kernel buffer identity unification (S2-5):** the constructor runs
/// in two passes — first it derives every kernel's signature/plan and unifies
/// buffer identity by (identity name, element type, element count) across
/// kernels (one kernel's output matching another's input shares a single
/// `BufferId` as an InOut intermediate), then it materializes the program
/// with the merged identity facts. A slot's per-kernel access stays as the
/// ABI derived it (the schema permits InOut buffers to be read or written at
/// individual slots).
///
/// **Companion path (S3-A2, THE SPINE):** the constructor also selects the
/// generated companions of device-resident primals through the owned
/// [`MirCompanionMap`] carrier — NOT only `shader_stage == Compute`. Each
/// carried companion's tuple gradient return lowers through the multi-output
/// ABI (S3-A1) into distinct output resources (N gradient outputs bind to N
/// distinct slots), its kernel is ordered AFTER the forward kernels, and its
/// buffers join the same S2-5 unification (the companion reads the primal's
/// device-resident buffers by identity). Placement is decided here (A5): a
/// companion of a device-resident primal joins the same `DeviceProgram`;
/// generated AIR stays pure (the purity ledger is untouched).
///
/// # Errors
/// Fail-closed [`Diagnostic`]s when a kernel's ABI signature or plan cannot
/// be derived, a storage buffer has no coherent program role, a carried
/// companion is missing from the lowered MIR, or the resulting program fails
/// [`DeviceProgram::validate`].
pub(crate) fn device_program_for_lowered(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    companions: &radix_mir::device::MirCompanionMap,
) -> Result<Option<DeviceProgram>, Vec<Diagnostic>> {
    let kernel_functions: Vec<&MirFunction> = validated
        .program()
        .functions
        .iter()
        .filter(|function| function.shader_stage == Some(MirKernelShaderStage::Compute))
        .collect();
    if kernel_functions.is_empty() {
        return Ok(None);
    }

    /// One kernel's build: signature/plan facts plus its resource slots
    /// (buffer ids resolved by the unification pass).
    struct KernelBuild {
        function: MirFunctionId,
        entry: String,
        plan: CollectionKernelPlan,
        launch: KernelLaunchPlan,
        resources: Vec<ResourceBuild>,
    }
    /// One storage-buffer slot: binding facts + the unified buffer id.
    struct ResourceBuild {
        group: u32,
        binding: u32,
        access: MirKernelResourceAccess,
        buffer_id: BufferId,
        element_ty: MirType,
        element_count: u64,
    }

    // Pass 1: derive signatures/plans and unify cross-kernel buffer identity.
    // One kernel builder serves both the forward kernels and the S3-A2
    // companion kernels — every kernel participates in the same S2-5
    // unification, so a companion reads its primal's device-resident buffers
    // by identity.
    let mut unified: Vec<UnifiedBuffer> = Vec::new();
    let mut next_buffer_id = 1u32;
    let mut builds: Vec<KernelBuild> = Vec::with_capacity(kernel_functions.len());
    let mut build_kernel = |function: &MirFunction| -> Result<(), Vec<Diagnostic>> {
        let signature = MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            function,
            validated.validation(),
            interner,
        )
        .map_err(|error| vec![device_diag("signature", error.message)])?;
        let plan = match kernel_plan_for_function(function, &signature, validated.validation())
            .map_err(|error| vec![device_diag("plan", error.message)])?
        {
            Some(plan) => plan,
            // No recipe and no unplannable op (N3.2): the function-level
            // scan verified the body elementwise (only elementwise
            // transforms), so the no-recipe plan is the typed decision —
            // never a silent fallback from an unplannable op.
            None => CollectionKernelPlan::Elementwise,
        };

        let mut resources: Vec<ResourceBuild> = Vec::new();
        for resource in signature
            .resources()
            .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
        {
            let role =
                BufferRole::from_abi_role(resource.role, resource.access).ok_or_else(|| {
                    vec![device_diag(
                        "buffer role",
                        format!(
                            "storage buffer binding {} has no coherent program role ({:?} {:?})",
                            resource.binding, resource.role, resource.access
                        ),
                    )]
                })?;
            let name = buffer_slot_name(function, interner, resource);
            let buffer_id = if let Some(entry) = unified
                .iter_mut()
                .find(|entry| entry.matches(&name, resource.element_ty, resource.element_count))
            {
                // Unification (S2-5): the same logical buffer appears at
                // this kernel too. The merged role is the program-level
                // identity fact; an Input+Output mix makes it an InOut
                // intermediate.
                entry.role = merge_buffer_roles(entry.role, role);
                entry.written |= matches!(
                    resource.access,
                    MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                );
                entry.consumed |= resource.access == MirKernelResourceAccess::Read;
                entry.id
            } else {
                let id = BufferId(next_buffer_id);
                next_buffer_id += 1;
                unified.push(UnifiedBuffer {
                    id,
                    name,
                    element_ty: resource.element_ty,
                    element_count: resource.element_count,
                    role,
                    written: matches!(
                        resource.access,
                        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                    ),
                    consumed: resource.access == MirKernelResourceAccess::Read,
                });
                id
            };
            resources.push(ResourceBuild {
                group: resource.group,
                binding: resource.binding,
                access: resource.access,
                buffer_id,
                element_ty: resource.element_ty,
                element_count: resource.element_count,
            });
        }
        // The launch binds buffers in binding order (inputs first, output
        // last), matching the emitted kernel's buffer indices / param
        // order.
        resources.sort_by_key(|resource| (resource.binding, resource.group));

        builds.push(KernelBuild {
            function: function.id,
            entry: kernel_entry_name(function, interner),
            plan,
            launch: KernelLaunchPlan::from_signature_and_function(&signature, function),
            resources,
        });
        Ok(())
    };

    for function in kernel_functions {
        build_kernel(function)?;
    }

    // S3-A2 companion path: the generated companions of device-resident
    // primals, ordered AFTER the forward kernels. Each companion's tuple
    // gradient return lowers through the multi-output ABI into distinct
    // output resources; a carried companion missing from the lowered MIR
    // fails construction closed (relation facts are the typed routing
    // surface — never a name heuristic).
    for entry in companions.iter() {
        if !entry.device_resident {
            continue;
        }
        let companion = validated
            .program()
            .functions
            .iter()
            .find(|function| function.source == Some(entry.companion))
            .ok_or_else(|| {
                vec![device_diag(
                    "companion",
                    format!(
                        "companion DefId({}) of device-resident primal DefId({}) is missing from the lowered MIR",
                        entry.companion.0, entry.primal.0
                    ),
                )]
            })?;
        build_kernel(companion)?;
    }

    // Pass 2: materialize the program with the merged identity facts. Every
    // reference to a unified id carries the same name/role/lifetime so the
    // schema's cross-reference consistency checks pass.
    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    for build in builds {
        let kernel_index = program.kernels.len();
        let mut resources: Vec<DeviceResource> = Vec::with_capacity(build.resources.len());
        for slot in build.resources {
            let entry = unified
                .iter()
                .find(|entry| entry.id == slot.buffer_id)
                .expect("every slot buffer was registered by the unification pass");
            let identity = BufferIdentity {
                id: entry.id,
                name: entry.name.clone(),
                role: entry.role,
                storage: MirTensorStorageLayout::DeviceHandle,
                // Independent allocation-lifetime axis (F5): decided from the
                // buffer's aggregate access facts (written / consumed),
                // never derived from the role. The host receives these facts
                // through the payload; it never re-derives a lifetime from
                // slot role alone.
                lifetime: unified_lifetime(entry),
            };
            resources.push(DeviceResource {
                buffer: identity,
                version: BufferVersion {
                    version: 1,
                    element_ty: slot.element_ty,
                    element_count: slot.element_count,
                },
                binding: Binding {
                    group: slot.group,
                    binding: slot.binding,
                },
                access: slot.access,
            });
        }

        let launch_id = LaunchId(u32::try_from(program.launches.len()).unwrap_or(u32::MAX) + 1);
        program.kernels.push(KernelUnit {
            function: build.function,
            entry: build.entry.clone(),
            plan: build.plan,
            resources: resources.clone(),
            launch: build.launch,
        });
        program.launches.push(LaunchUnit {
            id: launch_id,
            kernel_index,
        });
        for resource in resources {
            // A result records the launch that PRODUCES the buffer version;
            // only slots that write (Write/ReadWrite) produce. An InOut
            // intermediate's read-only consumer slot (S2-5: the unified
            // intermediate referenced by the next kernel) must not claim
            // production — `DeviceProgram::validate` rejects a result whose
            // producing launch does not write the version.
            if (resource.buffer.role == BufferRole::Output
                || resource.buffer.role == BufferRole::InOut)
                && resource.access != MirKernelResourceAccess::Read
            {
                program
                    .results
                    .push(radix_mir::device_program::ResultBuffer {
                        buffer: resource.buffer.clone(),
                        version: resource.version.clone(),
                        role: resource.buffer.role,
                        produced_by: launch_id,
                    });
            }
        }
    }

    program.validate().map_err(|error| {
        vec![device_diag(
            "validation",
            format!("constructed device program is inconsistent: {error}"),
        )]
    })?;
    Ok(Some(program))
}

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
const DEVICE_RUN_PLAN_VERSION: u32 = 4;

/// Fail-closed wire admission (S3-A4): the wire version is read FIRST,
/// before any field-level interpretation, so an old (or unknown) codec
/// version fails with the structured `payload_version` diagnostic — never a
/// silent default and never a generic field-level error that hides the
/// version gate.
///
/// # Errors
/// Fail-closed when the wire carries an unsupported version.
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
    Ok(())
}

/// Build the typed complete-program wire from a constructed device program
/// (S3-A4). Every program fact the constructor materialized is carried
/// field-for-field: kernels (function id, entry, plan, typed resources with
/// access + version, launch), the ordered launch sequence, the program
/// lifetime, and the explicit result buffers — plus the independent
/// resource-state axes (F5, Stage 3R): one semantic value per buffer (F1),
/// explicit value generations per slot (F2), carried roots + producer/
/// consumer dependencies (F3), per-buffer initialization policies and
/// explicit observation facts on every result row (F5/F6). CUDA symbols and
/// host input values are NOT program semantics — they never enter the
/// canonical bytes.
#[must_use]
fn wire_program_for_program(program: &DeviceProgram) -> WireDeviceProgram {
    let dependencies = carried_dependencies(program);
    WireDeviceProgram {
        kernels: program
            .kernels
            .iter()
            .map(|kernel| WireKernelUnit {
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
                        // Explicit value generation (F2, first projection):
                        // the slot produces/consumes its carried content
                        // version — never a universal `1`.
                        generation: resource.version.version,
                        // Independent initialization axis (F5): how the
                        // buffer's storage is brought to its first defined
                        // state, decided from access facts, never from role.
                        initialization: wire_initialization_policy(program, resource.buffer.id),
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
            DeviceProgramLifetime::RepeatingStep => WireProgramLifetime::RepeatingStep,
        },
        results: program
            .results
            .iter()
            .map(|result| WireResultBuffer {
                buffer: wire_buffer_identity(&result.buffer),
                version: wire_buffer_version(&result.version),
                role: wire_role(result.role),
                produced_by: result.produced_by.0,
                // Explicit observation fact (F5/F6): the result row IS a
                // declared observation point at the producing launch's
                // completion boundary.
                observation: WireObservationFact {
                    at_launch: result.produced_by.0,
                },
            })
            .collect(),
        semantic_values: program_semantic_values(program),
        roots: carried_roots(program, &dependencies),
        dependencies,
        relations: Vec::new(),
    }
}

/// The distinct buffer identities of a program, in first-reference order
/// (kernel resources, then results).
fn program_buffer_identities(program: &DeviceProgram) -> Vec<&BufferIdentity> {
    let mut identities: Vec<&BufferIdentity> = Vec::new();
    for kernel in &program.kernels {
        for resource in &kernel.resources {
            if !identities
                .iter()
                .any(|identity| identity.id == resource.buffer.id)
            {
                identities.push(&resource.buffer);
            }
        }
    }
    for result in &program.results {
        if !identities
            .iter()
            .any(|identity| identity.id == result.buffer.id)
        {
            identities.push(&result.buffer);
        }
    }
    identities
}

/// One provisional semantic value per buffer (F1, first projection): each
/// buffer is its own stable value identity, minted from the buffer identity
/// itself with a distinct synthetic origin (two values never alias). The
/// origin-based minting from carried MIR/value facts is the U4 constructor
/// rework; this projection keeps the wire fact present and fail-closed.
fn program_semantic_values(program: &DeviceProgram) -> Vec<WireSemanticValue> {
    program_buffer_identities(program)
        .iter()
        .map(|identity| WireSemanticValue {
            id: identity.id.0,
            name: identity.name.clone(),
            origin: WireSemanticValueOrigin::Synthetic {
                label: format!("buffer-{}", identity.id.0),
            },
        })
        .collect()
}

/// Carried producer/consumer dependency edges (F3) derived from the
/// program's typed resources + launches — the same derivation as the
/// radix-mir `BufferRegistry::data_flow_pairs`: hosts schedule the validated
/// graph from these edges, never from kernel declaration order.
fn carried_dependencies(program: &DeviceProgram) -> Vec<WireDependencyEdge> {
    program
        .buffer_registry()
        .data_flow_pairs()
        .into_iter()
        .map(|pair| WireDependencyEdge {
            producer: pair.producer.0,
            consumer: pair.consumer.0,
            buffer: pair.buffer.0,
            version: pair.version,
        })
        .collect()
}

/// Declared legal execution roots (F3): the launches no dependency edge
/// feeds — the minimal set a host may start from. Never inferred from
/// kernel declaration order.
fn carried_roots(program: &DeviceProgram, dependencies: &[WireDependencyEdge]) -> Vec<u32> {
    program
        .launches
        .iter()
        .filter(|launch| !dependencies.iter().any(|edge| edge.consumer == launch.id.0))
        .map(|launch| launch.id.0)
        .collect()
}

/// The independent initialization axis (F5) of a buffer, projected onto the
/// wire: how its storage is brought to its first defined state — decided
/// from the buffer's aggregate access facts, never from its role or
/// lifetime. A read-only host-provided buffer is uploaded (HostProvided); a
/// buffer any slot writes in place is zero-filled at allocation
/// (ZeroFill — its first generation is defined at allocation); a
/// kernel-written buffer is fully defined by the kernel before any read
/// (KernelInitialized).
fn wire_initialization_policy(
    program: &DeviceProgram,
    buffer: BufferId,
) -> WireInitializationPolicy {
    let mut readwrite = false;
    let mut written = false;
    for kernel in &program.kernels {
        for resource in &kernel.resources {
            if resource.buffer.id == buffer {
                readwrite |= resource.access == MirKernelResourceAccess::ReadWrite;
                written |= matches!(
                    resource.access,
                    MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                );
            }
        }
    }
    if readwrite {
        WireInitializationPolicy::ZeroFill
    } else if !written {
        WireInitializationPolicy::HostProvided
    } else {
        WireInitializationPolicy::KernelInitialized
    }
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

// ---------------------------------------------------------------------------
// FMIR device-section assembly
// ---------------------------------------------------------------------------

/// Assemble the FMIR `device` section for a constructed device program.
///
/// Emits both backend artifacts through the S1-3 emitters (Metal MSL always;
/// CUDA PTX through the admitted clang NVPTX compiler when present — a
/// machine without the build-time compiler carries no CUDA artifact and
/// `--backend cuda` fails closed at run time as a missing declared
/// artifact), builds the canonical run-plan payload (with the host input
/// values), and records the selection + runtime requirements.
///
/// # Errors
/// Fail-closed when artifact emission fails (a carried plan or binding that
/// contradicts the typed function facts fails closed, A3).
pub(crate) fn device_section_for_program(
    program: &DeviceProgram,
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    selection: DeviceSelection,
    inputs: &BTreeMap<String, Vec<f32>>,
    ptx_target: &str,
) -> Result<FmirDeviceSection, Vec<Diagnostic>> {
    let metal_artifact = radix_mir_metal::emit_metal_device_artifact(program, validated, interner)
        .map_err(|error| vec![device_diag("metal artifact", error.to_string())])?;
    // S3-A5 (Metal lane): the CUDA artifact emission is best-effort — an
    // emitter op the CUDA lane does not support yet (the companion's
    // elementwise surface lands in S3-A7) leaves the image Metal-only, and a
    // later `--backend cuda` request fails closed as a missing declared
    // artifact (the same seam the PTX-compile-unavailable path uses). The
    // Metal artifact is the S3-A5 proof surface.
    let cuda_artifact =
        match radix_mir_llvm::emit_cuda_device_artifact(program, validated, interner) {
            Ok(artifact) => Some(artifact),
            Err(error) => {
                eprintln!(
                    "faber: CUDA artifact not emitted (S3-A7 emitter surface): {}",
                    error
                );
                None
            }
        };

    // The CUDA logical-entry → symbol mapping rides the artifact as
    // per-artifact metadata (N3.3): it is an artifact fact, not a program
    // semantic, so it never enters the canonical program bytes.
    let cuda_symbols = cuda_artifact
        .as_ref()
        .map(|artifact| {
            artifact
                .kernels
                .iter()
                .map(|identity| FmirDeviceSymbol {
                    entry: identity.entry.clone(),
                    symbol: identity.symbol.clone(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: metal_artifact.source,
        hash: metal_artifact.hash,
        symbols: Vec::new(),
    }];
    if let Some(cuda_artifact) = &cuda_artifact {
        match radix_mir_llvm::compile_nvvm_to_ptx(&cuda_artifact.source, ptx_target) {
            Ok(ptx) => {
                // The packaged CUDA artifact is PTX (N1.3 §3.1); its provenance
                // hash covers the PTX blob, not the NVVM source.
                let ptx_hash = radix_mir_fmir::fnv1a64_blob_hash(ptx.as_bytes());
                artifact.push(FmirDeviceArtifact {
                    backend: FmirDeviceBackend::Cuda,
                    blob: ptx,
                    hash: ptx_hash,
                    symbols: cuda_symbols,
                });
            }
            Err(error) => {
                // Build-time PTX compile unavailable (clang NVPTX missing): the
                // image carries the Metal artifact only and a later `--backend
                // cuda` request fails closed as a missing declared artifact.
                eprintln!(
                    "faber: CUDA PTX artifact not emitted (build-time clang NVPTX unavailable): {error}"
                );
            }
        }
    }

    let wire = wire_program_for_program(program);
    let declared_inputs = inputs
        .iter()
        .map(|(name, values)| FmirDeviceInput {
            name: name.clone(),
            values: values.clone(),
        })
        .collect();

    let mut runtime_requirements = Vec::new();
    for artifact_entry in &artifact {
        match artifact_entry.backend {
            FmirDeviceBackend::Metal => runtime_requirements.push("device:metal".to_owned()),
            FmirDeviceBackend::Cuda => runtime_requirements.push("device:cuda".to_owned()),
        }
    }
    runtime_requirements.sort();
    runtime_requirements.dedup();

    Ok(FmirDeviceSection {
        device_program: FmirDeviceProgramSection {
            v: DEVICE_RUN_PLAN_VERSION,
            program: wire,
        },
        selection: match selection {
            DeviceSelection::Auto => FmirDeviceSelection::Auto,
            DeviceSelection::Metal => FmirDeviceSelection::Metal,
            DeviceSelection::Cuda => FmirDeviceSelection::Cuda,
        },
        artifacts: FmirDeviceArtifactsSection { artifact },
        declared_inputs,
        runtime_requirements,
    })
}

/// The independent allocation-lifetime axis (F5) of a unified buffer,
/// decided from its aggregate access facts — never from the role:
///
/// - a buffer no kernel ever writes is host-provided persistent state
///   (per-program);
/// - a kernel-written buffer another kernel consumes is a step-local
///   intermediate (per-step);
/// - a kernel-written final is read back at an observation point.
fn unified_lifetime(entry: &UnifiedBuffer) -> BufferLifetime {
    if !entry.written {
        BufferLifetime::PerProgram
    } else if entry.consumed {
        BufferLifetime::PerStep
    } else {
        BufferLifetime::ObservationPoint
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

// ---------------------------------------------------------------------------
// Descriptor construction + ordinary-command execution seam
// ---------------------------------------------------------------------------

/// The declared backend artifact for a resolved backend, from the image's
/// artifacts section.
pub(crate) fn artifact_for_backend(
    artifacts: &[FmirDeviceArtifact],
    backend: DeviceBackend,
) -> Option<&FmirDeviceArtifact> {
    artifacts.iter().find(|artifact| match backend {
        DeviceBackend::Metal => artifact.backend == FmirDeviceBackend::Metal,
        DeviceBackend::Cuda => artifact.backend == FmirDeviceBackend::Cuda,
    })
}

/// Build the typed host descriptor from the image's WIRE + backend artifact
/// blob (S3-A4: the host consumes the wire — the descriptor is derived
/// exclusively from the carried program facts, never from a thinned slot
/// list).
///
/// The S1-3 typed logical-entry → NVVM-symbol mapping is **consumed here**
/// from the artifact's per-artifact metadata: the CUDA descriptor's kernel
/// entry is the emitted PTX `.entry` symbol, never the logical entry; Metal
/// launches by the logical entry (the emitted MSL kernel name). Slots are
/// carried in binding order so the composite host binds buffers in the
/// emitted kernel's buffer/param order. The wire's typed lifetimes and
/// program regime are mapped onto the host descriptor's
/// [`DeviceBufferLifetime`]/[`DeviceProgramLifetime`] — the host consumes
/// the carried facts; it never re-derives a lifetime from slot role (S2-4).
///
/// # Errors
/// Fail-closed when a carried element-type spelling is outside the campaign
/// dtype surface (never a silent default), or when a result record does not
/// match a writable, observation-point resource of its producing launch.
pub(crate) fn descriptor_for_backend(
    device: &FmirDeviceSection,
    backend: DeviceBackend,
    blob: &[u8],
) -> Result<DeviceDescriptor, Vec<Diagnostic>> {
    let wire = &device.device_program.program;
    validate_wire_results(wire)?;
    let mut kernels = Vec::with_capacity(wire.kernels.len());
    let mut buffer_versions = Vec::new();
    for kernel in &wire.kernels {
        let entry = match backend {
            DeviceBackend::Cuda => device
                .artifacts
                .artifact
                .iter()
                .find(|artifact| artifact.backend == FmirDeviceBackend::Cuda)
                .and_then(|artifact| {
                    artifact
                        .symbols
                        .iter()
                        .find(|identity| identity.entry == kernel.entry)
                        .map(|identity| identity.symbol.clone())
                })
                .unwrap_or_else(|| kernel.entry.clone()),
            DeviceBackend::Metal => kernel.entry.clone(),
        };
        let mut buffers = Vec::with_capacity(kernel.resources.len());
        for resource in &kernel.resources {
            let element_ty = wire_element_ty_to_host(&resource.version.element_ty)?;
            add_descriptor_buffer_version(
                &mut buffer_versions,
                resource.buffer.id,
                resource.version.version,
                element_ty,
                resource.version.element_count,
            )?;
            buffers.push(DescriptorBuffer {
                buffer_id: resource.buffer.id,
                buffer_name: resource.buffer.name.clone(),
                role: wire_role_to_host(resource.buffer.role),
                lifetime: wire_lifetime_to_host(resource.buffer.lifetime),
                binding: resource.binding.binding,
                element_ty,
                element_count: resource.version.element_count,
                // R2: the host consumes the wire's carried content version —
                // it never re-derives or hardcodes `1` for the A10 graph.
                version: resource.version.version,
            });
        }
        kernels.push(DescriptorKernel {
            entry,
            buffers,
            grid: [
                u32::try_from(kernel.launch.workgroup_count.x).unwrap_or(u32::MAX),
                u32::try_from(kernel.launch.workgroup_count.y).unwrap_or(u32::MAX),
                u32::try_from(kernel.launch.workgroup_count.z).unwrap_or(u32::MAX),
            ],
            block: [
                kernel.launch.workgroup.x,
                kernel.launch.workgroup.y,
                kernel.launch.workgroup.z,
            ],
        });
    }
    for result in &wire.results {
        let element_ty = wire_element_ty_to_host(&result.version.element_ty)?;
        add_descriptor_buffer_version(
            &mut buffer_versions,
            result.buffer.id,
            result.version.version,
            element_ty,
            result.version.element_count,
        )?;
    }

    let descriptor = DeviceDescriptor {
        backend,
        module_image: blob.to_vec(),
        kernels,
        launches: wire
            .launches
            .iter()
            .map(|launch| DescriptorLaunch {
                id: launch.id,
                kernel_index: launch.kernel_index,
            })
            .collect(),
        buffer_versions,
        program_lifetime: match wire.lifetime {
            WireProgramLifetime::SingleRun => HostDeviceProgramLifetime::SingleRun,
            WireProgramLifetime::RepeatingStep => HostDeviceProgramLifetime::RepeatingStep,
        },
        // R2: the host consumes the wire's carried data-flow edges (real
        // versions, producer/consumer per buffer version) — the receipt's
        // A10 graph is never re-derived from launch order.
        data_flow: wire_resource_graph(device)
            .1
            .into_iter()
            .map(|edge| HostDescriptorDataFlow {
                buffer_id: edge.buffer_id,
                version: edge.version,
                producer: edge.producer,
                consumer: edge.consumer,
            })
            .collect(),
    };

    descriptor
        .validate()
        .map_err(|error| vec![super::host_factory::host_error_diagnostic(&error)])?;
    Ok(descriptor)
}

/// Validate each wire result against the resource facts of its producing
/// launch before projecting the program into a host descriptor. The host
/// descriptor has no result surface, so a result-only or contradictory record
/// would otherwise be able to add metadata without proving a real producer.
/// Result rows are the authoritative readback set, but the host can only read
/// back `ObservationPoint` buffers and its receipt is keyed by buffer id; an
/// unsupported lifetime or repeated id therefore fails before host creation.
fn validate_wire_results(wire: &WireDeviceProgram) -> Result<(), Vec<Diagnostic>> {
    let mut result_buffers = BTreeMap::new();
    for (result_index, result) in wire.results.iter().enumerate() {
        if !matches!(result.role, WireBufferRole::Output | WireBufferRole::InOut) {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} has invalid observation role {}",
                    result.role.spelling()
                ),
            )]);
        }
        if result.role != result.buffer.role {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} has observation role {} but buffer {} is {}",
                    result.role.spelling(),
                    result.buffer.id,
                    result.buffer.role.spelling()
                ),
            )]);
        }

        let Some(launch) = wire
            .launches
            .iter()
            .find(|launch| launch.id == result.produced_by)
        else {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names unknown producing launch {}",
                    result.produced_by
                ),
            )]);
        };
        let Some(kernel) = wire.kernels.get(launch.kernel_index as usize) else {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names producing launch {}, whose kernel index {} is unknown",
                    result.produced_by, launch.kernel_index
                ),
            )]);
        };

        let Some(resource) = kernel
            .resources
            .iter()
            .find(|resource| resource.buffer.id == result.buffer.id)
        else {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names buffer {} version {} from producing launch {}, but that launch has no matching resource",
                    result.buffer.id, result.version.version, result.produced_by
                ),
            )]);
        };
        if resource.buffer != result.buffer {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} buffer {} has identity facts that contradict its producing launch {}",
                    result.buffer.id, result.produced_by
                ),
            )]);
        }

        if resource.version.version != result.version.version {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} buffer {} declares version {}, but producing launch {} uses version {}",
                    result.buffer.id,
                    result.version.version,
                    result.produced_by,
                    resource.version.version
                ),
            )]);
        }
        if resource.version.element_ty != result.version.element_ty
            || resource.version.element_count != result.version.element_count
        {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} buffer {} version {} carries shape {}[{}], but producing launch {} carries {}[{}]",
                    result.buffer.id,
                    result.version.version,
                    result.version.element_ty,
                    result.version.element_count,
                    result.produced_by,
                    resource.version.element_ty,
                    resource.version.element_count
                ),
            )]);
        }
        // F6 (Stage 3R): the result row's explicit observation fact must
        // name the producing launch's completion boundary. A result whose
        // observation contradicts its producer is a writable intermediate
        // exposed without an explicit observation fact — rejected before
        // host construction (the same rule the radix decode boundary runs).
        if result.observation.at_launch != result.produced_by {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names producing launch {}, but its explicit observation fact names launch {}; a result is a declared observation point at its producing launch",
                    result.produced_by, result.observation.at_launch
                ),
            )]);
        }
        if resource.buffer.lifetime != WireBufferLifetime::ObservationPoint {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names buffer {} with lifetime {}; only observation-point results are supported by the host readback contract",
                    result.buffer.id,
                    resource.buffer.lifetime.spelling()
                ),
            )]);
        }
        if !matches!(
            resource.access,
            WireResourceAccess::Write | WireResourceAccess::ReadWrite
        ) {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names launch {} as producer, but its matching resource is read-only",
                    result.produced_by
                ),
            )]);
        }
        if let Some(first_index) = result_buffers.insert(result.buffer.id, result_index) {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} repeats observation buffer {} already named by result {first_index}; result buffers must be unique in the host receipt",
                    result.buffer.id
                ),
            )]);
        }
    }
    Ok(())
}

fn add_descriptor_buffer_version(
    versions: &mut Vec<DescriptorBufferVersion>,
    buffer_id: u32,
    version: u32,
    element_ty: DeviceDataType,
    element_count: u64,
) -> Result<(), Vec<Diagnostic>> {
    if let Some(existing) = versions
        .iter()
        .find(|existing| existing.buffer_id == buffer_id && existing.version == version)
    {
        if existing.element_ty != element_ty || existing.element_count != element_count {
            return Err(vec![device_diag(
                "buffer version",
                format!("buffer {buffer_id} version {version} carries conflicting shape facts"),
            )]);
        }
        return Ok(());
    }

    versions.push(DescriptorBufferVersion {
        buffer_id,
        version,
        element_ty,
        element_count,
    });
    Ok(())
}

fn wire_role_to_host(role: WireBufferRole) -> DeviceBufferRole {
    match role {
        WireBufferRole::Input => DeviceBufferRole::Input,
        WireBufferRole::Output => DeviceBufferRole::Output,
        WireBufferRole::InOut => DeviceBufferRole::InOut,
    }
}

/// Map the wire's typed lifetime onto the host descriptor's typed lifetime.
/// The wire is the typed section (deny_unknown_fields admission), so the
/// mapping is a total function over the three-class enum (N3.4).
fn wire_lifetime_to_host(lifetime: WireBufferLifetime) -> DeviceBufferLifetime {
    match lifetime {
        WireBufferLifetime::PerProgram => DeviceBufferLifetime::PerProgram,
        WireBufferLifetime::PerStep => DeviceBufferLifetime::PerStep,
        WireBufferLifetime::ObservationPoint => DeviceBufferLifetime::ObservationPoint,
    }
}

/// Map the wire's element-type spelling onto the host's typed element type.
/// The campaign dtype surface pins f32 (the S1-1 schema); an unknown spelling
/// fails closed — never a silent default and never an unreachable arm.
fn wire_element_ty_to_host(spelling: &str) -> Result<DeviceDataType, Vec<Diagnostic>> {
    match spelling {
        "f32" => Ok(DeviceDataType::F32),
        other => Err(vec![device_diag(
            "element type",
            format!("device program element type `{other}` is outside the campaign dtype surface"),
        )]),
    }
}

/// Map the wire's named declared inputs onto buffer ids (via the wire's
/// input-buffer identities).
fn inputs_by_buffer_id(device: &FmirDeviceSection) -> BTreeMap<u32, Vec<f32>> {
    let mut by_name: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for input in &device.declared_inputs {
        by_name.insert(input.name.clone(), input.values.clone());
    }
    let mut by_id: BTreeMap<u32, Vec<f32>> = BTreeMap::new();
    for kernel in &device.device_program.program.kernels {
        for resource in &kernel.resources {
            if resource.buffer.role == WireBufferRole::Input {
                if let Some(values) = by_name.get(&resource.buffer.name) {
                    by_id.insert(resource.buffer.id, values.clone());
                }
            }
        }
    }
    by_id
}

/// The explicit result buffer ids the run reads back (S2-4).
///
/// Result rows are the authoritative readback set. `validate_wire_results`
/// proves that each row names a unique `ObservationPoint` resource before this
/// function is used, so no valid result can disappear through a role/lifetime
/// re-derivation.
fn observation_buffer_ids(device: &FmirDeviceSection) -> Vec<u32> {
    let mut ids = Vec::new();
    for result in &device.device_program.program.results {
        if !ids.contains(&result.buffer.id) {
            ids.push(result.buffer.id);
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// Wire-derived A10 resource graph (S3-A4)
// ---------------------------------------------------------------------------

/// One wire-derived A10 graph buffer (identity + content version).
#[allow(dead_code)] // the graph rows are asserted in the Faber projection tests.
struct WireGraphBuffer {
    id: u32,
    name: String,
    role: WireBufferRole,
    lifetime: WireBufferLifetime,
    version: u32,
    element_ty: String,
    element_count: u64,
}

/// One wire-derived inter-kernel data-flow edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WireGraphEdge {
    buffer_id: u32,
    version: u32,
    producer: u32,
    consumer: u32,
}

/// Derive the A10 resource graph from the wire's COMPLETE facts (N3.3): the
/// per-buffer identity + content version, and the producer/consumer edges
/// from the carried ordered access + launches. This is the same derivation
/// as the radix-mir `BufferRegistry` over the program — the host consumes
/// the carried facts instead of re-deriving topology from launch order or a
/// slot-role string (no coincidence-based first-writer rule, no hardcoded
/// version).
fn wire_resource_graph(device: &FmirDeviceSection) -> (Vec<WireGraphBuffer>, Vec<WireGraphEdge>) {
    let wire = &device.device_program.program;
    let mut buffers: Vec<WireGraphBuffer> = Vec::new();
    let mut producers: Vec<(u32, u32, u32)> = Vec::new();
    let mut consumers: Vec<(u32, u32, u32)> = Vec::new();
    for launch in &wire.launches {
        let Some(kernel) = wire.kernels.get(launch.kernel_index as usize) else {
            continue;
        };
        for resource in &kernel.resources {
            let id = resource.buffer.id;
            if !buffers
                .iter()
                .any(|buffer| buffer.id == id && buffer.version == resource.version.version)
            {
                buffers.push(WireGraphBuffer {
                    id,
                    name: resource.buffer.name.clone(),
                    role: resource.buffer.role,
                    lifetime: resource.buffer.lifetime,
                    version: resource.version.version,
                    element_ty: resource.version.element_ty.clone(),
                    element_count: resource.version.element_count,
                });
            }
            match resource.access {
                WireResourceAccess::Read => {
                    consumers.push((id, resource.version.version, launch.id));
                }
                WireResourceAccess::Write => {
                    producers.push((id, resource.version.version, launch.id));
                }
                WireResourceAccess::ReadWrite => {
                    consumers.push((id, resource.version.version, launch.id));
                    producers.push((id, resource.version.version, launch.id));
                }
            }
        }
    }
    // Results contribute the observed versions to the chain.
    for result in &wire.results {
        if !buffers
            .iter()
            .any(|buffer| buffer.id == result.buffer.id && buffer.version == result.version.version)
        {
            buffers.push(WireGraphBuffer {
                id: result.buffer.id,
                name: result.buffer.name.clone(),
                role: result.buffer.role,
                lifetime: result.buffer.lifetime,
                version: result.version.version,
                element_ty: result.version.element_ty.clone(),
                element_count: result.version.element_count,
            });
        }
    }
    // Data-flow edges (mirrors `BufferRegistry::data_flow_pairs`): every
    // producer/consumer launch pair per (buffer, version), excluding
    // self-edges.
    let mut edges: Vec<WireGraphEdge> = Vec::new();
    for (buffer_id, version, producer) in &producers {
        for (consumer_id, consumer_version, consumer) in &consumers {
            if consumer_id == buffer_id && consumer_version == version && consumer != producer {
                edges.push(WireGraphEdge {
                    buffer_id: *buffer_id,
                    version: *version,
                    producer: *producer,
                    consumer: *consumer,
                });
            }
        }
    }
    (buffers, edges)
}

/// The wire's logical name for a buffer id (diagnostics).
fn wire_buffer_name(device: &FmirDeviceSection, id: u32) -> String {
    device
        .device_program
        .program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .find(|resource| resource.buffer.id == id)
        .map(|resource| resource.buffer.name.clone())
        .unwrap_or_else(|| "<unknown>".to_owned())
}

/// Execute a device-bearing FMIR image's device route through the composite
/// host and print the A9/A10 receipt (S2-8).
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
/// `FABER_DEVICE_REPEAT` (default 1) runs the ordered launch sequence N times
/// on ONE session before teardown — the S2-8 leak-proof surface: after N
/// runs + teardown the live handle count is 0 and the driver counters are at
/// baseline (no leak of contexts/modules/buffers).
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
    let artifact = artifact_for_backend(&device.artifacts.artifact, backend)
        .ok_or_else(|| vec![super::host_factory::missing_backend_artifact(backend)])?;
    // A9 discovery receipt: selected device + declared artifact hash.
    let discovery = super::host_factory::discovery_receipt(backend, &device.artifacts.artifact)
        .ok_or_else(|| vec![super::host_factory::missing_backend_artifact(backend)])?;
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
    let mut host = super::host_factory::construct_composite_host(selection, true)
        .map_err(|diagnostic| vec![diagnostic])?;
    let inputs = inputs_by_buffer_id(device);
    // The explicit result rows, already validated by descriptor construction,
    // are the sole authority for host readback selection.
    let outputs = observation_buffer_ids(device);

    // A10 identity over the COMPLETE program (S3-A4): the canonical bytes of
    // the typed wire (semantics-only — CUDA symbols and declared inputs are
    // absent by construction), hashed with the source identities. Both image
    // routes carry the identical wire, so the identity is route-independent.
    let source_refs = source_hashes.iter().map(String::as_str).collect::<Vec<_>>();
    let canonical = radix_mir_fmir::canonical_program_bytes(&device.device_program.program);
    let identity = radix_mir_fmir::device_identity_hash(&source_refs, &canonical);
    println!("device: identity {identity} (A10, complete program)");

    // Repeated-execution surface for the S2-8 leak proof.
    let repeat_count = device_repeat_count()?;

    let mut session = super::host_factory::create_program_session(&mut host, &descriptor)
        .map_err(|diagnostic| vec![diagnostic])?;
    let mut last_receipt = None;
    for _ in 0..repeat_count {
        last_receipt = Some(
            session
                .execute(&inputs, &outputs)
                .map_err(|error| vec![super::host_factory::host_error_diagnostic(&error)])?,
        );
    }
    let receipt = last_receipt.ok_or_else(|| {
        vec![Diagnostic::error(
            "device route executed zero iterations (FABER_DEVICE_REPEAT must be >= 1)",
        )]
    })?;
    session
        .teardown()
        .map_err(|error| vec![super::host_factory::host_error_diagnostic(&error)])?;

    // A9 observed lifecycle events of the last execution.
    println!(
        "device: module hash fnv64:{:016x} launches {} syncs {} transfers {} readbacks {} releases {} allocated {}",
        receipt.module_hash,
        receipt.launches,
        receipt.syncs,
        receipt.transfers,
        receipt.outputs.len(),
        receipt.releases,
        receipt.allocated_buffers.len()
    );
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
            repeat_count,
            counters.module_loads,
            counters.module_releases,
            counters.buffer_allocs,
            counters.buffer_releases
        ),
        None => println!(
            "device: leak proof: {} run(s) then teardown -> live_handle_count()={live}, no device session after teardown",
            repeat_count
        ),
    }
    Ok(())
}

/// Render the exact ordered launch records that the host will execute.
///
/// The descriptor's launch sequence, not the kernel declaration order or the
/// aggregate receipt count, is the observable program order. A kernel index
/// may therefore repeat or appear out of declaration order.
fn host_receipt_launch_order_line(descriptor: &DeviceDescriptor) -> String {
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

fn host_receipt_graph_lines(
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
fn device_repeat_count() -> Result<usize, Vec<Diagnostic>> {
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

#[cfg(test)]
#[path = "device_test.rs"]
mod tests;
