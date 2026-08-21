//! MD3H-F1 public-surface consumption of the committed wire images.
//!
//! Later units (H4/C1/P2) load these postcard fixtures as built artifacts.

use radix_mir_fmir::admit_device_section_with_compat;
use radix_mir_fmir::schema::{
    default_compatibility_table, FmirDeviceSection, WIRE_DISTRIBUTED_SECTION_VERSION,
};

#[test]
fn eight_rank_postcard_admits_on_the_public_surface() {
    let device: FmirDeviceSection =
        postcard::from_bytes(include_bytes!("fixtures/md3h/eight-rank.postcard"))
            .expect("eight-rank.postcard decodes");
    admit_device_section_with_compat(&device, &default_compatibility_table())
        .expect("eight-rank.postcard admits");
    let section = device
        .distributed
        .as_ref()
        .expect("8-rank image carries Some(distributed)");
    assert_eq!(section.v, WIRE_DISTRIBUTED_SECTION_VERSION);
    assert_eq!(section.logical_plan.partitions.len(), 8);
}

#[test]
fn one_partition_postcard_admits_on_the_public_surface() {
    let device: FmirDeviceSection =
        postcard::from_bytes(include_bytes!("fixtures/md3h/one-partition.postcard"))
            .expect("one-partition.postcard decodes");
    admit_device_section_with_compat(&device, &default_compatibility_table())
        .expect("one-partition.postcard admits");
    let section = device
        .distributed
        .as_ref()
        .expect("1-partition image carries Some(distributed)");
    assert_eq!(section.v, WIRE_DISTRIBUTED_SECTION_VERSION);
    assert_eq!(section.logical_plan.partitions.len(), 1);
}
