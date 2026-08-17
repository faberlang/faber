//! Tensor operation parity: which MIR backends support which tensor operation families.
//!
//! Tests: each family has expected target-status rows with evidence citations.

use super::{indexed_access_parity, TensorOperationFamily, TensorParityStatus, TensorParityTarget};
use radix_types::MirCollectionOp;

#[test]
fn tensor_operation_parity_indexed_access_tracks_get_and_set() {
    assert_eq!(
        TensorOperationFamily::IndexedAccess.mir_ops(),
        &[MirCollectionOp::TensorGet, MirCollectionOp::TensorSet]
    );
}

#[test]
fn tensor_operation_parity_indexed_access_has_expected_target_rows() {
    let rows = indexed_access_parity();
    let expected = [
        (TensorParityTarget::RustRuntime, TensorParityStatus::Support),
        (TensorParityTarget::MirLowering, TensorParityStatus::Support),
        (TensorParityTarget::MirStepper, TensorParityStatus::Support),
        (TensorParityTarget::LlvmText, TensorParityStatus::ImportProbeOnly),
        (TensorParityTarget::WasmText, TensorParityStatus::ImportProbeOnly),
        (TensorParityTarget::MetalText, TensorParityStatus::Rank1DeviceViewSupport),
        (TensorParityTarget::WgslText, TensorParityStatus::Rank1DeviceViewSupport),
        (TensorParityTarget::SexpDump, TensorParityStatus::StructuralDump),
    ];

    assert_eq!(rows.len(), expected.len());
    for (target, status) in expected {
        let row = rows
            .iter()
            .find(|row| row.target == target)
            .unwrap_or_else(|| panic!("missing parity row for {target:?}"));
        assert_eq!(row.family, TensorOperationFamily::IndexedAccess);
        assert_eq!(row.status, status);
        assert!(
            row.evidence.contains("crates/radix/src") || row.evidence.contains("crates/faber/src"),
            "parity row evidence should point at checked source: {row:?}"
        );
    }
}
