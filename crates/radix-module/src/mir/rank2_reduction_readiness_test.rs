//! Tests for rank-2 matmul and reduction readiness across target paths.
//!
//! These tests prove that WGSL, LLVM text, and CUDA-directed paths
//! each have a readiness row tied to the operation floor matrix,
//! placement policy, and inference blocker. The readiness matrix
//! gates which paths can claim native support.

use super::*;
use crate::codegen::Target;
use crate::mir::{
    InferenceBlockerKind, InferenceBlockerStatus, TensorOperationFloorStatus, TensorOperationFloorTarget,
    TensorPlacementFacet, TensorPlacementStatus, TensorSystemsTarget, TensorSystemsTargetStatus,
    inference_blocker_rows, tensor_operation_floor_rows, tensor_placement_policy_rows, tensor_systems_target_rows,
};
use crate::services::{TargetCommandSurface, target_capabilities_for_surface};

#[test]
fn readiness_matrix_covers_rank2_and_reductions_across_requested_paths() {
    for family in [
        TensorOperationFloorFamily::Rank2MatMul,
        TensorOperationFloorFamily::Reductions,
    ] {
        for path in [
            Rank2ReductionReadinessPath::WgslText,
            Rank2ReductionReadinessPath::LlvmText,
            Rank2ReductionReadinessPath::CudaDirected,
        ] {
            assert!(
                rank2_reduction_readiness_rows()
                    .iter()
                    .any(|row| row.family == family && row.path == path),
                "missing {} readiness row for {}",
                family.name(),
                path.label()
            );
        }
    }
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_rows_match_native_systems_checked_debt() {
    for family in [
        TensorOperationFloorFamily::Rank2MatMul,
        TensorOperationFloorFamily::Reductions,
    ] {
        let row = row_for(family, Rank2ReductionReadinessPath::WgslText);
        assert_eq!(row.status, Rank2ReductionReadinessStatus::NativeSystemsCheckedDebt);

        assert!(tensor_systems_target_rows().iter().any(|target_row| {
            target_row.family == family
                && target_row.target == TensorSystemsTarget::NativeSystemsRemainder
                && target_row.status == TensorSystemsTargetStatus::CheckedDebt
        }));
        assert!(row.gap.contains("rank-1"));
    }

    let capabilities = target_capabilities_for_surface(TargetCommandSurface::Radix, Target::MirWgsl);
    assert!(capabilities.build);
    assert!(!capabilities.run);
    assert!(!capabilities.package);
}

#[cfg(feature = "mir-llvm")]
#[test]
fn llvm_rows_match_import_probe_only_operation_floor() {
    for family in [
        TensorOperationFloorFamily::Rank2MatMul,
        TensorOperationFloorFamily::Reductions,
    ] {
        let row = row_for(family, Rank2ReductionReadinessPath::LlvmText);
        assert_eq!(row.status, Rank2ReductionReadinessStatus::RuntimeImportOnly);

        assert!(tensor_operation_floor_rows().iter().any(|floor_row| {
            floor_row.family == family
                && floor_row.target == TensorOperationFloorTarget::LlvmText
                && floor_row.status == TensorOperationFloorStatus::ImportProbeOnly
        }));
        assert!(row.gap.contains("import/probe"));
    }

    let capabilities = target_capabilities_for_surface(TargetCommandSurface::Radix, Target::MirLlvm);
    assert!(capabilities.build);
    assert!(!capabilities.run);
    assert!(!capabilities.package);
}

#[test]
fn cuda_directed_rows_match_host_provider_and_placement_blockers() {
    for family in [
        TensorOperationFloorFamily::Rank2MatMul,
        TensorOperationFloorFamily::Reductions,
    ] {
        let row = row_for(family, Rank2ReductionReadinessPath::CudaDirected);
        assert_eq!(row.status, Rank2ReductionReadinessStatus::HostProviderBlocked);
        assert!(row.owner.contains("cuda-kernel-emit"));
        assert!(row.gap.contains("output-checked"));
    }

    for facet in [
        TensorPlacementFacet::CopyIn,
        TensorPlacementFacet::Readback,
        TensorPlacementFacet::Sync,
    ] {
        assert!(tensor_placement_policy_rows().iter().any(|placement_row| {
            placement_row.facet == facet && placement_row.status == TensorPlacementStatus::ReservedExplicitOperation
        }));
    }
}

#[test]
fn readiness_matrix_is_tied_to_inference_tensor_blocker() {
    let tensor_blocker = inference_blocker_rows()
        .iter()
        .find(|row| row.category == InferenceBlockerKind::TensorOps)
        .expect("inference tensor blocker row");
    assert_eq!(tensor_blocker.status, InferenceBlockerStatus::CheckedDebt);
    assert!(tensor_blocker.blocker.contains("reductions"));
    assert!(tensor_blocker.blocker.contains("rank-2 matmul"));
}

fn row_for(
    family: TensorOperationFloorFamily,
    path: Rank2ReductionReadinessPath,
) -> &'static Rank2ReductionReadinessRow {
    rank2_reduction_readiness_rows()
        .iter()
        .find(|row| row.family == family && row.path == path)
        .expect("rank2/reduction readiness row")
}
