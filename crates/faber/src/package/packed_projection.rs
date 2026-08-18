//! Dense-projection packed GEMM device-run seam (R-PACK-02O).
//!
//! Names the attention QKV/O, FFN gate/up/down, and logits-head projections
//! and admits each through `QuantizedMatMul`. The prefill f32 bring-up path
//! stays unchanged; this is the packed request the device-run binds.

use radix_mir::abi::PackedStorageLayout;
use radix_mir::kernel_plan::{
    admit_quantized_matmul, CollectionKernelPlan, MatMulShapes, QuantizedMatMulPlan,
};

/// One dense-rung projection (SmolLM2-360M pinned shapes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedProjectionOp {
    /// Tensor / kernel name (`attn_q`, `ffn_gate`, `logits`, …).
    pub name: &'static str,
    /// Activation rows (prompt tokens).
    pub m: u64,
    /// Contracted hidden / head / FFN dim.
    pub k: u64,
    /// Output columns.
    pub n: u64,
}

/// SmolLM2-360M-Instruct dense-rung projection inventory (GQA per-head
/// QKV/O, SwiGLU FFN, tied logits). `T = 9`, `H = 960`, `D = 64`,
/// `FFN = 2560`, `V = 49152`.
pub const DENSE_PROJECTIONS: &[PackedProjectionOp] = &[
    PackedProjectionOp {
        name: "attn_q",
        m: 9,
        k: 960,
        n: 64,
    },
    PackedProjectionOp {
        name: "attn_k",
        m: 9,
        k: 960,
        n: 64,
    },
    PackedProjectionOp {
        name: "attn_v",
        m: 9,
        k: 960,
        n: 64,
    },
    PackedProjectionOp {
        name: "attn_o",
        m: 9,
        k: 64,
        n: 960,
    },
    PackedProjectionOp {
        name: "ffn_gate",
        m: 9,
        k: 960,
        n: 2560,
    },
    PackedProjectionOp {
        name: "ffn_up",
        m: 9,
        k: 960,
        n: 2560,
    },
    PackedProjectionOp {
        name: "ffn_down",
        m: 9,
        k: 2560,
        n: 960,
    },
    PackedProjectionOp {
        name: "logits",
        m: 9,
        k: 960,
        n: 49152,
    },
];

/// Admit `op` through the packed GEMM. F32 would fall back to
/// `TiledMatMul`; the dense-rung layouts stay `QuantizedMatMul`.
#[must_use]
pub fn admit_packed_projection(
    op: PackedProjectionOp,
    layout: PackedStorageLayout,
) -> CollectionKernelPlan {
    admit_quantized_matmul(
        layout,
        MatMulShapes {
            m: op.m,
            k: op.k,
            n: op.n,
        },
    )
}

/// True when the admitted plan is packed (no whole-model F32 expansion).
#[must_use]
pub fn is_packed_quantized_matmul(plan: &CollectionKernelPlan) -> bool {
    matches!(plan, CollectionKernelPlan::QuantizedMatMul(_))
}

/// Extract the packed plan, or `None` if the projection fell back to dense.
#[must_use]
pub fn packed_projection_plan(plan: &CollectionKernelPlan) -> Option<&QuantizedMatMulPlan> {
    match plan {
        CollectionKernelPlan::QuantizedMatMul(quantized) => Some(quantized),
        _ => None,
    }
}

#[cfg(test)]
#[path = "packed_projection_test.rs"]
mod tests;
