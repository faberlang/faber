use super::{
    tensor_workload_proof_rows, TensorWorkloadProofBucket, TensorWorkloadProofOwner,
    TensorWorkloadProofTier,
};
use crate::exempla_e2e::gpu_workload::read_reference_fixture;

#[test]
fn tensor_workload_proof_selects_rung0_matmul() {
    let rows = tensor_workload_proof_rows();
    assert_eq!(rows.len(), 2);

    let row = rows[0];
    assert_eq!(row.rung, 0);
    assert_eq!(row.exemplar_path, "gpu-workload/rung-0-matmul.fab");
    assert_eq!(row.reference_path, "gpu-workload/rung-0-matmul.ref.json");
    assert_eq!(
        row.expected_stdout_path,
        "gpu-workload/rung-0-matmul.expected"
    );
    assert_eq!(row.selected_operation, "rank-2 f32 matmul workload");
}

#[test]
fn tensor_workload_proof_records_current_stable_blocker() {
    let row = tensor_workload_proof_rows()[0];

    assert_eq!(row.tier, TensorWorkloadProofTier::MirLowered);
    assert_eq!(row.bucket, Some(TensorWorkloadProofBucket::DeviceStagingFailed));
    assert!(!row.output_checked);
    assert_eq!(
        row.blocker_owner,
        Some(TensorWorkloadProofOwner::CudaKernelEmitHostProvider)
    );
    assert!(row.blocker_issue.contains("host provider"));
    assert!(row.blocker_issue.contains("SermoOpen"));
    assert!(row.blocker_issue.contains("cuda:launch"));
}

#[test]
fn tensor_workload_proof_rung0_reference_fixture_is_valid() {
    // exemplar_path is examples-relative (`gpu-workload/rung-0-matmul.fab`).
    let path = crate::paths::gpu_workload_dir()
        .parent()
        .expect("examples home")
        .join(tensor_workload_proof_rows()[0].exemplar_path);

    let fixture = read_reference_fixture(&path, 0).expect("rung 0 reference fixture");

    assert_eq!(fixture.tolerance, 0.00001);
    assert_eq!(
        fixture.reference,
        serde_json::json!([58.0, 64.0, 139.0, 154.0])
    );
}

#[test]
fn tensor_workload_proof_cites_pinned_gpu_baseline() {
    let row = tensor_workload_proof_rows()[0];

    assert!(row
        .evidence
        .contains("gpu-workload-floor/baseline-ledger.md"));
    assert!(row.evidence.contains("Bucket Ownership"));
}

#[test]
fn tensor_workload_proof_selects_rung1_device_linear() {
    let row = tensor_workload_proof_rows()[1];

    assert_eq!(row.rung, 1);
    assert_eq!(row.tier, TensorWorkloadProofTier::OutputChecked);
    assert!(row.output_checked);
    assert_eq!(row.bucket, None);
    assert_eq!(row.blocker_owner, None);
    assert_eq!(
        row.exemplar_path,
        "corpus/tensor-fragment/tiny-linear-device/src/main.fab"
    );
}

#[test]
#[ignore = "G-P-08 device linear reference + G-SPINE-02 tensor-workload output stepper gate not yet complete"]
fn tensor_workload_proof_rung1_device_linear_matches_stepper() {
    let row = tensor_workload_proof_rows()[1];
    let path = crate::paths::gpu_workload_dir()
        .parent()
        .expect("examples home")
        .join(row.exemplar_path);
    let fixture = read_reference_fixture(&path, 1).expect("rung 1 reference fixture");
    assert_eq!(fixture.tolerance, 0.00001);
    assert_eq!(
        fixture.reference,
        serde_json::json!([9.1, 12.2, 18.1, 24.2, 27.1, 36.2, 36.1, 48.2])
    );
}
