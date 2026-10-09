//! MD3J-F1: budget-declared 8-rank distributed FMIR wire images.
//!
//! The honest image keeps the MD3H-F1 device program and logical execution
//! graph unchanged while declaring 8 GiB for every partition. The over-budget
//! image is intentionally wire-legal: hosts admission owns its rejection.

#[path = "md3j_fixtures_test.rs"]
mod md3j_fixtures;

use super::{
    FmirDeviceSection, WIRE_DISTRIBUTED_SECTION_VERSION, WirePlacementConstraint,
    admit_device_section, admit_distributed_section, wire_plan_to_plan,
};
use md3j_fixtures::{
    EIGHT_GIB_BYTES, EIGHT_RANK_COUNT, OVER_BUDGET_BYTES, REALCARD_OVER_BUDGET_BYTES,
    REALCARD_OVER_BUDGET_DECLARED_TOTAL_BYTES,
};

fn eight_gib_device_section() -> FmirDeviceSection {
    md3j_fixtures::eight_gib_device_section().expect("MD3J honest fixture builds")
}

fn over_budget_device_section() -> FmirDeviceSection {
    md3j_fixtures::over_budget_device_section().expect("MD3J over-budget fixture builds")
}

fn realcard_over_budget_device_section() -> FmirDeviceSection {
    md3j_fixtures::realcard_over_budget_device_section()
        .expect("MD3J real-card over-budget fixture builds")
}

fn distributed(device: &FmirDeviceSection) -> &super::FmirDistributedExecutionSection {
    device
        .distributed
        .as_ref()
        .expect("MD3J image carries a distributed section")
}

fn budgets(device: &FmirDeviceSection) -> Vec<(u32, u64)> {
    distributed(device)
        .logical_plan
        .declared_placement
        .constraints
        .iter()
        .map(|constraint| match constraint {
            WirePlacementConstraint::PartitionBudgetBytes {
                partition,
                budget_bytes,
            } => (*partition, *budget_bytes),
            WirePlacementConstraint::RequireOwner { .. } => {
                panic!("MD3J fixture carries an unexpected constraint")
            }
        })
        .collect()
}

fn assert_admits_at_wire_level(device: &FmirDeviceSection) {
    let section = distributed(device);
    assert_eq!(section.v, WIRE_DISTRIBUTED_SECTION_VERSION);
    admit_distributed_section(section, &device.device_program.program)
        .expect("MD3J distributed section admits at wire level");
    admit_device_section(device).expect("MD3J device section admits at wire level");
}

#[test]
fn eight_gib_image_declares_every_partition_budget() {
    let device = eight_gib_device_section();
    assert_admits_at_wire_level(&device);
    assert_eq!(
        distributed(&device).logical_plan.partitions.len(),
        EIGHT_RANK_COUNT as usize
    );
    assert_eq!(
        budgets(&device),
        (0..EIGHT_RANK_COUNT)
            .map(|partition| (partition, EIGHT_GIB_BYTES))
            .collect::<Vec<_>>()
    );
}

#[test]
fn over_budget_image_is_wire_legal_and_admitted_before_hosts_policy() {
    let device = over_budget_device_section();
    assert_admits_at_wire_level(&device);
    assert_eq!(
        budgets(&device),
        (0..EIGHT_RANK_COUNT)
            .map(|partition| {
                (
                    partition,
                    if partition == 0 {
                        OVER_BUDGET_BYTES
                    } else {
                        EIGHT_GIB_BYTES
                    },
                )
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn realcard_over_budget_image_is_wire_legal_and_documents_the_policy_margin() {
    const _: () = assert!(
        REALCARD_OVER_BUDGET_DECLARED_TOTAL_BYTES > (80 * 1024 * 1024 * 1024 * 9) / 10 /* 79 GiB ledger declaration must exceed the 80 GiB A100 OQ-2 limit */
    );
    let device = realcard_over_budget_device_section();
    assert_admits_at_wire_level(&device);
    assert_eq!(
        REALCARD_OVER_BUDGET_BYTES / 2,
        REALCARD_OVER_BUDGET_DECLARED_TOTAL_BYTES
    );
    assert_eq!(
        budgets(&device),
        (0..EIGHT_RANK_COUNT)
            .map(|partition| {
                (
                    partition,
                    if partition == 0 {
                        REALCARD_OVER_BUDGET_BYTES
                    } else {
                        EIGHT_GIB_BYTES
                    },
                )
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn budget_variants_keep_the_md3h_program_and_logical_graph_byte_stable() {
    let baseline: FmirDeviceSection = postcard::from_bytes(include_bytes!(
        "../../tests/fixtures/md3h/eight-rank.postcard"
    ))
    .expect("MD3H 8-rank postcard decodes");
    let baseline_section = distributed(&baseline);
    let baseline_graph = postcard::to_allocvec(&baseline_section.logical_plan.graph)
        .expect("MD3H logical graph serializes");

    for candidate in [
        eight_gib_device_section(),
        over_budget_device_section(),
        realcard_over_budget_device_section(),
    ] {
        let candidate_section = distributed(&candidate);
        assert_eq!(
            candidate.device_program.program, baseline.device_program.program,
            "MD3J must not change the single-device program"
        );
        assert_eq!(
            postcard::to_allocvec(&candidate_section.logical_plan.graph)
                .expect("MD3J logical graph serializes"),
            baseline_graph,
            "MD3J must not change the MD3H logical graph"
        );
        assert_eq!(
            candidate_section.logical_plan.partitions, baseline_section.logical_plan.partitions,
            "MD3J must not change the MD3H partition graph"
        );
    }
}

#[test]
fn the_same_image_admits_unchanged_for_bind_counts_one_and_two() {
    let bytes = postcard::to_allocvec(&eight_gib_device_section()).expect("MD3J image serializes");
    let mut hashes = Vec::new();

    for bind_count in [1_u32, 2] {
        let decoded: FmirDeviceSection =
            postcard::from_bytes(&bytes).expect("MD3J image decodes for either bind count");
        assert_admits_at_wire_level(&decoded);
        hashes.push(distributed(&decoded).logical_distributed_plan_hash.clone());
        assert_eq!(
            distributed(&decoded).logical_plan.partitions.len(),
            EIGHT_RANK_COUNT as usize,
            "bind-count {bind_count} must not rewrite the logical image"
        );
    }

    assert_eq!(
        hashes[0], hashes[1],
        "bind count is a host fact, not a logical-plan input"
    );
}

#[test]
fn md3j_logical_hash_matches_the_reconstructed_wire_plan() {
    for device in [
        eight_gib_device_section(),
        over_budget_device_section(),
        realcard_over_budget_device_section(),
    ] {
        let section = distributed(&device);
        let reconstructed =
            wire_plan_to_plan(&section.logical_plan).expect("MD3J plan reconstructs");
        assert_eq!(
            section.logical_distributed_plan_hash,
            super::logical_distributed_plan_hash(&reconstructed)
        );
    }
}

fn assert_committed_postcard(name: &str, committed: &[u8], device: &FmirDeviceSection) {
    let bytes = postcard::to_allocvec(device).expect("MD3J fixture serializes");
    assert_eq!(
        bytes.as_slice(),
        committed,
        "{name} is stale — regenerate tests/fixtures/md3j/{name} from the builder"
    );
    let decoded: FmirDeviceSection =
        postcard::from_bytes(committed).unwrap_or_else(|error| panic!("{name} decodes: {error}"));
    assert_admits_at_wire_level(&decoded);
    assert_eq!(decoded, *device);
}

#[test]
fn committed_honest_postcard_matches_the_builder() {
    assert_committed_postcard(
        "eight-rank.postcard",
        include_bytes!("../../tests/fixtures/md3j/eight-rank.postcard"),
        &eight_gib_device_section(),
    );
}

#[test]
fn committed_over_budget_postcard_matches_the_builder() {
    assert_committed_postcard(
        "eight-rank-over-budget.postcard",
        include_bytes!("../../tests/fixtures/md3j/eight-rank-over-budget.postcard"),
        &over_budget_device_section(),
    );
}

#[test]
fn committed_realcard_over_budget_postcard_matches_the_builder() {
    assert_committed_postcard(
        "eight-rank-over-budget-realcard.postcard",
        include_bytes!("../../tests/fixtures/md3j/eight-rank-over-budget-realcard.postcard"),
        &realcard_over_budget_device_section(),
    );
}
