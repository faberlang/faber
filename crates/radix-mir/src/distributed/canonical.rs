//! Canonical byte encoding of the logical distributed plan (MD2-D1).
//!
//! One **stable field-ordered encoding** over the logical plan classes — the
//! hash substrate for `logical_distributed_plan_hash` (MD2-H1/W1). It
//! mirrors the `canonical_program_bytes` discipline (FC17): a fixed field
//! order, fixed integer endianness (little-endian), length-prefixed lists,
//! and a discriminant byte per enum. Identical plans encode to identical
//! bytes; different plans encode to different bytes (the determinism test in
//! `distributed_test.rs` proves both directions).
//!
//! The encoding is a **plan substrate only** — it never contains physical
//! ids, ordinals-as-identity, topology facts, or priority policy, and it is
//! not the A10 `canonical_program_bytes` (the single-device program bytes
//! stay the A10 substrate; the distributed section never enters them —
//! naming contract §2).

use crate::distributed::types::{
    CollectiveKind, DistributedExecutionPlan, ExecutionCommitBoundary, ExecutionOperation,
    PlacementConstraint, PlacementContract, PlacementObjective, TransferDirection, TransportPath,
};
use crate::layout::MirTensorStorageLayout;

/// Canonical encoding magic header.
pub const CANONICAL_MAGIC: &[u8; 8] = b"RDXLPLAN";

/// Canonical encoding revision (bump only for a breaking field-order change;
/// the plan shape version is [`crate::distributed::LOGICAL_PLAN_VERSION`]).
pub const CANONICAL_REVISION: u8 = 1;

/// Encode one logical plan as the canonical byte string.
#[must_use]
pub fn encode_logical_plan(plan: &DistributedExecutionPlan) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(CANONICAL_MAGIC);
    bytes.push(CANONICAL_REVISION);

    // Plan version.
    write_u32(&mut bytes, plan.version.0);

    // Declared placement: placements (ascending value order), then
    // constraints, then objectives.
    write_count(&mut bytes, plan.declared_placement.placements.len());
    for placement in &plan.declared_placement.placements {
        write_u32(&mut bytes, placement.value.0);
        write_contract(&mut bytes, &placement.contract);
    }
    write_count(&mut bytes, plan.declared_placement.constraints.len());
    for constraint in &plan.declared_placement.constraints {
        match constraint {
            PlacementConstraint::PartitionBudgetBytes {
                partition,
                budget_bytes,
            } => {
                bytes.push(0x00);
                write_u32(&mut bytes, partition.0);
                write_u64(&mut bytes, *budget_bytes);
            }
            PlacementConstraint::RequireOwner { value, partition } => {
                bytes.push(0x01);
                write_u32(&mut bytes, value.0);
                write_u32(&mut bytes, partition.0);
            }
        }
    }
    write_count(&mut bytes, plan.declared_placement.objectives.len());
    for objective in &plan.declared_placement.objectives {
        match objective {
            PlacementObjective::MinimizeTransferBytes => bytes.push(0x00),
            PlacementObjective::MaximizeOwnerAffinity => bytes.push(0x01),
        }
    }

    // Partitions (ascending partition id).
    write_count(&mut bytes, plan.partitions.len());
    for partition in &plan.partitions {
        write_u32(&mut bytes, partition.id.0);
        write_count(&mut bytes, partition.kernels.len());
        for kernel in &partition.kernels {
            write_u64(&mut bytes, *kernel as u64);
        }
        write_u32_list(
            &mut bytes,
            &partition.launches.iter().map(|id| id.0).collect::<Vec<_>>(),
        );
        write_u32_list(
            &mut bytes,
            &partition.buffers.iter().map(|id| id.0).collect::<Vec<_>>(),
        );
        write_count(&mut bytes, partition.results.len());
        for result in &partition.results {
            write_u32(&mut bytes, result.buffer.0);
            write_u32(&mut bytes, result.version);
            write_u32(&mut bytes, result.produced_by.0);
        }
        write_count(&mut bytes, partition.placements.len());
        for placement in &partition.placements {
            write_u32(&mut bytes, placement.value.0);
            write_contract(&mut bytes, &placement.contract);
        }
    }

    // Execution graph operations, in plan order.
    write_count(&mut bytes, plan.graph.operations.len());
    for operation in &plan.graph.operations {
        match operation {
            ExecutionOperation::Launch(launch) => {
                bytes.push(0x00);
                write_u32(&mut bytes, launch.launch.0);
                write_u32(&mut bytes, launch.partition.0);
            }
            ExecutionOperation::Transfer(transfer) => {
                bytes.push(0x01);
                write_u32(&mut bytes, transfer.id.0);
                write_u32(&mut bytes, transfer.value.0);
                write_u32(&mut bytes, transfer.source.0);
                write_u32(&mut bytes, transfer.destination.0);
                write_u64(&mut bytes, transfer.byte_count);
                match transfer.direction {
                    TransferDirection::H2D => bytes.push(0x00),
                    TransferDirection::D2H => bytes.push(0x01),
                    TransferDirection::BIDI => bytes.push(0x02),
                }
                write_mir_type(&mut bytes, transfer.element_ty);
                write_storage_layout(&mut bytes, transfer.layout);
                match transfer.path_label {
                    TransportPath::HostStaged => bytes.push(0x00),
                }
                write_u32(&mut bytes, transfer.producer_generation);
                write_u32(&mut bytes, transfer.consumer_generation);
                write_boundary(&mut bytes, &transfer.completion_boundary);
            }
            ExecutionOperation::Collective(collective) => {
                bytes.push(0x02);
                write_u32(&mut bytes, collective.id.0);
                write_u32(&mut bytes, collective.value.0);
                match collective.kind {
                    CollectiveKind::Broadcast => bytes.push(0x00),
                }
                write_u32(&mut bytes, collective.source.0);
                write_u32_list(
                    &mut bytes,
                    &collective
                        .participants
                        .iter()
                        .map(|id| id.0)
                        .collect::<Vec<_>>(),
                );
                write_u64(&mut bytes, collective.byte_count);
            }
            ExecutionOperation::Barrier(barrier) => {
                bytes.push(0x03);
                write_u32(&mut bytes, barrier.id.0);
                write_u32_list(
                    &mut bytes,
                    &barrier.partitions.iter().map(|id| id.0).collect::<Vec<_>>(),
                );
            }
        }
    }

    // Graph-level declared commit boundary.
    write_boundary(&mut bytes, &plan.graph.commit_boundary);

    bytes
}

/// Write a placement contract.
fn write_contract(bytes: &mut Vec<u8>, contract: &PlacementContract) {
    match contract {
        PlacementContract::Owned { owner } => {
            bytes.push(0x00);
            write_u32(bytes, owner.0);
        }
        PlacementContract::Replicated { primary, replicas } => {
            bytes.push(0x01);
            write_u32(bytes, primary.0);
            write_u32_list(bytes, &replicas.iter().map(|id| id.0).collect::<Vec<_>>());
        }
    }
}

/// Write an execution commit boundary (barriers, then launches).
fn write_boundary(bytes: &mut Vec<u8>, boundary: &ExecutionCommitBoundary) {
    write_u32_list(
        bytes,
        &boundary.barriers.iter().map(|id| id.0).collect::<Vec<_>>(),
    );
    write_u32_list(
        bytes,
        &boundary.launches.iter().map(|id| id.0).collect::<Vec<_>>(),
    );
}

/// Write a MIR type as its stable semantic id plus an optional layout id.
fn write_mir_type(bytes: &mut Vec<u8>, ty: crate::MirType) {
    write_u32(bytes, ty.semantic_id().0);
    match ty.layout_id() {
        None => bytes.push(0x00),
        Some(layout) => {
            bytes.push(0x01);
            write_u32(bytes, layout.0);
        }
    }
}

/// Write a tensor storage layout.
fn write_storage_layout(bytes: &mut Vec<u8>, layout: MirTensorStorageLayout) {
    match layout {
        MirTensorStorageLayout::HostOwned => bytes.push(0x00),
        MirTensorStorageLayout::DeviceHandle => bytes.push(0x01),
    }
}

/// Write a length prefix.
fn write_count(bytes: &mut Vec<u8>, count: usize) {
    write_u64(bytes, count as u64);
}

/// Write a u32, little-endian.
fn write_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// Write a u64, little-endian.
fn write_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// Write a length-prefixed u32 list.
fn write_u32_list(bytes: &mut Vec<u8>, values: &[u32]) {
    write_count(bytes, values.len());
    for value in values {
        write_u32(bytes, *value);
    }
}
