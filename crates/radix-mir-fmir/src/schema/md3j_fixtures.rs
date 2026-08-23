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

/// A single over-budget declaration for synthetic small-card policy tests.
///
/// The partition ledger reports half of this wire declaration (40 GiB), so
/// it stays wire-legal and above the small synthetic policy limit while still
/// admitting on a real 80 GiB A100. Hosts admission, not FMIR decoding, owns
/// the policy decision.
pub(super) const OVER_BUDGET_BYTES: u64 = 80 * 1024 * 1024 * 1024;

/// The real-card rejection variant's post-ledger declaration.
///
/// An A100 80 GiB card has an OQ-2 limit of
/// `floor(85_899_345_920 × 0.9) = 77_309_411_328` bytes. The 79 GiB
/// post-ledger declaration is strictly above that limit. Because the
/// partition ledger is half the wire budget, the postcard carries 158 GiB
/// for its over-budget partition.
pub(super) const REALCARD_OVER_BUDGET_DECLARED_TOTAL_BYTES: u64 = 79 * 1024 * 1024 * 1024;
pub(super) const REALCARD_OVER_BUDGET_BYTES: u64 = REALCARD_OVER_BUDGET_DECLARED_TOTAL_BYTES * 2;

fn base_device_section() -> Result<FmirDeviceSection, String> {
    postcard::from_bytes(include_bytes!(
        "../../tests/fixtures/md3h/eight-rank.postcard"
    ))
    .map_err(|error| format!("MD3H 8-rank postcard decodes: {error}"))
}

fn budget_constraints(over_budget_bytes: Option<u64>) -> Vec<WirePlacementConstraint> {
    (0..EIGHT_RANK_COUNT)
        .map(|partition| WirePlacementConstraint::PartitionBudgetBytes {
            partition,
            budget_bytes: if partition == 0 {
                over_budget_bytes.unwrap_or(EIGHT_GIB_BYTES)
            } else {
                EIGHT_GIB_BYTES
            },
        })
        .collect()
}

fn device_section(over_budget_bytes: Option<u64>) -> Result<FmirDeviceSection, String> {
    let mut device = base_device_section()?;
    let mut distributed = device
        .distributed
        .take()
        .ok_or_else(|| "MD3H 8-rank postcard carries no distributed section".to_owned())?;

    distributed.logical_plan.declared_placement.constraints = budget_constraints(over_budget_bytes);
    let reconstructed = wire_plan_to_plan(&distributed.logical_plan)
        .map_err(|error| format!("MD3J plan reconstructs: {error}"))?;
    distributed.logical_distributed_plan_hash = logical_distributed_plan_hash(&reconstructed);
    device.distributed = Some(distributed);
    Ok(device)
}

/// Honest 8-rank image: every partition declares an 8 GiB budget.
pub(super) fn eight_gib_device_section() -> Result<FmirDeviceSection, String> {
    device_section(None)
}

/// Wire-legal 8-rank image whose 40 GiB ledger declaration is over budget for
/// the small synthetic snapshots used by MD3J-B2, but admits on a real A100.
pub(super) fn over_budget_device_section() -> Result<FmirDeviceSection, String> {
    device_section(Some(OVER_BUDGET_BYTES))
}

/// Wire-legal 8-rank image whose 79 GiB ledger declaration is over budget even
/// for the OQ-2 policy limit of an 80 GiB A100.
pub(super) fn realcard_over_budget_device_section() -> Result<FmirDeviceSection, String> {
    device_section(Some(REALCARD_OVER_BUDGET_BYTES))
}
