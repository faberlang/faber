use crate::{
    MirAggregateFields, MirAggregateItem, MirAggregateKind, MirFunction, MirIntrinsic,
    MirLayoutKind, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase,
    MirScalarLayout, MirScanOp, MirStatementKind, MirTerminatorKind, MirType, MirValidationContext,
    MirValueKind,
};
use radix_lexer::Interner;
use radix_types::{NumericWidth, Primitive, Type, TypeId, TypeTable};

use super::{
    CollectionOpContract, DEFAULT_KERNEL_RESOURCE_GROUP, DEFAULT_STORAGE_BUFFER_LIMIT,
    ERR_INDEXED_VIEW_REQUIRES_INPUT, ERR_INDEXED_VIEW_RETURNS_VACUUM,
    ERR_KERNEL_IO_ELEMENT_MISMATCH, ERR_KERNEL_PARAM_RETURN_MISMATCH,
    ERR_KERNEL_RESOURCE_BINDING_OVERFLOW, ERR_KERNEL_RESULT_TUPLE_REQUIRED,
    ERR_KERNEL_RESULT_TUPLE_REQUIRES_INPUT, ERR_KERNEL_TENSOR_EXTENT_OPERAND,
    ERR_KERNEL_TENSOR_EXTENT_U32, MirAbiError, MirKernelBuiltin, MirKernelBuiltinInput,
    MirKernelBuiltinUsage, MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess,
    MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature, MirWorkgroupSize,
    RUNTIME_SHAPED_ELEMENT_COUNT, StorageBufferLimitRecord, apply_collection_op_contract,
    buffer_element_count, buffer_element_type, builtin_input_for_index_param,
    builtin_inputs_for_named_params, called_builtins_for_function, collection_op_contract,
    combined_builtin_dependencies, device_view_element_count, device_view_element_count_or_runtime,
    device_view_element_type, dispatch_size_for_1d_elements, genus_param_layout, is_vacuum,
    kernel_element_byte_width, kernel_has_collection_op, named_builtin_for_param,
    param_source_name, reject_kernel_matrix_abi, require_same_element_type, require_u32_index,
    result_tuple_count, storage_buffer_limit_error, vector_param_layout,
};

struct IndexedViewKernelContext<'a> {
    validation: &'a MirValidationContext<'a>,
    builtin_inputs: Vec<MirKernelBuiltinInput>,
    interner: Option<&'a Interner>,
    allow_host_owned_tensor_entry: bool,
    /// Declared position of the output among the resource-bearing params.
    /// Every input resource binds at its declared position, skipping this
    /// slot, so the binding index is the parameter order (the dispatcher and
    /// the emitter agree on it). The positional `input*/output/index` shape
    /// puts the output after every input.
    output_position: u32,
    /// Declared positions of the further `mut` outputs of a lane-form kernel
    /// (`extra_outputs`, in declaration order). Empty for every other shape.
    extra_output_positions: Vec<u32>,
}

/// Binding index of input `ordinal`: the `ordinal`-th declared position that
/// is not an output position (`output_position` and `extra_output_positions`).
fn input_binding(
    ordinal: usize,
    output_position: u32,
    extra_output_positions: &[u32],
) -> Result<u32, MirAbiError> {
    let ordinal = u32::try_from(ordinal)
        .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?;
    let mut remaining = ordinal;
    let mut position = 0u32;
    loop {
        let is_output = position == output_position || extra_output_positions.contains(&position);
        if !is_output {
            if remaining == 0 {
                return Ok(position);
            }
            remaining -= 1;
        }
        position = position
            .checked_add(1)
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?;
    }
}

/// Whether `function` is a lane-form kernel (it carries the compiler-internal
/// `Lane` builtin parameter): tensors are read and written at lane coordinates,
/// not elementwise over one shared element type.
fn is_lane_form(function: &MirFunction) -> bool {
    function
        .params
        .iter()
        .any(|param| param.mode == MirParamMode::Lane)
}

/// The dense matmul glyph admits BF16 weights only in the first input slot;
/// the result and all F32 peers retain their ordinary strict element rule.
fn require_indexed_element_compatibility(
    function: &MirFunction,
    input_element_ty: MirType,
    output_element_ty: MirType,
    validation: &MirValidationContext<'_>,
    allow_bf16_left: bool,
) -> Result<(), MirAbiError> {
    let is_bf16_left = allow_bf16_left
        && kernel_has_collection_op(function, |op| {
            matches!(op, crate::MirCollectionOp::TensorMatMul)
        })
        && matches!(
            validation.layouts.layout_for_type(input_element_ty),
            Ok(layout) if matches!(layout.kind, MirLayoutKind::Scalar(MirScalarLayout::Bf16))
        )
        && matches!(
            validation.layouts.layout_for_type(output_element_ty),
            Ok(layout) if matches!(layout.kind, MirLayoutKind::Scalar(MirScalarLayout::F32))
        );
    if is_bf16_left {
        Ok(())
    } else {
        require_same_element_type(input_element_ty, output_element_ty, validation)
    }
}

/// K7c: whether `param` is the row-gather ids operand of this function — a
/// rank-1 `tensor<u32, [n]>` kernel input that is `args[1]` of the function's
/// `TensorGather` statement. The ids index the table's rows; they do not flow
/// elementwise into the result, so unlike every other tensor input they are
/// bound to neither the output's element type nor its element count. The
/// resource still comes from the ordinary device-view law (element `u32`,
/// count `n`), and the gather plan cross-checks `n` against the result rows.
fn is_gather_ids_param(
    function: &MirFunction,
    param: &MirParam,
    validation: &MirValidationContext<'_>,
) -> bool {
    let is_u32_tensor = matches!(
        validation.types.get(param.ty.semantic_id()),
        Type::Tensor(element, _)
            if matches!(
                validation.types.get(*element),
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
            )
    );
    if !is_u32_tensor {
        return false;
    }
    function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .any(|statement| {
            if crate::collection_ops::statement_collection_op(statement)
                != Some(crate::MirCollectionOp::TensorGather)
            {
                return false;
            }
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                return false;
            };
            matches!(
                call.args.get(1),
                Some(MirOperand::Place(place))
                    if place.projections.is_empty()
                        && place.base == MirPlaceBase::Local(param.local)
            )
        })
}

/// Well-known resource name for the runtime-extent count channel.
///
/// The host contract populates the extent binding with a `u32` element count
/// before dispatch (CTO #4 option b — well-known binding, `combineMetadata`
/// runtime-param precedent). L8/L9 dtype/stride headroom: the extent is
/// count-typed (`u32`) with no f32-width assumption; payload element types
/// stay per-resource.
const RUNTIME_EXTENT_RESOURCE_NAME: &str = "runtime_extent";

/// CXO F2: extents are `Vec`-capable from day one — one entry per extent
/// channel, appended to `extra_inputs`; Wave 1 populates at most one. The
/// binding is caller-assigned as the next free binding from the positional
/// allocator.
fn runtime_extent_resource(binding: u32, extent_ty: TypeId) -> MirKernelResource {
    MirKernelResource {
        group: DEFAULT_KERNEL_RESOURCE_GROUP,
        binding,
        kind: MirKernelResourceKind::RuntimeExtent,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty: MirType::semantic(extent_ty),
        element_count: 1,
        source_local: None,
        source_name: Some(RUNTIME_EXTENT_RESOURCE_NAME.to_owned()),
        reduced_projection: None,
    }
}

/// The next free binding index: one past the highest binding the signature
/// already holds (real signatures always carry at least the input and the
/// output, so the maximum exists).
fn next_free_binding(signature: &MirKernelSignature) -> u32 {
    signature
        .resources()
        .map(|resource| resource.binding)
        .max()
        .map_or(0, |binding| binding + 1)
}

fn scan_tensor_rank(shape: radix_types::IndexId, types: &TypeTable) -> Option<usize> {
    Some(match types.get_index(shape) {
        radix_types::IndexExpr::Tuple(axes) => axes.len(),
        radix_types::IndexExpr::Literal(_)
        | radix_types::IndexExpr::Param(_)
        | radix_types::IndexExpr::Runtime(_)
        | radix_types::IndexExpr::Affine { .. }
        | radix_types::IndexExpr::Divide { .. } => 1,
        radix_types::IndexExpr::Infer(_) | radix_types::IndexExpr::Unspecified => return None,
    })
}

fn scan_tensor_shape(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Option<radix_types::IndexId> {
    let ty = validation.types.peel_access_marker(ty.semantic_id()).1;
    match validation.types.get(ty) {
        Type::Tensor(_, shape) => Some(*shape),
        Type::Alias(_, inner) => scan_tensor_shape(MirType::semantic(*inner), validation),
        _ => None,
    }
}

/// Scan signatures preserve a literal empty shape in their resources, but
/// keep a nonzero placeholder dispatch extent. The device session returns the
/// empty output before allocating scratch or launching a stage.
fn scan_tensor_element_count(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<u64, MirAbiError> {
    let shape = scan_tensor_shape(ty, validation)
        .ok_or_else(|| MirAbiError::new("scan source/return must be a tensor"))?;
    let axes: Vec<radix_types::IndexId> = match validation.types.get_index(shape) {
        radix_types::IndexExpr::Tuple(axes) => axes.clone(),
        radix_types::IndexExpr::Literal(_)
        | radix_types::IndexExpr::Param(_)
        | radix_types::IndexExpr::Runtime(_)
        | radix_types::IndexExpr::Affine { .. }
        | radix_types::IndexExpr::Divide { .. } => vec![shape],
        radix_types::IndexExpr::Infer(_) | radix_types::IndexExpr::Unspecified => {
            return Ok(RUNTIME_SHAPED_ELEMENT_COUNT);
        }
    };
    let extents = axes
        .iter()
        .map(|axis| {
            radix_types::evaluate_index_dimension(*axis, &validation.types)
                .map_err(|error| MirAbiError::new(format!("scan tensor extent: {error:?}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if extents.contains(&Some(0)) {
        return Ok(0);
    }
    if extents.iter().any(Option::is_none) {
        return Ok(RUNTIME_SHAPED_ELEMENT_COUNT);
    }
    extents
        .into_iter()
        .flatten()
        .try_fold(1_u64, u64::checked_mul)
        .ok_or_else(|| MirAbiError::new("scan tensor element count overflows u64"))
}

/// Append a runtime-extent count resource when the kernel is scalar-shaped
/// or a run-time-shaped whole-tensor elementwise body.
///
/// "Where the runtime extent applies" (U2): a kernel whose static IO element
/// count is 1 has `dispatch_size = (1, 1, 1)`, so a guard over the static
/// count culls every host-dispatched thread past the first. The extent
/// channel fixes that: the host supplies the element count at dispatch and
/// the guard reads the binding. Kernels with statically-known multi-element
/// shapes keep the static limit (correct), and collection-op contracts
/// (reduction/matmul) stay static per the U2 non-goals.
///
/// The same hazard applies to a run-time-shaped whole-tensor elementwise
/// output-parameter kernel (`o ⇇ a + b` over a `size N` extent,
/// [`MirKernelSignature::is_elementwise_output_form`]): its buffers carry the
/// [`RUNTIME_SHAPED_ELEMENT_COUNT`] marker, the one-lane dispatch is the
/// placeholder, and the per-call launch geometry rounds the real count up to
/// whole workgroups — so the emitted bounds guard must read the per-call
/// count channel exactly like the decomposed elementwise emission signature
/// ([`crate::device_program_plans::append_runtime_extent_count_channel`]).
/// Without the channel the Metal emitter refuses the kernel fail-closed
/// (`mir_metal_runtime_shaped_elementwise_extent_missing`). `TensorAdd` and
/// friends are collection ops, but they carry NO dispatch contract — the
/// contract-bearing kernels (reduction, matmul, transpose, ...) stay static
/// per the U2 non-goals. Lane-form and sequential single-lane bodies are
/// excluded from the runtime rule: the lane domain carries its own
/// axis-extent guard and a single-lane body runs one invocation, so neither
/// dispatches per element — and a run-time-shaped surface outside the
/// elementwise rule keeps the pre-U2 skip.
///
/// The count type (`numerus<u32>`, L8/L9) must already exist in the frozen
/// type table; when it cannot be resolved the kernel keeps the static
/// dispatch-axis fallback (pre-U2 behavior) — the guard still fires, but at
/// the static `oob_guard_limit` instead of the host-supplied count.
/// PRODUCTION INVARIANT: the GPU builtin registry interns `numerus<u32>`
/// for every kernel program, so real kernels always carry the extent; only
/// hand-built unit tables may lack it (`abi_test` pins the fallback).
fn maybe_append_runtime_extent(
    function: &MirFunction,
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) {
    let runtime_shaped = signature
        .resources()
        .any(MirKernelResource::is_runtime_shaped);
    // The whole-tensor elementwise output-parameter rule (below): a
    // run-time-shaped `o ⇇ a + b` body whose collection op carries NO
    // dispatch contract. Everything else keeps the pre-existing guard
    // story — collection-op contracts stay static (U2 non-goal), lane-form
    // bodies guard at the lane domain's axis extents, a sequential
    // single-lane body runs one invocation, and statically-shaped
    // multi-element kernels keep the honest static limit.
    let elementwise_runtime = runtime_shaped
        && matches!(collection_op_contract(function, validation), Ok(None))
        && MirKernelSignature::is_elementwise_output_form(function, validation);
    if kernel_has_collection_op(function, |_| true) && !elementwise_runtime {
        return;
    }
    if !elementwise_runtime
        && (signature.input.element_count != 1 || signature.output.element_count != 1)
    {
        return;
    }
    let Some(extent_ty) = validation
        .types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
    else {
        return;
    };
    let binding = next_free_binding(signature);
    signature
        .extra_inputs
        .push(runtime_extent_resource(binding, extent_ty));
}

/// The whole-grid reduction node of a lane-form function (C4-8): the node
/// facts and its source parameter local (alias-resolved).
pub(crate) fn grid_reduce_node(
    function: &MirFunction,
) -> Option<(&crate::MirFilumReduceOp, MirLocalId)> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find_map(|statement| {
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                return None;
            };
            let crate::MirIntrinsic::FilumReduce(filum) = &call.intrinsic else {
                return None;
            };
            if filum.form != crate::MirFilumForm::Grid {
                return None;
            }
            let Some(MirOperand::Place(place)) = call.args.first() else {
                return None;
            };
            let MirPlaceBase::Local(local) = place.base else {
                return None;
            };
            Some((filum, ParameterAliases::new(function).resolve(local)))
        })
}

/// The kernel tensor parameters a whole-grid reduction captures into a
/// function-form term or group-key helper, in operand order (term first, then
/// each key), each once. An index inside a helper body is never proven by the
/// lane (the helper has no grid guard), so each such tensor owns one index
/// fault slot and one element-count resource; the slots follow the grouped-key
/// axis slots in the fault record. The helper bodies are separate
/// functions the signature cannot see, so the set is the captures, not the
/// indexes the bodies actually make.
#[must_use]
pub fn grid_reduce_helper_tensors(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Vec<MirLocalId> {
    let Some((filum, _)) = grid_reduce_node(function) else {
        return Vec::new();
    };
    let Some(call) = function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. }
                if matches!(&call.intrinsic, crate::MirIntrinsic::FilumReduce(_)) =>
            {
                Some(call)
            }
            _ => None,
        })
    else {
        return Vec::new();
    };
    let args = &call.args;
    let key_operands: usize = filum.group.as_ref().map_or(0, |group| {
        group.keys.iter().map(|key| key.captures as usize + 1).sum()
    });
    // Capture runs as `(start, count)` in the node's operand list.
    let mut runs = Vec::new();
    if let Some(captures) = filum.term_captures {
        let captures = captures as usize;
        if let Some(function_at) = args.len().checked_sub(key_operands + 1)
            && let Some(start) = function_at.checked_sub(captures)
        {
            runs.push((start, captures));
        }
    }
    if let Some(group) = &filum.group {
        let mut at = args.len().saturating_sub(key_operands);
        for key in &group.keys {
            runs.push((at, key.captures as usize));
            at += key.captures as usize + 1;
        }
    }
    let aliases = ParameterAliases::new(function);
    let mut tensors = Vec::new();
    for (start, count) in runs {
        for operand in args.iter().skip(start).take(count) {
            let MirOperand::Place(place) = operand else {
                continue;
            };
            let MirPlaceBase::Local(local) = place.base else {
                continue;
            };
            if !place.projections.is_empty() {
                continue;
            }
            let local = aliases.resolve(local);
            let is_tensor_param = function.params.iter().any(|param| {
                param.local == local
                    && matches!(
                        validation.types.get(param.ty.semantic_id()),
                        Type::Tensor(..)
                    )
            });
            if is_tensor_param && !tensors.contains(&local) {
                tensors.push(local);
            }
        }
    }
    tensors
}

/// The whole-source scan node and its source parameter (alias-resolved).
pub(crate) fn scan_node(function: &MirFunction) -> Option<(&MirScanOp, MirLocalId)> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find_map(|statement| {
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                return None;
            };
            let crate::MirIntrinsic::Scan(scan) = &call.intrinsic else {
                return None;
            };
            let Some(MirOperand::Place(place)) = call.args.first() else {
                return None;
            };
            let MirPlaceBase::Local(local) = place.base else {
                return None;
            };
            Some((scan, ParameterAliases::new(function).resolve(local)))
        })
}

/// Layout used by each scan summary. Count and boolean scans carry a u32
/// summary; extrema carry the output value plus the existing `seen` flag.
pub(crate) fn scan_partial_layout(
    scan: &MirScanOp,
    validation: &MirValidationContext<'_>,
) -> Result<crate::kernel_plan::GridReducePartialLayout, MirAbiError> {
    let partial_ty = scan_partial_type(scan, validation)?;
    let value_bytes = match validation.layouts.layout_for_type(partial_ty) {
        Ok(layout) => match layout.kind {
            MirLayoutKind::Scalar(scalar) => {
                u32::from(kernel_element_byte_width(scalar).unwrap_or(4))
            }
            _ => return Err(MirAbiError::new("scan result element is not scalar")),
        },
        Err(_) => {
            return Err(MirAbiError::new(
                "scan result element layout is unavailable",
            ));
        }
    };
    Ok(crate::kernel_plan::GridReducePartialLayout::for_op(
        scan.op,
        value_bytes,
        scan.rank,
    ))
}

/// Append the dispatcher-owned prefix and summary scratch resources after
/// declared parameters and before runtime extents. Every scan stage uses the
/// same binding order.
fn append_scan_scratch_resources(
    function: &MirFunction,
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    let Some((scan, _)) = scan_node(function) else {
        return Ok(());
    };
    let group_count = scan.group.as_ref().map_or(Some(1_u64), |group| {
        group
            .axes
            .iter()
            .try_fold(1_u64, |count, axis| count.checked_mul(axis.extent))
    });
    let group_count = group_count.ok_or_else(|| MirAbiError::new("scan group count overflows"))?;
    if group_count == 0 {
        return Err(MirAbiError::new("scan group count is empty"));
    }
    let group_count =
        u32::try_from(group_count).map_err(|_| MirAbiError::new("scan group count exceeds u32"))?;
    let layout = scan_partial_layout(scan, validation)?;
    let partial_ty = scan_partial_type(scan, validation)?;
    let blocks_cap = crate::kernel_plan::GRID_REDUCE_PARTIALS_CAP;
    let partial_slots = u64::from(blocks_cap)
        .checked_mul(u64::from(group_count))
        .and_then(|slots| u32::try_from(slots).ok())
        .ok_or_else(|| MirAbiError::new("scan summary slot count overflows u32"))?;
    let element_bytes = match validation.layouts.layout_for_type(scan.result_element_ty) {
        Ok(layout) => match layout.kind {
            MirLayoutKind::Scalar(scalar) => {
                u32::from(kernel_element_byte_width(scalar).unwrap_or(4))
            }
            _ => return Err(MirAbiError::new("scan result element is not scalar")),
        },
        Err(_) => {
            return Err(MirAbiError::new(
                "scan result element layout is unavailable",
            ));
        }
    };
    let prefix_binding = next_free_binding(signature);
    signature.extra_inputs.push(MirKernelResource {
        group: DEFAULT_KERNEL_RESOURCE_GROUP,
        binding: prefix_binding,
        kind: MirKernelResourceKind::PrefixScratch { element_bytes },
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::ReadWrite,
        element_ty: scan.result_element_ty,
        // Live capacity comes from the source length and plan at dispatch.
        element_count: 0,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    });
    let partial_binding = next_free_binding(signature);
    signature.extra_inputs.push(MirKernelResource {
        group: DEFAULT_KERNEL_RESOURCE_GROUP,
        binding: partial_binding,
        kind: MirKernelResourceKind::PartialsScratch {
            cap: partial_slots,
            element_bytes: layout.element_bytes(),
        },
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::ReadWrite,
        element_ty: partial_ty,
        element_count: u64::from(partial_slots),
        source_local: None,
        source_name: None,
        reduced_projection: None,
    });
    Ok(())
}

fn scan_partial_type(
    scan: &MirScanOp,
    validation: &MirValidationContext<'_>,
) -> Result<MirType, MirAbiError> {
    if matches!(
        scan.op,
        crate::MirReduceOp::Count | crate::MirReduceOp::All | crate::MirReduceOp::Any
    ) {
        return validation
            .types
            .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
            .map(MirType::semantic)
            .ok_or_else(|| MirAbiError::new("scan boolean/count summary requires a u32 carrier"));
    }
    Ok(scan.result_element_ty)
}

pub(crate) fn grid_reduce_partial_layout(
    filum: &crate::MirFilumReduceOp,
    validation: &MirValidationContext<'_>,
) -> Result<crate::kernel_plan::GridReducePartialLayout, MirAbiError> {
    let rank = u8::try_from(filum.coordinates.len())
        .map_err(|_| MirAbiError::new("grid reduce rank exceeds 255"))?;
    let partial_ty = match (filum.group.as_ref(), filum.op) {
        (Some(group), crate::MirReduceOp::Sum) => group.result_element_ty,
        (Some(_), crate::MirReduceOp::Count) => MirType::semantic(
            validation
                .types
                .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
                .ok_or_else(|| MirAbiError::new("grouped count needs a u32 partial carrier"))?,
        ),
        _ => filum.term_ty,
    };
    let value_bytes = match validation.layouts.layout_for_type(partial_ty) {
        Ok(layout) => match layout.kind {
            MirLayoutKind::Scalar(scalar) => {
                u32::from(kernel_element_byte_width(scalar).unwrap_or(4))
            }
            _ => 4,
        },
        Err(_) => 4,
    };
    Ok(crate::kernel_plan::GridReducePartialLayout::for_op(
        filum.op,
        value_bytes,
        rank,
    ))
}

/// C4-8: append the dispatcher-owned partials scratch of a whole-grid
/// reduction after every declared parameter resource (and before the tensor
/// extents, which stay last). A function with no grid reduction is untouched.
fn append_partials_scratch_resource(
    function: &MirFunction,
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    let Some((filum, _)) = grid_reduce_node(function) else {
        return Ok(());
    };
    let layout = grid_reduce_partial_layout(filum, validation)?;
    let group_count = filum
        .group
        .as_ref()
        .map_or(Some(1), |group| {
            group
                .dimensions
                .iter()
                .try_fold(1_u64, |count, extent| count.checked_mul(*extent))
        })
        .ok_or_else(|| MirAbiError::new("grid reduction bucket count overflows"))?;
    let partial_slots = u64::from(crate::kernel_plan::GRID_REDUCE_PARTIALS_CAP)
        .checked_mul(group_count)
        .and_then(|slots| u32::try_from(slots).ok())
        .ok_or_else(|| MirAbiError::new("grid reduction partial slot count overflows u32"))?;
    let partial_ty = match (filum.group.as_ref(), filum.op) {
        (Some(group), crate::MirReduceOp::Sum) => group.result_element_ty,
        (Some(_), crate::MirReduceOp::Count) => MirType::semantic(
            validation
                .types
                .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
                .ok_or_else(|| MirAbiError::new("grouped count needs a u32 partial carrier"))?,
        ),
        _ => filum.term_ty,
    };
    let binding = next_free_binding(signature);
    signature.extra_inputs.push(MirKernelResource {
        group: DEFAULT_KERNEL_RESOURCE_GROUP,
        binding,
        kind: MirKernelResourceKind::PartialsScratch {
            cap: partial_slots,
            element_bytes: layout.element_bytes(),
        },
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::ReadWrite,
        element_ty: partial_ty,
        element_count: u64::from(partial_slots),
        source_local: None,
        source_name: None,
        reduced_projection: None,
    });
    Ok(())
}

/// Collects the distinct `(tensor operand local, axis)` pairs a kernel body
/// reads through `MirValueKind::TensorExtent`.
struct TensorExtentReads {
    reads: Vec<(MirLocalId, u32)>,
    non_parameter_operand: bool,
}

impl crate::visit::MirVisitor for TensorExtentReads {
    fn visit_value(&mut self, value: &crate::MirValue) {
        if let crate::MirValueKind::TensorExtent { tensor, axis } = &value.kind {
            match tensor {
                MirOperand::Place(place)
                    if place.projections.is_empty()
                        && matches!(place.base, MirPlaceBase::Local(_)) =>
                {
                    if let MirPlaceBase::Local(local) = place.base {
                        self.reads.push((local, *axis));
                    }
                }
                _ => self.non_parameter_operand = true,
            }
        }
        crate::visit::walk_value(self, value);
    }
}

/// How the statements of one function write a local, for
/// [`ParameterAliases`].
#[derive(Clone, Copy)]
enum AliasWrites {
    /// Exactly one assignment, a whole copy of this other local.
    Copy(MirLocalId),
    /// Any other write pattern: several assignments, a projected store, or a
    /// value that is not a whole-local copy.
    Other,
}

/// The parameter alias facts of one function: which parameter a local is a
/// single-assignment copy chain of. Composition (rule 12.7 inlining) binds a
/// callee parameter by `callee_param = caller_value`; a tensor or genus
/// parameter copied that way is one storage, so everything that asks which
/// buffer a local names resolves through the copy.
///
/// The per-local write facts are indexed from one pass over the function the
/// first time a non-parameter local is resolved, so a caller that resolves
/// many locals holds one `ParameterAliases` and pays for one scan, and a
/// caller that only ever resolves parameters pays for none.
pub struct ParameterAliases<'a> {
    function: &'a MirFunction,
    writes: std::sync::OnceLock<handroll::FxHashMap<MirLocalId, AliasWrites>>,
}

impl<'a> ParameterAliases<'a> {
    #[must_use]
    pub fn new(function: &'a MirFunction) -> Self {
        Self {
            function,
            writes: std::sync::OnceLock::new(),
        }
    }

    fn writes(&self) -> &handroll::FxHashMap<MirLocalId, AliasWrites> {
        self.writes.get_or_init(|| {
            let mut writes = handroll::FxHashMap::default();
            for statement in self.function.blocks.iter().flat_map(|b| &b.statements) {
                let crate::MirStatementKind::Assign { place, value } = &statement.kind else {
                    continue;
                };
                let MirPlaceBase::Local(target) = place.base else {
                    continue;
                };
                let copy = match &value.kind {
                    crate::MirValueKind::Operand(MirOperand::Place(source))
                        if place.projections.is_empty() && source.projections.is_empty() =>
                    {
                        match source.base {
                            MirPlaceBase::Local(source) => Some(source),
                            MirPlaceBase::Temp(_) => None,
                        }
                    }
                    _ => None,
                };
                let write = copy.map_or(AliasWrites::Other, AliasWrites::Copy);
                writes
                    .entry(target)
                    .and_modify(|seen| *seen = AliasWrites::Other)
                    .or_insert(write);
            }
            writes
        })
    }

    /// The parameter local `local` is a single-assignment copy chain of, or
    /// the end of the chain when it does not reach a parameter.
    #[must_use]
    pub fn resolve(&self, local: MirLocalId) -> MirLocalId {
        let function = self.function;
        let mut current = local;
        // Bounded by the number of locals: a copy chain cannot be longer.
        for _ in 0..function.locals.len() + function.params.len() {
            if function.params.iter().any(|param| param.local == current) {
                return current;
            }
            match self.writes().get(&current) {
                Some(AliasWrites::Copy(source)) => current = *source,
                _ => return current,
            }
        }
        current
    }
}

/// gpu-reset C4-2e-a: append one count-one `u32` input resource per distinct
/// `(tensor parameter, axis)` the kernel body reads through `TensorExtent`.
///
/// Appended after every declared parameter resource (and the Lane builtin,
/// which is not a resource), ordered by tensor parameter declaration order
/// then axis ascending, deduplicated. Each carries
/// [`MirKernelResourceKind::TensorAxisExtent`] so the host knows which tensor
/// resource and axis it is. A kernel with no `TensorExtent` read is untouched.
fn append_tensor_extent_resources(
    function: &MirFunction,
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    use crate::visit::MirVisitor;
    let mut collected = TensorExtentReads {
        reads: Vec::new(),
        non_parameter_operand: false,
    };
    collected.visit_function(function);
    collect_matmul_runtime_reads(function, validation, &mut collected.reads);
    collect_gather_runtime_reads(function, validation, &mut collected.reads);
    if collected.non_parameter_operand {
        return Err(MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND));
    }
    let scan = scan_node(function);
    if collected.reads.is_empty() && scan.is_none() {
        return Ok(());
    }
    let aliases = ParameterAliases::new(function);
    let mut ordered = std::collections::BTreeSet::new();
    for (local, axis) in collected.reads {
        // A kernel composed from helpers binds each callee tensor parameter
        // with one copy of the caller's value; the extent is the caller
        // parameter's.
        let local = aliases.resolve(local);
        let Some(parameter) = function
            .params
            .iter()
            .position(|param| param.local == local)
        else {
            return Err(MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND));
        };
        ordered.insert((parameter, local, axis));
    }
    // A scan's shape belongs to its source even though the typed declaration
    // deliberately keeps the source/helper operands compact. Record each axis
    // extent explicitly so all three scan stages bind one identical ABI.
    if let Some((scan, local)) = scan {
        let parameter = function
            .params
            .iter()
            .position(|param| param.local == local)
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?;
        for axis in 0..u32::from(scan.rank) {
            ordered.insert((parameter, local, axis));
        }
        let call = function
            .blocks
            .iter()
            .flat_map(|block| &block.statements)
            .find_map(|statement| {
                let crate::MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                    return None;
                };
                matches!(&call.intrinsic, crate::MirIntrinsic::Scan(_)).then_some(call)
            })
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?;
        let term_captures = usize::try_from(scan.term_captures)
            .map_err(|_| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?;
        let term_end = 1_usize
            .checked_add(term_captures)
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?;
        for operand in call
            .args
            .get(1..term_end)
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?
        {
            append_scan_capture_extents(operand, function, &aliases, validation, &mut ordered)?;
        }
        let group = scan.group.as_ref();
        if let Some(group) = group {
            let mut position = term_end + 1;
            for axis in &group.axes {
                let captures = usize::try_from(axis.captures)
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?;
                let end = position
                    .checked_add(captures)
                    .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?;
                for operand in call
                    .args
                    .get(position..end)
                    .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?
                {
                    append_scan_capture_extents(
                        operand,
                        function,
                        &aliases,
                        validation,
                        &mut ordered,
                    )?;
                }
                position = end
                    .checked_add(1)
                    .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?;
            }
        }
    }
    for (_, local, axis) in ordered {
        let tensor_binding = signature
            .resources()
            .find(|resource| {
                resource.kind == MirKernelResourceKind::StorageBuffer
                    && resource.source_local == Some(local)
            })
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?
            .binding;
        push_axis_extent_resource(signature, tensor_binding, axis, validation)?;
    }
    Ok(())
}

/// Scan helper captures can read tensor parameters through the generated term
/// and key functions. Bind their axis extents in the enclosing kernel so the
/// Metal helper can receive the same buffer and shape facts as its caller.
fn append_scan_capture_extents(
    operand: &MirOperand,
    function: &MirFunction,
    aliases: &ParameterAliases<'_>,
    validation: &MirValidationContext<'_>,
    ordered: &mut std::collections::BTreeSet<(usize, MirLocalId, u32)>,
) -> Result<(), MirAbiError> {
    let (parameter, local, ty) = match operand {
        MirOperand::Place(place) if place.projections.is_empty() => match place.base {
            MirPlaceBase::Local(local) => {
                let root = aliases.resolve(local);
                if let Some((position, parameter)) = function
                    .params
                    .iter()
                    .enumerate()
                    .find(|(_, parameter)| parameter.local == root)
                {
                    (Some(position), Some(root), Some(parameter.ty))
                } else {
                    let ty = function
                        .locals
                        .iter()
                        .find(|decl| decl.id == local)
                        .map(|decl| decl.ty);
                    (None, Some(local), ty)
                }
            }
            MirPlaceBase::Temp(temp) => {
                let ty = function
                    .temps
                    .iter()
                    .find(|candidate| candidate.id == temp)
                    .map(|temp| temp.ty);
                (None, None, ty)
            }
        },
        MirOperand::Temp(temp) => {
            let ty = function
                .temps
                .iter()
                .find(|candidate| candidate.id == *temp)
                .map(|temp| temp.ty);
            (None, None, ty)
        }
        _ => return Ok(()),
    };
    let Some(ty) = ty else {
        return Err(MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND));
    };
    let Type::Tensor(_, shape) = validation.types.get(ty.semantic_id()) else {
        return Ok(());
    };
    let rank = match validation.types.get_index(*shape) {
        radix_types::IndexExpr::Tuple(axes) => axes.len(),
        radix_types::IndexExpr::Literal(_)
        | radix_types::IndexExpr::Param(_)
        | radix_types::IndexExpr::Runtime(_)
        | radix_types::IndexExpr::Affine { .. }
        | radix_types::IndexExpr::Divide { .. } => 1,
        radix_types::IndexExpr::Infer(_) | radix_types::IndexExpr::Unspecified => {
            return Err(MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND));
        }
    };
    let (Some(parameter), Some(local)) = (parameter, local) else {
        return Err(MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND));
    };
    for axis in 0..rank {
        ordered.insert((
            parameter,
            local,
            u32::try_from(axis).map_err(|_| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?,
        ));
    }
    Ok(())
}

/// Rule 11: appends the run-time axis extent resources of a row-gather
/// signature — the table's live axes (`table_binding`) and the ids vector's
/// (`ids_binding`), in axis order. A subchain emission signature carries the
/// same resources the whole-function ABI appends, so the emitted header
/// declares the count-one `u32` extents the body reads.
pub(crate) fn append_gather_axis_extent_resources(
    signature: &mut MirKernelSignature,
    table_dims: &[crate::tensor_type::Dim],
    ids_dims: &[crate::tensor_type::Dim],
    table_binding: u32,
    ids_binding: u32,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    let runtime_axes = |dims: &[crate::tensor_type::Dim]| {
        dims.iter()
            .enumerate()
            .filter(|(_, dim)| matches!(dim, crate::tensor_type::Dim::Runtime))
            .map(|(axis, _)| axis as u32)
            .collect::<Vec<u32>>()
    };
    for axis in runtime_axes(table_dims) {
        push_axis_extent_resource(signature, table_binding, axis, validation)?;
    }
    for axis in runtime_axes(ids_dims) {
        push_axis_extent_resource(signature, ids_binding, axis, validation)?;
    }
    Ok(())
}

/// Appends one count-one `u32` [`MirKernelResourceKind::TensorAxisExtent`]
/// input after the highest binding the signature already holds.
fn push_axis_extent_resource(
    signature: &mut MirKernelSignature,
    tensor_binding: u32,
    axis: u32,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    let extent_ty = validation
        .types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
        .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_U32))?;
    let binding = next_free_binding(signature);
    signature.extra_inputs.push(MirKernelResource {
        group: DEFAULT_KERNEL_RESOURCE_GROUP,
        binding,
        kind: MirKernelResourceKind::TensorAxisExtent {
            tensor_binding,
            axis,
        },
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty: MirType::semantic(extent_ty),
        element_count: 1,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    });
    Ok(())
}

/// A gather recipe consumes each run-time table axis and, when dynamic, the
/// ids vector's single extent even though neither is read as a MIR
/// `TensorExtent` value in the kernel body.
fn collect_gather_runtime_reads(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    reads: &mut Vec<(MirLocalId, u32)>,
) {
    for block in &function.blocks {
        for statement in &block.statements {
            if crate::collection_ops::statement_collection_op(statement)
                != Some(crate::MirCollectionOp::TensorGather)
            {
                continue;
            }
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                continue;
            };
            let [table, ids] = call.args.as_slice() else {
                continue;
            };
            let operand_dims = |arg: &MirOperand| match arg {
                MirOperand::Place(place)
                    if place.projections.is_empty()
                        && matches!(place.base, MirPlaceBase::Local(_)) =>
                {
                    let MirPlaceBase::Local(local) = place.base else {
                        return None;
                    };
                    let ty = crate::local_ty::operand_ty(function, arg)?;
                    let dims = crate::tensor_type::tensor_dims(ty, validation).ok()?;
                    Some((local, dims))
                }
                _ => None,
            };
            let (Some((table_local, table_dims)), Some((ids_local, ids_dims))) =
                (operand_dims(table), operand_dims(ids))
            else {
                continue;
            };
            reads.extend(
                table_dims
                    .iter()
                    .enumerate()
                    .filter(|(_, dim)| matches!(dim, crate::tensor_type::Dim::Runtime))
                    .map(|(axis, _)| (table_local, axis as u32)),
            );
            if matches!(ids_dims.first(), Some(crate::tensor_type::Dim::Runtime)) {
                reads.push((ids_local, 0));
            }
        }
    }
}

/// The operand axes a run-time-extent matmul consumes: every axis whose dim is
/// `Runtime`, left operand first (M = left axis 0, K = left axis 1), then the
/// right operand (K = right axis 0, N = right axis 1; a rank-1 right operand
/// has only axis 0). Literal axes need no resource.
fn matmul_runtime_axes(
    left: &[crate::tensor_type::Dim],
    right: &[crate::tensor_type::Dim],
) -> (Vec<u32>, Vec<u32>) {
    let runtime = |dims: &[crate::tensor_type::Dim]| {
        dims.iter()
            .enumerate()
            .filter(|(_, dim)| matches!(dim, crate::tensor_type::Dim::Runtime))
            .map(|(axis, _)| axis as u32)
            .collect::<Vec<u32>>()
    };
    (runtime(left), runtime(right))
}

/// Design-note item 4: a collection-op matmul with run-time extents consumes
/// its operands' extents but contains no `TensorExtent` read, so the extents
/// are collected from the matmul statement itself.
fn collect_matmul_runtime_reads(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    reads: &mut Vec<(MirLocalId, u32)>,
) {
    for block in &function.blocks {
        for statement in &block.statements {
            if crate::collection_ops::statement_collection_op(statement)
                != Some(crate::MirCollectionOp::TensorMatMul)
            {
                continue;
            }
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                continue;
            };
            let [left, right] = call.args.as_slice() else {
                continue;
            };
            let operand = |arg: &MirOperand| match arg {
                MirOperand::Place(place)
                    if place.projections.is_empty()
                        && matches!(place.base, MirPlaceBase::Local(_)) =>
                {
                    let MirPlaceBase::Local(local) = place.base else {
                        return None;
                    };
                    let ty = crate::local_ty::operand_ty(function, arg)?;
                    let dims = crate::tensor_type::tensor_dims(ty, validation).ok()?;
                    Some((local, dims))
                }
                _ => None,
            };
            let (Some((left_local, left_dims)), Some((right_local, right_dims))) =
                (operand(left), operand(right))
            else {
                continue;
            };
            let (left_axes, right_axes) = matmul_runtime_axes(&left_dims, &right_dims);
            reads.extend(left_axes.into_iter().map(|axis| (left_local, axis)));
            reads.extend(right_axes.into_iter().map(|axis| (right_local, axis)));
        }
    }
}

/// Design-note item 4: appends the extent resources of a run-time-extent
/// matmul contract signature (left operand at binding 0, right at binding 1),
/// after the output. Same shape as [`append_tensor_extent_resources`].
pub(crate) fn append_matmul_runtime_extent_resources(
    signature: &mut MirKernelSignature,
    left_dims: &[crate::tensor_type::Dim],
    right_dims: &[crate::tensor_type::Dim],
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    let (left_axes, right_axes) = matmul_runtime_axes(left_dims, right_dims);
    for axis in left_axes {
        push_axis_extent_resource(signature, 0, axis, validation)?;
    }
    for axis in right_axes {
        push_axis_extent_resource(signature, 1, axis, validation)?;
    }
    Ok(())
}

/// gpu-reset U9-2: a guarded lane-form kernel with at least one UNPROVEN
/// tensor index (see [`super::LaneIndexProof`]) gets, after every other
/// resource, one count-one `u32` [`MirKernelResourceKind::TensorElementCount`]
/// per indexed tensor with an unproven index (tensor parameter declaration
/// order) and then ONE [`MirKernelResourceKind::FaultRecord`] with a slot per
/// fault site, LAST. Sites are tensor index forms (loads before stores), then
/// distinct run-time vector index operands in first-use MIR order, then one
/// grouped-key validity slot per output axis, then one slot per constant throw
/// (see [`super::throw_sites`]), then ONE `d64` slot when the body does decimal
/// arithmetic (see [`super::d64_fault_site`]). A whole-grid reduction adds, after its
/// grouped-key slots, one slot per tensor captured into a term or key helper
/// (see [`grid_reduce_helper_tensors`]). A kernel whose every
/// tensor index is proven and that has no vector index or throw is untouched.
fn append_fault_resources(
    function: &MirFunction,
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    let forms = super::unproven_indexes(function, validation);
    let vector_forms = super::unproven_vector_indexes(function, validation);
    let throws = super::throw_sites(function, &validation.types).map_or(0, |throws| throws.len());
    let group_axes = grid_reduce_node(function)
        .and_then(|(filum, _)| filum.group.as_ref())
        .map_or(0, |group| group.keys.len())
        + scan_node(function)
            .and_then(|(scan, _)| scan.group.as_ref())
            .map_or(0, |group| group.axes.len());
    let helper_tensors = grid_reduce_helper_tensors(function, validation);
    let d64 = usize::from(super::d64_fault_site(function, validation));
    if forms.is_empty()
        && vector_forms.is_empty()
        && group_axes == 0
        && throws == 0
        && d64 == 0
        && helper_tensors.is_empty()
    {
        return Ok(());
    }
    let word = validation
        .types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
        .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_U32))?;
    let mut tensors: Vec<MirLocalId> = Vec::new();
    for form in &forms {
        if !tensors.contains(&form.tensor) {
            tensors.push(form.tensor);
        }
    }
    for tensor in &helper_tensors {
        if !tensors.contains(tensor) {
            tensors.push(*tensor);
        }
    }
    let mut next_binding = next_free_binding(signature);
    for tensor in tensors {
        let resource = signature
            .resources()
            .find(|resource| {
                resource.kind == MirKernelResourceKind::StorageBuffer
                    && resource.source_local == Some(tensor)
            })
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_TENSOR_EXTENT_OPERAND))?;
        let tensor_binding = resource.binding;
        signature.extra_inputs.push(MirKernelResource {
            group: DEFAULT_KERNEL_RESOURCE_GROUP,
            binding: next_binding,
            kind: MirKernelResourceKind::TensorElementCount { tensor_binding },
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty: MirType::semantic(word),
            element_count: 1,
            source_local: None,
            source_name: None,
            reduced_projection: None,
        });
        next_binding += 1;
    }
    let sites = u32::try_from(
        forms.len() + vector_forms.len() + helper_tensors.len() + group_axes + throws + d64,
    )
    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?;
    signature.extra_inputs.push(MirKernelResource {
        group: DEFAULT_KERNEL_RESOURCE_GROUP,
        binding: next_binding,
        kind: MirKernelResourceKind::FaultRecord { sites },
        role: MirKernelResourceRole::Output,
        access: MirKernelResourceAccess::ReadWrite,
        element_ty: MirType::semantic(word),
        element_count: u64::from(sites),
        source_local: None,
        source_name: None,
        reduced_projection: None,
    });
    Ok(())
}

/// Build the read-input resources for a kernel's params, in binding order,
/// skipping `vacuum` params (the reverse-AD nil placeholder carries no
/// device storage). Returns the resources and the next free binding.
///
/// `element_type`/`element_count` select the element-shape law per kernel
/// family: the result-tuple arm reads buffer views (`buffer_element_*`), the
/// reduction arm reads device views (`device_view_element_*`).
fn input_resources_for_params(
    inputs: &[MirParam],
    validation: &MirValidationContext<'_>,
    interner: Option<&Interner>,
    allow_host_owned_tensor_entry: bool,
    element_type: impl Fn(MirType, &MirValidationContext<'_>, bool) -> Result<MirType, MirAbiError>,
    element_count: impl Fn(MirType, &MirValidationContext<'_>) -> Result<u64, MirAbiError>,
) -> Result<(Vec<MirKernelResource>, u32), MirAbiError> {
    let mut input_resources = Vec::with_capacity(inputs.len());
    let mut next_binding = 0u32;
    for input in inputs {
        if is_vacuum(input.ty, validation) {
            continue;
        }
        input_resources.push(MirKernelResource {
            group: DEFAULT_KERNEL_RESOURCE_GROUP,
            binding: next_binding,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty: element_type(input.ty, validation, allow_host_owned_tensor_entry)?,
            element_count: element_count(input.ty, validation)?,
            source_local: Some(input.local),
            source_name: param_source_name(input, interner),
            reduced_projection: None,
        });
        next_binding += 1;
    }
    Ok((input_resources, next_binding))
}

/// The primary output resource of an indexed-view kernel: the `mut` tensor
/// parameter bound at its declared position.
fn indexed_view_output_resource(
    output: &MirParam,
    binding: u32,
    element_ty: MirType,
    element_count: u64,
    interner: Option<&Interner>,
) -> MirKernelResource {
    MirKernelResource {
        group: DEFAULT_KERNEL_RESOURCE_GROUP,
        binding,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Output,
        access: MirKernelResourceAccess::Write,
        element_ty,
        element_count,
        source_local: Some(output.local),
        source_name: param_source_name(output, interner),
        reduced_projection: None,
    }
}

impl MirKernelSignature {
    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_limit(function, validation, DEFAULT_STORAGE_BUFFER_LIMIT)
    }

    /// Synthesize a storage-buffer kernel ABI against an explicit lane limit.
    ///
    /// The shared default remains the conservative minimum across admitted
    /// lanes. A backend lane that admits a different binding capacity passes
    /// its own [`StorageBufferLimitRecord`] here.
    ///
    /// # Errors
    /// Returns an error when validation, type resolution, or the selected
    /// lane's storage-buffer capacity fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_limit(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        limit: StorageBufferLimitRecord,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_names(function, validation, None, false, limit)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_interner(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_interner_and_limit(
            function,
            validation,
            interner,
            DEFAULT_STORAGE_BUFFER_LIMIT,
        )
    }

    /// Synthesize a named storage-buffer kernel ABI against an explicit lane
    /// limit.
    ///
    /// # Errors
    /// Returns an error when validation, type resolution, or the selected
    /// lane's storage-buffer capacity fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_interner_and_limit(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
        limit: StorageBufferLimitRecord,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_names(function, validation, Some(interner), false, limit)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_interner_for_target_entry(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_interner_for_target_entry_and_limit(
            function,
            validation,
            interner,
            DEFAULT_STORAGE_BUFFER_LIMIT,
        )
    }

    /// Synthesize a target-entry storage-buffer ABI against an explicit lane
    /// limit.
    ///
    /// # Errors
    /// Returns an error when validation, type resolution, or the selected
    /// lane's storage-buffer capacity fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_interner_for_target_entry_and_limit(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
        limit: StorageBufferLimitRecord,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_names(function, validation, Some(interner), true, limit)
    }

    fn storage_buffer_kernel_with_names(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
        limit: StorageBufferLimitRecord,
    ) -> Result<Self, MirAbiError> {
        // Unit B: stable shape before arity matching so matrix kernel IO does not
        // fall through to the generic "one buffer parameter" message.
        reject_kernel_matrix_abi(function, validation)?;

        // S3-A1: a result-tuple return (a non-empty tuple of device views) is
        // the multi-output contract — every param is an input and the tuple
        // elements are DISTINCT output resources. Checked before the
        // single-output arms, which would reject the tuple as an aggregate.
        let signature =
            if !function.params.is_empty() && result_tuple_count(function, validation).is_some() {
                Self::multi_output_return_kernel(
                    function,
                    &function.params,
                    validation,
                    interner,
                    allow_host_owned_tensor_entry,
                )?
            } else if let Some(signature) = Self::scan_return_kernel(
                function,
                validation,
                interner,
                allow_host_owned_tensor_entry,
            )? {
                signature
            } else if let Some(signature) = Self::grouped_grid_reduce_return_kernel(
                function,
                validation,
                interner,
                allow_host_owned_tensor_entry,
            )? {
                signature
            } else if let Some(interner) = interner {
                if let Some(signature) = Self::source_named_indexed_view_kernel(
                    function,
                    validation,
                    interner,
                    allow_host_owned_tensor_entry,
                )? {
                    signature
                } else {
                    Self::fallback_kernel_signature(
                        function,
                        validation,
                        Some(interner),
                        allow_host_owned_tensor_entry,
                    )?
                }
            } else {
                Self::fallback_kernel_signature(
                    function,
                    validation,
                    interner,
                    allow_host_owned_tensor_entry,
                )?
            };
        let mut signature = signature;
        append_scan_scratch_resources(function, &mut signature, validation)?;
        append_partials_scratch_resource(function, &mut signature, validation)?;
        append_tensor_extent_resources(function, &mut signature, validation)?;
        append_fault_resources(function, &mut signature, validation)?;
        // The per-kernel storage-buffer binding maximum (the active
        // per-lane `StorageBufferLimitRecord`; the shared default is the
        // minimum capacity across admitted lanes): a signature over
        // it cannot be emitted as one kernel, so the synthesis refuses it
        // fail-closed — the callers (the faber whole-function path, the
        // emitters' whole-kernel cross-check, the image input resolution)
        // fall through to the decomposition split.
        let buffer_count = signature
            .resources()
            .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
            .count();
        if !limit.admits(buffer_count) {
            return Err(storage_buffer_limit_error(buffer_count, limit));
        }
        Ok(signature)
    }

    /// A scan returns one value per source cell. Its output keeps the source
    /// shape but may have a different element type (count returns numerus;
    /// all/any return bivalens), so it must be selected before the generic
    /// same-element return-buffer rules.
    fn scan_return_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Option<Self>, MirAbiError> {
        let Some((scan, source_local)) = scan_node(function) else {
            return Ok(None);
        };
        let Some(source_param) = function
            .params
            .iter()
            .find(|param| param.local == source_local)
        else {
            return Err(MirAbiError::new(
                "scan source does not resolve to a kernel input",
            ));
        };
        if source_param.mode == MirParamMode::MutRef {
            return Err(MirAbiError::new(
                "scan source must be a read-only kernel input",
            ));
        }
        let source_shape = scan_tensor_shape(source_param.ty, validation)
            .ok_or_else(|| MirAbiError::new("scan source must be a tensor kernel input"))?;
        let result_shape = scan_tensor_shape(function.return_ty, validation)
            .ok_or_else(|| MirAbiError::new("scan return must be a source-shaped tensor"))?;
        if !validation.types.index_equals(source_shape, result_shape) {
            return Err(MirAbiError::new(
                "scan return shape differs from its source",
            ));
        }
        if let Some(rank) = scan_tensor_rank(source_shape, &validation.types)
            && rank != usize::from(scan.rank)
        {
            return Err(MirAbiError::new("scan rank differs from its source tensor"));
        }
        let source_element_count = scan_tensor_element_count(source_param.ty, validation)?;
        let return_element_count = scan_tensor_element_count(function.return_ty, validation)?;
        if source_element_count != return_element_count {
            return Err(MirAbiError::new(
                "scan return element count differs from its source",
            ));
        }
        let return_element_ty = buffer_element_type(
            function.return_ty,
            validation,
            allow_host_owned_tensor_entry,
        )?;
        let mut resources = Vec::new();
        for param in function
            .params
            .iter()
            .filter(|param| param.mode != MirParamMode::Lane)
        {
            if param.mode == MirParamMode::MutRef {
                return Err(MirAbiError::new(
                    "scan kernels require read-only capture parameters",
                ));
            }
            if is_vacuum(param.ty, validation) {
                continue;
            }
            if let Some(genus) = genus_param_layout(param.ty, validation)? {
                resources.push(MirKernelResource {
                    group: DEFAULT_KERNEL_RESOURCE_GROUP,
                    binding: u32::try_from(resources.len())
                        .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                    kind: MirKernelResourceKind::GenusParam {
                        size_bytes: genus.size,
                    },
                    role: MirKernelResourceRole::Input,
                    access: MirKernelResourceAccess::Read,
                    element_ty: param.ty,
                    element_count: 1,
                    source_local: Some(param.local),
                    source_name: param_source_name(param, interner),
                    reduced_projection: None,
                });
                continue;
            }
            let element_ty =
                buffer_element_type(param.ty, validation, allow_host_owned_tensor_entry)?;
            let element_count = if param.local == source_local {
                source_element_count
            } else if Self::is_kernel_scalar_input(param.ty, validation)? {
                buffer_element_count(param.ty, validation)?
            } else {
                device_view_element_count_or_runtime(param.ty, validation)?
            };
            resources.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(resources.len())
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty,
                element_count,
                source_local: Some(param.local),
                source_name: param_source_name(param, interner),
                reduced_projection: None,
            });
        }
        let input = resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let output_binding = u32::try_from(resources.len())
            .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?;
        let extra_inputs = resources.into_iter().skip(1).collect();
        let builtin_inputs = function
            .params
            .iter()
            .filter(|param| param.mode == MirParamMode::Lane)
            .map(|param| MirKernelBuiltinInput {
                local: param.local,
                source_name: param_source_name(param, interner),
                builtin: MirKernelBuiltin::GlobalInvocationX,
                ty: param.ty,
            })
            .collect();
        let mut called_builtins = vec![MirKernelBuiltin::GlobalInvocationX];
        called_builtins.extend(called_builtins_for_function(function));
        Ok(Some(Self {
            function: function.id,
            input,
            extra_inputs,
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: output_binding,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: return_element_ty,
                element_count: return_element_count,
                source_local: None,
                source_name: None,
                reduced_projection: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs,
            called_builtins,
            workgroup: MirWorkgroupSize {
                x: crate::kernel_plan::GRID_REDUCE_WORKGROUP,
                y: 1,
                z: 1,
            },
            dispatch_size: MirKernelDispatchSize {
                x: if source_element_count == 0
                    || source_element_count == RUNTIME_SHAPED_ELEMENT_COUNT
                {
                    1
                } else {
                    source_element_count
                },
                y: 1,
                z: 1,
            },
        }))
    }

    /// A grouped grid reduction returns one value per literal bucket. Its
    /// output extent is the grouped result shape, independent of the reduced
    /// source length, so it uses a fresh output resource instead of the
    /// elementwise equal-count return-buffer contract.
    fn grouped_grid_reduce_return_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Option<Self>, MirAbiError> {
        let Some((filum, _)) = grid_reduce_node(function) else {
            return Ok(None);
        };
        let Some(group) = filum.group.as_ref() else {
            return Ok(None);
        };
        let Type::Tensor(_, result_shape) = validation.types.get(function.return_ty.semantic_id())
        else {
            return Ok(None);
        };
        let returned = radix_types::i64s_from_literal_index(*result_shape, &validation.types)
            .and_then(|shape| {
                shape
                    .into_iter()
                    .map(u64::try_from)
                    .collect::<Result<Vec<_>, _>>()
                    .ok()
            });
        if returned.as_deref() != Some(group.dimensions.as_slice()) {
            return Err(MirAbiError::new(
                "grouped grid reduction return shape differs from its literal bucket shape",
            ));
        }
        let builtin_inputs = function
            .params
            .iter()
            .filter(|param| param.mode == MirParamMode::Lane)
            .map(|param| MirKernelBuiltinInput {
                local: param.local,
                source_name: param_source_name(param, interner),
                builtin: MirKernelBuiltin::GlobalInvocationX,
                ty: param.ty,
            })
            .collect();
        let params: Vec<_> = function
            .params
            .iter()
            .filter(|param| param.mode != MirParamMode::Lane)
            .collect();
        if params.is_empty()
            || params
                .iter()
                .any(|param| param.mode == MirParamMode::MutRef)
        {
            return Err(MirAbiError::new(
                "grouped grid reduction return requires read-only input parameters",
            ));
        }
        let return_element_ty = buffer_element_type(
            function.return_ty,
            validation,
            allow_host_owned_tensor_entry,
        )?;
        let return_element_count = buffer_element_count(function.return_ty, validation)?;
        let mut inputs = Vec::with_capacity(params.len());
        for (binding, param) in params.iter().enumerate() {
            let element_ty =
                buffer_element_type(param.ty, validation, allow_host_owned_tensor_entry)?;
            let element_count = buffer_element_count(param.ty, validation)?;
            inputs.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(binding)
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty,
                element_count,
                source_local: Some(param.local),
                source_name: param_source_name(param, interner),
                reduced_projection: None,
            });
        }
        let input = inputs
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let extra_inputs = inputs.into_iter().skip(1).collect();
        let output_binding = u32::try_from(params.len())
            .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?;
        Ok(Some(Self {
            function: function.id,
            input,
            extra_inputs,
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: output_binding,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: return_element_ty,
                element_count: return_element_count,
                source_local: None,
                source_name: None,
                reduced_projection: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs,
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize {
                x: crate::kernel_plan::GRID_REDUCE_WORKGROUP,
                y: 1,
                z: 1,
            },
            dispatch_size: MirKernelDispatchSize {
                x: return_element_count,
                y: 1,
                z: 1,
            },
        }))
    }

    /// The single-input / multi-input / indexed-view kernel shape dispatch —
    /// the non-tuple, non-named-param arms of the storage-buffer synthesis.
    fn fallback_kernel_signature(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        // S3-A5: a kernel whose body carries a Reduction contract and whose
        // return is a scalar (the mean/sum result) synthesizes the
        // indexed-view reduction signature WITHOUT explicit output/index
        // parameters: every tensor param becomes a read input and the output
        // is the per-workgroup partials buffer from the resolved contract.
        // The scalar return is the host-combined reduction result; the
        // emitter's reduction arm writes the per-workgroup partials (the
        // S3-B1 mul-mean forward `simple_loss(x, w) → f32` shape).
        if let Some(signature) = Self::reduction_scalar_return_kernel(
            function,
            validation,
            interner,
            allow_host_owned_tensor_entry,
        )? {
            return Ok(signature);
        }

        if Self::is_single_lane_form(function, validation)
            || Self::is_elementwise_output_form(function, validation)
        {
            let output_position = function
                .params
                .iter()
                .position(|param| param.mode == MirParamMode::MutRef)
                .ok_or_else(|| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?;
            return Self::single_lane_kernel(
                function,
                output_position,
                IndexedViewKernelContext {
                    validation,
                    builtin_inputs: Vec::new(),
                    interner,
                    allow_host_owned_tensor_entry,
                    output_position: u32::try_from(output_position)
                        .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                    extra_output_positions: Vec::new(),
                },
            );
        }

        match function.params.as_slice() {
            [param] => Self::return_buffer_kernel(
                function,
                validation,
                param,
                interner,
                allow_host_owned_tensor_entry,
            ),
            inputs if !inputs.is_empty() && !is_vacuum(function.return_ty, validation) => {
                Self::multi_input_return_buffer_kernel(
                    function,
                    validation,
                    inputs,
                    interner,
                    allow_host_owned_tensor_entry,
                )
            }
            [inputs @ .., output, index] if !inputs.is_empty() => {
                let context = IndexedViewKernelContext {
                    validation,
                    builtin_inputs: vec![builtin_input_for_index_param(index, interner)],
                    interner,
                    allow_host_owned_tensor_entry,
                    output_position: u32::try_from(inputs.len())
                        .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                    extra_output_positions: Vec::new(),
                };
                if let [input] = inputs {
                    Self::indexed_view_kernel(function, input, output, index, context)
                } else {
                    Self::multi_input_indexed_view_kernel(
                        function,
                        inputs,
                        output,
                        &[],
                        Some(index),
                        context,
                    )
                }
            }
            _ => Err(MirAbiError::new(
                "kernel must have either one buffer parameter or input*/output/index parameters",
            )),
        }
    }

    fn source_named_indexed_view_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Option<Self>, MirAbiError> {
        let params = function.params.as_slice();
        if params.len() < 3 {
            return Ok(None);
        }

        let mut builtin_start = params.len();
        while builtin_start > 0
            && (params[builtin_start - 1].mode == MirParamMode::Lane
                || named_builtin_for_param(&params[builtin_start - 1], interner).is_some())
        {
            builtin_start -= 1;
        }
        if builtin_start == params.len() {
            return Ok(None);
        }
        if builtin_start < 2 {
            return Err(MirAbiError::new(
                "indexed view kernel requires input/output parameters before builtin parameters",
            ));
        }

        // The output is the `mut` tensor parameter, wherever it sits among
        // the non-lane parameters; a kernel with no `mut` parameter keeps the
        // legacy positional rule (the last one). Scalars and input tensors may
        // appear before or after it, in any number and interleaving.
        let declared = &params[..builtin_start];
        let mut_positions: Vec<usize> = declared
            .iter()
            .enumerate()
            .filter(|(_, param)| param.mode == MirParamMode::MutRef)
            .map(|(position, _)| position)
            .collect();
        // A lane-form kernel may declare several `mut` outputs: the first is
        // the output and the rest are `extra_outputs`, each written at its own
        // lane coordinates. Any other shape keeps the one-output rule.
        let (output_position, extra_positions): (usize, Vec<usize>) = match mut_positions.as_slice()
        {
            [] => (declared.len() - 1, Vec::new()),
            [position] => (*position, Vec::new()),
            [first, rest @ ..] if is_lane_form(function) => (*first, rest.to_vec()),
            _ => {
                return Err(MirAbiError::new(
                    "indexed view kernel declares more than one `mut` output parameter",
                ));
            }
        };
        let output = &declared[output_position];
        let inputs: Vec<MirParam> = declared
            .iter()
            .enumerate()
            .filter(|(position, _)| {
                *position != output_position && !extra_positions.contains(position)
            })
            .map(|(_, param)| param.clone())
            .collect();
        let extra_outputs: Vec<(u32, MirParam)> = extra_positions
            .iter()
            .map(|position| {
                Ok((
                    u32::try_from(*position)
                        .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                    declared[*position].clone(),
                ))
            })
            .collect::<Result<_, MirAbiError>>()?;
        let builtin_params = &params[builtin_start..];
        let builtin_inputs = builtin_inputs_for_named_params(builtin_params, validation, interner)?;
        let context = IndexedViewKernelContext {
            validation,
            builtin_inputs,
            interner: Some(interner),
            allow_host_owned_tensor_entry,
            output_position: u32::try_from(output_position)
                .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
            extra_output_positions: extra_outputs
                .iter()
                .map(|(position, _)| *position)
                .collect(),
        };
        // The single positional-input arm reads its input as a tensor device
        // view: a lone scalar input (`seed(mut w, i32 salt)`) is a count-one
        // resource and takes the multi-input arm like every scalar beside
        // tensors.
        let single_tensor_input = match inputs.as_slice() {
            [input]
                if extra_outputs.is_empty()
                    && genus_param_layout(input.ty, validation)?.is_none()
                    && !Self::is_kernel_scalar_input(input.ty, validation)? =>
            {
                Some(input)
            }
            _ => None,
        };
        if let Some(input) = single_tensor_input {
            Self::indexed_view_kernel(function, input, output, &builtin_params[0], context)
                .map(Some)
        } else {
            Self::multi_input_indexed_view_kernel(
                function,
                &inputs,
                output,
                &extra_outputs,
                Some(&builtin_params[0]),
                context,
            )
            .map(Some)
        }
    }

    /// Synthesize the storage-buffer ABI for a **result-tuple** kernel
    /// (S3-A1): a function whose return type is a non-empty tuple of device
    /// views. Every param becomes a read input buffer; the tuple's elements
    /// become DISTINCT output resources — one per tuple element, in tuple
    /// order, each bound to its own slot (the first element is `output`, the
    /// rest `extra_outputs`).
    ///
    /// This is the device ABI for a backward companion's `(grad_x, grad_w)`
    /// tuple: N gradient outputs bind to N distinct output slots, and no
    /// kernel value carries an aggregate type — backend aggregate rejection
    /// is unreachable through this ABI because tuple outputs are separate
    /// resources, never an aggregate.
    fn multi_output_return_kernel(
        function: &MirFunction,
        inputs: &[MirParam],
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        let Type::Tuple(elements) = validation.types.get(function.return_ty.semantic_id()) else {
            return Err(MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRED));
        };
        if elements.is_empty() {
            return Err(MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRED));
        }

        // Every non-vacuum param becomes a read input; a vacuum "residual"
        // param (the reverse-AD nil placeholder) carries no device storage
        // and is skipped (S3-A2 companion shape: `backward(x, w, nil(),
        // upstream) → (grad_x, grad_w)`). Output bindings start after the
        // inputs, so each output binds a distinct slot.
        let (input_resources, next_binding) = input_resources_for_params(
            inputs,
            validation,
            interner,
            allow_host_owned_tensor_entry,
            buffer_element_type,
            buffer_element_count,
        )?;
        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();

        // Tuple elements → distinct output resources (first → `output`).
        // The output resources carry the return-tuple construct's field
        // locals as their `MirLocal` origin (the S5-U1 forward-save
        // identity): a decomposed source's subchain synthetics share the
        // source local table, so the (function, local) identity fact lets
        // the faber cross-kernel unification join a producer subchain's
        // output slot with a consumer subchain's input slot for the SAME
        // local — without which an elementwise split's intermediate
        // outputs would mint distinct PerProgram input buffers
        // (uninitialized reads / missing once-init values).
        let return_fields = Self::return_tuple_field_locals(function);
        let mut outputs = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            let element_ty = MirType::semantic(*element);
            let view_ty =
                device_view_element_type(element_ty, validation, allow_host_owned_tensor_entry)?;
            let view_count = device_view_element_count(element_ty, validation)?;
            outputs.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: next_binding
                    .checked_add(u32::try_from(index).unwrap_or(u32::MAX))
                    .ok_or_else(|| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: view_ty,
                element_count: view_count,
                source_local: return_fields
                    .as_ref()
                    .and_then(|fields| fields.get(index).copied()),
                source_name: None,
                reduced_projection: None,
            });
        }
        let output = outputs.remove(0);
        let extra_outputs = outputs;
        // R6 (P1): the dispatch must cover EVERY output's extent — the union
        // (max) over the per-output extents, never the first output's count
        // alone. A first-output-only dispatch either overruns a smaller
        // buffer or leaves a larger one incomplete; each output's write is
        // guarded by its own extent under the union (kernel_plan).
        let dispatch_count = extra_outputs
            .iter()
            .fold(output.element_count, |union, extra| {
                union.max(extra.element_count)
            });

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output,
            extra_outputs,
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(dispatch_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        Ok(signature)
    }

    /// The ordered field locals of a kernel's return-tuple `Construct` — the
    /// locals the result-tuple binds at full-tuple index `i` (the
    /// local-return and the temp-return shapes both resolve). `None` when
    /// the function does not return an ordered non-spread tuple of plain
    /// local places. The multi-output ABI carries these as the output
    /// resources' `MirLocal` origins (the S5-U1 forward-save identity), so
    /// the decomposition subchains' cross-kernel unification joins a
    /// producer and a consumer slot for the same local.
    fn return_tuple_field_locals(function: &MirFunction) -> Option<Vec<MirLocalId>> {
        if function.blocks.len() != 1 {
            return None;
        }
        let block = &function.blocks[0];
        let MirTerminatorKind::Return(Some(operand)) = &block.terminator.kind else {
            return None;
        };
        let base = match operand {
            MirOperand::Place(place) => place.base.clone(),
            MirOperand::Temp(temp) => MirPlaceBase::Temp(*temp),
            _ => return None,
        };
        let destination_matches = |place: &MirPlace| place.base == base;
        block.statements.iter().find_map(|statement| {
            let MirStatementKind::Construct {
                destination,
                aggregate,
            } = &statement.kind
            else {
                return None;
            };
            if !destination_matches(destination) {
                return None;
            }
            if aggregate.kind != MirAggregateKind::Tuple {
                return None;
            }
            let MirAggregateFields::Ordered(items) = &aggregate.fields else {
                return None;
            };
            items
                .iter()
                .map(|item| match item {
                    MirAggregateItem::Operand(MirOperand::Place(place)) => {
                        crate::read_analysis::place_local_base(place)
                    }
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
        })
    }

    /// S3-A5: synthesize the indexed-view reduction signature for a kernel
    /// whose body carries a Reduction contract and whose return is a scalar
    /// (the mean/sum result — the S3-B1 forward shape
    /// `simple_loss(x, w) → f32` with `mean(mul(x, w))`).
    ///
    /// Every non-vacuum tensor param becomes a read input; the output is the
    /// per-workgroup partials buffer from the resolved contract (count =
    /// `partials`), and the workgroup/dispatch come from the contract. The
    /// emitter's reduction arm writes each workgroup's partial there; the
    /// scalar return is the host-combined reduction result. `None` when the
    /// body carries no Reduction contract or the return is not a scalar.
    fn reduction_scalar_return_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Option<Self>, MirAbiError> {
        // The lane form never takes the host-combined partials ABI: a grid
        // reduction combines through the dispatcher-owned scratch instead.
        if is_lane_form(function) {
            return Ok(None);
        }
        let Some(contract) = collection_op_contract(function, validation)? else {
            return Ok(None);
        };
        let CollectionOpContract::Reduction {
            length,
            workgroup_x,
            partials,
        } = contract
        else {
            return Ok(None);
        };
        // The return must be a scalar (the reduction result) — tensor and
        // tuple returns have their own signature arms.
        if !matches!(
            validation.layouts.layout_for_type(function.return_ty),
            Ok(layout) if matches!(layout.kind, MirLayoutKind::Scalar(_))
        ) {
            return Ok(None);
        }

        let (input_resources, next_binding) = input_resources_for_params(
            &function.params,
            validation,
            interner,
            allow_host_owned_tensor_entry,
            device_view_element_type,
            device_view_element_count,
        )?;
        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();
        let reduction_element_ty = input.element_ty;

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: next_binding,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: reduction_element_ty,
                element_count: partials,
                source_local: None,
                source_name: None,
                reduced_projection: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            // The reduction recipe's grid-stride loop starts from the GLOBAL
            // thread index (the header's first builtin selects the index
            // expression); the tree + partial write add `local_id.x`/
            // `workgroup_id.x` via the emitter's C5 builtins.
            called_builtins: {
                let mut builtins = vec![MirKernelBuiltin::GlobalInvocationX];
                builtins.extend(called_builtins_for_function(function));
                builtins
            },
            workgroup: MirWorkgroupSize {
                x: workgroup_x,
                y: 1,
                z: 1,
            },
            dispatch_size: MirKernelDispatchSize {
                x: length,
                y: 1,
                z: 1,
            },
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        Ok(Some(signature))
    }

    fn return_buffer_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        param: &MirParam,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        // The strict semantic-type match is the plain-elementwise law. A
        // collection-op kernel remaps the shape by contract (Transpose
        // `[M,N] → [N,M]`), so for those the flat element-count equality
        // below carries the binding shape and
        // `apply_collection_op_contract` supplies the dispatch facts.
        if !kernel_has_collection_op(function, |_| true)
            && !validation
                .types
                .equals(param.ty.semantic_id(), function.return_ty.semantic_id())
        {
            return Err(MirAbiError::new(ERR_KERNEL_PARAM_RETURN_MISMATCH));
        }

        let param_element_ty =
            buffer_element_type(param.ty, validation, allow_host_owned_tensor_entry)?;
        let return_element_ty = buffer_element_type(
            function.return_ty,
            validation,
            allow_host_owned_tensor_entry,
        )?;
        require_same_element_type(param_element_ty, return_element_ty, validation)?;
        let param_element_count = buffer_element_count(param.ty, validation)?;
        let return_element_count = buffer_element_count(function.return_ty, validation)?;
        // The return-buffer equality law (shared with the multi-input arm):
        // the input and output buffers are flat views of the same element
        // count, so a host can assume one elementwise shape across bindings.
        // A rectangular transpose's `[M,N]` and `[N,M]` shapes both hold the
        // flat `M·N` count, so it passes here; genuinely size-changing
        // kernels fail closed with the elementwise-only diagnostic.
        if param_element_count != return_element_count {
            return Err(MirAbiError::new(ERR_KERNEL_IO_ELEMENT_MISMATCH));
        }

        let mut signature = Self {
            function: function.id,
            input: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: 0,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: param_element_ty,
                element_count: param_element_count,
                source_local: Some(param.local),
                source_name: param_source_name(param, interner),
                reduced_projection: None,
            },
            extra_inputs: Vec::new(),
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: 1,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: return_element_ty,
                element_count: return_element_count,
                source_local: None,
                source_name: None,
                reduced_projection: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(return_element_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): collection-op kernels (Transpose, Reduction,
        // MatMul) carry their dispatch/workgroup/output-buffer contract even
        // through the single-input return-buffer arm (shared ABI law, all
        // compute targets). A rank-2 transpose `[M,N] → [N,M]` keeps the
        // flat `M·N` element counts but must advertise the transpose
        // workgroup/dispatch contract, not the generic 1D one.
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    /// Synthesize the storage-buffer ABI for a multi-input device kernel
    /// entry (C3): every param becomes a read input buffer and the tensor
    /// return becomes the write output buffer.
    ///
    /// Shared ABI law for every compute lane: each backend consumes this
    /// resource shape through its own syntax. Equal element counts across
    /// every input and the return are enforced with the same equality law the
    /// single-input return-buffer path uses, so a host can assume one
    /// elementwise shape across bindings.
    fn multi_input_return_buffer_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        inputs: &[MirParam],
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        let return_element_ty = buffer_element_type(
            function.return_ty,
            validation,
            allow_host_owned_tensor_entry,
        )?;
        let return_element_count = buffer_element_count(function.return_ty, validation)?;

        let mut input_resources = Vec::with_capacity(inputs.len());
        for (binding, input) in inputs.iter().enumerate() {
            let vector = vector_param_layout(input.ty, validation)?;
            let (input_element_ty, input_element_count) = if vector.is_some() {
                // A by-value vector parameter (`vf32[4]`) is a count-one
                // input resource whose element is the vector itself: the
                // reflection resolves its lane scalar and packed byte width,
                // and the emitters declare it as one constant/uniform value
                // (`float4` on Metal). It does not participate in the tensor
                // elementwise equality law — its value is supplied once and
                // read lane-wise by the kernel body, like a scalar.
                (input.ty, 1)
            } else {
                (
                    buffer_element_type(input.ty, validation, allow_host_owned_tensor_entry)?,
                    buffer_element_count(input.ty, validation)?,
                )
            };
            // A device-safe scalar is a count-one input resource. It does
            // not participate in the tensor elementwise equality law: its
            // value is supplied once and broadcast by the kernel body.
            if vector.is_none() && !Self::is_kernel_scalar_input(input.ty, validation)? {
                require_same_element_type(input_element_ty, return_element_ty, validation)?;
                if input_element_count != return_element_count {
                    return Err(MirAbiError::new(ERR_KERNEL_IO_ELEMENT_MISMATCH));
                }
            }
            input_resources.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(binding)
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: input_element_ty,
                element_count: input_element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, interner),
                reduced_projection: None,
            });
        }

        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(inputs.len())
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: return_element_ty,
                element_count: return_element_count,
                source_local: None,
                source_name: None,
                reduced_projection: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(return_element_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): collection-op kernels carry their contract
        // through this arm too — the dispatch/workgroup/output-buffer shape
        // is a property of the resolved collection op, not of the arity that
        // picked the ABI arm (shared ABI law, all compute targets).
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    /// True when `ty` is a device-safe scalar represented as a count-one
    /// storage resource rather than a tensor device view.
    fn is_kernel_scalar_input(
        ty: MirType,
        validation: &MirValidationContext<'_>,
    ) -> Result<bool, MirAbiError> {
        let layout = validation
            .layouts
            .layout_for_type(ty)
            .map_err(|error| MirAbiError::new(error.message))?;
        Ok(matches!(
            layout.kind,
            MirLayoutKind::Scalar(scalar) if crate::tensor_type::is_kernel_element_layout(scalar)
        ))
    }

    fn indexed_view_kernel(
        function: &MirFunction,
        input: &MirParam,
        output: &MirParam,
        index: &MirParam,
        context: IndexedViewKernelContext<'_>,
    ) -> Result<Self, MirAbiError> {
        let validation = context.validation;
        if !is_vacuum(function.return_ty, validation) {
            return Err(MirAbiError::new(ERR_INDEXED_VIEW_RETURNS_VACUUM));
        }
        require_u32_index(index.ty, validation)?;

        let input_element_ty =
            device_view_element_type(input.ty, validation, context.allow_host_owned_tensor_entry)?;
        let output_element_ty =
            device_view_element_type(output.ty, validation, context.allow_host_owned_tensor_entry)?;
        if !is_lane_form(function) {
            require_indexed_element_compatibility(
                function,
                input_element_ty,
                output_element_ty,
                validation,
                true,
            )?;
        }
        let input_element_count = device_view_element_count_or_runtime(input.ty, validation)?;
        let output_element_count = device_view_element_count_or_runtime(output.ty, validation)?;

        let mut signature = Self {
            function: function.id,
            input: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: input_binding(0, context.output_position, &[])?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: input_element_ty,
                element_count: input_element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, context.interner),
                reduced_projection: None,
            },
            extra_inputs: Vec::new(),
            output: indexed_view_output_resource(
                output,
                context.output_position,
                output_element_ty,
                output_element_count,
                context.interner,
            ),
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: context.builtin_inputs,
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(
                if output_element_count == super::RUNTIME_SHAPED_ELEMENT_COUNT {
                    1
                } else {
                    output_element_count
                },
            ),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): reduction/matmul kernels carry a corrected
        // dispatch/workgroup/output-buffer contract (shared ABI law, all
        // compute targets).
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    fn multi_input_indexed_view_kernel(
        function: &MirFunction,
        inputs: &[MirParam],
        output: &MirParam,
        extra_outputs: &[(u32, MirParam)],
        index: Option<&MirParam>,
        context: IndexedViewKernelContext<'_>,
    ) -> Result<Self, MirAbiError> {
        let validation = context.validation;
        if !is_vacuum(function.return_ty, validation) {
            return Err(MirAbiError::new(ERR_INDEXED_VIEW_RETURNS_VACUUM));
        }
        // No index parameter: a single-lane kernel, whose tensors are read and
        // written at the body's own indices exactly like a lane-form kernel's.
        if let Some(index) = index {
            require_u32_index(index.ty, validation)?;
        }
        // An elementwise output-parameter kernel has no index parameter either,
        // but it is not a single lane: it runs per element.
        let elementwise_output = index.is_none() && body_has_tensor_elementwise(function);
        let per_lane = (index.is_none() && !elementwise_output) || is_lane_form(function);

        let output_element_ty =
            device_view_element_type(output.ty, validation, context.allow_host_owned_tensor_entry)?;
        let output_element_count = device_view_element_count_or_runtime(output.ty, validation)?;
        let input_resources = Self::multi_input_resources(
            inputs,
            function,
            validation,
            per_lane,
            output_element_ty,
            &context,
        )?;

        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();

        // Further `mut` outputs of a lane-form kernel, each bound at its
        // declared position. The dispatch covers the largest output, so no
        // output's per-output domain is under-filled.
        let mut extra_output_resources = Vec::with_capacity(extra_outputs.len());
        let mut dispatch_count = output_element_count;
        for (position, extra) in extra_outputs {
            let element_ty = device_view_element_type(
                extra.ty,
                validation,
                context.allow_host_owned_tensor_entry,
            )?;
            let element_count = device_view_element_count_or_runtime(extra.ty, validation)?;
            if dispatch_count == super::RUNTIME_SHAPED_ELEMENT_COUNT
                || (element_count != super::RUNTIME_SHAPED_ELEMENT_COUNT
                    && element_count > dispatch_count)
            {
                dispatch_count = element_count;
            }
            extra_output_resources.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: *position,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty,
                element_count,
                source_local: Some(extra.local),
                source_name: param_source_name(extra, context.interner),
                reduced_projection: None,
            });
        }

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output: indexed_view_output_resource(
                output,
                context.output_position,
                output_element_ty,
                output_element_count,
                context.interner,
            ),
            extra_outputs: extra_output_resources,
            accumulation_buffers: Vec::new(),
            builtin_inputs: context.builtin_inputs,
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(
                if dispatch_count == super::RUNTIME_SHAPED_ELEMENT_COUNT {
                    1
                } else {
                    dispatch_count
                },
            ),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        if index.is_none() && !elementwise_output {
            // Single lane: one thread runs the whole body. No recipe contract
            // applies; the body's collection-op statements are scalar math.
            signature.dispatch_size = MirKernelDispatchSize { x: 1, y: 1, z: 1 };
            return Ok(signature);
        }
        // PRP-004 U3 (D1-A): reduction/matmul kernels carry a corrected
        // dispatch/workgroup/output-buffer contract (shared ABI law, all
        // compute targets).
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }
    fn multi_input_resources(
        inputs: &[MirParam],
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        per_lane: bool,
        output_element_ty: MirType,
        context: &IndexedViewKernelContext<'_>,
    ) -> Result<Vec<MirKernelResource>, MirAbiError> {
        let mut input_resources = Vec::new();
        for (binding, input) in inputs.iter().enumerate() {
            let genus = genus_param_layout(input.ty, validation)?;
            let (input_element_ty, input_element_count) = if genus.is_some() {
                // C4-6b: a genus parameter is a count-one resource whose
                // element is the struct itself; its layout travels beside the
                // artifact. It does not take part in tensor compatibility.
                (input.ty, 1)
            } else if Self::is_kernel_scalar_input(input.ty, validation)? {
                // Indexed kernels may carry explicit scalar resources beside
                // their device-view inputs. The count-one resource is bound
                // and reflected like every other input; only tensor inputs
                // must satisfy indexed-view compatibility with the output.
                (
                    buffer_element_type(
                        input.ty,
                        validation,
                        context.allow_host_owned_tensor_entry,
                    )?,
                    buffer_element_count(input.ty, validation)?,
                )
            } else {
                let input_element_ty = device_view_element_type(
                    input.ty,
                    validation,
                    context.allow_host_owned_tensor_entry,
                )?;
                // A lane-form kernel indexes each tensor by its own lane
                // coordinates, so inputs and the output need not share an
                // element type or count.
                if !per_lane && !is_gather_ids_param(function, input, validation) {
                    require_indexed_element_compatibility(
                        function,
                        input_element_ty,
                        output_element_ty,
                        validation,
                        binding == 0,
                    )?;
                }
                let input_element_count =
                    device_view_element_count_or_runtime(input.ty, validation)?;
                (input_element_ty, input_element_count)
            };
            input_resources.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: input_binding(
                    binding,
                    context.output_position,
                    &context.extra_output_positions,
                )?,
                kind: genus.map_or(MirKernelResourceKind::StorageBuffer, |layout| {
                    MirKernelResourceKind::GenusParam {
                        size_bytes: layout.size,
                    }
                }),
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: input_element_ty,
                element_count: input_element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, context.interner),
                reduced_projection: None,
            });
        }
        Ok(input_resources)
    }

    /// Whether `function` is a single-lane kernel (gpu-reset A2 rung 3): a
    /// void compute function with no `Lane` builtin parameter and no index
    /// parameter, one `mut` tensor parameter (the output, which the body may
    /// also read) and at least one other parameter. Such a body has no
    /// parallel loop, so it is sequential and runs as exactly one lane.
    /// Positional `input*/output/index` kernels (a trailing `u32`) and every
    /// shape with a result are not single-lane.
    #[must_use]
    pub fn is_single_lane_form(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
    ) -> bool {
        Self::is_void_single_output_form(function, validation)
            && !body_has_tensor_elementwise(function)
    }

    /// Whether `function` is an elementwise output-parameter kernel: the
    /// single-lane shape (a void body, no lane loop, no index parameter, one
    /// `mut` tensor, tensor / scalar / genus inputs, no tensor recipe) whose
    /// body is a whole-tensor elementwise statement (`out ⇇ a ⊙ b`,
    /// `out ⇇ w - g * lr`). It is a per-element tensor expression, not a
    /// sequential body: one invocation per output element, the result stored
    /// into the `mut` parameter.
    #[must_use]
    pub fn is_elementwise_output_form(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
    ) -> bool {
        Self::is_void_single_output_form(function, validation)
            && body_has_tensor_elementwise(function)
    }

    fn is_void_single_output_form(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
    ) -> bool {
        if function.params.len() < 2
            || is_lane_form(function)
            || !is_vacuum(function.return_ty, validation)
        {
            return false;
        }
        // A body carrying a tensor recipe (matmul, reduction, softmax, ...)
        // is that recipe's kernel, not a sequential one. The scalar `abs` /
        // `minimum` / `floor` / `sqrt` operations are not recipes.
        if crate::collection_ops::collection_ops_for_function(function)
            .into_iter()
            .any(crate::device_program_plans::is_recipe_op)
        {
            return false;
        }
        let is_tensor = |param: &MirParam| {
            matches!(
                validation.types.get(param.ty.semantic_id()),
                Type::Tensor(..)
            )
        };
        let mut mut_params = function
            .params
            .iter()
            .filter(|param| param.mode == MirParamMode::MutRef);
        let (Some(output), None) = (mut_params.next(), mut_params.next()) else {
            return false;
        };
        is_tensor(output)
            && function.params.iter().all(|param| {
                is_tensor(param)
                    || genus_param_layout(param.ty, validation).is_ok_and(|genus| genus.is_some())
                    || Self::is_kernel_scalar_input(param.ty, validation).is_ok_and(|scalar| {
                        scalar && require_u32_index(param.ty, validation).is_err()
                    })
            })
    }

    /// The single-lane kernel ABI: every parameter in declared order, the
    /// `mut` tensor the output, no builtin inputs, a 1x1x1 dispatch. The
    /// caller passes the `mut` parameter's declared position.
    fn single_lane_kernel(
        function: &MirFunction,
        output_position: usize,
        context: IndexedViewKernelContext<'_>,
    ) -> Result<Self, MirAbiError> {
        let inputs: Vec<MirParam> = function
            .params
            .iter()
            .enumerate()
            .filter(|(position, _)| *position != output_position)
            .map(|(_, param)| param.clone())
            .collect();
        Self::multi_input_indexed_view_kernel(
            function,
            &inputs,
            &function.params[output_position],
            &[],
            None,
            context,
        )
    }

    pub fn input_resources(&self) -> impl Iterator<Item = &MirKernelResource> {
        std::iter::once(&self.input).chain(self.extra_inputs.iter())
    }

    /// S3-A1: attach an accumulation-buffer resource — read-write device
    /// state the kernel accumulates into across the grid. Bound after the
    /// outputs. The companion-aware materializer (S3-A2) attaches these when
    /// partial gradients accumulate; the ABI carries them so the plan surface
    /// and emitters bind them after the result-tuple outputs.
    pub fn push_accumulation_buffer(&mut self, resource: MirKernelResource) {
        self.accumulation_buffers.push(resource);
    }

    /// Every output resource (the first plus the extra tuple elements), in
    /// tuple order — each binds to a distinct slot (S3-A1 result-tuple
    /// contract).
    pub fn output_resources(&self) -> impl Iterator<Item = &MirKernelResource> {
        std::iter::once(&self.output).chain(self.extra_outputs.iter())
    }

    /// All storage-buffer resources in role order: inputs, then the first
    /// output, then the extra tuple outputs, then the accumulation buffers.
    pub fn resources(&self) -> impl Iterator<Item = &MirKernelResource> {
        self.input_resources()
            .chain(std::iter::once(&self.output))
            .chain(self.extra_outputs.iter())
            .chain(self.accumulation_buffers.iter())
    }

    /// [`Self::resources`] ordered by binding index. An indexed-view output
    /// declared before an input binds at its declared position, so slot
    /// consumers (the device program's resource list, the dispatcher's
    /// argument slots) take this order, where slot `i` is parameter `i`.
    pub fn resources_by_binding(&self) -> impl Iterator<Item = &MirKernelResource> {
        let mut resources: Vec<&MirKernelResource> = self.resources().collect();
        resources.sort_by_key(|resource| resource.binding);
        resources.into_iter()
    }

    /// The kernel's runtime-extent count resource, when present (U2).
    ///
    /// Both emitters read the guard limit from this binding and fall back to
    /// the static dispatch-axis limit via [`Self::oob_guard_limit`] when
    /// absent. Returns the first extent resource (CXO F2: `extra_inputs` is
    /// `Vec`-capable; Wave 1 populates at most one).
    #[must_use]
    pub fn runtime_extent(&self) -> Option<&MirKernelResource> {
        self.extra_inputs
            .iter()
            .find(|resource| resource.kind == MirKernelResourceKind::RuntimeExtent)
    }

    #[must_use]
    pub fn header_builtins(&self) -> Vec<MirKernelBuiltin> {
        combined_builtin_dependencies(
            self.builtin_inputs.iter().map(|input| input.builtin),
            &self.called_builtins,
        )
    }

    /// Classify which invocation-header params the signature's builtins
    /// require. Backend-local text mapping stays in each emitter (L2).
    #[must_use]
    pub fn builtin_usage(&self) -> MirKernelBuiltinUsage {
        MirKernelBuiltinUsage::from_builtins(&self.header_builtins())
    }

    /// Return the dispatch-size axis that corresponds to the first invocation
    /// builtin.
    ///
    /// F-036/F-049: the OOB guard must compare against the correct dispatch
    /// axis instead of always using `dispatch_size.x`.
    ///
    /// `first_builtin` is an explicit argument (L1) because a lane may
    /// augment the header builtins with body-required builtins before
    /// selecting the first one; each emitter supplies its selected first
    /// builtin.
    #[must_use]
    pub fn oob_guard_limit(&self, first_builtin: MirKernelBuiltin) -> u32 {
        match first_builtin {
            // WorkgroupSize* are compile-time constants and never drive an OOB
            // guard; fall back to the X axis bound (same as pure-X invocation ids).
            MirKernelBuiltin::GlobalInvocationX
            | MirKernelBuiltin::GlobalInvocationId
            | MirKernelBuiltin::WorkgroupIdX
            | MirKernelBuiltin::WorkgroupId
            | MirKernelBuiltin::LocalInvocationIdX
            | MirKernelBuiltin::LocalInvocationId
            | MirKernelBuiltin::WorkgroupSizeX
            | MirKernelBuiltin::WorkgroupSizeY
            | MirKernelBuiltin::WorkgroupSizeZ => {
                u32::try_from(self.dispatch_size.x).unwrap_or(u32::MAX)
            }
            MirKernelBuiltin::GlobalInvocationY
            | MirKernelBuiltin::WorkgroupIdY
            | MirKernelBuiltin::LocalInvocationIdY => {
                u32::try_from(self.dispatch_size.y).unwrap_or(u32::MAX)
            }
            MirKernelBuiltin::GlobalInvocationZ
            | MirKernelBuiltin::WorkgroupIdZ
            | MirKernelBuiltin::LocalInvocationIdZ => {
                u32::try_from(self.dispatch_size.z).unwrap_or(u32::MAX)
            }
        }
    }

    /// The launched THREAD count on the first invocation builtin's axis —
    /// the bound an OOB early-return guard against a GLOBAL invocation id
    /// must compare with (F-049 successor, ordg).
    ///
    /// The shared dispatch law (`collection_op_workgroup_count`) counts
    /// `dispatch_size` in threads on every contract except the matmul
    /// family, whose axes are workgroup/tile grids — there the coverage is
    /// `workgroup × dispatch_size` per axis, exactly the facts the
    /// one-row dispatch setter knows together. Returning the raw
    /// matmul-family axis truncated real launches (Review 2 A1/A2: a
    /// one-row GEMV kept 60 of 960 columns, poisoning the 0.845 t/s
    /// capture). Non-global builtins keep [`Self::oob_guard_limit`]'s
    /// static axis bound (workgroup-id guards compare against the
    /// workgroup grid, which the matmul-family dispatch already is).
    ///
    /// Returns `None` when the coverage does not fit `u32` — callers must
    /// fail closed instead of emitting a silently truncated guard.
    #[must_use]
    pub fn oob_guard_thread_coverage(
        &self,
        function: &MirFunction,
        first_builtin: MirKernelBuiltin,
    ) -> Option<u32> {
        let (extent, width) = match first_builtin {
            MirKernelBuiltin::GlobalInvocationX | MirKernelBuiltin::GlobalInvocationId => {
                (self.dispatch_size.x, u64::from(self.workgroup.x))
            }
            MirKernelBuiltin::GlobalInvocationY => {
                (self.dispatch_size.y, u64::from(self.workgroup.y))
            }
            MirKernelBuiltin::GlobalInvocationZ => {
                (self.dispatch_size.z, u64::from(self.workgroup.z))
            }
            _ => return Some(self.oob_guard_limit(first_builtin)),
        };
        let matmul_grid = kernel_has_collection_op(function, |op| {
            matches!(
                op,
                crate::MirCollectionOp::TensorMatMul
                    | crate::MirCollectionOp::TensorQuantizedMatMul
                    | crate::MirCollectionOp::TensorGroupedMatMul
            )
        });
        let threads = if matmul_grid {
            extent.checked_mul(width)?
        } else {
            extent
        };
        u32::try_from(threads).ok()
    }
}

/// Whether a body carries a whole-tensor elementwise statement: an elementwise
/// collection op (`TensorAdd`, `TensorMul`, ...), a tensor lift (a scalar
/// operator applied per element, such as `g * lr`), or a whole-tensor store
/// of one tensor operand into the `mut` parameter (`out ⇇ a`).
fn body_has_tensor_elementwise(function: &MirFunction) -> bool {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .any(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. } => match &call.intrinsic {
                MirIntrinsic::Collection(op) => {
                    crate::device_program_plans::is_elementwise_collection_op(*op)
                }
                MirIntrinsic::TensorLift(_) => true,
                _ => false,
            },
            MirStatementKind::Assign { place, value } => {
                place.projections.is_empty()
                    && matches!(place.base, MirPlaceBase::Local(local)
                    if function.params.iter().any(|param| {
                        param.local == local && param.mode == MirParamMode::MutRef
                    }))
                    && matches!(&value.kind, MirValueKind::Operand(MirOperand::Place(source))
                        if source.projections.is_empty())
            }
            _ => false,
        })
}
