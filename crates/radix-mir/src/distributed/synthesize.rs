//! N=1 plan synthesis from a single-device [`DeviceProgram`] (MD3H-R1).
//!
//! [`synthesize`] is a **pure, total** constructor: every single-device
//! program yields a [`DistributedExecutionPlan`] with exactly one logical
//! partition and an empty communication graph. It never rejects, never
//! invents Transfer/Collective/Barrier or host-staged copies, and never
//! names a physical device. The plan is a run-time synthesis — it is not
//! hashed into A10 program bytes.

use crate::device_program::{DeviceProgram, LaunchId};
use crate::distributed::types::{
    DeclaredPlacement, DeviceProgramPartition, DistributedExecutionGraph, DistributedExecutionPlan,
    ExecutionCommitBoundary, ExecutionOperation, LOGICAL_PLAN_VERSION, LaunchOperation,
    PartitionId, PlanVersion, ResultRef,
};

/// The single logical partition of an N=1 synthesized plan (rank 0).
const N1_PARTITION: PartitionId = PartitionId(0);

/// Synthesize the N=1 logical plan from a single-device program.
///
/// The resulting plan has:
/// - exactly one [`DeviceProgramPartition`] (`PartitionId(0)`) covering the
///   program's kernels, launches, buffers, and results;
/// - Launch operations only — zero Transfer, Collective, or Barrier;
/// - empty declared placement (no invented ownership facts or objectives);
/// - [`LOGICAL_PLAN_VERSION`].
///
/// Identical input yields an identical plan. Physical ids never appear.
#[must_use]
pub fn synthesize(program: &DeviceProgram) -> DistributedExecutionPlan {
    let kernels: Vec<usize> = (0..program.kernels.len()).collect();
    let launches: Vec<LaunchId> = program.launches.iter().map(|launch| launch.id).collect();

    let mut buffers: Vec<_> = program
        .buffer_registry()
        .buffers
        .iter()
        .map(|entry| entry.identity.id)
        .collect();
    buffers.sort_unstable();

    let results: Vec<ResultRef> = program
        .results
        .iter()
        .map(|result| ResultRef {
            buffer: result.buffer.id,
            version: result.version.version,
            produced_by: result.produced_by,
        })
        .collect();

    let partition = DeviceProgramPartition {
        id: N1_PARTITION,
        kernels,
        launches,
        buffers,
        results,
        placements: Vec::new(),
    };

    let operations: Vec<ExecutionOperation> = program
        .launches
        .iter()
        .map(|launch| {
            ExecutionOperation::Launch(LaunchOperation {
                launch: launch.id,
                partition: N1_PARTITION,
            })
        })
        .collect();

    let mut boundary_launches: Vec<LaunchId> = Vec::new();
    for result in &program.results {
        if !boundary_launches.contains(&result.produced_by) {
            boundary_launches.push(result.produced_by);
        }
    }

    let graph = DistributedExecutionGraph {
        operations,
        commit_boundary: ExecutionCommitBoundary {
            barriers: Vec::new(),
            launches: boundary_launches,
        },
    };

    DistributedExecutionPlan::assemble(
        DeclaredPlacement::default(),
        vec![partition],
        graph,
        PlanVersion(LOGICAL_PLAN_VERSION),
    )
}

#[cfg(test)]
#[path = "synthesize_test.rs"]
mod synthesize_tests;
