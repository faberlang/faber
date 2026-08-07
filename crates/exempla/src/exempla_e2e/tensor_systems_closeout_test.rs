use super::{
    tensor_systems_closeout_rows, TensorSystemsCloseoutFacet, TensorSystemsCloseoutStatus,
};
use crate::exempla_e2e::tensor_package::{
    tensor_package_proof_rows, TensorPackageProofTarget, TENSOR_PACKAGE_PROOF_FIXTURE,
};
use crate::exempla_e2e::tensor_workload_proof::{
    tensor_workload_proof_rows, TensorWorkloadProofTier,
};
use radix::mir::{
    required_tensor_operation_floor_families, tensor_operation_floor_rows,
    tensor_systems_target_rows, TensorOperationFloorStatus, TensorOperationFloorTarget,
    TensorSystemsTarget, TensorSystemsTargetStatus,
};

#[test]
fn tensor_systems_closeout_has_one_row_per_required_facet() {
    let rows = tensor_systems_closeout_rows();

    for facet in [
        TensorSystemsCloseoutFacet::OperationFloor,
        TensorSystemsCloseoutFacet::SystemsTargetSupport,
        TensorSystemsCloseoutFacet::WorkloadFloor,
        TensorSystemsCloseoutFacet::PackageProof,
    ] {
        assert!(
            rows.iter().any(|row| row.facet == facet),
            "missing closeout facet {} in {rows:?}",
            facet.label()
        );
    }
}

#[test]
fn tensor_systems_closeout_keeps_capability_floors_code_owned() {
    assert!(tensor_systems_closeout_rows().iter().any(|row| {
        row.facet == TensorSystemsCloseoutFacet::OperationFloor
            && row.status == TensorSystemsCloseoutStatus::CodeOwnedRatchet
    }));
    assert!(tensor_systems_closeout_rows().iter().any(|row| {
        row.facet == TensorSystemsCloseoutFacet::SystemsTargetSupport
            && row.status == TensorSystemsCloseoutStatus::CodeOwnedRatchet
    }));

    for family in required_tensor_operation_floor_families() {
        assert!(
            tensor_operation_floor_rows().iter().any(|row| {
                row.family == *family
                    && row.target == TensorOperationFloorTarget::MirStepper
                    && row.status == TensorOperationFloorStatus::MirExecutable
            }),
            "operation floor family {} must remain MIR-stepper executable",
            family.name()
        );
    }

    assert!(
        tensor_systems_target_rows().iter().any(|row| {
            row.target == TensorSystemsTarget::MetalText
                && row.status == TensorSystemsTargetStatus::NativeSupport
        }),
        "closeout requires at least one Metal systems-target support row"
    );
    assert!(
        tensor_systems_target_rows().iter().any(|row| {
            row.target == TensorSystemsTarget::WgslText
                && row.status == TensorSystemsTargetStatus::NativeSupport
        }),
        "closeout requires at least one WGSL systems-target support row"
    );
}

#[test]
fn tensor_systems_closeout_keeps_workload_blocker_and_package_proof_explicit() {
    // U-06 re-anchor (codex-gap Stage 2): the rung-0 workload row is now
    // output-checked (blocker resolved) via the re-verified nonce-bound U-05
    // receipt (radix 7f520f067, run u05-rerun-nonce1) — the WorkloadFloor
    // facet moved from StableBlocker to ExecutableProof (expected outcome of
    // the U-06 promotion, not a weakening).
    assert!(tensor_systems_closeout_rows().iter().any(|row| {
        row.facet == TensorSystemsCloseoutFacet::WorkloadFloor
            && row.status == TensorSystemsCloseoutStatus::ExecutableProof
    }));
    assert!(tensor_systems_closeout_rows().iter().any(|row| {
        row.facet == TensorSystemsCloseoutFacet::PackageProof
            && row.status == TensorSystemsCloseoutStatus::ExecutableProof
    }));

    let workload = tensor_workload_proof_rows()
        .first()
        .expect("workload proof row");
    // The rung-0 CUDA-route launch-contract blocker is resolved: the row is
    // output-checked with no bucket, no owner, no blocker issue, and the
    // evidence anchors the re-verified receipt (radix 7f520f067) — never a
    // CPU or staged-LLVM fallback.
    assert_eq!(workload.tier, TensorWorkloadProofTier::OutputChecked);
    assert_eq!(workload.bucket, None);
    assert!(workload.output_checked);
    assert_eq!(workload.blocker_owner, None);
    assert_eq!(workload.blocker_issue, "");
    assert!(workload.evidence.contains("7f520f067"));
    assert!(workload.evidence.contains("u05-rerun-nonce1"));

    for target in [
        TensorPackageProofTarget::FmirText,
        TensorPackageProofTarget::Fmir,
        TensorPackageProofTarget::FmirBin,
    ] {
        assert!(
            tensor_package_proof_rows().iter().any(|row| {
                row.fixture_path == TENSOR_PACKAGE_PROOF_FIXTURE && row.target == target
            }),
            "missing package proof row for {target:?}"
        );
    }
}
