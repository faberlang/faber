//! MD3J-F1 budget-declared 8-rank FMIR fixtures.
//!
//! These fixtures start from the accepted MD3H-F1 8-rank postcard and change
//! only its declared per-partition budgets. The device program and derived
//! logical execution graph therefore remain byte-stable for 8:1 comparability.

use super::super::{
    logical_distributed_plan_hash, wire_plan_to_plan, FmirDeviceSection, WirePlacementConstraint,
};

/// Logical rank count of the MD3J fixture.
pub(super) const EIGHT_RANK_COUNT: u32 = 8;

/// The small software budget declared for every honest partition.
pub(super) const EIGHT_GIB_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// A single over-budget declaration for the A100 80 GiB policy shape.
///
/// Hosts admission applies the named headroom policy. This value is kept
/// wire-legal here so that the hosts seam, not FMIR decoding, owns rejection.
pub(super) const OVER_BUDGET_BYTES: u64 = 80 * 1024 * 1024 * 1024;

fn base_device_section() -> Result<FmirDeviceSection, String> {
    postcard::from_bytes(include_bytes!(
        "../../tests/fixtures/md3h/eight-rank.postcard"
    ))
    .map_err(|error| format!("MD3H 8-rank postcard decodes: {error}"))
}

fn budget_constraints(over_budget: bool) -> Vec<WirePlacementConstraint> {
    (0..EIGHT_RANK_COUNT)
        .map(|partition| WirePlacementConstraint::PartitionBudgetBytes {
            partition,
            budget_bytes: if over_budget && partition == 0 {
                OVER_BUDGET_BYTES
            } else {
                EIGHT_GIB_BYTES
            },
        })
        .collect()
}

fn device_section(over_budget: bool) -> Result<FmirDeviceSection, String> {
    let mut device = base_device_section()?;
    let mut distributed = device
        .distributed
        .take()
        .ok_or_else(|| "MD3H 8-rank postcard carries no distributed section".to_owned())?;

    distributed.logical_plan.declared_placement.constraints = budget_constraints(over_budget);
    let reconstructed = wire_plan_to_plan(&distributed.logical_plan)
        .map_err(|error| format!("MD3J plan reconstructs: {error}"))?;
    distributed.logical_distributed_plan_hash = logical_distributed_plan_hash(&reconstructed);
    device.distributed = Some(distributed);
    Ok(device)
}

/// Honest 8-rank image: every partition declares an 8 GiB budget.
pub(super) fn eight_gib_device_section() -> Result<FmirDeviceSection, String> {
    device_section(false)
}

/// Wire-legal 8-rank image with one declaration above the hosts policy limit.
pub(super) fn over_budget_device_section() -> Result<FmirDeviceSection, String> {
    device_section(true)
}
