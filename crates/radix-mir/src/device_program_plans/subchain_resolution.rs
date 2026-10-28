//! Subchain plan resolution.

use super::{
    PlanMappingError, contract_shape_signature, first_collection_call,
    is_elementwise_collection_op, is_recipe_op, is_transformer_recipe_op, plan_for_collection_op,
    transformer_shape_signature,
};
use crate::abi::{
    CollectionOpContract, MatMulDispatch, MirKernelResourceAccess, MirKernelResourceKind,
    MirKernelSignature, collection_op_contract,
};
use crate::kernel_plan::{CollectionKernelPlan, MatMulShapes};
use crate::{MirCollectionOp, MirFunction, MirLayoutKind, MirType, MirValidationContext};

// ---------------------------------------------------------------------------
// SUBCHAIN PLAN RESOLUTION
// ---------------------------------------------------------------------------

/// Resolve the single plan a subchain kernel carries (S5-U1 decomposition
/// driver seam).
///
/// Consumes the same typed facts the whole-function plan pass consumes —
/// the complete (dead-filtered) op set, the dispatch contract, and the
/// contract-shaped signature ([`contract_shape_signature`]) — so the
/// subchain's plan is decided exactly as a whole-function kernel's is, and
/// a carried plan can be A3 cross-checked against it later. The S3-A5 dead
/// filter resolves reads against `read_context` (the SOURCE function when
/// the plan is resolved for a decomposition subchain: the subchain's
/// produced local is read by a later subchain, which the slice itself does
/// not contain).
///
/// Fail-closed: an unplannable op in the subchain (neither recipe nor
/// elementwise), a recipe op without a resolvable contract or plan, or a
/// subchain whose op set mixes distinct recipes is a typed
/// [`PlanMappingError`] — never a silent `Elementwise`.
///
/// # Errors
/// Returns [`PlanMappingError`] on any of the fail-closed conditions above.
pub fn resolve_subchain_plan(
    function: &MirFunction,
    read_context: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<CollectionKernelPlan, PlanMappingError> {
    let ops = crate::collection_ops::collection_ops_with_read_context(function, read_context);
    let stability_max_dead = crate::abi::stability_max_is_dead(function);
    let mut recipe_op: Option<MirCollectionOp> = None;
    for op in ops {
        // PGC-R5: a DEAD stability-max reduction is not a recipe — the
        // same dead skip the contract resolver applies (a subchain whose
        // only recipe is a dead stability max keeps the elementwise lane;
        // the emitters' dead skips are the precedent).
        if stability_max_dead
            && matches!(op, MirCollectionOp::TensorMax | MirCollectionOp::TensorMin)
        {
            continue;
        }
        if is_recipe_op(op) {
            if let Some(existing) = recipe_op
                && existing != op
            {
                return Err(PlanMappingError::mapping(format!(
                    "subchain op set mixes distinct recipes ({existing:?} and {op:?}); \
                         the decomposition must split them into separate kernels"
                )));
            }
            recipe_op = Some(op);
        } else if !is_elementwise_collection_op(op) {
            return Err(PlanMappingError::mapping(format!(
                "collection op {op:?} has no kernel plan \
                 (no recipe and not an elementwise transform)"
            )));
        }
    }
    let Some(recipe_op) = recipe_op else {
        return Ok(CollectionKernelPlan::Elementwise);
    };
    if is_transformer_recipe_op(recipe_op) {
        // S6-P1: the transformer recipes have no `CollectionOpContract`
        // variant; the plan resolves from the transformer-shaped signature
        // (the same typed facts the plan pass consumes) — the single source
        // of recipe truth.
        let signature = transformer_shape_signature(function, recipe_op, validation)?;
        return CollectionKernelPlan::from_transformer_op(
            &signature, function, validation, recipe_op,
        )
        .ok_or_else(|| {
            PlanMappingError::mapping(format!(
                "collection op {recipe_op:?} has no kernel plan under its typed facts \
                 (axis/epsilon from the const operands, shapes from the tensor types)"
            ))
        });
    }
    let contract = collection_op_contract(function, validation)?.ok_or_else(|| {
        PlanMappingError::mapping(format!(
            "collection op {recipe_op:?} has a recipe but no resolved dispatch contract"
        ))
    })?;
    let signature = contract_shape_signature(function, &contract, validation)?;
    let runtime_matmul = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Runtime,
    };
    let shapes = if recipe_op == MirCollectionOp::TensorMatMul && contract != runtime_matmul {
        kernel_matmul_shapes(function, validation)?
    } else {
        None
    };
    plan_for_collection_op(&signature, &contract, recipe_op, shapes)
}

/// Matmul operand shapes `(M, K, N)` resolved from the `TensorMatMul`
/// statement's rank-2 left and rank-1/rank-2 right tensor operands — the same
/// literal dims the ABI contract resolver reads. The plan also carries each
/// operand's scalar layout so dense emitters can widen BF16 at load time.
/// `None` when the function carries no matmul statement.
pub(super) fn kernel_matmul_shapes(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Option<MatMulShapes>, PlanMappingError> {
    let Some((call, _)) = first_collection_call(function, |op| op == MirCollectionOp::TensorMatMul)
    else {
        return Ok(None);
    };
    if call.args.len() != 2 {
        return Err(PlanMappingError::mapping(
            "matmul requires exactly two tensor operands",
        ));
    }
    let left_ty = crate::local_ty::operand_ty(function, &call.args[0])
        .ok_or_else(|| PlanMappingError::mapping("matmul left operand has no local type"))?;
    let right_ty = crate::local_ty::operand_ty(function, &call.args[1])
        .ok_or_else(|| PlanMappingError::mapping("matmul right operand has no local type"))?;
    let left_all =
        crate::tensor_type::tensor_dims(left_ty, validation).map_err(PlanMappingError::mapping)?;
    let right_all =
        crate::tensor_type::tensor_dims(right_ty, validation).map_err(PlanMappingError::mapping)?;
    if left_all
        .iter()
        .chain(&right_all)
        .any(|dim| matches!(dim, crate::tensor_type::Dim::Runtime))
    {
        // The contract resolver accepts rank-2 run-time extents
        // (`MatMulDispatch::Runtime`); `MatMulShapes` carries literal dims
        // only, so no plan is built from them until the plan arm lands.
        return Err(PlanMappingError::mapping(
            "matmul with run-time extents has no kernel plan yet",
        ));
    }
    let left_dims = crate::tensor_type::tensor_literal_dims(left_ty, validation)
        .map_err(PlanMappingError::mapping)?;
    let right_dims = crate::tensor_type::tensor_literal_dims(right_ty, validation)
        .map_err(PlanMappingError::mapping)?;
    // U1 head-axis: a rank-3/4 receiver with a rank-2+ right operand is the
    // batched contraction; the argument's leading batch dims must be a
    // prefix of the receiver's (the GQA share). The plan's batch counts
    // derive from the signature's resource extents in `from_contract`;
    // this resolver pins the dims law.
    let batched_rank = left_dims.len() >= 3
        && left_dims.len() <= 4
        && right_dims.len() >= 2
        && right_dims.len() <= left_dims.len();
    if batched_rank {
        for axis in 0..right_dims.len() - 2 {
            if left_dims[axis] != right_dims[axis] {
                return Err(PlanMappingError::mapping(
                    "batched matmul requires the right operand's leading batch dims to be a prefix of the left operand's batch dims",
                ));
            }
        }
        if left_dims[left_dims.len() - 1] != right_dims[right_dims.len() - 2] {
            return Err(PlanMappingError::mapping(
                "matmul inner dimensions must match",
            ));
        }
    } else if left_dims.len() != 2 || !(1..=2).contains(&right_dims.len()) {
        return Err(PlanMappingError::mapping(
            "matmul requires a rank-2 left tensor and rank-1 or rank-2 right tensor",
        ));
    } else if left_dims[1] != right_dims[0] {
        return Err(PlanMappingError::mapping(
            "matmul inner dimensions must match",
        ));
    }
    let left_layout = validation
        .layouts
        .layout_for_type(left_ty)
        .map_err(|error| PlanMappingError::mapping(error.message))?;
    let right_layout = validation
        .layouts
        .layout_for_type(right_ty)
        .map_err(|error| PlanMappingError::mapping(error.message))?;
    let (MirLayoutKind::DeviceView(left_view), MirLayoutKind::DeviceView(right_view)) =
        (&left_layout.kind, &right_layout.kind)
    else {
        return Err(PlanMappingError::mapping(
            "matmul operands require device-view layouts",
        ));
    };
    let (Some(left_element), Some(right_element)) =
        (left_view.element.scalar(), right_view.element.scalar())
    else {
        return Err(PlanMappingError::mapping(
            "matmul operands cannot be vector-element device views",
        ));
    };
    // Batched trailing dims: the last two axes are the contraction's
    // [M, K] / [K, N]; rank-2 keeps `right_dims.get(1)` (the promoted
    // rank-1 RHS carries N = 1).
    let (m_index, k_index) = if left_dims.len() >= 3 {
        (left_dims.len() - 2, left_dims.len() - 1)
    } else {
        (0, 1)
    };
    let n = if right_dims.len() >= 2 {
        right_dims[right_dims.len() - 1]
    } else {
        1
    };
    Ok(Some(MatMulShapes {
        m: left_dims[m_index],
        k: left_dims[k_index],
        n,
        left_layout: left_element,
        right_layout: right_element,
    }))
}

/// U1 head-axis: the left and right operand types of the body's
/// `TensorMatMul` statement — the same typed statement scan
/// [`kernel_matmul_shapes`] resolves dims from, factored so
/// [`contract_shape_signature`] can bind the operands' FULL tensor extents
/// (`batch·M·K` / `right_batch·K·N`) as the resource element counts.
pub(super) fn matmul_operand_tys(
    function: &MirFunction,
) -> Result<(MirType, MirType), PlanMappingError> {
    let Some((call, _)) = first_collection_call(function, |op| op == MirCollectionOp::TensorMatMul)
    else {
        return Err(PlanMappingError::mapping(
            "matmul subchain has no TensorMatMul statement",
        ));
    };
    let left_ty = crate::local_ty::operand_ty(function, &call.args[0])
        .ok_or_else(|| PlanMappingError::mapping("matmul left operand has no local type"))?;
    let right_ty = crate::local_ty::operand_ty(function, &call.args[1])
        .ok_or_else(|| PlanMappingError::mapping("matmul right operand has no local type"))?;
    Ok((left_ty, right_ty))
}

/// The declared result type of the body's `TensorMatMul` statement — the
/// `MirRuntimeCall.return_ty` the checked dims reader sizes the output buffer
/// from, so a zero literal axis or an overflowing literal product in the
/// result is a refusal rather than a saturated count.
pub(super) fn matmul_return_ty(function: &MirFunction) -> Result<MirType, PlanMappingError> {
    let Some((call, _)) = first_collection_call(function, |op| op == MirCollectionOp::TensorMatMul)
    else {
        return Err(PlanMappingError::mapping(
            "matmul subchain has no TensorMatMul statement",
        ));
    };
    Ok(call.return_ty)
}
/// The source and scratch contract of one typed prefix collective.
pub(crate) fn scan_plan(
    function: &MirFunction,
    signature: &MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<Option<CollectionKernelPlan>, String> {
    let Some((scan, source)) = crate::abi::scan_node(function) else {
        return Ok(None);
    };
    let source_resource = signature
        .resources_by_binding()
        .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
        .position(|resource| resource.source_local == Some(source))
        .ok_or_else(|| "scan source has no storage-buffer resource".to_owned())?;
    let groups = scan
        .group
        .as_ref()
        .map_or(Some(1u64), |group| {
            group
                .axes
                .iter()
                .try_fold(1u64, |n, axis| n.checked_mul(axis.extent))
        })
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| "scan group domain exceeds device capacity".to_owned())?;
    let element_bytes = signature
        .resources_by_binding()
        .find(|resource| {
            resource.access != MirKernelResourceAccess::Read && resource.source_local.is_some()
        })
        .or(Some(&signature.output))
        .and_then(|resource| validation.layouts.layout_for_type(resource.element_ty).ok())
        .and_then(|layout| match layout.kind {
            crate::MirLayoutKind::Scalar(scalar) => {
                crate::abi::kernel_element_byte_width(scalar).ok()
            }
            _ => None,
        })
        .map(u32::from)
        .ok_or_else(|| "scan output element width is unsupported".to_owned())?;
    let partial_layout =
        crate::abi::scan_partial_layout(scan, validation).map_err(|error| error.message)?;
    Ok(Some(CollectionKernelPlan::Scan {
        source_resource,
        rank: scan.rank,
        workgroup: crate::kernel_plan::GRID_REDUCE_WORKGROUP,
        blocks_cap: crate::kernel_plan::GRID_REDUCE_PARTIALS_CAP,
        group_count: groups,
        partial_layout,
        element_bytes,
        reverse: scan.reverse,
    }))
}
