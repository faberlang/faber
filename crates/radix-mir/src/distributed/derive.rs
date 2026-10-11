//! Deterministic derivation of per-device partitions + the execution graph
//! (MD2-D1, exit-gate bullet 1).
//!
//! `derive(program, facts)` is a **pure** function: identical `(program,
//! facts)` produce identical output, byte-for-byte. Every iteration order is
//! a program order or an ascending id order — nothing here iterates a map or
//! a set, so the derivation is reproducible across runs and processes.
//!
//! The derivation is also **total**: any input derives a deterministic
//! result. Consistency rules (missing owners, conflicting replica contracts,
//! undeclared byte contracts, cycles) are the fail-before-launch validation
//! gate's (MD2-V1), not this function's — this module never rejects and
//! never invents facts.

use crate::MirType;
use crate::abi::MirKernelResourceAccess;
use crate::device_program::{BufferId, DeviceProgram, LaunchId, LaunchUnit};
use crate::distributed::types::{
    BarrierId, BarrierOperation, CollectiveId, CollectiveOperation, DeviceProgramPartition,
    DistributedExecutionGraph, ExecutionCommitBoundary, ExecutionOperation, LaunchOperation,
    LogicalPlacement, PartitionId, PlacementContract, PlacementFact, ResultRef, TransferDirection,
    TransferId, TransferOperation, TransportPath,
};
use crate::layout::MirTensorStorageLayout;

/// Deterministically derive the per-device partitions and the upper execution
/// graph for one semantic program + declared placement facts.
///
/// Returns the partition list (ascending by partition id) and the execution
/// graph. The caller wraps them in a [`DistributedExecutionPlan`] together
/// with the declared placement records (constraints/objectives ride the plan,
/// not the derivation input).
///
/// # Derivation rules (deterministic)
///
/// 1. **Partition universe** — every partition referenced by a declared fact
///    (owner or replica), ascending; the smallest partition is the default
///    owner for buffers with no declared fact.
/// 2. **Launch placement** — a launch runs on the partition owning its first
///    write resource's buffer; a launch without a write resource runs on the
///    owner of its first resource's buffer; a resource-less launch runs on
///    the default owner.
/// 3. **Cross-partition references** — a launch on a partition that holds
///    **no copy** of a buffer emits one transfer: reads move
///    `owner → launch partition`, writes move `launch partition → owner` (a
///    read-write cross reference emits both). Partitions holding a copy
///    (the owner, or any replica under an explicit replication contract)
///    need no transfer — copies are local by contract. Transfers are
///    deduplicated per `(buffer, version, source, destination)`. The byte
///    count is the value's **declared** byte contract — never invented. A
///    buffer with no declared fact yields no transfer (an undeclared
///    cross-device value fails the MD2-V1 completeness gate).
/// 4. **Replicated values** — a replicated buffer referenced on more than
///    one partition emits one [`CollectiveOperation`] broadcast (primary
///    replica → all participants) representing the replication composition;
///    the per-copy copies are composed at MD3 from labeled host-staged
///    transfers + local kernels (no collective library).
/// 5. **Barriers** — one barrier per declared result row at its commit
///    boundary, with the participant partition set (every partition holding
///    or referencing the result's buffer).
/// 6. **Completion boundaries** — a transfer's boundary references the
///    result barriers of the moved `(buffer, version)` and the launch that
///    produces it; the graph's declared commit boundary references every
///    result barrier and every result's producing launch.
#[must_use]
pub fn derive(
    program: &DeviceProgram,
    facts: &[PlacementFact],
) -> (Vec<DeviceProgramPartition>, DistributedExecutionGraph) {
    let partition_ids = declared_partition_ids(facts);
    let default_owner = partition_ids.first().copied().unwrap_or(PartitionId(0));

    // Typed resources of every launch (parallel to `program.launches`).
    let launch_resources: Vec<Vec<(BufferId, u32, MirType, MirKernelResourceAccess)>> = program
        .launches
        .iter()
        .map(|launch| launch_resources(program, launch))
        .collect();

    // Launch → partition assignment, parallel to `program.launches`.
    let mut launch_partitions: Vec<PartitionId> = Vec::with_capacity(program.launches.len());
    for resources in &launch_resources {
        launch_partitions.push(launch_partition(facts, default_owner, resources));
    }

    // 1. Partitions (ascending partition id).
    let partitions = build_partitions(
        &partition_ids,
        default_owner,
        facts,
        program,
        &launch_partitions,
    );

    // 2. Execution graph.
    let mut operations: Vec<ExecutionOperation> = Vec::new();

    // Result → barrier map, in program result order (barrier ids assigned
    // before the transfers so completion boundaries can reference them).
    let mut result_barrier_ids: Vec<(BufferId, u32, BarrierId)> = Vec::new();
    for (index, result) in program.results.iter().enumerate() {
        result_barrier_ids.push((
            result.buffer.id,
            result.version.version,
            #[expect(
                clippy::cast_possible_truncation,
                reason = "BarrierId is a u32 ordinal by design; result-row indices are far below u32::MAX"
            )]
            BarrierId(index as u32),
        ));
    }

    // Launches, in program order.
    for (index, launch) in program.launches.iter().enumerate() {
        operations.push(ExecutionOperation::Launch(LaunchOperation {
            launch: launch.id,
            partition: launch_partitions[index],
        }));
    }

    // Transfers, in registry order (buffer first-reference order) × program
    // launch order, deduplicated per (buffer, version, source, destination).
    append_transfers(
        facts,
        default_owner,
        program,
        &launch_partitions,
        &launch_resources,
        &result_barrier_ids,
        &mut operations,
    );

    // Collectives (one broadcast per replicated value) and barriers (one per
    // result row at its declared commit boundary).
    append_collectives_and_barriers(
        facts,
        default_owner,
        program,
        &launch_partitions,
        &launch_resources,
        &mut operations,
    );

    // Graph-level declared commit boundary.
    let mut boundary_launches: Vec<LaunchId> = Vec::new();
    for result in &program.results {
        push_launch(&mut boundary_launches, result.produced_by);
    }
    let graph = DistributedExecutionGraph {
        operations,
        commit_boundary: ExecutionCommitBoundary {
            barriers: result_barrier_ids.iter().map(|(_, _, id)| *id).collect(),
            launches: boundary_launches,
        },
    };

    (partitions, graph)
}

/// Partition records for every declared partition id, ascending.
fn build_partitions(
    partition_ids: &[PartitionId],
    default_owner: PartitionId,
    facts: &[PlacementFact],
    program: &DeviceProgram,
    launch_partitions: &[PartitionId],
) -> Vec<DeviceProgramPartition> {
    let registry = program.buffer_registry();
    let mut partitions: Vec<DeviceProgramPartition> = Vec::with_capacity(partition_ids.len());
    for &id in partition_ids {
        let mut launches: Vec<LaunchId> = Vec::new();
        let mut kernels: Vec<usize> = Vec::new();
        for (index, launch) in program.launches.iter().enumerate() {
            if launch_partitions[index] == id {
                launches.push(launch.id);
                kernels.push(launch.kernel_index);
            }
        }
        kernels.sort_unstable();
        kernels.dedup();

        let mut buffers: Vec<BufferId> = registry
            .buffers
            .iter()
            .filter(|entry| buffer_copies(facts, default_owner, entry.identity.id).contains(&id))
            .map(|entry| entry.identity.id)
            .collect();
        buffers.sort_unstable();

        let results: Vec<ResultRef> = program
            .results
            .iter()
            .filter(|result| buffer_copies(facts, default_owner, result.buffer.id).contains(&id))
            .map(|result| ResultRef {
                buffer: result.buffer.id,
                version: result.version.version,
                produced_by: result.produced_by,
            })
            .collect();

        let mut placements: Vec<LogicalPlacement> = Vec::new();
        for fact in facts {
            let involves = match &fact.contract {
                PlacementContract::Owned { owner } => *owner == id,
                PlacementContract::Replicated { replicas, .. } => replicas.contains(&id),
            };
            if involves {
                placements.push(LogicalPlacement {
                    value: fact.value,
                    contract: fact.contract.clone(),
                });
            }
        }
        placements.sort_by_key(|placement| placement.value.0);

        partitions.push(DeviceProgramPartition {
            id,
            kernels,
            launches,
            buffers,
            results,
            placements,
        });
    }
    partitions
}

/// Append one transfer per cross-partition reference, deduplicated per
/// `(buffer, version, source, destination)`.
fn append_transfers(
    facts: &[PlacementFact],
    default_owner: PartitionId,
    program: &DeviceProgram,
    launch_partitions: &[PartitionId],
    launch_resources: &[Vec<(BufferId, u32, MirType, MirKernelResourceAccess)>],
    result_barrier_ids: &[(BufferId, u32, BarrierId)],
    operations: &mut Vec<ExecutionOperation>,
) {
    let registry = program.buffer_registry();
    let mut next_transfer_id: u32 = 0;
    let mut seen_transfers: Vec<(BufferId, u32, PartitionId, PartitionId)> = Vec::new();
    for entry in &registry.buffers {
        let buffer = entry.identity.id;
        let Some(fact) = fact_for(facts, buffer) else {
            // No declared contract for the buffer: no value/byte facts to
            // carry on a transfer. The completeness gate (MD2-V1) rejects a
            // plan with an undeclared cross-device value.
            continue;
        };
        let owner = buffer_owner(facts, default_owner, buffer);
        let copies = buffer_copies(facts, default_owner, buffer);
        for (launch_index, _launch) in program.launches.iter().enumerate() {
            let launch_partition = launch_partitions[launch_index];
            if copies.contains(&launch_partition) {
                // The partition already holds a copy (owner, or a replica
                // under an explicit replication contract) — no transfer.
                continue;
            }
            for (referenced, version, element_ty, access) in &launch_resources[launch_index] {
                if *referenced != buffer {
                    continue;
                }
                // Reads move owner → reader; writes move writer → owner; a
                // read-write cross reference emits both moves.
                let moves = match access {
                    MirKernelResourceAccess::Read => vec![(owner, launch_partition)],
                    MirKernelResourceAccess::Write => vec![(launch_partition, owner)],
                    MirKernelResourceAccess::ReadWrite => {
                        vec![(owner, launch_partition), (launch_partition, owner)]
                    }
                };
                for (source, destination) in moves {
                    let key = (buffer, *version, source, destination);
                    if seen_transfers.contains(&key) {
                        continue;
                    }
                    seen_transfers.push(key);
                    let completion_boundary = ExecutionCommitBoundary {
                        barriers: barrier_ids_for(result_barrier_ids, buffer, *version),
                        launches: producing_launch(program, buffer, *version)
                            .into_iter()
                            .collect(),
                    };
                    operations.push(ExecutionOperation::Transfer(TransferOperation {
                        id: TransferId(next_transfer_id),
                        value: fact.value,
                        source,
                        destination,
                        byte_count: fact.byte_count,
                        direction: TransferDirection::BIDI,
                        element_ty: *element_ty,
                        layout: MirTensorStorageLayout::DeviceHandle,
                        path_label: TransportPath::HostStaged,
                        producer_generation: *version,
                        consumer_generation: *version,
                        completion_boundary,
                    }));
                    next_transfer_id += 1;
                }
            }
        }
    }
}

/// Append one broadcast per replicated value referenced on more than one
/// partition, then one barrier per result row at its declared commit boundary.
fn append_collectives_and_barriers(
    facts: &[PlacementFact],
    default_owner: PartitionId,
    program: &DeviceProgram,
    launch_partitions: &[PartitionId],
    launch_resources: &[Vec<(BufferId, u32, MirType, MirKernelResourceAccess)>],
    operations: &mut Vec<ExecutionOperation>,
) {
    let registry = program.buffer_registry();
    let mut next_collective_id: u32 = 0;
    for entry in &registry.buffers {
        let buffer = entry.identity.id;
        let Some(fact) = fact_for(facts, buffer) else {
            continue;
        };
        let PlacementContract::Replicated {
            primary, replicas, ..
        } = &fact.contract
        else {
            continue;
        };
        let mut participants: Vec<PartitionId> = replicas.clone();
        for (launch_index, _launch) in program.launches.iter().enumerate() {
            if launch_resources[launch_index]
                .iter()
                .any(|(referenced, _, _, _)| *referenced == buffer)
            {
                push_partition(&mut participants, launch_partitions[launch_index]);
            }
        }
        participants.sort_unstable();
        participants.dedup();
        if participants.len() < 2 {
            continue;
        }
        operations.push(ExecutionOperation::Collective(
            CollectiveOperation::broadcast(
                CollectiveId(next_collective_id),
                fact.value,
                *primary,
                participants,
                fact.byte_count,
            ),
        ));
        next_collective_id += 1;
    }

    // Barriers: one per result row at its declared commit boundary.
    for (index, result) in program.results.iter().enumerate() {
        let buffer = result.buffer.id;
        let mut participants: Vec<PartitionId> = buffer_copies(facts, default_owner, buffer);
        for (launch_index, _launch) in program.launches.iter().enumerate() {
            if launch_resources[launch_index]
                .iter()
                .any(|(referenced, _, _, _)| *referenced == buffer)
            {
                push_partition(&mut participants, launch_partitions[launch_index]);
            }
        }
        participants.sort_unstable();
        participants.dedup();
        operations.push(ExecutionOperation::Barrier(BarrierOperation {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "BarrierId is a u32 ordinal by design; result-row indices are far below u32::MAX"
            )]
            id: BarrierId(index as u32),
            partitions: participants,
        }));
    }
}

/// All distinct partition ids referenced by the declared facts, ascending.
fn declared_partition_ids(facts: &[PlacementFact]) -> Vec<PartitionId> {
    let mut ids: Vec<PartitionId> = Vec::new();
    for fact in facts {
        match &fact.contract {
            PlacementContract::Owned { owner } => push_partition(&mut ids, *owner),
            PlacementContract::Replicated { replicas, .. } => {
                for replica in replicas {
                    push_partition(&mut ids, *replica);
                }
            }
        }
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The declared fact for a buffer (the first declared fact wins).
fn fact_for(facts: &[PlacementFact], buffer: BufferId) -> Option<&PlacementFact> {
    facts.iter().find(|fact| fact.buffer == buffer)
}

/// The authoritative owner of a buffer: its declared owner (or the primary
/// replica of a replicated contract), else the default owner.
fn buffer_owner(
    facts: &[PlacementFact],
    default_owner: PartitionId,
    buffer: BufferId,
) -> PartitionId {
    match fact_for(facts, buffer) {
        Some(fact) => match &fact.contract {
            PlacementContract::Owned { owner } => *owner,
            PlacementContract::Replicated { primary, .. } => *primary,
        },
        None => default_owner,
    }
}

/// Every partition holding a complete copy of a buffer (ascending,
/// deduplicated): the replica set for a replicated contract, the owner
/// otherwise, the default owner when undeclared.
fn buffer_copies(
    facts: &[PlacementFact],
    default_owner: PartitionId,
    buffer: BufferId,
) -> Vec<PartitionId> {
    match fact_for(facts, buffer) {
        Some(fact) => match &fact.contract {
            PlacementContract::Owned { owner } => vec![*owner],
            PlacementContract::Replicated { replicas, .. } => {
                let mut sorted = replicas.clone();
                sorted.sort_unstable();
                sorted.dedup();
                sorted
            }
        },
        None => vec![default_owner],
    }
}

/// The typed resources of a launch, in kernel resource order.
fn launch_resources(
    program: &DeviceProgram,
    launch: &LaunchUnit,
) -> Vec<(BufferId, u32, MirType, MirKernelResourceAccess)> {
    let Some(kernel) = program.kernels.get(launch.kernel_index) else {
        return Vec::new();
    };
    kernel
        .resources
        .iter()
        .map(|resource| {
            (
                resource.buffer.id,
                resource.version.version,
                resource.version.element_ty,
                resource.access,
            )
        })
        .collect()
}

/// The partition that runs a launch: the owner of its first write resource,
/// else the owner of its first resource, else the default owner.
fn launch_partition(
    facts: &[PlacementFact],
    default_owner: PartitionId,
    resources: &[(BufferId, u32, MirType, MirKernelResourceAccess)],
) -> PartitionId {
    if let Some(&(buffer, _, _, _)) = resources.iter().find(|(_, _, _, access)| {
        matches!(
            access,
            MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
        )
    }) {
        return buffer_owner(facts, default_owner, buffer);
    }
    if let Some(&(buffer, _, _, _)) = resources.first() {
        return buffer_owner(facts, default_owner, buffer);
    }
    default_owner
}

/// The launch that produces a `(buffer, version)` (first producer wins).
fn producing_launch(program: &DeviceProgram, buffer: BufferId, version: u32) -> Option<LaunchId> {
    program
        .launches
        .iter()
        .find(|launch| {
            launch_resources(program, launch)
                .iter()
                .any(|(referenced, v, _, access)| {
                    *referenced == buffer
                        && *v == version
                        && matches!(
                            access,
                            MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                        )
                })
        })
        .map(|launch| launch.id)
}

/// The barrier ids declared at the commit boundary of a `(buffer, version)`.
fn barrier_ids_for(
    result_barrier_ids: &[(BufferId, u32, BarrierId)],
    buffer: BufferId,
    version: u32,
) -> Vec<BarrierId> {
    result_barrier_ids
        .iter()
        .filter(|(b, v, _)| *b == buffer && *v == version)
        .map(|(_, _, id)| *id)
        .collect()
}

/// Push a partition id when absent (order-preserving dedup).
fn push_partition(ids: &mut Vec<PartitionId>, id: PartitionId) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

/// Push a launch id when absent (order-preserving dedup).
fn push_launch(ids: &mut Vec<LaunchId>, id: LaunchId) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}
