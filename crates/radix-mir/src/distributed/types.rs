//! Logical plan classes of the distributed execution model (MD2-D1).
//!
//! This module is the **shared seam** the MD2 downstream units consume
//! (MD2-V1/H1/W1): the four plan classes per the naming contract §3 (CTO
//! `06ac0f04`):
//!
//! ```text
//! DistributedExecutionPlan { declared_placement, partitions, graph, version }
//! DeviceProgramPartition
//! DistributedExecutionGraph over ExecutionOperation::{Launch,Transfer,Collective,Barrier}
//! LogicalPlacement / PlacementConstraint / PlacementObjective
//! ```
//!
//! Contract anchors (md0-naming-contract.md §3, md0-transport.md §7/§8,
//! md2-delivery.md §MD2-D1):
//!
//! - The plan is **portable and target-neutral** — it never references a
//!   `PhysicalDeviceId` (radix-mir does not depend on faber-runtime, FC18).
//! - [`DeviceProgramPartition`] references the semantic program by **stable
//!   id** — launch/buffer ids, the program's own kernel index space, and
//!   `(buffer, version, producing launch)` result triples — never by name,
//!   order, or binding position (A3).
//! - `Collective::Broadcast` is the **only admitted collective** (v1
//!   admissible set). AllReduce/AllGather/AllToAll/ReduceScatter are declared
//!   unsupported; the closed enum has no variant for them, so no plan can
//!   name one.
//! - Transfers carry byte count, direction (H2D/D2H/BIDI), dtype/layout, the
//!   `host-staged` path label (T2 §7 — silent host staging forbidden), the
//!   producer/consumer generations of the moved value, and a completion
//!   boundary. The completion boundary is an [`ExecutionCommitBoundary`] —
//!   **never "execution generation"** (naming contract §3; `ValueGeneration`
//!   is taken).
//! - The logical placement records are keyed by [`SemanticValueId`] and
//!   declare one owner or an explicit coherent replication contract per
//!   MD-A5; MD2 carries declared constraints and objectives, never a policy
//!   engine (the ranking engine is MD5's).

use crate::device_program::{BufferId, LaunchId};
use crate::device_semantics::SemanticValueId;
use crate::layout::MirTensorStorageLayout;
use crate::MirType;
use std::fmt;

/// Program-unique identity of one logical partition.
///
/// An **ordinal locator only** — never a physical identity (naming contract
/// §1). The logical plan never references a `PhysicalDeviceId`; virtual
/// partitions on the CS fixtures are a distinct fixture identity class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartitionId(pub u32);

impl fmt::Display for PartitionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PartitionId({})", self.0)
    }
}

/// Version of the logical distributed plan shape.
///
/// The plan carries its own version so a stale plan version vs the declared
/// placement can fail before launch (MD2-V1 stale-version check).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PlanVersion(pub u32);

impl fmt::Display for PlanVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PlanVersion({})", self.0)
    }
}

/// The first logical-plan shape version.
pub const LOGICAL_PLAN_VERSION: u32 = 1;

/// Declared placement contract of one semantic value (MD-A5).
///
/// One declared owner, or an **explicit coherent replication contract**: every
/// replica partition holds a complete copy under a generation-tracked
/// contract. A replica without a contract is an un-owned copy and is rejected
/// (md0-transport §4.1 ownership gate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementContract {
    /// Exactly one declared owner partition (sharded / resident — one owner).
    Owned {
        /// The single owner partition.
        owner: PartitionId,
    },
    /// Explicit coherent replication contract: every listed partition holds a
    /// complete copy.
    Replicated {
        /// The primary replica — the copy that serves as the broadcast
        /// source ("owner partition" of a replicated value).
        primary: PartitionId,
        /// The full replica set (the primary plus every other replica).
        replicas: Vec<PartitionId>,
    },
}

/// One declared placement fact consumed by the deterministic derivation.
///
/// Keyed by the stable [`SemanticValueId`] (F1), carrying the F1
/// value→buffer binding and the value's declared byte contract so the pure
/// derivation can resolve the fact against the program's buffer universe —
/// the single-device leaf alone carries no value table. The plan-level
/// [`LogicalPlacement`] record is the fact minus the derivation-only
/// binding/byte fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementFact {
    /// The semantic value this fact declares for (F1 stable key).
    pub value: SemanticValueId,
    /// The device buffer holding the value (F1 binding; derivation input).
    pub buffer: BufferId,
    /// The declared ownership or explicit replication contract.
    pub contract: PlacementContract,
    /// The declared byte count of the value's content — the byte/shape
    /// contract the cross-device completeness gate checks (exit-gate bullet
    /// 2). The derivation copies it onto every transfer of the value; it
    /// never invents a byte count.
    pub byte_count: u64,
}

/// Plan-level record of one declared value placement (MD-A5).
///
/// Keyed by [`SemanticValueId`]: one declared owner or an explicit coherent
/// replication contract. Derived from the declared [`PlacementFact`]s — never
/// hand-authored (MD2-D1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalPlacement {
    /// The semantic value this placement declares for.
    pub value: SemanticValueId,
    /// The declared ownership / replication contract.
    pub contract: PlacementContract,
}

impl LogicalPlacement {
    /// Build the plan record from a declared fact.
    #[must_use]
    pub fn from_fact(fact: &PlacementFact) -> Self {
        Self {
            value: fact.value,
            contract: fact.contract.clone(),
        }
    }
}

/// A declared hard placement constraint.
///
/// MD2 carries **declared** constraints; it never invents a policy engine
/// (the planner/objective ranking is MD5's). Constraints are plan facts, not
/// enforcement — enforcement is the validation gate's and MD3's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementConstraint {
    /// Declared memory budget of one partition in bytes (CS-1: 160 MiB per
    /// virtual partition — `md0-mode-fixtures.md` §3).
    PartitionBudgetBytes {
        /// The constrained partition.
        partition: PartitionId,
        /// Declared budget in bytes.
        budget_bytes: u64,
    },
    /// Hard residency constraint: the value must be owned by the named
    /// partition.
    RequireOwner {
        /// The constrained semantic value.
        value: SemanticValueId,
        /// The partition that must own it.
        partition: PartitionId,
    },
}

/// A declared placement objective.
///
/// MD5's planner ranks gate-passing plans by objectives; MD2 carries the
/// **declared** objectives only (md0-cost-contract §1 — gates reject, then
/// objectives rank). The objective set here is closed and frozen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlacementObjective {
    /// Declared objective: minimize cross-partition transfer bytes.
    MinimizeTransferBytes,
    /// Declared objective: prefer serving values at their owner.
    MaximizeOwnerAffinity,
}

/// The declared placement of a logical plan: facts, constraints, objectives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclaredPlacement {
    /// One record per declared value placement, ascending by value id.
    pub placements: Vec<LogicalPlacement>,
    /// Declared hard constraints.
    pub constraints: Vec<PlacementConstraint>,
    /// Declared objectives.
    pub objectives: Vec<PlacementObjective>,
}

/// Stable reference to one result row of the semantic program.
///
/// A result row is identified by the observed buffer/version triple and the
/// producing launch — the program's own result identity — never by position
/// in `DeviceProgram::results` (A3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResultRef {
    /// The observed buffer.
    pub buffer: BufferId,
    /// The observed content version.
    pub version: u32,
    /// The launch whose completion makes the observation valid.
    pub produced_by: LaunchId,
}

/// A logical per-device leaf of one semantic program (MD2-D1).
///
/// References a subset of the program's kernels/launches/buffers/results by
/// **stable id** — never by name, order, or binding position (A3):
///
/// - kernels by the program's own kernel reference (index into
///   `DeviceProgram::kernels` — the same index space `LaunchUnit` uses);
/// - launches by [`LaunchId`];
/// - buffers by [`BufferId`];
/// - results by the [`ResultRef`] triple.
///
/// Derived deterministically; never hand-authored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceProgramPartition {
    /// Stable partition identity (ordinal locator only — never a physical
    /// identity).
    pub id: PartitionId,
    /// Kernel subset, by index into `DeviceProgram::kernels` (the program's
    /// own kernel reference space; ascending, deduplicated).
    pub kernels: Vec<usize>,
    /// Launch subset, by stable launch id (program order).
    pub launches: Vec<LaunchId>,
    /// Buffer subset, by stable buffer id (ascending).
    pub buffers: Vec<BufferId>,
    /// Result subset, by (buffer, version, producing launch).
    pub results: Vec<ResultRef>,
    /// The declared placement records involving this partition (ascending by
    /// value id).
    pub placements: Vec<LogicalPlacement>,
}

/// Stable identity of one transfer operation in the execution graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TransferId(pub u32);

impl fmt::Display for TransferId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TransferId({})", self.0)
    }
}

/// Direction of a host-staged transfer (T1 §2.2 measurement vocabulary).
///
/// `H2D` / `D2H` are the host-boundary half-moves; `BIDI` is the combined
/// device-to-device host-staged copy (concurrent H2D ∥ D2H — the way every
/// cross-partition move on this host traverses the admitted path).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferDirection {
    /// Host → device (copy in to the destination partition).
    H2D,
    /// Device → host (copy out from the source partition).
    D2H,
    /// Both directions concurrently (the host-staged device-to-device move).
    BIDI,
}

impl TransferDirection {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::H2D => "h2d",
            Self::D2H => "d2h",
            Self::BIDI => "bidi",
        }
    }
}

/// Transport path label of a transfer (T2 §7 — silent host staging
/// forbidden).
///
/// `host-staged` is the only admitted path on the acceptance host; peer paths
/// become admissible only per a measured directed pair (T2 §6 fallback rule)
/// — never assumed. Every transfer is labeled with its path; a plan can
/// never relabel a copy to hide cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportPath {
    /// Pinned host memory ↔ device over `PCIe` — the admitted path (T2 §1).
    HostStaged,
}

impl TransportPath {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::HostStaged => "host-staged",
        }
    }
}

/// Stable identity of one barrier operation in the execution graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BarrierId(pub u32);

impl fmt::Display for BarrierId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BarrierId({})", self.0)
    }
}

/// The declared completion boundary of a value (naming contract §3).
///
/// References the barrier/launch set that commits a value — **never called
/// "execution generation"**: `ValueGeneration` is the taken F2 name, and this
/// is a boundary declaration over barrier/launch ids, not a generation
/// number. MD3 owns the `ExecutionTransaction` mechanics; MD2 carries the
/// declaration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionCommitBoundary {
    /// Barriers whose completion commits the value.
    pub barriers: Vec<BarrierId>,
    /// Launches whose completion commits the value.
    pub launches: Vec<LaunchId>,
}

/// One cross-partition value transfer (host-staged; T2 §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferOperation {
    /// Stable transfer identity within the graph.
    pub id: TransferId,
    /// The semantic value whose bytes move (F1 key).
    pub value: SemanticValueId,
    /// Source partition (the value's owner for a read move; the producing
    /// partition for a write-back move).
    pub source: PartitionId,
    /// Destination partition.
    pub destination: PartitionId,
    /// Byte count of the transfer (the value's declared byte contract).
    pub byte_count: u64,
    /// Direction (H2D / D2H / BIDI; cross-partition moves on this host are
    /// BIDI host-staged copies).
    pub direction: TransferDirection,
    /// Element type of the transferred content (dtype).
    pub element_ty: MirType,
    /// Storage layout of the transferred content.
    pub layout: MirTensorStorageLayout,
    /// Transport path label — `host-staged` (the only admitted path).
    pub path_label: TransportPath,
    /// Producer generation — the content version the producer wrote.
    pub producer_generation: u32,
    /// Consumer generation — the content version the consumer reads.
    pub consumer_generation: u32,
    /// Completion boundary that commits the value (barrier/launch set).
    pub completion_boundary: ExecutionCommitBoundary,
}

/// Stable identity of one collective operation in the execution graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectiveId(pub u32);

impl fmt::Display for CollectiveId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CollectiveId({})", self.0)
    }
}

/// Admissible collective kind (v1 set — the admissible-set discriminant).
///
/// `Broadcast` is the **only** admitted collective in v1. `AllReduce`,
/// `AllGather`, `AllToAll`, and `ReduceScatter` are **declared unsupported** and
/// fail before launch (md2-delivery §P1; T2 §3/§8). The enum is closed — it
/// has no variant for any other kind, so no plan can name one and the graph
/// API rejects any other kind at the type boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectiveKind {
    /// Owner partition → all consumers, composed at MD3 from labeled
    /// host-staged transfers + local kernels — no collective library.
    Broadcast,
}

impl CollectiveKind {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Broadcast => "broadcast",
        }
    }
}

/// One collective operation in the execution graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectiveOperation {
    /// Stable collective identity within the graph.
    pub id: CollectiveId,
    /// The semantic value the collective moves.
    pub value: SemanticValueId,
    /// The admissible-kind discriminant (v1 = [`CollectiveKind::Broadcast`]
    /// only).
    pub kind: CollectiveKind,
    /// Source partition (the owner / primary replica).
    pub source: PartitionId,
    /// The participant partition set (source plus all consumers).
    pub participants: Vec<PartitionId>,
    /// Byte count of the value moved (the value's declared byte contract).
    pub byte_count: u64,
}

impl CollectiveOperation {
    /// Construct a broadcast — the only admitted collective.
    ///
    /// The admissible set is closed: there is no constructor for any other
    /// kind, so an unsupported collective cannot be named by a plan
    /// (fail-before-launch at the type boundary).
    #[must_use]
    pub fn broadcast(
        id: CollectiveId,
        value: SemanticValueId,
        source: PartitionId,
        participants: Vec<PartitionId>,
        byte_count: u64,
    ) -> Self {
        Self {
            id,
            value,
            kind: CollectiveKind::Broadcast,
            source,
            participants,
            byte_count,
        }
    }
}

/// One launch operation in the execution graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchOperation {
    /// The semantic launch executed (stable launch id).
    pub launch: LaunchId,
    /// The partition that runs the launch.
    pub partition: PartitionId,
}

/// One barrier operation in the execution graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarrierOperation {
    /// Stable barrier identity within the graph.
    pub id: BarrierId,
    /// Participant partition set.
    pub partitions: Vec<PartitionId>,
}

/// One operation of the upper execution graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionOperation {
    /// A launch of the semantic program on one partition.
    Launch(LaunchOperation),
    /// A host-staged cross-partition transfer (T2 §7).
    Transfer(TransferOperation),
    /// A collective (v1 admissible set: [`CollectiveKind::Broadcast`] only).
    Collective(CollectiveOperation),
    /// A barrier synchronizing its participant partitions.
    Barrier(BarrierOperation),
}

/// The upper execution graph of a distributed plan.
///
/// Launches, transfers, collectives, and barriers all participate — it is
/// **not** a "`CommunicationGraph`" (naming contract §3). Operations are
/// stored in deterministic plan order (launches in program order, then
/// transfers, collectives, and barriers in the derivation's fixed order).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DistributedExecutionGraph {
    /// Operations in deterministic plan order.
    pub operations: Vec<ExecutionOperation>,
    /// The declared commit boundary of the plan — the barrier/launch set
    /// that commits the plan's results (never "execution generation").
    pub commit_boundary: ExecutionCommitBoundary,
}

impl DistributedExecutionGraph {
    /// The launch operations, in plan order.
    #[must_use]
    pub fn launches(&self) -> Vec<&LaunchOperation> {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                ExecutionOperation::Launch(launch) => Some(launch),
                _ => None,
            })
            .collect()
    }

    /// The transfer operations, in plan order.
    #[must_use]
    pub fn transfers(&self) -> Vec<&TransferOperation> {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                ExecutionOperation::Transfer(transfer) => Some(transfer),
                _ => None,
            })
            .collect()
    }

    /// The collective operations, in plan order.
    #[must_use]
    pub fn collectives(&self) -> Vec<&CollectiveOperation> {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                ExecutionOperation::Collective(collective) => Some(collective),
                _ => None,
            })
            .collect()
    }

    /// The barrier operations, in plan order.
    #[must_use]
    pub fn barriers(&self) -> Vec<&BarrierOperation> {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                ExecutionOperation::Barrier(barrier) => Some(barrier),
                _ => None,
            })
            .collect()
    }
}

/// A portable, target-neutral distributed execution plan (naming contract
/// §3, CTO `06ac0f04`).
///
/// ```text
/// DistributedExecutionPlan { declared_placement, partitions, graph, version }
/// ```
///
/// The plan is **portable and target-neutral** — it never references a
/// `PhysicalDeviceId` (FC18). This is the logical plan the FMIR distributed
/// section mirrors (MD2-W1) and the hash substrate for
/// `logical_distributed_plan_hash` (MD2-H1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributedExecutionPlan {
    /// The declared logical placement (facts + constraints + objectives).
    pub declared_placement: DeclaredPlacement,
    /// The derived per-device leaf partitions, ascending by partition id.
    pub partitions: Vec<DeviceProgramPartition>,
    /// The derived upper execution graph.
    pub graph: DistributedExecutionGraph,
    /// The plan shape version (stale-version checks, MD2-V1).
    pub version: PlanVersion,
}

impl DistributedExecutionPlan {
    /// Assemble a plan from the declared placement and a derivation result.
    #[must_use]
    pub fn assemble(
        declared_placement: DeclaredPlacement,
        partitions: Vec<DeviceProgramPartition>,
        graph: DistributedExecutionGraph,
        version: PlanVersion,
    ) -> Self {
        Self {
            declared_placement,
            partitions,
            graph,
            version,
        }
    }
}
