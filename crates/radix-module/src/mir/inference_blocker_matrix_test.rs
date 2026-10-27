//! Tests for the inference blocker matrix.
//!
//! These tests prove that each inference blocker category (model
//! loading, tensor ops, placement, target surface, session) has a
//! row with a stable status, blocker text, and evidence. The matrix
//! records what the compiler does not yet claim for inference support.

use super::*;
use crate::codegen::Target;
use crate::mir::{
    TensorOperationFloorFamily, TensorPlacementFacet, TensorPlacementStatus, TensorSystemsTarget,
    TensorSystemsTargetStatus, tensor_placement_policy_rows, tensor_systems_target_rows,
};
use crate::services::{TargetCommandSurface, target_capabilities_for_surface};

#[test]
fn matrix_covers_required_inference_blocker_categories() {
    let rows = inference_blocker_rows();

    for category in [
        InferenceBlockerKind::ModelLoading,
        InferenceBlockerKind::TensorOps,
        InferenceBlockerKind::Placement,
        InferenceBlockerKind::TargetSurface,
        InferenceBlockerKind::Session,
    ] {
        assert!(
            rows.iter().any(|row| row.category == category),
            "missing inference blocker category {}",
            category.label()
        );
    }
}

#[test]
fn matrix_records_explicit_inference_non_claims() {
    let non_claims = inference_non_claims();

    for claim in [
        InferenceNonClaimKind::LlamaCppEquivalent,
        InferenceNonClaimKind::ProductionModelLoader,
        InferenceNonClaimKind::TokenizerRuntime,
        InferenceNonClaimKind::QuantizedExecution,
        InferenceNonClaimKind::GpuLaunchRuntime,
        InferenceNonClaimKind::InferenceSessionRunner,
    ] {
        assert!(
            non_claims.iter().any(|non_claim| non_claim.claim == claim),
            "missing inference non-claim {}",
            claim.label()
        );
    }
}

#[test]
fn target_surface_blocker_matches_radix_runtime_capabilities() {
    let row = row_for(InferenceBlockerKind::TargetSurface);
    assert_eq!(row.status, InferenceBlockerStatus::NoRuntimeClaim);

    for target in [
        Target::MirMetal,
        Target::MirWgsl,
        Target::MirFmir,
        Target::MirFmirBinary,
    ] {
        let capabilities = target_capabilities_for_surface(TargetCommandSurface::Radix, target);
        assert!(capabilities.check);
        assert!(!capabilities.run, "{target:?} must not claim radix-run inference support");
        assert!(
            !capabilities.package,
            "{target:?} must not claim radix package inference support"
        );
    }

    for delegated in [Target::MirFmir, Target::MirFmirBinary] {
        let capabilities = target_capabilities_for_surface(TargetCommandSurface::Radix, delegated);
        assert!(capabilities.note.contains("delegated to `faber build"));
    }
}

#[test]
fn tensor_ops_blocker_matches_systems_target_debt() {
    let row = row_for(InferenceBlockerKind::TensorOps);
    assert_eq!(row.status, InferenceBlockerStatus::CheckedDebt);

    for family in [
        TensorOperationFloorFamily::Reductions,
        TensorOperationFloorFamily::Rank2MatMul,
    ] {
        assert!(tensor_systems_target_rows().iter().any(|target_row| {
            target_row.family == family && target_row.status == TensorSystemsTargetStatus::CheckedDebt
        }));
    }

    assert!(tensor_systems_target_rows().iter().any(|target_row| {
        target_row.family == TensorOperationFloorFamily::ElementwiseArithmetic
            && target_row.target == TensorSystemsTarget::MetalText
            && target_row.status == TensorSystemsTargetStatus::NativeSupport
    }));
    assert!(tensor_systems_target_rows().iter().any(|target_row| {
        target_row.family == TensorOperationFloorFamily::ElementwiseArithmetic
            && target_row.target == TensorSystemsTarget::WgslText
            && target_row.status == TensorSystemsTargetStatus::NativeSupport
    }));
}

#[test]
fn placement_blocker_matches_reserved_transfer_policy() {
    let row = row_for(InferenceBlockerKind::Placement);
    assert_eq!(row.status, InferenceBlockerStatus::ReservedExplicitOperation);

    for facet in [
        TensorPlacementFacet::CopyIn,
        TensorPlacementFacet::Readback,
        TensorPlacementFacet::Sync,
    ] {
        assert!(tensor_placement_policy_rows().iter().any(|placement_row| {
            placement_row.facet == facet && placement_row.status == TensorPlacementStatus::ReservedExplicitOperation
        }));
    }

    assert!(tensor_placement_policy_rows().iter().any(|placement_row| {
        placement_row.facet == TensorPlacementFacet::ImplicitMovement
            && placement_row.status == TensorPlacementStatus::FailClosed
    }));
}

#[test]
fn model_loading_and_session_rows_do_not_claim_runtime_support() {
    let model = row_for(InferenceBlockerKind::ModelLoading);
    assert_eq!(model.status, InferenceBlockerStatus::Parked);
    assert!(model.blocker.contains("no production model loader"));
    assert!(model.evidence.contains("safetensors-gguf"));

    let session = row_for(InferenceBlockerKind::Session);
    assert_eq!(session.status, InferenceBlockerStatus::Delegated);
    assert!(session.blocker.contains("not a loaded-model session"));
}

fn row_for(category: InferenceBlockerKind) -> &'static InferenceBlockerRow {
    inference_blocker_rows()
        .iter()
        .find(|row| row.category == category)
        .expect("inference blocker row")
}
