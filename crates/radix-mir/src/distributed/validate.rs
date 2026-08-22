//! Fail-before-launch validation of a distributed execution plan.
//!
//! [`DistributedExecutionPlan::validate`] is a pure evaluation: it never
//! mutates the plan or allocates execution resources. A violating plan is
//! rejected fail-closed with a typed [`ValidateError`] that names the class
//! and the exact failing fact. The [`CollectiveKind`] match is exhaustive,
//! so a new kind fails to compile until this module admits it.

use std::fmt;

use crate::device_program::LaunchId;
use crate::device_semantics::SemanticValueId;
use crate::distributed::types::{
    BarrierId, CollectiveId, CollectiveKind, CollectiveOperation, DistributedExecutionPlan,
    ExecutionOperation, LogicalPlacement, PartitionId, PlacementConstraint, PlacementContract,
    PlanVersion, TransferId, TransferOperation, LOGICAL_PLAN_VERSION,
};

// DIAGNOSTICS

/// Fail-closed check class named by [`ValidateError::class`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidateKind {
    /// Cross-device completeness — a field of the value contract is missing.
    Completeness,
    /// The value-move graph contains a cycle.
    Cycle,
    /// A value has no declared owner / coherent replication contract.
    MissingOwner,
    /// Replica or placement sets disagree.
    ConflictingReplicas,
    /// A collective outside the v1 admissible set / form.
    UnsupportedCollective,
    /// Declared topology is inconsistent with the plan's partition set.
    TopologyMismatch,
    /// The plan shape version is stale or unknown.
    StaleVersion,
}

impl fmt::Display for ValidateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completeness => write!(f, "completeness"),
            Self::Cycle => write!(f, "cycle"),
            Self::MissingOwner => write!(f, "missing-owner"),
            Self::ConflictingReplicas => write!(f, "conflicting-replicas"),
            Self::UnsupportedCollective => write!(f, "unsupported-collective"),
            Self::TopologyMismatch => write!(f, "topology-mismatch"),
            Self::StaleVersion => write!(f, "stale-version"),
        }
    }
}

/// Identity of the operation a completeness fact belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpRef {
    Transfer(TransferId),
    Collective(CollectiveId),
}

impl fmt::Display for OpRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transfer(id) => write!(f, "transfer {id}"),
            Self::Collective(id) => write!(f, "collective {id}"),
        }
    }
}

/// A boundary reference that must resolve to the plan's own graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryRef {
    Barrier(BarrierId),
    Launch(LaunchId),
}

impl fmt::Display for BoundaryRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Barrier(id) => write!(f, "barrier {id}"),
            Self::Launch(id) => write!(f, "launch {id}"),
        }
    }
}

/// The construct that names a partition in a topology check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyRef {
    Placement(SemanticValueId),
    Constraint,
    Launch(LaunchId),
    Transfer(TransferId),
    Collective(CollectiveId),
    Barrier(BarrierId),
}

impl fmt::Display for TopologyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Placement(value) => write!(f, "placement of {value}"),
            Self::Constraint => write!(f, "constraint"),
            Self::Launch(id) => write!(f, "launch {id}"),
            Self::Transfer(id) => write!(f, "transfer {id}"),
            Self::Collective(id) => write!(f, "collective {id}"),
            Self::Barrier(id) => write!(f, "barrier {id}"),
        }
    }
}

/// Why a collective is outside the v1 admissible form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectiveViolation {
    /// The broadcast source is not a participant.
    SourceNotParticipant {
        /// The offending source partition.
        source: PartitionId,
    },
    /// A broadcast needs the owner plus at least one consumer.
    TooFewParticipants {
        /// The declared participant count.
        count: u64,
    },
    /// The source is not the declared owner / primary replica.
    NotOwnerOriginated {
        /// The declared owner / primary.
        owner: PartitionId,
        /// The offending source partition.
        source: PartitionId,
    },
}

/// A heap-free fingerprint of a placement contract (for conflict facts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractFingerprint {
    /// One owner.
    Owned {
        /// The single owner partition.
        owner: PartitionId,
    },
    /// An explicit replication contract.
    Replicated {
        /// The primary replica.
        primary: PartitionId,
        /// Replica count (the set is re-scanned for the conflict).
        replica_count: u64,
    },
}

/// Typed diagnostic of the fail-before-launch gate.
///
/// Every variant names the **violated class** ([`ValidateError::class`]) and
/// carries the **exact failing fact** — ids and counts by value, so the
/// diagnostic is heap-free and trivially cloneable.
#[allow(clippy::enum_variant_names)] // variant prefixes ARE the diagnostic vocabulary (the violated class)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidateError {
    /// The plan shape version is stale or unknown.
    StaleVersion {
        /// The plan's declared version.
        plan_version: PlanVersion,
        /// The supported logical plan version.
        supported_version: u32,
    },

    /// The partition set contains a duplicate partition id.
    RepeatedPartition {
        /// The duplicated partition.
        partition: PartitionId,
    },
    /// A construct references a partition outside the plan's partition set.
    TopologyMismatch {
        /// The undeclared partition.
        partition: PartitionId,
        /// The construct referencing it.
        referenced_by: TopologyRef,
    },
    /// A declared partition budget is zero.
    ZeroPartitionBudget {
        /// The constrained partition.
        partition: PartitionId,
    },
    /// A `RequireOwner` constraint disagrees with the declared owner.
    RequireOwnerMismatch {
        /// The constrained value.
        value: SemanticValueId,
        /// The partition the constraint requires.
        required_partition: PartitionId,
        /// The declared owner (None when the value has no placement).
        actual_owner: Option<PartitionId>,
    },

    /// A cross-partition value has no declared placement.
    AbsentOwner {
        /// The value moved without a placement.
        value: SemanticValueId,
        /// The operation moving it.
        reference: OpRef,
    },
    /// A replicated contract has an empty replica set.
    AbsentReplicaSet {
        /// The value with the empty contract.
        value: SemanticValueId,
        /// The declared primary.
        primary: PartitionId,
    },
    /// A replicated contract's primary is not in its replica set.
    PrimaryOutsideReplicaSet {
        /// The value with the incoherent contract.
        value: SemanticValueId,
        /// The primary missing from the replica set.
        primary: PartitionId,
    },

    /// The same value is declared twice with disagreeing contracts.
    ConflictingContracts {
        /// The doubly-declared value.
        value: SemanticValueId,
        /// The first declared contract.
        first: ContractFingerprint,
        /// The second declared contract.
        second: ContractFingerprint,
    },
    /// A replica set repeats a partition id.
    RepeatedReplica {
        /// The replicated value.
        value: SemanticValueId,
        /// The repeated partition.
        partition: PartitionId,
    },
    /// A partition-level placement disagrees with the plan-level record.
    PartitionContractMismatch {
        /// The disputed value.
        value: SemanticValueId,
        /// The partition carrying the disagreeing placement.
        partition: PartitionId,
    },
    /// A launch is placed on two partitions.
    SharedLaunch {
        /// The shared launch.
        launch: LaunchId,
        /// The first partition.
        first: PartitionId,
        /// The second partition.
        second: PartitionId,
    },
    /// Operations of one value disagree on the byte/shape contract.
    ConflictingByteContract {
        /// The value with the conflicting contracts.
        value: SemanticValueId,
        /// The first declared byte count.
        first_byte_count: u64,
        /// The second declared byte count.
        second_byte_count: u64,
        /// The operation carrying the second contract.
        second: OpRef,
    },
    /// Two transfers declare the identical move of one value.
    RepeatedMove {
        /// The value moved twice.
        value: SemanticValueId,
        /// The original move.
        original: TransferId,
        /// The duplicate move.
        duplicate: TransferId,
    },

    /// A transfer lacks a generation (zero or absent).
    AbsentGeneration {
        /// The moved value.
        value: SemanticValueId,
        /// The operation.
        reference: OpRef,
        /// The producer generation.
        producer_generation: u32,
        /// The consumer generation.
        consumer_generation: u32,
    },
    /// The consumer reads a generation older than the producer wrote.
    GenerationOrder {
        /// The moved value.
        value: SemanticValueId,
        /// The operation.
        reference: OpRef,
        /// The producer generation.
        producer_generation: u32,
        /// The consumer generation.
        consumer_generation: u32,
    },
    /// A transfer/collective lacks a byte/shape contract.
    AbsentByteContract {
        /// The moved value.
        value: SemanticValueId,
        /// The operation.
        reference: OpRef,
        /// The declared byte count (zero).
        byte_count: u64,
    },
    /// A transfer names no producing launch.
    AbsentProducer {
        /// The moved value.
        value: SemanticValueId,
        /// The operation.
        reference: OpRef,
    },
    /// A transfer's destination neither holds a copy nor runs a launch.
    AbsentConsumer {
        /// The moved value.
        value: SemanticValueId,
        /// The operation.
        reference: OpRef,
        /// The source partition.
        source: PartitionId,
        /// The destination partition.
        destination: PartitionId,
    },
    /// A transfer has no completion boundary at all.
    AbsentCompletionBoundary {
        /// The moved value.
        value: SemanticValueId,
        /// The operation.
        reference: OpRef,
    },
    /// A completion boundary references a barrier/launch outside the plan.
    DanglingBoundaryReference {
        /// The operation with the dangling reference.
        reference: OpRef,
        /// The dangling barrier/launch.
        boundary_ref: BoundaryRef,
    },

    /// A transfer moves a value to its own partition.
    SelfMove {
        /// The moved value.
        value: SemanticValueId,
        /// The self-move transfer.
        transfer: TransferId,
    },
    /// The per-value move graph contains a directed cycle.
    ValueMoveCycle {
        /// The value cycling.
        value: SemanticValueId,
        /// The transfer whose outgoing edge closes the cycle.
        first: TransferId,
        /// The transfer closing back onto `first`.
        second: TransferId,
    },

    /// A collective is outside the v1 admissible set / form.
    UnadmittedCollective {
        /// The moved value.
        value: SemanticValueId,
        /// The collective operation.
        collective: CollectiveId,
        /// Why it is unsupported.
        violation: CollectiveViolation,
    },
}

impl ValidateError {
    /// The violated check class of this diagnostic.
    #[must_use]
    pub fn class(&self) -> ValidateKind {
        match self {
            Self::StaleVersion { .. } => ValidateKind::StaleVersion,
            Self::RepeatedPartition { .. }
            | Self::TopologyMismatch { .. }
            | Self::ZeroPartitionBudget { .. }
            | Self::RequireOwnerMismatch { .. } => ValidateKind::TopologyMismatch,
            Self::AbsentOwner { .. }
            | Self::AbsentReplicaSet { .. }
            | Self::PrimaryOutsideReplicaSet { .. } => ValidateKind::MissingOwner,
            Self::ConflictingContracts { .. }
            | Self::RepeatedReplica { .. }
            | Self::PartitionContractMismatch { .. }
            | Self::SharedLaunch { .. }
            | Self::ConflictingByteContract { .. }
            | Self::RepeatedMove { .. } => ValidateKind::ConflictingReplicas,
            Self::AbsentGeneration { .. }
            | Self::GenerationOrder { .. }
            | Self::AbsentByteContract { .. }
            | Self::AbsentProducer { .. }
            | Self::AbsentConsumer { .. }
            | Self::AbsentCompletionBoundary { .. }
            | Self::DanglingBoundaryReference { .. } => ValidateKind::Completeness,
            Self::SelfMove { .. } | Self::ValueMoveCycle { .. } => ValidateKind::Cycle,
            Self::UnadmittedCollective { .. } => ValidateKind::UnsupportedCollective,
        }
    }
}

impl fmt::Display for ValidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleVersion {
                plan_version,
                supported_version,
            } => write!(
                f,
                "{}: plan version {plan_version} != supported logical plan version {supported_version}",
                self.class()
            ),
            Self::RepeatedPartition { partition } => {
                write!(f, "{}: partition set contains {partition} twice", self.class())
            }
            Self::TopologyMismatch {
                partition,
                referenced_by,
            } => write!(
                f,
                "{}: {partition} (referenced by {referenced_by}) is not a declared partition",
                self.class()
            ),
            Self::ZeroPartitionBudget { partition } => write!(
                f,
                "{}: declared budget of {partition} is zero bytes",
                self.class()
            ),
            Self::RequireOwnerMismatch {
                value,
                required_partition,
                actual_owner,
            } => match actual_owner {
                Some(owner) => write!(
                    f,
                    "{}: {value} must be owned by {required_partition} but is owned by {owner}",
                    self.class()
                ),
                None => write!(
                    f,
                    "{}: {value} must be owned by {required_partition} but has no placement",
                    self.class()
                ),
            },
            Self::AbsentOwner { value, reference } => write!(
                f,
                "{}: {value} moved by {reference} has no declared owner or replication contract",
                self.class()
            ),
            Self::AbsentReplicaSet { value, primary } => write!(
                f,
                "{}: {value} declares primary {primary} with an empty replica set",
                self.class()
            ),
            Self::PrimaryOutsideReplicaSet { value, primary } => write!(
                f,
                "{}: {value} declares primary {primary} outside its replica set",
                self.class()
            ),
            Self::ConflictingContracts { value, first, second } => write!(
                f,
                "{}: {value} declared twice — {first:?} vs {second:?}",
                self.class()
            ),
            Self::RepeatedReplica { value, partition } => write!(
                f,
                "{}: replica set of {value} repeats {partition}",
                self.class()
            ),
            Self::PartitionContractMismatch { value, partition } => write!(
                f,
                "{}: {partition} carries a placement for {value} that disagrees with the plan",
                self.class()
            ),
            Self::SharedLaunch {
                launch,
                first,
                second,
            } => write!(
                f,
                "{}: {launch} is placed on both {first} and {second}",
                self.class()
            ),
            Self::ConflictingByteContract {
                value,
                first_byte_count,
                second_byte_count,
                second,
            } => write!(
                f,
                "{}: {value} byte contract {first_byte_count} vs {second_byte_count} on {second}",
                self.class()
            ),
            Self::RepeatedMove {
                value,
                original,
                duplicate,
            } => write!(
                f,
                "{}: {duplicate} repeats the move of {value} already declared by {original}",
                self.class()
            ),
            Self::AbsentGeneration {
                value,
                reference,
                producer_generation,
                consumer_generation,
            } => write!(
                f,
                "{}: {reference} moves {value} without a generation (producer {producer_generation}, consumer {consumer_generation})",
                self.class()
            ),
            Self::GenerationOrder {
                value,
                reference,
                producer_generation,
                consumer_generation,
            } => write!(
                f,
                "{}: {reference} consumes {value} at generation {consumer_generation} before it is produced at {producer_generation}",
                self.class()
            ),
            Self::AbsentByteContract {
                value,
                reference,
                byte_count,
            } => write!(
                f,
                "{}: {reference} moves {value} with byte count {byte_count} — no byte/shape contract",
                self.class()
            ),
            Self::AbsentProducer { value, reference } => write!(
                f,
                "{}: {reference} moves {value} without a producing launch",
                self.class()
            ),
            Self::AbsentConsumer {
                value,
                reference,
                source,
                destination,
            } => write!(
                f,
                "{}: {reference} moves {value} {source} → {destination} but {destination} neither holds a copy nor runs a launch",
                self.class()
            ),
            Self::AbsentCompletionBoundary { value, reference } => write!(
                f,
                "{}: {reference} moves {value} with no completion boundary",
                self.class()
            ),
            Self::DanglingBoundaryReference {
                reference,
                boundary_ref,
            } => write!(
                f,
                "{}: {reference} references {boundary_ref} which does not exist in the plan",
                self.class()
            ),
            Self::SelfMove { value, transfer } => write!(
                f,
                "{}: {transfer} moves {value} to its own partition",
                self.class()
            ),
            Self::ValueMoveCycle {
                value,
                first,
                second,
            } => write!(
                f,
                "{}: {value} moves cycle through {first} and {second}",
                self.class()
            ),
            Self::UnadmittedCollective {
                value,
                collective,
                violation,
            } => match violation {
                CollectiveViolation::SourceNotParticipant { source } => write!(
                    f,
                    "{}: {collective} broadcasts {value} from {source} which is not a participant",
                    self.class()
                ),
                CollectiveViolation::TooFewParticipants { count } => write!(
                    f,
                    "{}: {collective} broadcasts {value} to {count} participant(s) — a broadcast needs the owner plus at least one consumer",
                    self.class()
                ),
                CollectiveViolation::NotOwnerOriginated { owner, source } => write!(
                    f,
                    "{}: {collective} broadcasts {value} from {source}, but the declared owner is {owner}",
                    self.class()
                ),
            },
        }
    }
}

// GATE

impl DistributedExecutionPlan {
    /// Run the fail-before-launch validation gate.
    ///
    /// Pure evaluation: does not mutate the plan or allocate execution
    /// resources. A plan returning `Ok` passed every check.
    ///
    /// # Errors
    ///
    /// Returns the first [`ValidateError`], naming the violated class and
    /// the exact failing fact.
    pub fn validate(&self) -> Result<(), ValidateError> {
        validate_plan(self)
    }
}

fn validate_plan(plan: &DistributedExecutionPlan) -> Result<(), ValidateError> {
    if plan.version.0 != LOGICAL_PLAN_VERSION {
        return Err(ValidateError::StaleVersion {
            plan_version: plan.version,
            supported_version: LOGICAL_PLAN_VERSION,
        });
    }

    let partitions = partition_ids(plan);
    let launches = launch_universe(plan);
    let barriers = barrier_universe(plan);

    check_duplicate_partitions(plan)?;
    check_placement_topology(plan, &partitions)?;
    check_constraint_topology(plan, &partitions)?;
    check_operation_topology(plan, &partitions)?;

    check_declared_placements(plan)?;
    check_partition_placements(plan)?;
    check_shared_launches(plan)?;

    check_completeness(plan, &launches, &barriers)?;
    check_mover_agreement(plan)?;

    check_cycles(plan)?;
    check_collectives(plan)?;

    Ok(())
}

/// The plan's partition ids, in plan order.
fn partition_ids(plan: &DistributedExecutionPlan) -> Vec<PartitionId> {
    plan.partitions
        .iter()
        .map(|partition| partition.id)
        .collect()
}

/// Every launch id placed on a partition (deduplicated).
fn launch_universe(plan: &DistributedExecutionPlan) -> Vec<LaunchId> {
    let mut ids: Vec<LaunchId> = Vec::new();
    for partition in &plan.partitions {
        for launch in &partition.launches {
            if !ids.contains(launch) {
                ids.push(*launch);
            }
        }
    }
    ids
}

/// Every barrier id in the graph, in plan order.
fn barrier_universe(plan: &DistributedExecutionPlan) -> Vec<BarrierId> {
    let mut ids: Vec<BarrierId> = Vec::new();
    for operation in &plan.graph.operations {
        if let ExecutionOperation::Barrier(barrier) = operation {
            ids.push(barrier.id);
        }
    }
    ids
}

/// The declared plan-level placement of a semantic value.
fn placement_for(
    plan: &DistributedExecutionPlan,
    value: SemanticValueId,
) -> Option<&LogicalPlacement> {
    plan.declared_placement
        .placements
        .iter()
        .find(|placement| placement.value == value)
}

/// The declared owner / primary replica of a value.
fn declared_owner(plan: &DistributedExecutionPlan, value: SemanticValueId) -> Option<PartitionId> {
    placement_for(plan, value).map(|placement| match &placement.contract {
        PlacementContract::Owned { owner } => *owner,
        PlacementContract::Replicated { primary, .. } => *primary,
    })
}

/// Whether a contract holds a complete copy on the partition.
fn copy_holder(contract: &PlacementContract, partition: PartitionId) -> bool {
    match contract {
        PlacementContract::Owned { owner } => *owner == partition,
        PlacementContract::Replicated { replicas, .. } => replicas.contains(&partition),
    }
}

/// Heap-free fingerprint of a contract for conflict diagnostics.
fn contract_fingerprint(contract: &PlacementContract) -> ContractFingerprint {
    match contract {
        PlacementContract::Owned { owner } => ContractFingerprint::Owned { owner: *owner },
        PlacementContract::Replicated { primary, replicas } => ContractFingerprint::Replicated {
            primary: *primary,
            replica_count: replicas.len() as u64,
        },
    }
}

// TOPOLOGY

fn check_duplicate_partitions(plan: &DistributedExecutionPlan) -> Result<(), ValidateError> {
    for (index, partition) in plan.partitions.iter().enumerate() {
        if plan.partitions[index + 1..]
            .iter()
            .any(|later| later.id == partition.id)
        {
            return Err(ValidateError::RepeatedPartition {
                partition: partition.id,
            });
        }
    }
    Ok(())
}

fn check_placement_topology(
    plan: &DistributedExecutionPlan,
    partitions: &[PartitionId],
) -> Result<(), ValidateError> {
    for placement in &plan.declared_placement.placements {
        match &placement.contract {
            PlacementContract::Owned { owner } => {
                if !partitions.contains(owner) {
                    return Err(ValidateError::TopologyMismatch {
                        partition: *owner,
                        referenced_by: TopologyRef::Placement(placement.value),
                    });
                }
            }
            PlacementContract::Replicated { primary, replicas } => {
                if !partitions.contains(primary) {
                    return Err(ValidateError::TopologyMismatch {
                        partition: *primary,
                        referenced_by: TopologyRef::Placement(placement.value),
                    });
                }
                for replica in replicas {
                    if !partitions.contains(replica) {
                        return Err(ValidateError::TopologyMismatch {
                            partition: *replica,
                            referenced_by: TopologyRef::Placement(placement.value),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_constraint_topology(
    plan: &DistributedExecutionPlan,
    partitions: &[PartitionId],
) -> Result<(), ValidateError> {
    for constraint in &plan.declared_placement.constraints {
        match constraint {
            PlacementConstraint::PartitionBudgetBytes {
                partition,
                budget_bytes,
            } => {
                if !partitions.contains(partition) {
                    return Err(ValidateError::TopologyMismatch {
                        partition: *partition,
                        referenced_by: TopologyRef::Constraint,
                    });
                }
                if *budget_bytes == 0 {
                    return Err(ValidateError::ZeroPartitionBudget {
                        partition: *partition,
                    });
                }
            }
            PlacementConstraint::RequireOwner { value, partition } => {
                if !partitions.contains(partition) {
                    return Err(ValidateError::TopologyMismatch {
                        partition: *partition,
                        referenced_by: TopologyRef::Constraint,
                    });
                }
                let actual_owner = declared_owner(plan, *value);
                if actual_owner != Some(*partition) {
                    return Err(ValidateError::RequireOwnerMismatch {
                        value: *value,
                        required_partition: *partition,
                        actual_owner,
                    });
                }
            }
        }
    }
    Ok(())
}

fn check_operation_topology(
    plan: &DistributedExecutionPlan,
    partitions: &[PartitionId],
) -> Result<(), ValidateError> {
    for operation in &plan.graph.operations {
        match operation {
            ExecutionOperation::Launch(launch) => {
                if !partitions.contains(&launch.partition) {
                    return Err(ValidateError::TopologyMismatch {
                        partition: launch.partition,
                        referenced_by: TopologyRef::Launch(launch.launch),
                    });
                }
            }
            ExecutionOperation::Transfer(transfer) => {
                if !partitions.contains(&transfer.source) {
                    return Err(ValidateError::TopologyMismatch {
                        partition: transfer.source,
                        referenced_by: TopologyRef::Transfer(transfer.id),
                    });
                }
                if !partitions.contains(&transfer.destination) {
                    return Err(ValidateError::TopologyMismatch {
                        partition: transfer.destination,
                        referenced_by: TopologyRef::Transfer(transfer.id),
                    });
                }
            }
            ExecutionOperation::Collective(collective) => {
                if !partitions.contains(&collective.source) {
                    return Err(ValidateError::TopologyMismatch {
                        partition: collective.source,
                        referenced_by: TopologyRef::Collective(collective.id),
                    });
                }
                for participant in &collective.participants {
                    if !partitions.contains(participant) {
                        return Err(ValidateError::TopologyMismatch {
                            partition: *participant,
                            referenced_by: TopologyRef::Collective(collective.id),
                        });
                    }
                }
            }
            ExecutionOperation::Barrier(barrier) => {
                for participant in &barrier.partitions {
                    if !partitions.contains(participant) {
                        return Err(ValidateError::TopologyMismatch {
                            partition: *participant,
                            referenced_by: TopologyRef::Barrier(barrier.id),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

// PLACEMENT

fn check_declared_placements(plan: &DistributedExecutionPlan) -> Result<(), ValidateError> {
    for (index, placement) in plan.declared_placement.placements.iter().enumerate() {
        let later = &plan.declared_placement.placements[index + 1..];
        if let Some(conflict) = later.iter().find(|p| p.value == placement.value) {
            return Err(ValidateError::ConflictingContracts {
                value: placement.value,
                first: contract_fingerprint(&placement.contract),
                second: contract_fingerprint(&conflict.contract),
            });
        }
        if let PlacementContract::Replicated { primary, replicas } = &placement.contract {
            if replicas.is_empty() {
                return Err(ValidateError::AbsentReplicaSet {
                    value: placement.value,
                    primary: *primary,
                });
            }
            if !replicas.contains(primary) {
                return Err(ValidateError::PrimaryOutsideReplicaSet {
                    value: placement.value,
                    primary: *primary,
                });
            }
            for (position, replica) in replicas.iter().enumerate() {
                if replicas[position + 1..]
                    .iter()
                    .any(|later| later == replica)
                {
                    return Err(ValidateError::RepeatedReplica {
                        value: placement.value,
                        partition: *replica,
                    });
                }
            }
        }
    }
    Ok(())
}

fn check_partition_placements(plan: &DistributedExecutionPlan) -> Result<(), ValidateError> {
    for partition in &plan.partitions {
        for placement in &partition.placements {
            let declared = placement_for(plan, placement.value);
            let matches_plan = matches!(
                declared,
                Some(declared)
                    if declared.contract == placement.contract
                        && copy_holder(&placement.contract, partition.id)
            );
            if !matches_plan {
                return Err(ValidateError::PartitionContractMismatch {
                    value: placement.value,
                    partition: partition.id,
                });
            }
        }
    }
    Ok(())
}

fn check_shared_launches(plan: &DistributedExecutionPlan) -> Result<(), ValidateError> {
    for (index, partition) in plan.partitions.iter().enumerate() {
        for (position, launch) in partition.launches.iter().enumerate() {
            if partition.launches[position + 1..]
                .iter()
                .any(|later| later == launch)
            {
                return Err(ValidateError::SharedLaunch {
                    launch: *launch,
                    first: partition.id,
                    second: partition.id,
                });
            }
            if let Some(other) = plan.partitions[index + 1..]
                .iter()
                .find(|p| p.launches.contains(launch))
            {
                return Err(ValidateError::SharedLaunch {
                    launch: *launch,
                    first: partition.id,
                    second: other.id,
                });
            }
        }
    }
    Ok(())
}

// COMPLETENESS

fn check_completeness(
    plan: &DistributedExecutionPlan,
    launches: &[LaunchId],
    barriers: &[BarrierId],
) -> Result<(), ValidateError> {
    for operation in &plan.graph.operations {
        match operation {
            ExecutionOperation::Transfer(transfer) => {
                check_transfer_completeness(plan, transfer, launches, barriers)?;
            }
            ExecutionOperation::Collective(collective) => {
                check_collective_completeness(plan, collective)?;
            }
            ExecutionOperation::Launch(_) | ExecutionOperation::Barrier(_) => {}
        }
    }
    Ok(())
}

fn check_collective_completeness(
    plan: &DistributedExecutionPlan,
    collective: &CollectiveOperation,
) -> Result<(), ValidateError> {
    let reference = OpRef::Collective(collective.id);
    if placement_for(plan, collective.value).is_none() {
        return Err(ValidateError::AbsentOwner {
            value: collective.value,
            reference,
        });
    }
    if collective.byte_count == 0 {
        return Err(ValidateError::AbsentByteContract {
            value: collective.value,
            reference,
            byte_count: 0,
        });
    }
    Ok(())
}

fn check_transfer_completeness(
    plan: &DistributedExecutionPlan,
    transfer: &TransferOperation,
    launches: &[LaunchId],
    barriers: &[BarrierId],
) -> Result<(), ValidateError> {
    let reference = OpRef::Transfer(transfer.id);

    let Some(placement) = placement_for(plan, transfer.value) else {
        return Err(ValidateError::AbsentOwner {
            value: transfer.value,
            reference,
        });
    };

    if transfer.producer_generation == 0 || transfer.consumer_generation == 0 {
        return Err(ValidateError::AbsentGeneration {
            value: transfer.value,
            reference,
            producer_generation: transfer.producer_generation,
            consumer_generation: transfer.consumer_generation,
        });
    }
    if transfer.consumer_generation < transfer.producer_generation {
        return Err(ValidateError::GenerationOrder {
            value: transfer.value,
            reference,
            producer_generation: transfer.producer_generation,
            consumer_generation: transfer.consumer_generation,
        });
    }

    if transfer.byte_count == 0 {
        return Err(ValidateError::AbsentByteContract {
            value: transfer.value,
            reference,
            byte_count: transfer.byte_count,
        });
    }

    if transfer.completion_boundary.barriers.is_empty()
        && transfer.completion_boundary.launches.is_empty()
    {
        return Err(ValidateError::AbsentCompletionBoundary {
            value: transfer.value,
            reference,
        });
    }

    if transfer.completion_boundary.launches.is_empty() {
        return Err(ValidateError::AbsentProducer {
            value: transfer.value,
            reference,
        });
    }

    let destination_runs_launches = plan
        .partitions
        .iter()
        .any(|partition| partition.id == transfer.destination && !partition.launches.is_empty());
    if !copy_holder(&placement.contract, transfer.destination) && !destination_runs_launches {
        return Err(ValidateError::AbsentConsumer {
            value: transfer.value,
            reference,
            source: transfer.source,
            destination: transfer.destination,
        });
    }

    for barrier in &transfer.completion_boundary.barriers {
        if !barriers.contains(barrier) {
            return Err(ValidateError::DanglingBoundaryReference {
                reference,
                boundary_ref: BoundaryRef::Barrier(*barrier),
            });
        }
    }
    for launch in &transfer.completion_boundary.launches {
        if !launches.contains(launch) {
            return Err(ValidateError::DanglingBoundaryReference {
                reference,
                boundary_ref: BoundaryRef::Launch(*launch),
            });
        }
    }

    Ok(())
}

// MOVE AGREEMENT

fn check_mover_agreement(plan: &DistributedExecutionPlan) -> Result<(), ValidateError> {
    for (index, operation) in plan.graph.operations.iter().enumerate() {
        let Some((value, byte_count, reference)) = mover(operation) else {
            continue;
        };
        for later in &plan.graph.operations[index + 1..] {
            let Some((later_value, later_byte, later_reference)) = mover(later) else {
                continue;
            };
            if later_value != value {
                continue;
            }
            if later_byte != byte_count {
                return Err(ValidateError::ConflictingByteContract {
                    value,
                    first_byte_count: byte_count,
                    second_byte_count: later_byte,
                    second: later_reference,
                });
            }
            if let (OpRef::Transfer(first), OpRef::Transfer(second)) = (reference, later_reference)
            {
                if identical_move(plan, first, second) {
                    return Err(ValidateError::RepeatedMove {
                        value,
                        original: first,
                        duplicate: second,
                    });
                }
            }
        }
    }
    Ok(())
}

/// The (value, byte count, reference) of a value-moving operation.
fn mover(operation: &ExecutionOperation) -> Option<(SemanticValueId, u64, OpRef)> {
    match operation {
        ExecutionOperation::Transfer(transfer) => Some((
            transfer.value,
            transfer.byte_count,
            OpRef::Transfer(transfer.id),
        )),
        ExecutionOperation::Collective(collective) => Some((
            collective.value,
            collective.byte_count,
            OpRef::Collective(collective.id),
        )),
        ExecutionOperation::Launch(_) | ExecutionOperation::Barrier(_) => None,
    }
}

/// Whether two transfers declare the identical move of one value.
fn identical_move(plan: &DistributedExecutionPlan, first: TransferId, second: TransferId) -> bool {
    let first_transfer = find_transfer(plan, first);
    let second_transfer = find_transfer(plan, second);
    match (first_transfer, second_transfer) {
        (Some(a), Some(b)) => {
            a.source == b.source
                && a.destination == b.destination
                && a.producer_generation == b.producer_generation
                && a.consumer_generation == b.consumer_generation
        }
        _ => false,
    }
}

fn find_transfer(plan: &DistributedExecutionPlan, id: TransferId) -> Option<&TransferOperation> {
    plan.graph
        .operations
        .iter()
        .find_map(|operation| match operation {
            ExecutionOperation::Transfer(transfer) if transfer.id == id => Some(transfer),
            _ => None,
        })
}

// CYCLES

fn check_cycles(plan: &DistributedExecutionPlan) -> Result<(), ValidateError> {
    let transfers: Vec<&TransferOperation> = plan
        .graph
        .operations
        .iter()
        .filter_map(|operation| match operation {
            ExecutionOperation::Transfer(transfer) => Some(transfer),
            _ => None,
        })
        .collect();

    for transfer in &transfers {
        if transfer.source == transfer.destination {
            return Err(ValidateError::SelfMove {
                value: transfer.value,
                transfer: transfer.id,
            });
        }
    }

    let mut values: Vec<SemanticValueId> = Vec::new();
    for transfer in &transfers {
        if !values.contains(&transfer.value) {
            values.push(transfer.value);
        }
    }
    for value in values {
        let group: Vec<&TransferOperation> = transfers
            .iter()
            .filter(|transfer| transfer.value == value)
            .copied()
            .collect();
        if let Some((first, second)) = directed_cycle(&group) {
            return Err(ValidateError::ValueMoveCycle {
                value,
                first,
                second,
            });
        }
    }
    Ok(())
}

/// Detect a directed cycle in one value's move graph.
///
/// Edge `producer → consumer` when `consumer.source == producer.destination`
/// and `consumer.producer_generation <= producer.consumer_generation` (the
/// copy lands where the consumer reads it, at an overlapping generation).
/// Returns the back-edge pair `(node, successor)` that closes the cycle.
fn directed_cycle(transfers: &[&TransferOperation]) -> Option<(TransferId, TransferId)> {
    let mut colors = vec![0u8; transfers.len()];
    for start in 0..transfers.len() {
        if colors[start] != 0 {
            continue;
        }
        if let Some(back_edge) = visit(start, transfers, &mut colors) {
            return Some(back_edge);
        }
    }
    None
}

fn visit(
    node: usize,
    transfers: &[&TransferOperation],
    colors: &mut [u8],
) -> Option<(TransferId, TransferId)> {
    colors[node] = 1;
    for (candidate, other) in transfers.iter().enumerate() {
        if colors[candidate] == 2 || !feeds(transfers[node], other) {
            continue;
        }
        if colors[candidate] == 1 {
            return Some((transfers[node].id, other.id));
        }
        if let Some(back_edge) = visit(candidate, transfers, colors) {
            return Some(back_edge);
        }
    }
    colors[node] = 2;
    None
}

/// Edge predicate: the producer's delivery feeds the consumer's read.
fn feeds(producer: &TransferOperation, consumer: &TransferOperation) -> bool {
    consumer.source == producer.destination
        && consumer.producer_generation <= producer.consumer_generation
}

// COLLECTIVES

fn check_collectives(plan: &DistributedExecutionPlan) -> Result<(), ValidateError> {
    for operation in &plan.graph.operations {
        let ExecutionOperation::Collective(collective) = operation else {
            continue;
        };
        // Keep this match exhaustive. A new collective kind must choose its
        // admission policy rather than silently passing.
        match collective.kind {
            CollectiveKind::Broadcast => {
                if !collective.participants.contains(&collective.source) {
                    return Err(ValidateError::UnadmittedCollective {
                        value: collective.value,
                        collective: collective.id,
                        violation: CollectiveViolation::SourceNotParticipant {
                            source: collective.source,
                        },
                    });
                }
                if collective.participants.len() < 2 {
                    return Err(ValidateError::UnadmittedCollective {
                        value: collective.value,
                        collective: collective.id,
                        violation: CollectiveViolation::TooFewParticipants {
                            count: collective.participants.len() as u64,
                        },
                    });
                }
                if let Some(owner) = declared_owner(plan, collective.value)
                    .filter(|owner| *owner != collective.source)
                {
                    return Err(ValidateError::UnadmittedCollective {
                        value: collective.value,
                        collective: collective.id,
                        violation: CollectiveViolation::NotOwnerOriginated {
                            owner,
                            source: collective.source,
                        },
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "validate_test.rs"]
mod validate_tests;
