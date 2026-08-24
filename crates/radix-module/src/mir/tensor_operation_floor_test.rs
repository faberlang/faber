//! Tests for the tensor operation floor matrix.
//!
//! These tests prove that required operation families (construction,
//! reductions, matmul, activations, etc.) are each mapped to a per-target
//! status with cited evidence. The floor matrix tracks which tensor
//! operations each backend supports, probes, or defers.

use super::{
    required_tensor_operation_floor_families, tensor_operation_floor_rows, TensorOperationFloorFamily,
    TensorOperationFloorStatus, TensorOperationFloorTarget,
};
use radix_types::MirCollectionOp;

#[test]
fn tensor_operation_floor_families_cover_stage_8_requirements() {
    let expected = [
        TensorOperationFloorFamily::Construction,
        TensorOperationFloorFamily::FlattenOutput,
        TensorOperationFloorFamily::Shape,
        TensorOperationFloorFamily::IndexedAccess,
        TensorOperationFloorFamily::ElementwiseArithmetic,
        TensorOperationFloorFamily::Reductions,
        TensorOperationFloorFamily::Rank2MatMul,
        TensorOperationFloorFamily::Activations,
        TensorOperationFloorFamily::Normalizations,
        TensorOperationFloorFamily::GradientHandles,
    ];

    assert_eq!(required_tensor_operation_floor_families(), &expected);
    for family in expected {
        assert!(
            tensor_operation_floor_rows()
                .iter()
                .any(|row| row.family == family),
            "missing operation floor row for {family:?}"
        );
    }
}

#[test]
fn tensor_operation_floor_families_map_required_mir_ops() {
    let expected = [
        (
            TensorOperationFloorFamily::Construction,
            &[MirCollectionOp::TensorCreate, MirCollectionOp::TensorFromFlat][..],
        ),
        (TensorOperationFloorFamily::FlattenOutput, &[MirCollectionOp::TensorFlatten][..]),
        (TensorOperationFloorFamily::Shape, &[MirCollectionOp::TensorShape][..]),
        (
            TensorOperationFloorFamily::IndexedAccess,
            &[MirCollectionOp::TensorGet, MirCollectionOp::TensorSet][..],
        ),
        (
            TensorOperationFloorFamily::ElementwiseArithmetic,
            &[
                MirCollectionOp::TensorAdd,
                MirCollectionOp::TensorSub,
                MirCollectionOp::TensorMul,
                MirCollectionOp::TensorDiv,
            ][..],
        ),
        (
            TensorOperationFloorFamily::Reductions,
            &[
                MirCollectionOp::Sum,
                MirCollectionOp::TensorMean,
                MirCollectionOp::TensorMax,
                MirCollectionOp::TensorMin,
            ][..],
        ),
        (TensorOperationFloorFamily::Rank2MatMul, &[MirCollectionOp::TensorMatMul][..]),
        // Activations: MirUnOp variants + TensorGelu/TensorRelu (MirCollectionOp activations).
        (
            TensorOperationFloorFamily::Activations,
            &[MirCollectionOp::TensorGelu, MirCollectionOp::TensorRelu][..],
        ),
        (
            TensorOperationFloorFamily::Normalizations,
            &[MirCollectionOp::TensorLayerNorm, MirCollectionOp::TensorSoftmax][..],
        ),
    ];

    for (family, ops) in expected {
        assert_eq!(family.mir_ops(), ops);
    }
}

#[test]
fn tensor_operation_floor_rows_have_required_target_statuses() {
    for family in required_tensor_operation_floor_families() {
        // Normalizations uses different statuses: runner-only execution,
        // all other targets are checked debt (no device-native kernels).
        if *family == TensorOperationFloorFamily::Normalizations {
            let expected = [
                (
                    TensorOperationFloorTarget::MirStepper,
                    TensorOperationFloorStatus::MirExecutable,
                ),
                (TensorOperationFloorTarget::LlvmText, TensorOperationFloorStatus::CheckedDebt),
                (TensorOperationFloorTarget::WasmText, TensorOperationFloorStatus::CheckedDebt),
                (TensorOperationFloorTarget::SexpDump, TensorOperationFloorStatus::CheckedDebt),
                (
                    TensorOperationFloorTarget::NativeSystemsTargets,
                    TensorOperationFloorStatus::CheckedDebt,
                ),
            ];
            for (target, status) in expected {
                let row = tensor_operation_floor_rows()
                    .iter()
                    .find(|row| row.family == *family && row.target == target)
                    .unwrap_or_else(|| panic!("missing floor row for {family:?} on {target:?}"));
                assert_eq!(row.status, status);
                assert!(!row.evidence.is_empty(), "floor row should cite evidence: {row:?}");
            }
            continue;
        }
        if *family == TensorOperationFloorFamily::GradientHandles {
            let expected = [(
                TensorOperationFloorTarget::MirStepper,
                TensorOperationFloorStatus::MirExecutable,
            )];
            for (target, status) in expected {
                let row = tensor_operation_floor_rows()
                    .iter()
                    .find(|row| row.family == *family && row.target == target)
                    .unwrap_or_else(|| panic!("missing floor row for {family:?} on {target:?}"));
                assert_eq!(row.status, status);
                assert!(!row.evidence.is_empty(), "floor row should cite evidence: {row:?}");
            }
            continue;
        }
        let expected = [
            (
                TensorOperationFloorTarget::MirStepper,
                TensorOperationFloorStatus::MirExecutable,
            ),
            (
                TensorOperationFloorTarget::LlvmText,
                TensorOperationFloorStatus::ImportProbeOnly,
            ),
            (
                TensorOperationFloorTarget::WasmText,
                TensorOperationFloorStatus::ImportProbeOnly,
            ),
            (TensorOperationFloorTarget::SexpDump, TensorOperationFloorStatus::StructuralDump),
            (
                TensorOperationFloorTarget::NativeSystemsTargets,
                TensorOperationFloorStatus::CheckedDebt,
            ),
        ];

        for (target, status) in expected {
            let row = tensor_operation_floor_rows()
                .iter()
                .find(|row| row.family == *family && row.target == target)
                .unwrap_or_else(|| panic!("missing floor row for {family:?} on {target:?}"));
            assert_eq!(row.status, status);
            let cites_mir_evidence =
                row.evidence.contains("crates/radix/src/mir") || row.evidence.contains("crates/radix-mir-runner");
            let cites_stage_9_debt = row
                .evidence
                .contains("Stage 9 - Kernel And Systems Target Lowering");
            assert!(
                cites_mir_evidence || cites_stage_9_debt,
                "floor row should cite evidence: {row:?}"
            );
        }
    }
}

#[test]
fn tensor_operation_floor_reuses_indexed_access_parity_source() {
    for row in tensor_operation_floor_rows().iter().filter(|row| {
        row.family == TensorOperationFloorFamily::IndexedAccess
            && row.target != TensorOperationFloorTarget::NativeSystemsTargets
    }) {
        assert!(
            row.evidence
                .contains("tensor_operation_parity.rs::INDEXED_ACCESS_PARITY"),
            "indexed access floor rows should cite Stage 4 parity source: {row:?}"
        );
    }
}
