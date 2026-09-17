use crate::abi::{
    MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind,
    MirKernelResourceRole, MirKernelSignature, MirKernelWorkgroupCount, MirWorkgroupSize,
    collection_op_workgroup_count,
};
use crate::device_plan_compiler::LibraryOperation;
use crate::kernel_plan::CollectionKernelPlan;
use crate::layout::MirTensorStorageLayout;
use crate::{MirFunction, MirFunctionId, MirType};

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
    /// The producer-defined reduced-buffer projection (council-8 CB-3):
    /// present only when this version is the reduced output of an axis
    /// reduction — the mapping a keep-dims consumer uses to address the
    /// reduced buffer (see [`crate::abi::ReducedProjection`]). Carried here
    /// and on the wire so consumers never reconstruct the projection from
    /// element count.
    pub reduced_projection: Option<crate::abi::ReducedProjection>,
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
/// everything else dispatches `ceil(dispatch / workgroup)` per axis), then
/// corrected by the kernel plan's typed launch facts via
/// [`KernelLaunchPlan::from_signature_plan_and_function`] (decode M=1
/// batched-attention projections launch `ceil(dispatch / outputs)`).
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

    /// Plan-aware launch derivation (R4 repair): the shared rule above, then
    /// the kernel plan's typed launch facts override the grid.
    ///
    /// A decode (M=1) batched-attention projection computes
    /// [`crate::kernel_plan::BatchedAttentionPlan::decode_outputs_per_workgroup`]
    /// outputs per workgroup, so its grid is `ceil(dispatch / outputs)`
    /// workgroups — the generic `ceil(dispatch / workgroup)` law
    /// under-dispatches by the partition ratio (15 workgroups published 120
    /// of 960 logical Q). Emitters and launches must be built from the same
    /// plan; prefer this constructor whenever the plan is at hand.
    #[must_use]
    pub fn from_signature_plan_and_function(
        signature: &MirKernelSignature,
        plan: &crate::kernel_plan::CollectionKernelPlan,
        function: &MirFunction,
    ) -> Self {
        let mut launch = Self::from_signature_and_function(signature, function);
        if let crate::kernel_plan::CollectionKernelPlan::BatchedAttention(attention) = plan
            && let Some(outputs) = attention
                .decode_outputs_per_workgroup
                .filter(|outputs| *outputs > 0)
        {
            launch.workgroup_count.x = launch.dispatch_size.x.div_ceil(outputs);
        }
        launch
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
    /// Compiler-owned library operation, when a typed producer admitted one.
    ///
    /// This is deliberately carried beside the recipe rather than in the
    /// recipe payload: fusion owns the latter, while dispatch selection owns
    /// this bounded bridge fact. It is compiler-internal and is not serialized.
    pub library_operation: LibraryOperation,
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

/// One entry in the deterministic launch enumeration.
///
/// `launches` is a stable topological enumeration over the carried dependency
/// edges, not the dependency graph itself — the generalized form of the
/// WGSL chain descriptor's `chain`. Adjacency in the vector imposes no
/// semantic order between launches: only the carried producer-consumer edges
/// ([`BufferRegistry`] data-flow pairs, [`DeviceSemantics`] dependencies)
/// decide what must wait for what, and independent launches may execute in
/// any order or concurrently. A kernel may be launched multiple times
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

/// Program-unique identity of one prepared submission region.
///
/// Region identity is compiler-owned and static at compile time: the id is
/// minted deterministically by
/// [`DeviceSemantics::derive_prepared_regions`](crate::device_semantics::DeviceSemantics::derive_prepared_regions)
/// from the carried dependency graph — never recovered from names, bindings,
/// or launch positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionId(pub u32);

/// A prepared submission region (contract §Prepared submission regions).
///
/// One compiler-visible ordered group of member launches that can be encoded
/// and enqueued as a single host call (NGAB0-R1: one host call → one
/// prepared submission region) without CPU observation between its members.
/// Boundaries arise only from real sources — real dependencies, host
/// control, cancellation, effects, or declared observations — never from a
/// generic runtime wrapper and never to "match" a runtime interface.
/// `launches` is the member enqueue order (topological on the carried
/// dependency graph); the minimal region is the one-kernel case, not a
/// separate ABI shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRegion {
    /// Compiler-owned, static region identity.
    pub id: RegionId,
    /// Member launches in enqueue order.
    pub launches: Vec<LaunchId>,
}

/// The observation cadence of a result row (S5A-U1): WHEN a declared
/// observation is read back. The cadence is an explicit carried fact of the
/// observation — never derived from the buffer's shape, role, or lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObservationCadence {
    /// Read back at the producing launch's completion boundary within EVERY
    /// step of a `RepeatingStep` program (e.g. the loss trace).
    PerStep,
    /// Read back ONCE after the step loop at the final step's completion
    /// boundary (e.g. the final forward, final gradients, final params).
    EndOfRun,
}

impl ObservationCadence {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::PerStep => "per-step",
            Self::EndOfRun => "end-of-run",
        }
    }
}

/// An explicit result buffer (multi-output readback).
///
/// Multi-output results are declared explicitly: each result names the buffer
/// identity and version to observe, the launch that produced it, its role
/// (output or in-out — an in-place-updated buffer read back at an observation
/// point is a result too), and the **observation cadence** (S5A-U1): whether
/// the declared observation is read back every step (`PerStep` — e.g. the loss)
/// or once after the step loop (`EndOfRun` — e.g. the final forward, gradients,
/// params). The observation role must agree with the buffer's identity role
/// ([`DeviceProgram::validate`] enforces the cross-check), and the cadence
/// must be consistent with the buffer's lifetime class: a `PerStep` result
/// names an `ObservationPoint` buffer; an `EndOfRun` result names a `PerStep` or
/// `PerProgram` buffer — never read-only input state.
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
    /// The declared observation cadence (S5A-U1): `PerStep` (read back every
    /// step) or `EndOfRun` (read back once after the step loop).
    pub cadence: ObservationCadence,
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
/// - `launches`: the deterministic launch enumeration over the kernels (a
///   stable topological order; the dependency graph is carried separately).
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
    /// Deterministic launch enumeration (stable topological order, not the
    /// dependency graph).
    pub launches: Vec<LaunchUnit>,
    /// Program execution-lifetime regime.
    pub lifetime: DeviceProgramLifetime,
    /// Explicit multi-output result buffers.
    pub results: Vec<ResultBuffer>,
}
