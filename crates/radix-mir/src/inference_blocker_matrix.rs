//! Code-owned blocker matrix for Radix inference north-star claims.
//!
//! Radix has tensor, target, and device-probe building blocks, but it does not
//! yet own an inference runtime. These rows record the current blockers and
//! non-claims so future feature work has a stable gate before advertising
//! model-loading or session execution support.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InferenceBlockerRow {
    pub category: InferenceBlockerKind,
    pub status: InferenceBlockerStatus,
    pub blocker: &'static str,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceBlockerKind {
    ModelLoading,
    TensorOps,
    Placement,
    TargetSurface,
    Session,
}

impl InferenceBlockerKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ModelLoading => "model loading",
            Self::TensorOps => "tensor ops",
            Self::Placement => "placement",
            Self::TargetSurface => "target surface",
            Self::Session => "session",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceBlockerStatus {
    Parked,
    CheckedDebt,
    ReservedExplicitOperation,
    NoRuntimeClaim,
    Delegated,
}

impl InferenceBlockerStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Parked => "parked",
            Self::CheckedDebt => "checked debt",
            Self::ReservedExplicitOperation => "reserved explicit operation",
            Self::NoRuntimeClaim => "no runtime claim",
            Self::Delegated => "delegated",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InferenceNonClaim {
    pub claim: InferenceNonClaimKind,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceNonClaimKind {
    LlamaCppEquivalent,
    ProductionModelLoader,
    TokenizerRuntime,
    QuantizedExecution,
    GpuLaunchRuntime,
    InferenceSessionRunner,
}

impl InferenceNonClaimKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::LlamaCppEquivalent => "llama.cpp-equivalent inference",
            Self::ProductionModelLoader => "production model loader",
            Self::TokenizerRuntime => "tokenizer runtime",
            Self::QuantizedExecution => "quantized execution",
            Self::GpuLaunchRuntime => "GPU launch/runtime",
            Self::InferenceSessionRunner => "inference session runner",
        }
    }
}

pub const INFERENCE_BLOCKER_ROWS: &[InferenceBlockerRow] = &[
    InferenceBlockerRow {
        category: InferenceBlockerKind::ModelLoading,
        status: InferenceBlockerStatus::Parked,
        blocker: "GGUF parsing, tokenizer ownership, quantization, and dynamic-shape handoff remain parked; no production model loader is claimed. F32 safetensors tensor materialization lives in Gradus (`gradus/src/model/`). Inference fragment architecture (tiny linear → BERT-shaped) defines the vertical integration path (docs/factory/mir-swarm/fragment-architecture.md).",
        evidence: "docs/factory/mir-swarm/fragment-architecture.md; gradus/src/model/; docs/factory/safetensors-gguf-breakpoint-spike/goal.md",
    },
    InferenceBlockerRow {
        category: InferenceBlockerKind::TensorOps,
        status: InferenceBlockerStatus::CheckedDebt,
        blocker: "Native systems targets prove rank-1 f32 device-view elementwise arithmetic and reductions; rank-2 matmul promoted to NativeSupport on WgslText; batched matmul (rank-3+), non-WGSL targets (Metal, LLVM, Wasm), and f16/bf16 remain checked debt. Activations (ReLU, Sqrt, Gelu, Softmax forward) are available on Rust reference carrier; device-native activations are checked debt. Softmax VJP is deferred. Normalizations (LayerNorm) are available on Rust reference carrier; device-native normalizations are checked debt.",
        evidence: "crates/radix/src/mir/tensor_systems_target.rs::TENSOR_SYSTEMS_TARGET_ROWS",
    },
    InferenceBlockerRow {
        category: InferenceBlockerKind::Placement,
        status: InferenceBlockerStatus::ReservedExplicitOperation,
        blocker: "Host/device placement descriptors exist, but copy-in, readback, and sync are reserved explicit operations and implicit movement is fail-closed.",
        evidence: "crates/radix/src/mir/tensor_placement.rs::TENSOR_PLACEMENT_POLICY_ROWS",
    },
    InferenceBlockerRow {
        category: InferenceBlockerKind::TargetSurface,
        status: InferenceBlockerStatus::NoRuntimeClaim,
        blocker: "Metal/WGSL emit shader source only and report run/package=false; Radix FMIR image targets are check-only and delegated to Faber package builds.",
        evidence: "crates/radix/src/tool/commands/targets.rs::target_capabilities_for_surface",
    },
    InferenceBlockerRow {
        category: InferenceBlockerKind::Session,
        status: InferenceBlockerStatus::Delegated,
        blocker: "Radix owns compiler inspection and fail-closed emitters, not a loaded-model session lifecycle or product inference runner.",
        evidence: "crates/radix/src/mir/tensor_runtime_abi.rs::TENSOR_RUNTIME_ABI_ROWS",
    },
];

pub const INFERENCE_NON_CLAIMS: &[InferenceNonClaim] = &[
    InferenceNonClaim {
        claim: InferenceNonClaimKind::LlamaCppEquivalent,
        evidence: "crates/radix/src/mir/inference_blocker_matrix.rs::INFERENCE_BLOCKER_ROWS",
    },
    InferenceNonClaim {
        claim: InferenceNonClaimKind::ProductionModelLoader,
        evidence: "docs/factory/safetensors-gguf-breakpoint-spike/goal.md",
    },
    InferenceNonClaim {
        claim: InferenceNonClaimKind::TokenizerRuntime,
        evidence: "docs/factory/safetensors-gguf-breakpoint-spike/goal.md",
    },
    InferenceNonClaim {
        claim: InferenceNonClaimKind::QuantizedExecution,
        evidence: "docs/factory/safetensors-gguf-breakpoint-spike/goal.md",
    },
    InferenceNonClaim {
        claim: InferenceNonClaimKind::GpuLaunchRuntime,
        evidence: "crates/radix/src/tool/commands/targets.rs::radix_target_capabilities",
    },
    InferenceNonClaim {
        claim: InferenceNonClaimKind::InferenceSessionRunner,
        evidence: "crates/radix/src/mir/tensor_runtime_abi.rs::TENSOR_RUNTIME_ABI_ROWS",
    },
];

#[must_use]
pub fn inference_blocker_rows() -> &'static [InferenceBlockerRow] {
    INFERENCE_BLOCKER_ROWS
}

#[must_use]
pub fn inference_non_claims() -> &'static [InferenceNonClaim] {
    INFERENCE_NON_CLAIMS
}
