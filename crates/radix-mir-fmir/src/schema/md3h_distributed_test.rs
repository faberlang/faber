//! MD3H-F1: 8-rank and 1-partition distributed FMIR wire images.
//!
//! Hand-assembled fixtures that exercise `FmirDeviceSection.distributed =
//! Some(_)` at decode/admit. Schema sources are not edited; these tests only
//! consume them. Malformed variants fail closed with named diagnostics.
//!
//! The honest 8-rank image is today's `E_DEVICE_DISTRIBUTED_REQUIRES_MD3`
//! run-route subject (captured from `crates/faber/src/package/device/run.rs`
//! lines 193–197, read-only). C1 retires that branch; this crate does not
//! call the faber run route.

#[path = "md3h_fixtures_test.rs"]
mod md3h_fixtures;

use super::{
    FMIR_TARGET_NAME, FmirBinaryImageFile, FmirDeviceSection, FmirTextRuntimeSection,
    FmirTextSourceIdentity, FmirTextSourcesSection, FmirTextToolchainSection, FmirTextTypesSection,
    PACKAGE_MIR_ARTIFACT_VERSION, WIRE_DISTRIBUTED_SECTION_VERSION, WireExecutionOperation,
    WirePlacementContract, WireTransportPath, admit_device_section, admit_distributed_section,
    wire_plan_to_plan,
};
use crate::error::FmirImageError;
use crate::{decode_binary_image, encode_binary_image};
use md3h_fixtures::{
    EIGHT_RANK_COUNT, eight_rank_device_section, eight_rank_program, eight_rank_section,
    one_partition_device_section, one_partition_program, one_partition_section,
};
use radix_mir::MirProgram;
use radix_types::TypeTable;

/// Issue identity of today's single-device run-route rejection
/// (`crates/faber/src/package/device/run.rs:197`). C1 retires this code.
const RUN_ROUTE_REJECTION_ISSUE: &str = "E_DEVICE_DISTRIBUTED_REQUIRES_MD3";

/// Diagnostic body of today's single-device run-route rejection
/// (`crates/faber/src/package/device/run.rs:195`).
const RUN_ROUTE_REJECTION_MESSAGE: &str = "this image declares a distributed execution section; distributed execution requires MD3 — the single-device run route cannot execute it";

fn binary_image(device: FmirDeviceSection) -> FmirBinaryImageFile {
    FmirBinaryImageFile {
        version: PACKAGE_MIR_ARTIFACT_VERSION,
        target: FMIR_TARGET_NAME.to_owned(),
        package_root: ".".to_owned(),
        entry: "main.fab".to_owned(),
        entry_function: "run_entry".to_owned(),
        toolchain: FmirTextToolchainSection {
            faber_cli_version: "0.79.0".to_owned(),
        },
        runtime: FmirTextRuntimeSection {
            requirement: vec!["host:argv".to_owned()],
        },
        sources: FmirTextSourcesSection {
            source: vec![FmirTextSourceIdentity {
                file: "main.fab".to_owned(),
                hash: "fnv64:0000000000000000".to_owned(),
            }],
        },
        cli: None,
        exit_code: Some(0),
        types: FmirTextTypesSection {
            table: TypeTable::new().snapshot(),
        },
        interner: vec!["main".to_owned()],
        program: MirProgram::new(),
        device: Some(device),
    }
}

fn assert_committed_postcard(name: &str, committed: &[u8], device: &FmirDeviceSection) {
    let bytes = postcard::to_allocvec(device).expect("MD3H fixture serializes");
    assert_eq!(
        bytes.as_slice(),
        committed,
        "{name} is stale — regenerate tests/fixtures/md3h/{name} from the constructor"
    );
    let decoded: FmirDeviceSection =
        postcard::from_bytes(committed).unwrap_or_else(|error| panic!("{name} decodes: {error}"));
    admit_device_section(&decoded).unwrap_or_else(|error| panic!("{name} admits: {error}"));
    assert_eq!(decoded, *device);
}

// ---------------------------------------------------------------------------
// Honest 8-rank image: decode + admit
// ---------------------------------------------------------------------------

#[test]
fn eight_rank_section_admits_with_its_own_section_version() {
    let section = eight_rank_section();
    assert_eq!(section.v, WIRE_DISTRIBUTED_SECTION_VERSION);
    admit_distributed_section(&section, &eight_rank_program())
        .expect("honest 8-rank section admits");
    admit_device_section(&eight_rank_device_section())
        .expect("honest 8-rank device section admits");
}

#[test]
fn eight_rank_image_round_trips_through_the_binary_codec() {
    let bytes = encode_binary_image(&binary_image(eight_rank_device_section()))
        .expect("8-rank image encodes");
    let decoded = decode_binary_image(&bytes, "0.79.0").expect("8-rank image decodes and admits");
    let section = decoded
        .device
        .as_ref()
        .and_then(|device| device.distributed.as_ref())
        .expect("decoded 8-rank image carries the distributed section");
    assert_eq!(section.v, WIRE_DISTRIBUTED_SECTION_VERSION);
    assert_eq!(
        section.logical_plan.partitions.len(),
        EIGHT_RANK_COUNT as usize
    );
}

#[test]
fn eight_rank_plan_carries_placements_transfers_broadcast_barrier_and_commit() {
    let section = eight_rank_section();
    let plan = wire_plan_to_plan(&section.logical_plan).expect("8-rank plan reconstructs");
    assert_eq!(plan.partitions.len(), EIGHT_RANK_COUNT as usize);
    for rank in 0..EIGHT_RANK_COUNT {
        assert_eq!(plan.partitions[rank as usize].id.0, rank);
    }
    assert_eq!(plan.graph.launches().len(), EIGHT_RANK_COUNT as usize);
    assert_eq!(plan.graph.transfers().len(), 7);
    let mut transfer_count = 0;
    for operation in &section.logical_plan.graph.operations {
        let WireExecutionOperation::Transfer(transfer) = operation else {
            continue;
        };
        transfer_count += 1;
        assert_eq!(transfer.element_ty, "f32");
        assert_eq!(transfer.path_label, WireTransportPath::HostStaged);
        assert!(transfer.byte_count > 0);
    }
    assert_eq!(transfer_count, 7);
    let collectives = plan.graph.collectives();
    assert_eq!(collectives.len(), 1);
    assert_eq!(collectives[0].participants.len(), EIGHT_RANK_COUNT as usize);
    assert_eq!(collectives[0].source.0, 0);
    assert_eq!(plan.graph.barriers().len(), 1);
    assert!(
        !plan.graph.commit_boundary.barriers.is_empty(),
        "8-rank commit boundary names the result barrier"
    );
    assert!(
        !plan.graph.commit_boundary.launches.is_empty(),
        "8-rank commit boundary names the producing launch"
    );
}

#[test]
fn eight_rank_committed_postcard_matches_the_honest_section() {
    assert_committed_postcard(
        "eight-rank.postcard",
        include_bytes!("../../tests/fixtures/md3h/eight-rank.postcard"),
        &eight_rank_device_section(),
    );
}

// ---------------------------------------------------------------------------
// Honest 1-partition image
// ---------------------------------------------------------------------------

#[test]
fn one_partition_section_admits_with_its_own_section_version() {
    let section = one_partition_section();
    assert_eq!(section.v, WIRE_DISTRIBUTED_SECTION_VERSION);
    admit_distributed_section(&section, &one_partition_program())
        .expect("honest 1-partition section admits");
    admit_device_section(&one_partition_device_section())
        .expect("honest 1-partition device section admits");
}

#[test]
fn one_partition_image_round_trips_through_the_binary_codec() {
    let bytes = encode_binary_image(&binary_image(one_partition_device_section()))
        .expect("1-partition image encodes");
    let decoded =
        decode_binary_image(&bytes, "0.79.0").expect("1-partition image decodes and admits");
    let section = decoded
        .device
        .as_ref()
        .and_then(|device| device.distributed.as_ref())
        .expect("decoded 1-partition image carries the distributed section");
    assert_eq!(section.v, WIRE_DISTRIBUTED_SECTION_VERSION);
    assert_eq!(section.logical_plan.partitions.len(), 1);
}

#[test]
fn one_partition_plan_has_no_transfers_or_collectives() {
    let plan = wire_plan_to_plan(&one_partition_section().logical_plan)
        .expect("1-partition plan reconstructs");
    assert_eq!(plan.partitions.len(), 1);
    assert_eq!(plan.partitions[0].id.0, 0);
    assert!(plan.graph.transfers().is_empty());
    assert!(plan.graph.collectives().is_empty());
    assert_eq!(plan.graph.launches().len(), 2);
}

#[test]
fn one_partition_committed_postcard_matches_the_honest_section() {
    assert_committed_postcard(
        "one-partition.postcard",
        include_bytes!("../../tests/fixtures/md3h/one-partition.postcard"),
        &one_partition_device_section(),
    );
}

// ---------------------------------------------------------------------------
// Malformed variants fail closed with named diagnostics
// ---------------------------------------------------------------------------

#[test]
fn eight_rank_bad_placement_fails_closed() {
    let mut section = eight_rank_section();
    section.declared_placement[1].contract = WirePlacementContract::Owned { owner: 7 };
    match admit_distributed_section(&section, &eight_rank_program()) {
        Err(FmirImageError::DistributedSectionEquivalenceMismatch { detail }) => {
            assert!(
                detail.contains("stale or tampered"),
                "bad placement must name the equivalence-gate diagnostic, got {detail}"
            );
        }
        other => panic!("expected DistributedSectionEquivalenceMismatch, got {other:?}"),
    }
}

#[test]
fn eight_rank_unknown_partition_fails_closed() {
    let mut section = eight_rank_section();
    let launch = match &mut section.logical_plan.graph.operations[0] {
        WireExecutionOperation::Launch(launch) => launch,
        other => panic!("operation 0 must be a launch, got {other:?}"),
    };
    launch.partition = 99;
    match admit_distributed_section(&section, &eight_rank_program()) {
        Err(FmirImageError::DistributedSectionInvalid { detail }) => {
            assert!(
                detail.contains("topology-mismatch"),
                "unknown partition must name the topology-mismatch class, got {detail}"
            );
            assert!(
                detail.contains("PartitionId(99)"),
                "unknown partition diagnostic must name PartitionId(99), got {detail}"
            );
        }
        other => panic!("expected DistributedSectionInvalid topology-mismatch, got {other:?}"),
    }
}

#[test]
fn eight_rank_stale_section_version_fails_closed() {
    for stale in [
        WIRE_DISTRIBUTED_SECTION_VERSION - 1,
        WIRE_DISTRIBUTED_SECTION_VERSION + 1,
    ] {
        let mut section = eight_rank_section();
        section.v = stale;
        match admit_distributed_section(&section, &eight_rank_program()) {
            Err(FmirImageError::DistributedSectionVersionMismatch { actual, expected }) => {
                assert_eq!(actual, stale);
                assert_eq!(expected, WIRE_DISTRIBUTED_SECTION_VERSION);
            }
            other => {
                panic!("expected UnsupportedDistributedSectionVersion for {stale}, got {other:?}")
            }
        }
        assert!(matches!(
            admit_device_section(&{
                let mut device = eight_rank_device_section();
                device.distributed = Some(section);
                device
            }),
            Err(FmirImageError::DistributedSectionVersionMismatch { .. })
        ));
    }
}

// ---------------------------------------------------------------------------
// C1 flip subject: today's run-route rejection
// ---------------------------------------------------------------------------

#[test]
fn eight_rank_image_is_the_c1_run_route_rejection_subject() {
    let device = eight_rank_device_section();
    assert!(
        device.distributed.is_some(),
        "the 8-rank fixture must carry Some(distributed) so today's run route rejects it"
    );
    // Captured 2026-08-21 from crates/faber/src/package/device/run.rs:193-197
    // (read-only). C1 retires this branch; these constants are the flip
    // evidence, not a live coupling to faber sources.
    assert_eq!(
        RUN_ROUTE_REJECTION_ISSUE,
        "E_DEVICE_DISTRIBUTED_REQUIRES_MD3"
    );
    assert_eq!(
        RUN_ROUTE_REJECTION_MESSAGE,
        "this image declares a distributed execution section; distributed execution requires MD3 — the single-device run route cannot execute it"
    );
}
