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
//! - [`DeviceRunPlan`] + [`encode_payload`]/[`parse_payload`] — the
//!   producer-owned canonical payload the FMIR `device_program.payload`
//!   field carries (N1.7 §7.1): the typed per-kernel launch-descriptor facts
//!   plus the host input values for the input buffers. The serialization is
//!   deterministic (named-field JSON), so Metal and CUDA routes derive
//!   identical semantic bytes (A10). The S1-3 typed logical-entry → NVVM
//!   symbol mapping ([`CudaKernelIdentity`]) is carried in the payload and
//!   consumed by the host when it constructs the CUDA descriptor.
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
    DescriptorBuffer, DescriptorKernel, DeviceBufferLifetime, DeviceDataType, DeviceDescriptor,
    DeviceBufferRole, DeviceProgramLifetime as HostDeviceProgramLifetime,
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
    FmirDeviceArtifact, FmirDeviceArtifactsSection, FmirDeviceBackend, FmirDeviceProgramSection,
    FmirDeviceSection, FmirDeviceSelection,
};
use serde::{Deserialize, Serialize};
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
struct UnifiedBuffer {
    id: BufferId,
    name: String,
    element_ty: MirType,
    element_count: u64,
    role: BufferRole,
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
    let mut build_kernel =
        |function: &MirFunction| -> Result<(), Vec<Diagnostic>> {
            let signature =
                MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
                    function, validated.validation(), interner,
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
                let role = BufferRole::from_abi_role(resource.role, resource.access)
                    .ok_or_else(|| {
                        vec![device_diag(
                            "buffer role",
                            format!(
                                "storage buffer binding {} has no coherent program role ({:?} {:?})",
                                resource.binding, resource.role, resource.access
                            ),
                        )]
                    })?;
                let name = buffer_slot_name(function, interner, &resource);
                let buffer_id = if let Some(entry) = unified.iter_mut().find(|entry| {
                    entry.matches(&name, resource.element_ty, resource.element_count)
                }) {
                    // Unification (S2-5): the same logical buffer appears at
                    // this kernel too. The merged role is the program-level
                    // identity fact; an Input+Output mix makes it an InOut
                    // intermediate.
                    entry.role = merge_buffer_roles(entry.role, role);
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
                // S2-4 lifetime derivation (council 3): the constructor
                // derives the typed lifetime from the program's ABI facts —
                // default mapping Input → PerProgram, Output →
                // ObservationPoint, InOut → PerStep (delivery spec N2.4).
                // The host receives these facts through the payload; it never
                // re-derives a lifetime from slot role alone.
                lifetime: lifetime_for_role(entry.role),
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
                program.results.push(radix_mir::device_program::ResultBuffer {
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
// Packaged payload codec (producer-owned canonical form)
// ---------------------------------------------------------------------------

/// Canonical payload version. Bump only with a documented representation
/// change (the payload is the identity substrate of the same-package hash).
///
/// **v1 → v2 (S2-4, the faber-owned codec bump):** `PlanSlot` gained a typed
/// `lifetime` field and `DeviceRunPlan` gained the program `lifetime` regime
/// — the constructor-derived [`BufferLifetime`] facts ride the payload so the
/// host receives them from the packaged image (it never re-derives them from
/// slot role). Admission is fail-closed: `parse_payload` checks the `v`
/// field before any field-level parse, so old v1 payloads fail with the
/// structured `payload_version` diagnostic. This is a **faber-owned codec
/// change, not an A7/FMIR bump** — `PACKAGE_MIR_ARTIFACT_VERSION` stays 4
/// (N2.8; S2-9 forbids radix schema changes).
const DEVICE_RUN_PLAN_VERSION: u32 = 2;

/// Typed buffer-lifetime spelling carried by the canonical payload (codec v2,
/// S2-4). Mirrors the radix [`BufferLifetime`] and hosts
/// [`DeviceBufferLifetime`] spellings; an unknown spelling fails the payload
/// admission closed (never a silent default).
///
/// [`BufferLifetime`]: radix_mir::device_program::BufferLifetime
/// [`DeviceBufferLifetime`]: faber_host_macos_arm64::device_descriptor::DeviceBufferLifetime
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PlanSlotLifetime {
    /// Allocated once for the whole program; persists across executions.
    PerProgram,
    /// Live within one step; recycled at the step boundary.
    PerStep,
    /// Read back at a declared observation point; read-then-release.
    ObservationPoint,
}

/// Program execution-lifetime regime carried by the canonical payload (codec
/// v2, S2-4). `SingleRun` is the Stage 2 fixture regime; `RepeatingStep` is
/// the repeating training-step regime whose per-step recycling semantics land
/// with Stage 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PlanProgramLifetime {
    /// One-shot program run.
    #[default]
    SingleRun,
    /// Repeating training step; per-step buffers recycle between iterations.
    RepeatingStep,
}

/// One typed slot of a plan kernel (mirrors the S1-4 [`DescriptorBuffer`]
/// facts; produced from the S1-1 [`DeviceResource`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PlanSlot {
    /// Program-level buffer identity key.
    pub id: u32,
    /// Logical buffer name (diagnostics + input mapping).
    pub name: String,
    /// Slot role (`"input"` | `"output"` | `"in-out"`).
    pub role: String,
    /// Typed buffer lifetime (codec v2; derived by the constructor from the
    /// ABI facts, consumed by the host session's allocation/release policy).
    pub lifetime: PlanSlotLifetime,
    /// Target-neutral binding index.
    pub binding: u32,
    /// Element type spelling (`"f32"`).
    pub ty: String,
    /// Element count of this version's shape.
    pub count: u64,
}

/// One ordered kernel of the run plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PlanKernel {
    /// Target-neutral logical entry.
    pub entry: String,
    /// Typed buffer slots in binding order.
    pub slots: Vec<PlanSlot>,
    /// 3D workgroup-count grid the host launches.
    pub grid: [u32; 3],
    /// 3D workgroup (block) shape per axis.
    pub block: [u32; 3],
}

/// One CUDA logical-entry → NVVM-symbol identity (S1-3 [`CudaKernelIdentity`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PlanCudaKernel {
    /// Logical entry (a [`DeviceProgram`] kernel-unit fact).
    pub entry: String,
    /// Emitted NVVM/PTX `.entry` symbol the host launches by.
    pub symbol: String,
}

/// One host input for an input buffer (by buffer name).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PlanInput {
    /// Buffer name this input targets.
    pub name: String,
    /// Flat f32 values (row-major).
    pub values: Vec<f32>,
}

/// The producer-owned canonical run plan carried in the FMIR
/// `device_program.payload` field (N1.7 §7.1).
///
/// The representation is backend-neutral (the logical entry is the program
/// fact; the CUDA symbol mapping rides beside it) and deterministic, so the
/// same package derives identical payload bytes on both routes (A10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DeviceRunPlan {
    /// Canonical representation version ([`DEVICE_RUN_PLAN_VERSION`]).
    pub v: u32,
    /// Program execution-lifetime regime (codec v2, S2-4).
    pub lifetime: PlanProgramLifetime,
    /// Ordered kernels, in program order.
    pub kernels: Vec<PlanKernel>,
    /// CUDA logical-entry → symbol identities (empty for non-CUDA images).
    pub cuda_kernels: Vec<PlanCudaKernel>,
    /// Host input values for the program's input buffers.
    pub inputs: Vec<PlanInput>,
}

/// Encode the canonical run plan to its deterministic JSON payload.
///
/// Named-field serde JSON is deterministic for a fixed struct shape; the
/// identity substrate of the same-package hash must be byte-stable.
///
/// # Errors
/// Fail-closed when the plan cannot be serialized (never in practice — every
/// field is a plain JSON value).
fn encode_payload(plan: &DeviceRunPlan) -> Result<String, Vec<Diagnostic>> {
    serde_json::to_string(plan).map_err(|error| {
        vec![device_diag(
            "payload",
            format!("canonical run plan serialization failed: {error}"),
        )]
    })
}

/// Parse a canonical run-plan payload back from an FMIR device section.
///
/// Fail-closed admission (S2-4): the version field is read **first**, before
/// any field-level parse, so an old (or unknown) codec version fails with the
/// structured `payload_version` diagnostic — never a silent default and never
/// a generic field-level error that hides the version gate.
///
/// # Errors
/// Fail-closed when the payload is not a valid run plan or carries an
/// unsupported version.
pub(crate) fn parse_payload(payload: &str) -> Result<DeviceRunPlan, Vec<Diagnostic>> {
    let value: serde_json::Value = serde_json::from_str(payload).map_err(|error| {
        vec![Diagnostic::error(format!(
            "device section payload is not a valid run plan: {error}"
        ))
        .with_arg("issue", "E_DEVICE_DESCRIPTOR")]
    })?;
    let Some(version) = value.get("v").and_then(serde_json::Value::as_u64) else {
        return Err(vec![Diagnostic::error(
            "device section payload is not a valid run plan (missing version field `v`)",
        )
        .with_arg("issue", "E_DEVICE_DESCRIPTOR")]);
    };
    if version != u64::from(DEVICE_RUN_PLAN_VERSION) {
        return Err(vec![Diagnostic::error(format!(
            "device section payload version {} is not supported (expected {})",
            version, DEVICE_RUN_PLAN_VERSION
        ))
        .with_arg("issue", "E_DEVICE_DESCRIPTOR")
        .with_arg("payload_version", version.to_string())]);
    }
    let plan: DeviceRunPlan = serde_json::from_value(value).map_err(|error| {
        vec![Diagnostic::error(format!(
            "device section payload is not a valid run plan: {error}"
        ))
        .with_arg("issue", "E_DEVICE_DESCRIPTOR")]
    })?;
    Ok(plan)
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
    let cuda_artifact =
        radix_mir_llvm::emit_cuda_device_artifact(program, validated, interner).map_err(|error| {
            vec![device_diag("cuda artifact", error.to_string())]
        })?;

    let mut artifact = vec![FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        blob: metal_artifact.source,
        hash: metal_artifact.hash,
    }];
    match radix_mir_llvm::compile_nvvm_to_ptx(&cuda_artifact.source, ptx_target) {
        Ok(ptx) => {
            // The packaged CUDA artifact is PTX (N1.3 §3.1); its provenance
            // hash covers the PTX blob, not the NVVM source.
            let ptx_hash = radix_mir_fmir::fnv1a64_blob_hash(ptx.as_bytes());
            artifact.push(FmirDeviceArtifact {
                backend: FmirDeviceBackend::Cuda,
                blob: ptx,
                hash: ptx_hash,
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

    let plan = build_run_plan_with_ids(program, Some(&cuda_artifact), inputs);
    let payload = encode_payload(&plan)?;

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
        device_program: FmirDeviceProgramSection { payload },
        selection: match selection {
            DeviceSelection::Auto => FmirDeviceSelection::Auto,
            DeviceSelection::Metal => FmirDeviceSelection::Metal,
            DeviceSelection::Cuda => FmirDeviceSelection::Cuda,
        },
        artifacts: FmirDeviceArtifactsSection { artifact },
        runtime_requirements,
    })
}

/// Build the run plan directly from the device program's typed facts: one
/// plan kernel per kernel unit, with program-level buffer ids/names, the
/// derived launch grid/block, the CUDA symbol mapping, and the host inputs.
#[must_use]
fn build_run_plan_with_ids(
    program: &DeviceProgram,
    cuda_artifact: Option<&radix_mir_llvm::CudaDeviceArtifact>,
    inputs: &BTreeMap<String, Vec<f32>>,
) -> DeviceRunPlan {
    let cuda_kernels = cuda_artifact
        .map(|artifact| {
            artifact
                .kernels
                .iter()
                .map(|identity| PlanCudaKernel {
                    entry: identity.entry.clone(),
                    symbol: identity.symbol.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let kernels = program
        .kernels
        .iter()
        .map(|kernel| PlanKernel {
            entry: kernel.entry.clone(),
            slots: kernel
                .resources
                .iter()
                .map(|resource| PlanSlot {
                    id: resource.buffer.id.0,
                    name: resource.buffer.name.clone(),
                    role: role_spelling(resource.buffer.role),
                    lifetime: plan_slot_lifetime(resource.buffer.lifetime),
                    binding: resource.binding.binding,
                    ty: data_type_spelling(resource.version.element_ty),
                    count: resource.version.element_count,
                })
                .collect(),
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
        })
        .collect();
    let plan_inputs = inputs
        .iter()
        .map(|(name, values)| PlanInput {
            name: name.clone(),
            values: values.clone(),
        })
        .collect();
    DeviceRunPlan {
        v: DEVICE_RUN_PLAN_VERSION,
        lifetime: plan_program_lifetime(program.lifetime),
        kernels,
        cuda_kernels,
        inputs: plan_inputs,
    }
}

fn role_spelling(role: BufferRole) -> String {
    match role {
        BufferRole::Input => "input".to_owned(),
        BufferRole::Output => "output".to_owned(),
        BufferRole::InOut => "in-out".to_owned(),
    }
}

/// The S2-4 constructor mapping (delivery spec N2.4, open question 3's
/// default): Input → PerProgram, Output → ObservationPoint, InOut → PerStep.
/// Derived from the language/ABI facts by the constructor with the schema's
/// `BufferLifetime` semantics as the authority; the host never re-derives it.
fn lifetime_for_role(role: BufferRole) -> BufferLifetime {
    match role {
        BufferRole::Input => BufferLifetime::PerProgram,
        BufferRole::Output => BufferLifetime::ObservationPoint,
        BufferRole::InOut => BufferLifetime::PerStep,
    }
}

/// Map a radix [`BufferLifetime`] onto its payload spelling (codec v2).
fn plan_slot_lifetime(lifetime: BufferLifetime) -> PlanSlotLifetime {
    match lifetime {
        BufferLifetime::PerProgram => PlanSlotLifetime::PerProgram,
        BufferLifetime::PerStep => PlanSlotLifetime::PerStep,
        BufferLifetime::ObservationPoint => PlanSlotLifetime::ObservationPoint,
    }
}

/// Map a radix [`DeviceProgramLifetime`] onto its payload spelling (codec v2).
fn plan_program_lifetime(lifetime: DeviceProgramLifetime) -> PlanProgramLifetime {
    match lifetime {
        DeviceProgramLifetime::SingleRun => PlanProgramLifetime::SingleRun,
        DeviceProgramLifetime::RepeatingStep => PlanProgramLifetime::RepeatingStep,
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
pub(crate) fn artifact_for_backend<'a>(
    artifacts: &'a [FmirDeviceArtifact],
    backend: DeviceBackend,
) -> Option<&'a FmirDeviceArtifact> {
    artifacts.iter().find(|artifact| match backend {
        DeviceBackend::Metal => artifact.backend == FmirDeviceBackend::Metal,
        DeviceBackend::Cuda => artifact.backend == FmirDeviceBackend::Cuda,
    })
}

/// Build the typed host descriptor for a run plan + backend artifact blob.
///
/// The S1-3 typed logical-entry → NVVM-symbol mapping is **consumed here**:
/// the CUDA descriptor's kernel entry is the emitted PTX `.entry` symbol
/// ([`CudaKernelIdentity`]`::symbol`), never the logical entry; Metal
/// launches by the logical entry (the emitted MSL kernel name). Slots are
/// carried in binding order so the composite host binds buffers in the
/// emitted kernel's buffer/param order. The payload's typed lifetimes (codec
/// v2) are mapped onto the host descriptor's [`DeviceBufferLifetime`]/[`DeviceProgramLifetime`]
/// — the host consumes the constructor-derived facts; it never re-derives a
/// lifetime from slot role (S2-4).
#[must_use]
pub(crate) fn descriptor_for_backend(
    plan: &DeviceRunPlan,
    backend: DeviceBackend,
    blob: &[u8],
) -> DeviceDescriptor {
    let kernels = plan
        .kernels
        .iter()
        .map(|kernel| {
            let entry = match backend {
                DeviceBackend::Cuda => plan
                    .cuda_kernels
                    .iter()
                    .find(|identity| identity.entry == kernel.entry)
                    .map(|identity| identity.symbol.clone())
                    .unwrap_or_else(|| kernel.entry.clone()),
                DeviceBackend::Metal => kernel.entry.clone(),
            };
            let buffers = kernel
                .slots
                .iter()
                .map(|slot| DescriptorBuffer {
                    buffer_id: slot.id,
                    buffer_name: slot.name.clone(),
                    role: parse_role(&slot.role),
                    lifetime: parse_slot_lifetime(slot.lifetime),
                    binding: slot.binding,
                    element_ty: DeviceDataType::F32,
                    element_count: slot.count,
                })
                .collect();
            DescriptorKernel {
                entry,
                buffers,
                grid: kernel.grid,
                block: kernel.block,
            }
        })
        .collect();
    DeviceDescriptor {
        backend,
        module_image: blob.to_vec(),
        kernels,
        program_lifetime: match plan.lifetime {
            PlanProgramLifetime::SingleRun => HostDeviceProgramLifetime::SingleRun,
            PlanProgramLifetime::RepeatingStep => HostDeviceProgramLifetime::RepeatingStep,
        },
    }
}

fn parse_role(spelling: &str) -> DeviceBufferRole {
    match spelling {
        "output" => DeviceBufferRole::Output,
        "in-out" => DeviceBufferRole::InOut,
        _ => DeviceBufferRole::Input,
    }
}

/// Map the payload's typed lifetime onto the host descriptor's typed
/// lifetime. The payload admission already guarantees the spelling is one of
/// the three (an unknown variant fails the serde parse closed); this is a
/// total function over that admitted set.
fn parse_slot_lifetime(lifetime: PlanSlotLifetime) -> DeviceBufferLifetime {
    match lifetime {
        PlanSlotLifetime::PerProgram => DeviceBufferLifetime::PerProgram,
        PlanSlotLifetime::PerStep => DeviceBufferLifetime::PerStep,
        PlanSlotLifetime::ObservationPoint => DeviceBufferLifetime::ObservationPoint,
    }
}

/// Map the plan's named inputs onto buffer ids (via the plan slots).
fn inputs_by_buffer_id(plan: &DeviceRunPlan) -> BTreeMap<u32, Vec<f32>> {
    let mut by_name: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for input in &plan.inputs {
        by_name.insert(input.name.clone(), input.values.clone());
    }
    let mut by_id: BTreeMap<u32, Vec<f32>> = BTreeMap::new();
    for kernel in &plan.kernels {
        for slot in &kernel.slots {
            if slot.role == "input" {
                if let Some(values) = by_name.get(&slot.name) {
                    by_id.insert(slot.id, values.clone());
                }
            }
        }
    }
    by_id
}

/// The observation-point buffer ids the run reads back (S2-4).
///
/// The readback set is exactly the buffers whose typed lifetime is
/// ObservationPoint — the declared observation points. InOut intermediates
/// (PerStep lifetime under the constructor mapping) are never read back, so
/// the ordinary `faber run` path performs no undeclared readback between
/// kernels (campaign exit-gate bullet 1).
fn observation_buffer_ids(plan: &DeviceRunPlan) -> Vec<u32> {
    let mut ids = Vec::new();
    for kernel in &plan.kernels {
        for slot in &kernel.slots {
            if slot.lifetime == PlanSlotLifetime::ObservationPoint && !ids.contains(&slot.id) {
                ids.push(slot.id);
            }
        }
    }
    ids
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
) -> Result<(), Vec<Diagnostic>> {
    let plan = parse_payload(&device.device_program.payload)?;
    let artifact = artifact_for_backend(&device.artifacts.artifact, backend).ok_or_else(|| {
        vec![super::host_factory::missing_backend_artifact(backend)]
    })?;
    // A9 discovery receipt: selected device + declared artifact hash.
    let discovery = super::host_factory::discovery_receipt(backend, &device.artifacts.artifact)
        .ok_or_else(|| vec![super::host_factory::missing_backend_artifact(backend)])?;
    discovery.print();
    let descriptor = descriptor_for_backend(&plan, backend, artifact.blob.as_bytes());
    // Fail-before-launch: the descriptor is validated by the composite host
    // before any kernel is dispatched.
    let selection = match backend {
        DeviceBackend::Metal => DeviceSelection::Metal,
        DeviceBackend::Cuda => DeviceSelection::Cuda,
    };
    let mut host = super::host_factory::construct_composite_host(selection, true)
        .map_err(|diagnostic| vec![diagnostic])?;
    let inputs = inputs_by_buffer_id(&plan);
    let outputs = observation_buffer_ids(&plan);

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

    // A10 declared logical resource graph: buffer identities, roles,
    // lifetimes, content versions, and the inter-kernel data-flow edges.
    println!("device: declared resource graph (A10):");
    for buffer in &receipt.resource_graph {
        println!(
            "device:   buffer {} `{}` {} {} version {} ({}[{}])",
            buffer.id,
            buffer.name,
            buffer.role.spelling(),
            buffer.lifetime.spelling(),
            buffer.version,
            buffer.element_ty.spelling(),
            buffer.element_count
        );
    }
    if receipt.data_flow_edges.is_empty() {
        println!("device:   data-flow edges: none");
    } else {
        for edge in &receipt.data_flow_edges {
            println!(
                "device:   data-flow {} -> {} via buffer {} version {}",
                edge.producer, edge.consumer, edge.buffer_id, edge.version
            );
        }
    }

    for (buffer_id, values) in &receipt.outputs {
        let name = plan
            .kernels
            .iter()
            .flat_map(|kernel| kernel.slots.iter())
            .find(|slot| slot.id == *buffer_id)
            .map(|slot| slot.name.as_str())
            .unwrap_or("<unknown>");
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
