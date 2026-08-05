use crate::{
    MirCollectionOp, MirFunction, MirIntrinsic, MirParam, MirRuntimeCall, MirStatementKind,
    MirValidationContext,
};
use radix_lexer::Interner;

use super::{
    device_view_element_count, require_u32_index, MirAbiError, MirKernelBufferBindingType,
    MirKernelBuiltin, MirKernelBuiltinInput, MirKernelDispatchSize, MirKernelResourceAccess,
    MirKernelShaderAccess, MirKernelSignature, MirWorkgroupSize, ERR_KERNEL_BUILTIN_DUPLICATE_NAME,
    ERR_KERNEL_BUILTIN_UNKNOWN_NAME,
};

pub(crate) fn combined_builtin_dependencies(
    input_builtins: impl IntoIterator<Item = MirKernelBuiltin>,
    called_builtins: &[MirKernelBuiltin],
) -> Vec<MirKernelBuiltin> {
    let mut builtins = Vec::new();
    for builtin in input_builtins {
        push_unique_builtin(&mut builtins, builtin);
    }
    for builtin in called_builtins {
        push_unique_builtin(&mut builtins, *builtin);
    }
    builtins
}

pub(crate) fn called_builtins_for_function(function: &MirFunction) -> Vec<MirKernelBuiltin> {
    let mut builtins = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            if let MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::GpuBuiltin(builtin),
                        ..
                    },
                ..
            } = &statement.kind
            {
                push_unique_builtin(&mut builtins, *builtin);
            }
        }
    }
    builtins
}

fn push_unique_builtin(builtins: &mut Vec<MirKernelBuiltin>, builtin: MirKernelBuiltin) {
    if !builtins.contains(&builtin) {
        builtins.push(builtin);
    }
}

/// Corrected dispatch/workgroup/output-buffer contract for collection-op
/// kernels (PRP-004 U3, decision D1-A default).
///
/// The generic indexed-view contract (`workgroup (1,1,1)`, dispatch = output
/// length) cannot drive real multi-workgroup execution:
/// - reduction output length is 1, so the generic contract launches exactly
///   one thread — the emitted kernel would read `buffer[0]` only;
/// - matmul dispatch sized by M·N with `workgroup (1,1,1)` pins `id.y` to 0,
///   so only output row 0 can ever be written.
///
/// The corrected contract:
/// - **reduction (Sum/Mean)**: `dispatch_size.x = n` (the grid covers the
///   whole tensor), power-of-two workgroup `min(256, next_pow2(n))`, output
///   buffer sized to `workgroup_count = ceil(n/ws)` partial slots (one
///   per-workgroup partial; the host combines);
/// - **matmul (`TensorMatMul`)**: `workgroup (8,8,1)` (the emitted tile),
///   `dispatch (ceil(N/8), ceil(M/8), 1)` — one workgroup per output tile;
/// - **transpose (`TensorTranspose`, S5-U1)**: `workgroup
///   (TRANSPOSE_WORKGROUP_X, 1, 1)` (elementwise-style 1D lanes),
///   `dispatch (M·N, 1, 1)` — one thread per output element of the rank-2
///   `[M,N] → [N,M]` remap.
///
/// Consumed by the kernel plan module
/// (`kernel_plan::CollectionKernelPlan::from_contract`) as the dispatch half
/// of the collection-op recipe (council L5: plan and contract stay separate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionOpContract {
    Reduction {
        /// True flat tensor length `n`.
        length: u64,
        /// Power-of-two workgroup lane count.
        workgroup_x: u32,
        /// Partial slot count `ceil(n / ws)`.
        partials: u64,
    },
    MatMul {
        /// Output tile grid `(ceil(N/8), ceil(M/8))`.
        dispatch_x: u64,
        dispatch_y: u64,
    },
    Transpose {
        /// Source rows `M` of the rank-2 operand (output columns).
        m: u64,
        /// Source columns `N` of the rank-2 operand (output rows).
        n: u64,
    },
}

/// Apply the collection-op dispatch contract to a kernel signature in place.
///
/// Non-collection kernels are untouched (elementwise and scalar kernels keep
/// the generic 1D contract).
pub(crate) fn apply_collection_op_contract(
    function: &MirFunction,
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    let Some(contract) = collection_op_contract(function, validation)? else {
        return Ok(());
    };
    match contract {
        CollectionOpContract::Reduction {
            length,
            workgroup_x,
            partials,
        } => {
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
            signature.output.element_count = partials;
        }
        CollectionOpContract::MatMul {
            dispatch_x,
            dispatch_y,
        } => {
            signature.workgroup = MirWorkgroupSize { x: 8, y: 8, z: 1 };
            signature.dispatch_size = MirKernelDispatchSize {
                x: dispatch_x,
                y: dispatch_y,
                z: 1,
            };
        }
        CollectionOpContract::Transpose { m, n } => {
            // Elementwise-style 1D contract: one thread per output element
            // of the rank-2 remap (S5-U1).
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
    }
    Ok(())
}

/// Detect a collection-op kernel and resolve its dispatch contract from the
/// operand MIR types (statement scan analogous to
/// [`called_builtins_for_function`]).
///
/// The plan pass (`kernel_plan::CollectionKernelPlan::from_contract`) consumes
/// the resolved contract as its dispatch half (L5); this resolver stays the
/// single source of truth for the dispatch math.
///
/// # Errors
/// Returns [`MirAbiError`] when an operand type cannot be resolved to a
/// static device-view shape, when the reduction tensor length is zero, when
/// matmul operands are not rank-2 with matching inner dimensions, or when
/// transpose operands are not rank-2 with non-zero dimensions.
pub fn collection_op_contract(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Option<CollectionOpContract>, MirAbiError> {
    for block in &function.blocks {
        for statement in &block.statements {
            let Some(op) = crate::collection_ops::statement_collection_op(statement) else {
                continue;
            };
            let MirStatementKind::RuntimeCall {
                call: MirRuntimeCall { args, .. },
                ..
            } = &statement.kind
            else {
                continue;
            };
            match op {
                MirCollectionOp::Sum | MirCollectionOp::TensorMean => {
                    let Some(operand_ty) = args
                        .first()
                        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
                    else {
                        continue;
                    };
                    let length = device_view_element_count(operand_ty, validation)?;
                    if length == 0 {
                        return Err(MirAbiError::new("reduction tensor length must be non-zero"));
                    }
                    let workgroup_x = reduction_workgroup_x(length);
                    let partials = length.div_ceil(u64::from(workgroup_x));
                    return Ok(Some(CollectionOpContract::Reduction {
                        length,
                        workgroup_x,
                        partials,
                    }));
                }
                MirCollectionOp::TensorMatMul => {
                    let Some(left_ty) = args
                        .first()
                        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
                    else {
                        continue;
                    };
                    let Some(right_ty) = args
                        .get(1)
                        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
                    else {
                        continue;
                    };
                    let left_dims = crate::tensor_type::tensor_literal_dims(left_ty, validation)
                        .map_err(MirAbiError::new)?;
                    let right_dims = crate::tensor_type::tensor_literal_dims(right_ty, validation)
                        .map_err(MirAbiError::new)?;
                    if left_dims.len() != 2 || right_dims.len() != 2 {
                        return Err(MirAbiError::new(
                            "matmul dispatch contract requires rank-2 tensor operands",
                        ));
                    }
                    if left_dims[1] != right_dims[0] {
                        return Err(MirAbiError::new("matmul inner dimensions must match"));
                    }
                    let tile: u64 = 8;
                    return Ok(Some(CollectionOpContract::MatMul {
                        dispatch_x: right_dims[1].div_ceil(tile),
                        dispatch_y: left_dims[0].div_ceil(tile),
                    }));
                }
                MirCollectionOp::TensorTranspose => {
                    let Some(operand_ty) = args
                        .first()
                        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
                    else {
                        continue;
                    };
                    let dims = crate::tensor_type::tensor_literal_dims(operand_ty, validation)
                        .map_err(MirAbiError::new)?;
                    if dims.len() != 2 {
                        return Err(MirAbiError::new(
                            "transpose dispatch contract requires a rank-2 tensor operand",
                        ));
                    }
                    if dims[0] == 0 || dims[1] == 0 {
                        return Err(MirAbiError::new(
                            "transpose dispatch contract requires non-zero dimensions",
                        ));
                    }
                    return Ok(Some(CollectionOpContract::Transpose {
                        m: dims[0],
                        n: dims[1],
                    }));
                }
                _ => {}
            }
        }
    }
    Ok(None)
}

/// Power-of-two workgroup lane count for a reduction: `min(256, next_pow2(n))`
/// (D1-A default).
fn reduction_workgroup_x(length: u64) -> u32 {
    let next = if length <= 1 {
        1
    } else {
        length.next_power_of_two()
    };
    u32::try_from(next.min(256)).unwrap_or(256)
}

pub(crate) fn builtin_input_for_index_param(
    index: &MirParam,
    interner: Option<&Interner>,
) -> MirKernelBuiltinInput {
    MirKernelBuiltinInput {
        local: index.local,
        source_name: param_source_name(index, interner),
        builtin: interner
            .and_then(|interner| named_builtin_for_param(index, interner))
            .unwrap_or(MirKernelBuiltin::GlobalInvocationX),
        ty: index.ty,
    }
}

pub(crate) fn builtin_inputs_for_named_params(
    params: &[MirParam],
    validation: &MirValidationContext<'_>,
    interner: &Interner,
) -> Result<Vec<MirKernelBuiltinInput>, MirAbiError> {
    let mut inputs = Vec::new();
    for param in params {
        require_u32_index(param.ty, validation)?;
        let builtin = named_builtin_for_param(param, interner)
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_BUILTIN_UNKNOWN_NAME))?;
        if inputs
            .iter()
            .any(|input: &MirKernelBuiltinInput| input.builtin == builtin)
        {
            return Err(MirAbiError::new(ERR_KERNEL_BUILTIN_DUPLICATE_NAME));
        }
        inputs.push(MirKernelBuiltinInput {
            local: param.local,
            source_name: param_source_name(param, Some(interner)),
            builtin,
            ty: param.ty,
        });
    }
    Ok(inputs)
}

pub(crate) fn shader_access_for_resource(access: MirKernelResourceAccess) -> MirKernelShaderAccess {
    match access {
        MirKernelResourceAccess::Read => MirKernelShaderAccess::Read,
        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite => {
            MirKernelShaderAccess::ReadWrite
        }
    }
}

pub(crate) fn buffer_binding_type_for_shader_access(
    shader_access: MirKernelShaderAccess,
) -> MirKernelBufferBindingType {
    match shader_access {
        MirKernelShaderAccess::Read => MirKernelBufferBindingType::ReadOnlyStorage,
        MirKernelShaderAccess::ReadWrite => MirKernelBufferBindingType::Storage,
    }
}

pub(crate) fn param_source_name(param: &MirParam, interner: Option<&Interner>) -> Option<String> {
    // A synthetic symbol outside the interner's string table (the reverse-AD
    // residual/upstream placeholders of a generated companion, S3-A2) is
    // treated as unnamed rather than indexing out of bounds.
    let name = param.name?;
    let interner = interner?;
    if (name.0 as usize) < interner.strings().len() {
        Some(interner.resolve(name).to_owned())
    } else {
        None
    }
}

pub(crate) fn named_builtin_for_param(
    param: &MirParam,
    interner: &Interner,
) -> Option<MirKernelBuiltin> {
    param
        .name
        .and_then(|name| MirKernelBuiltin::from_spelling(interner.resolve(name)))
}
