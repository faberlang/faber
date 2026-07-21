use super::{
    tensor_systems_closeout_rows, TensorSystemsCloseoutFacet, TensorSystemsCloseoutStatus,
};
use crate::exempla_e2e::tensor_package::{
    tensor_package_proof_rows, TensorPackageProofTarget, TENSOR_PACKAGE_PROOF_FIXTURE,
};
use crate::exempla_e2e::tensor_workload_proof::{
    tensor_workload_proof_rows, TensorWorkloadProofBucket,
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
    assert!(tensor_systems_closeout_rows().iter().any(|row| {
        row.facet == TensorSystemsCloseoutFacet::WorkloadFloor
            && row.status == TensorSystemsCloseoutStatus::StableBlocker
    }));
    assert!(tensor_systems_closeout_rows().iter().any(|row| {
        row.facet == TensorSystemsCloseoutFacet::PackageProof
            && row.status == TensorSystemsCloseoutStatus::ExecutableProof
    }));

    let workload = tensor_workload_proof_rows()
        .first()
        .expect("workload proof row");
    assert_eq!(
        workload.bucket,
        TensorWorkloadProofBucket::MirLoweringFailed
    );
    assert!(!workload.output_checked);
    assert!(
        workload.blocker_issue.contains("expression ad"),
        "workload blocker must stay named: {workload:?}"
    );

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
