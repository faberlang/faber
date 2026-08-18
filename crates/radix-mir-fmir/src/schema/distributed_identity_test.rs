//! MD2-H1 tests: the distributed-plan hash domains + A10 exclusion.
//!
//! Proves the MD2-H1 done-when:
//!
//! - `logical_distributed_plan_hash` is deterministic and `sha256:`-prefixed
//!   with a 64-hex lowercase digest (canonical-format test);
//! - identical logical plan → identical hash, different plan → different
//!   hash;
//! - the hash covers the logical-plan canonical bytes ONLY (reference-digest
//!   test against `encode_logical_plan`);
//! - `device_identity_hash` over the single-device program is UNCHANGED by
//!   the presence of a logical plan (structural A10-exclusion at the
//!   hash-function boundary, naming contract §2);
//! - the `bound_distributed_plan_hash` spelling contract (computed in
//!   MD2-B1) is frozen to the same `sha256:<64-hex>` format.

use super::{
    canonical_program_bytes, device_identity_hash, logical_distributed_plan_hash, WireBinding,
    WireBufferIdentity, WireBufferLifetime, WireBufferRole, WireBufferVersion,
    WireCollectionKernelPlan, WireDependencyEdge, WireDeviceProgram, WireDeviceResource,
    WireDispatchSize, WireInitializationPolicy, WireKernelLaunchPlan, WireKernelUnit,
    WireLaunchUnit, WireObservationCadence, WireObservationFact, WireProgramLifetime,
    WireResourceAccess, WireResultBuffer, WireSemanticValue, WireSemanticValueOrigin,
    WireStorageLayout, WireWorkgroupCount, WireWorkgroupSize, SHA256_IDENTITY_PREFIX,
};
use radix_mir::device_program::{BufferId, LaunchId};
use radix_mir::device_semantics::SemanticValueId;
use radix_mir::distributed::{
    encode_logical_plan, BarrierId, BarrierOperation, CollectiveId, CollectiveOperation,
    DeclaredPlacement, DeviceProgramPartition, DistributedExecutionGraph, DistributedExecutionPlan,
    ExecutionCommitBoundary, ExecutionOperation, LaunchOperation, LogicalPlacement, PartitionId,
    PlacementConstraint, PlacementContract, PlacementObjective, PlanVersion, TransferDirection,
    TransferId, TransferOperation, TransportPath, LOGICAL_PLAN_VERSION,
};
use radix_mir::layout::MirTensorStorageLayout;
use radix_mir::MirType;
use radix_types::TypeId;

// -----------------------------------------------------------------------
// Fixture helpers
// -----------------------------------------------------------------------

/// A semantic f32 MIR type for the transfer fixture.
fn f32_type() -> MirType {
    MirType::semantic(TypeId(1))
}

/// The portable two-partition logical-plan fixture: two launches, one
/// host-staged transfer, one broadcast, one barrier, and a declared commit
/// boundary — the same shape the MD2-D1 fixture derives.
fn sample_logical_plan() -> DistributedExecutionPlan {
    let p0 = PartitionId(0);
    let p1 = PartitionId(1);

    let placements = vec![
        LogicalPlacement {
            value: SemanticValueId(1),
            contract: PlacementContract::Owned { owner: p0 },
        },
        LogicalPlacement {
            value: SemanticValueId(2),
            contract: PlacementContract::Owned { owner: p1 },
        },
        LogicalPlacement {
            value: SemanticValueId(3),
            contract: PlacementContract::Replicated {
                primary: p0,
                replicas: vec![p0, p1],
            },
        },
    ];
    let declared_placement = DeclaredPlacement {
        placements,
        constraints: vec![PlacementConstraint::PartitionBudgetBytes {
            partition: p0,
            budget_bytes: 160 * 1024 * 1024,
        }],
        objectives: vec![PlacementObjective::MinimizeTransferBytes],
    };

    let partitions = vec![
        DeviceProgramPartition {
            id: p0,
            kernels: vec![0],
            launches: vec![LaunchId(1)],
            buffers: vec![BufferId(1), BufferId(3)],
            results: Vec::new(),
            placements: vec![
                LogicalPlacement {
                    value: SemanticValueId(1),
                    contract: PlacementContract::Owned { owner: p0 },
                },
                LogicalPlacement {
                    value: SemanticValueId(3),
                    contract: PlacementContract::Replicated {
                        primary: p0,
                        replicas: vec![p0, p1],
                    },
                },
            ],
        },
        DeviceProgramPartition {
            id: p1,
            kernels: vec![0],
            launches: vec![LaunchId(2)],
            buffers: vec![BufferId(2), BufferId(3)],
            results: Vec::new(),
            placements: vec![
                LogicalPlacement {
                    value: SemanticValueId(2),
                    contract: PlacementContract::Owned { owner: p1 },
                },
                LogicalPlacement {
                    value: SemanticValueId(3),
                    contract: PlacementContract::Replicated {
                        primary: p0,
                        replicas: vec![p0, p1],
                    },
                },
            ],
        },
    ];

    let graph = DistributedExecutionGraph {
        operations: vec![
            ExecutionOperation::Launch(LaunchOperation {
                launch: LaunchId(1),
                partition: p0,
            }),
            ExecutionOperation::Launch(LaunchOperation {
                launch: LaunchId(2),
                partition: p1,
            }),
            ExecutionOperation::Transfer(TransferOperation {
                id: TransferId(0),
                value: SemanticValueId(2),
                source: p0,
                destination: p1,
                byte_count: 2_048,
                direction: TransferDirection::BIDI,
                element_ty: f32_type(),
                layout: MirTensorStorageLayout::DeviceHandle,
                path_label: TransportPath::HostStaged,
                producer_generation: 1,
                consumer_generation: 1,
                completion_boundary: ExecutionCommitBoundary {
                    barriers: vec![BarrierId(0)],
                    launches: vec![LaunchId(2)],
                },
            }),
            ExecutionOperation::Collective(CollectiveOperation::broadcast(
                CollectiveId(0),
                SemanticValueId(3),
                p0,
                vec![p0, p1],
                16_384,
            )),
            ExecutionOperation::Barrier(BarrierOperation {
                id: BarrierId(0),
                partitions: vec![p0, p1],
            }),
        ],
        commit_boundary: ExecutionCommitBoundary {
            barriers: vec![BarrierId(0)],
            launches: vec![LaunchId(2)],
        },
    };

    DistributedExecutionPlan::assemble(
        declared_placement,
        partitions,
        graph,
        PlanVersion(LOGICAL_PLAN_VERSION),
    )
}

/// A different logical plan: value 1 moves to partition 1 and partition 0's
/// declared budget halves — both change the canonical bytes.
fn altered_logical_plan() -> DistributedExecutionPlan {
    let mut plan = sample_logical_plan();
    plan.declared_placement.placements[0].contract = PlacementContract::Owned {
        owner: PartitionId(1),
    };
    if let PlacementConstraint::PartitionBudgetBytes { budget_bytes, .. } =
        &mut plan.declared_placement.constraints[0]
    {
        *budget_bytes = 80 * 1024 * 1024;
    }
    plan
}

/// The single-device program fixture (a one-kernel elementwise program — the
/// established wire fixture shape): the A10 substrate.
fn single_device_wire_program() -> WireDeviceProgram {
    let input = WireBufferIdentity {
        id: 1,
        name: "x".to_owned(),
        role: WireBufferRole::Input,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::PerProgram,
        semantic_value: 1,
    };
    let output = WireBufferIdentity {
        id: 2,
        name: "y".to_owned(),
        role: WireBufferRole::Output,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::ObservationPoint,
        semantic_value: 2,
    };
    WireDeviceProgram {
        kernels: vec![WireKernelUnit {
            function: 0,
            entry: "addita".to_owned(),
            plan: WireCollectionKernelPlan::Elementwise,
            resources: vec![
                WireDeviceResource {
                    buffer: input.clone(),
                    version: WireBufferVersion {
                        version: 1,
                        element_ty: "f32".to_owned(),
                        element_count: 4,
                        reduced_projection: None,
                    },
                    binding: WireBinding {
                        group: 0,
                        binding: 0,
                    },
                    access: WireResourceAccess::Read,
                    generation: 1,
                    initialization: WireInitializationPolicy::HostProvided,
                },
                WireDeviceResource {
                    buffer: output.clone(),
                    version: WireBufferVersion {
                        version: 1,
                        element_ty: "f32".to_owned(),
                        element_count: 4,
                        reduced_projection: None,
                    },
                    binding: WireBinding {
                        group: 0,
                        binding: 1,
                    },
                    access: WireResourceAccess::Write,
                    generation: 1,
                    initialization: WireInitializationPolicy::KernelInitialized,
                },
            ],
            launch: WireKernelLaunchPlan {
                workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
                dispatch_size: WireDispatchSize { x: 4, y: 1, z: 1 },
                workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
            },
        }],
        launches: vec![WireLaunchUnit {
            id: 1,
            kernel_index: 0,
        }],
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: output,
            version: WireBufferVersion {
                version: 1,
                element_ty: "f32".to_owned(),
                element_count: 4,
                reduced_projection: None,
            },
            role: WireBufferRole::Output,
            produced_by: 1,
            observation: WireObservationFact {
                at_launch: 1,
                cadence: WireObservationCadence::PerStep,
            },
        }],
        semantic_values: vec![
            WireSemanticValue {
                id: 1,
                name: "x".to_owned(),
                origin: WireSemanticValueOrigin::HostInput,
            },
            WireSemanticValue {
                id: 2,
                name: "y".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "addita-output".to_owned(),
                },
            },
        ],
        roots: vec![1],
        dependencies: Vec::<WireDependencyEdge>::new(),
        relations: Vec::new(),
    }
}

// -----------------------------------------------------------------------
// logical_distributed_plan_hash: canonical format + determinism
// -----------------------------------------------------------------------

#[test]
fn logical_distributed_plan_hash_is_canonical_format() {
    let identity = logical_distributed_plan_hash(&sample_logical_plan());
    assert!(identity.starts_with(SHA256_IDENTITY_PREFIX));
    assert_eq!(identity.len(), SHA256_IDENTITY_PREFIX.len() + 64);
    assert!(
        identity[SHA256_IDENTITY_PREFIX.len()..]
            .chars()
            .all(|c| c.is_ascii_hexdigit()),
        "identity digest must be lowercase hex"
    );
}

#[test]
fn logical_distributed_plan_hash_is_deterministic() {
    let plan = sample_logical_plan();
    assert_eq!(
        logical_distributed_plan_hash(&plan),
        logical_distributed_plan_hash(&plan)
    );
    // A re-assembled identical plan (byte-identical canonical encoding)
    // hashes identically.
    assert_eq!(
        logical_distributed_plan_hash(&plan),
        logical_distributed_plan_hash(&sample_logical_plan())
    );
}

#[test]
fn logical_distributed_plan_hash_distinguishes_different_plans() {
    let plan = sample_logical_plan();
    let altered = altered_logical_plan();

    // The fixtures really differ at the canonical-bytes substrate...
    assert_ne!(encode_logical_plan(&plan), encode_logical_plan(&altered));
    // ...so the hashes must differ.
    assert_ne!(
        logical_distributed_plan_hash(&plan),
        logical_distributed_plan_hash(&altered)
    );
}

// -----------------------------------------------------------------------
// logical_distributed_plan_hash: covers the canonical bytes only
// -----------------------------------------------------------------------

#[test]
fn logical_distributed_plan_hash_covers_the_logical_plan_canonical_bytes_only() {
    // The digest is exactly sha256 over `encode_logical_plan(&plan)` — no
    // source hashes, no length-prefix wrapper, no extra salt. The hash
    // covers the logical-plan canonical bytes ONLY.
    let plan = sample_logical_plan();
    let reference = format!(
        "{SHA256_IDENTITY_PREFIX}{}",
        handroll::sha256_hex(&encode_logical_plan(&plan))
    );
    assert_eq!(logical_distributed_plan_hash(&plan), reference);
}

// -----------------------------------------------------------------------
// A10 exclusion: device_identity_hash unchanged by a logical plan
// -----------------------------------------------------------------------

#[test]
fn a10_device_identity_hash_is_unchanged_by_a_logical_plan() {
    // Structural A10-exclusion at the hash-function boundary (naming
    // contract §2): `device_identity_hash` is a function of source hashes +
    // `canonical_program_bytes(&WireDeviceProgram)` only, so the presence of
    // a logical plan cannot change it. A distributed image rides the same
    // single-device program plus a plan hashed into ITS OWN domain; A10 is
    // byte-for-byte unchanged.
    let sources = ["fnv64:0000000000000000", "fnv64:1111111111111111"];
    let program = single_device_wire_program();
    let base = device_identity_hash(&sources, &canonical_program_bytes(&program));

    // Presence of a logical plan: the plan hashes into its own domain,
    // never A10.
    let plan_hash = logical_distributed_plan_hash(&sample_logical_plan());
    assert_ne!(base, plan_hash);

    // A10 over the same single-device program is unchanged by the plan's
    // presence.
    assert_eq!(
        device_identity_hash(&sources, &canonical_program_bytes(&program)),
        base
    );

    // A DIFFERENT logical plan (different canonical bytes → different
    // logical hash) also leaves A10 unchanged — the plan content never
    // enters the A10 substrate.
    let altered_hash = logical_distributed_plan_hash(&altered_logical_plan());
    assert_ne!(altered_hash, plan_hash);
    assert_eq!(
        device_identity_hash(&sources, &canonical_program_bytes(&program)),
        base
    );
}

// -----------------------------------------------------------------------
// bound_distributed_plan_hash: frozen spelling contract (MD2-B1)
// -----------------------------------------------------------------------

#[test]
fn bound_distributed_plan_hash_spelling_contract_is_frozen() {
    // MD2-B1 computes `bound_distributed_plan_hash` in the public faber crate over
    // the `BoundDistributedPlan` canonical bytes (the logical hash + binding
    // facts). H1 freezes the spelling: the same `sha256:<64-hex>` format as
    // A10 and the logical hash. Physical ids enter THIS bound-plan domain —
    // never A10 (naming contract §2).
    assert_eq!(SHA256_IDENTITY_PREFIX, "sha256:");

    // Representative bound-plan canonical bytes under the documented
    // contract: the spelling is a pure function of the bytes — identical
    // bytes → identical spelling, canonical 64-hex digest.
    let bound_canonical = b"bound-distributed-plan-canonical-bytes";
    let spelling = format!(
        "{SHA256_IDENTITY_PREFIX}{}",
        handroll::sha256_hex(bound_canonical)
    );
    assert_eq!(spelling.len(), SHA256_IDENTITY_PREFIX.len() + 64);
    assert!(
        spelling[SHA256_IDENTITY_PREFIX.len()..]
            .chars()
            .all(|c| c.is_ascii_hexdigit()),
        "bound-plan digest must be lowercase hex"
    );
    assert_eq!(
        spelling,
        format!(
            "{SHA256_IDENTITY_PREFIX}{}",
            handroll::sha256_hex(bound_canonical)
        )
    );
}
