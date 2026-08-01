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
    /// The chain staged AND dispatched on the real device path, but the
    /// readback does not match the stepper reference — a device-result
    /// correctness defect (mirrors the gpu_workload harness NumericMismatch /
    /// RunFailed family). NOT OutputChecked: real dispatch happened but the
    /// output is wrong.
    DeviceResultMismatch,
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
            "sermo_open declare+define collision fixed (radix 663cbfe58, 2026-07-31 23:07): emitted device LLVM text now stages and verifies with llvm-as; binding blocker is the absent CUDA launch provider — host provider for route 'cuda:launch' has no real device executor; SermoOpen returns stub handle but launch contract step discovers no device-side kernel launcher",
        evidence: "docs/factory/gpu-workload-floor/baseline-ledger.md::Bucket Ownership (2026-07-31 re-measurement); re-measured 2026-08-01 (exempla_gpu_workload_e2e, llvm-as 22.1.8): sermo_open declare+define collision fixed in radix 663cbfe58 (2026-07-31 23:07) — emitted device LLVM text now stages and verifies with llvm-as, so rung 0 re-measured to DeviceStaged/LaunchContractFailed (LaunchContractFailed bucket reappeared per reconcile residual #2); CUDA launch provider/runner absent",
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
        evidence: "crates/exempla/src/exempla_e2e/tensor_workload_proof_test.rs::tensor_workload_proof_rung1_device_gpu_chain_dispatch (live: tiny-linear-device compiled via faber Session + WGSL chain-descriptor; headless Chrome WebGPU dispatch through triga/scripta/w4-06c-gpu-chain-proof.mjs + dispatchChainFromDescriptor (hosts 735df10); readback [9.1, 12.2, 18.1, 24.2, 27.1, 36.2, 36.1, 48.2] within 0.00001, exit 0) + triga/scripta/w4-06b-gpu-proof.mjs (hand-authored inline WGSL mirroring main.fab; headless Chrome WebGPU matmul+add, exit 0, values within f32 tolerance) + hosts/webgpu-browser/public/src/compiler-chain-bridge.mjs (G-P-12 S2 275263e: compiler-emitted WGSL+reflection JSON through buildChainFromReflection+runKernelChain; headless Chrome WebGPU matmul+add, exit 0) + hosts/webgpu-browser/public/matmul-proof.html + app-matmul.mjs (W4-06b proof page, e45a9e0)",
    },
    TensorWorkloadProofRow {
        rung: 2,
        exemplar_path: "tensor-fragment/tiny-linear-device-relu/src/main.fab",
        reference_path: "tensor-fragment/tiny-linear-device-relu/src/main.ref.json",
        expected_stdout_path: "tensor-fragment/tiny-linear-device-relu/src/main.expected",
        selected_operation:
            "rank-2 f32 linear layer + ReLU activation on WebGPU device (matmul + elementwise add + relu)",
        tier: TensorWorkloadProofTier::OutputChecked,
        bucket: None,
        output_checked: true,
        blocker_owner: None,
        blocker_issue: "",
        evidence: "rung-2 promoted to OutputChecked 2026-08-01 (task 42aed477): radix 05a47f864 (B1-follow-up) emits a workgroupBarrier() between the fused matmul-add store and the relu pass in the TensorRelu emitter arm (crates/radix-mir-wgsl/src/lib.rs), closing the intra-workgroup readback race first proven by the w4-06d dispatch (the pre-fix all-zeros readback of radix d495c2cff is superseded). Live real-device evidence: crates/exempla/src/exempla_e2e/tensor_workload_proof_test.rs::tensor_workload_proof_rung2_device_gpu_chain_dispatch (non-ignored; tiny-linear-device-relu compiled via faber Session + radix emit_chain_descriptor; headless Chrome WebGPU dispatch through triga/scripta/w4-06d-gpu-relu-proof.mjs (triga c919e62) + dispatchChainFromDescriptor (hosts); readback [10.1, 0, 0, 24.2, 28.1, 0, 0, 48.2] within 0.00001, exactly 4 zeroed elements (ReLU active, not identity), exit 0). Ordering regression gate: wgsl_text_tensor_relu_chain_stages_through_emit_chain_descriptor asserts fused-store -> workgroupBarrier -> relu in the staged kernel",
    }];

pub(super) fn tensor_workload_proof_rows() -> &'static [TensorWorkloadProofRow] {
    TENSOR_WORKLOAD_PROOF_ROWS
}

#[cfg(test)]
#[path = "tensor_workload_proof_test.rs"]
mod tests;
