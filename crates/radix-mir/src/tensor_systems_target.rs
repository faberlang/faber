//! Code-owned systems-target tensor support rows.
//!
//! TARGET: Stage 9 of the tensor systems timeline. Rows here pay down native
//! systems checked debt from the Stage 8 operation floor one target/family/rank
//! slice at a time.

use super::TensorOperationFloorFamily;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TensorSystemsTargetRow {
    pub family: TensorOperationFloorFamily,
    pub target: TensorSystemsTarget,
    pub rank: TensorSystemsTargetRank,
    pub layout: TensorSystemsTargetLayout,
    pub status: TensorSystemsTargetStatus,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorSystemsTarget {
    MetalText,
    WgslText,
    NativeSystemsRemainder,
    RustReference,
}

impl TensorSystemsTarget {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::MetalText => "metal-text",
            Self::WgslText => "wgsl-text",
            Self::NativeSystemsRemainder => "native systems remainder",
            Self::RustReference => "rust-reference",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorSystemsTargetRank {
    Rank1,
    Rank2,
    Unselected,
}

impl TensorSystemsTargetRank {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Rank1 => "rank-1",
            Self::Rank2 => "rank-2",
            Self::Unselected => "unselected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorSystemsTargetLayout {
    F32DeviceView,
    Unselected,
}

impl TensorSystemsTargetLayout {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::F32DeviceView => "f32 device view",
            Self::Unselected => "unselected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorSystemsTargetStatus {
    NativeSupport,
    CheckedDebt,
}

impl TensorSystemsTargetStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::NativeSupport => "native support",
            Self::CheckedDebt => "checked debt",
        }
    }
}

pub const SELECTED_TENSOR_SYSTEMS_FAMILY: TensorOperationFloorFamily =
    TensorOperationFloorFamily::ElementwiseArithmetic;

pub const TENSOR_SYSTEMS_TARGET_ROWS: &[TensorSystemsTargetRow] = &[
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::ElementwiseArithmetic,
        target: TensorSystemsTarget::MetalText,
        rank: TensorSystemsTargetRank::Rank1,
        layout: TensorSystemsTargetLayout::F32DeviceView,
        status: TensorSystemsTargetStatus::NativeSupport,
        evidence: "crates/radix/src/mir/metal_text_test.rs::gpu_math_metal_text_target_emits_f32_addition_kernel",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::ElementwiseArithmetic,
        target: TensorSystemsTarget::WgslText,
        rank: TensorSystemsTargetRank::Rank1,
        layout: TensorSystemsTargetLayout::F32DeviceView,
        status: TensorSystemsTargetStatus::NativeSupport,
        evidence: "crates/radix/src/mir/wgsl_text_test.rs::gpu_math_wgsl_text_target_emits_f32_addition_kernel",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Reductions,
        target: TensorSystemsTarget::WgslText,
        rank: TensorSystemsTargetRank::Rank1,
        layout: TensorSystemsTargetLayout::F32DeviceView,
        status: TensorSystemsTargetStatus::NativeSupport,
        evidence: "crates/radix/src/mir/wgsl_text_test.rs::gpu_math_wgsl_text_target_emits_f32_reduction_kernel",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        target: TensorSystemsTarget::WgslText,
        rank: TensorSystemsTargetRank::Rank2,
        layout: TensorSystemsTargetLayout::F32DeviceView,
        status: TensorSystemsTargetStatus::NativeSupport,
        evidence: "crates/radix/src/mir/wgsl_text_test.rs::wgsl_text_target_emits_f32_matmul_kernel",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Construction,
        target: TensorSystemsTarget::NativeSystemsRemainder,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-target-lowering/stage-9-delivery.md::Non-goals",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::FlattenOutput,
        target: TensorSystemsTarget::NativeSystemsRemainder,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-target-lowering/stage-9-delivery.md::Non-goals",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Shape,
        target: TensorSystemsTarget::NativeSystemsRemainder,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::CheckedDebt,
        evidence: "docs/factory/tensor-systems-target-lowering/stage-9-delivery.md::Non-goals",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::IndexedAccess,
        target: TensorSystemsTarget::NativeSystemsRemainder,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::CheckedDebt,
        evidence: "crates/radix/src/mir/tensor_operation_parity.rs::INDEXED_ACCESS_PARITY",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Reductions,
        target: TensorSystemsTarget::NativeSystemsRemainder,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::CheckedDebt,
        evidence: "Reductions promoted to WgslText rank-1 f32 device-view via commit f4917c740; non-WGSL targets remain checked debt (Stage 9 delivery :: Non-goals)",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Activations,
        target: TensorSystemsTarget::RustReference,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::NativeSupport,
        evidence: "faber/runtime/rust/src/tensor.rs::relu, sqrt, gelu, softmax; crates/radix-air/src/reverse_ad_ops_test.rs activation companion proofs",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Activations,
        target: TensorSystemsTarget::NativeSystemsRemainder,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::CheckedDebt,
        evidence: "docs/factory/mir-swarm/delivery/d-p-09-activation-expansion.md::Non-goals — device-native Gelu and Softmax deferred to post-v1; Softmax VJP deferred",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Normalizations,
        target: TensorSystemsTarget::RustReference,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::NativeSupport,
        evidence: "faber/runtime/rust/src/tensor.rs::layernorm; crates/radix-air/src/reverse_ad_ops_test.rs LayerNorm companion proofs",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Normalizations,
        target: TensorSystemsTarget::NativeSystemsRemainder,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::CheckedDebt,
        evidence: "docs/factory/mir-swarm/delivery/d-p-10-layernorm.md::Non-goals — No device-native layernorm kernels",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        target: TensorSystemsTarget::MetalText,
        rank: TensorSystemsTargetRank::Rank2,
        layout: TensorSystemsTargetLayout::F32DeviceView,
        status: TensorSystemsTargetStatus::NativeSupport,
        evidence: "crates/radix-mir-metal/src/lib.rs::metal_text_emits_tiled_matmul_kernel (Metal M-mat tiled-matmul lowering landed via commit 3da583286)",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::Rank2MatMul,
        target: TensorSystemsTarget::NativeSystemsRemainder,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::CheckedDebt,
        evidence: "Rank2MatMul promoted to MetalText (commit 3da583286) and WgslText rank-2 f32 device-view; LLVM, Wasm, and Sexp targets remain checked debt (Stage 9 delivery :: Non-goals)",
    },
    TensorSystemsTargetRow {
        family: TensorOperationFloorFamily::GradientHandles,
        target: TensorSystemsTarget::WgslText,
        rank: TensorSystemsTargetRank::Unselected,
        layout: TensorSystemsTargetLayout::Unselected,
        status: TensorSystemsTargetStatus::NativeSupport,
        evidence: "radix-mir-wgsl/src/gradient.rs emits host-side gradient annotations; hosts/webgpu-browser/public/src/gradient-handle-check.mjs passes all 9 behavioral test steps (create, accumulate ×2, read, zero, shape-mismatch, unknown-handle)",
    },
];

#[must_use]
pub fn selected_tensor_systems_family() -> TensorOperationFloorFamily {
    SELECTED_TENSOR_SYSTEMS_FAMILY
}

#[must_use]
pub fn tensor_systems_target_rows() -> &'static [TensorSystemsTargetRow] {
    TENSOR_SYSTEMS_TARGET_ROWS
}
