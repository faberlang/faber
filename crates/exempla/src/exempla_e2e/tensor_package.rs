//! Tensor package proof rows for the FMIR package lane.
//!
//! TARGET: Stage 12 of the tensor systems timeline. These tests prove a tracked
//! tensor package fixture runs through FMIR package artifacts without falling
//! back to generated Rust package output.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TensorPackageProofTarget {
    FmirText,
    Fmir,
    FmirBin,
}

impl TensorPackageProofTarget {
    pub(super) fn cli_target(self) -> &'static str {
        match self {
            Self::FmirText => "fmir-text",
            Self::Fmir => "fmir",
            Self::FmirBin => "fmir-bin",
        }
    }

    fn artifact_path(self) -> &'static str {
        match self {
            Self::FmirText => "target/faber-mir/image.fmir.txt",
            Self::Fmir => "target/faber-mir/image.fmir",
            Self::FmirBin => "target/faber-mir/exe/run",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TensorPackageProofRow {
    pub(super) fixture_path: &'static str,
    pub(super) expected_stdout: &'static str,
    pub(super) target: TensorPackageProofTarget,
    pub(super) evidence: &'static str,
}

pub(super) const TENSOR_PACKAGE_PROOF_FIXTURE: &str = "tensor-package/fmir-matmul";

pub(super) const TENSOR_PACKAGE_PROOF_STDOUT: &str =
    "38.0\n44.0\n50.0\n56.0\n83.0\n98.0\n113.0\n128.0\n";

pub(super) const TENSOR_PACKAGE_PROOF_ROWS: &[TensorPackageProofRow] = &[
    TensorPackageProofRow {
        fixture_path: TENSOR_PACKAGE_PROOF_FIXTURE,
        expected_stdout: TENSOR_PACKAGE_PROOF_STDOUT,
        target: TensorPackageProofTarget::FmirText,
        evidence: "crates/exempla/src/exempla_e2e/tensor_package.rs::tensor_package_runs_through_fmir_targets_without_rust_fallback",
    },
    TensorPackageProofRow {
        fixture_path: TENSOR_PACKAGE_PROOF_FIXTURE,
        expected_stdout: TENSOR_PACKAGE_PROOF_STDOUT,
        target: TensorPackageProofTarget::Fmir,
        evidence: "crates/exempla/src/exempla_e2e/tensor_package.rs::tensor_package_runs_through_fmir_targets_without_rust_fallback",
    },
    TensorPackageProofRow {
        fixture_path: TENSOR_PACKAGE_PROOF_FIXTURE,
        expected_stdout: TENSOR_PACKAGE_PROOF_STDOUT,
        target: TensorPackageProofTarget::FmirBin,
        evidence: "crates/exempla/src/exempla_e2e/tensor_package.rs::tensor_package_runs_through_fmir_targets_without_rust_fallback",
    },
];

pub(super) fn tensor_package_proof_rows() -> &'static [TensorPackageProofRow] {
    TENSOR_PACKAGE_PROOF_ROWS
}

#[cfg(test)]
#[path = "tensor_package_test.rs"]
mod tests;
