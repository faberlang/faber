//! Target-neutral MIR device-program schema (A2 — the common device program).
//!
//! This module is the **shared-schema authority** for the differentiable-GPU
//! campaign (`docs/factory/gpu-training-lowering/`): the target-neutral
//! [`DeviceProgram`] representation that `hosts` sessions, the product CLI
//! (`faber`), and the FMIR device payload (S1-2) all consume. It is
//! single-writer in `radix` (campaign N2); nothing else in the workspace may
//! fork or embed a private copy of this schema.
//!
//! # Contract mapping (N1.6 / architecture record §6)
//!
//! The frozen contract (Stage 0 delivery spec §2 N1.6) names the
//! representation
//!
//! ```text
//! DeviceProgram { kernels: Vec<KernelUnit>, launches, lifetime, results }
//! ```
//!
//! and the ordered kernel unit (function id, entry, plan — `CollectionKernelPlan`
//! or elementwise — typed resource list, workgroup/dispatch/launch plan),
//! resources (buffer identity with role input/output/in-out, binding, element
//! type, element count, storage layout `DeviceHandle`/`HostOwned`), buffer
//! versions/lifetimes (per-program / per-step / observation point),
//! multi-output results (explicit result buffers), and shape-changing
//! operations (a new shape is a new version; no in-place reinterpretation).
//! The mapping is direct:
//!
//! | Contract concept | Schema type |
//! | --- | --- |
//! | `DeviceProgram` | [`DeviceProgram`] |
//! | ordered kernel unit | [`KernelUnit`] |
//! | function id | [`KernelUnit`]`.function` (`MirFunctionId`) |
//! | entry | [`KernelUnit`]`.entry` |
//! | plan (`CollectionKernelPlan` or elementwise) | [`KernelUnit`]`.plan` |
//! | typed resource list | [`KernelUnit`]`.resources` (`Vec<DeviceResource>`) |
//! | workgroup/dispatch/launch plan | [`KernelLaunchPlan`] |
//! | buffer identity | [`BufferIdentity`] (id, name, role, storage, lifetime) |
//! | binding / element type / element count | [`DeviceResource`] (binding + [`BufferVersion`] shape) |
//! | buffer versions / lifetimes | [`BufferVersion`] + [`BufferLifetime`] |
//! | multi-output results | [`ResultBuffer`] |
//! | shape-changing operations | version numbers (new shape ⇒ new version; see [`DeviceProgram::validate`]) |
//! | launches | [`LaunchUnit`] |
//! | lifetime | [`DeviceProgramLifetime`] |
//!
//! # WGSL chain-descriptor generalization (§6.3)
//!
//! The WGSL chain descriptor (`radix-mir-wgsl/src/chain_descriptor.rs`)
//! models an ordered kernel chain with per-kernel storage-buffer declarations,
//! bind-group layouts, output bindings, and positional inter-kernel identity
//! mappings. This schema generalizes that concept in common MIR without
//! promoting the WGSL schema verbatim:
//!
//! | WGSL chain descriptor | Generalized form |
//! | --- | --- |
//! | `chain: Vec<KernelDescriptor>` | ordered [`DeviceProgram`]`.kernels` + [`DeviceProgram`]`.launches` |
//! | `entry_point` + WGSL `source` | target-neutral [`KernelUnit`]`.entry`; no source text (A3) |
//! | `storage_buffers: Vec<StorageBufferDecl>` | typed [`DeviceResource`] list (element type + count + storage layout) |
//! | `bind_group_layout` / `BindGroupEntry` | [`DeviceResource`]`.binding` |
//! | `output_bindings` | explicit [`ResultBuffer`] entries |
//! | `InterKernelBufferIdentity` (output→input position pairs) | buffer identity + content version; producer/consumer edges derived in [`BufferRegistry`] and exposed as [`DataFlowPair`] |
//!
//! The WGSL descriptor is positional (kernel index + binding) because the WGSL
//! text is the identity. This schema keys data flow on a declared buffer
//! identity and a content version, so hosts never infer connectivity from
//! emitted text or binding positions (A3). What is **not** carried over: WGSL
//! source strings, per-kernel pipeline/bind-group reflection, and positional
//! identity — those are emitter/host concerns, not program facts.
//!
//! # Materialization seam (A2/A4) — S1-1 design-note decision
//!
//! **Decision**: the device program is produced by a **device-program
//! constructor over typed AIR/MIR facts** — never a structural clone of
//! `radix-air/src/to_mir.rs` and never a GPU-specific relowering.
//!
//! Evidence:
//! - `to_mir.rs` is a structural per-function lowering
//!   (`AirProgramToMir::lower_program` → `AirFunctionToMir::lower_function`)
//!   that reconstructs locals, statements, and CFG from AIR expressions. The
//!   device program needs none of that: it needs signatures, plans, typed
//!   resources, and launch plans, all of which already exist as typed facts on
//!   a validated MIR program.
//! - The typed facts the constructor composes are live today:
//!   [`MirKernelSignature`](crate::abi::MirKernelSignature) per device function
//!   (the `storage_buffer_kernel` family in `abi.rs`), the typed
//!   `MirCollectionOp → CollectionKernelPlan` mapping (council TB5,
//!   [`CollectionKernelPlan::from_contract`] in `kernel_plan.rs`, which already
//!   consumes [`CollectionOpContract`](crate::abi::CollectionOpContract),
//!   the typed op, and matmul shapes), and
//!   [`MirDeviceRole`](crate::device::MirDeviceRole) /
//!   [`MirDeviceContext`](crate::device::MirDeviceContext) (which functions are
//!   device-resident, and — via `classify_device_safe` keying on
//!   [`MirTensorStorageLayout::DeviceHandle`] /
//!   [`MirTensorLocusLayout::Device`] — which tensors are device-resident).
//! - A1/A4: `to_mir.rs` remains the single CPU materialization (untouched);
//!   typed collection calls remain valid compiler input; differentiation math
//!   stays in `radix-air`. A constructor that folds typed facts cannot fork
//!   VJP formulas or introduce GPU-specific AIR nodes.
//! - The building-block adapters in this module —
//!   [`KernelLaunchPlan::from_signature_and_function`],
//!   [`DeviceResource::from_kernel_resource`], [`BufferRole::from_abi_role`] —
//!   are exactly the typed-fact surface a materializer composes. The concrete
//!   materializer module (radix-air, beside `to_mir.rs`, or radix-mir beside
//!   `kernel_plan.rs`) is picked when the TB5 typed plan mapping lands (S1-3);
//!   S1-1 owns the schema and these adapters only.
//!
//! **Constructor shape**: `ValidatedMir` + `MirDeviceContext` + per-kernel
//! `MirKernelSignature` + `CollectionKernelPlan` → [`DeviceProgram`], via
//! [`DeviceProgram::new`] and the typed adapters above. If a later stage finds
//! this reading incompatible with the frozen schema, it stops and routes a
//! need to the Mind (delivery spec §11); it does not fork silently.

use crate::abi::{
    collection_op_workgroup_count, MirKernelDispatchSize, MirKernelResource,
    MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature,
    MirKernelWorkgroupCount, MirWorkgroupSize,
};
use crate::device_semantics::{DependencyEdge, DeviceSemantics, SemanticValueId};
use crate::kernel_plan::{CollectionKernelPlan, PlanValidationError};
use crate::layout::MirTensorStorageLayout;
use crate::{MirFunction, MirFunctionId, MirType};
use radix_hir::DefId;
use std::fmt;

/// Program-unique identity of one device-resident buffer.
///
/// Buffers are the unit of data flow between kernels: a kernel writes a buffer
/// version and a later kernel reads it. The numeric id is the identity key;
/// [`DeviceProgram::buffer_registry`] derives the program-level registry from
/// every resource and result that references an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BufferId(pub u32);

/// Logical role of a buffer across the whole program.
///
/// - [`Input`](BufferRole::Input): copied in from the host, read-only on the
///   device (weights, constants, inputs).
/// - [`Output`](BufferRole::Output): written by a device kernel and read back
///   as a program result.
/// - [`InOut`](BufferRole::InOut): device-resident intermediate, written by
///   one kernel and read by another (activations, gradients, optimizer state).
///
/// The role is program-level; the per-slot read/write usage at a kernel is the
/// resource's [`MirKernelResourceAccess`]. [`BufferRole::from_abi_role`] maps
/// the ABI role/access pair onto the program role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BufferRole {
    /// Host-provided input; every slot accesses it read-only.
    Input,
    /// Device-produced result; read back at an observation point.
    Output,
    /// Device-resident intermediate shared across kernels.
    InOut,
}

impl BufferRole {
    /// Map an ABI kernel-resource role/access pair onto a program buffer role.
    ///
    /// The ABI surface ([`MirKernelResourceRole`], [`MirKernelResourceAccess`])
    /// has no `in-out` role; an in-out buffer is expressed as a read-write
    /// access (on either role). Pairs that cannot express a coherent program
    /// role — writing an input, reading an output — yield `None`.
    #[must_use]
    pub fn from_abi_role(
        role: MirKernelResourceRole,
        access: MirKernelResourceAccess,
    ) -> Option<Self> {
        match (role, access) {
            (MirKernelResourceRole::Input, MirKernelResourceAccess::Read) => Some(Self::Input),
            (MirKernelResourceRole::Output, MirKernelResourceAccess::Write) => Some(Self::Output),
            (_, MirKernelResourceAccess::ReadWrite) => Some(Self::InOut),
            _ => None,
        }
    }

    /// The ABI resource role this program role maps back to
    /// (in-out maps to the input role with read-write access).
    #[must_use]
    pub fn abi_role(self) -> MirKernelResourceRole {
        match self {
            Self::Input | Self::InOut => MirKernelResourceRole::Input,
            Self::Output => MirKernelResourceRole::Output,
        }
    }

    /// The ABI resource access this program role maps back to.
    #[must_use]
    pub fn abi_access(self) -> MirKernelResourceAccess {
        match self {
            Self::Input => MirKernelResourceAccess::Read,
            Self::Output => MirKernelResourceAccess::Write,
            Self::InOut => MirKernelResourceAccess::ReadWrite,
        }
    }

    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::InOut => "in-out",
        }
    }
}

/// How long a buffer's storage lives in the device program.
///
/// The contract names three lifetimes: **per-program** buffers are allocated
/// once and persist for the whole program duration (weights, persistent
/// optimizer state); **per-step** buffers are recycled between training-step
/// iterations (activations, gradients); an **observation point** buffer is
/// read back at a declared point (a program result). These classifications
/// feed host allocation/teardown policy (Stage 2 lifecycle) and the A9
/// receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BufferLifetime {
    /// Allocated once for the whole program; persists across steps.
    PerProgram,
    /// Live within one training step; recycled between iterations.
    PerStep,
    /// Read back at a declared observation point.
    ObservationPoint,
}

impl BufferLifetime {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::PerProgram => "per-program",
            Self::PerStep => "per-step",
            Self::ObservationPoint => "observation-point",
        }
    }
}

/// Stable identity facts of one device buffer.
///
/// Every resource and result that references a buffer repeats these facts; the
/// program registry verifies they agree. Storage layout is the live MIR
/// [`MirTensorStorageLayout`] — a device-program buffer is
/// [`DeviceHandle`](MirTensorStorageLayout::DeviceHandle) for device-resident
/// storage or [`HostOwned`](MirTensorStorageLayout::HostOwned) for host-side
/// storage that the host copies in/out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferIdentity {
    /// Program-unique identity key.
    pub id: BufferId,
    /// Logical name (diagnostics; target-neutral — never a backend binding).
    pub name: String,
    /// Program-level role (input / output / in-out).
    pub role: BufferRole,
    /// Where the storage lives (`DeviceHandle` / `HostOwned`).
    pub storage: MirTensorStorageLayout,
    /// How long the storage lives.
    pub lifetime: BufferLifetime,
}

/// One content version of a buffer: a shape snapshot.
///
/// Shape-changing operations are explicit: a new shape is a **new version**,
/// and no slot may rebind an existing version to a different shape (no
/// in-place reinterpretation — [`DeviceProgram::validate`] enforces this).
/// Version numbers are program-unique per buffer and start at 1 (0 is
/// reserved as the unset sentinel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferVersion {
    /// Monotonic version number within the buffer (≥ 1).
    pub version: u32,
    /// Element type of this version's shape.
    pub element_ty: MirType,
    /// Element count of this version's shape.
    pub element_count: u64,
}

/// Target-neutral binding indices for a kernel slot.
///
/// `group`/`binding` are the two-level binding indices used by the kernel ABI
/// (`MirKernelResource.group`/`.binding`); backends map them to their own
/// binding syntax. The values are program facts, never inferred from emitted
/// text (A3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Binding {
    /// First-level binding index.
    pub group: u32,
    /// Second-level binding index within the group.
    pub binding: u32,
}

/// A typed buffer slot bound to a kernel.
///
/// This is the typed resource list entry of a [`KernelUnit`]: one buffer
/// version bound at one binding, with the slot's read/write access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceResource {
    /// The buffer's program-level identity facts.
    pub buffer: BufferIdentity,
    /// The content version (and shape) this slot binds.
    pub version: BufferVersion,
    /// Target-neutral binding indices.
    pub binding: Binding,
    /// Slot read/write access (live ABI enum).
    pub access: MirKernelResourceAccess,
}

impl DeviceResource {
    /// Adapt a live ABI kernel resource into a device-program slot.
    ///
    /// Maps the resource's group/binding and access; the caller supplies the
    /// program-level buffer identity and the content version to bind.
    /// Non-data resources — the runtime-extent count channel — are not device
    /// buffers and yield `None` (the extent stays a kernel-signature concern).
    #[must_use]
    pub fn from_kernel_resource(
        buffer: BufferIdentity,
        version: BufferVersion,
        resource: &MirKernelResource,
    ) -> Option<Self> {
        if resource.kind != MirKernelResourceKind::StorageBuffer {
            return None;
        }
        Some(Self {
            buffer,
            version,
            binding: Binding {
                group: resource.group,
                binding: resource.binding,
            },
            access: resource.access,
        })
    }
}

/// The workgroup / dispatch / launch plan of one kernel.
///
/// Carries the per-kernel thread-shape facts hosts need to launch: the
/// workgroup size, the dispatch size, and the derived workgroup (grid) count.
/// The count is derived by the live ABI helper
/// [`collection_op_workgroup_count`] (matmul launches the tile grid verbatim;
/// everything else dispatches `ceil(dispatch / workgroup)` per axis).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernelLaunchPlan {
    /// Threads per workgroup.
    pub workgroup: MirWorkgroupSize,
    /// Dispatch extent in workgroups for each axis.
    pub dispatch_size: MirKernelDispatchSize,
    /// Derived workgroup count the host launches.
    pub workgroup_count: MirKernelWorkgroupCount,
}

impl KernelLaunchPlan {
    /// Build the launch plan from a live kernel signature and its MIR
    /// function, deriving the workgroup count via the shared ABI rule.
    #[must_use]
    pub fn from_signature_and_function(
        signature: &MirKernelSignature,
        function: &MirFunction,
    ) -> Self {
        let workgroup = signature.workgroup;
        let dispatch_size = signature.dispatch_size;
        let workgroup_count = collection_op_workgroup_count(function, dispatch_size, workgroup);
        Self {
            workgroup,
            dispatch_size,
            workgroup_count,
        }
    }
}

/// One ordered kernel declaration in the device program.
///
/// `function` identifies the MIR function this kernel lowers from; `entry` is
/// the target-neutral logical entry name (backends map it to their syntax);
/// `plan` is the shared algorithmic recipe ([`CollectionKernelPlan`], with the
/// `Elementwise` variant covering the no-recipe fallback); `resources` is the
/// typed resource list; `launch` is the workgroup/dispatch/launch plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelUnit {
    /// MIR function id this kernel is lowered from.
    pub function: MirFunctionId,
    /// Target-neutral logical entry name.
    pub entry: String,
    /// Shared algorithmic recipe (elementwise, matmul, reduction).
    pub plan: CollectionKernelPlan,
    /// Typed buffer slots bound by this kernel.
    pub resources: Vec<DeviceResource>,
    /// Workgroup / dispatch / launch plan.
    pub launch: KernelLaunchPlan,
}

/// Program-unique identity of one launch in the ordered sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LaunchId(pub u32);

/// One entry in the ordered launch sequence.
///
/// `launches` is the program's execution order — the generalized form of the
/// WGSL chain descriptor's `chain`. A kernel may be launched multiple times
/// (a repeated per-step launch or a re-launch after a shape change); the
/// launch plan is the launched kernel's [`KernelUnit`]`::launch`, so the host
/// needs no per-launch override for the S1-1 contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchUnit {
    /// Program-unique launch id.
    pub id: LaunchId,
    /// Index into [`DeviceProgram`]`::kernels`.
    pub kernel_index: usize,
}

/// An explicit result buffer (multi-output readback).
///
/// Multi-output results are declared explicitly: each result names the buffer
/// identity and version to observe, the launch that produced it, and its role
/// (output or in-out — an in-place-updated buffer read back at an observation
/// point is a result too). The observation role must agree with the buffer's
/// identity role ([`DeviceProgram::validate`] enforces the cross-check).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultBuffer {
    /// Identity of the observed buffer.
    pub buffer: BufferIdentity,
    /// Observed content version and shape.
    pub version: BufferVersion,
    /// Role of the observation (output or in-out).
    pub role: BufferRole,
    /// The launch that produced the observed version.
    pub produced_by: LaunchId,
}

/// The execution-lifetime regime of the whole program.
///
/// Buffer lifetimes (per-program / per-step / observation point) live on each
/// [`BufferIdentity`]; this is the program-level counterpart: whether the
/// program runs once or repeats as a training step (which is when per-step
/// buffers recycle between iterations while per-program buffers persist).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceProgramLifetime {
    /// One-shot program run.
    #[default]
    SingleRun,
    /// Repeating training step; per-step buffers recycle between iterations.
    RepeatingStep,
}

/// A target-neutral MIR device program (A2; the frozen N1.6 schema).
///
/// ```text
/// DeviceProgram { kernels: Vec<KernelUnit>, launches, lifetime, results }
/// ```
///
/// - `kernels`: the ordered kernel declarations (function id, entry, plan,
///   typed resource list, launch plan).
/// - `launches`: the ordered execution sequence over the kernels.
/// - `lifetime`: the program execution-lifetime regime.
/// - `results`: the explicit multi-output result buffers.
///
/// Buffer identities, versions, and the producer/consumer data-flow edges are
/// derived as a [`BufferRegistry`] (see [`DeviceProgram::buffer_registry`]);
/// [`DeviceProgram::validate`] enforces the consistency rules, including the
/// shape-changing contract (a new shape is a new version; no in-place
/// reinterpretation).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceProgram {
    /// Ordered kernel declarations.
    pub kernels: Vec<KernelUnit>,
    /// Ordered launch sequence.
    pub launches: Vec<LaunchUnit>,
    /// Program execution-lifetime regime.
    pub lifetime: DeviceProgramLifetime,
    /// Explicit multi-output result buffers.
    pub results: Vec<ResultBuffer>,
}

impl DeviceProgram {
    /// A new, empty device program with the given lifetime regime.
    #[must_use]
    pub fn new(lifetime: DeviceProgramLifetime) -> Self {
        Self {
            kernels: Vec::new(),
            launches: Vec::new(),
            lifetime,
            results: Vec::new(),
        }
    }

    /// Derive the program's buffer registry: every distinct [`BufferId`] with
    /// its identity facts, version chain, and producer/consumer launch edges.
    ///
    /// The registry is a view over the program — it is not stored in the
    /// program, so the frozen four-field shape stays intact. Callers should
    /// [`validate`](DeviceProgram::validate) first so launch references are in
    /// range; an unvalidated program still derives (out-of-range launches are
    /// skipped).
    #[must_use]
    pub fn buffer_registry(&self) -> BufferRegistry {
        let mut buffers: Vec<RegistryBuffer> = Vec::new();

        for launch in &self.launches {
            let Some(kernel) = self.kernels.get(launch.kernel_index) else {
                continue;
            };
            for resource in &kernel.resources {
                let entry = registry_buffer_mut(&mut buffers, &resource.buffer);
                let version = registry_version_mut(&mut entry.versions, &resource.version);
                match resource.access {
                    MirKernelResourceAccess::Read => version.consumed_by.push(launch.id),
                    MirKernelResourceAccess::Write => version.produced_by.push(launch.id),
                    MirKernelResourceAccess::ReadWrite => {
                        version.consumed_by.push(launch.id);
                        version.produced_by.push(launch.id);
                    }
                }
            }
        }

        // Results contribute the observed versions to the chain.
        for result in &self.results {
            let entry = registry_buffer_mut(&mut buffers, &result.buffer);
            registry_version_mut(&mut entry.versions, &result.version);
        }

        BufferRegistry { buffers }
    }

    /// Validate the device program's consistency rules.
    ///
    /// Checks, in order:
    /// 1. the program declares at least one kernel and one launch;
    /// 2. per kernel: a non-empty entry name, a valid
    ///    [`CollectionKernelPlan`], unique (group, binding) slots, version
    ///    numbers ≥ 1, and role/access consistency (inputs are read-only;
    ///    outputs are written; in-out buffers may be read or written);
    /// 3. the launch sequence: every kernel reference is in range and launch
    ///    ids are unique;
    /// 4. results: version ≥ 1, role output or in-out, the observation role
    ///    agrees with the buffer's identity role, and the producing
    ///    launch exists;
    /// 5. cross-reference consistency: every reference to the same
    ///    [`BufferId`] carries the same identity facts (name, role, storage,
    ///    lifetime), and every reference to the same `(BufferId, version)`
    ///    carries the same shape — a shape change must be a **new version**
    ///    (no in-place reinterpretation);
    /// 6. every result is actually produced by its named launch.
    ///
    /// # Errors
    /// Returns the first [`DeviceProgramError`] the program violates.
    pub fn validate(&self) -> Result<(), DeviceProgramError> {
        if self.kernels.is_empty() {
            return Err(DeviceProgramError::EmptyKernels);
        }
        if self.launches.is_empty() {
            return Err(DeviceProgramError::EmptyLaunches);
        }

        for (kernel_index, kernel) in self.kernels.iter().enumerate() {
            if kernel.entry.trim().is_empty() {
                return Err(DeviceProgramError::EmptyEntryName {
                    kernel: kernel_index,
                });
            }
            kernel
                .plan
                .validate()
                .map_err(|error| DeviceProgramError::Plan { error })?;
            let mut seen_bindings: Vec<Binding> = Vec::new();
            for resource in &kernel.resources {
                if resource.version.version == 0 {
                    return Err(DeviceProgramError::ZeroVersion {
                        buffer: resource.buffer.id,
                        version: resource.version.version,
                    });
                }
                if seen_bindings.contains(&resource.binding) {
                    return Err(DeviceProgramError::DuplicateBinding {
                        kernel: kernel_index,
                        group: resource.binding.group,
                        binding: resource.binding.binding,
                    });
                }
                seen_bindings.push(resource.binding);
                if !role_permits_access(resource.buffer.role, resource.access) {
                    return Err(DeviceProgramError::RoleAccessMismatch {
                        buffer: resource.buffer.id,
                        role: resource.buffer.role,
                        access: resource.access,
                    });
                }
            }
        }

        let mut seen_launches: Vec<LaunchId> = Vec::new();
        for launch in &self.launches {
            if launch.kernel_index >= self.kernels.len() {
                return Err(DeviceProgramError::LaunchOutOfRange {
                    launch: launch.id,
                    kernel_index: launch.kernel_index,
                    kernel_count: self.kernels.len(),
                });
            }
            if seen_launches.contains(&launch.id) {
                return Err(DeviceProgramError::DuplicateLaunchId { launch: launch.id });
            }
            seen_launches.push(launch.id);
        }

        for (result_index, result) in self.results.iter().enumerate() {
            if result.version.version == 0 {
                return Err(DeviceProgramError::ZeroVersion {
                    buffer: result.buffer.id,
                    version: result.version.version,
                });
            }
            if !matches!(result.role, BufferRole::Output | BufferRole::InOut) {
                return Err(DeviceProgramError::ResultRole {
                    result: result_index,
                    role: result.role,
                });
            }
            // The observation role must agree with the buffer's identity role:
            // a result declaring `InOut` on an `Output` buffer (or vice versa)
            // is a contradictory schema fact — both roles are individually
            // valid, so only the cross-check catches it.
            if result.role != result.buffer.role {
                return Err(DeviceProgramError::ResultBufferRoleMismatch {
                    result: result_index,
                    role: result.role,
                    buffer_role: result.buffer.role,
                });
            }
            if !self
                .launches
                .iter()
                .any(|launch| launch.id == result.produced_by)
            {
                return Err(DeviceProgramError::UnknownProducingLaunch {
                    result: result_index,
                    produced_by: result.produced_by,
                });
            }
        }

        let mut first_identities: Vec<BufferIdentity> = Vec::new();
        let mut first_shapes: Vec<(BufferId, u32, MirType, u64)> = Vec::new();
        for kernel in &self.kernels {
            for resource in &kernel.resources {
                record_identity(&mut first_identities, &resource.buffer)?;
                record_shape(&mut first_shapes, resource.buffer.id, &resource.version)?;
            }
        }
        for (result_index, result) in self.results.iter().enumerate() {
            record_identity(&mut first_identities, &result.buffer)?;
            record_shape(&mut first_shapes, result.buffer.id, &result.version)?;
            if !result_is_produced(self, result) {
                return Err(DeviceProgramError::ResultNotProduced {
                    result: result_index,
                    produced_by: result.produced_by,
                    buffer: result.buffer.id,
                    version: result.version.version,
                });
            }
        }

        Ok(())
    }

    /// Validate the frozen program together with its carried semantic facts
    /// (Stage 3R F1–F7).
    ///
    /// Runs the program rules ([`DeviceProgram::validate`]) and the semantic
    /// rules ([`DeviceSemantics::validate`]), then the **F6 coupling** that
    /// makes results declare observation points only:
    ///
    /// - every [`ResultBuffer`] must be backed by an explicit
    ///   [`ObservationFact`](crate::device_semantics::ObservationFact) for the
    ///   same buffer, content version, and producing launch — a writable
    ///   intermediate exposed as a result without being declared observed is
    ///   rejected (`ResultWithoutObservationFact`);
    /// - every observation fact must be backed by a declared result row —
    ///   a readback the host would never be told to perform is rejected
    ///   (`UndeclaredObservationFact`).
    ///
    /// The axes stay **independent** (F5): initialization, lifetime, access,
    /// observation, and generation each ride their own carried facts; this
    /// cross-check is the one declared coupling — that a result IS an
    /// explicit observation point (F6).
    ///
    /// # Errors
    /// Returns the first [`DeviceProgramError`] the program or its carried
    /// facts violate.
    pub fn validate_with_semantics(
        &self,
        semantics: &DeviceSemantics,
    ) -> Result<(), DeviceProgramError> {
        self.validate()?;
        let launches: Vec<LaunchId> = self.launches.iter().map(|launch| launch.id).collect();
        semantics.validate(&launches)?;
        for (result_index, result) in self.results.iter().enumerate() {
            let declared = semantics.observations.iter().any(|observation| {
                observation.buffer == result.buffer.id
                    && observation.version == result.version.version
                    && observation.at_launch == result.produced_by
            });
            if !declared {
                return Err(DeviceProgramError::ResultWithoutObservationFact {
                    result: result_index,
                    buffer: result.buffer.id,
                    version: result.version.version,
                });
            }
        }
        for observation in &semantics.observations {
            let backed = self.results.iter().any(|result| {
                result.buffer.id == observation.buffer
                    && result.version.version == observation.version
                    && result.produced_by == observation.at_launch
            });
            if !backed {
                return Err(DeviceProgramError::UndeclaredObservationFact {
                    buffer: observation.buffer,
                    version: observation.version,
                    at_launch: observation.at_launch,
                });
            }
        }
        Ok(())
    }
}

/// The derived registry of every buffer, version, and data-flow edge in a
/// device program (see [`DeviceProgram::buffer_registry`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferRegistry {
    /// One entry per distinct buffer id, in first-reference order.
    pub buffers: Vec<RegistryBuffer>,
}

impl BufferRegistry {
    /// Look up a buffer entry by id.
    #[must_use]
    pub fn buffer(&self, id: BufferId) -> Option<&RegistryBuffer> {
        self.buffers.iter().find(|entry| entry.identity.id == id)
    }

    /// Inter-kernel data-flow edges: for every buffer version, the
    /// producer→consumer launch pairs.
    ///
    /// This is the generalized [`InterKernelBufferIdentity`] concept: the WGSL
    /// descriptor maps positional output (kernel, binding) pairs to input
    /// pairs; here an edge is a declared buffer identity + content version,
    /// attributed to the launching that produce and consume it. Self-edges
    /// (a launch that both writes and reads the same version) are excluded —
    /// they are intra-kernel access, not inter-kernel flow.
    #[must_use]
    pub fn data_flow_pairs(&self) -> Vec<DataFlowPair> {
        let mut pairs = Vec::new();
        for entry in &self.buffers {
            for version in &entry.versions {
                for &producer in &version.produced_by {
                    for &consumer in &version.consumed_by {
                        if producer != consumer {
                            pairs.push(DataFlowPair {
                                buffer: entry.identity.id,
                                version: version.version.version,
                                producer,
                                consumer,
                            });
                        }
                    }
                }
            }
        }
        pairs
    }
}

/// One registry entry: the identity facts of a buffer plus its version chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryBuffer {
    /// The buffer's program-level identity facts.
    pub identity: BufferIdentity,
    /// Content versions observed in the program, in first-reference order.
    pub versions: Vec<RegistryVersion>,
}

/// One registry entry: a content version with its producer/consumer launches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryVersion {
    /// The content version and its shape.
    pub version: BufferVersion,
    /// Launches whose kernels write the version (write or read-write slots).
    pub produced_by: Vec<LaunchId>,
    /// Launches whose kernels read the version (read or read-write slots).
    pub consumed_by: Vec<LaunchId>,
}

/// One inter-kernel data-flow edge (see [`BufferRegistry::data_flow_pairs`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DataFlowPair {
    /// Buffer whose content flows between the two launches.
    pub buffer: BufferId,
    /// Content version that flows.
    pub version: u32,
    /// Launch that writes the version.
    pub producer: LaunchId,
    /// Launch that reads the version.
    pub consumer: LaunchId,
}

/// Device-program validation failure (see [`DeviceProgram::validate`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceProgramError {
    /// The program declares no kernels.
    EmptyKernels,
    /// The program declares kernels but no ordered launch sequence.
    EmptyLaunches,
    /// A kernel has an empty entry name.
    EmptyEntryName { kernel: usize },
    /// The kernel's collection-op plan failed its own validation.
    Plan {
        /// The underlying plan failure.
        error: PlanValidationError,
    },
    /// A kernel binds the same (group, binding) slot twice.
    DuplicateBinding {
        kernel: usize,
        group: u32,
        binding: u32,
    },
    /// A resource or result references version 0 (the reserved unset value).
    ZeroVersion { buffer: BufferId, version: u32 },
    /// A slot's access contradicts the buffer's program role.
    RoleAccessMismatch {
        buffer: BufferId,
        role: BufferRole,
        access: MirKernelResourceAccess,
    },
    /// A launch references a kernel index outside the kernel list.
    LaunchOutOfRange {
        launch: LaunchId,
        kernel_index: usize,
        kernel_count: usize,
    },
    /// A launch id appears more than once in the ordered sequence.
    DuplicateLaunchId { launch: LaunchId },
    /// A result names a producing launch that does not exist.
    UnknownProducingLaunch {
        result: usize,
        produced_by: LaunchId,
    },
    /// A result's role is neither output nor in-out.
    ResultRole { result: usize, role: BufferRole },
    /// A result's observation role disagrees with its buffer's identity role.
    ResultBufferRoleMismatch {
        result: usize,
        role: BufferRole,
        buffer_role: BufferRole,
    },
    /// A result's named producing launch does not actually write the version.
    ResultNotProduced {
        result: usize,
        produced_by: LaunchId,
        buffer: BufferId,
        version: u32,
    },
    /// Two references to the same buffer id disagree on an identity fact.
    BufferIdentityConflict {
        buffer: BufferId,
        field: &'static str,
        first: String,
        second: String,
    },
    /// Two references to the same (buffer, version) disagree on the shape —
    /// a shape change must be a new version (no in-place reinterpretation).
    VersionShapeConflict {
        buffer: BufferId,
        version: u32,
        first: String,
        second: String,
    },
    // ── Stage 3R carried-semantics contract (F1–F7) ────────────────────────
    /// A semantic value repeats with conflicting carried facts (F1).
    SemanticIdentityConflict {
        value: SemanticValueId,
        field: &'static str,
        first: String,
        second: String,
    },
    /// Two distinct semantic values are minted from the same carried origin —
    /// they would silently alias (F1).
    SemanticOriginAliased {
        first: SemanticValueId,
        second: SemanticValueId,
        origin: String,
    },
    /// A value id is referenced that no [`SemanticValue`](crate::device_semantics::SemanticValue)
    /// declares (F1).
    UnknownSemanticValue { value: SemanticValueId },
    /// A semantic value is bound to more than one buffer (F1).
    DuplicateValueBinding {
        value: SemanticValueId,
        buffer: BufferId,
    },
    /// Two different values claim the same buffer (F1).
    BufferHoldsMultipleValues {
        buffer: BufferId,
        first: SemanticValueId,
        second: SemanticValueId,
    },
    /// A write carries generation 0 — the reserved unset sentinel (F2).
    UnsetValueGeneration { value: SemanticValueId },
    /// A second producer claims an existing (value, generation) (F2).
    DuplicateGenerationProducer {
        value: SemanticValueId,
        generation: u32,
    },
    /// A root names a launch that does not exist (F3).
    IllegalRoot { root: LaunchId },
    /// A root appears more than once (F3).
    DuplicateRoot { root: LaunchId },
    /// A dependency edge runs from a launch to itself (F3).
    SelfDependency { launch: LaunchId },
    /// A dependency edge names a launch that does not exist (F3).
    UnknownDependencyEndpoint { launch: LaunchId },
    /// The same dependency edge is declared twice (F3).
    DuplicateDependencyEdge {
        producer: LaunchId,
        consumer: LaunchId,
    },
    /// The carried dependency graph contains a cycle (F3).
    DependencyCycle { edge: DependencyEdge },
    /// A generation producer is not reachable from any root — a missing
    /// dependency (F3).
    MissingDependency {
        launch: LaunchId,
        value: SemanticValueId,
    },
    /// A relation row is malformed (primal equals companion, duplicate
    /// selected positions, …) (F4).
    IllegalRelationRow { primal: DefId, reason: &'static str },
    /// Two relation rows name the same primal with identical facts (F4).
    DuplicateRelationRow { primal: DefId },
    /// Two relation rows for the same primal disagree on a fact (F4).
    ContradictoryRelationRow {
        primal: DefId,
        field: &'static str,
        first: String,
        second: String,
    },
    /// Two initialization facts for the same buffer disagree (F5).
    ContradictoryInitialization {
        buffer: BufferId,
        first: String,
        second: String,
    },
    /// An observation names a launch that does not exist (F5/F6).
    ObservationOutOfRange { launch: LaunchId },
    /// An observation names a buffer no value is bound to (F6).
    UnknownObservationBuffer { buffer: BufferId },
    /// A result row has no explicit observation fact in the carried
    /// semantics — a writable intermediate exposed as a result without being
    /// declared observed (F6).
    ResultWithoutObservationFact {
        result: usize,
        buffer: BufferId,
        version: u32,
    },
    /// An observation fact has no declared result row — a readback the host
    /// would never be told to perform (F6).
    UndeclaredObservationFact {
        buffer: BufferId,
        version: u32,
        at_launch: LaunchId,
    },
}

impl fmt::Display for DeviceProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKernels => write!(f, "device program declares no kernels"),
            Self::EmptyLaunches => write!(f, "device program declares no launches"),
            Self::EmptyEntryName { kernel } => {
                write!(f, "kernel {kernel} has an empty entry name")
            }
            Self::Plan { error } => write!(f, "kernel plan validation failed: {error}"),
            Self::DuplicateBinding {
                kernel,
                group,
                binding,
            } => write!(
                f,
                "kernel {kernel} binds group {group} binding {binding} more than once"
            ),
            Self::ZeroVersion { buffer, version } => write!(
                f,
                "buffer {buffer} references version {version}; version 0 is reserved (unset)"
            ),
            Self::RoleAccessMismatch {
                buffer,
                role,
                access,
            } => write!(
                f,
                "buffer {buffer} has role {} but a slot accesses it as {access:?}",
                role.spelling()
            ),
            Self::LaunchOutOfRange {
                launch,
                kernel_index,
                kernel_count,
            } => write!(
                f,
                "launch {launch} references kernel {kernel_index} but the program has {kernel_count} kernels"
            ),
            Self::DuplicateLaunchId { launch } => {
                write!(f, "launch id {launch} appears more than once in the launch sequence")
            }
            Self::UnknownProducingLaunch { result, produced_by } => write!(
                f,
                "result {result} names producing launch {produced_by}, which does not exist"
            ),
            Self::ResultRole { result, role } => write!(
                f,
                "result {result} has role {}; results must be output or in-out",
                role.spelling()
            ),
            Self::ResultBufferRoleMismatch {
                result,
                role,
                buffer_role,
            } => write!(
                f,
                "result {result} has observation role {} but its buffer's identity role is {}",
                role.spelling(),
                buffer_role.spelling()
            ),
            Self::ResultNotProduced {
                result,
                produced_by,
                buffer,
                version,
            } => write!(
                f,
                "result {result} declares launch {produced_by} as producer of buffer {buffer} version {version}, but that launch does not write it"
            ),
            Self::BufferIdentityConflict {
                buffer,
                field,
                first,
                second,
            } => write!(
                f,
                "buffer {buffer} declares conflicting {field}: first {first}, second {second}"
            ),
            Self::VersionShapeConflict {
                buffer,
                version,
                first,
                second,
            } => write!(
                f,
                "buffer {buffer} version {version} has conflicting shapes: first {first}, second {second}"
            ),
            Self::SemanticIdentityConflict {
                value,
                field,
                first,
                second,
            } => write!(
                f,
                "semantic value {value} declares conflicting {field}: first {first}, second {second}"
            ),
            Self::SemanticOriginAliased {
                first,
                second,
                origin,
            } => write!(
                f,
                "semantic values {first} and {second} both derive from origin {origin}; distinct values must derive from distinct carried facts"
            ),
            Self::UnknownSemanticValue { value } => {
                write!(f, "semantic value {value} is referenced but never declared")
            }
            Self::DuplicateValueBinding { value, buffer } => write!(
                f,
                "semantic value {value} is bound to more than one buffer (including {buffer})"
            ),
            Self::BufferHoldsMultipleValues {
                buffer,
                first,
                second,
            } => write!(
                f,
                "buffer {buffer} holds semantic values {first} and {second}; one buffer holds one value"
            ),
            Self::UnsetValueGeneration { value } => write!(
                f,
                "a write of semantic value {value} carries generation 0 (the reserved unset sentinel)"
            ),
            Self::DuplicateGenerationProducer { value, generation } => write!(
                f,
                "semantic value {value} generation {generation} has a second producer; a later write is a new generation"
            ),
            Self::IllegalRoot { root } => {
                write!(f, "root {root} is not a declared launch")
            }
            Self::DuplicateRoot { root } => {
                write!(f, "root {root} is declared more than once")
            }
            Self::SelfDependency { launch } => write!(
                f,
                "dependency edge runs from launch {launch} to itself"
            ),
            Self::UnknownDependencyEndpoint { launch } => write!(
                f,
                "dependency edge names launch {launch}, which does not exist"
            ),
            Self::DuplicateDependencyEdge {
                producer,
                consumer,
            } => write!(
                f,
                "dependency edge {producer} → {consumer} is declared more than once"
            ),
            Self::DependencyCycle { edge } => write!(
                f,
                "dependency graph contains a cycle through launch {consumer} (edge {producer} → {consumer})",
                producer = edge.producer,
                consumer = edge.consumer,
            ),
            Self::MissingDependency { launch, value } => write!(
                f,
                "launch {launch} produces semantic value {value} but is not reachable from any root (missing dependency)"
            ),
            Self::IllegalRelationRow { primal, reason } => write!(
                f,
                "relation row for primal DefId({}) is illegal: {reason}",
                primal.0
            ),
            Self::DuplicateRelationRow { primal } => write!(
                f,
                "duplicate relation rows for primal DefId({}); duplicate or contradictory rows fail closed",
                primal.0
            ),
            Self::ContradictoryRelationRow {
                primal,
                field,
                first,
                second,
            } => write!(
                f,
                "relation rows for primal DefId({}) contradict on {field}: first {first}, second {second}",
                primal.0
            ),
            Self::ContradictoryInitialization {
                buffer,
                first,
                second,
            } => write!(
                f,
                "buffer {buffer} declares conflicting initialization policies: first {first}, second {second}"
            ),
            Self::ObservationOutOfRange { launch } => write!(
                f,
                "observation names launch {launch}, which does not exist"
            ),
            Self::UnknownObservationBuffer { buffer } => write!(
                f,
                "observation names buffer {buffer}, which no semantic value is bound to"
            ),
            Self::ResultWithoutObservationFact {
                result,
                buffer,
                version,
            } => write!(
                f,
                "result {result} exposes buffer {buffer} version {version} as a readback, but the carried semantics declare no observation fact for it; a writable intermediate or persistent state is a result only through an explicit observation fact (F6)"
            ),
            Self::UndeclaredObservationFact {
                buffer,
                version,
                at_launch,
            } => write!(
                f,
                "observation fact declares buffer {buffer} version {version} read back at launch {at_launch}, but no result row declares it (F6)"
            ),
        }
    }
}

impl std::error::Error for DeviceProgramError {}

impl fmt::Display for BufferId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BufferId({})", self.0)
    }
}

impl fmt::Display for LaunchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LaunchId({})", self.0)
    }
}

/// Whether a slot access is consistent with the buffer's program role.
///
/// Inputs are read-only; outputs are written (a read-write output slot is
/// also coherent — an in-place result); in-out intermediates may be read,
/// written, or both.
fn role_permits_access(role: BufferRole, access: MirKernelResourceAccess) -> bool {
    match role {
        BufferRole::Input => access == MirKernelResourceAccess::Read,
        BufferRole::Output => matches!(
            access,
            MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
        ),
        BufferRole::InOut => true,
    }
}

/// Record the first-seen identity for a buffer id; reject conflicting repeats.
fn record_identity(
    seen: &mut Vec<BufferIdentity>,
    identity: &BufferIdentity,
) -> Result<(), DeviceProgramError> {
    let Some(first) = seen.iter().find(|first| first.id == identity.id) else {
        seen.push(identity.clone());
        return Ok(());
    };
    if first.name != identity.name {
        return Err(DeviceProgramError::BufferIdentityConflict {
            buffer: identity.id,
            field: "name",
            first: first.name.clone(),
            second: identity.name.clone(),
        });
    }
    if first.role != identity.role {
        return Err(DeviceProgramError::BufferIdentityConflict {
            buffer: identity.id,
            field: "role",
            first: first.role.spelling().to_owned(),
            second: identity.role.spelling().to_owned(),
        });
    }
    if first.storage != identity.storage {
        return Err(DeviceProgramError::BufferIdentityConflict {
            buffer: identity.id,
            field: "storage",
            first: format!("{:?}", first.storage),
            second: format!("{:?}", identity.storage),
        });
    }
    if first.lifetime != identity.lifetime {
        return Err(DeviceProgramError::BufferIdentityConflict {
            buffer: identity.id,
            field: "lifetime",
            first: first.lifetime.spelling().to_owned(),
            second: identity.lifetime.spelling().to_owned(),
        });
    }
    Ok(())
}

/// Record the first-seen shape for a (buffer, version); reject conflicting
/// shapes (a shape change must be a new version — no in-place
/// reinterpretation).
fn record_shape(
    seen: &mut Vec<(BufferId, u32, MirType, u64)>,
    buffer: BufferId,
    version: &BufferVersion,
) -> Result<(), DeviceProgramError> {
    if let Some((_, _, first_ty, first_count)) =
        seen.iter().find(|(seen_buffer, seen_version, _, _)| {
            *seen_buffer == buffer && *seen_version == version.version
        })
    {
        if *first_ty != version.element_ty || *first_count != version.element_count {
            return Err(DeviceProgramError::VersionShapeConflict {
                buffer,
                version: version.version,
                first: shape_description(*first_ty, *first_count),
                second: shape_description(version.element_ty, version.element_count),
            });
        }
    } else {
        seen.push((
            buffer,
            version.version,
            version.element_ty,
            version.element_count,
        ));
    }
    Ok(())
}

/// Compact shape description for diagnostics.
fn shape_description(element_ty: MirType, element_count: u64) -> String {
    format!(
        "{element_count} elements of type {:?}",
        element_ty.semantic_id()
    )
}

/// Whether the named producing launch actually writes the result's version.
fn result_is_produced(program: &DeviceProgram, result: &ResultBuffer) -> bool {
    program
        .launches
        .iter()
        .find(|launch| launch.id == result.produced_by)
        .and_then(|launch| program.kernels.get(launch.kernel_index))
        .is_some_and(|kernel| {
            kernel.resources.iter().any(|resource| {
                resource.buffer.id == result.buffer.id
                    && resource.version.version == result.version.version
                    && matches!(
                        resource.access,
                        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                    )
            })
        })
}

/// Find or insert the registry entry for a buffer id.
fn registry_buffer_mut<'a>(
    buffers: &'a mut Vec<RegistryBuffer>,
    identity: &BufferIdentity,
) -> &'a mut RegistryBuffer {
    if let Some(index) = buffers
        .iter()
        .position(|entry| entry.identity.id == identity.id)
    {
        return &mut buffers[index];
    }
    buffers.push(RegistryBuffer {
        identity: identity.clone(),
        versions: Vec::new(),
    });
    let index = buffers.len() - 1;
    &mut buffers[index]
}

/// Find or insert the registry entry for a buffer version.
fn registry_version_mut<'a>(
    versions: &'a mut Vec<RegistryVersion>,
    version: &BufferVersion,
) -> &'a mut RegistryVersion {
    if let Some(index) = versions
        .iter()
        .position(|entry| entry.version.version == version.version)
    {
        return &mut versions[index];
    }
    versions.push(RegistryVersion {
        version: version.clone(),
        produced_by: Vec::new(),
        consumed_by: Vec::new(),
    });
    let index = versions.len() - 1;
    &mut versions[index]
}

#[cfg(test)]
#[path = "device_program_test.rs"]
mod tests;
