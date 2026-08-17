//! Tensor placement policy row and operation contracts.
//!
//! Tests: placement facets (descriptor-supported, reserved-explicit, fail-closed),
//! explicit operation set, and implicit-movement rejection evidence.

use super::{
    tensor_placement_operations, tensor_placement_policy_rows, TensorPlacementFacet, TensorPlacementOperation,
    TensorPlacementStatus,
};

#[test]
fn tensor_placement_policy_rows_record_descriptor_and_transfer_facets() {
    let rows = tensor_placement_policy_rows();
    let expected = [
        (
            TensorPlacementFacet::HostMaterializedPlacement,
            TensorPlacementStatus::DescriptorSupported,
        ),
        (
            TensorPlacementFacet::DeviceHandlePlacement,
            TensorPlacementStatus::DescriptorSupported,
        ),
        (TensorPlacementFacet::CopyIn, TensorPlacementStatus::ReservedExplicitOperation),
        (TensorPlacementFacet::Readback, TensorPlacementStatus::ReservedExplicitOperation),
        (TensorPlacementFacet::Sync, TensorPlacementStatus::ReservedExplicitOperation),
        (TensorPlacementFacet::ImplicitMovement, TensorPlacementStatus::FailClosed),
    ];

    for (facet, status) in expected {
        let row = rows
            .iter()
            .find(|row| row.facet == facet)
            .unwrap_or_else(|| panic!("missing placement policy row for {facet:?}"));
        assert_eq!(row.status, status);
        assert!(!row.evidence.is_empty(), "placement row must cite evidence: {row:?}");
    }
}

#[test]
fn tensor_placement_operations_are_explicit_only() {
    assert_eq!(
        tensor_placement_operations(),
        &[
            TensorPlacementOperation::CopyIn,
            TensorPlacementOperation::Readback,
            TensorPlacementOperation::Sync
        ]
    );
}

#[test]
fn tensor_placement_policy_keeps_implicit_movement_fail_closed() {
    let row = tensor_placement_policy_rows()
        .iter()
        .find(|row| row.facet == TensorPlacementFacet::ImplicitMovement)
        .expect("implicit movement row");

    assert_eq!(row.status, TensorPlacementStatus::FailClosed);
    assert!(row
        .evidence
        .contains("tensor_placement_rejects_implicit_host_to_device_movement"));
}
