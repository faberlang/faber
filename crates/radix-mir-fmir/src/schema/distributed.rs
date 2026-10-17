//! FMIR distributed execution section (MD2-W1).
//!
//! The optional `distributed` field of [`FmirDeviceSection`]
//! (schema/device.rs): the **wire mirror of the portable logical plan**
//! ([`WireDistributedExecutionPlan`] ↔ [`radix_mir::distributed::DistributedExecutionPlan`],
//! the `WireDeviceProgram`↔`DeviceProgram` pattern), the declared placement
//! facts, and the `logical_distributed_plan_hash` (MD2-H1's domain). It
//! carries **no bound plan** — a machine-specific bound-plan serialization
//! is MD3's.
//!
//! # Contract anchors (md2-delivery §MD2-W1)
//!
//! - **Optional sibling, absent for single-device packages** (MD-A15): a
//!   single-device image without the section decodes, admits, and
//!   runs unchanged; the section-level A10 exclusion is proven in
//!   `distributed_test.rs`. Old consumers reject distributed images via
//!   `deny_unknown_fields` (fail-closed, A7).
//! - **Own version ratchet** (`v` + [`WIRE_DISTRIBUTED_SECTION_VERSION`]),
//!   admitted fail-closed **before field interpretation** (the
//!   `admit_device_section` pattern, FC4). The nested
//!   `WIRE_DEVICE_PROGRAM_VERSION` and outer `PACKAGE_MIR_ARTIFACT_VERSION`
//!   stay untouched (the number is deliberately not restated here; mod.rs
//!   owns the artifact-version value).
//! - **Equivalence gate** (admission): re-derives `(program,
//!   declared_placement)` from the single-device wire program + the declared
//!   placement facts and compares **canonical bytes**
//!   ([`radix_mir::distributed::encode_logical_plan`]) with the **actually
//!   delivered plan** (reconstructed from the section). A mutated delivered
//!   plan (owner / edge / byte contract) or a mutated declared placement
//!   fails — the gate inspects the delivered bytes, never a self-declared
//!   hash (exit-gate bullet 4).
//!
//! # Element-type reconstruction contract
//!
//! The wire mirrors carry element types as the **same spelling strings** the
//! wire device program carries (`"f32"` — the campaign dtype surface). The
//! radix-mir canonical encoding (`encode_logical_plan`) writes element types
//! as numeric type ids, so admission reconstructs a synthetic radix-mir type
//! per spelling ([`mir_type_from_spelling`]) and the equivalence gate + hash
//! cross-check run over **wire-reconstructed plans** — a plan canonicalized
//! from the wire, never from a producer-local `TypeTable`. A producer must
//! therefore compute the section's `logical_distributed_plan_hash` over the
//! wire-reconstructed plan (this module's `wire_plan_to_plan`), not over a
//! producer-internal plan. This is a documented schema contract for MD3.

use serde::{Deserialize, Serialize};

use radix_mir::abi::{
    MirKernelDispatchSize, MirKernelResourceAccess, MirKernelWorkgroupCount, MirWorkgroupSize,
};
use radix_mir::abi::{PackedStorageLayout, ReducedProjection};
use radix_mir::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit,
    ObservationCadence, ResultBuffer,
};
use radix_mir::device_semantics::SemanticValueId;
use radix_mir::distributed::{
    BarrierId, BarrierOperation, CollectiveId, CollectiveKind, CollectiveOperation,
    DeclaredPlacement, DeviceProgramPartition, DistributedExecutionGraph, DistributedExecutionPlan,
    ExecutionCommitBoundary, ExecutionOperation, LaunchOperation, LogicalPlacement, PartitionId,
    PlacementConstraint, PlacementContract, PlacementFact, PlacementObjective, PlanVersion,
    ResultRef, TransferDirection, TransferId, TransferOperation, TransportPath,
};
use radix_mir::kernel_plan::{
    AxisReductionPlan, BarrierPhase, BarrierPoint, BatchedAttentionKind, BatchedAttentionPlan,
    CausalMaskedSoftmaxPlan, CollectionKernelPlan, GatherPlan, GroupedMatMulPlan,
    LayerNormalizationPlan, MatMulPlan, MatMulSharedMemory, OobPaddingPolicy, QuantizedMatMulPlan,
    ReduceOp, ReductionPlan, RmsNormalizationPlan, RopePlan, RowSoftmaxBatch, RowSoftmaxMask,
    RowSoftmaxPlan, SharedMemoryLayout, SsmConv1dPlan, SsmScanPlan, TransposePlan,
};
use radix_mir::layout::MirTensorStorageLayout;
use radix_mir::{MirFunctionId, MirScalarLayout, MirType};
use radix_types::TypeId;

use super::wire::{
    WireBarrierPhase, WireBarrierPoint, WireBatchedAttentionKind, WireBufferIdentity,
    WireBufferLifetime, WireBufferRole, WireBufferVersion, WireCollectionKernelPlan,
    WireDeviceProgram, WireKernelUnit, WireMatMulLayout, WireOobPaddingPolicy,
    WirePackedStorageLayout, WireProgramLifetime, WireReduceOp, WireResourceAccess,
    WireResultBuffer, WireSharedMemoryLayout, WireStorageLayout,
};
use crate::error::FmirImageError;

// ---------------------------------------------------------------------------
// The section + its wire mirror of the portable logical plan
// ---------------------------------------------------------------------------

/// The optional distributed execution section of an FMIR device image
/// (MD2-W1). Absent (`None`) for single-device packages (MD-A15) — the
/// independent single-device path. Present for distributed images; the
/// single-device run route rejects those fail-closed ("distributed execution
/// requires MD3") while admission admits them at load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmirDistributedExecutionSection {
    /// The section's OWN version ratchet (A7): admitted fail-closed before
    /// any field-level interpretation, independently of the nested
    /// device-program wire version and the outer image version.
    pub v: u32,
    /// The wire mirror of the portable logical plan (field-for-field).
    pub logical_plan: WireDistributedExecutionPlan,
    /// The declared placement facts the derivation consumes (value → buffer
    /// binding, ownership / replication contract, declared byte contract).
    pub declared_placement: Vec<WirePlacementFact>,
    /// MD2-H1's `logical_distributed_plan_hash` — `sha256:` over the
    /// wire-reconstructed plan's canonical bytes. Cross-checked at admission;
    /// the equivalence gate never trusts it (the gate inspects the delivered
    /// plan bytes).
    pub logical_distributed_plan_hash: String,
}

/// Wire mirror of [`radix_mir::distributed::DistributedExecutionPlan`] — the
/// portable, target-neutral logical plan. Never references a
/// `PhysicalDeviceId`; no bound plan rides this wire (MD3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDistributedExecutionPlan {
    /// The declared logical placement (facts + constraints + objectives).
    pub declared_placement: WireDeclaredPlacement,
    /// The derived per-device leaf partitions, ascending by partition id.
    pub partitions: Vec<WireDeviceProgramPartition>,
    /// The derived upper execution graph.
    pub graph: WireDistributedExecutionGraph,
    /// The plan shape version (stale-version checks, MD2-V1).
    pub version: u32,
}

/// Wire mirror of [`radix_mir::distributed::DeclaredPlacement`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDeclaredPlacement {
    /// One record per declared value placement, ascending by value id.
    pub placements: Vec<WireLogicalPlacement>,
    /// Declared hard constraints.
    pub constraints: Vec<WirePlacementConstraint>,
    /// Declared objectives.
    pub objectives: Vec<WirePlacementObjective>,
}

/// Wire mirror of [`radix_mir::distributed::LogicalPlacement`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireLogicalPlacement {
    /// The semantic value this placement declares for (F1 stable key).
    pub value: u32,
    /// The declared ownership / replication contract.
    pub contract: WirePlacementContract,
}

/// Wire mirror of [`radix_mir::distributed::PlacementContract`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum WirePlacementContract {
    /// Exactly one declared owner partition.
    Owned {
        /// The single owner partition.
        owner: u32,
    },
    /// Explicit coherent replication contract.
    Replicated {
        /// The primary replica — the broadcast source.
        primary: u32,
        /// The full replica set (primary plus every other replica).
        replicas: Vec<u32>,
    },
}

/// Wire mirror of [`radix_mir::distributed::PlacementConstraint`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum WirePlacementConstraint {
    /// Declared memory budget of one partition in bytes.
    PartitionBudgetBytes {
        /// The constrained partition.
        partition: u32,
        /// Declared budget in bytes.
        budget_bytes: u64,
    },
    /// Hard residency constraint.
    RequireOwner {
        /// The constrained semantic value.
        value: u32,
        /// The partition that must own it.
        partition: u32,
    },
}

/// Wire mirror of [`radix_mir::distributed::PlacementObjective`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WirePlacementObjective {
    /// Declared objective: minimize cross-partition transfer bytes.
    MinimizeTransferBytes,
    /// Declared objective: prefer serving values at their owner.
    MaximizeOwnerAffinity,
}

/// Wire mirror of [`radix_mir::distributed::PlacementFact`] — the declared
/// placement fact consumed by the deterministic derivation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WirePlacementFact {
    /// The semantic value this fact declares for (F1 stable key).
    pub value: u32,
    /// The device buffer holding the value (F1 binding; derivation input).
    pub buffer: u32,
    /// The declared ownership or explicit replication contract.
    pub contract: WirePlacementContract,
    /// The declared byte count of the value's content — the byte/shape
    /// contract the cross-device completeness gate checks.
    pub byte_count: u64,
}

/// Wire mirror of [`radix_mir::distributed::DeviceProgramPartition`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDeviceProgramPartition {
    /// Stable partition identity (ordinal locator only).
    pub id: u32,
    /// Kernel subset, by index into the program's kernel list (ascending,
    /// deduplicated).
    pub kernels: Vec<u64>,
    /// Launch subset, by stable launch id (program order).
    pub launches: Vec<u32>,
    /// Buffer subset, by stable buffer id (ascending).
    pub buffers: Vec<u32>,
    /// Result subset, by (buffer, version, producing launch).
    pub results: Vec<WireResultRef>,
    /// The declared placement records involving this partition (ascending by
    /// value id).
    pub placements: Vec<WireLogicalPlacement>,
}

/// Wire mirror of [`radix_mir::distributed::ResultRef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireResultRef {
    /// The observed buffer.
    pub buffer: u32,
    /// The observed content version.
    pub version: u32,
    /// The launch whose completion makes the observation valid.
    pub produced_by: u32,
}

/// Wire mirror of [`radix_mir::distributed::DistributedExecutionGraph`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDistributedExecutionGraph {
    /// Operations in deterministic plan order.
    pub operations: Vec<WireExecutionOperation>,
    /// The declared commit boundary of the plan.
    pub commit_boundary: WireExecutionCommitBoundary,
}

/// Wire mirror of [`radix_mir::distributed::ExecutionCommitBoundary`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireExecutionCommitBoundary {
    /// Barriers whose completion commits the value.
    pub barriers: Vec<u32>,
    /// Launches whose completion commits the value.
    pub launches: Vec<u32>,
}

/// Wire mirror of [`radix_mir::distributed::ExecutionOperation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum WireExecutionOperation {
    /// A launch of the semantic program on one partition.
    Launch(WireLaunchOperation),
    /// A host-staged cross-partition transfer.
    Transfer(WireTransferOperation),
    /// A collective (v1 admissible set: Broadcast only).
    Collective(WireCollectiveOperation),
    /// A barrier synchronizing its participant partitions.
    Barrier(WireBarrierOperation),
}

/// Wire mirror of [`radix_mir::distributed::LaunchOperation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireLaunchOperation {
    /// The semantic launch executed (stable launch id).
    pub launch: u32,
    /// The partition that runs the launch.
    pub partition: u32,
}

/// Wire mirror of [`radix_mir::distributed::TransferOperation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireTransferOperation {
    /// Stable transfer identity within the graph.
    pub id: u32,
    /// The semantic value whose bytes move (F1 key).
    pub value: u32,
    /// Source partition.
    pub source: u32,
    /// Destination partition.
    pub destination: u32,
    /// Byte count of the transfer (the value's declared byte contract).
    pub byte_count: u64,
    /// Direction (H2D / D2H / BIDI).
    pub direction: WireTransferDirection,
    /// Element-type spelling of the transferred content (the campaign dtype
    /// surface — the same spelling the wire device program's resources
    /// carry).
    pub element_ty: String,
    /// Storage layout of the transferred content.
    pub layout: WireStorageLayout,
    /// Transport path label — `host-staged` (the only admitted path).
    pub path_label: WireTransportPath,
    /// Producer generation — the content version the producer wrote.
    pub producer_generation: u32,
    /// Consumer generation — the content version the consumer reads.
    pub consumer_generation: u32,
    /// Completion boundary that commits the value.
    pub completion_boundary: WireExecutionCommitBoundary,
}

/// Wire mirror of [`radix_mir::distributed::TransferDirection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireTransferDirection {
    /// Host → device.
    H2D,
    /// Device → host.
    D2H,
    /// Both directions concurrently (the host-staged device-to-device move).
    BIDI,
}

/// Wire mirror of [`radix_mir::distributed::CollectiveOperation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCollectiveOperation {
    /// Stable collective identity within the graph.
    pub id: u32,
    /// The semantic value the collective moves.
    pub value: u32,
    /// The admissible-kind discriminant (v1 = Broadcast only).
    pub kind: WireCollectiveKind,
    /// Source partition (the owner / primary replica).
    pub source: u32,
    /// The participant partition set (source plus all consumers).
    pub participants: Vec<u32>,
    /// Byte count of the value moved.
    pub byte_count: u64,
}

/// Wire mirror of [`radix_mir::distributed::CollectiveKind`] — the v1
/// admissible set (Broadcast only; the closed enum has no other variant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum WireCollectiveKind {
    /// Owner partition → all consumers.
    Broadcast,
}

/// Wire mirror of [`radix_mir::distributed::BarrierOperation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBarrierOperation {
    /// Stable barrier identity within the graph.
    pub id: u32,
    /// Participant partition set.
    pub partitions: Vec<u32>,
}

/// Wire mirror of [`radix_mir::distributed::TransportPath`] — `host-staged`
/// is the only admitted path (silent host staging forbidden).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireTransportPath {
    /// Pinned host memory ↔ device over `PCIe` — the admitted path.
    HostStaged,
}

// ---------------------------------------------------------------------------
// Wire → radix-mir reconstruction (the equivalence-gate substrate)
// ---------------------------------------------------------------------------
//
// These reconstruct the radix-mir plan / program / facts from the wire so
// admission can run the V1 validation gate and the equivalence gate over the
// real `DistributedExecutionPlan` and its canonical encoding. The element
// types are reconstructed from the wire's spelling strings via
// [`mir_type_from_spelling`] — identical spellings map to identical types, so
// the re-derived expected plan and the delivered plan canonicalize
// consistently (see the module docs for the contract).

/// Reconstruct the radix-mir type of an element-type spelling (the campaign
/// dtype surface). Unknown spellings fail closed — never a silent default.
fn mir_type_from_spelling(spelling: &str) -> Result<MirType, FmirImageError> {
    match spelling {
        // The campaign dtype surface pins f32 (S1-1). The synthetic type id
        // is a reconstruction constant: both the re-derived expected plan
        // and the delivered plan canonicalize through it, and the section's
        // declared hash is defined over the wire-reconstructed plan.
        "f32" => Ok(MirType::semantic(TypeId(1))),
        other => Err(FmirImageError::DistributedSectionInvalid {
            detail: format!(
                "distributed plan element type `{other}` is outside the campaign dtype surface (f32)"
            ),
        }),
    }
}

/// The wire spelling of an element type (mirrors the wire device program's
/// dtype spelling; the campaign surface is f32).
#[must_use]
pub fn spelling_from_mir_type(_ty: MirType) -> String {
    "f32".to_owned()
}

fn wire_role_to_role(role: WireBufferRole) -> BufferRole {
    match role {
        WireBufferRole::Input => BufferRole::Input,
        WireBufferRole::Output => BufferRole::Output,
        WireBufferRole::InOut => BufferRole::InOut,
    }
}

fn wire_lifetime_to_lifetime(lifetime: WireBufferLifetime) -> BufferLifetime {
    match lifetime {
        WireBufferLifetime::PerProgram => BufferLifetime::PerProgram,
        WireBufferLifetime::PerStep => BufferLifetime::PerStep,
        WireBufferLifetime::ObservationPoint => BufferLifetime::ObservationPoint,
    }
}

fn wire_storage_to_storage(storage: WireStorageLayout) -> MirTensorStorageLayout {
    match storage {
        WireStorageLayout::HostOwned => MirTensorStorageLayout::HostOwned,
        WireStorageLayout::DeviceHandle => MirTensorStorageLayout::DeviceHandle,
        // MLC-4: the wire marker round-trips to the sparse layout fact.
        WireStorageLayout::Sparse => MirTensorStorageLayout::Sparse,
    }
}

fn wire_access_to_access(access: WireResourceAccess) -> MirKernelResourceAccess {
    match access {
        WireResourceAccess::Read => MirKernelResourceAccess::Read,
        WireResourceAccess::Write => MirKernelResourceAccess::Write,
        WireResourceAccess::ReadWrite => MirKernelResourceAccess::ReadWrite,
    }
}

fn wire_buffer_identity(identity: &WireBufferIdentity) -> BufferIdentity {
    BufferIdentity {
        id: BufferId(identity.id),
        name: identity.name.clone(),
        role: wire_role_to_role(identity.role),
        storage: wire_storage_to_storage(identity.storage),
        lifetime: wire_lifetime_to_lifetime(identity.lifetime),
    }
}

fn wire_buffer_version(version: &WireBufferVersion) -> Result<BufferVersion, FmirImageError> {
    Ok(BufferVersion {
        version: version.version,
        element_ty: mir_type_from_spelling(&version.element_ty)?,
        element_count: version.element_count,
        // The carried parameter shape vector (the wire-shape widening) rides
        // the wire version explicitly — the reverse arm maps it, never
        // defaults it.
        shape: version.shape.clone(),
        // The carried reduced-resource projection (council-8 CB-3) rides the
        // wire version explicitly — the reverse arm maps it, never defaults
        // it.
        reduced_projection: version
            .reduced_projection
            .map(|projection| ReducedProjection {
                axis_extent: projection.axis_extent,
                inner_stride: projection.inner_stride,
            }),
    })
}

fn wire_launch_plan(launch: &super::wire::WireKernelLaunchPlan) -> KernelLaunchPlan {
    KernelLaunchPlan {
        workgroup: MirWorkgroupSize {
            x: launch.workgroup.x,
            y: launch.workgroup.y,
            z: launch.workgroup.z,
        },
        dispatch_size: MirKernelDispatchSize {
            x: launch.dispatch_size.x,
            y: launch.dispatch_size.y,
            z: launch.dispatch_size.z,
        },
        workgroup_count: MirKernelWorkgroupCount {
            x: launch.workgroup_count.x,
            y: launch.workgroup_count.y,
            z: launch.workgroup_count.z,
        },
    }
}

fn wire_shared_layout(layout: &WireSharedMemoryLayout) -> SharedMemoryLayout {
    SharedMemoryLayout {
        element_byte_width: layout.element_byte_width as usize,
        slot_count: layout.slot_count,
        buffer_name: layout.buffer_name.clone(),
    }
}

fn wire_barrier_point(point: &WireBarrierPoint) -> BarrierPoint {
    BarrierPoint {
        after: match point.after {
            WireBarrierPhase::SharedMemoryLoad => BarrierPhase::SharedMemoryLoad,
            WireBarrierPhase::ReductionStep => BarrierPhase::ReductionStep,
            WireBarrierPhase::InnerProductStep => BarrierPhase::InnerProductStep,
        },
    }
}

fn wire_oob_padding(padding: WireOobPaddingPolicy) -> OobPaddingPolicy {
    match padding {
        WireOobPaddingPolicy::ZeroFill => OobPaddingPolicy::ZeroFill,
    }
}

fn wire_matmul_layout(layout: WireMatMulLayout) -> MirScalarLayout {
    match layout {
        WireMatMulLayout::F32 => MirScalarLayout::F32,
        WireMatMulLayout::Bf16 => MirScalarLayout::Bf16,
    }
}

fn wire_reduce_op(op: WireReduceOp) -> ReduceOp {
    match op {
        WireReduceOp::Sum => ReduceOp::Sum,
        WireReduceOp::Mean => ReduceOp::Mean,
        WireReduceOp::Max => ReduceOp::Max,
        WireReduceOp::Min => ReduceOp::Min,
    }
}

/// Field-for-field reverse of the kernel-plan mirror: `WireCollectionKernelPlan`
/// → `CollectionKernelPlan`. The `RankExtensionAdd` wire variant is an
/// elementwise kernel whose broadcast fact rides the S6-C2 ABI fact, not the
/// `CollectionKernelPlan` — it reconstructs as `Elementwise` (the plan-level
/// recipe identity is preserved; the carried fact lives on the ABI surface).
fn wire_kernel_plan(plan: &WireCollectionKernelPlan) -> CollectionKernelPlan {
    match plan {
        WireCollectionKernelPlan::Elementwise | WireCollectionKernelPlan::RankExtensionAdd(_) => {
            CollectionKernelPlan::Elementwise
        }
        WireCollectionKernelPlan::TiledMatMul(plan) => wire_tiled_matmul_plan(plan),
        // U1 head-axis: the batched tiled-matmul image (leading head axes;
        // the argument's batch dims a prefix of the receiver's). A distinct
        // variant — not a defaulted field on the rank-2 plan — so a
        // batched fact can never decode as rank-2 by omission.
        WireCollectionKernelPlan::BatchedTiledMatMul(plan) => wire_batched_tiled_matmul_plan(plan),
        WireCollectionKernelPlan::TreeReduction(plan) => wire_tree_reduction_plan(plan),
        WireCollectionKernelPlan::Transpose(plan) => wire_transpose_plan(plan),
        // U3 head-axis: the batched transpose image (leading head axes
        // over the swapped trailing [M, N]).
        WireCollectionKernelPlan::BatchedTranspose(plan) => wire_batched_transpose_plan(plan),
        WireCollectionKernelPlan::AxisReduction(plan) => wire_axis_reduction_plan(plan),
        WireCollectionKernelPlan::RowSoftmax(plan) => {
            CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan {
                axis: plan.axis,
                // The legacy wire image carries no batch axes — the
                // ordinary rank-1/2 law. Batched images ride the dedicated
                // `BatchedRowSoftmax` variant, never a silent default.
                batched: None,
            })
        }
        // U2 head-axis: the batched row-softmax image (leading head/query
        // axes; the additive mask operand kept as a broadcast count).
        WireCollectionKernelPlan::BatchedRowSoftmax(plan) => wire_batched_row_softmax_plan(plan),
        WireCollectionKernelPlan::LayerNormalization(plan) => {
            CollectionKernelPlan::LayerNormalization(LayerNormalizationPlan { axis: plan.axis })
        }
        // GI3-1: the inference-recipe wire mirrors reconstruct field-for-field
        // (the S6-C2 mirror precedent; mechanical, no semantic facts ride the
        // mirror beyond the plan fields).
        WireCollectionKernelPlan::Gather(plan) => CollectionKernelPlan::Gather(GatherPlan {
            table_rows: plan.table_rows,
            table_cols: plan.table_cols,
            id_count: plan.id_count,
        }),
        WireCollectionKernelPlan::RmsNormalization(plan) => {
            CollectionKernelPlan::RmsNormalization(RmsNormalizationPlan {
                axis: plan.axis,
                epsilon_bits: plan.epsilon_bits,
                width: plan.width,
            })
        }
        WireCollectionKernelPlan::Rope(plan) => CollectionKernelPlan::Rope(RopePlan {
            pos: plan.pos,
            dim: plan.dim,
            width: plan.width,
            per_row: plan.per_row,
            rows: plan.rows,
            rotate_half: plan.rotate_half,
        }),
        WireCollectionKernelPlan::CausalMaskedSoftmax(plan) => {
            CollectionKernelPlan::CausalMaskedSoftmax(CausalMaskedSoftmaxPlan {
                rows: plan.rows,
                cols: plan.cols,
            })
        }
        // R-PACK-01C: packed-kernel wire mirrors reconstruct field-for-field
        // (GI3-1 F1 mechanical-mirror discipline; no extra semantic facts).
        WireCollectionKernelPlan::QuantizedMatMul(plan) => wire_quantized_matmul_plan(plan),
        WireCollectionKernelPlan::GroupedMatMul(plan) => wire_grouped_matmul_plan(plan),
        WireCollectionKernelPlan::BatchedAttention(plan) => wire_batched_attention_plan(plan),
        WireCollectionKernelPlan::SsmConv1d(plan) => {
            CollectionKernelPlan::SsmConv1d(SsmConv1dPlan {
                length: plan.length,
                channels: plan.channels,
                kernel_width: plan.kernel_width,
                layout: wire_packed_layout(plan.layout),
            })
        }
        WireCollectionKernelPlan::SsmScan(plan) => CollectionKernelPlan::SsmScan(SsmScanPlan {
            length: plan.length,
            state_dim: plan.state_dim,
            layout: wire_packed_layout(plan.layout),
        }),
        WireCollectionKernelPlan::SingleLane => CollectionKernelPlan::SingleLane,
        WireCollectionKernelPlan::LaneGrid(plan) => CollectionKernelPlan::LaneGrid {
            source_resource: usize::try_from(plan.source_resource).unwrap_or(usize::MAX),
            rank: plan.rank,
        },
        WireCollectionKernelPlan::GridReduce(plan) => wire_grid_reduce_plan(plan),
    }
}

fn wire_tiled_matmul_plan(plan: &super::wire::WireMatMulPlan) -> CollectionKernelPlan {
    CollectionKernelPlan::TiledMatMul(MatMulPlan {
        dims: radix_mir::MatMulDims::Literal {
            m: plan.m,
            k: plan.k,
            n: plan.n,
        },
        m: plan.m,
        k: plan.k,
        n: plan.n,
        left_layout: wire_matmul_layout(plan.left_layout),
        right_layout: wire_matmul_layout(plan.right_layout),
        tile: plan.tile,
        workgroup_x: plan.workgroup_x,
        workgroup_y: plan.workgroup_y,
        shared_memory: MatMulSharedMemory {
            shared_a: wire_shared_layout(&plan.shared_memory.shared_a),
            shared_b: wire_shared_layout(&plan.shared_memory.shared_b),
        },
        barriers: plan.barriers.iter().map(wire_barrier_point).collect(),
        oob_padding: wire_oob_padding(plan.oob_padding),
        // The legacy wire plan carries no batch axes — an ordinary
        // rank-2 image. Batched images ride the dedicated
        // `BatchedTiledMatMul` variant, never a silent default.
        batch: 1,
        right_batch: 1,
    })
}

fn wire_batched_tiled_matmul_plan(
    plan: &super::wire::WireBatchedMatMulPlan,
) -> CollectionKernelPlan {
    CollectionKernelPlan::TiledMatMul(MatMulPlan {
        dims: radix_mir::MatMulDims::Literal {
            m: plan.m,
            k: plan.k,
            n: plan.n,
        },
        m: plan.m,
        k: plan.k,
        n: plan.n,
        left_layout: wire_matmul_layout(plan.left_layout),
        right_layout: wire_matmul_layout(plan.right_layout),
        tile: plan.tile,
        workgroup_x: plan.workgroup_x,
        workgroup_y: plan.workgroup_y,
        shared_memory: MatMulSharedMemory {
            shared_a: wire_shared_layout(&plan.shared_memory.shared_a),
            shared_b: wire_shared_layout(&plan.shared_memory.shared_b),
        },
        barriers: plan.barriers.iter().map(wire_barrier_point).collect(),
        oob_padding: wire_oob_padding(plan.oob_padding),
        batch: plan.batch,
        right_batch: plan.right_batch,
    })
}

fn wire_tree_reduction_plan(plan: &super::wire::WireReductionPlan) -> CollectionKernelPlan {
    CollectionKernelPlan::TreeReduction(ReductionPlan {
        op: wire_reduce_op(plan.op),
        length: plan.length,
        workgroup_x: plan.workgroup_x,
        partials: plan.partials,
        shared_memory: wire_shared_layout(&plan.shared_memory),
        barriers: plan.barriers.iter().map(wire_barrier_point).collect(),
        oob_padding: wire_oob_padding(plan.oob_padding),
    })
}

fn wire_transpose_plan(plan: &super::wire::WireTransposePlan) -> CollectionKernelPlan {
    CollectionKernelPlan::Transpose(TransposePlan {
        m: plan.m,
        n: plan.n,
        workgroup_x: plan.workgroup_x,
        dispatch_x: plan.dispatch_x,
        // The legacy wire image carries no batch axes — the
        // ordinary rank-2 law. Batched images ride the dedicated
        // `BatchedTranspose` variant, never a silent default.
        batched: None,
    })
}

fn wire_batched_transpose_plan(
    plan: &super::wire::WireBatchedTransposePlan,
) -> CollectionKernelPlan {
    CollectionKernelPlan::Transpose(TransposePlan {
        m: plan.m,
        n: plan.n,
        workgroup_x: plan.workgroup_x,
        dispatch_x: plan.dispatch_x,
        batched: Some(plan.batch),
    })
}

fn wire_axis_reduction_plan(plan: &super::wire::WireAxisReductionPlan) -> CollectionKernelPlan {
    CollectionKernelPlan::AxisReduction(AxisReductionPlan {
        op: wire_reduce_op(plan.op),
        axis: plan.axis,
        // The carried reduced-resource projection maps back
        // field-for-field (council-8 CB-3) — the reverse arm
        // reconstructs the producer fact, never infers it.
        projection: ReducedProjection {
            axis_extent: plan.projection.axis_extent,
            inner_stride: plan.projection.inner_stride,
        },
    })
}

fn wire_batched_row_softmax_plan(
    plan: &super::wire::WireBatchedRowSoftmaxPlan,
) -> CollectionKernelPlan {
    CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan {
        axis: plan.axis,
        batched: Some(RowSoftmaxBatch {
            batch: plan.batch,
            width: plan.width,
            mask: plan.mask_count.map(|count| RowSoftmaxMask { count }),
        }),
    })
}

fn wire_quantized_matmul_plan(plan: &super::wire::WireQuantizedMatMulPlan) -> CollectionKernelPlan {
    CollectionKernelPlan::QuantizedMatMul(QuantizedMatMulPlan {
        m: plan.m,
        k: plan.k,
        n: plan.n,
        layout: wire_packed_layout(plan.layout),
        tile: plan.tile,
        workgroup_x: plan.workgroup_x,
        workgroup_y: plan.workgroup_y,
    })
}

fn wire_grouped_matmul_plan(plan: &super::wire::WireGroupedMatMulPlan) -> CollectionKernelPlan {
    CollectionKernelPlan::GroupedMatMul(GroupedMatMulPlan {
        m: plan.m,
        k: plan.k,
        n: plan.n,
        groups: plan.groups,
        layout: wire_packed_layout(plan.layout),
        tile: plan.tile,
        workgroup_x: plan.workgroup_x,
        workgroup_y: plan.workgroup_y,
    })
}

fn wire_batched_attention_plan(
    plan: &super::wire::WireBatchedAttentionPlan,
) -> CollectionKernelPlan {
    CollectionKernelPlan::BatchedAttention(BatchedAttentionPlan {
        kind: match plan.kind {
            WireBatchedAttentionKind::ProjectionQ => BatchedAttentionKind::ProjectionQ,
            WireBatchedAttentionKind::ProjectionK => BatchedAttentionKind::ProjectionK,
            WireBatchedAttentionKind::ProjectionV => BatchedAttentionKind::ProjectionV,
            WireBatchedAttentionKind::Scores => BatchedAttentionKind::Scores,
            WireBatchedAttentionKind::Softmax => BatchedAttentionKind::Softmax,
            WireBatchedAttentionKind::ContextSequenceMajor => {
                BatchedAttentionKind::ContextSequenceMajor
            }
        },
        kv_heads: plan.kv_heads,
        q_per_kv: plan.q_per_kv,
        seq: plan.seq,
        head_dim: plan.head_dim,
        hidden: plan.hidden,
        layout: plan.layout.map(wire_packed_layout),
        rotate_half: plan.rotate_half,
        output_strides: plan.output_strides,
        scale_bits: plan.scale_bits,
        decode_outputs_per_workgroup: plan.decode_outputs_per_workgroup,
    })
}

fn wire_grid_reduce_plan(plan: &super::wire::WireGridReducePlan) -> CollectionKernelPlan {
    CollectionKernelPlan::GridReduce {
        source_resource: usize::try_from(plan.source_resource).unwrap_or(usize::MAX),
        rank: plan.rank,
        workgroup: plan.workgroup,
        partials_cap: plan.partials_cap,
        partial_layout: radix_mir::kernel_plan::GridReducePartialLayout {
            value_bytes: plan.partial_value_bytes,
            coordinates: plan.partial_coordinates,
            seen: plan.partial_seen,
        },
    }
}

fn wire_packed_layout(layout: WirePackedStorageLayout) -> PackedStorageLayout {
    match layout {
        WirePackedStorageLayout::F32 => PackedStorageLayout::F32,
        WirePackedStorageLayout::Bf16 => PackedStorageLayout::Bf16,
        WirePackedStorageLayout::Q8_0 => PackedStorageLayout::Q8_0,
        WirePackedStorageLayout::Q4_K => PackedStorageLayout::Q4_K,
        WirePackedStorageLayout::Q5_K => PackedStorageLayout::Q5_K,
        WirePackedStorageLayout::Q6_K => PackedStorageLayout::Q6_K,
        WirePackedStorageLayout::Q5_0 => PackedStorageLayout::Q5_0,
    }
}

/// Decode the legacy name-keyed Elementwise discriminant off the wire into
/// the canonical typed tag, exactly once, at this boundary (PD-29,
/// Option A). The persisted entry name is the wire ABI; the
/// predecessor contract applies verbatim: `swiglu*` selects `SwiGlu`,
/// `*residual*` and `sum_o_*` select `ResidualRmsNorm`. In-memory selection
/// stays typed-only.
fn wire_library_operation(
    kernel: &WireKernelUnit,
) -> radix_mir::device_plan_compiler::LibraryOperation {
    use radix_mir::device_plan_compiler::LibraryOperation;
    if !matches!(kernel.plan, WireCollectionKernelPlan::Elementwise) {
        return LibraryOperation::Generated;
    }
    let entry = kernel.entry.as_str();
    if entry.starts_with("swiglu") {
        LibraryOperation::SwiGlu
    } else if entry.contains("residual") || entry.starts_with("sum_o_") {
        LibraryOperation::ResidualRmsNorm
    } else {
        LibraryOperation::Generated
    }
}

fn wire_resource(
    resource: &super::wire::WireDeviceResource,
) -> Result<DeviceResource, FmirImageError> {
    Ok(DeviceResource {
        buffer: wire_buffer_identity(&resource.buffer),
        version: wire_buffer_version(&resource.version)?,
        binding: Binding {
            group: resource.binding.group,
            binding: resource.binding.binding,
        },
        access: wire_access_to_access(resource.access),
    })
}

fn wire_kernel(kernel: &WireKernelUnit) -> Result<KernelUnit, FmirImageError> {
    Ok(KernelUnit {
        function: MirFunctionId(kernel.function),
        entry: kernel.entry.clone(),
        library_operation: wire_library_operation(kernel),
        plan: wire_kernel_plan(&kernel.plan),
        resources: kernel
            .resources
            .iter()
            .map(wire_resource)
            .collect::<Result<Vec<_>, FmirImageError>>()?,
        launch: wire_launch_plan(&kernel.launch),
    })
}

fn wire_result(result: &WireResultBuffer) -> Result<ResultBuffer, FmirImageError> {
    Ok(ResultBuffer {
        buffer: wire_buffer_identity(&result.buffer),
        version: wire_buffer_version(&result.version)?,
        role: wire_role_to_role(result.role),
        produced_by: LaunchId(result.produced_by),
        cadence: match result.observation.cadence {
            super::wire::WireObservationCadence::PerStep => ObservationCadence::PerStep,
            super::wire::WireObservationCadence::EndOfRun => ObservationCadence::EndOfRun,
        },
    })
}

/// Reconstruct the single-device program (derivation-relevant facts) from the
/// wire — the equivalence gate's program substrate. The F1–F7 carried facts
/// (semantic values, generations, roots, dependencies, relations,
/// initializations) do not ride the `DeviceProgram` and are not needed by
/// the deterministic derivation.
pub(crate) fn wire_program_to_program(
    wire: &WireDeviceProgram,
) -> Result<DeviceProgram, FmirImageError> {
    Ok(DeviceProgram {
        kernels: wire
            .kernels
            .iter()
            .map(wire_kernel)
            .collect::<Result<Vec<_>, FmirImageError>>()?,
        launches: wire
            .launches
            .iter()
            .map(|launch| LaunchUnit {
                id: LaunchId(launch.id),
                kernel_index: launch.kernel_index as usize,
            })
            .collect(),
        lifetime: match wire.lifetime {
            WireProgramLifetime::SingleRun => DeviceProgramLifetime::SingleRun,
            WireProgramLifetime::RepeatingStep(_) => DeviceProgramLifetime::RepeatingStep,
        },
        results: wire
            .results
            .iter()
            .map(wire_result)
            .collect::<Result<Vec<_>, FmirImageError>>()?,
    })
}

fn wire_contract(contract: &WirePlacementContract) -> PlacementContract {
    match contract {
        WirePlacementContract::Owned { owner } => PlacementContract::Owned {
            owner: PartitionId(*owner),
        },
        WirePlacementContract::Replicated { primary, replicas } => PlacementContract::Replicated {
            primary: PartitionId(*primary),
            replicas: replicas.iter().map(|id| PartitionId(*id)).collect(),
        },
    }
}

fn wire_placement(placement: &WireLogicalPlacement) -> LogicalPlacement {
    LogicalPlacement {
        value: SemanticValueId(placement.value),
        contract: wire_contract(&placement.contract),
    }
}

/// Reconstruct the declared placement facts from the wire (derivation input).
#[must_use]
pub fn wire_facts_to_facts(facts: &[WirePlacementFact]) -> Vec<PlacementFact> {
    facts
        .iter()
        .map(|fact| PlacementFact {
            value: SemanticValueId(fact.value),
            buffer: BufferId(fact.buffer),
            contract: wire_contract(&fact.contract),
            byte_count: fact.byte_count,
        })
        .collect()
}

fn wire_constraint(constraint: &WirePlacementConstraint) -> PlacementConstraint {
    match constraint {
        WirePlacementConstraint::PartitionBudgetBytes {
            partition,
            budget_bytes,
        } => PlacementConstraint::PartitionBudgetBytes {
            partition: PartitionId(*partition),
            budget_bytes: *budget_bytes,
        },
        WirePlacementConstraint::RequireOwner { value, partition } => {
            PlacementConstraint::RequireOwner {
                value: SemanticValueId(*value),
                partition: PartitionId(*partition),
            }
        }
    }
}

fn wire_objective(objective: WirePlacementObjective) -> PlacementObjective {
    match objective {
        WirePlacementObjective::MinimizeTransferBytes => PlacementObjective::MinimizeTransferBytes,
        WirePlacementObjective::MaximizeOwnerAffinity => PlacementObjective::MaximizeOwnerAffinity,
    }
}

/// The declared constraints/objectives ride the delivered plan itself (they
/// are not derivation inputs, md2-delivery §MD2-D1), so both the delivered
/// reconstruction and the re-derived expected plan copy them from the wire's
/// declared placement.
fn wire_constraints(wire: &WireDeclaredPlacement) -> Vec<PlacementConstraint> {
    wire.constraints.iter().map(wire_constraint).collect()
}

fn wire_objectives(wire: &WireDeclaredPlacement) -> Vec<PlacementObjective> {
    wire.objectives
        .iter()
        .map(|objective| wire_objective(*objective))
        .collect()
}

fn wire_result_ref(result: &WireResultRef) -> ResultRef {
    ResultRef {
        buffer: BufferId(result.buffer),
        version: result.version,
        produced_by: LaunchId(result.produced_by),
    }
}

fn wire_boundary(boundary: &WireExecutionCommitBoundary) -> ExecutionCommitBoundary {
    ExecutionCommitBoundary {
        barriers: boundary.barriers.iter().map(|id| BarrierId(*id)).collect(),
        launches: boundary.launches.iter().map(|id| LaunchId(*id)).collect(),
    }
}

fn wire_operation(
    operation: &WireExecutionOperation,
) -> Result<ExecutionOperation, FmirImageError> {
    match operation {
        WireExecutionOperation::Launch(launch) => Ok(ExecutionOperation::Launch(LaunchOperation {
            launch: LaunchId(launch.launch),
            partition: PartitionId(launch.partition),
        })),
        WireExecutionOperation::Transfer(transfer) => {
            Ok(ExecutionOperation::Transfer(TransferOperation {
                id: TransferId(transfer.id),
                value: SemanticValueId(transfer.value),
                source: PartitionId(transfer.source),
                destination: PartitionId(transfer.destination),
                byte_count: transfer.byte_count,
                direction: match transfer.direction {
                    WireTransferDirection::H2D => TransferDirection::H2D,
                    WireTransferDirection::D2H => TransferDirection::D2H,
                    WireTransferDirection::BIDI => TransferDirection::BIDI,
                },
                element_ty: mir_type_from_spelling(&transfer.element_ty)?,
                layout: wire_storage_to_storage(transfer.layout),
                path_label: match transfer.path_label {
                    WireTransportPath::HostStaged => TransportPath::HostStaged,
                },
                producer_generation: transfer.producer_generation,
                consumer_generation: transfer.consumer_generation,
                completion_boundary: wire_boundary(&transfer.completion_boundary),
            }))
        }
        WireExecutionOperation::Collective(collective) => {
            Ok(ExecutionOperation::Collective(CollectiveOperation {
                id: CollectiveId(collective.id),
                value: SemanticValueId(collective.value),
                kind: match collective.kind {
                    WireCollectiveKind::Broadcast => CollectiveKind::Broadcast,
                },
                source: PartitionId(collective.source),
                participants: collective
                    .participants
                    .iter()
                    .map(|id| PartitionId(*id))
                    .collect(),
                byte_count: collective.byte_count,
            }))
        }
        WireExecutionOperation::Barrier(barrier) => {
            Ok(ExecutionOperation::Barrier(BarrierOperation {
                id: BarrierId(barrier.id),
                partitions: barrier
                    .partitions
                    .iter()
                    .map(|id| PartitionId(*id))
                    .collect(),
            }))
        }
    }
}

/// Reconstruct the delivered logical plan from the wire mirror
/// (field-for-field inverse of [`plan_to_wire_plan`]).
pub(crate) fn wire_plan_to_plan(
    wire: &WireDistributedExecutionPlan,
) -> Result<DistributedExecutionPlan, FmirImageError> {
    let declared_placement = DeclaredPlacement {
        placements: wire
            .declared_placement
            .placements
            .iter()
            .map(wire_placement)
            .collect(),
        constraints: wire_constraints(&wire.declared_placement),
        objectives: wire_objectives(&wire.declared_placement),
    };
    let partitions = wire
        .partitions
        .iter()
        .map(|partition| {
            Ok(DeviceProgramPartition {
                id: PartitionId(partition.id),
                kernels: partition
                    .kernels
                    .iter()
                    .map(|index| {
                        usize::try_from(*index).map_err(|_| {
                            FmirImageError::DistributedSectionInvalid {
                                detail: format!(
                                    "partition {} references kernel index {index}, which does not fit the host kernel-index space",
                                    partition.id
                                ),
                            }
                        })
                    })
                    .collect::<Result<Vec<_>, FmirImageError>>()?,
                launches: partition
                    .launches
                    .iter()
                    .map(|id| LaunchId(*id))
                    .collect(),
                buffers: partition
                    .buffers
                    .iter()
                    .map(|id| BufferId(*id))
                    .collect(),
                results: partition.results.iter().map(wire_result_ref).collect(),
                placements: partition
                    .placements
                    .iter()
                    .map(wire_placement)
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, FmirImageError>>()?;
    let graph = DistributedExecutionGraph {
        operations: wire
            .graph
            .operations
            .iter()
            .map(wire_operation)
            .collect::<Result<Vec<_>, FmirImageError>>()?,
        commit_boundary: wire_boundary(&wire.graph.commit_boundary),
    };
    Ok(DistributedExecutionPlan::assemble(
        declared_placement,
        partitions,
        graph,
        PlanVersion(wire.version),
    ))
}

// ---------------------------------------------------------------------------
// radix-mir → wire conversion (the section producer side; MD2-W1 tests build
// honest sections through it)
// ---------------------------------------------------------------------------

fn contract_to_wire(contract: &PlacementContract) -> WirePlacementContract {
    match contract {
        PlacementContract::Owned { owner } => WirePlacementContract::Owned { owner: owner.0 },
        PlacementContract::Replicated { primary, replicas } => WirePlacementContract::Replicated {
            primary: primary.0,
            replicas: replicas.iter().map(|id| id.0).collect(),
        },
    }
}

fn placement_to_wire(placement: &LogicalPlacement) -> WireLogicalPlacement {
    WireLogicalPlacement {
        value: placement.value.0,
        contract: contract_to_wire(&placement.contract),
    }
}

fn constraint_to_wire(constraint: &PlacementConstraint) -> WirePlacementConstraint {
    match constraint {
        PlacementConstraint::PartitionBudgetBytes {
            partition,
            budget_bytes,
        } => WirePlacementConstraint::PartitionBudgetBytes {
            partition: partition.0,
            budget_bytes: *budget_bytes,
        },
        PlacementConstraint::RequireOwner { value, partition } => {
            WirePlacementConstraint::RequireOwner {
                value: value.0,
                partition: partition.0,
            }
        }
    }
}

fn objective_to_wire(objective: PlacementObjective) -> WirePlacementObjective {
    match objective {
        PlacementObjective::MinimizeTransferBytes => WirePlacementObjective::MinimizeTransferBytes,
        PlacementObjective::MaximizeOwnerAffinity => WirePlacementObjective::MaximizeOwnerAffinity,
    }
}

fn result_ref_to_wire(result: &ResultRef) -> WireResultRef {
    WireResultRef {
        buffer: result.buffer.0,
        version: result.version,
        produced_by: result.produced_by.0,
    }
}

fn boundary_to_wire(boundary: &ExecutionCommitBoundary) -> WireExecutionCommitBoundary {
    WireExecutionCommitBoundary {
        barriers: boundary.barriers.iter().map(|id| id.0).collect(),
        launches: boundary.launches.iter().map(|id| id.0).collect(),
    }
}

fn operation_to_wire(operation: &ExecutionOperation) -> WireExecutionOperation {
    match operation {
        ExecutionOperation::Launch(launch) => WireExecutionOperation::Launch(WireLaunchOperation {
            launch: launch.launch.0,
            partition: launch.partition.0,
        }),
        ExecutionOperation::Transfer(transfer) => {
            WireExecutionOperation::Transfer(WireTransferOperation {
                id: transfer.id.0,
                value: transfer.value.0,
                source: transfer.source.0,
                destination: transfer.destination.0,
                byte_count: transfer.byte_count,
                direction: match transfer.direction {
                    TransferDirection::H2D => WireTransferDirection::H2D,
                    TransferDirection::D2H => WireTransferDirection::D2H,
                    TransferDirection::BIDI => WireTransferDirection::BIDI,
                },
                element_ty: spelling_from_mir_type(transfer.element_ty),
                layout: match transfer.layout {
                    MirTensorStorageLayout::HostOwned => WireStorageLayout::HostOwned,
                    MirTensorStorageLayout::DeviceHandle => WireStorageLayout::DeviceHandle,
                    // MLC-4: carried as the sparse marker; never coerced to a host/device row.
                    MirTensorStorageLayout::Sparse => WireStorageLayout::Sparse,
                },
                path_label: match transfer.path_label {
                    TransportPath::HostStaged => WireTransportPath::HostStaged,
                },
                producer_generation: transfer.producer_generation,
                consumer_generation: transfer.consumer_generation,
                completion_boundary: boundary_to_wire(&transfer.completion_boundary),
            })
        }
        ExecutionOperation::Collective(collective) => {
            WireExecutionOperation::Collective(WireCollectiveOperation {
                id: collective.id.0,
                value: collective.value.0,
                kind: match collective.kind {
                    CollectiveKind::Broadcast => WireCollectiveKind::Broadcast,
                },
                source: collective.source.0,
                participants: collective.participants.iter().map(|id| id.0).collect(),
                byte_count: collective.byte_count,
            })
        }
        ExecutionOperation::Barrier(barrier) => {
            WireExecutionOperation::Barrier(WireBarrierOperation {
                id: barrier.id.0,
                partitions: barrier.partitions.iter().map(|id| id.0).collect(),
            })
        }
    }
}

/// Convert a radix-mir logical plan to its wire mirror (field-for-field).
#[must_use]
pub fn plan_to_wire_plan(plan: &DistributedExecutionPlan) -> WireDistributedExecutionPlan {
    WireDistributedExecutionPlan {
        declared_placement: WireDeclaredPlacement {
            placements: plan
                .declared_placement
                .placements
                .iter()
                .map(placement_to_wire)
                .collect(),
            constraints: plan
                .declared_placement
                .constraints
                .iter()
                .map(constraint_to_wire)
                .collect(),
            objectives: plan
                .declared_placement
                .objectives
                .iter()
                .map(|objective| objective_to_wire(*objective))
                .collect(),
        },
        partitions: plan
            .partitions
            .iter()
            .map(|partition| WireDeviceProgramPartition {
                id: partition.id.0,
                kernels: partition
                    .kernels
                    .iter()
                    .map(|index| *index as u64)
                    .collect(),
                launches: partition.launches.iter().map(|id| id.0).collect(),
                buffers: partition.buffers.iter().map(|id| id.0).collect(),
                results: partition.results.iter().map(result_ref_to_wire).collect(),
                placements: partition.placements.iter().map(placement_to_wire).collect(),
            })
            .collect(),
        graph: WireDistributedExecutionGraph {
            operations: plan
                .graph
                .operations
                .iter()
                .map(operation_to_wire)
                .collect(),
            commit_boundary: boundary_to_wire(&plan.graph.commit_boundary),
        },
        version: plan.version.0,
    }
}

/// The expected plan the equivalence gate re-derives: the deterministic
/// derivation over `(program, declared placement)` with the delivered plan's
/// declared constraints/objectives (which ride the plan — they are not
/// derivation inputs, md2-delivery §MD2-D1). The placements are rebuilt from
/// the declared facts (never copied from the delivered plan), so a tampered
/// delivered owner/placement fails the canonical comparison.
pub(crate) fn expected_plan_for_section(
    section: &FmirDistributedExecutionSection,
    wire_program: &WireDeviceProgram,
) -> Result<DistributedExecutionPlan, FmirImageError> {
    let facts = wire_facts_to_facts(&section.declared_placement);
    let program = wire_program_to_program(wire_program)?;
    let (partitions, graph) = radix_mir::distributed::derive(&program, &facts);
    let mut placements: Vec<LogicalPlacement> =
        facts.iter().map(LogicalPlacement::from_fact).collect();
    placements.sort_by_key(|placement| placement.value.0);
    let declared_placement = DeclaredPlacement {
        placements,
        constraints: wire_constraints(&section.logical_plan.declared_placement),
        objectives: wire_objectives(&section.logical_plan.declared_placement),
    };
    Ok(DistributedExecutionPlan::assemble(
        declared_placement,
        partitions,
        graph,
        PlanVersion(section.logical_plan.version),
    ))
}
