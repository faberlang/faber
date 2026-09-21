//! MD3H-F1 hand-assembled distributed FMIR fixtures.
//!
//! Two execution images for later host/run units:
//!
//! - **8-rank**: partitions `0..=7`, declared placements, typed host-staged
//!   transfers, one Broadcast of the replicated weights, a result barrier,
//!   and a commit boundary.
//! - **1-partition**: a `Some` distributed section with a single partition
//!   and no transfers or collectives.
//!
//! Fixtures only — they exercise the existing schema; they do not change it.

use super::super::{
    FmirDeviceArtifactsSection, FmirDeviceProgramSection, FmirDeviceSection, FmirDeviceSelection,
    FmirDistributedExecutionSection, WIRE_DEVICE_PROGRAM_VERSION, WIRE_DISTRIBUTED_SECTION_VERSION,
    WireBinding, WireBufferIdentity, WireBufferLifetime, WireBufferRole, WireBufferVersion,
    WireCollectionKernelPlan, WireDependencyEdge, WireDeviceProgram, WireDeviceResource,
    WireDispatchSize, WireInitializationPolicy, WireKernelLaunchPlan, WireKernelUnit,
    WireLaunchUnit, WireObservationCadence, WireObservationFact, WirePlacementContract,
    WirePlacementFact, WireProgramLifetime, WireResourceAccess, WireResultBuffer,
    WireSemanticValue, WireSemanticValueOrigin, WireStorageLayout, WireWorkgroupCount,
    WireWorkgroupSize, logical_distributed_plan_hash, plan_to_wire_plan, wire_facts_to_facts,
    wire_plan_to_plan, wire_program_to_program,
};
use radix_mir::distributed::{
    DeclaredPlacement, DistributedExecutionPlan, LOGICAL_PLAN_VERSION, LogicalPlacement,
    PartitionId, PlacementConstraint, PlacementObjective, PlanVersion,
};

/// Logical rank count of the eight-rank fixture.
pub(super) const EIGHT_RANK_COUNT: u32 = 8;

/// CS-1 per-partition budget (160 MiB).
const PARTITION_BUDGET_BYTES: u64 = 160 * 1024 * 1024;

fn launch_plan() -> WireKernelLaunchPlan {
    WireKernelLaunchPlan {
        workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
        dispatch_size: WireDispatchSize { x: 8, y: 1, z: 1 },
        workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
    }
}

fn version(element_count: u64) -> WireBufferVersion {
    WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count,
        shape: None,
        reduced_projection: None,
    }
}

fn buffer(
    id: u32,
    name: &str,
    role: WireBufferRole,
    lifetime: WireBufferLifetime,
) -> WireBufferIdentity {
    WireBufferIdentity {
        id,
        name: name.to_owned(),
        role,
        storage: WireStorageLayout::DeviceHandle,
        lifetime,
        semantic_value: id,
    }
}

fn resource(
    identity: &WireBufferIdentity,
    version: &WireBufferVersion,
    binding: u32,
    access: WireResourceAccess,
    initialization: WireInitializationPolicy,
) -> WireDeviceResource {
    WireDeviceResource {
        buffer: identity.clone(),
        version: version.clone(),
        binding: WireBinding { group: 0, binding },
        access,
        generation: 1,
        initialization,
    }
}

fn kernel(function: u32, entry: &str, resources: Vec<WireDeviceResource>) -> WireKernelUnit {
    WireKernelUnit {
        function,
        entry: entry.to_owned(),
        plan: WireCollectionKernelPlan::Elementwise,
        resources,
        launch: launch_plan(),
    }
}

fn semantic(id: u32, name: &str, origin: WireSemanticValueOrigin) -> WireSemanticValue {
    WireSemanticValue {
        id,
        name: name.to_owned(),
        origin,
    }
}

fn owned_fact(id: u32, owner: u32, byte_count: u64) -> WirePlacementFact {
    WirePlacementFact {
        value: id,
        buffer: id,
        contract: WirePlacementContract::Owned { owner },
        byte_count,
    }
}

fn device_section(
    program: WireDeviceProgram,
    distributed: FmirDistributedExecutionSection,
) -> FmirDeviceSection {
    FmirDeviceSection {
        device_program: FmirDeviceProgramSection {
            v: WIRE_DEVICE_PROGRAM_VERSION,
            program,
        },
        executor_plan: None,
        selection: FmirDeviceSelection::Auto,
        artifacts: FmirDeviceArtifactsSection {
            artifact: Vec::new(),
        },
        declared_inputs: Vec::new(),
        distributed: Some(distributed),
        session: None,
    }
}

fn assemble_section(
    program: &WireDeviceProgram,
    facts: Vec<WirePlacementFact>,
    constraints: Vec<PlacementConstraint>,
) -> FmirDistributedExecutionSection {
    let mir_facts = wire_facts_to_facts(&facts);
    let reconstructed_program =
        wire_program_to_program(program).expect("MD3H fixture program reconstructs");
    let (partitions, graph) = radix_mir::distributed::derive(&reconstructed_program, &mir_facts);
    let mut placements: Vec<LogicalPlacement> =
        mir_facts.iter().map(LogicalPlacement::from_fact).collect();
    placements.sort_by_key(|placement| placement.value.0);
    let plan = DistributedExecutionPlan::assemble(
        DeclaredPlacement {
            placements,
            constraints,
            objectives: vec![PlacementObjective::MinimizeTransferBytes],
        },
        partitions,
        graph,
        PlanVersion(LOGICAL_PLAN_VERSION),
    );
    let wire_plan = plan_to_wire_plan(&plan);
    let reconstructed = wire_plan_to_plan(&wire_plan).expect("MD3H fixture plan round-trips");
    FmirDistributedExecutionSection {
        v: WIRE_DISTRIBUTED_SECTION_VERSION,
        logical_plan: wire_plan,
        declared_placement: facts,
        logical_distributed_plan_hash: logical_distributed_plan_hash(&reconstructed),
    }
}

fn partition_budgets(count: u32) -> Vec<PlacementConstraint> {
    (0..count)
        .map(|partition| PlacementConstraint::PartitionBudgetBytes {
            partition: PartitionId(partition),
            budget_bytes: PARTITION_BUDGET_BYTES,
        })
        .collect()
}

/// Eight-rank single-device program: replicated weights, a P0 input, seven
/// per-rank shards, and a P7 result. One launch per rank.
pub(super) fn eight_rank_program() -> WireDeviceProgram {
    let w = buffer(
        1,
        "w",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let x = buffer(
        2,
        "x",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let shards: Vec<WireBufferIdentity> = (0..7)
        .map(|rank| {
            buffer(
                3 + rank,
                &format!("h{rank}"),
                WireBufferRole::InOut,
                WireBufferLifetime::PerStep,
            )
        })
        .collect();
    let y = buffer(
        10,
        "y",
        WireBufferRole::Output,
        WireBufferLifetime::ObservationPoint,
    );
    let v_w = version(128);
    let v_x = version(64);
    let v_h = version(64);
    let v_y = version(32);

    let mut kernels = Vec::with_capacity(8);
    kernels.push(kernel(
        0,
        "rank0",
        vec![
            resource(
                &w,
                &v_w,
                0,
                WireResourceAccess::Read,
                WireInitializationPolicy::HostProvided,
            ),
            resource(
                &x,
                &v_x,
                1,
                WireResourceAccess::Read,
                WireInitializationPolicy::HostProvided,
            ),
            resource(
                &shards[0],
                &v_h,
                2,
                WireResourceAccess::Write,
                WireInitializationPolicy::KernelInitialized,
            ),
        ],
    ));
    for rank in 1..7 {
        let prev = &shards[(rank - 1) as usize];
        let cur = &shards[rank as usize];
        kernels.push(kernel(
            rank,
            &format!("rank{rank}"),
            vec![
                resource(
                    &w,
                    &v_w,
                    0,
                    WireResourceAccess::Read,
                    WireInitializationPolicy::HostProvided,
                ),
                resource(
                    prev,
                    &v_h,
                    1,
                    WireResourceAccess::Read,
                    WireInitializationPolicy::KernelInitialized,
                ),
                resource(
                    cur,
                    &v_h,
                    2,
                    WireResourceAccess::Write,
                    WireInitializationPolicy::KernelInitialized,
                ),
            ],
        ));
    }
    kernels.push(kernel(
        7,
        "rank7",
        vec![
            resource(
                &w,
                &v_w,
                0,
                WireResourceAccess::Read,
                WireInitializationPolicy::HostProvided,
            ),
            resource(
                &shards[6],
                &v_h,
                1,
                WireResourceAccess::Read,
                WireInitializationPolicy::KernelInitialized,
            ),
            resource(
                &y,
                &v_y,
                2,
                WireResourceAccess::Write,
                WireInitializationPolicy::KernelInitialized,
            ),
        ],
    ));

    let mut semantic_values = vec![
        semantic(1, "w", WireSemanticValueOrigin::HostInput),
        semantic(
            2,
            "x",
            WireSemanticValueOrigin::MirLocal {
                function: 0,
                local: 0,
            },
        ),
    ];
    for rank in 0..7 {
        semantic_values.push(semantic(
            3 + rank,
            &format!("h{rank}"),
            WireSemanticValueOrigin::Synthetic {
                label: format!("rank{rank}-shard"),
            },
        ));
    }
    semantic_values.push(semantic(
        10,
        "y",
        WireSemanticValueOrigin::Synthetic {
            label: "rank7-output".to_owned(),
        },
    ));

    let launches: Vec<WireLaunchUnit> = (0..8)
        .map(|rank| WireLaunchUnit {
            id: rank + 1,
            kernel_index: rank,
        })
        .collect();
    let dependencies: Vec<WireDependencyEdge> = (0..7)
        .map(|rank| WireDependencyEdge {
            producer: rank + 1,
            consumer: rank + 2,
            buffer: 3 + rank,
            version: 1,
        })
        .collect();

    WireDeviceProgram {
        kernels,
        launches,
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: y,
            version: v_y,
            role: WireBufferRole::Output,
            produced_by: 8,
            observation: WireObservationFact {
                at_launch: 8,
                cadence: WireObservationCadence::PerStep,
            },
        }],
        semantic_values,
        roots: vec![1],
        dependencies,
        relations: Vec::new(),
    }
}

/// Declared 8-rank placement: `w` replicated across all ranks (broadcast
/// source), `x` and `h0` on P0, shards `h1..h6` on P1..P6, result `y` on P7.
pub(super) fn eight_rank_facts() -> Vec<WirePlacementFact> {
    let replicas: Vec<u32> = (0..EIGHT_RANK_COUNT).collect();
    let mut facts = vec![
        WirePlacementFact {
            value: 1,
            buffer: 1,
            contract: WirePlacementContract::Replicated {
                primary: 0,
                replicas,
            },
            byte_count: 512,
        },
        owned_fact(2, 0, 256),
    ];
    for rank in 0..7 {
        facts.push(owned_fact(3 + rank, rank, 256));
    }
    facts.push(owned_fact(10, 7, 128));
    facts
}

/// Honest 8-rank distributed section (derived plan + H1 hash).
pub(super) fn eight_rank_section() -> FmirDistributedExecutionSection {
    assemble_section(
        &eight_rank_program(),
        eight_rank_facts(),
        partition_budgets(EIGHT_RANK_COUNT),
    )
}

/// Device section carrying the honest 8-rank distributed image.
pub(super) fn eight_rank_device_section() -> FmirDeviceSection {
    device_section(eight_rank_program(), eight_rank_section())
}

/// One-partition single-device program (two-kernel chain, one result).
pub(super) fn one_partition_program() -> WireDeviceProgram {
    let w = buffer(
        1,
        "w",
        WireBufferRole::Input,
        WireBufferLifetime::PerProgram,
    );
    let m = buffer(2, "m", WireBufferRole::InOut, WireBufferLifetime::PerStep);
    let o = buffer(
        3,
        "o",
        WireBufferRole::Output,
        WireBufferLifetime::ObservationPoint,
    );
    let v_w = version(64);
    let v_m = version(128);
    let v_o = version(32);
    WireDeviceProgram {
        kernels: vec![
            kernel(
                0,
                "proj",
                vec![
                    resource(
                        &w,
                        &v_w,
                        0,
                        WireResourceAccess::Read,
                        WireInitializationPolicy::HostProvided,
                    ),
                    resource(
                        &m,
                        &v_m,
                        1,
                        WireResourceAccess::Write,
                        WireInitializationPolicy::KernelInitialized,
                    ),
                ],
            ),
            kernel(
                1,
                "emit",
                vec![
                    resource(
                        &m,
                        &v_m,
                        0,
                        WireResourceAccess::Read,
                        WireInitializationPolicy::KernelInitialized,
                    ),
                    resource(
                        &o,
                        &v_o,
                        1,
                        WireResourceAccess::Write,
                        WireInitializationPolicy::KernelInitialized,
                    ),
                ],
            ),
        ],
        launches: vec![
            WireLaunchUnit {
                id: 1,
                kernel_index: 0,
            },
            WireLaunchUnit {
                id: 2,
                kernel_index: 1,
            },
        ],
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: o.clone(),
            version: v_o.clone(),
            role: WireBufferRole::Output,
            produced_by: 2,
            observation: WireObservationFact {
                at_launch: 2,
                cadence: WireObservationCadence::PerStep,
            },
        }],
        semantic_values: vec![
            semantic(1, "w", WireSemanticValueOrigin::HostInput),
            semantic(
                2,
                "m",
                WireSemanticValueOrigin::Synthetic {
                    label: "proj-output".to_owned(),
                },
            ),
            semantic(
                3,
                "o",
                WireSemanticValueOrigin::Synthetic {
                    label: "emit-output".to_owned(),
                },
            ),
        ],
        roots: vec![1],
        dependencies: vec![WireDependencyEdge {
            producer: 1,
            consumer: 2,
            buffer: 2,
            version: 1,
        }],
        relations: Vec::new(),
    }
}

/// All values owned by partition 0.
pub(super) fn one_partition_facts() -> Vec<WirePlacementFact> {
    vec![
        owned_fact(1, 0, 256),
        owned_fact(2, 0, 512),
        owned_fact(3, 0, 128),
    ]
}

/// Honest 1-partition distributed section.
pub(super) fn one_partition_section() -> FmirDistributedExecutionSection {
    assemble_section(
        &one_partition_program(),
        one_partition_facts(),
        partition_budgets(1),
    )
}

/// Device section carrying the honest 1-partition distributed image.
pub(super) fn one_partition_device_section() -> FmirDeviceSection {
    device_section(one_partition_program(), one_partition_section())
}
