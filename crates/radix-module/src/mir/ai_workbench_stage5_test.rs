//! Tests for the AI workbench readiness matrix.
//!
//! These tests prove that product-level inference operations (cosine
//! batch) are oracle-proven and that the native dot-batch path stays
//! locked to reduction checked debt. The matrix records what the
//! workbench pipeline requires from each backend.

use super::*;
use crate::mir::{
    AiWorkbenchStage5Facet, AiWorkbenchStage5Status, TensorOperationFloorFamily, TensorSystemsTarget,
    TensorSystemsTargetStatus, tensor_systems_target_rows,
};

#[test]
fn stage5_rows_select_cosine_batch_without_native_claim() {
    let rows = ai_workbench_stage5_rows();

    let product = rows
        .iter()
        .find(|row| row.facet == AiWorkbenchStage5Facet::ProductCosineBatch)
        .expect("product row");
    assert_eq!(product.status, AiWorkbenchStage5Status::ProductOracleProven);
    assert_eq!(product.required_family, None);

    let native = rows
        .iter()
        .find(|row| row.facet == AiWorkbenchStage5Facet::NativeSystemsDotBatch)
        .expect("native dot batch row");
    assert_eq!(native.status, AiWorkbenchStage5Status::NativeCheckedDebt);
    assert_eq!(native.required_family, Some(TensorOperationFloorFamily::Reductions));
    assert_eq!(ai_workbench_stage5_native_status(), TensorSystemsTargetStatus::CheckedDebt);
}

#[test]
fn stage5_native_blocker_reuses_reduction_checked_debt() {
    let reduction_rows: Vec<_> = tensor_systems_target_rows()
        .iter()
        .filter(|row| row.family == TensorOperationFloorFamily::Reductions)
        .collect();

    assert!(
        !reduction_rows.is_empty(),
        "Stage 5 dot batches must stay tied to the code-owned reduction family"
    );

    // WGSL reductions are now native (promoted via commit f4917c740);
    // the stage 5 native blocker stays CheckedDebt because
    // NativeSystemsRemainder reductions are still debt.
    let wgsl_row = reduction_rows
        .iter()
        .find(|row| row.target == TensorSystemsTarget::WgslText)
        .expect("WGSL reduction row must exist");
    assert_eq!(
        wgsl_row.status,
        TensorSystemsTargetStatus::NativeSupport,
        "WGSL reductions are now native; stage 5 blocker uses NativeSystemsRemainder debt"
    );

    let remainder_row = reduction_rows
        .iter()
        .find(|row| row.target == TensorSystemsTarget::NativeSystemsRemainder)
        .expect("NativeSystemsRemainder reduction row must exist");
    assert_eq!(
        remainder_row.status,
        TensorSystemsTargetStatus::CheckedDebt,
        "native dot batches must not be promoted while NativeSystemsRemainder reductions remain checked debt"
    );
}
