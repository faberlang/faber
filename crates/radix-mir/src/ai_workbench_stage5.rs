//! Code-owned discovery rows for AI Workbench Stage 5.
//!
//! Stage 5 selects the product query path's batched cosine scoring operation:
//! fixed-width f32 dot products over L2-normalized vectors. These rows connect
//! that product slice to existing MIR tensor capability floors without claiming
//! native GPU support before reductions/dot-product lowering is proven.

use super::{TensorOperationFloorFamily, TensorSystemsTargetStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiWorkbenchStage5Row {
    pub facet: AiWorkbenchStage5Facet,
    pub status: AiWorkbenchStage5Status,
    pub required_family: Option<TensorOperationFloorFamily>,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiWorkbenchStage5Facet {
    ProductCosineBatch,
    Rank1F32ElementwiseProduct,
    Rank1F32Reduction,
    NativeSystemsDotBatch,
}

impl AiWorkbenchStage5Facet {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ProductCosineBatch => "product cosine batch",
            Self::Rank1F32ElementwiseProduct => "rank-1 f32 elementwise product",
            Self::Rank1F32Reduction => "rank-1 f32 reduction",
            Self::NativeSystemsDotBatch => "native systems dot batch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiWorkbenchStage5Status {
    ProductOracleProven,
    ExistingMirFloor,
    NativeCheckedDebt,
}

impl AiWorkbenchStage5Status {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ProductOracleProven => "product oracle proven",
            Self::ExistingMirFloor => "existing MIR floor",
            Self::NativeCheckedDebt => "native checked debt",
        }
    }
}

pub const AI_WORKBENCH_STAGE5_ROWS: &[AiWorkbenchStage5Row] = &[
    AiWorkbenchStage5Row {
        facet: AiWorkbenchStage5Facet::ProductCosineBatch,
        status: AiWorkbenchStage5Status::ProductOracleProven,
        required_family: None,
        evidence: "examples commit 6dcbb3c::check-cosine-batch.py",
    },
    AiWorkbenchStage5Row {
        facet: AiWorkbenchStage5Facet::Rank1F32ElementwiseProduct,
        status: AiWorkbenchStage5Status::ExistingMirFloor,
        required_family: Some(TensorOperationFloorFamily::ElementwiseArithmetic),
        evidence: "crates/radix/src/mir/tensor_systems_target.rs::SELECTED_TENSOR_SYSTEMS_FAMILY",
    },
    AiWorkbenchStage5Row {
        facet: AiWorkbenchStage5Facet::Rank1F32Reduction,
        status: AiWorkbenchStage5Status::ExistingMirFloor,
        required_family: Some(TensorOperationFloorFamily::Reductions),
        evidence: "crates/radix/src/mir/tensor_operation_floor.rs::TENSOR_OPERATION_FLOOR_ROWS",
    },
    AiWorkbenchStage5Row {
        facet: AiWorkbenchStage5Facet::NativeSystemsDotBatch,
        status: AiWorkbenchStage5Status::NativeCheckedDebt,
        required_family: Some(TensorOperationFloorFamily::Reductions),
        evidence: "crates/radix/src/mir/tensor_systems_target.rs::TENSOR_SYSTEMS_TARGET_ROWS",
    },
];

#[must_use]
pub fn ai_workbench_stage5_rows() -> &'static [AiWorkbenchStage5Row] {
    AI_WORKBENCH_STAGE5_ROWS
}

#[must_use]
pub fn ai_workbench_stage5_native_status() -> TensorSystemsTargetStatus {
    TensorSystemsTargetStatus::CheckedDebt
}
