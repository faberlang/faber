//! Tensor systems workload proof rows.
//!
//! TARGET: Stage 11 of the tensor systems timeline. These rows consume the GPU
//! workload floor as measured evidence; they do not implement CUDA launch or
//! move output floors by themselves.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TensorWorkloadProofTier {
    FrontendAnalyzed,
    /// MIR lowering succeeded (expression `ad` → `SermoOpen` works).
    /// Rung fails during device staging, not lowering.
    MirLowered,
    /// Output verified against stepper reference — the rung reaches the top tier.
    OutputChecked,
    /// Device staging passed (gate fix + stub); launch is the next step.
    /// Rung fails during launch, not staging.
    DeviceStaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TensorWorkloadProofBucket {
    MirLoweringFailed,
    /// Device IR staging failed — the LLVM/MIR emitter cannot produce a
    /// device-side kernel without a device handle/HostProvider for the route.
    DeviceStagingFailed,
    /// Launch contract step discovered no real device executor.
    /// SermoOpen returns stub handle, but the kernel cannot be dispatched.
    LaunchContractFailed,
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
    pub(super) bucket: Option<TensorWorkloadProofBucket>,
    pub(super) output_checked: bool,
    pub(super) blocker_owner: Option<TensorWorkloadProofOwner>,
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
        tier: TensorWorkloadProofTier::DeviceStaged,
        bucket: Some(TensorWorkloadProofBucket::LaunchContractFailed),
        output_checked: false,
        blocker_owner: Some(TensorWorkloadProofOwner::CudaKernelEmitHostProvider),
        blocker_issue:
            "host provider for route 'cuda:launch' has no real device executor; SermoOpen returns stub handle but launch contract step discovers no device-side kernel launcher",
        evidence: "docs/factory/gpu-workload-floor/baseline-ledger.md::Bucket Ownership (2026-07-22 remeasurement)",
    },
    TensorWorkloadProofRow {
        rung: 1,
        exemplar_path: "tensor-fragment/tiny-linear-device/src/main.fab",
        reference_path: "tensor-fragment/tiny-linear-device/src/main.ref.json",
        expected_stdout_path: "tensor-fragment/tiny-linear-device/src/main.expected",
        selected_operation:
            "rank-2 f32 linear layer on WebGPU device (matmul + elementwise add)",
        tier: TensorWorkloadProofTier::OutputChecked,
        bucket: None,
        output_checked: true,
        blocker_owner: None,
        blocker_issue: "",
        evidence: "crates/exempla/src/exempla_e2e/tensor_workload_proof_test.rs::tensor_workload_proof_rung1_device_linear_matches_stepper + triga/scripta/w4-06b-gpu-proof.mjs (hand-authored inline WGSL mirroring main.fab; headless Chrome WebGPU matmul+add, exit 0, values within f32 tolerance) + hosts/webgpu-browser/public/src/compiler-chain-bridge.mjs (G-P-12 S1 c11cd04c1+S2 275263e: compiler-emitted WGSL+reflection JSON through buildChainFromReflection+runKernelChain; headless Chrome WebGPU matmul+add, exit 0, values within f32 tolerance)",
    },
    TensorWorkloadProofRow {
        rung: 2,
        exemplar_path: "tensor-fragment/tiny-linear-device-relu/src/main.fab",
        reference_path: "tensor-fragment/tiny-linear-device-relu/src/main.ref.json",
        expected_stdout_path: "tensor-fragment/tiny-linear-device-relu/src/main.expected",
        selected_operation:
            "rank-2 f32 linear layer + ReLU activation on WebGPU device (matmul + elementwise add + relu)",
        tier: TensorWorkloadProofTier::MirLowered,
        bucket: Some(TensorWorkloadProofBucket::DeviceStagingFailed),
        output_checked: false,
        blocker_owner: None,
        blocker_issue: "",
        evidence: "rung-2 recorded at the honest tier per wave-4 council item 10 (no OutputChecked claim from fixture validation alone): no w4-06d-gpu-relu-proof.mjs and no chain test exist yet, so device dispatch is a follow-on. Measured 2026-08-01 through the real pipeline (hand-3 G-P-13 S4 probe): frontend analysis OK; MIR lowering now OK — radix 8a09995e4 wired activatio_relu (registry row + method-call lowering, radix crates/radix/src/intrinsics/registry.rs + mir/lower/runtime.rs), unblocking the formerly missing MIR method-call registry row. Current blocker is device IR staging: emit_chain_descriptor fails at the WGSL text probe — target-policy classifier rejects 'MIR-to-WGSL unsupported: kernel runtime call' (device kernel cannot contain a MIR runtime call). Proven so far: radix wgsl_text_test.rs::relu_kernel_emits_valid_wgsl (MirUnOp::Relu emits `max(0.0, operand)` through the expr_for_value arm); exemplar corpus/tensor-fragment/tiny-linear-device-relu frontend-analyzes; hand-computed reference [10.1, 0.0, 0.0, 24.2, 28.1, 0.0, 0.0, 48.2] (4 zeros, 4 non-zeros — ReLU not identity)",
    }];

pub(super) fn tensor_workload_proof_rows() -> &'static [TensorWorkloadProofRow] {
    TENSOR_WORKLOAD_PROOF_ROWS
}

#[cfg(test)]
#[path = "tensor_workload_proof_test.rs"]
mod tests;
