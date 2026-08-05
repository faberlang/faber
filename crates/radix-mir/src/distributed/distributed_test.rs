//! MD2-D1 fixture tests: the CS-1 declared-placement shape over a synthetic
//! two-partition device program with cross-partition buffer references.
//!
//! Proves the MD2-D1 done-when: two derived `DeviceProgramPartition`s + an
//! explicit `DistributedExecutionGraph` (transfers + Broadcast + Barrier +
//! commit boundary), byte-deterministic derivation for identical input, the
//! full Transfer fact set, the closed Broadcast-only collective set, and the
//! stable canonical encoding.

use super::*;
use crate::abi::{
    MirKernelDispatchSize, MirKernelResourceAccess, MirKernelWorkgroupCount, MirWorkgroupSize,
};
use crate::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit,
    ObservationCadence, ResultBuffer,
};
use crate::device_semantics::SemanticValueId;
use crate::kernel_plan::CollectionKernelPlan;
use crate::layout::MirTensorStorageLayout;
use crate::{MirFunctionId, MirType};
use radix_types::{NumericWidth, Primitive, TypeTable};

/// CS-1 declared per-partition budget: 160 MiB (md0-mode-fixtures.md §3).
const CS1_PARTITION_BUDGET_BYTES: u64 = 160 * 1024 * 1024;

// -----------------------------------------------------------------------
// Fixture helpers
// -----------------------------------------------------------------------

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn buffer_identity(
    id: u32,
    name: &str,
    role: BufferRole,
    lifetime: BufferLifetime,
) -> BufferIdentity {
    BufferIdentity {
        id: BufferId(id),
        name: name.to_owned(),
        role,
        storage: MirTensorStorageLayout::DeviceHandle,
        lifetime,
    }
}

fn buffer_version(number: u32, ty: MirType, count: u64) -> BufferVersion {
    BufferVersion {
        version: number,
        element_ty: ty,
        element_count: count,
    }
}

fn resource(
    identity: &BufferIdentity,
    version: &BufferVersion,
    group: u32,
    binding: u32,
    access: MirKernelResourceAccess,
) -> DeviceResource {
    DeviceResource {
        buffer: identity.clone(),
        version: version.clone(),
        binding: Binding { group, binding },
        access,
    }
}

fn launch_plan() -> KernelLaunchPlan {
    KernelLaunchPlan {
        workgroup: MirWorkgroupSize { x: 64, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
        workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
    }
}

fn kernel(entry: &str, resources: Vec<DeviceResource>) -> KernelUnit {
    KernelUnit {
        function: MirFunctionId(0),
        entry: entry.to_owned(),
        plan: CollectionKernelPlan::Elementwise,
        resources,
        launch: launch_plan(),
    }
}

fn launch(id: u32, kernel_index: usize) -> LaunchUnit {
    LaunchUnit {
        id: LaunchId(id),
        kernel_index,
    }
}

fn fact(value: u32, buffer: u32, contract: PlacementContract, byte_count: u64) -> PlacementFact {
    PlacementFact {
        value: SemanticValueId(value),
        buffer: BufferId(buffer),
        contract,
        byte_count,
    }
}

/// The CS-1 mechanics fixture: a synthetic two-partition device program with
/// cross-partition buffer references (the full SmolLM2 materialization is
/// GI1/MD4A; the mechanics proof uses a synthetic program per the MD2
/// delivery spec §P1 fixture row).
///
/// Buffers: W/I (P0 inputs), M/A/C (P0 intermediates), B (P1 intermediate),
/// R (replicated [P0 primary, P1]), O (P1-owned output, cross-consumed by a
/// P0 launch, declared result). Launches: L1 (P0, writes M), L2 (P1, reads M
/// cross-partition), L3 (P1, writes O), L4 (P0, reads R), L5 (P0, reads O
/// cross-partition).
fn cs1_program() -> (TypeTable, DeviceProgram) {
    let mut types = TypeTable::new();
    let f32 = f32_ty(&mut types);

    let w = buffer_identity(1, "weights", BufferRole::Input, BufferLifetime::PerProgram);
    let i = buffer_identity(2, "input", BufferRole::Input, BufferLifetime::PerProgram);
    let m = buffer_identity(3, "mid_a", BufferRole::InOut, BufferLifetime::PerStep);
    let a = buffer_identity(4, "mid_b", BufferRole::InOut, BufferLifetime::PerStep);
    let b = buffer_identity(5, "mid_c", BufferRole::InOut, BufferLifetime::PerStep);
    let r = buffer_identity(6, "shared", BufferRole::Input, BufferLifetime::PerProgram);
    let o = buffer_identity(7, "out", BufferRole::InOut, BufferLifetime::PerProgram);
    let c = buffer_identity(8, "mid_d", BufferRole::InOut, BufferLifetime::PerStep);

    let v_w = buffer_version(1, f32, 1000);
    let v_i = buffer_version(1, f32, 512);
    let v_m = buffer_version(1, f32, 2048);
    let v_a = buffer_version(1, f32, 1024);
    let v_b = buffer_version(1, f32, 2048);
    let v_r = buffer_version(1, f32, 4096);
    let v_o = buffer_version(1, f32, 512);
    let v_c = buffer_version(1, f32, 1024);

    let k0 = kernel(
        "proj",
        vec![
            resource(&w, &v_w, 0, 0, MirKernelResourceAccess::Read),
            resource(&i, &v_i, 0, 1, MirKernelResourceAccess::Read),
            resource(&m, &v_m, 0, 2, MirKernelResourceAccess::Write),
        ],
    );
    let k1 = kernel(
        "hidden",
        vec![
            resource(&m, &v_m, 0, 0, MirKernelResourceAccess::Read),
            resource(&r, &v_r, 0, 1, MirKernelResourceAccess::Read),
            resource(&b, &v_b, 0, 2, MirKernelResourceAccess::Write),
        ],
    );
    let k2 = kernel(
        "output",
        vec![
            resource(&b, &v_b, 0, 0, MirKernelResourceAccess::Read),
            resource(&o, &v_o, 0, 1, MirKernelResourceAccess::Write),
        ],
    );
    let k3 = kernel(
        "aux",
        vec![
            resource(&r, &v_r, 0, 0, MirKernelResourceAccess::Read),
            resource(&a, &v_a, 0, 1, MirKernelResourceAccess::Write),
        ],
    );
    let k4 = kernel(
        "consume",
        vec![
            resource(&o, &v_o, 0, 0, MirKernelResourceAccess::Read),
            resource(&c, &v_c, 0, 1, MirKernelResourceAccess::Write),
        ],
    );

    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels = vec![k0, k1, k2, k3, k4];
    program.launches = vec![launch(1, 0), launch(2, 1), launch(3, 2), launch(4, 3), launch(5, 4)];
    program.results = vec![ResultBuffer {
        buffer: o.clone(),
        version: v_o.clone(),
        role: BufferRole::InOut,
        produced_by: LaunchId(3),
        cadence: ObservationCadence::EndOfRun,
    }];

    program.validate().expect("fixture program must validate");
    (types, program)
}

/// The CS-1 declared placement facts: one fact per program buffer, keyed by
/// semantic value id, each naming one owner or an explicit replication
/// contract with its declared byte contract.
fn cs1_facts() -> Vec<PlacementFact> {
    let p0 = PartitionId(0);
    let p1 = PartitionId(1);
    vec![
        fact(1, 1, PlacementContract::Owned { owner: p0 }, 4_000),
        fact(2, 2, PlacementContract::Owned { owner: p0 }, 2_048),
        fact(3, 3, PlacementContract::Owned { owner: p0 }, 8_192),
        fact(4, 4, PlacementContract::Owned { owner: p0 }, 4_096),
        fact(
            5,
            5,
            PlacementContract::Owned { owner: p1 },
            8_192,
        ),
        fact(
            6,
            6,
            PlacementContract::Replicated {
                primary: p0,
                replicas: vec![p0, p1],
            },
            16_384,
        ),
        fact(7, 7, PlacementContract::Owned { owner: p1 }, 2_048),
        fact(8, 8, PlacementContract::Owned { owner: p0 }, 4_096),
    ]
}

/// Assemble the CS-1 logical plan: declared placement (facts + the CS-1
/// 160 MiB partition budgets + one declared objective) + the derivation.
fn cs1_plan(
    facts: &[PlacementFact],
    partitions: Vec<DeviceProgramPartition>,
    graph: DistributedExecutionGraph,
) -> DistributedExecutionPlan {
    let mut placements: Vec<LogicalPlacement> =
        facts.iter().map(LogicalPlacement::from_fact).collect();
    placements.sort_by_key(|placement| placement.value.0);
    let declared_placement = DeclaredPlacement {
        placements,
        constraints: vec![
            PlacementConstraint::PartitionBudgetBytes {
                partition: PartitionId(0),
                budget_bytes: CS1_PARTITION_BUDGET_BYTES,
            },
            PlacementConstraint::PartitionBudgetBytes {
                partition: PartitionId(1),
                budget_bytes: CS1_PARTITION_BUDGET_BYTES,
            },
        ],
        objectives: vec![PlacementObjective::MinimizeTransferBytes],
    };
    DistributedExecutionPlan::assemble(
        declared_placement,
        partitions,
        graph,
        PlanVersion(LOGICAL_PLAN_VERSION),
    )
}

// -----------------------------------------------------------------------
// CS-1 derivation: two partitions + explicit graph
// -----------------------------------------------------------------------

#[test]
fn cs1_derives_two_partitions_and_an_explicit_graph() {
    let (_types, program) = cs1_program();
    let facts = cs1_facts();
    let (partitions, graph) = derive(&program, &facts);

    assert_eq!(partitions.len(), 2);
    assert_eq!(partitions[0].id, PartitionId(0));
    assert_eq!(partitions[1].id, PartitionId(1));

    // Partition 0: kernels [0, 3, 4], launches [1, 4, 5], buffers
    // [1, 2, 3, 4, 6, 8] (owner, or replica of R), no results, placements
    // for values 1, 2, 3, 4, 6, 8.
    assert_eq!(partitions[0].kernels, vec![0, 3, 4]);
    assert_eq!(
        partitions[0].launches,
        vec![LaunchId(1), LaunchId(4), LaunchId(5)]
    );
    assert_eq!(
        partitions[0].buffers,
        vec![BufferId(1), BufferId(2), BufferId(3), BufferId(4), BufferId(6), BufferId(8)]
    );
    assert!(partitions[0].results.is_empty());
    let p0_values: Vec<u32> = partitions[0].placements.iter().map(|p| p.value.0).collect();
    assert_eq!(p0_values, vec![1, 2, 3, 4, 6, 8]);

    // Partition 1: kernels [1, 2], launches [2, 3], buffers [5, 6, 7]
    // (owner, or replica of R), the O result row, placements for values
    // 5, 6, 7.
    assert_eq!(partitions[1].kernels, vec![1, 2]);
    assert_eq!(partitions[1].launches, vec![LaunchId(2), LaunchId(3)]);
    assert_eq!(
        partitions[1].buffers,
        vec![BufferId(5), BufferId(6), BufferId(7)]
    );
    assert_eq!(
        partitions[1].results,
        vec![ResultRef {
            buffer: BufferId(7),
            version: 1,
            produced_by: LaunchId(3),
        }]
    );
    let p1_values: Vec<u32> = partitions[1].placements.iter().map(|p| p.value.0).collect();
    assert_eq!(p1_values, vec![5, 6, 7]);

    // Explicit graph: 5 launches, 2 transfers (M P0→P1, O P1→P0), 1
    // broadcast (R), 1 barrier at O's declared commit boundary.
    assert_eq!(graph.launches().len(), 5);
    assert_eq!(graph.transfers().len(), 2);
    assert_eq!(graph.collectives().len(), 1);
    assert_eq!(graph.barriers().len(), 1);
}

// -----------------------------------------------------------------------
// Transfer fact set
// -----------------------------------------------------------------------

#[test]
fn transfer_carries_byte_direction_dtype_layout_path_generations_boundary() {
    let (mut types, program) = cs1_program();
    let f32 = f32_ty(&mut types);
    let (_, graph) = derive(&program, &cs1_facts());

    let transfers = graph.transfers();
    assert_eq!(transfers.len(), 2);

    // Transfer 0: M (value 3) P0 → P1, read by launch 2 on P1.
    let m = transfers[0];
    assert_eq!(m.id, TransferId(0));
    assert_eq!(m.value, SemanticValueId(3));
    assert_eq!(m.source, PartitionId(0));
    assert_eq!(m.destination, PartitionId(1));
    assert_eq!(m.byte_count, 8_192);
    assert_eq!(m.direction, TransferDirection::BIDI);
    assert_eq!(m.path_label, TransportPath::HostStaged);
    assert_eq!(m.producer_generation, 1);
    assert_eq!(m.consumer_generation, 1);
    assert_eq!(m.element_ty.semantic_id(), f32.semantic_id());
    assert!(m.completion_boundary.barriers.is_empty());
    assert_eq!(m.completion_boundary.launches, vec![LaunchId(1)]);

    // Transfer 1: O (value 7) P1 → P0, read by launch 5 on P0, committing
    // at the O result barrier.
    let o = transfers[1];
    assert_eq!(o.id, TransferId(1));
    assert_eq!(o.value, SemanticValueId(7));
    assert_eq!(o.source, PartitionId(1));
    assert_eq!(o.destination, PartitionId(0));
    assert_eq!(o.byte_count, 2_048);
    assert_eq!(o.direction, TransferDirection::BIDI);
    assert_eq!(o.layout, MirTensorStorageLayout::DeviceHandle);
    assert_eq!(o.path_label, TransportPath::HostStaged);
    assert_eq!(o.producer_generation, 1);
    assert_eq!(o.consumer_generation, 1);
    assert_eq!(o.completion_boundary.barriers, vec![BarrierId(0)]);
    assert_eq!(o.completion_boundary.launches, vec![LaunchId(3)]);

    // The replicated value R (value 6) has no per-reference transfer — its
    // copies are local by contract; one broadcast represents the
    // replication composition.
    assert!(transfers.iter().all(|t| t.value != SemanticValueId(6)));
}

// -----------------------------------------------------------------------
// Collective admissible set: Broadcast only
// -----------------------------------------------------------------------

#[test]
fn collective_broadcast_is_the_only_admitted_kind() {
    let (_types, program) = cs1_program();
    let (_, graph) = derive(&program, &cs1_facts());

    let collectives = graph.collectives();
    assert_eq!(collectives.len(), 1);
    let broadcast = collectives[0];
    assert_eq!(broadcast.id, CollectiveId(0));
    assert_eq!(broadcast.value, SemanticValueId(6));
    assert!(matches!(broadcast.kind, CollectiveKind::Broadcast));
    assert_eq!(broadcast.source, PartitionId(0));
    assert_eq!(
        broadcast.participants,
        vec![PartitionId(0), PartitionId(1)]
    );
    assert_eq!(broadcast.byte_count, 16_384);

    // The graph API rejects any other kind: `broadcast` is the only
    // constructor and `CollectiveKind` has exactly one variant, so an
    // unsupported collective (AllReduce/AllGather/AllToAll/ReduceScatter are
    // declared unsupported) cannot be named by a plan.
    let constructed = CollectiveOperation::broadcast(
        CollectiveId(9),
        SemanticValueId(9),
        PartitionId(0),
        vec![PartitionId(0), PartitionId(1)],
        42,
    );
    assert!(matches!(constructed.kind, CollectiveKind::Broadcast));
}

// -----------------------------------------------------------------------
// Barrier + declared commit boundary
// -----------------------------------------------------------------------

#[test]
fn result_barrier_marks_the_declared_commit_boundary() {
    let (_types, program) = cs1_program();
    let (_, graph) = derive(&program, &cs1_facts());

    let barriers = graph.barriers();
    assert_eq!(barriers.len(), 1);
    assert_eq!(barriers[0].id, BarrierId(0));
    assert_eq!(
        barriers[0].partitions,
        vec![PartitionId(0), PartitionId(1)]
    );

    // The graph-level declared commit boundary references the O barrier and
    // O's producing launch — never "execution generation".
    assert_eq!(graph.commit_boundary.barriers, vec![BarrierId(0)]);
    assert_eq!(graph.commit_boundary.launches, vec![LaunchId(3)]);

    // O's transfer commits at that boundary.
    let o_transfer = graph
        .transfers()
        .into_iter()
        .find(|t| t.value == SemanticValueId(7))
        .expect("O transfer");
    assert_eq!(o_transfer.completion_boundary.barriers, vec![BarrierId(0)]);
    assert_eq!(o_transfer.completion_boundary.launches, vec![LaunchId(3)]);
}

// -----------------------------------------------------------------------
// Deterministic derivation (exit-gate bullet 1)
// -----------------------------------------------------------------------

#[test]
fn derive_is_byte_deterministic_for_identical_input() {
    let (_types, program) = cs1_program();
    let facts = cs1_facts();

    let (partitions_a, graph_a) = derive(&program, &facts);
    let (partitions_b, graph_b) = derive(&program, &facts);

    assert_eq!(partitions_a, partitions_b);
    assert_eq!(graph_a, graph_b);

    let plan_a = cs1_plan(&facts, partitions_a, graph_a);
    let plan_b = cs1_plan(&facts, partitions_b, graph_b);
    assert_eq!(encode_logical_plan(&plan_a), encode_logical_plan(&plan_b));
}

// -----------------------------------------------------------------------
// Canonical encoding
// -----------------------------------------------------------------------

#[test]
fn canonical_encoding_is_stable_and_distinguishes_plans() {
    let (_types, program) = cs1_program();
    let facts = cs1_facts();
    let (partitions, graph) = derive(&program, &facts);
    let plan = cs1_plan(&facts, partitions, graph);

    let bytes = encode_logical_plan(&plan);
    assert!(bytes.starts_with(CANONICAL_MAGIC));
    assert_eq!(bytes, encode_logical_plan(&plan));

    // A different declared placement (M owned by P1 instead of P0) changes
    // the derived graph and therefore the canonical bytes.
    let mut moved_facts = facts.clone();
    let m_fact = moved_facts
        .iter_mut()
        .find(|f| f.value == SemanticValueId(3))
        .expect("M fact");
    m_fact.contract = PlacementContract::Owned {
        owner: PartitionId(1),
    };
    let (partitions_b, graph_b) = derive(&program, &moved_facts);
    let plan_b = cs1_plan(&moved_facts, partitions_b, graph_b);
    assert_ne!(bytes, encode_logical_plan(&plan_b));
}

// -----------------------------------------------------------------------
// Plan assembly + declared placement records
// -----------------------------------------------------------------------

#[test]
fn plan_assembles_declared_placement_constraints_and_version() {
    let (_types, program) = cs1_program();
    let facts = cs1_facts();
    let (partitions, graph) = derive(&program, &facts);
    let plan = cs1_plan(&facts, partitions, graph);

    assert_eq!(plan.version, PlanVersion(LOGICAL_PLAN_VERSION));
    assert_eq!(plan.partitions.len(), 2);

    // Declared placements are keyed by semantic value id (ascending), and
    // each names one owner or an explicit replication contract (MD-A5).
    let values: Vec<u32> = plan
        .declared_placement
        .placements
        .iter()
        .map(|p| p.value.0)
        .collect();
    assert_eq!(values, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(matches!(
        plan.declared_placement.placements[5].contract,
        PlacementContract::Replicated {
            primary,
            ref replicas,
        } if primary == PartitionId(0)
            && replicas == &vec![PartitionId(0), PartitionId(1)]
    ));

    // CS-1 declared budgets: 2 virtual partitions @ 160 MiB (FC14).
    assert_eq!(plan.declared_placement.constraints.len(), 2);
    assert!(plan.declared_placement.constraints.contains(
        &PlacementConstraint::PartitionBudgetBytes {
            partition: PartitionId(0),
            budget_bytes: CS1_PARTITION_BUDGET_BYTES,
        }
    ));
    assert!(plan.declared_placement.constraints.contains(
        &PlacementConstraint::PartitionBudgetBytes {
            partition: PartitionId(1),
            budget_bytes: CS1_PARTITION_BUDGET_BYTES,
        }
    ));
    assert_eq!(
        plan.declared_placement.objectives,
        vec![PlacementObjective::MinimizeTransferBytes]
    );
}

// -----------------------------------------------------------------------
// Cross-partition write-back path
// -----------------------------------------------------------------------

#[test]
fn cross_partition_write_emits_a_writeback_transfer() {
    let mut types = TypeTable::new();
    let f32 = f32_ty(&mut types);

    let a = buffer_identity(1, "mid_a", BufferRole::InOut, BufferLifetime::PerStep);
    let b = buffer_identity(2, "mid_c", BufferRole::InOut, BufferLifetime::PerStep);
    let v_a = buffer_version(1, f32, 1024);
    let v_b = buffer_version(1, f32, 2048);

    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels = vec![kernel(
        "double_write",
        vec![
            resource(&a, &v_a, 0, 0, MirKernelResourceAccess::Write),
            resource(&b, &v_b, 0, 1, MirKernelResourceAccess::Write),
        ],
    )];
    program.launches = vec![launch(1, 0)];
    program.validate().expect("fixture program must validate");

    let facts = vec![
        fact(
            1,
            1,
            PlacementContract::Owned {
                owner: PartitionId(0),
            },
            4_096,
        ),
        fact(
            2,
            2,
            PlacementContract::Owned {
                owner: PartitionId(1),
            },
            8_192,
        ),
    ];

    let (partitions, graph) = derive(&program, &facts);

    // The launch runs on P0 (owner of its first write buffer A); its write
    // of B (owned by P1) is a cross-partition write → one write-back
    // transfer P0 → P1.
    assert_eq!(partitions.len(), 2);
    assert_eq!(partitions[0].launches, vec![LaunchId(1)]);
    assert!(partitions[1].launches.is_empty());

    let transfers = graph.transfers();
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].id, TransferId(0));
    assert_eq!(transfers[0].value, SemanticValueId(2));
    assert_eq!(transfers[0].source, PartitionId(0));
    assert_eq!(transfers[0].destination, PartitionId(1));
    assert_eq!(transfers[0].byte_count, 8_192);
    assert_eq!(transfers[0].direction, TransferDirection::BIDI);
    assert_eq!(transfers[0].producer_generation, 1);
    assert_eq!(transfers[0].consumer_generation, 1);
    assert_eq!(
        transfers[0].completion_boundary.launches,
        vec![LaunchId(1)]
    );
}

// -----------------------------------------------------------------------
// Degenerate declared placement
// -----------------------------------------------------------------------

#[test]
fn empty_facts_derive_no_partitions_and_no_transfers() {
    let (_types, program) = cs1_program();
    let (partitions, graph) = derive(&program, &[]);

    assert!(partitions.is_empty());
    assert!(graph.transfers().is_empty());
    assert!(graph.collectives().is_empty());
}
