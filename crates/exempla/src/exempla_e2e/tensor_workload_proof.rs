//! Tensor systems workload proof rows.
//!
//! TARGET: Stage 11 of the tensor systems timeline. These rows consume the GPU
//! workload floor as measured evidence; they do not implement CUDA launch or
//! move output floors by themselves.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TensorWorkloadProofTier {
    FrontendAnalyzed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TensorWorkloadProofBucket {
    MirLoweringFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TensorWorkloadProofOwner {
    CudaKernelEmitHostProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TensorWorkloadProofRow {
    pub(super) rung: usize,
    pub(super) exemplar_path: &'static str,
    pub(super) reference_path: &'static str,
    pub(super) expected_stdout_path: &'static str,
    pub(super) selected_operation: &'static str,
    pub(super) tier: TensorWorkloadProofTier,
    pub(super) bucket: TensorWorkloadProofBucket,
    pub(super) output_checked: bool,
    pub(super) blocker_owner: TensorWorkloadProofOwner,
    pub(super) blocker_issue: &'static str,
    pub(super) evidence: &'static str,
}

pub(super) const TENSOR_WORKLOAD_PROOF_ROWS: &[TensorWorkloadProofRow] =
    &[TensorWorkloadProofRow {
        rung: 0,
        exemplar_path: "gpu-workload/rung-0-matmul.fab",
        reference_path: "gpu-workload/rung-0-matmul.ref.json",
        expected_stdout_path: "gpu-workload/rung-0-matmul.expected",
        selected_operation: "rank-2 f32 matmul workload",
        tier: TensorWorkloadProofTier::FrontendAnalyzed,
        bucket: TensorWorkloadProofBucket::MirLoweringFailed,
        output_checked: false,
        blocker_owner: TensorWorkloadProofOwner::CudaKernelEmitHostProvider,
        blocker_issue:
            "expression ad has no stream-conversation MIR lowering before CUDA launch measurement",
        evidence: "docs/factory/gpu-workload-floor/baseline-ledger.md::Bucket Ownership",
    }];

pub(super) fn tensor_workload_proof_rows() -> &'static [TensorWorkloadProofRow] {
    TENSOR_WORKLOAD_PROOF_ROWS
}

#[cfg(test)]
#[path = "tensor_workload_proof_test.rs"]
mod tests;
