//! Code-owned placement and transfer policy rows for MIR tensor systems work.
//!
//! TARGET: Stage 7 of the tensor systems timeline. Descriptor facts may record
//! where tensor data lives, but movement between host and device is explicit and
//! fail-closed until real providers implement these operations.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TensorPlacementPolicyRow {
    pub facet: TensorPlacementFacet,
    pub status: TensorPlacementStatus,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorPlacementFacet {
    HostMaterializedPlacement,
    DeviceHandlePlacement,
    CopyIn,
    Readback,
    Sync,
    ImplicitMovement,
}

impl TensorPlacementFacet {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::HostMaterializedPlacement => "host materialized placement",
            Self::DeviceHandlePlacement => "device handle placement",
            Self::CopyIn => "copy-in transfer",
            Self::Readback => "readback transfer",
            Self::Sync => "sync",
            Self::ImplicitMovement => "implicit movement",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorPlacementStatus {
    DescriptorSupported,
    ReservedExplicitOperation,
    FailClosed,
}

impl TensorPlacementStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::DescriptorSupported => "descriptor-supported",
            Self::ReservedExplicitOperation => "reserved explicit operation",
            Self::FailClosed => "fail-closed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorPlacementOperation {
    CopyIn,
    Readback,
    Sync,
}

impl TensorPlacementOperation {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::CopyIn => "copy-in",
            Self::Readback => "readback",
            Self::Sync => "sync",
        }
    }
}

pub const TENSOR_PLACEMENT_OPERATIONS: &[TensorPlacementOperation] = &[
    TensorPlacementOperation::CopyIn,
    TensorPlacementOperation::Readback,
    TensorPlacementOperation::Sync,
];

pub const TENSOR_PLACEMENT_POLICY_ROWS: &[TensorPlacementPolicyRow] = &[
    TensorPlacementPolicyRow {
        facet: TensorPlacementFacet::HostMaterializedPlacement,
        status: TensorPlacementStatus::DescriptorSupported,
        evidence: "crates/radix-mir/src/layout.rs::MirDeviceViewLayout::materialized_row_major",
    },
    TensorPlacementPolicyRow {
        facet: TensorPlacementFacet::DeviceHandlePlacement,
        status: TensorPlacementStatus::DescriptorSupported,
        evidence: "crates/radix-mir/src/layout.rs::MirDeviceViewLayout::device_handle_row_major",
    },
    TensorPlacementPolicyRow {
        facet: TensorPlacementFacet::CopyIn,
        status: TensorPlacementStatus::ReservedExplicitOperation,
        evidence: "crates/radix-mir/src/tensor_placement.rs::TensorPlacementOperation::CopyIn",
    },
    TensorPlacementPolicyRow {
        facet: TensorPlacementFacet::Readback,
        status: TensorPlacementStatus::ReservedExplicitOperation,
        evidence: "crates/radix-mir/src/tensor_placement.rs::TensorPlacementOperation::Readback",
    },
    TensorPlacementPolicyRow {
        facet: TensorPlacementFacet::Sync,
        status: TensorPlacementStatus::ReservedExplicitOperation,
        evidence: "crates/radix-mir/src/tensor_placement.rs::TensorPlacementOperation::Sync",
    },
    TensorPlacementPolicyRow {
        facet: TensorPlacementFacet::ImplicitMovement,
        status: TensorPlacementStatus::FailClosed,
        evidence: "crates/radix/src/mir/abi_test.rs::tensor_placement_rejects_implicit_host_to_device_movement",
    },
];

#[must_use]
pub fn tensor_placement_operations() -> &'static [TensorPlacementOperation] {
    TENSOR_PLACEMENT_OPERATIONS
}

#[must_use]
pub fn tensor_placement_policy_rows() -> &'static [TensorPlacementPolicyRow] {
    TENSOR_PLACEMENT_POLICY_ROWS
}
