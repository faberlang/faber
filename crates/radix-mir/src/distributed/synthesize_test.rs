//! MD3H-R1: N=1 synthesis is total, 1-partition, communication-empty, and
//! stable. A synthesized plan that carries any Transfer/Collective/Barrier
//! fails. Physical ids are unrepresentable on this plan.

use super::synthesize;
use crate::abi::{
    MirKernelDispatchSize, MirKernelResourceAccess, MirKernelWorkgroupCount, MirWorkgroupSize,
};
use crate::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit,
    ObservationCadence, ResultBuffer,
};
use crate::distributed::types::{
    ExecutionOperation, LOGICAL_PLAN_VERSION, PartitionId, PlanVersion, ResultRef,
};
use crate::distributed::{
    DistributedExecutionGraph, DistributedExecutionPlan, encode_logical_plan,
};
use crate::kernel_plan::CollectionKernelPlan;
use crate::layout::MirTensorStorageLayout;
use crate::{MirFunctionId, MirType};
use radix_types::{NumericWidth, Primitive, TypeTable};

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn launch_plan() -> KernelLaunchPlan {
    KernelLaunchPlan {
        workgroup: MirWorkgroupSize { x: 64, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
        workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
    }
}

fn identity(id: u32, name: &str, role: BufferRole, lifetime: BufferLifetime) -> BufferIdentity {
    BufferIdentity {
        id: BufferId(id),
        name: name.to_owned(),
        role,
        storage: MirTensorStorageLayout::DeviceHandle,
        lifetime,
    }
}

/// Fail if the graph carries any communication operation.
///
/// The match is exhaustive on [`ExecutionOperation`]: a new communication
/// variant cannot slip through as a silent Launch-shaped no-op.
fn communication_edge_count(graph: &DistributedExecutionGraph) -> usize {
    graph
        .operations
        .iter()
        .filter(|operation| match operation {
            ExecutionOperation::Launch(_) => false,
            ExecutionOperation::Transfer(_)
            | ExecutionOperation::Collective(_)
            | ExecutionOperation::Barrier(_) => true,
        })
        .count()
}

fn assert_empty_communication_graph(plan: &DistributedExecutionPlan) {
    assert_eq!(
        communication_edge_count(&plan.graph),
        0,
        "N=1 synthesis must not carry Transfer, Collective, or Barrier: {:?}",
        plan.graph.operations
    );
    assert!(plan.graph.transfers().is_empty());
    assert!(plan.graph.collectives().is_empty());
    assert!(plan.graph.barriers().is_empty());
    assert!(plan.graph.commit_boundary.barriers.is_empty());
}

/// Two-kernel single-device program: L1 writes mid, L2 reads mid and writes
/// out (the declared result). All work is local to one device.
fn single_device_program() -> DeviceProgram {
    let mut types = TypeTable::new();
    let f32 = f32_ty(&mut types);
    let version = BufferVersion {
        version: 1,
        element_ty: f32,
        element_count: 64,
        reduced_projection: None,
    };
    let input = identity(1, "input", BufferRole::Input, BufferLifetime::PerProgram);
    let mid = identity(2, "mid", BufferRole::InOut, BufferLifetime::PerStep);
    let output = identity(3, "out", BufferRole::InOut, BufferLifetime::PerProgram);

    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels = vec![
        KernelUnit {
            function: MirFunctionId(0),
            entry: "proj".to_owned(),
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: input,
                    version: version.clone(),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: mid.clone(),
                    version: version.clone(),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(),
        },
        KernelUnit {
            function: MirFunctionId(0),
            entry: "epilogue".to_owned(),
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: mid,
                    version: version.clone(),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: output.clone(),
                    version: version.clone(),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(),
        },
    ];
    program.launches = vec![
        LaunchUnit {
            id: LaunchId(1),
            kernel_index: 0,
        },
        LaunchUnit {
            id: LaunchId(2),
            kernel_index: 1,
        },
    ];
    program.results = vec![ResultBuffer {
        buffer: output,
        version,
        role: BufferRole::InOut,
        produced_by: LaunchId(2),
        cadence: ObservationCadence::EndOfRun,
    }];
    program
        .validate()
        .expect("single-device fixture must validate");
    program
}

#[test]
fn synthesize_is_total_over_single_device_facts() {
    let program = single_device_program();
    let plan = synthesize(&program);

    assert_eq!(plan.partitions.len(), 1);
    let partition = &plan.partitions[0];
    assert_eq!(partition.id, PartitionId(0));
    assert_eq!(partition.kernels, vec![0, 1]);
    assert_eq!(partition.launches, vec![LaunchId(1), LaunchId(2)]);
    assert_eq!(
        partition.buffers,
        vec![BufferId(1), BufferId(2), BufferId(3)]
    );
    assert_eq!(
        partition.results,
        vec![ResultRef {
            buffer: BufferId(3),
            version: 1,
            produced_by: LaunchId(2),
        }]
    );
    assert!(partition.placements.is_empty());

    assert_eq!(plan.graph.launches().len(), 2);
    assert_eq!(plan.graph.launches()[0].launch, LaunchId(1));
    assert_eq!(plan.graph.launches()[0].partition, PartitionId(0));
    assert_eq!(plan.graph.launches()[1].launch, LaunchId(2));
    assert_eq!(plan.graph.launches()[1].partition, PartitionId(0));
    assert_eq!(plan.graph.commit_boundary.launches, vec![LaunchId(2)]);

    assert!(plan.declared_placement.placements.is_empty());
    assert!(plan.declared_placement.constraints.is_empty());
    assert!(plan.declared_placement.objectives.is_empty());
    assert_eq!(plan.version, PlanVersion(LOGICAL_PLAN_VERSION));
}

#[test]
fn synthesize_graph_carries_zero_communication_ops() {
    let plan = synthesize(&single_device_program());
    assert_empty_communication_graph(&plan);
}

#[test]
fn synthesize_is_stable_across_repeated_calls() {
    let program = single_device_program();
    let first = synthesize(&program);
    let second = synthesize(&program);
    assert_eq!(first, second);
    assert_eq!(encode_logical_plan(&first), encode_logical_plan(&second));
}

#[test]
fn synthesize_empty_program_still_yields_one_partition() {
    let program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    let plan = synthesize(&program);

    assert_eq!(plan.partitions.len(), 1);
    let partition = &plan.partitions[0];
    assert_eq!(partition.id, PartitionId(0));
    assert!(partition.kernels.is_empty());
    assert!(partition.launches.is_empty());
    assert!(partition.buffers.is_empty());
    assert!(partition.results.is_empty());
    assert!(plan.graph.operations.is_empty());
    assert_empty_communication_graph(&plan);
}

#[test]
fn synthesize_does_not_invent_a_second_partition() {
    let plan = synthesize(&single_device_program());
    let ids: Vec<PartitionId> = plan
        .partitions
        .iter()
        .map(|partition| partition.id)
        .collect();
    assert_eq!(ids, vec![PartitionId(0)]);
}
