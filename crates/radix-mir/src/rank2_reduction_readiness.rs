//! Code-owned readiness map for rank-2 matmul and reduction systems paths.
//!
//! These rows explain why inference-shaped tensor work still stops short of a
//! native systems target claim. They do not implement lowering or execution;
//! they pin the next blocking facts across WGSL, LLVM text, and CUDA-directed
//! paths so later work has to move an explicit gate.

use super::TensorOperationFloorFamily;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rank2ReductionReadinessRow {
    pub family: TensorOperationFloorFamily,
    pub path: Rank2ReductionReadinessPath,
    pub status: Rank2ReductionReadinessStatus,
    pub owner: &'static str,
    pub gap: &'static str,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rank2ReductionReadinessPath {
    WgslText,
    LlvmText,
    CudaDirected,
}

impl Rank2ReductionReadinessPath {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::WgslText => "wgsl-text",
            Self::LlvmText => "llvm-text",
            Self::CudaDirected => "cuda-directed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rank2ReductionReadinessStatus {
    NativeSystemsCheckedDebt,
    RuntimeImportOnly,
    HostProviderBlocked,
}

impl Rank2ReductionReadinessStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::NativeSystemsCheckedDebt => "native systems checked debt",
            Self::RuntimeImportOnly => "runtime import only",
            Self::HostProviderBlocked => "host provider blocked",
        }
    }
}

pub const RANK2_REDUCTION_READINESS_ROWS: &[Rank2ReductionReadinessRow] = &[
    Rank2ReductionReadinessRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        path: Rank2ReductionReadinessPath::WgslText,
        status: Rank2ReductionReadinessStatus::NativeSystemsCheckedDebt,
        owner: "radix MIR GPU/tensor systems",
        gap: "WGSL native systems support only proves rank-1 f32 device-view elementwise arithmetic; rank-2 matmul still needs multi-axis indexing, inner-dimension accumulation, output-shape, and storage-view proof before shader-source readiness.",
        evidence: "crates/radix/src/mir/tensor_systems_target.rs::TENSOR_SYSTEMS_TARGET_ROWS",
    },
    Rank2ReductionReadinessRow {
        family: TensorOperationFloorFamily::Reductions,
        path: Rank2ReductionReadinessPath::WgslText,
        status: Rank2ReductionReadinessStatus::NativeSystemsCheckedDebt,
        owner: "radix MIR GPU/tensor systems",
        gap: "WGSL native systems support lacks reduction-axis, accumulator, broadcast/softmax, synchronization, and reduced-output-shape proof; current native rows stop at rank-1 elementwise arithmetic.",
        evidence: "crates/radix/src/mir/tensor_systems_target.rs::TENSOR_SYSTEMS_TARGET_ROWS",
    },
    Rank2ReductionReadinessRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        path: Rank2ReductionReadinessPath::LlvmText,
        status: Rank2ReductionReadinessStatus::RuntimeImportOnly,
        owner: "runtime helper consumer",
        gap: "LLVM text can name the tensor_matmul runtime helper over opaque tensor handles, but that is an import/probe boundary, not native loop/vector lowering or linked output-checked execution.",
        evidence: "crates/radix/src/mir/llvm_text/tensor.rs::TensorCall::MatMul",
    },
    Rank2ReductionReadinessRow {
        family: TensorOperationFloorFamily::Reductions,
        path: Rank2ReductionReadinessPath::LlvmText,
        status: Rank2ReductionReadinessStatus::RuntimeImportOnly,
        owner: "runtime helper consumer",
        gap: "LLVM text can name tensor_sum and tensor_mean runtime helpers over opaque tensor handles, but reductions are still import/probe-only rather than native lowering or linked output-checked execution.",
        evidence: "crates/radix/src/mir/llvm_text/tensor.rs::TensorCall::Sum",
    },
    Rank2ReductionReadinessRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        path: Rank2ReductionReadinessPath::CudaDirected,
        status: Rank2ReductionReadinessStatus::HostProviderBlocked,
        owner: "cuda-kernel-emit host/provider path",
        gap: "The GPU workload rung-0 matmul proof has a reference but no output-checked device success; device handle placement, CUDA launch, copy/readback, and provider lifecycle remain outside Radix execution.",
        evidence: "docs/factory/gpu-workload-floor/baseline-ledger.md::rung-0-matmul",
    },
    Rank2ReductionReadinessRow {
        family: TensorOperationFloorFamily::Reductions,
        path: Rank2ReductionReadinessPath::CudaDirected,
        status: Rank2ReductionReadinessStatus::HostProviderBlocked,
        owner: "cuda-kernel-emit host/provider path",
        gap: "The GPU workload rung-1 softmax/reduction path has a reference but no output-checked device success; device reduction/broadcasting plus CUDA launch and readback remain unproven.",
        evidence: "docs/factory/gpu-workload-floor/baseline-ledger.md::rung-1-softmax",
    },
];

#[must_use]
pub fn rank2_reduction_readiness_rows() -> &'static [Rank2ReductionReadinessRow] {
    RANK2_REDUCTION_READINESS_ROWS
}
