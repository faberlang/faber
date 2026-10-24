//! MD2-V1 fail-before-launch validation gate tests.
//!
//! Every exit-gate check has a test: a cycle fails, a missing-owner value
//! fails, conflicting replicas fail, a collective outside the admissible
//! form fails as unsupported, a topology-inconsistent plan fails, a
//! stale-version plan fails, and the fully-complete CS-1 plan validates as
//! the positive control. Each rejection names the violated class + the
//! exact failing fact (asserted per test). The A14 regression asserts the
//! `CollectionKernelPlan` variant set stays recipe-only — no
//! Transfer/Collective/Barrier variant (fail-closed at compile time via the
//! exhaustive match; the wire-mirror half lives in radix-mir-fmir where
//! `WireCollectionKernelPlan` is defined — MD2-W1, FC18: radix-mir cannot
//! depend on radix-mir-fmir).

use super::{
    BoundaryRef, CollectiveViolation, ContractFingerprint, OpRef, TopologyRef, ValidateError,
    ValidateKind,
};
use crate::abi::{
    MirKernelDispatchSize, MirKernelResourceAccess, MirKernelWorkgroupCount, MirWorkgroupSize,
};
use crate::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit,
    ObservationCadence, ResultBuffer,
};
use crate::device_semantics::SemanticValueId;
use crate::distributed::derive;
use crate::distributed::types::{
    BarrierId, CollectiveId, CollectiveOperation, DeclaredPlacement, DeviceProgramPartition,
    DistributedExecutionGraph, DistributedExecutionPlan, ExecutionCommitBoundary,
    ExecutionOperation, LOGICAL_PLAN_VERSION, LogicalPlacement, PartitionId, PlacementConstraint,
    PlacementContract, PlacementFact, PlacementObjective, PlanVersion, TransferDirection,
    TransferId, TransferOperation, TransportPath,
};
use crate::kernel_plan::{BatchedAttentionKind, BatchedAttentionPlan, CollectionKernelPlan};
use crate::layout::MirTensorStorageLayout;
use crate::{MirFunctionId, MirType};
use radix_types::{NumericWidth, Primitive, TypeTable};

// -----------------------------------------------------------------------
// Fixture helpers (the CS-1 declared-placement shape — md0-mode-fixtures
// §3; duplicated here so the validate gate tests are self-contained).
// -----------------------------------------------------------------------

/// CS-1 declared per-partition budget: 160 MiB.
const CS1_PARTITION_BUDGET_BYTES: u64 = 160 * 1024 * 1024;

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
        shape: None,
        reduced_projection: None,
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
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
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

/// The CS-1 mechanics fixture: a synthetic two-partition device program
/// with cross-partition buffer references (see `distributed_test.rs`).
fn cs1_program() -> (TypeTable, DeviceProgram) {
    let mut types = TypeTable::new();
    let f32 = f32_ty(&mut types);

    let weights = buffer_identity(1, "weights", BufferRole::Input, BufferLifetime::PerProgram);
    let input = buffer_identity(2, "input", BufferRole::Input, BufferLifetime::PerProgram);
    let mid_a = buffer_identity(3, "mid_a", BufferRole::InOut, BufferLifetime::PerStep);
    let mid_b = buffer_identity(4, "mid_b", BufferRole::InOut, BufferLifetime::PerStep);
    let mid_c = buffer_identity(5, "mid_c", BufferRole::InOut, BufferLifetime::PerStep);
    let shared = buffer_identity(6, "shared", BufferRole::Input, BufferLifetime::PerProgram);
    let out = buffer_identity(7, "out", BufferRole::InOut, BufferLifetime::PerProgram);
    let mid_d = buffer_identity(8, "mid_d", BufferRole::InOut, BufferLifetime::PerStep);

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
            resource(&weights, &v_w, 0, 0, MirKernelResourceAccess::Read),
            resource(&input, &v_i, 0, 1, MirKernelResourceAccess::Read),
            resource(&mid_a, &v_m, 0, 2, MirKernelResourceAccess::Write),
        ],
    );
    let k1 = kernel(
        "hidden",
        vec![
            resource(&mid_a, &v_m, 0, 0, MirKernelResourceAccess::Read),
            resource(&shared, &v_r, 0, 1, MirKernelResourceAccess::Read),
            resource(&mid_c, &v_b, 0, 2, MirKernelResourceAccess::Write),
        ],
    );
    let k2 = kernel(
        "output",
        vec![
            resource(&mid_c, &v_b, 0, 0, MirKernelResourceAccess::Read),
            resource(&out, &v_o, 0, 1, MirKernelResourceAccess::Write),
        ],
    );
    let k3 = kernel(
        "aux",
        vec![
            resource(&shared, &v_r, 0, 0, MirKernelResourceAccess::Read),
            resource(&mid_b, &v_a, 0, 1, MirKernelResourceAccess::Write),
        ],
    );
    let k4 = kernel(
        "consume",
        vec![
            resource(&out, &v_o, 0, 0, MirKernelResourceAccess::Read),
            resource(&mid_d, &v_c, 0, 1, MirKernelResourceAccess::Write),
        ],
    );

    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels = vec![k0, k1, k2, k3, k4];
    program.launches = vec![
        launch(1, 0),
        launch(2, 1),
        launch(3, 2),
        launch(4, 3),
        launch(5, 4),
    ];
    program.results = vec![ResultBuffer {
        buffer: out.clone(),
        version: v_o.clone(),
        role: BufferRole::InOut,
        produced_by: LaunchId(3),
        cadence: ObservationCadence::EndOfRun,
    }];

    program.validate().expect("fixture program must validate");
    (types, program)
}

/// The CS-1 declared placement facts (one fact per program buffer).
fn cs1_facts() -> Vec<PlacementFact> {
    let p0 = PartitionId(0);
    let p1 = PartitionId(1);
    vec![
        fact(1, 1, PlacementContract::Owned { owner: p0 }, 4_000),
        fact(2, 2, PlacementContract::Owned { owner: p0 }, 2_048),
        fact(3, 3, PlacementContract::Owned { owner: p0 }, 8_192),
        fact(4, 4, PlacementContract::Owned { owner: p0 }, 4_096),
        fact(5, 5, PlacementContract::Owned { owner: p1 }, 8_192),
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

/// Assemble the fixture plan: declared placement + derivation.
fn fixture_plan(
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

/// The derived CS-1 plan (positive-control substrate).
fn cs1() -> DistributedExecutionPlan {
    let (_types, program) = cs1_program();
    let facts = cs1_facts();
    let (partitions, graph) = derive(&program, &facts);
    fixture_plan(&facts, partitions, graph)
}

/// A minimal 2-partition program whose second partition runs no launch
/// (P1 owns B; the single launch writes A and B on P0 — the write-back
/// transfer of B is the derived move).
fn minimal_program() -> (TypeTable, DeviceProgram) {
    let mut types = TypeTable::new();
    let f32 = f32_ty(&mut types);

    let a = buffer_identity(1, "a", BufferRole::InOut, BufferLifetime::PerStep);
    let b = buffer_identity(2, "b", BufferRole::InOut, BufferLifetime::PerStep);
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
    (types, program)
}

fn minimal_facts() -> Vec<PlacementFact> {
    vec![
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
    ]
}

/// A hand-built transfer of the CS-1 result value O (value 7).
fn o_transfer(
    id: u32,
    source: PartitionId,
    destination: PartitionId,
    byte_count: u64,
    element_ty: MirType,
) -> TransferOperation {
    TransferOperation {
        id: TransferId(id),
        value: SemanticValueId(7),
        source,
        destination,
        byte_count,
        direction: TransferDirection::BIDI,
        element_ty,
        layout: MirTensorStorageLayout::DeviceHandle,
        path_label: TransportPath::HostStaged,
        producer_generation: 1,
        consumer_generation: 1,
        completion_boundary: ExecutionCommitBoundary {
            barriers: Vec::new(),
            launches: vec![LaunchId(3)],
        },
    }
}

fn transfer_mut(plan: &mut DistributedExecutionPlan, id: u32) -> &mut TransferOperation {
    plan.graph
        .operations
        .iter_mut()
        .find_map(|operation| match operation {
            ExecutionOperation::Transfer(transfer) if transfer.id == TransferId(id) => {
                Some(transfer)
            }
            _ => None,
        })
        .expect("transfer exists in the fixture plan")
}

fn collective_mut(plan: &mut DistributedExecutionPlan) -> &mut CollectiveOperation {
    plan.graph
        .operations
        .iter_mut()
        .find_map(|operation| match operation {
            ExecutionOperation::Collective(collective) => Some(collective),
            _ => None,
        })
        .expect("collective exists in the fixture plan")
}

// -----------------------------------------------------------------------
// Positive control
// -----------------------------------------------------------------------

#[test]
fn complete_cs1_plan_validates() {
    let plan = cs1();
    plan.validate()
        .expect("the complete CS-1 plan must pass every gate");
}

// -----------------------------------------------------------------------
// (g) stale versions
// -----------------------------------------------------------------------

#[test]
fn stale_plan_version_fails() {
    let mut plan = cs1();
    plan.version = PlanVersion(0);
    let error = plan.validate().expect_err("a stale plan version must fail");
    assert_eq!(error.class(), ValidateKind::StaleVersion);
    assert_eq!(
        error,
        ValidateError::StaleVersion {
            plan_version: PlanVersion(0),
            supported_version: LOGICAL_PLAN_VERSION,
        }
    );
}

#[test]
fn unknown_future_plan_version_fails() {
    let mut plan = cs1();
    plan.version = PlanVersion(2);
    let error = plan
        .validate()
        .expect_err("an unknown plan version must fail");
    assert_eq!(error.class(), ValidateKind::StaleVersion);
    assert!(matches!(
        error,
        ValidateError::StaleVersion { plan_version, .. } if plan_version == PlanVersion(2)
    ));
}

// -----------------------------------------------------------------------
// (f) topology mismatch
// -----------------------------------------------------------------------

#[test]
fn constraint_on_undeclared_partition_fails_topology() {
    let mut plan = cs1();
    plan.declared_placement
        .constraints
        .push(PlacementConstraint::PartitionBudgetBytes {
            partition: PartitionId(9),
            budget_bytes: 1_024,
        });
    let error = plan
        .validate()
        .expect_err("a constraint on an undeclared partition must fail");
    assert_eq!(error.class(), ValidateKind::TopologyMismatch);
    assert!(matches!(
        error,
        ValidateError::TopologyMismatch {
            partition: PartitionId(9),
            referenced_by: TopologyRef::Constraint,
        }
    ));
}

#[test]
fn duplicate_partition_id_fails_topology() {
    let mut plan = cs1();
    plan.partitions.push(plan.partitions[0].clone());
    let error = plan
        .validate()
        .expect_err("a duplicate partition id must fail");
    assert_eq!(error.class(), ValidateKind::TopologyMismatch);
    assert_eq!(
        error,
        ValidateError::RepeatedPartition {
            partition: PartitionId(0)
        }
    );
}

#[test]
fn zero_partition_budget_fails_topology() {
    let mut plan = cs1();
    plan.declared_placement
        .constraints
        .push(PlacementConstraint::PartitionBudgetBytes {
            partition: PartitionId(0),
            budget_bytes: 0,
        });
    let error = plan
        .validate()
        .expect_err("a zero partition budget must fail");
    assert_eq!(error.class(), ValidateKind::TopologyMismatch);
    assert_eq!(
        error,
        ValidateError::ZeroPartitionBudget {
            partition: PartitionId(0)
        }
    );
}

#[test]
fn require_owner_disagreeing_with_declared_owner_fails_topology() {
    let mut plan = cs1();
    plan.declared_placement
        .constraints
        .push(PlacementConstraint::RequireOwner {
            value: SemanticValueId(3),
            partition: PartitionId(1),
        });
    let error = plan
        .validate()
        .expect_err("a disagreeing RequireOwner constraint must fail");
    assert_eq!(error.class(), ValidateKind::TopologyMismatch);
    assert_eq!(
        error,
        ValidateError::RequireOwnerMismatch {
            value: SemanticValueId(3),
            required_partition: PartitionId(1),
            actual_owner: Some(PartitionId(0)),
        }
    );
}

// -----------------------------------------------------------------------
// (c) missing owners
// -----------------------------------------------------------------------

#[test]
fn transfer_of_undeclared_value_fails_missing_owner() {
    let mut plan = cs1();
    transfer_mut(&mut plan, 0).value = SemanticValueId(99);
    let error = plan
        .validate()
        .expect_err("a transfer of an undeclared value must fail");
    assert_eq!(error.class(), ValidateKind::MissingOwner);
    assert!(matches!(
        error,
        ValidateError::AbsentOwner {
            value: SemanticValueId(99),
            reference: OpRef::Transfer(TransferId(0)),
        }
    ));
}

#[test]
fn replicated_primary_outside_replica_set_fails_missing_owner() {
    let mut plan = cs1();
    let r_placement = plan
        .declared_placement
        .placements
        .iter_mut()
        .find(|placement| placement.value == SemanticValueId(6))
        .expect("R placement");
    r_placement.contract = PlacementContract::Replicated {
        primary: PartitionId(0),
        replicas: vec![PartitionId(1)],
    };
    let error = plan
        .validate()
        .expect_err("a primary outside its replica set must fail");
    assert_eq!(error.class(), ValidateKind::MissingOwner);
    assert_eq!(
        error,
        ValidateError::PrimaryOutsideReplicaSet {
            value: SemanticValueId(6),
            primary: PartitionId(0),
        }
    );
}

// -----------------------------------------------------------------------
// (d) conflicting replicas
// -----------------------------------------------------------------------

#[test]
fn duplicate_value_contracts_fail_conflicting() {
    let mut plan = cs1();
    plan.declared_placement.placements.push(LogicalPlacement {
        value: SemanticValueId(3),
        contract: PlacementContract::Owned {
            owner: PartitionId(1),
        },
    });
    let error = plan
        .validate()
        .expect_err("a doubly-declared value must fail");
    assert_eq!(error.class(), ValidateKind::ConflictingReplicas);
    assert_eq!(
        error,
        ValidateError::ConflictingContracts {
            value: SemanticValueId(3),
            first: ContractFingerprint::Owned {
                owner: PartitionId(0)
            },
            second: ContractFingerprint::Owned {
                owner: PartitionId(1)
            },
        }
    );
}

#[test]
fn duplicate_replica_in_set_fails_conflicting() {
    let mut plan = cs1();
    let r_placement = plan
        .declared_placement
        .placements
        .iter_mut()
        .find(|placement| placement.value == SemanticValueId(6))
        .expect("R placement");
    r_placement.contract = PlacementContract::Replicated {
        primary: PartitionId(0),
        replicas: vec![PartitionId(0), PartitionId(0), PartitionId(1)],
    };
    let error = plan.validate().expect_err("a repeated replica must fail");
    assert_eq!(error.class(), ValidateKind::ConflictingReplicas);
    assert_eq!(
        error,
        ValidateError::RepeatedReplica {
            value: SemanticValueId(6),
            partition: PartitionId(0),
        }
    );
}

#[test]
fn partition_placement_disagreeing_with_plan_fails_conflicting() {
    let mut plan = cs1();
    plan.partitions[0].placements.push(LogicalPlacement {
        value: SemanticValueId(5),
        contract: PlacementContract::Owned {
            owner: PartitionId(1),
        },
    });
    let error = plan
        .validate()
        .expect_err("a disagreeing partition placement must fail");
    assert_eq!(error.class(), ValidateKind::ConflictingReplicas);
    assert_eq!(
        error,
        ValidateError::PartitionContractMismatch {
            value: SemanticValueId(5),
            partition: PartitionId(0),
        }
    );
}

#[test]
fn shared_launch_fails_conflicting() {
    let mut plan = cs1();
    plan.partitions[0].launches.push(LaunchId(3));
    let error = plan
        .validate()
        .expect_err("a launch on two partitions must fail");
    assert_eq!(error.class(), ValidateKind::ConflictingReplicas);
    assert_eq!(
        error,
        ValidateError::SharedLaunch {
            launch: LaunchId(3),
            first: PartitionId(0),
            second: PartitionId(1),
        }
    );
}

#[test]
fn conflicting_byte_contract_fails_conflicting() {
    let (mut types, program) = cs1_program();
    let f32 = f32_ty(&mut types);
    let facts = cs1_facts();
    let (partitions, graph) = derive(&program, &facts);
    let mut plan = fixture_plan(&facts, partitions, graph);
    plan.graph
        .operations
        .push(ExecutionOperation::Transfer(o_transfer(
            2,
            PartitionId(1),
            PartitionId(0),
            4_096,
            f32,
        )));
    let error = plan
        .validate()
        .expect_err("disagreeing byte contracts must fail");
    assert_eq!(error.class(), ValidateKind::ConflictingReplicas);
    assert_eq!(
        error,
        ValidateError::ConflictingByteContract {
            value: SemanticValueId(7),
            first_byte_count: 2_048,
            second_byte_count: 4_096,
            second: OpRef::Transfer(TransferId(2)),
        }
    );
}

#[test]
fn duplicate_move_fails_conflicting() {
    let (mut types, program) = cs1_program();
    let f32 = f32_ty(&mut types);
    let facts = cs1_facts();
    let (partitions, graph) = derive(&program, &facts);
    let mut plan = fixture_plan(&facts, partitions, graph);
    plan.graph
        .operations
        .push(ExecutionOperation::Transfer(o_transfer(
            2,
            PartitionId(1),
            PartitionId(0),
            2_048,
            f32,
        )));
    let error = plan.validate().expect_err("a duplicate move must fail");
    assert_eq!(error.class(), ValidateKind::ConflictingReplicas);
    assert_eq!(
        error,
        ValidateError::RepeatedMove {
            value: SemanticValueId(7),
            original: TransferId(1),
            duplicate: TransferId(2),
        }
    );
}

// -----------------------------------------------------------------------
// (a) cross-device completeness
// -----------------------------------------------------------------------

#[test]
fn missing_generation_fails() {
    let mut plan = cs1();
    transfer_mut(&mut plan, 0).producer_generation = 0;
    let error = plan
        .validate()
        .expect_err("a transfer without a generation must fail");
    assert_eq!(error.class(), ValidateKind::Completeness);
    assert!(matches!(
        error,
        ValidateError::AbsentGeneration {
            value: SemanticValueId(3),
            reference: OpRef::Transfer(TransferId(0)),
            producer_generation: 0,
            consumer_generation: 1,
        }
    ));
}

#[test]
fn consumer_generation_before_producer_fails() {
    let mut plan = cs1();
    let m = transfer_mut(&mut plan, 0);
    m.producer_generation = 2;
    m.consumer_generation = 1;
    let error = plan
        .validate()
        .expect_err("a consumer before its producer must fail");
    assert_eq!(error.class(), ValidateKind::Completeness);
    assert!(matches!(
        error,
        ValidateError::GenerationOrder {
            value: SemanticValueId(3),
            reference: OpRef::Transfer(TransferId(0)),
            producer_generation: 2,
            consumer_generation: 1,
        }
    ));
}

#[test]
fn missing_byte_contract_fails() {
    let mut plan = cs1();
    transfer_mut(&mut plan, 0).byte_count = 0;
    let error = plan.validate().expect_err("a zero byte contract must fail");
    assert_eq!(error.class(), ValidateKind::Completeness);
    assert!(matches!(
        error,
        ValidateError::AbsentByteContract {
            value: SemanticValueId(3),
            reference: OpRef::Transfer(TransferId(0)),
            byte_count: 0,
        }
    ));
}

#[test]
fn missing_producer_fails() {
    let mut plan = cs1();
    transfer_mut(&mut plan, 1)
        .completion_boundary
        .launches
        .clear();
    let error = plan
        .validate()
        .expect_err("a transfer without a producing launch must fail");
    assert_eq!(error.class(), ValidateKind::Completeness);
    assert!(matches!(
        error,
        ValidateError::AbsentProducer {
            value: SemanticValueId(7),
            reference: OpRef::Transfer(TransferId(1)),
        }
    ));
}

#[test]
fn missing_completion_boundary_fails() {
    let mut plan = cs1();
    let m = transfer_mut(&mut plan, 0);
    m.completion_boundary.barriers.clear();
    m.completion_boundary.launches.clear();
    let error = plan
        .validate()
        .expect_err("a transfer without a completion boundary must fail");
    assert_eq!(error.class(), ValidateKind::Completeness);
    assert!(matches!(
        error,
        ValidateError::AbsentCompletionBoundary {
            value: SemanticValueId(3),
            reference: OpRef::Transfer(TransferId(0)),
        }
    ));
}

#[test]
fn missing_consumer_fails() {
    let (mut types, program) = minimal_program();
    let f32 = f32_ty(&mut types);
    let facts = minimal_facts();
    let (partitions, graph) = derive(&program, &facts);
    let mut plan = fixture_plan(&facts, partitions, graph);
    // Deliver A (owned by P0) to P1 — P1 holds no copy of A and runs no
    // launch, so the move has no consumer.
    plan.graph
        .operations
        .push(ExecutionOperation::Transfer(TransferOperation {
            id: TransferId(1),
            value: SemanticValueId(1),
            source: PartitionId(0),
            destination: PartitionId(1),
            byte_count: 4_096,
            direction: TransferDirection::BIDI,
            element_ty: f32,
            layout: MirTensorStorageLayout::DeviceHandle,
            path_label: TransportPath::HostStaged,
            producer_generation: 1,
            consumer_generation: 1,
            completion_boundary: ExecutionCommitBoundary {
                barriers: Vec::new(),
                launches: vec![LaunchId(1)],
            },
        }));
    let error = plan
        .validate()
        .expect_err("a move with no consumer must fail");
    assert_eq!(error.class(), ValidateKind::Completeness);
    assert!(matches!(
        error,
        ValidateError::AbsentConsumer {
            value: SemanticValueId(1),
            reference: OpRef::Transfer(TransferId(1)),
            source: PartitionId(0),
            destination: PartitionId(1),
        }
    ));
}

#[test]
fn dangling_boundary_barrier_fails() {
    let mut plan = cs1();
    transfer_mut(&mut plan, 0)
        .completion_boundary
        .barriers
        .push(BarrierId(99));
    let error = plan
        .validate()
        .expect_err("a dangling boundary barrier must fail");
    assert_eq!(error.class(), ValidateKind::Completeness);
    assert_eq!(
        error,
        ValidateError::DanglingBoundaryReference {
            reference: OpRef::Transfer(TransferId(0)),
            boundary_ref: BoundaryRef::Barrier(BarrierId(99)),
        }
    );
}

#[test]
fn dangling_boundary_launch_fails() {
    let mut plan = cs1();
    transfer_mut(&mut plan, 0)
        .completion_boundary
        .launches
        .push(LaunchId(99));
    let error = plan
        .validate()
        .expect_err("a dangling boundary launch must fail");
    assert_eq!(error.class(), ValidateKind::Completeness);
    assert_eq!(
        error,
        ValidateError::DanglingBoundaryReference {
            reference: OpRef::Transfer(TransferId(0)),
            boundary_ref: BoundaryRef::Launch(LaunchId(99)),
        }
    );
}

// -----------------------------------------------------------------------
// (b) cycles
// -----------------------------------------------------------------------

#[test]
fn bidirectional_value_move_cycle_fails() {
    let (mut types, program) = cs1_program();
    let f32 = f32_ty(&mut types);
    let facts = cs1_facts();
    let (partitions, graph) = derive(&program, &facts);
    let mut plan = fixture_plan(&facts, partitions, graph);
    // Add the reverse move of O (P1 → P0 exists; add P0 → P1 at the same
    // generation): the value flows in a directed cycle.
    plan.graph
        .operations
        .push(ExecutionOperation::Transfer(o_transfer(
            2,
            PartitionId(0),
            PartitionId(1),
            2_048,
            f32,
        )));
    let error = plan.validate().expect_err("a value-move cycle must fail");
    assert_eq!(error.class(), ValidateKind::Cycle);
    assert!(matches!(
        error,
        ValidateError::ValueMoveCycle {
            value: SemanticValueId(7),
            ..
        }
    ));
}

#[test]
fn self_move_cycle_fails() {
    let mut plan = cs1();
    transfer_mut(&mut plan, 0).destination = PartitionId(0);
    let error = plan.validate().expect_err("a self-move must fail");
    assert_eq!(error.class(), ValidateKind::Cycle);
    assert!(matches!(
        error,
        ValidateError::SelfMove {
            value: SemanticValueId(3),
            transfer: TransferId(0),
        }
    ));
}

// -----------------------------------------------------------------------
// (e) unsupported collectives
// -----------------------------------------------------------------------

#[test]
fn collective_source_not_participant_fails_unsupported() {
    let mut plan = cs1();
    collective_mut(&mut plan).participants = vec![PartitionId(1)];
    let error = plan
        .validate()
        .expect_err("a broadcast from a non-participant must fail");
    assert_eq!(error.class(), ValidateKind::UnsupportedCollective);
    assert_eq!(
        error,
        ValidateError::UnadmittedCollective {
            value: SemanticValueId(6),
            collective: CollectiveId(0),
            violation: CollectiveViolation::SourceNotParticipant {
                source: PartitionId(0),
            },
        }
    );
}

#[test]
fn collective_not_owner_originated_fails_unsupported() {
    let mut plan = cs1();
    collective_mut(&mut plan).source = PartitionId(1);
    let error = plan
        .validate()
        .expect_err("a broadcast not from the owner must fail");
    assert_eq!(error.class(), ValidateKind::UnsupportedCollective);
    assert_eq!(
        error,
        ValidateError::UnadmittedCollective {
            value: SemanticValueId(6),
            collective: CollectiveId(0),
            violation: CollectiveViolation::NotOwnerOriginated {
                owner: PartitionId(0),
                source: PartitionId(1),
            },
        }
    );
}

// -----------------------------------------------------------------------
// Diagnostic contract: each rejection names the class + the failing fact
// -----------------------------------------------------------------------

#[test]
fn diagnostic_display_names_class_and_failing_fact() {
    let mut plan = cs1();
    plan.version = PlanVersion(0);
    let error = plan.validate().expect_err("stale version must fail");
    let message = error.to_string();
    assert!(message.contains("stale-version"));
    assert!(message.contains("PlanVersion(0)"));
    assert!(message.contains("supported logical plan version"));

    // The failed-class mapping covers every class (each exit-gate bullet).
    assert_eq!(
        ValidateError::AbsentOwner {
            value: SemanticValueId(1),
            reference: OpRef::Transfer(TransferId(0)),
        }
        .class(),
        ValidateKind::MissingOwner
    );
    assert_eq!(
        ValidateError::RepeatedMove {
            value: SemanticValueId(1),
            original: TransferId(0),
            duplicate: TransferId(1),
        }
        .class(),
        ValidateKind::ConflictingReplicas
    );
    assert_eq!(
        ValidateError::ValueMoveCycle {
            value: SemanticValueId(1),
            first: TransferId(0),
            second: TransferId(1),
        }
        .class(),
        ValidateKind::Cycle
    );
    assert_eq!(
        ValidateError::UnadmittedCollective {
            value: SemanticValueId(1),
            collective: CollectiveId(0),
            violation: CollectiveViolation::TooFewParticipants { count: 1 },
        }
        .class(),
        ValidateKind::UnsupportedCollective
    );
    assert_eq!(
        ValidateError::TopologyMismatch {
            partition: PartitionId(9),
            referenced_by: TopologyRef::Constraint,
        }
        .class(),
        ValidateKind::TopologyMismatch
    );
    assert_eq!(
        ValidateError::StaleVersion {
            plan_version: PlanVersion(0),
            supported_version: LOGICAL_PLAN_VERSION,
        }
        .class(),
        ValidateKind::StaleVersion
    );
    assert_eq!(
        ValidateError::AbsentByteContract {
            value: SemanticValueId(1),
            reference: OpRef::Transfer(TransferId(0)),
            byte_count: 0,
        }
        .class(),
        ValidateKind::Completeness
    );
}

// -----------------------------------------------------------------------
// A14 regression: the kernel-plan enums carry no communication variant
// -----------------------------------------------------------------------

#[test]
fn collection_kernel_plan_carries_no_communication_variant() {
    // A14: `CollectionKernelPlan` describes work inside ONE kernel.
    // Transfers/collectives/barriers are operations of the upper execution
    // graph (`ExecutionOperation`), never collection-kernel variants. The
    // exhaustive match is the regression: adding a Transfer/Collective/
    // Barrier variant to the enum makes this match non-exhaustive and the
    // crate stops compiling — fail-closed at compile time. (The wire mirror
    // `WireCollectionKernelPlan` is asserted in radix-mir-fmir, which owns
    // that test surface — MD2-W1; radix-mir cannot reference radix-mir-fmir
    // types, FC18.)
    fn is_recipe_plan(plan: &CollectionKernelPlan) -> bool {
        match plan {
            CollectionKernelPlan::Elementwise => true,
            CollectionKernelPlan::TiledMatMul(_) => true,
            CollectionKernelPlan::TreeReduction(_) => true,
            CollectionKernelPlan::Transpose(_) => true,
            CollectionKernelPlan::AxisReduction(_) => true,
            CollectionKernelPlan::RowSoftmax(_) => true,
            CollectionKernelPlan::LayerNormalization(_) => true,
            // GI3-1: the pinned-row inference recipes are recipe plans too
            // (the frozen seven-variant set extended by the GI3-1 op
            // surface) — still no communication variant.
            CollectionKernelPlan::Gather(_) => true,
            CollectionKernelPlan::RmsNormalization(_) => true,
            CollectionKernelPlan::Rope(_) => true,
            CollectionKernelPlan::CausalMaskedSoftmax(_) => true,
            CollectionKernelPlan::QuantizedMatMul(_) => true,
            CollectionKernelPlan::GroupedMatMul(_) => true,
            CollectionKernelPlan::BatchedAttention(_) => true,
            CollectionKernelPlan::SsmConv1d(_) => true,
            CollectionKernelPlan::SsmScan(_) => true,
            // SFR-5: the declared distributed sum is an in-kernel recipe —
            // no communication variant.
            CollectionKernelPlan::DistributedSum(_) => true,
            // gpu-reset C4-1: a lane grid is an in-kernel recipe plan.
            CollectionKernelPlan::LaneGrid { .. } | CollectionKernelPlan::SingleLane => true,
            // C4-8: a grid reduction is an in-kernel recipe plan.
            CollectionKernelPlan::GridReduce { .. } => true,
            // A staged scan is three in-kernel stages, no communication variant.
            CollectionKernelPlan::Scan { .. } => true,
        }
    }

    // Every kernel plan a program can carry is a recipe plan — the frozen
    // seven-variant set (D1 FC6) plus the GI3-1 inference recipes, no
    // communication variant.
    let (_types, program) = cs1_program();
    assert!(
        program
            .kernels
            .iter()
            .all(|kernel| is_recipe_plan(&kernel.plan))
    );

    // The communication seam is the upper execution graph, not the kernel
    // plan: the derived plan carries transfers/collectives/barriers as
    // `ExecutionOperation` variants.
    let plan = cs1();
    let graph = &plan.graph.operations;
    assert!(
        graph
            .iter()
            .any(|operation| matches!(operation, ExecutionOperation::Transfer(_)))
    );
    assert!(
        graph
            .iter()
            .any(|operation| matches!(operation, ExecutionOperation::Collective(_)))
    );
    assert!(
        graph
            .iter()
            .any(|operation| matches!(operation, ExecutionOperation::Barrier(_)))
    );
}

#[test]
fn batched_attention_accepts_both_m3_gqa_shapes() {
    const SEQ: u64 = 32;
    const HEAD_DIM: u64 = 64;

    // M3-U1 admits both live GQA shape classes. Context is written directly
    // in sequence-major `[seq, hidden]` storage, so the carried hidden width
    // and leading row stride must preserve the full grouped-head structure.
    for (q_per_kv, kv_heads) in [(3, 5), (7, 2)] {
        let hidden = kv_heads * q_per_kv * HEAD_DIM;
        let plan = CollectionKernelPlan::BatchedAttention(BatchedAttentionPlan {
            kind: BatchedAttentionKind::ContextSequenceMajor,
            kv_heads,
            q_per_kv,
            seq: SEQ,
            head_dim: HEAD_DIM,
            hidden,
            layout: None,
            rotate_half: false,
            output_strides: [hidden, 1, 1, 1],
            scale_bits: f32::to_bits(0.125),
            decode_outputs_per_workgroup: None,
        });

        plan.validate().expect("both M3 GQA shapes must validate");
        assert!(matches!(&plan, CollectionKernelPlan::BatchedAttention(_)));
        if let CollectionKernelPlan::BatchedAttention(attention) = &plan {
            assert_eq!(
                (attention.q_per_kv, attention.kv_heads),
                (q_per_kv, kv_heads)
            );
            assert_eq!(
                attention.hidden,
                attention.kv_heads * attention.q_per_kv * attention.head_dim
            );
            assert_eq!(attention.output_strides, [attention.hidden, 1, 1, 1]);
        }
    }
}
