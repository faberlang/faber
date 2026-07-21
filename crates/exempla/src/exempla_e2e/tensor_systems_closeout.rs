//! Tensor systems campaign closeout ratchet.
//!
//! TARGET: Stage 13 of the tensor systems timeline. This table ties the code
//! owned tensor operation floor, systems target rows, workload blocker row, and
//! FMIR package proof together so closeout cannot drift into a docs-only claim.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TensorSystemsCloseoutFacet {
    OperationFloor,
    SystemsTargetSupport,
    WorkloadFloor,
    PackageProof,
}

impl TensorSystemsCloseoutFacet {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::OperationFloor => "operation floor",
            Self::SystemsTargetSupport => "systems target support",
            Self::WorkloadFloor => "workload floor",
            Self::PackageProof => "package proof",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TensorSystemsCloseoutStatus {
    CodeOwnedRatchet,
    StableBlocker,
    ExecutableProof,
}

impl TensorSystemsCloseoutStatus {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::CodeOwnedRatchet => "code-owned ratchet",
            Self::StableBlocker => "stable blocker",
            Self::ExecutableProof => "executable proof",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TensorSystemsCloseoutRow {
    pub(super) facet: TensorSystemsCloseoutFacet,
    pub(super) status: TensorSystemsCloseoutStatus,
    pub(super) evidence: &'static str,
}

pub(super) const TENSOR_SYSTEMS_CLOSEOUT_ROWS: &[TensorSystemsCloseoutRow] = &[
    TensorSystemsCloseoutRow {
        facet: TensorSystemsCloseoutFacet::OperationFloor,
        status: TensorSystemsCloseoutStatus::CodeOwnedRatchet,
        evidence: "crates/radix/src/mir/tensor_operation_floor.rs::tensor_operation_floor_rows",
    },
    TensorSystemsCloseoutRow {
        facet: TensorSystemsCloseoutFacet::SystemsTargetSupport,
        status: TensorSystemsCloseoutStatus::CodeOwnedRatchet,
        evidence: "crates/radix/src/mir/tensor_systems_target.rs::tensor_systems_target_rows",
    },
    TensorSystemsCloseoutRow {
        facet: TensorSystemsCloseoutFacet::WorkloadFloor,
        status: TensorSystemsCloseoutStatus::StableBlocker,
        evidence:
            "crates/exempla/src/exempla_e2e/tensor_workload_proof.rs::tensor_workload_proof_rows",
    },
    TensorSystemsCloseoutRow {
        facet: TensorSystemsCloseoutFacet::PackageProof,
        status: TensorSystemsCloseoutStatus::ExecutableProof,
        evidence: "crates/exempla/src/exempla_e2e/tensor_package.rs::tensor_package_proof_rows",
    },
];

pub(super) fn tensor_systems_closeout_rows() -> &'static [TensorSystemsCloseoutRow] {
    TENSOR_SYSTEMS_CLOSEOUT_ROWS
}

#[cfg(test)]
#[path = "tensor_systems_closeout_test.rs"]
mod tests;
