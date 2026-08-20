use crate::{
    MirCollectionOp, MirConstant, MirFunction, MirIntrinsic, MirOperand, MirParam, MirRuntimeCall,
    MirStatementKind, MirValidationContext,
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
    /// Packed quantized GEMM (R-PACK-01A). Same tile grid as [`Self::MatMul`];
    /// the layout class is a typed GGML type-id fact.
    QuantizedMatMul {
        /// Output tile grid `(ceil(N/8), ceil(M/8))`.
        dispatch_x: u64,
        dispatch_y: u64,
        /// Left matrix rows `M`.
        m: u64,
        /// Contracted inner dimension `K`.
        k: u64,
        /// Right matrix columns `N`.
        n: u64,
        /// Stored weight layout.
        layout: PackedStorageLayout,
    },
    /// Grouped packed GEMM over rank-3 expert slices (R-PACK-01A).
    GroupedMatMul {
        /// Output tile grid `(ceil(N/8), ceil(M/8))`.
        dispatch_x: u64,
        dispatch_y: u64,
        /// Left matrix rows `M`.
        m: u64,
        /// Contracted inner dimension `K`.
        k: u64,
        /// Right matrix columns `N`.
        n: u64,
        /// Expert / group count (leading rank-3 axis).
        groups: u64,
        /// Stored expert-weight layout.
        layout: PackedStorageLayout,
    },
    /// M3 head-batched attention. The operation uses explicit group/head
    /// axes and a flat output dispatch; the plan carries typed strides and
    /// the sequence-major context fact consumed by the Metal materializer.
    BatchedAttention {
        /// Operation family encoded by the rank/shape contract.
        kind: crate::kernel_plan::BatchedAttentionKind,
        /// Number of KV groups.
        kv_heads: u64,
        /// Query heads per KV group.
        q_per_kv: u64,
        /// Sequence extent.
        seq: u64,
        /// Per-head feature width.
        head_dim: u64,
        /// Hidden-state width.
        hidden: u64,
        /// Packed projection layout, when this is a projection.
        layout: Option<PackedStorageLayout>,
        /// Whether the projection includes the Qwen rotate-half rope
        /// mapping. The mode is a carried fact so the emitter never guesses
        /// from a model name.
        rotate_half: bool,
        /// IEEE-754 scale bits for grouped scores.
        scale_bits: u32,
        /// Flat output element count.
        output_elements: u64,
    },
    /// SSM conv1d (R-PACK-01A). 1D elementwise-style dispatch over
    /// `length · channels`.
    SsmConv1d {
        /// Sequence length.
        length: u64,
        /// Channel extent.
        channels: u64,
        /// Convolution kernel width.
        kernel_width: u64,
        /// Stored kernel/state layout.
        layout: PackedStorageLayout,
    },
    /// SSM linear-recurrence scan (R-PACK-01A). 1D elementwise-style dispatch
    /// over `length · state_dim`.
    SsmScan {
        /// Sequence length.
        length: u64,
        /// Recurrence state dimension.
        state_dim: u64,
        /// Stored state/weight layout.
        layout: PackedStorageLayout,
    },
}

/// Admitted packed storage layouts for native kernel execution (R-PACK-01A).
///
/// Type ids and block geometries bind to the pinned Gradus GGUF-A3 facts
/// (`gguf_manifest` / `dequant` @ the GI2-1 ggml pin) — never re-derived
/// from a filename. An id outside this closed set is unrepresentable.
#[allow(non_camel_case_types)] // GGUF type names are the layout authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedStorageLayout {
    /// Plain f32 passthrough (existing `TiledMatMul` / gather surface).
    F32,
    /// Native 2-byte IEEE-754 binary16; in-kernel f32 convert.
    F16,
    /// Native 2-byte bfloat16; in-kernel f32 convert.
    Bf16,
    /// f16 scale + int8, 32 elements / 34 bytes.
    Q8_0,
    /// 6-bit superblock, 256 elements / 144 bytes.
    Q4_K,
    /// 6-bit superblock, 256 elements / 176 bytes.
    Q5_K,
    /// 6-bit superblock, 256 elements / 210 bytes.
    Q6_K,
    /// f16 scale + 5-bit, 32 elements / 22 bytes (dense-rung `SmolLM2`).
    Q5_0,
    /// MXFP4: E8M0 group scale + packed 4-bit values, 32 elements / 17 bytes.
    ///
    /// llama.cpp `block_mxfp4` (`ggml-common.h:214-219`): one E8M0 byte
    /// plus `QK_MXFP4/2` packed nibbles. Type id 39 (`ggml.h:429`).
    MXFP4,
}

impl PackedStorageLayout {
    /// GGML type id (`ggml.h` / Gradus `GGML_*` constants).
    #[must_use]
    pub const fn ggml_type_id(self) -> u32 {
        match self {
            Self::F32 => 0,
            Self::F16 => 1,
            Self::Q5_0 => 6,
            Self::Q8_0 => 8,
            Self::Q4_K => 12,
            Self::Q5_K => 13,
            Self::Q6_K => 14,
            Self::Bf16 => 30,
            Self::MXFP4 => 39,
        }
    }

    /// Resolve a closed layout from a GGML type id. Unknown ids are `None`
    /// (fail closed — never a guessed block geometry).
    #[must_use]
    pub const fn from_ggml_type_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::F32),
            1 => Some(Self::F16),
            6 => Some(Self::Q5_0),
            8 => Some(Self::Q8_0),
            12 => Some(Self::Q4_K),
            13 => Some(Self::Q5_K),
            14 => Some(Self::Q6_K),
            30 => Some(Self::Bf16),
            39 => Some(Self::MXFP4),
            _ => None,
        }
    }

    /// Logical elements per stored block (pinned Gradus `dequant` table).
    #[must_use]
    pub const fn block_elements(self) -> u32 {
        match self {
            Self::F32 | Self::F16 | Self::Bf16 => 1,
            Self::Q8_0 | Self::Q5_0 | Self::MXFP4 => 32,
            Self::Q4_K | Self::Q5_K | Self::Q6_K => 256,
        }
    }

    /// Packed bytes per stored block (pinned Gradus `dequant` table).
    #[must_use]
    pub const fn block_bytes(self) -> u32 {
        match self {
            Self::F32 => 4,
            Self::F16 | Self::Bf16 => 2,
            Self::Q8_0 => 34,
            Self::Q4_K => 144,
            Self::Q5_K => 176,
            Self::Q6_K => 210,
            Self::Q5_0 => 22,
            Self::MXFP4 => 17,
        }
    }
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
        CollectionOpContract::QuantizedMatMul {
            dispatch_x,
            dispatch_y,
            ..
        }
        | CollectionOpContract::GroupedMatMul {
            dispatch_x,
            dispatch_y,
            ..
        } => {
            signature.workgroup = MirWorkgroupSize { x: 8, y: 8, z: 1 };
            signature.dispatch_size = MirKernelDispatchSize {
                x: dispatch_x,
                y: dispatch_y,
                z: 1,
            };
        }
        CollectionOpContract::BatchedAttention { output_elements, .. } => {
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
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: length.saturating_mul(channels),
                y: 1,
                z: 1,
            };
        }
        CollectionOpContract::SsmScan {
            length, state_dim, ..
        } => {
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: length.saturating_mul(state_dim),
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
                destination,
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
                MirCollectionOp::TensorQuantizedMatMul => {
                    let (left_dims, right_dims, layout) =
                        packed_rank2_matmul_operands(function, args, validation)?;
                    let tile: u64 = 8;
                    return Ok(Some(CollectionOpContract::QuantizedMatMul {
                        dispatch_x: right_dims[1].div_ceil(tile),
                        dispatch_y: left_dims[0].div_ceil(tile),
                        m: left_dims[0],
                        k: left_dims[1],
                        n: right_dims[1],
                        layout,
                    }));
                }
                MirCollectionOp::TensorGroupedMatMul => {
                    return Ok(Some(grouped_matmul_contract(function, args, validation)?));
                }
                MirCollectionOp::TensorBatchedAttention => {
                    return Ok(Some(batched_attention_contract(
                        function,
                        args,
                        destination.as_ref(),
                        validation,
                    )?));
                }
                MirCollectionOp::TensorSsmConv1d => {
                    return Ok(Some(ssm_conv1d_contract(function, args, validation)?));
                }
                MirCollectionOp::TensorSsmScan => {
                    return Ok(Some(ssm_scan_contract(function, args, validation)?));
                }
                _ => {}
            }
        }
    }
    Ok(None)
}

fn packed_layout_from_operand(operand: &MirOperand) -> Result<PackedStorageLayout, MirAbiError> {
    let id = match operand {
        MirOperand::Constant(MirConstant::Int(value)) => u32::try_from(*value).ok(),
        MirOperand::Constant(MirConstant::UInt(value)) => u32::try_from(*value).ok(),
        _ => None,
    };
    id.and_then(PackedStorageLayout::from_ggml_type_id)
        .ok_or_else(|| {
            MirAbiError::new(
                "packed kernel layout must be an admitted GGML type id \
                 (F32, F16, BF16, Q8_0, Q4_K, Q5_K, Q6_K, Q5_0, MXFP4)",
            )
        })
}

fn packed_rank2_matmul_operands(
    function: &MirFunction,
    args: &[MirOperand],
    validation: &MirValidationContext<'_>,
) -> Result<([u64; 2], [u64; 2], PackedStorageLayout), MirAbiError> {
    let Some(left_ty) = args
        .first()
        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
    else {
        return Err(MirAbiError::new(
            "quantized matmul requires a rank-2 left tensor operand",
        ));
    };
    let Some(right_ty) = args
        .get(1)
        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
    else {
        return Err(MirAbiError::new(
            "quantized matmul requires a rank-2 right tensor operand",
        ));
    };
    let Some(layout_operand) = args.get(2) else {
        return Err(MirAbiError::new(
            "quantized matmul requires a trailing GGML type-id const",
        ));
    };
    let left_dims =
        crate::tensor_type::tensor_literal_dims(left_ty, validation).map_err(MirAbiError::new)?;
    let right_dims =
        crate::tensor_type::tensor_literal_dims(right_ty, validation).map_err(MirAbiError::new)?;
    if left_dims.len() != 2 || right_dims.len() != 2 {
        return Err(MirAbiError::new(
            "quantized matmul dispatch contract requires rank-2 tensor operands",
        ));
    }
    if left_dims[1] != right_dims[0] {
        return Err(MirAbiError::new(
            "quantized matmul inner dimensions must match",
        ));
    }
    if left_dims[0] == 0 || left_dims[1] == 0 || right_dims[1] == 0 {
        return Err(MirAbiError::new(
            "quantized matmul dispatch contract requires non-zero dimensions",
        ));
    }
    let layout = packed_layout_from_operand(layout_operand)?;
    Ok((
        [left_dims[0], left_dims[1]],
        [right_dims[0], right_dims[1]],
        layout,
    ))
}

fn batched_attention_contract(
    function: &MirFunction,
    args: &[MirOperand],
    destination: Option<&crate::MirPlace>,
    validation: &MirValidationContext<'_>,
) -> Result<CollectionOpContract, MirAbiError> {
    if !(7..=12).contains(&args.len()) {
        return Err(MirAbiError::new(
            "batched attention has an unsupported operand/const arity",
        ));
    }
    // The final five operands are always the explicit axes. The operation
    // kind immediately precedes them; scores additionally carry the scale,
    // and projections carry their packed layout immediately before the kind.
    let axes_base = args.len() - 5;
    let kind_index = axes_base - 1;
    let base = axes_base;
    let kind_value = const_nonnegative_u64(args, kind_index, "attention kind")?;
    let (kind, rotate_half) = match kind_value {
        0 => (crate::kernel_plan::BatchedAttentionKind::ProjectionQ, false),
        1 => (crate::kernel_plan::BatchedAttentionKind::ProjectionK, false),
        2 => (crate::kernel_plan::BatchedAttentionKind::ProjectionV, false),
        // 6/7 are the same Q/K projection contracts with the explicit
        // Qwen/NeoX rotate-half mapping carried as a plan fact.
        6 => (crate::kernel_plan::BatchedAttentionKind::ProjectionQ, true),
        7 => (crate::kernel_plan::BatchedAttentionKind::ProjectionK, true),
        3 => (crate::kernel_plan::BatchedAttentionKind::Scores, false),
        4 => (crate::kernel_plan::BatchedAttentionKind::Softmax, false),
        5 => (crate::kernel_plan::BatchedAttentionKind::ContextSequenceMajor, false),
        other => {
            return Err(MirAbiError::new(format!(
                "batched attention kind {other} is not admitted"
            )))
        }
    };
    let kv_heads = const_u64(args, base, "kv_heads")?;
    let q_per_kv = const_u64(args, base + 1, "q_per_kv")?;
    let seq = const_u64(args, base + 2, "sequence")?;
    let head_dim = const_u64(args, base + 3, "head_dim")?;
    let hidden = const_u64(args, base + 4, "hidden")?;
    if kv_heads == 0 || q_per_kv == 0 || seq == 0 || head_dim == 0 || hidden == 0 {
        return Err(MirAbiError::new(
            "batched attention axes must be non-zero",
        ));
    }
    let output_ty = destination
        .and_then(crate::read_analysis::place_local_base)
        .and_then(|local| crate::local_ty::local_ty(function, local))
        .ok_or_else(|| MirAbiError::new("batched attention output has no typed local"))?;
    let output_dims = crate::tensor_type::tensor_literal_dims(output_ty, validation)
        .map_err(MirAbiError::new)?;
    let expected = match kind {
        crate::kernel_plan::BatchedAttentionKind::ProjectionQ
        | crate::kernel_plan::BatchedAttentionKind::ProjectionK
        | crate::kernel_plan::BatchedAttentionKind::ProjectionV => {
            // Keep Q explicit as [kv_group, query_head, seq, head_dim]
            // even when the physical projection has one contiguous head
            // axis. The sentinel and context contract depend on this rank-4
            // view, so no emitter may flatten the GQA mapping implicitly.
            if matches!(kind, crate::kernel_plan::BatchedAttentionKind::ProjectionQ) {
                vec![kv_heads, q_per_kv, seq, head_dim]
            } else {
                vec![kv_heads, seq, head_dim]
            }
        }
        crate::kernel_plan::BatchedAttentionKind::Scores
        | crate::kernel_plan::BatchedAttentionKind::Softmax => {
            vec![kv_heads, q_per_kv, seq, seq]
        }
        crate::kernel_plan::BatchedAttentionKind::ContextSequenceMajor => vec![seq, hidden],
    };
    if output_dims != expected {
        return Err(MirAbiError::new(format!(
            "batched attention {:?} output dims {:?} do not match {:?}",
            kind, output_dims, expected
        )));
    }
    let layout = if matches!(
        kind,
        crate::kernel_plan::BatchedAttentionKind::ProjectionQ
            | crate::kernel_plan::BatchedAttentionKind::ProjectionK
            | crate::kernel_plan::BatchedAttentionKind::ProjectionV
    ) {
        Some(packed_layout_from_operand(
            args.get(kind_index - 1).ok_or_else(|| {
                MirAbiError::new("batched projection requires a GGML layout const")
            })?,
        )?)
    } else {
        None
    };
    let scale_bits = if matches!(kind, crate::kernel_plan::BatchedAttentionKind::Scores) {
        let value = const_f64(args, kind_index - 1, "attention scale")? as f32;
        if !value.is_finite() || value == 0.0 {
            return Err(MirAbiError::new("attention scale must be finite and non-zero"));
        }
        value.to_bits()
    } else {
        0
    };
    let output_elements = expected.iter().try_fold(1u64, |product, dim| {
        product
            .checked_mul(*dim)
            .ok_or_else(|| MirAbiError::new("batched attention output size overflow"))
    })?;
    Ok(CollectionOpContract::BatchedAttention {
        kind,
        kv_heads,
        q_per_kv,
        seq,
        head_dim,
        hidden,
        layout,
        rotate_half,
        scale_bits,
        output_elements,
    })
}

fn const_nonnegative_u64(
    args: &[MirOperand],
    index: usize,
    what: &str,
) -> Result<u64, MirAbiError> {
    match args.get(index) {
        Some(MirOperand::Constant(MirConstant::Int(value))) if *value >= 0 => {
            u64::try_from(*value).map_err(|_| MirAbiError::new(format!("{what} is too large")))
        }
        Some(MirOperand::Constant(MirConstant::UInt(value))) => Ok(*value),
        _ => Err(MirAbiError::new(format!(
            "batched attention {what} must be a non-negative const"
        ))),
    }
}

fn const_u64(args: &[MirOperand], index: usize, what: &str) -> Result<u64, MirAbiError> {
    match args.get(index) {
        Some(MirOperand::Constant(MirConstant::Int(value))) if *value > 0 => {
            u64::try_from(*value).map_err(|_| MirAbiError::new(format!("{what} is too large")))
        }
        Some(MirOperand::Constant(MirConstant::UInt(value))) if *value > 0 => Ok(*value),
        _ => Err(MirAbiError::new(format!("batched attention {what} must be a positive const"))),
    }
}

fn const_f64(args: &[MirOperand], index: usize, what: &str) -> Result<f64, MirAbiError> {
    match args.get(index) {
        Some(MirOperand::Constant(MirConstant::Float(value))) => Ok(*value),
        _ => Err(MirAbiError::new(format!("batched attention {what} must be a float const"))),
    }
}

fn grouped_matmul_contract(
    function: &MirFunction,
    args: &[MirOperand],
    validation: &MirValidationContext<'_>,
) -> Result<CollectionOpContract, MirAbiError> {
    let Some(left_ty) = args
        .first()
        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
    else {
        return Err(MirAbiError::new(
            "grouped matmul requires a left tensor operand",
        ));
    };
    let Some(right_ty) = args
        .get(1)
        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
    else {
        return Err(MirAbiError::new(
            "grouped matmul requires a rank-3 right tensor operand",
        ));
    };
    let Some(layout_operand) = args.get(2) else {
        return Err(MirAbiError::new(
            "grouped matmul requires a trailing GGML type-id const",
        ));
    };
    let left_dims =
        crate::tensor_type::tensor_literal_dims(left_ty, validation).map_err(MirAbiError::new)?;
    let right_dims =
        crate::tensor_type::tensor_literal_dims(right_ty, validation).map_err(MirAbiError::new)?;
    if right_dims.len() != 3 {
        return Err(MirAbiError::new(
            "grouped matmul dispatch contract requires a rank-3 right operand",
        ));
    }
    if left_dims.len() != 2 {
        return Err(MirAbiError::new(
            "grouped matmul dispatch contract requires a rank-2 left operand",
        ));
    }
    let groups = right_dims[0];
    let k = right_dims[1];
    let n = right_dims[2];
    if left_dims[1] != k {
        return Err(MirAbiError::new(
            "grouped matmul inner dimensions must match",
        ));
    }
    if groups == 0 || left_dims[0] == 0 || k == 0 || n == 0 {
        return Err(MirAbiError::new(
            "grouped matmul dispatch contract requires non-zero dimensions",
        ));
    }
    let tile: u64 = 8;
    Ok(CollectionOpContract::GroupedMatMul {
        dispatch_x: n.div_ceil(tile),
        dispatch_y: left_dims[0].div_ceil(tile),
        m: left_dims[0],
        k,
        n,
        groups,
        layout: packed_layout_from_operand(layout_operand)?,
    })
}

fn ssm_conv1d_contract(
    function: &MirFunction,
    args: &[MirOperand],
    validation: &MirValidationContext<'_>,
) -> Result<CollectionOpContract, MirAbiError> {
    let Some(input_ty) = args
        .first()
        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
    else {
        return Err(MirAbiError::new("ssm conv1d requires an input tensor"));
    };
    let Some(kernel_ty) = args
        .get(1)
        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
    else {
        return Err(MirAbiError::new("ssm conv1d requires a kernel tensor"));
    };
    let Some(layout_operand) = args.get(2) else {
        return Err(MirAbiError::new(
            "ssm conv1d requires a trailing GGML type-id const",
        ));
    };
    let input_dims =
        crate::tensor_type::tensor_literal_dims(input_ty, validation).map_err(MirAbiError::new)?;
    let kernel_dims =
        crate::tensor_type::tensor_literal_dims(kernel_ty, validation).map_err(MirAbiError::new)?;
    if input_dims.len() != 2 {
        return Err(MirAbiError::new(
            "ssm conv1d dispatch contract requires a rank-2 input",
        ));
    }
    if kernel_dims.is_empty() {
        return Err(MirAbiError::new(
            "ssm conv1d dispatch contract requires a kernel operand with a width axis",
        ));
    }
    if input_dims[0] == 0 || input_dims[1] == 0 || kernel_dims[0] == 0 {
        return Err(MirAbiError::new(
            "ssm conv1d dispatch contract requires non-zero dimensions",
        ));
    }
    Ok(CollectionOpContract::SsmConv1d {
        length: input_dims[0],
        channels: input_dims[1],
        kernel_width: kernel_dims[0],
        layout: packed_layout_from_operand(layout_operand)?,
    })
}

fn ssm_scan_contract(
    function: &MirFunction,
    args: &[MirOperand],
    validation: &MirValidationContext<'_>,
) -> Result<CollectionOpContract, MirAbiError> {
    let Some(input_ty) = args
        .first()
        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
    else {
        return Err(MirAbiError::new("ssm scan requires an input tensor"));
    };
    let Some(layout_operand) = args.get(1) else {
        return Err(MirAbiError::new(
            "ssm scan requires a trailing GGML type-id const",
        ));
    };
    let input_dims =
        crate::tensor_type::tensor_literal_dims(input_ty, validation).map_err(MirAbiError::new)?;
    if input_dims.len() != 2 {
        return Err(MirAbiError::new(
            "ssm scan dispatch contract requires a rank-2 input",
        ));
    }
    if input_dims[0] == 0 || input_dims[1] == 0 {
        return Err(MirAbiError::new(
            "ssm scan dispatch contract requires non-zero dimensions",
        ));
    }
    Ok(CollectionOpContract::SsmScan {
        length: input_dims[0],
        state_dim: input_dims[1],
        layout: packed_layout_from_operand(layout_operand)?,
    })
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
