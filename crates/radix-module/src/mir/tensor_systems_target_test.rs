//! Tests for the tensor systems target selection matrix.
//!
//! These tests prove that one operation family (elementwise arithmetic)
//! is selected for native GPU targeting, that its rank-1 f32 support
//! is recorded for Metal and WGSL, and that unselected families remain
//! as checked debt.

use super::{
    TensorSystemsTarget, TensorSystemsTargetLayout, TensorSystemsTargetRank, TensorSystemsTargetStatus,
    selected_tensor_systems_family, tensor_systems_target_rows,
};
use crate::mir::TensorOperationFloorFamily;

#[test]
fn tensor_systems_target_selects_elementwise_arithmetic_first() {
    assert_eq!(
        selected_tensor_systems_family(),
        TensorOperationFloorFamily::ElementwiseArithmetic
    );
}

#[test]
fn tensor_systems_target_records_rank1_f32_metal_and_wgsl_support() {
    for target in [TensorSystemsTarget::MetalText, TensorSystemsTarget::WgslText] {
        let row = tensor_systems_target_rows()
            .iter()
            .find(|row| row.family == TensorOperationFloorFamily::ElementwiseArithmetic && row.target == target)
            .unwrap_or_else(|| panic!("missing systems target row for {target:?}"));

        assert_eq!(row.rank, TensorSystemsTargetRank::Rank1);
        assert_eq!(row.layout, TensorSystemsTargetLayout::F32DeviceView);
        assert_eq!(row.status, TensorSystemsTargetStatus::NativeSupport);
        assert!(
            row.evidence.contains("gpu_math_") && row.evidence.contains("f32_addition_kernel"),
            "selected support row should cite focused target evidence: {row:?}"
        );
    }
}

#[test]
fn tensor_systems_target_keeps_unselected_families_as_checked_debt() {
    for family in [
        TensorOperationFloorFamily::Construction,
        TensorOperationFloorFamily::FlattenOutput,
        TensorOperationFloorFamily::Shape,
        TensorOperationFloorFamily::IndexedAccess,
        TensorOperationFloorFamily::Reductions,
        TensorOperationFloorFamily::Rank2MatMul,
    ] {
        let row = tensor_systems_target_rows()
            .iter()
            .find(|row| row.family == family && row.target == TensorSystemsTarget::NativeSystemsRemainder)
            .unwrap_or_else(|| panic!("missing native systems debt row for {family:?}"));

        assert_eq!(row.rank, TensorSystemsTargetRank::Unselected);
        assert_eq!(row.layout, TensorSystemsTargetLayout::Unselected);
        assert_eq!(row.status, TensorSystemsTargetStatus::CheckedDebt);
    }
}
