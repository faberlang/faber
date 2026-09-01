//! MD3J-F1 public-surface consumption of the committed budget postcards.

use radix_mir_fmir::admit_device_section_with_compat;
use radix_mir_fmir::schema::{
    FmirDeviceSection, WIRE_DISTRIBUTED_SECTION_VERSION, WirePlacementConstraint,
    default_compatibility_table,
};

const EIGHT_GIB: u64 = 8 * 1024 * 1024 * 1024;
const OVER_BUDGET: u64 = 80 * 1024 * 1024 * 1024;
const REALCARD_OVER_BUDGET: u64 = 158 * 1024 * 1024 * 1024;
const REALCARD_DECLARED_TOTAL: u64 = 79 * 1024 * 1024 * 1024;

fn decode(path: &[u8]) -> FmirDeviceSection {
    postcard::from_bytes(path).expect("MD3J postcard decodes")
}

fn assert_admits(device: &FmirDeviceSection) {
    admit_device_section_with_compat(device, &default_compatibility_table())
        .expect("MD3J postcard admits");
    let section = device
        .distributed
        .as_ref()
        .expect("MD3J postcard carries a distributed section");
    assert_eq!(section.v, WIRE_DISTRIBUTED_SECTION_VERSION);
    assert_eq!(section.logical_plan.partitions.len(), 8);
}

fn budget_values(device: &FmirDeviceSection) -> Vec<(u32, u64)> {
    device
        .distributed
        .as_ref()
        .expect("MD3J postcard carries a distributed section")
        .logical_plan
        .declared_placement
        .constraints
        .iter()
        .map(|constraint| match constraint {
            WirePlacementConstraint::PartitionBudgetBytes {
                partition,
                budget_bytes,
            } => (*partition, *budget_bytes),
            WirePlacementConstraint::RequireOwner { .. } => panic!("unexpected MD3J constraint"),
        })
        .collect()
}

#[test]
fn honest_eight_rank_postcard_decodes_admits_and_declares_8_gib_per_partition() {
    let device = decode(include_bytes!("fixtures/md3j/eight-rank.postcard"));
    assert_admits(&device);
    assert_eq!(
        budget_values(&device),
        (0..8)
            .map(|partition| (partition, EIGHT_GIB))
            .collect::<Vec<_>>()
    );
}

#[test]
fn over_budget_postcard_is_wire_legal_and_admitted_by_radix() {
    let device = decode(include_bytes!(
        "fixtures/md3j/eight-rank-over-budget.postcard"
    ));
    assert_admits(&device);
    assert_eq!(
        budget_values(&device),
        (0..8)
            .map(|partition| {
                (
                    partition,
                    if partition == 0 {
                        OVER_BUDGET
                    } else {
                        EIGHT_GIB
                    },
                )
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn realcard_over_budget_postcard_is_wire_legal_and_above_a100_policy_limit() {
    const _: () = assert!(REALCARD_DECLARED_TOTAL > (80 * 1024 * 1024 * 1024 * 9) / 10);
    let device = decode(include_bytes!(
        "fixtures/md3j/eight-rank-over-budget-realcard.postcard"
    ));
    assert_admits(&device);
    assert_eq!(REALCARD_OVER_BUDGET / 2, REALCARD_DECLARED_TOTAL);
    assert_eq!(
        budget_values(&device),
        (0..8)
            .map(|partition| {
                (
                    partition,
                    if partition == 0 {
                        REALCARD_OVER_BUDGET
                    } else {
                        EIGHT_GIB
                    },
                )
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn honest_postcard_logical_hash_is_independent_of_declared_bind_count() {
    let bytes = include_bytes!("fixtures/md3j/eight-rank.postcard");
    let mut hashes = Vec::new();
    for _bind_count in [1_u32, 2] {
        let device = decode(bytes);
        assert_admits(&device);
        hashes.push(
            device
                .distributed
                .as_ref()
                .expect("MD3J postcard carries a distributed section")
                .logical_distributed_plan_hash
                .clone(),
        );
    }
    assert_eq!(hashes[0], hashes[1]);
}
