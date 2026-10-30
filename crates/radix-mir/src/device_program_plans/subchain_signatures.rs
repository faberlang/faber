//! Builds plan and emission signatures from typed MIR subchains.
//!
//! Plan signatures describe recipe resources; emission signatures additionally
//! bind carried data-flow inputs and consumed outputs. Resource extents and
//! source-local identities must agree across producing and consuming subchains.

use super::{
    PlanMappingError, is_contract_recipe_op, is_recipe_op, kernel_matmul_shapes,
    matmul_operand_tys, matmul_return_ty,
};
use crate::abi::{
    CollectionOpContract, MatMulDispatch, MirKernelResource, MirKernelResourceAccess,
    MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature, device_view_element_count,
    device_view_element_count_or_runtime,
};
use crate::{
    MirCollectionOp, MirConstant, MirFunction, MirIntrinsic, MirKernelDispatchSize, MirLayoutKind,
    MirLocalId, MirOperand, MirPlaceBase, MirRuntimeCall, MirStatementKind, MirType,
    MirValidationContext, MirValueKind, MirWorkgroupSize,
};
use radix_lexer::Interner;
use radix_types::{NumericWidth, Primitive, Type, TypeId};

/// First collection runtime call that satisfies `visit`, in statement-scan
/// order. Shared by the recipe-operand, destination, and shape scans so each
/// site names the op it wants instead of re-walking the body.
fn find_collection_call<'a, T>(
    function: &'a MirFunction,
    mut visit: impl FnMut(MirCollectionOp, &'a MirRuntimeCall, Option<&'a crate::MirPlace>) -> Option<T>,
) -> Option<T> {
    for block in &function.blocks {
        for statement in &block.statements {
            let Some(op) = crate::collection_ops::statement_collection_op(statement) else {
                continue;
            };
            let MirStatementKind::RuntimeCall { destination, call } = &statement.kind else {
                continue;
            };
            if let Some(found) = visit(op, call, destination.as_ref()) {
                return Some(found);
            }
        }
    }
    None
}

pub(super) fn first_collection_call(
    function: &MirFunction,
    pred: impl Fn(MirCollectionOp) -> bool,
) -> Option<(&MirRuntimeCall, Option<&crate::MirPlace>)> {
    find_collection_call(function, |op, call, dest| pred(op).then_some((call, dest)))
}

/// The element type of a tensor-typed MIR type — the buffer element type the
/// contract-shaped subchain signature needs (mirrors the ABI's
/// `buffer_element_type` for the decomposition seam; typed facts only).
fn tensor_element_ty(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<MirType, PlanMappingError> {
    let Type::Tensor(element, _) = validation.types.get(ty.semantic_id()) else {
        return Err(PlanMappingError::mapping(
            "recipe operand requires a tensor type",
        ));
    };
    Ok(MirType::semantic(*element))
}

/// The buffer element type and element count for a data-flow input bound by
/// [`subchain_signature_for_emission`] beyond the recipe operands.
///
/// Tensors bind at their device-view element type and literal dim product
/// ([`tensor_element_ty`] / [`crate::tensor_type::tensor_literal_dims`]). Kernel
/// scalars bind as one element at their own type, matching the ABI's
/// `F32`/`I32`/`U32` scalar surface
/// ([`crate::tensor_type::is_kernel_element_layout`]).
///
/// An f64 scalar stays fail-closed with the ABI's `kernel scalar layout f64`
/// diagnostic: graph shape proves nothing about an individual scalar's value,
/// so it cannot justify f64→f32 narrowing. Other non-scalar, non-tensor layouts
/// also stay fail-closed.
pub(crate) fn data_flow_element_ty_and_count(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<(MirType, u64), PlanMappingError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| PlanMappingError::mapping(error.message))?;
    match layout.kind {
        MirLayoutKind::Scalar(scalar) if crate::tensor_type::is_kernel_element_layout(scalar) => {
            Ok((ty, 1))
        }
        MirLayoutKind::Scalar(scalar) => Err(PlanMappingError::mapping(format!(
            "kernel scalar layout {}",
            scalar.spelling()
        ))),
        MirLayoutKind::DeviceView(_) => {
            let element_ty = tensor_element_ty(ty, validation)?;
            let count = crate::tensor_type::tensor_literal_dims(ty, validation)
                .map_err(PlanMappingError::mapping)?
                .into_iter()
                .fold(1u64, u64::saturating_mul);
            Ok((element_ty, count))
        }
        _ => Err(PlanMappingError::mapping(
            "data-flow input requires a tensor or kernel-scalar type",
        )),
    }
}

/// Resolve a data-flow buffer's kernel element type and extent, preserving a
/// run-time-shaped tensor as the launch-time marker while keeping scalar and
/// unsupported-layout handling identical to the literal-only reader.
pub(crate) fn data_flow_element_ty_and_count_or_runtime(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<(MirType, u64), PlanMappingError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| PlanMappingError::mapping(error.message))?;
    match layout.kind {
        MirLayoutKind::Scalar(scalar) if crate::tensor_type::is_kernel_element_layout(scalar) => {
            Ok((ty, 1))
        }
        MirLayoutKind::Scalar(scalar) => Err(PlanMappingError::mapping(format!(
            "kernel scalar layout {}",
            scalar.spelling()
        ))),
        MirLayoutKind::DeviceView(_) => {
            let element_ty = tensor_element_ty(ty, validation)?;
            let dims = crate::tensor_type::tensor_dims(ty, validation)
                .map_err(PlanMappingError::mapping)?;
            let count = match crate::tensor_type::dims_product_checked(&dims) {
                Ok(count) => count,
                Err(crate::tensor_type::NON_LITERAL_DIM_MESSAGE) => {
                    crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT
                }
                Err(message) => return Err(PlanMappingError::mapping(message)),
            };
            Ok((element_ty, count))
        }
        _ => Err(PlanMappingError::mapping(
            "data-flow input requires a tensor or kernel-scalar type",
        )),
    }
}

/// The local type of a recipe-op statement's first operand (the tensor the
/// reduction/transpose recipe reads) — resolved from the subchain body's
/// typed statement scan.
fn recipe_operand_ty(
    function: &MirFunction,
    op: MirCollectionOp,
) -> Result<MirType, PlanMappingError> {
    let Some((call, _)) = first_collection_call(function, |found| found == op) else {
        return Err(PlanMappingError::mapping(format!(
            "recipe op {op:?} statement not found in the subchain body"
        )));
    };
    call.args
        .first()
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
        .ok_or_else(|| {
            PlanMappingError::mapping(format!("{op:?} recipe operand has no local type"))
        })
}

fn storage_input(binding: u32, element_ty: MirType, count: u64) -> MirKernelResource {
    MirKernelResource {
        group: 0,
        binding,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty,
        element_count: count,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    }
}

fn storage_output(binding: u32, element_ty: MirType, count: u64) -> MirKernelResource {
    MirKernelResource {
        group: 0,
        binding,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Output,
        access: MirKernelResourceAccess::Write,
        element_ty,
        element_count: count,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    }
}

/// Build the contract-shaped storage-buffer signature a recipe subchain
/// carries (S5-U1 decomposition driver seam).
///
/// The full ABI synthesis (`MirKernelSignature::storage_buffer_kernel`)
/// cannot build a signature for an arbitrary subchain slice — a rectangular
/// transpose kernel whose `[M,N] → [N,M]` return is not semantically equal
/// to its param, or a matmul subchain whose operands are locals produced by
/// an earlier subchain — so the seam reconstructs exactly the contract facts
/// the plan pass requires: the recipe operands as read inputs at their
/// device-view element counts and the output buffer sized from the resolved
/// dispatch contract (reduction partials / matmul M·N / transpose M·N),
/// with the corrected workgroup/dispatch [`CollectionOpContract`] applies.
/// [`from_contract`](crate::kernel_plan::CollectionKernelPlan::from_contract)
/// checks the subchain signature exactly as it does a whole-function kernel.
///
/// Extra data-flow inputs (locals read from earlier subchains) are NOT part
/// of this signature: they are a device-program buffer concern, not a plan
/// concern — the plan is decided by the recipe op and its operands alone.
///
/// # Errors
/// Returns [`PlanMappingError`] when the recipe operand types cannot be
/// resolved from the subchain body.
pub fn contract_shape_signature(
    function: &MirFunction,
    contract: &CollectionOpContract,
    validation: &MirValidationContext<'_>,
) -> Result<MirKernelSignature, PlanMappingError> {
    let mut signature = MirKernelSignature {
        function: function.id,
        input: storage_input(0, MirType::semantic(TypeId(0)), 0),
        extra_inputs: Vec::new(),
        output: storage_output(1, MirType::semantic(TypeId(0)), 0),
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 0, y: 1, z: 1 },
    };
    match *contract {
        CollectionOpContract::Reduction {
            length,
            workgroup_x,
            partials,
        } => {
            let mut reduce_op = MirCollectionOp::Sum;
            find_collection_call(function, |op, _, _| {
                match op {
                    MirCollectionOp::TensorMean => reduce_op = MirCollectionOp::TensorMean,
                    MirCollectionOp::TensorMax if reduce_op != MirCollectionOp::TensorMean => {
                        reduce_op = MirCollectionOp::TensorMax;
                    }
                    MirCollectionOp::TensorMin
                        if !matches!(
                            reduce_op,
                            MirCollectionOp::TensorMean | MirCollectionOp::TensorMax
                        ) =>
                    {
                        reduce_op = MirCollectionOp::TensorMin;
                    }
                    _ => {}
                }
                None::<()>
            });
            let operand_ty = recipe_operand_ty(function, reduce_op)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            signature.input = storage_input(0, element_ty, length);
            signature.output = storage_output(1, element_ty, partials);
            signature.output.source_local = recipe_destination_local_for(function, reduce_op);
            signature.workgroup = MirWorkgroupSize {
                x: workgroup_x,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: length,
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::MatMul {
            dispatch:
                MatMulDispatch::Literal {
                    dispatch_x,
                    dispatch_y,
                },
        } => {
            let shapes = kernel_matmul_shapes(function, validation)?.ok_or_else(|| {
                PlanMappingError::mapping("matmul subchain has no operand shapes")
            })?;
            let (left_ty, right_ty) = matmul_operand_tys(function)?;
            let element_ty = tensor_element_ty(left_ty, validation)?;
            // U1 head-axis: the signature describes the buffers the launch
            // binds — the operands' FULL tensor extents (`batch·M·K` left,
            // `right_batch·K·N` right, `batch·M·N` output), never the bare
            // rank-2 dims. `from_contract` re-derives the batch counts
            // from exactly these extents.
            signature.input = storage_input(
                0,
                element_ty,
                device_view_element_count(left_ty, validation)?,
            );
            signature.extra_inputs.push(storage_input(
                1,
                element_ty,
                device_view_element_count(right_ty, validation)?,
            ));
            signature.output = storage_output(
                2,
                element_ty,
                signature.input.element_count / shapes.k * shapes.n,
            );
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorMatMul);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::MATMUL_TILE,
                y: crate::kernel_plan::MATMUL_TILE,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: dispatch_x,
                y: dispatch_y,
                z: 1,
            };
        }
        CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime,
        } => {
            // Run-time extents (design note item 4): operand and output
            // buffers are runtime-shaped; M, K and N travel as
            // `TensorAxisExtent` resources appended after the output. The
            // grid comes from the call-site shapes at launch, so
            // `dispatch_size` is left untouched.
            let (left_ty, right_ty) = matmul_operand_tys(function)?;
            let return_ty = matmul_return_ty(function)?;
            let element_ty = tensor_element_ty(left_ty, validation)?;
            let left_dims = crate::tensor_type::tensor_dims(left_ty, validation)
                .map_err(PlanMappingError::mapping)?;
            let right_dims = crate::tensor_type::tensor_dims(right_ty, validation)
                .map_err(PlanMappingError::mapping)?;
            // Output is the full output tensor extent read from the DECLARED
            // result: every left axis but the last, times the right's last
            // axis — `[B, M, N]` for a batched left, `[M, N]` (or `[M]`) for a
            // rank-2 left. A run-time axis anywhere in that product binds at
            // launch, so the buffer takes the runtime marker; a zero literal
            // axis or an overflowing literal product is refused instead of
            // storing a saturated count. The contract resolver enforces the
            // same ranks and prefix law (rank-2 left, or the batched
            // rank-2..4 form).
            signature.input = storage_input(
                0,
                element_ty,
                device_view_element_count_or_runtime(left_ty, validation)?,
            );
            signature.extra_inputs.push(storage_input(
                1,
                element_ty,
                device_view_element_count_or_runtime(right_ty, validation)?,
            ));
            signature.output = storage_output(
                2,
                element_ty,
                device_view_element_count_or_runtime(return_ty, validation)?,
            );
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorMatMul);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::MATMUL_TILE,
                y: crate::kernel_plan::MATMUL_TILE,
                z: 1,
            };
            crate::abi::append_matmul_runtime_extent_resources(
                &mut signature,
                &left_dims,
                &right_dims,
                validation,
            )?;
        }
        CollectionOpContract::Transpose { m, n } => {
            let operand_ty = recipe_operand_ty(function, MirCollectionOp::TensorTranspose)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            signature.input = storage_input(0, element_ty, m.saturating_mul(n));
            signature.output = storage_output(1, element_ty, m.saturating_mul(n));
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorTranspose);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: m.saturating_mul(n),
                y: 1,
                z: 1,
            };
        }
        // U3 head-axis: the signature describes the buffers the launch
        // binds — the batched operand's FULL `batch·M·N` extent, never the
        // bare per-batch dims. `from_contract` re-derives the batch count
        // from exactly this extent.
        CollectionOpContract::BatchedTranspose { batch, m, n } => {
            let operand_ty = recipe_operand_ty(function, MirCollectionOp::TensorTranspose)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            let count = batch.saturating_mul(m).saturating_mul(n);
            signature.input = storage_input(0, element_ty, count);
            signature.output = storage_output(1, element_ty, count);
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorTranspose);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: count,
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::QuantizedMatMul {
            dispatch_x,
            dispatch_y,
            m,
            k,
            n,
            ..
        } => {
            let left_ty = recipe_operand_ty(function, MirCollectionOp::TensorQuantizedMatMul)?;
            let element_ty = tensor_element_ty(left_ty, validation)?;
            signature.input = storage_input(0, element_ty, m.saturating_mul(k));
            signature
                .extra_inputs
                .push(storage_input(1, element_ty, k.saturating_mul(n)));
            signature.output = storage_output(2, element_ty, m.saturating_mul(n));
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorQuantizedMatMul);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::MATMUL_TILE,
                y: crate::kernel_plan::MATMUL_TILE,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: dispatch_x,
                y: dispatch_y,
                z: 1,
            };
        }
        CollectionOpContract::GroupedMatMul {
            dispatch_x,
            dispatch_y,
            m,
            k,
            n,
            groups,
            ..
        } => {
            let left_ty = recipe_operand_ty(function, MirCollectionOp::TensorGroupedMatMul)?;
            let element_ty = tensor_element_ty(left_ty, validation)?;
            signature.input = storage_input(0, element_ty, m.saturating_mul(k));
            signature.extra_inputs.push(storage_input(
                1,
                element_ty,
                groups.saturating_mul(k).saturating_mul(n),
            ));
            signature.output = storage_output(2, element_ty, m.saturating_mul(n));
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorGroupedMatMul);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::MATMUL_TILE,
                y: crate::kernel_plan::MATMUL_TILE,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: dispatch_x,
                y: dispatch_y,
                z: 1,
            };
        }
        // PGC-R5 (verdict 91fe3d59): the row-reduction family. Resources
        // are the SAME facts the transformer seam builds (input at the
        // flattened dims, RMSNorm's gamma as the typed extra input with the
        // output at binding 2, output at the flattened dims); only the
        // launch geometry is the row-reduction contract (one workgroup per
        // row). A mixed subchain (additional consumed outputs) keeps the
        // generic 1D dispatch — a rows grid would under-cover the extra
        // output writes (the R6 P1 union-dispatch law).
        CollectionOpContract::RowReduction { rows, width } => {
            let op = find_collection_call(function, |op, _, _| {
                (op == MirCollectionOp::TensorRmsNorm).then_some(op)
            })
            .unwrap_or(MirCollectionOp::TensorCausalMaskedSoftmax);
            let operand_ty = recipe_operand_ty(function, op)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            let extent = rows.saturating_mul(width);
            signature.input = storage_input(0, element_ty, extent);
            signature.output = storage_output(1, element_ty, extent);
            signature.output.source_local = recipe_destination_local_for(function, op);
            if op == MirCollectionOp::TensorRmsNorm {
                let gamma_local = rms_norm_gamma_local(function).ok_or_else(|| {
                    PlanMappingError::mapping(
                        "TensorRmsNorm gamma peer has no local in the subchain body",
                    )
                })?;
                let gamma_ty = function_local_ty(function, gamma_local)?;
                let (gamma_element_ty, gamma_count) =
                    data_flow_element_ty_and_count(gamma_ty, validation)?;
                signature.extra_inputs.push(storage_input_with_local(
                    1,
                    gamma_element_ty,
                    gamma_count,
                    gamma_local,
                ));
                signature.output.binding = 2;
            }
            let lanes = u64::from(crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            // 1D dispatch law: threads (rows workgroups × lanes).
            signature.dispatch_size = MirKernelDispatchSize {
                x: rows.saturating_mul(lanes),
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::BatchedAttention {
            kv_heads,
            seq,
            head_dim,
            output_elements,
            ..
        } => {
            let op = MirCollectionOp::TensorBatchedAttention;
            // Whole functions carry ABI-only output/dispatch params, while a
            // decomposed recipe subchain retains only the tensor locals read
            // by the batched call. Resolve the call operands directly so both
            // forms preserve every projection peer (cos/sin/bias included).
            let operands = recipe_operand_locals(function)?;
            let first = operands.first().ok_or_else(|| {
                PlanMappingError::mapping(format!(
                    "batched attention function {} has no tensor operands",
                    function.id.0
                ))
            })?;
            let (element_ty, _) =
                data_flow_element_ty_and_count(function_local_ty(function, *first)?, validation)?;
            for (binding, local) in operands.iter().copied().enumerate() {
                let ty = function_local_ty(function, local)?;
                let (operand_ty, extent) = data_flow_element_ty_and_count(ty, validation)?;
                if binding == 0 {
                    signature.input = storage_input_with_local(0, operand_ty, extent, local);
                } else {
                    let mut extra = storage_input_with_local(
                        u32::try_from(binding).unwrap_or(u32::MAX),
                        operand_ty,
                        extent,
                        local,
                    );
                    // Persistent K/V views are written in-shader at
                    // write_position. Mark them ReadWrite so Metal binds
                    // `device` rather than `device const`.
                    if let Ok(dims) = crate::tensor_type::tensor_literal_dims(ty, validation)
                        && dims.len() == 3
                        && dims[0] == kv_heads
                        && dims[2] == head_dim
                        && dims[1] != seq
                    {
                        extra.access = MirKernelResourceAccess::ReadWrite;
                    }
                    signature.extra_inputs.push(extra);
                }
            }
            signature.output = storage_output(
                u32::try_from(operands.len()).unwrap_or(u32::MAX),
                element_ty,
                output_elements,
            );
            signature.output.source_local = recipe_destination_local_for(function, op);
            signature.workgroup = MirWorkgroupSize { x: 64, y: 1, z: 1 };
            signature.dispatch_size = MirKernelDispatchSize {
                x: output_elements,
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::SsmConv1d {
            length, channels, ..
        } => {
            let operand_ty = recipe_operand_ty(function, MirCollectionOp::TensorSsmConv1d)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            let extent = length.saturating_mul(channels);
            signature.input = storage_input(0, element_ty, extent);
            signature.output = storage_output(1, element_ty, extent);
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorSsmConv1d);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: extent,
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::SsmScan {
            length, state_dim, ..
        } => {
            let operand_ty = recipe_operand_ty(function, MirCollectionOp::TensorSsmScan)?;
            let element_ty = tensor_element_ty(operand_ty, validation)?;
            let extent = length.saturating_mul(state_dim);
            signature.input = storage_input(0, element_ty, extent);
            signature.output = storage_output(1, element_ty, extent);
            signature.output.source_local =
                recipe_destination_local_for(function, MirCollectionOp::TensorSsmScan);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: extent,
                y: 1,
                z: 1,
            };
        }
    }
    Ok(signature)
}

/// Build the transformer-shaped storage-buffer signature a transformer
/// recipe subchain carries (S6-P1 seam, the sibling of
/// [`contract_shape_signature`]).
///
/// The transformer recipes have no `CollectionOpContract` variant, so the
/// plan pass cannot resolve one; the seam reconstructs exactly the facts the
/// plan resolution requires: the recipe operand as the read input at its
/// device-view element count (the flattened input dims), any typed affine or
/// index peer as an extra read input, and the output buffer at the flattened
/// output dims (input dims for the same-shape softmax/LN recipes; input dims
/// minus the reduced axis for `TensorSumAxis`), under the generic 1D contract
/// the ABI applies to these kernels. The element-count cross-checks in
/// [`from_transformer_op`](crate::kernel_plan::CollectionKernelPlan::from_transformer_op)
/// then pass exactly as they do for a whole-function kernel.
///
/// # Errors
/// Returns [`PlanMappingError`] when the transformer recipe's typed shape
/// facts (dims/axis/const constraints) or the operand tensor type cannot be
/// resolved from the subchain body.
pub fn transformer_shape_signature(
    function: &MirFunction,
    op: MirCollectionOp,
    validation: &MirValidationContext<'_>,
) -> Result<MirKernelSignature, PlanMappingError> {
    let (dims, axis) = if op == MirCollectionOp::TensorGather {
        // Gather plan facts support run-time dimensions; the signature uses
        // the typed table shape directly instead of the literal-only recipe
        // shape helper shared by the other transformer recipes.
        let operand_local = transformer_operand_local(function, op).ok_or_else(|| {
            PlanMappingError::mapping(
                "TensorGather operand has no local in the transformer signature",
            )
        })?;
        let operand_ty = crate::local_ty::local_ty(function, operand_local)
            .ok_or_else(|| PlanMappingError::mapping("TensorGather operand has no local type"))?;
        let table_dims = crate::tensor_type::tensor_dims(operand_ty, validation)
            .map_err(PlanMappingError::mapping)?;
        if table_dims.len() != 2 {
            return Err(PlanMappingError::mapping(
                "TensorGather table must have rank two",
            ));
        }
        (Vec::new(), 0)
    } else {
        crate::kernel_plan::transformer_recipe_shape(function, validation, op).ok_or_else(|| {
            PlanMappingError::mapping(format!(
                "transformer recipe {op:?} has no resolvable shape facts \
                 (axis/epsilon from the const operands, dims from the tensor types)"
            ))
        })?
    };
    let operand_local = transformer_operand_local(function, op).ok_or_else(|| {
        PlanMappingError::mapping(format!(
            "transformer recipe {op:?} operand has no local in the subchain body"
        ))
    })?;
    let operand_ty = crate::local_ty::local_ty(function, operand_local).ok_or_else(|| {
        PlanMappingError::mapping(format!(
            "transformer recipe {op:?} operand has no local type"
        ))
    })?;
    let input_count = if op == MirCollectionOp::TensorGather {
        let dims = crate::tensor_type::tensor_dims(operand_ty, validation)
            .map_err(PlanMappingError::mapping)?;
        runtime_capable_dim_product(&dims)
    } else {
        dims.iter().copied().fold(1u64, u64::saturating_mul)
    };
    let output_count = if op == MirCollectionOp::TensorSumAxis {
        dims.iter()
            .enumerate()
            .filter(|(index, _)| *index as u64 != axis)
            .fold(1u64, |acc, (_, &dim)| acc.saturating_mul(dim))
    } else if op == MirCollectionOp::TensorGather {
        // GI3-1: the row-gather result shape `[T, D]` is typed on the
        // statement; a run-time axis carries the launch-shaped marker.
        gather_return_count(function, validation)?
    } else {
        input_count
    };
    let element_ty = tensor_element_ty(operand_ty, validation)?;
    let mut signature = MirKernelSignature {
        function: function.id,
        input: storage_input(0, element_ty, input_count),
        extra_inputs: Vec::new(),
        output: storage_output(1, element_ty, output_count),
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        // The generic 1D contract the ABI applies to kernels without a
        // collection-op contract (the transformer recipes have none); the
        // emitter units (S6-P2/P3) own the row-wise thread mapping under it.
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: output_count,
            y: 1,
            z: 1,
        },
    };
    signature.input.source_local = Some(operand_local);
    signature.output.source_local = recipe_destination_local_for(function, op);
    // RMSNorm's affine gamma is a kernel resource, not a carried plan field.
    // Bind the typed rank-1 peer here so both the plan/emitter lookup and the
    // decomposition seam see the same stable input ordering: x, gamma, out.
    if op == MirCollectionOp::TensorRmsNorm {
        bind_rms_norm_affine_resources(&mut signature, function, validation)?;
    }
    // RoPE's interleaved angle table is a caller-owned kernel resource. Bind
    // the exact fourth MIR operand; the planner and emitters must never scan
    // unrelated tensor parameters to discover a table.
    if op.is_rope() {
        let table_local = rope_table_local(function, op).ok_or_else(|| {
            PlanMappingError::mapping(
                "TensorRope table operand has no local in the transformer signature",
            )
        })?;
        let table_ty = function_local_ty(function, table_local)?;
        let (table_element_ty, table_count) = data_flow_element_ty_and_count(table_ty, validation)?;
        signature.extra_inputs.push(storage_input_with_local(
            1,
            table_element_ty,
            table_count,
            table_local,
        ));
        signature.output.binding = 2;
    }
    // Council-8 CB-3: the axis-reduction producer stamps its
    // producer-defined reduced-resource projection on the reduced OUTPUT
    // resource — the mapping a keep-dims consumer uses to address the
    // reduced buffer. The fact is carried (plan → resource → wire version),
    // never reconstructed from element count by a consumer.
    if op == MirCollectionOp::TensorSumAxis {
        signature.output.reduced_projection =
            crate::kernel_plan::axis_reduction_projection(&dims, axis);
    }
    // GI3-1: the row-gather recipe binds its id vector as the signature's
    // first extra input (binding 1, the kernel-index element type), sized
    // from the typed return dims; a run-time row axis uses the launch-shaped
    // marker. The output moves to binding 2. Gather values remain run-time
    // tokens, never inferred from source text (A3), and the emitted body
    // bounds-checks each token against the table row count (SEC-G1).
    if op == MirCollectionOp::TensorGather {
        let ids_local = gather_ids_local(function).ok_or_else(|| {
            PlanMappingError::mapping(
                "row-gather recipe ids operand has no local in the subchain body",
            )
        })?;
        let ids_ty = crate::local_ty::local_ty(function, ids_local).ok_or_else(|| {
            PlanMappingError::mapping("row-gather recipe ids operand has no local type")
        })?;
        let id_count = gather_id_count(function, validation)?;
        let id_element_ty = gather_ids_element_ty(ids_ty, validation)?;
        let ids_dims = crate::tensor_type::tensor_dims(ids_ty, validation)
            .map_err(PlanMappingError::mapping)?;
        let ids_input = if ids_dims
            .iter()
            .any(|dim| matches!(dim, crate::tensor_type::Dim::Runtime))
        {
            storage_input_with_local(1, id_element_ty, id_count, ids_local)
        } else {
            storage_input(1, id_element_ty, id_count)
        };
        signature.extra_inputs.push(ids_input);
        signature.output.binding = 2;
    }
    Ok(signature)
}

fn bind_rms_norm_affine_resources(
    signature: &mut MirKernelSignature,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<(), PlanMappingError> {
    let epsilon_local = rms_norm_epsilon_local(function);
    if let Some(epsilon_local) = epsilon_local {
        let epsilon_ty = function_local_ty(function, epsilon_local)?;
        let (epsilon_element_ty, epsilon_count) =
            data_flow_element_ty_and_count(epsilon_ty, validation)?;
        signature.extra_inputs.push(storage_input_with_local(
            1,
            epsilon_element_ty,
            epsilon_count,
            epsilon_local,
        ));
    }
    let gamma_local = rms_norm_gamma_local(function).ok_or_else(|| {
        PlanMappingError::mapping(
            "TensorRmsNorm gamma peer has no local in the transformer signature",
        )
    })?;
    let gamma_ty = function_local_ty(function, gamma_local)?;
    let (gamma_element_ty, gamma_count) = data_flow_element_ty_and_count(gamma_ty, validation)?;
    let gamma_binding = if epsilon_local.is_some() { 2 } else { 1 };
    signature.extra_inputs.push(storage_input_with_local(
        gamma_binding,
        gamma_element_ty,
        gamma_count,
        gamma_local,
    ));
    signature.output.binding = gamma_binding + 1;
    Ok(())
}

/// The product of typed dimensions for a device resource. Any launch-bound
/// axis makes its flattened count run-time shaped.
fn runtime_capable_dim_product(dims: &[crate::tensor_type::Dim]) -> u64 {
    if dims
        .iter()
        .any(|dim| matches!(dim, crate::tensor_type::Dim::Runtime))
    {
        return crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;
    }
    dims.iter().fold(1u64, |product, dim| match dim {
        crate::tensor_type::Dim::Literal(count) => product.saturating_mul(*count),
        crate::tensor_type::Dim::Runtime => product,
    })
}

/// The row-gather return element count (GI3-1): the product of the gather
/// statement's typed `[T, D]` dims, or the run-time marker when an axis is
/// bound at launch.
fn gather_return_count(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<u64, PlanMappingError> {
    let dims = gather_statement_return_dims(function, validation)?;
    Ok(runtime_capable_dim_product(&dims))
}

/// The typed dims of the gather statement's return type.
fn gather_statement_return_dims(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Vec<crate::tensor_type::Dim>, PlanMappingError> {
    let Some((call, _)) = first_collection_call(function, |op| op == MirCollectionOp::TensorGather)
    else {
        return Err(PlanMappingError::mapping(
            "row-gather statement not found in the subchain body",
        ));
    };
    crate::tensor_type::tensor_dims(call.return_ty, validation).map_err(PlanMappingError::mapping)
}

/// The row-gather id-vector length (GI3-1): the return dims' first element
/// (`[T, D]`), including its run-time-shaped marker when T is dynamic.
fn gather_id_count(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<u64, PlanMappingError> {
    let dims = gather_statement_return_dims(function, validation)?;
    match dims.first() {
        Some(crate::tensor_type::Dim::Literal(count)) => Ok(*count),
        Some(crate::tensor_type::Dim::Runtime) => Ok(crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT),
        None => Err(PlanMappingError::mapping(
            "row-gather return shape has no row dimension",
        )),
    }
}

/// The id vector's device element type (GI3-1, K7c): the ids are a rank-1
/// `tensor<u32, [n]>`, whose element is already the kernel `numerus<u32>`
/// index layout (the ABI's index surface). The ids' values are runtime
/// tokens; the emitted body bounds-checks each against the table row count
/// (SEC-G1). Any other ids operand (a `lista`, another element type) stays
/// fail-closed with a typed diagnostic.
fn gather_ids_element_ty(
    ids_ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<MirType, PlanMappingError> {
    let Type::Tensor(element, _) = validation.types.get(ids_ty.semantic_id()) else {
        return Err(PlanMappingError::mapping(
            "row-gather ids operand must be a tensor<u32, [n]>",
        ));
    };
    if !matches!(
        validation.types.get(*element),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
    ) {
        return Err(PlanMappingError::mapping(
            "row-gather ids operand must be a tensor<u32, [n]>",
        ));
    }
    Ok(MirType::semantic(*element))
}

/// The ids operand local of the gather statement (the second argument — the
/// first is the table).
fn gather_ids_local(function: &MirFunction) -> Option<MirLocalId> {
    let (call, _) = first_collection_call(function, |op| op == MirCollectionOp::TensorGather)?;
    call.args
        .get(1)
        .and_then(crate::read_analysis::operand_local)
}

/// The recipe operand local of a transformer recipe statement — the first
/// TENSOR arg (const operands like axis/epsilon are plan facts, never
/// buffers). `None` when the op statement is absent or carries no tensor
/// operand.
fn transformer_operand_local(function: &MirFunction, op: MirCollectionOp) -> Option<MirLocalId> {
    let (call, _) = first_collection_call(function, |found| found == op)?;
    call.args
        .iter()
        .find_map(crate::read_analysis::operand_local)
}

/// The `RMSNorm` affine gamma peer is the fourth runtime-call argument after
/// the tensor operand and its two const plan facts. It is data, not a plan
/// field, so the source/signature seam binds its local as a read resource.
fn rms_norm_gamma_local(function: &MirFunction) -> Option<MirLocalId> {
    let (call, _) = first_collection_call(function, |op| op == MirCollectionOp::TensorRmsNorm)?;
    call.args
        .get(3)
        .and_then(crate::read_analysis::operand_local)
}

/// The explicit `RMSNorm` epsilon operand when it is a kernel parameter. A
/// literal epsilon is specialized into the recipe plan and has no resource.
fn rms_norm_epsilon_local(function: &MirFunction) -> Option<MirLocalId> {
    let (call, _) = first_collection_call(function, |op| op == MirCollectionOp::TensorRmsNorm)?;
    match call.args.get(2)? {
        MirOperand::Place(place) if place.projections.is_empty() => match place.base {
            MirPlaceBase::Local(local) => Some(local),
            MirPlaceBase::Temp(_) => None,
        },
        _ => None,
    }
}

/// The explicit caller-provided interleaved `RoPE` table operand (the fourth
/// runtime-call argument after `[x, pos, dim]`).
pub(super) fn rope_table_local(function: &MirFunction, op: MirCollectionOp) -> Option<MirLocalId> {
    let (call, _) = first_collection_call(function, |found| found == op)?;
    call.args
        .get(3)
        .and_then(crate::read_analysis::operand_local)
}

/// The emission signature for a decomposition subchain kernel (S5-U1b
/// emitter seam): the contract-shaped recipe resources plus the subchain's
/// data-flow inputs as read inputs, with the recipe operands' source locals
/// patched so the emitters' input-buffer wiring resolves.
///
/// The plan pass resolves a subchain's plan from the contract-shaped
/// signature alone ([`contract_shape_signature`] — extra data-flow inputs
/// are a buffer concern, not a plan concern). The emitters additionally need
/// every read input bound: the recipe operands at the contract bindings
/// (input at 0, matmul's right operand at 1), then each remaining data-flow
/// input (a local read from an earlier subchain, e.g. the elementwise
/// operand folded into a reduction subchain's load) as a read input at the
/// next binding, and the contract output buffer last.
///
/// Data-flow inputs bind after the recipe operands: tensor-typed params at
/// their device-view element type and literal dim product, and scalar-typed
/// params (S5-W-A1: the companion's scalar upstream seed, e.g. the MLP
/// backward's f32 seed flowing into a Fill/Div) as 1-element inputs at the
/// scalar's own type — mirroring the ABI kernel scalar surface
/// (`F32`/`I32`/`U32`). An f64-class scalar stays fail-closed exactly as
/// the ABI rejects it; other non-scalar, non-tensor layouts stay fail-closed
/// with a typed diagnostic.
///
/// D-2b-1: `outputs` is the subchain's consumed-output set (D-2a), the
/// locals the synthetic exposes as kernel outputs. The first element is the
/// primary (the contract-shaped [`MirKernelSignature::output`]); each
/// ADDITIONAL output (`outputs[1..]`, the consumed elementwise destinations
/// a later subchain reads) binds as a [`MirKernelSignature::extra_outputs`] entry
/// after the primary, sized from its local's type exactly like the input
/// data-flow resolution (device-view element type + literal dim product).
/// `outputs.len() == 1` keeps the existing single-output shape.
///
/// # Errors
/// Returns [`PlanMappingError`] when the recipe operand's tensor type, a
/// data-flow input's tensor type, a scalar data-flow input's kernel layout,
/// or an additional output local's type cannot be resolved from the subchain
/// body.
pub fn subchain_signature_for_emission(
    function: &MirFunction,
    contract: &CollectionOpContract,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
) -> Result<MirKernelSignature, PlanMappingError> {
    subchain_signature_for_emission_with_source(function, contract, validation, outputs, None)
}

/// The source-aware form of [`subchain_signature_for_emission`] (widened
/// d8518267 seam repair): `source` is the ORIGINAL function the subchain was
/// decomposed from. The data-flow count resolution uses it to bind a
/// `TensorSumAxis`-produced input at the REDUCED count — the same count the
/// producing subchain's seam output carries — so the emitters' and the faber
/// constructor's cross-kernel unification see one shape per local
/// (`carried_resources_agree`). `None` keeps the type-derived count (whole-
/// function and test call sites).
///
/// # Errors
/// Returns [`PlanMappingError`] when the recipe operand's tensor type, a
/// data-flow input's tensor type, a scalar data-flow input's kernel layout,
/// or an additional output local's type cannot be resolved from the subchain
/// body (same failure modes as [`subchain_signature_for_emission`]).
pub fn subchain_signature_for_emission_with_source(
    function: &MirFunction,
    contract: &CollectionOpContract,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
    source: Option<&MirFunction>,
) -> Result<MirKernelSignature, PlanMappingError> {
    let mut signature = contract_shape_signature(function, contract, validation)?;
    let operands = recipe_operand_locals(function)?;
    for (binding, local) in operands.iter().copied().enumerate() {
        if binding == 0 {
            signature.input.source_local = Some(local);
        } else if let Some(extra) = signature.extra_inputs.get_mut(binding - 1) {
            extra.source_local = Some(local);
        }
    }
    finish_subchain_emission_signature(
        function,
        validation,
        outputs,
        &mut signature,
        &operands,
        source,
    )?;
    Ok(signature)
}

/// The emission signature for the decomposition subchain that stores into the
/// source function's explicit output parameter ([`crate::kernel_decomposition::KernelSubchain::output_param`]
/// `Some`).
///
/// That subchain is the indexed-view kernel the source is: its synthetic
/// declares the slice's inputs, the output parameter `y` and the source's
/// trailing index parameters, so the whole-function ABI synthesis binds `y` as
/// the output at its own position and the index as the builtin. The
/// contract-shaped subchain signature
/// ([`subchain_signature_for_emission_with_source`]) cannot: it binds the
/// recipe result as the output, so the final store to `y` and the fused tail
/// behind the recipe are never emitted and the kernel writes a dead local.
///
/// The whole-function ABI lays the inputs out in the synthetic's parameter
/// order (the slice's read order) and the emitters take the first two as the
/// recipe's operands, so the recipe operands must lead that order; a body
/// whose folded elementwise prefix reads something first fails closed.
///
/// # Errors
/// Returns [`PlanMappingError`] when the ABI synthesis refuses the synthetic,
/// when its output is not the explicit output parameter, or when the recipe
/// operands do not lead the bound inputs.
pub fn explicit_output_subchain_signature(
    function: &MirFunction,
    output_param: MirLocalId,
    validation: &MirValidationContext<'_>,
    interner: &Interner,
) -> Result<MirKernelSignature, PlanMappingError> {
    let signature = MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
        function, validation, interner,
    )
    .map_err(|error| PlanMappingError::mapping(error.message))?;
    if signature.output.source_local != Some(output_param) {
        return Err(PlanMappingError::mapping(format!(
            "subchain ABI synthesis did not bind the output parameter local {} as its output",
            output_param.0
        )));
    }
    if let Ok(operands) = recipe_operand_locals(function) {
        let inputs = std::iter::once(&signature.input).chain(signature.extra_inputs.iter());
        for (position, (operand, input)) in operands.iter().zip(inputs).enumerate() {
            if input.source_local != Some(*operand) {
                return Err(PlanMappingError::mapping(format!(
                    "subchain storing to its output parameter binds input {position} to a \
                     different local than recipe operand local {}; the recipe operands must \
                     lead the subchain's inputs",
                    operand.0
                )));
            }
        }
    }
    Ok(signature)
}

/// Whether a subchain's data-flow surface carries a run-time-shaped buffer:
/// any non-vacuum param type or any outputs local type whose tensor shape has
/// a run-time extent ([`crate::tensor_type::Dim::Runtime`]). Callers use this
/// to pick the runtime-aware [`elementwise_subchain_signature_for_emission`]
/// over the literal-only ABI synthesis, whose buffer element counts cannot
/// carry a run-time extent. A param/local type that is not a tensor (e.g. a
/// kernel scalar) has no shape axes and never reads as runtime-shaped here —
/// the signature builders stay the fail-closed authority on such types.
pub fn subchain_has_runtime_shaped_buffer(
    function: &MirFunction,
    outputs: &[MirLocalId],
    validation: &MirValidationContext<'_>,
) -> bool {
    fn shape_has_runtime_dim(ty: MirType, validation: &MirValidationContext<'_>) -> bool {
        crate::tensor_type::tensor_dims(ty, validation).is_ok_and(|dims| {
            dims.iter()
                .any(|dim| matches!(dim, crate::tensor_type::Dim::Runtime))
        })
    }
    function.params.iter().any(|param| {
        !crate::abi::is_vacuum(param.ty, validation) && shape_has_runtime_dim(param.ty, validation)
    }) || outputs.iter().any(|&local| {
        function_local_ty(function, local).is_ok_and(|ty| shape_has_runtime_dim(ty, validation))
    })
}

/// The emission signature for a decomposed ELEMENTWISE-ONLY subchain: every
/// non-vacuum param binds as a read storage-buffer input in declaration order
/// (the first at binding 0, the rest as extra inputs at the following
/// bindings), then the outputs bind — the primary as the signature output,
/// each additional consumed output as an extra output — all sized by
/// [`data_flow_element_ty_and_count_or_runtime`] so a run-time-shaped tensor
/// binds at the [`crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT`] marker instead
/// of failing the literal-dimension law. A run-time-shaped surface appends one
/// count-one `u32` [`MirKernelResourceKind::RuntimeExtent`] channel after the
/// outputs: the host binds the written tensor's whole element count per call
/// and the emitters' out-of-bounds guard reads it, so the generic launch
/// geometry's rounded-up last workgroup cannot index past the last element.
///
/// Why not [`MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry`]:
/// the whole-function ABI synthesis sizes every buffer with the literal-only
/// `device_view_element_count`, so an elementwise subchain carrying a
/// run-time-shaped intermediate is refused before emission. This builder is
/// the runtime-aware path: the launch geometry (workgroup and dispatch) is a
/// per-call fact for a runtime-shaped kernel — one thread-block of one lane,
/// like the `MatMulDispatch::Runtime` signature — never a literal extent product.
///
/// # Errors
/// Returns [`PlanMappingError`] when `outputs` is empty, or a param/local
/// type is neither a tensor nor a kernel scalar (the same fail-closed law the
/// literal data-flow reader applies).
pub fn elementwise_subchain_signature_for_emission(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
) -> Result<MirKernelSignature, PlanMappingError> {
    if outputs.is_empty() {
        return Err(PlanMappingError::mapping(
            "elementwise subchain signature requires at least one output local",
        ));
    }
    let mut signature = MirKernelSignature {
        function: function.id,
        input: storage_input(0, MirType::semantic(TypeId(0)), 0),
        extra_inputs: Vec::new(),
        output: storage_output(0, MirType::semantic(TypeId(0)), 0),
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: crate::abi::called_builtins_for_function(function),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
    };
    let mut next_binding = 0u32;
    let mut bound_primary_input = false;
    let mut runtime_shaped = false;
    for param in &function.params {
        if crate::abi::is_vacuum(param.ty, validation) {
            continue;
        }
        let (element_ty, count) = data_flow_element_ty_and_count_or_runtime(param.ty, validation)?;
        runtime_shaped |= count == crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;
        let resource = storage_input_with_local(next_binding, element_ty, count, param.local);
        if bound_primary_input {
            signature.extra_inputs.push(resource);
        } else {
            signature.input = resource;
            bound_primary_input = true;
        }
        next_binding = next_binding.saturating_add(1);
    }
    for (index, &local) in outputs.iter().enumerate() {
        let local_ty = function_local_ty(function, local)?;
        let (element_ty, count) = data_flow_element_ty_and_count_or_runtime(local_ty, validation)?;
        runtime_shaped |= count == crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;
        let resource = storage_output_with_local(next_binding, element_ty, count, local);
        if index == 0 {
            signature.output = resource;
        } else {
            signature.extra_outputs.push(resource);
        }
        next_binding = next_binding.saturating_add(1);
    }
    // Run-time launch geometry rounds the per-call count up to whole
    // workgroups, so the body must guard indexed loads and stores with the
    // host-bound count, not the one-lane placeholder dispatch. The count type
    // must already exist in the frozen table; without it the Metal emitter
    // refuses the unbounded surface.
    if runtime_shaped
        && let Some(extent_ty) = validation
            .types
            .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
    {
        signature.extra_inputs.push(runtime_extent_count_resource(
            next_binding,
            MirType::semantic(extent_ty),
        ));
    }
    Ok(signature)
}

/// Append the runtime-extent count channel (U2/L7) to a run-time-shaped
/// elementwise emission signature.
///
/// [`elementwise_subchain_signature_for_emission`] sizes a run-time-shaped
/// buffer at the [`crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT`] marker and
/// keeps the one-lane placeholder dispatch, because the real extent is a
/// per-call launch fact. An emitter whose OOB guard compares the dispatch
/// index against a static limit therefore has no honest limit to use: the
/// placeholder `dispatch_size.x == 1` would cull every invocation past the
/// first element and leave the rest of a multi-element output unwritten.
/// The count channel is the fix — the host binds the written output's whole
/// element count at this slot before dispatch and the guard reads it.
///
/// Returns `true` when the resource was appended. A hand-built type table
/// without `numerus<u32>` (unit fixtures) keeps the signature untouched; the
/// production registry interns it for every kernel program.
pub fn append_runtime_extent_count_channel(
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> bool {
    if !signature
        .resources()
        .any(MirKernelResource::is_runtime_shaped)
    {
        return false;
    }
    let Some(extent_ty) = validation
        .types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
    else {
        return false;
    };
    let binding = signature
        .resources()
        .map(|resource| resource.binding)
        .max()
        .map_or(0, |binding| binding.saturating_add(1));
    signature.extra_inputs.push(runtime_extent_count_resource(
        binding,
        MirType::semantic(extent_ty),
    ));
    true
}

/// The runtime-extent count resource a run-time-shaped emission signature
/// carries: a count-one `u32` read input the host populates with the written
/// output's whole element count before dispatch (the same channel the
/// whole-function ABI appends for a scalar-shaped kernel).
fn runtime_extent_count_resource(binding: u32, element_ty: MirType) -> MirKernelResource {
    MirKernelResource {
        group: 0,
        binding,
        kind: MirKernelResourceKind::RuntimeExtent,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty,
        element_count: 1,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    }
}

/// The emission signature for a decomposition subchain kernel carrying a
/// S6-P1 transformer recipe (S6-P1 seam): the transformer-shaped recipe
/// resources plus the subchain's data-flow inputs, with the recipe
/// operand's source local patched so the emitters' input-buffer wiring
/// resolves. Mirrors [`subchain_signature_for_emission`] for the
/// transformer recipes (no `CollectionOpContract` variant exists, so the
/// base signature comes from [`transformer_shape_signature`]).
///
/// The transformer recipes bind exactly one recipe operand (the tensor the
/// recipe reads). Const operands (axis / epsilon) remain plan facts, never
/// buffers. `RMSNorm`'s mandatory gamma peer is a typed extra read resource;
/// other affine peers remain data-flow inputs when their recipe admits them.
///
/// # Errors
/// Returns [`PlanMappingError`] when the recipe operand's tensor type, a
/// data-flow input's tensor type, a scalar data-flow input's kernel layout,
/// or an additional output local's type cannot be resolved from the subchain
/// body.
pub fn transformer_subchain_signature_for_emission(
    function: &MirFunction,
    op: MirCollectionOp,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
) -> Result<MirKernelSignature, PlanMappingError> {
    transformer_subchain_signature_for_emission_with_source(function, op, validation, outputs, None)
}

/// The source-aware form of [`transformer_subchain_signature_for_emission`]
/// (widened d8518267 seam repair): `source` is the ORIGINAL function the
/// subchain was decomposed from. The data-flow count resolution uses it to
/// bind a `TensorSumAxis`-produced input at the REDUCED count — the same
/// count this seam sizes the producing subchain's primary output at — so
/// the emitters' and the faber constructor's cross-kernel unification see
/// one shape per local (`carried_resources_agree`). `None` keeps the
/// type-derived count (whole-function and test call sites).
///
/// # Errors
/// Returns [`PlanMappingError`] when the recipe operand's tensor type, a
/// data-flow input's tensor type, a scalar data-flow input's kernel layout,
/// or an additional output local's type cannot be resolved from the subchain
/// body (same failure modes as [`transformer_subchain_signature_for_emission`]).
pub fn transformer_subchain_signature_for_emission_with_source(
    function: &MirFunction,
    op: MirCollectionOp,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
    source: Option<&MirFunction>,
) -> Result<MirKernelSignature, PlanMappingError> {
    let mut signature = transformer_shape_signature(function, op, validation)?;
    let mut bound: Vec<MirLocalId> = Vec::new();
    if let Some(operand) = transformer_operand_local(function, op) {
        signature.input.source_local = Some(operand);
        bound.push(operand);
    }
    // GI3-1: the row-gather recipe binds its id vector as the
    // transformer-shaped signature's first extra input (binding 1) — the
    // same binding [`transformer_shape_signature`] synthesized, so the
    // data-flow pass never re-binds the ids as a separate input.
    if let Some(ids_local) = gather_ids_local(function) {
        if let Some(extra) = signature.extra_inputs.first_mut() {
            extra.source_local = Some(ids_local);
        }
        bound.push(ids_local);
    }
    // RMSNorm's gamma peer is likewise already a typed resource in the
    // transformer-shaped signature. Mark its local bound so the generic
    // data-flow tail does not append a duplicate gamma buffer after it.
    if let Some(gamma_local) = rms_norm_gamma_local(function) {
        bound.push(gamma_local);
    }
    // RoPE's table resource is already part of the recipe-shaped signature;
    // mark its explicit call operand bound so generic data-flow discovery does
    // not append a duplicate input.
    if op.is_rope()
        && let Some(table_local) = rope_table_local(function, op)
    {
        bound.push(table_local);
    }
    finish_subchain_emission_signature(
        function,
        validation,
        outputs,
        &mut signature,
        &bound,
        source,
    )?;
    // Rule 11: a run-time row-gather reads its table's and its ids vector's
    // launch-bound extents from `TensorAxisExtent` resources. The
    // whole-function ABI appends those slots; a subchain emission signature
    // must carry the same ones, or the emitted body reads an extent buffer
    // the header never declares.
    if op == MirCollectionOp::TensorGather {
        let table_local = signature.input.source_local.ok_or_else(|| {
            PlanMappingError::mapping("TensorGather table has no local in the emission signature")
        })?;
        let table_ty = function_local_ty(function, table_local)?;
        let table_dims = crate::tensor_type::tensor_dims(table_ty, validation)
            .map_err(PlanMappingError::mapping)?;
        let ids_local = gather_ids_local(function).ok_or_else(|| {
            PlanMappingError::mapping("row-gather ids operand has no local in the subchain body")
        })?;
        let ids_ty = function_local_ty(function, ids_local)?;
        let ids_dims = crate::tensor_type::tensor_dims(ids_ty, validation)
            .map_err(PlanMappingError::mapping)?;
        let table_binding = signature.input.binding;
        let ids_binding = signature
            .extra_inputs
            .first()
            .map(|resource| resource.binding)
            .ok_or_else(|| PlanMappingError::mapping("row-gather ids binding is missing"))?;
        crate::abi::append_gather_axis_extent_resources(
            &mut signature,
            &table_dims,
            &ids_dims,
            table_binding,
            ids_binding,
            validation,
        )?;
    }
    // The transformer dispatch must cover EVERY output's extent — the union
    // (max) over the recipe output and the folded elementwise tail's
    // additional consumed outputs (the keep-dims SumAxis shape: a REDUCED
    // `[2]` recipe output with full-shape `[2,8]` tail outputs must dispatch
    // 16 threads, never the reduced 2 — a reduced-only dispatch leaves 14
    // elements of each tail output unwritten). The recipe arm writes its
    // reduced output at the flat index and the tuple tail guards each write
    // by its own extent, so the union dispatch never overruns the smaller
    // recipe buffer (R6 P1, mirroring the elementwise multi-output union).
    apply_transformer_union_dispatch(&mut signature);
    // PGC-R5 (verdict 91fe3d59): the row-reduction family hands its two
    // ops to the contract geometry — one workgroup per row — whenever the
    // admission predicate resolves and the subchain has no additional
    // consumed outputs (a mixed subchain keeps the union 1D dispatch; the
    // emitter's per-element body covers the tail writes).
    if matches!(
        op,
        MirCollectionOp::TensorRmsNorm | MirCollectionOp::TensorCausalMaskedSoftmax
    ) && signature.extra_outputs.is_empty()
    {
        let admission = if op == MirCollectionOp::TensorRmsNorm {
            crate::kernel_plan::rmsnorm_row_reduction_admission
        } else {
            crate::kernel_plan::causal_softmax_row_reduction_admission
        };
        if let Some((dims, _)) =
            crate::kernel_plan::transformer_recipe_shape(function, validation, op)
            && dims.len() == 2
            && admission(dims[0], dims[1]).is_some()
        {
            let lanes = u64::from(crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X);
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            // 1D dispatch law: threads (rows workgroups × lanes).
            signature.dispatch_size = MirKernelDispatchSize {
                x: dims[0].saturating_mul(lanes),
                y: 1,
                z: 1,
            };
        }
    }
    Ok(signature)
}

/// Set a transformer subchain signature's dispatch to the union of its
/// output extents: the recipe output plus every additional consumed output
/// (the R6 P1 per-output-guard contract the elementwise multi-output ABI
/// already applies). Same-shape recipes (softmax / layer-norm) keep their
/// full-shape dispatch; the keep-dims `SumAxis` tail lifts it to the full
/// elementwise tail extent.
fn apply_transformer_union_dispatch(signature: &mut MirKernelSignature) {
    let union = signature
        .output_resources()
        .fold(0u64, |acc, output| acc.max(output.element_count));
    signature.dispatch_size.x = union;
}

/// The shared tail of the subchain emission signatures: the data-flow
/// inputs beyond the bound recipe operands, the primary output binding, and
/// the additional consumed outputs (D-2b-1).
///
/// `source` is the ORIGINAL function the subchain was decomposed from (the
/// widened d8518267 seam repair): the data-flow count resolution consults it
/// so a `TensorSumAxis`-produced input binds at the REDUCED count — the same
/// count the producing subchain's seam output carries — instead of the
/// keep-dims local type's element product (the AIR `LayerNorm` VJP types
/// `SumAxis` results at the operand shape). `None` keeps the type-derived
/// count (whole-function and test call sites).
fn finish_subchain_emission_signature(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    outputs: &[MirLocalId],
    signature: &mut MirKernelSignature,
    bound: &[MirLocalId],
    source: Option<&MirFunction>,
) -> Result<(), PlanMappingError> {
    // Data-flow inputs: params beyond the recipe operands — locals read from
    // earlier subchains. They bind after the recipe inputs, before the
    // output, mirroring the whole-function ABI's inputs-then-output layout.
    // Run-time matmul extent resources (`TensorAxisExtent`) bind after the
    // output, so they do not take a data-flow/output slot.
    let operand_inputs = signature
        .extra_inputs
        .iter()
        .filter(|resource| {
            !matches!(
                resource.kind,
                MirKernelResourceKind::TensorAxisExtent { .. }
            )
        })
        .count();
    let mut next_binding = 1u32
        + u32::try_from(operand_inputs).map_err(|_| {
            PlanMappingError::mapping("subchain signature has too many extra inputs")
        })?;
    for param in &function.params {
        if bound.contains(&param.local) {
            continue;
        }
        // The recipe's own destination param (the mut-out surface,
        // `out ⇇ a.matmul(b)`) is the output buffer the contract shape
        // already carries. Binding it AGAIN as a data-flow input
        // double-binds the buffer, shifts the output binding past what the
        // descriptor records, and the plan pass' operand-count law
        // (`signature_matches_matmul`'s single extra input) refuses the
        // signature — so the mut-out whole-function seam fails closed. The
        // param stays the output resource.
        if signature.output.source_local == Some(param.local) {
            continue;
        }
        let (element_ty, count) = data_flow_element_ty_and_count_or_runtime(param.ty, validation)?;
        // S6-P1 seam repair (widened d8518267): a data-flow input that is a
        // `TensorSumAxis` result of an EARLIER subchain binds at the REDUCED
        // count the producer's seam output carries — the keep-dims local
        // type would over-size the buffer to the operand shape and the faber
        // (function, local) unification would see two shapes for one local
        // (VersionShapeConflict). The producing SumAxis statement lives in an
        // earlier subchain, so it is scanned from the SOURCE function.
        // Council-8 CB-3: the producer-defined reduced-resource projection
        // rides the consumer input too — the keep-dims read is driven by
        // the carried fact, never by count-equality inference.
        let projection = source
            .and_then(|source| sum_axis_destination_projection(source, param.local, validation));
        let count = source
            .and_then(|source| sum_axis_destination_count(source, param.local, validation))
            .unwrap_or(count);
        let mut input = storage_input_with_local(next_binding, element_ty, count, param.local);
        input.reduced_projection = projection;
        signature.extra_inputs.push(input);
        next_binding = next_binding.saturating_add(1);
    }
    // The primary output (the recipe output) binds after the inputs; each
    // ADDITIONAL consumed output binds its own storage-buffer output after
    // it (D-2b-1).
    signature.output.binding = next_binding;
    next_binding = next_binding.saturating_add(1);
    for &output in outputs.iter().skip(1) {
        let local_ty = function_local_ty(function, output)?;
        let (element_ty, count) = data_flow_element_ty_and_count_or_runtime(local_ty, validation)?;
        signature.extra_outputs.push(storage_output_with_local(
            next_binding,
            element_ty,
            count,
            output,
        ));
        next_binding = next_binding.saturating_add(1);
    }
    Ok(())
}

/// The reduced element count of a `TensorSumAxis` destination local in the
/// source function — the flattening of the operand dims minus the reduced
/// axis, exactly the count [`transformer_shape_signature`] sizes the
/// reduction output at (SumAxis-specific: the CCI collection-op variants
/// `TensorRmsNorm` / `TensorRope` / `TensorGather` have no `SumAxis` reduction
/// semantics and are never matched). `None` when the local is not a
/// `TensorSumAxis` destination — a data-flow input with no reduction
/// semantics keeps its type-derived count.
fn sum_axis_destination_count(
    source: &MirFunction,
    local: MirLocalId,
    validation: &MirValidationContext<'_>,
) -> Option<u64> {
    let (dims, axis) = sum_axis_destination_shape(source, local, validation)?;
    Some(
        dims.iter()
            .enumerate()
            .filter(|(index, _)| *index as u64 != axis)
            .fold(1u64, |acc, (_, &dim)| acc.saturating_mul(dim)),
    )
}

/// The producer-defined reduced-resource projection of a `TensorSumAxis`
/// destination local in the source function (council-8 CB-3): `axis_extent`
/// (`dims[axis]`) + `inner_stride` (the dim product after the axis). This is
/// the fact a keep-dims consumer carries on its input resource so the
/// reduced read is never reconstructed from element count — which is
/// ambiguous for equal dims (a `[4,4]` axis-0 vs axis-1 reduction both
/// produce 4 elements but map differently). `None` when the local is not a
/// `TensorSumAxis` destination.
fn sum_axis_destination_projection(
    source: &MirFunction,
    local: MirLocalId,
    validation: &MirValidationContext<'_>,
) -> Option<crate::abi::ReducedProjection> {
    let (dims, axis) = sum_axis_destination_shape(source, local, validation)?;
    crate::kernel_plan::axis_reduction_projection(&dims, axis)
}

/// The operand dims + reduction axis of a `TensorSumAxis` destination local
/// in the source function — the shared scan behind the reduced count and the
/// reduced-resource projection (council-8 CB-3). `None` when the local is
/// not a `TensorSumAxis` destination with resolvable shape facts.
fn sum_axis_destination_shape(
    source: &MirFunction,
    local: MirLocalId,
    validation: &MirValidationContext<'_>,
) -> Option<(Vec<u64>, u64)> {
    find_collection_call(source, |op, call, destination| {
        if op != MirCollectionOp::TensorSumAxis {
            return None;
        }
        let place = destination?;
        let MirPlaceBase::Local(dest_local) = place.base else {
            return None;
        };
        if dest_local != local {
            return None;
        }
        let operand = call.args.first()?;
        let operand_ty = crate::local_ty::operand_ty(source, operand)?;
        let dims = crate::tensor_type::tensor_literal_dims(operand_ty, validation).ok()?;
        let axis = match call.args.get(1)? {
            MirOperand::Constant(MirConstant::Int(axis)) => u64::try_from(*axis).ok()?,
            MirOperand::Constant(MirConstant::UInt(axis)) => *axis,
            _ => return None,
        };
        Some((dims, axis))
    })
}

/// The local a recipe statement produces (its destination place's base
/// local), if the destination is a plain local (A2-2b) — matched to the
/// RECIPE op specifically (D-2b verification, hand-5's hypothesis): a
/// subchain slice can fold elementwise collection statements BEFORE the
/// recipe, and the primary output `source_local` must bind the RECIPE result,
/// never an elementwise destination — a misbound `[2]`-count reduction
/// output would carry a `[2,8]`-shaped source local and conflate with the
/// elementwise buffer. The recipe ops carry no `CollectionOpContract`
/// variant (transformer) or a contract the caller knows the op of
/// (reduction/matmul/transpose), so the op is threaded in; the new CCI
/// collection-op variants (`TensorRmsNorm` / `TensorRope` / `TensorGather`)
/// are never matched here unless they are the declared recipe op.
///
/// The forward-save value a companion subchain reads is produced by an
/// EARLIER subchain of the same decomposed function, and decomposition
/// subchains share the source function's local tables — so recording the
/// produced local on the contract output gives the producer a
/// `MirLocal` origin the consumer (same function, same local) joins by
/// semantic identity. A source-shaped recipe may instead write a result
/// temp that the IMMEDIATELY-following assignment forwards to an unprojected
/// local (the real lowering's assign-then-use shape) — the forwarded local
/// is the recipe's data-flow identity. Destinations without such a local
/// identity (a bare temp with no forward, a projected place, or no
/// destination) yield `None` and the Synthetic origin fallback stays honest.
fn recipe_destination_local_for(function: &MirFunction, op: MirCollectionOp) -> Option<MirLocalId> {
    let (block_index, statement_index, destination) = 'found: {
        for (block_index, block) in function.blocks.iter().enumerate() {
            for (statement_index, statement) in block.statements.iter().enumerate() {
                if crate::collection_ops::statement_collection_op(statement) != Some(op) {
                    continue;
                }
                let MirStatementKind::RuntimeCall { destination, .. } = &statement.kind else {
                    continue;
                };
                break 'found (block_index, statement_index, destination.as_ref());
            }
        }
        return None;
    };
    let place = destination?;
    if !place.projections.is_empty() {
        return None;
    }
    let (mut identity, cursor) = match place.base {
        MirPlaceBase::Local(local) => (local, statement_index),
        MirPlaceBase::Temp(temp) => {
            // The assign-then-use forward: the next statement copies the
            // recipe's result temp into the declared stage local.
            let next = function
                .blocks
                .get(block_index)?
                .statements
                .get(statement_index + 1)?;
            let MirStatementKind::Assign { place, value } = &next.kind else {
                return None;
            };
            let MirPlaceBase::Local(local) = place.base else {
                return None;
            };
            if !place.projections.is_empty() {
                return None;
            }
            let MirValueKind::Operand(MirOperand::Temp(forwarded_temp)) = &value.kind else {
                return None;
            };
            if *forwarded_temp != temp {
                return None;
            }
            (local, statement_index + 1)
        }
    };
    // The mut-out tail: one plain copy of the identity into a declared
    // param (projected or not) is the recipe's host-visible output buffer
    // (`out ⇇ a.matmul(b)` forwards to the param directly;
    // `let total ← a.sum(); out[0] ← total` forwards through the stage
    // local). The param — not the intermediate local — is what the ABI
    // output resource binds, so the emitters' output-write suppression laws
    // key on it.
    if let Some(next) = function
        .blocks
        .get(block_index)
        .and_then(|block| block.statements.get(cursor + 1))
    {
        let MirStatementKind::Assign { place, value } = &next.kind else {
            return Some(identity);
        };
        let MirValueKind::Operand(MirOperand::Place(source)) = &value.kind else {
            return Some(identity);
        };
        if !source.projections.is_empty() {
            return Some(identity);
        }
        let MirPlaceBase::Local(source_local) = source.base else {
            return Some(identity);
        };
        if source_local != identity {
            return Some(identity);
        }
        if let MirPlaceBase::Local(param_local) = place.base
            && function
                .params
                .iter()
                .any(|param| param.local == param_local)
        {
            identity = param_local;
        }
    }
    Some(identity)
}

/// Whether the subchain's recipe result is forwarded straight into the
/// declared parameter `param` (the lowering's `out ⇇ a.matmul(b)`: the recipe
/// writes a result temp and the very next statement copies it into `param`,
/// the mut-out tail [`recipe_destination_local_for`] resolves): the
/// contract-shaped emission signature then already binds `param` as its
/// output, so the subchain needs no indexed-view reading of its own. A body
/// with no recipe, or a recipe whose result reaches `param` through a stage
/// local or a fused tail, answers `false`.
pub(crate) fn recipe_result_is_param(function: &MirFunction, param: MirLocalId) -> bool {
    let recipe_op = find_collection_call(function, |op, _, _| is_recipe_op(op).then_some(op));
    recipe_op.is_some_and(|op| recipe_destination_local_for(function, op) == Some(param))
}

/// The recipe statement's operand locals, in operand order (the read inputs
/// the contract-shaped signature binds at 0..).
fn recipe_operand_locals(function: &MirFunction) -> Result<Vec<MirLocalId>, PlanMappingError> {
    // Bind the contract recipe's operands, not the first collection statement.
    // Decomposition folds a leading elementwise op into the adjacent recipe
    // subchain; those prefix operands are data-flow inputs, not the recipe.
    let Some((call, _)) = first_collection_call(function, is_contract_recipe_op) else {
        return Err(PlanMappingError::mapping(
            "subchain body has no recipe statement",
        ));
    };
    let operand_end = if matches!(
        call.intrinsic,
        MirIntrinsic::Collection(MirCollectionOp::TensorBatchedAttention)
    ) {
        let axes_base = call.args.len().saturating_sub(5);
        let kind_index = axes_base.saturating_sub(1);
        let kind = match call.args.get(kind_index) {
            Some(MirOperand::Constant(MirConstant::Int(value))) => *value,
            Some(MirOperand::Constant(MirConstant::UInt(value))) => {
                i64::try_from(*value).unwrap_or(-1)
            }
            _ => -1,
        };
        // Projection layout is the const immediately before the kind, while
        // scores carry a scale const there. Fused CausalAttention reuses the
        // context kind with three tensor operands plus a scale const before
        // the kind so WIRE v8 stays unchanged.
        if matches!(kind, 0 | 1 | 2 | 3 | 6 | 7)
            || (kind == 5
                && matches!(
                    call.args.get(kind_index.saturating_sub(1)),
                    Some(MirOperand::Constant(MirConstant::Float(_)))
                ))
        {
            kind_index.saturating_sub(1)
        } else {
            kind_index
        }
    } else {
        call.args.len()
    };
    call.args
        .get(..operand_end)
        .unwrap_or_default()
        .iter()
        .map(|arg| {
            crate::read_analysis::operand_local(arg).ok_or_else(|| {
                PlanMappingError::mapping("recipe operand must reference a tensor local")
            })
        })
        .collect()
}

/// A read input resource carrying a source local (the data-flow inputs the
/// subchain emission signature binds after the recipe operands).
fn storage_input_with_local(
    binding: u32,
    element_ty: MirType,
    count: u64,
    local: MirLocalId,
) -> MirKernelResource {
    let mut resource = storage_input(binding, element_ty, count);
    resource.source_local = Some(local);
    resource
}

/// A write output resource carrying a source local (the subchain's
/// ADDITIONAL consumed outputs the emission signature binds after the
/// primary output — D-2b-1).
fn storage_output_with_local(
    binding: u32,
    element_ty: MirType,
    count: u64,
    local: MirLocalId,
) -> MirKernelResource {
    let mut resource = storage_output(binding, element_ty, count);
    resource.source_local = Some(local);
    resource
}

/// The MIR type of a local in the subchain function — the additional-output
/// sizing source for the emission signature seam.
///
/// # Errors
/// Returns [`PlanMappingError`] when the local is neither a param nor a
/// declared local of the subchain body.
pub(super) fn function_local_ty(
    function: &MirFunction,
    local: MirLocalId,
) -> Result<MirType, PlanMappingError> {
    crate::local_ty::local_ty(function, local).ok_or_else(|| {
        PlanMappingError::mapping(format!(
            "subchain output local {local:?} has no type in the subchain body"
        ))
    })
}
