use crate::{
    MirCollectionOp, MirConstant, MirFunction, MirIntrinsic, MirLocalId, MirOperand, MirParam,
    MirParamMode, MirPlace, MirPlaceBase, MirRuntimeCall, MirStatementKind, MirTempId,
    MirValidationContext, MirValueKind,
};
use radix_lexer::Interner;
use radix_types::NumericWidth;

use super::{
    ERR_KERNEL_BUILTIN_DUPLICATE_NAME, ERR_KERNEL_BUILTIN_UNKNOWN_NAME, MirAbiError,
    MirKernelBufferBindingKind, MirKernelBuiltin, MirKernelBuiltinInput, MirKernelDispatchSize,
    MirKernelResourceAccess, MirKernelShaderAccess, MirKernelSignature, MirWorkgroupSize,
    device_view_element_count, require_u32_index,
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
/// - **reduction (Sum/Mean/Max/Min)**: `dispatch_size.x = n` (the grid covers the
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
    /// Rank-2 matmul whose M, K or N is bound at run time. The tile grid is
    /// not a number here: `tiled_matmul` (launch geometry) computes
    /// `(ceil(N/8), ceil(M/8))` from the call-site shapes, and the
    /// inner-dimension equality becomes a launch-time check. Batched and
    /// quantized matmul stay literal-only.
    MatMulRuntime,
    Transpose {
        /// Source rows `M` of the rank-2 operand (output columns).
        m: u64,
        /// Source columns `N` of the rank-2 operand (output rows).
        n: u64,
    },
    /// U3 head-axis batched transpose: leading batch (head) axes over the
    /// swapped trailing `[M, N]`. A distinct variant — the rank-2
    /// `Transpose` image and every rank-2 backend arm stay unchanged, and
    /// a batched fact can never resolve as rank-2 by omission.
    BatchedTranspose {
        /// Product of the leading batch axes (the head count).
        batch: u64,
        /// Source rows `M` per batch (output columns).
        m: u64,
        /// Source columns `N` per batch (output rows).
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
        /// Sequence extent of THIS op: projection input rows (M) for Q/K/V,
        /// context length for scores/softmax/context. Distinct from a seq
        /// const that may still name conversation position N+1.
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
    /// PGC-R5 row-reduction contract (`TensorRmsNorm` /
    /// `TensorCausalMaskedSoftmax`, head-cto verdict `91fe3d59`): one
    /// workgroup per row — `ROW_REDUCTION_WORKGROUP_X` lanes tree-reduce
    /// the row statistic in threadgroup memory, then a vectorized scale
    /// pass writes the row. Buffer counts and element counts are
    /// UNCHANGED from the generic 1D contract; only the launch geometry
    /// differs (`workgroup (32,1,1)`, `dispatch (rows,1,1)`). Resolution
    /// is admission-gated
    /// ([`crate::kernel_plan::rmsnorm_row_reduction_admission`] /
    /// [`crate::kernel_plan::causal_softmax_row_reduction_admission`]):
    /// non-admitted shapes keep the generic 1D contract byte-for-byte.
    RowReduction {
        /// Row count (one workgroup per row; the dispatch extent).
        rows: u64,
        /// Row width (the reduction extent the lanes stride over).
        width: u64,
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

    /// Native packed-region byte length for `logical_elems` stored values.
    ///
    /// Returns `None` when the element count is not a whole number of blocks
    /// (fail closed — never a guessed pad to F32).
    #[must_use]
    pub const fn packed_bytes_for_elements(self, logical_elems: u64) -> Option<u64> {
        let block_elements = self.block_elements() as u64;
        let block_bytes = self.block_bytes() as u64;
        if block_elements == 0 || !logical_elems.is_multiple_of(block_elements) {
            return None;
        }
        (logical_elems / block_elements).checked_mul(block_bytes)
    }

    /// Language width carried by a native layout, if any.
    ///
    /// The plan seam correspondence: `PackedStorageLayout::Bf16 ↔
    /// NumericWidth::BF16` (and the sibling native layouts) is named here so
    /// packed admission and language widths share one fact. Quantized layouts
    /// have no single language width — `None` keeps the seam fail-closed.
    #[must_use]
    pub const fn numeric_width(self) -> Option<NumericWidth> {
        match self {
            Self::F32 => Some(NumericWidth::F32),
            Self::F16 => Some(NumericWidth::F16),
            Self::Bf16 => Some(NumericWidth::BF16),
            Self::Q8_0 | Self::Q5_0 | Self::Q4_K | Self::Q5_K | Self::Q6_K | Self::MXFP4 => None,
        }
    }

    /// Admitted native layout for a language width, if any.
    ///
    /// Reverse of [`PackedStorageLayout::numeric_width`]; integer and f64
    /// widths have no packed layout today, so `None` fails the seam closed.
    #[must_use]
    pub const fn from_numeric_width(width: NumericWidth) -> Option<Self> {
        match width {
            NumericWidth::F32 => Some(Self::F32),
            NumericWidth::F16 => Some(Self::F16),
            NumericWidth::BF16 => Some(Self::Bf16),
            NumericWidth::I8
            | NumericWidth::I16
            | NumericWidth::I32
            | NumericWidth::I64
            | NumericWidth::U8
            | NumericWidth::U16
            | NumericWidth::U32
            | NumericWidth::U64
            | NumericWidth::F64
            | NumericWidth::D64
            // `inf` has no native device layout (ruling 39(d)).
            | NumericWidth::Inf => None,
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
    if apply_filum_declaration_launch(function, validation, signature)? {
        return Ok(());
    }
    let Some(contract) = collection_op_contract(function, validation)? else {
        return Ok(());
    };
    apply_launch_contract(contract, signature);
    Ok(())
}

/// Apply the `FilumSum` declaration's launch shape when the body declares one.
///
/// Returns `true` when the declaration supplied the whole contract, so the
/// collection-op scan must be skipped.
fn apply_filum_declaration_launch(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    signature: &mut MirKernelSignature,
) -> Result<bool, MirAbiError> {
    // SFR-6: a `FilumSum` declaration is the author-stated recipe — it has
    // no collection-op contract, so the collection scan below never sees
    // it. The native collective arm needs the declaration's launch shape
    // (workgroup = lane width W, dispatch = source length, one partial
    // slot per workgroup) from the same facts the plan pass resolves
    // (`kernel_plan_for_function`'s declared-recipe scan).
    let Some((lane_width, length)) = filum_declaration_launch_facts(function, validation)? else {
        return Ok(false);
    };
    if lane_width == 0 {
        return Err(MirAbiError::new("filum summa lane width must be non-zero"));
    }
    let partials = length.div_ceil(u64::from(lane_width));
    signature.workgroup = MirWorkgroupSize {
        x: lane_width,
        y: 1,
        z: 1,
    };
    signature.dispatch_size = MirKernelDispatchSize {
        x: length,
        y: 1,
        z: 1,
    };
    signature.output.element_count = partials;
    Ok(true)
}

/// Assign a flat 1D workgroup and dispatch pair.
fn set_flat_launch(signature: &mut MirKernelSignature, workgroup_x: u32, dispatch: u64) {
    signature.workgroup = MirWorkgroupSize {
        x: workgroup_x,
        y: 1,
        z: 1,
    };
    signature.dispatch_size = MirKernelDispatchSize {
        x: dispatch,
        y: 1,
        z: 1,
    };
}

/// Assign the square 8x8 matmul workgroup and the given dispatch grid.
fn set_matmul_launch(signature: &mut MirKernelSignature, dispatch_x: u64, dispatch_y: u64) {
    signature.workgroup = MirWorkgroupSize { x: 8, y: 8, z: 1 };
    signature.dispatch_size = MirKernelDispatchSize {
        x: dispatch_x,
        y: dispatch_y,
        z: 1,
    };
}

/// Translate one resolved collection-op contract into launch geometry.
fn apply_launch_contract(contract: CollectionOpContract, signature: &mut MirKernelSignature) {
    match contract {
        CollectionOpContract::Reduction {
            length,
            workgroup_x,
            partials,
        } => {
            set_flat_launch(signature, workgroup_x, length);
            signature.output.element_count = partials;
        }
        CollectionOpContract::MatMul {
            dispatch_x,
            dispatch_y,
        }
        | CollectionOpContract::QuantizedMatMul {
            dispatch_x,
            dispatch_y,
            ..
        }
        | CollectionOpContract::GroupedMatMul {
            dispatch_x,
            dispatch_y,
            ..
        } => {
            set_matmul_launch(signature, dispatch_x, dispatch_y);
        }
        CollectionOpContract::MatMulRuntime => {
            // Same 8x8 workgroup; the grid comes from the call-site shapes
            // at launch, so `dispatch_size` is left untouched.
            signature.workgroup = MirWorkgroupSize { x: 8, y: 8, z: 1 };
        }
        CollectionOpContract::RowReduction { rows, .. } => {
            // PGC-R5: one workgroup per row. Only the launch geometry
            // changes — buffer bindings and element counts stay the
            // generic-1D facts. Gated on the single-output shape: a kernel
            // with additional outputs or accumulation buffers keeps the 1D
            // contract (a rows grid would under-cover the extra writes).
            if signature.extra_outputs.is_empty() && signature.accumulation_buffers.is_empty() {
                // The 1D dispatch law counts THREADS (the reduction
                // contract's law): rows workgroups × 32 lanes, so the
                // derived `ceil(dispatch / workgroup)` grid is exactly the
                // row count.
                set_flat_launch(
                    signature,
                    crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X,
                    rows.saturating_mul(u64::from(crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X)),
                );
            }
        }
        CollectionOpContract::Transpose { m, n } => {
            // Elementwise-style 1D contract: one thread per output element
            // of the rank-2 remap (S5-U1).
            set_flat_launch(
                signature,
                crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                m.saturating_mul(n),
            );
        }
        // U3 head-axis: the batch folds into the same elementwise-style 1D
        // contract — one thread per output element of the whole batched
        // `[batch, N, M]` output (`batch · M · N` threads).
        CollectionOpContract::BatchedTranspose { batch, m, n } => {
            set_flat_launch(
                signature,
                crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                batch.saturating_mul(m).saturating_mul(n),
            );
        }
        CollectionOpContract::BatchedAttention {
            output_elements, ..
        } => {
            set_flat_launch(signature, 64, output_elements);
        }
        CollectionOpContract::SsmConv1d {
            length, channels, ..
        } => {
            set_flat_launch(
                signature,
                crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                length.saturating_mul(channels),
            );
        }
        CollectionOpContract::SsmScan {
            length, state_dim, ..
        } => {
            set_flat_launch(
                signature,
                crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                length.saturating_mul(state_dim),
            );
        }
    }
}

/// The launch facts of the kernel body's `FilumSum` declaration (SFR-6):
/// `(lane width W, source length K)` resolved from the declaration's lone
/// tensor operand type — `None` when the body declares none.
fn filum_declaration_launch_facts(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Option<(u32, u64)>, MirAbiError> {
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::FilumReduce(filum),
                        args,
                        ..
                    },
                ..
            } = &statement.kind
            else {
                continue;
            };
            // The grid form has its own plan and launch split; it never
            // takes the declared sum's launch facts.
            if filum.form != crate::MirFilumForm::Declared {
                continue;
            }
            let Some(MirOperand::Place(place)) = args.first() else {
                continue;
            };
            if !place.projections.is_empty() {
                continue;
            }
            let MirPlaceBase::Local(source_local) = place.base else {
                continue;
            };
            let Some(param) = function.params.iter().find(|p| p.local == source_local) else {
                continue;
            };
            let length = device_view_element_count(param.ty, validation)?;
            return Ok(Some((filum.lane_width, length)));
        }
    }
    Ok(None)
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
                MirCollectionOp::Sum
                | MirCollectionOp::TensorMean
                | MirCollectionOp::TensorMax
                | MirCollectionOp::TensorMin => {
                    if let Some(contract) =
                        reduction_contract(op, function, args, destination, validation)?
                    {
                        return Ok(Some(contract));
                    }
                }
                MirCollectionOp::TensorMatMul => {
                    if let Some(contract) = matmul_contract(function, args, validation)? {
                        return Ok(Some(contract));
                    }
                }
                MirCollectionOp::TensorTranspose => {
                    if let Some(contract) = transpose_contract(function, args, validation)? {
                        return Ok(Some(contract));
                    }
                }
                MirCollectionOp::TensorQuantizedMatMul => {
                    return Ok(Some(quantized_matmul_contract(function, args, validation)?));
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
                // PGC-R5 (verdict 91fe3d59): the row-reduction family.
                // Admission-gated — a non-admitted shape keeps scanning
                // (no contract) and lands on the generic 1D contract with
                // its frozen per-element emission.
                MirCollectionOp::TensorRmsNorm => {
                    if let Some(contract) = row_reduction_contract(
                        function,
                        validation,
                        op,
                        crate::kernel_plan::rmsnorm_row_reduction_admission,
                    ) {
                        return Ok(Some(contract));
                    }
                }
                // PGC-R5 additive-idiom twin: the causal softmax entries
                // lower as a plain `TensorSoftmax` over an `(x + mask)`
                // elementwise temp (the GEA2-U5e surface — the mask carries
                // the causality as data). Under the established mask data
                // contract (0 at/below the diagonal, -inf above) the causal
                // row reduction is the exact same output class, so the
                // multi-row idiom resolves the same RowReduction contract.
                // A plain softmax (operand is a direct tensor input) never
                // matches and keeps the generic 1D contract.
                MirCollectionOp::TensorSoftmax => {
                    if args.first().is_some_and(|operand| {
                        softmax_operand_is_additive_mask_temp(function, operand)
                    }) && let Some(contract) = row_reduction_contract(
                        function,
                        validation,
                        op,
                        crate::kernel_plan::causal_softmax_row_reduction_admission,
                    ) {
                        return Ok(Some(contract));
                    }
                }
                MirCollectionOp::TensorCausalMaskedSoftmax => {
                    if let Some(contract) = row_reduction_contract(
                        function,
                        validation,
                        op,
                        crate::kernel_plan::causal_softmax_row_reduction_admission,
                    ) {
                        return Ok(Some(contract));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(None)
}

/// Matmul contract for the current `TensorMatMul`: the run-time-extent
/// rank-2 contraction, the rank-2 literal grid, or the batched fold.
fn matmul_contract(
    function: &MirFunction,
    args: &[MirOperand],
    validation: &MirValidationContext<'_>,
) -> Result<Option<CollectionOpContract>, MirAbiError> {
    let Some(left_ty) = args
        .first()
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
    else {
        return Ok(None);
    };
    let Some(right_ty) = args
        .get(1)
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
    else {
        return Ok(None);
    };
    let left_all =
        crate::tensor_type::tensor_dims(left_ty, validation).map_err(MirAbiError::new)?;
    let right_all =
        crate::tensor_type::tensor_dims(right_ty, validation).map_err(MirAbiError::new)?;
    let runtime = left_all
        .iter()
        .chain(&right_all)
        .any(|dim| matches!(dim, crate::tensor_type::Dim::Runtime));
    if runtime {
        // Only the rank-2 contraction takes run-time extents;
        // batched stays literal-only (named refusal).
        if left_all.len() != 2 || !(1..=2).contains(&right_all.len()) {
            return Err(MirAbiError::new(
                "matmul with run-time extents requires a rank-2 left tensor and rank-1 or rank-2 right tensor",
            ));
        }
        if let (crate::tensor_type::Dim::Literal(left_k), crate::tensor_type::Dim::Literal(right_k)) =
            (left_all[1], right_all[0])
            && left_k != right_k
        {
            return Err(MirAbiError::new("matmul inner dimensions must match"));
        }
        return Ok(Some(CollectionOpContract::MatMulRuntime));
    }
    let left_dims =
        crate::tensor_type::tensor_literal_dims(left_ty, validation).map_err(MirAbiError::new)?;
    let right_dims =
        crate::tensor_type::tensor_literal_dims(right_ty, validation).map_err(MirAbiError::new)?;
    let tile = u64::from(crate::kernel_plan::MATMUL_TILE);
    // U1 head-axis: a rank-3/4 receiver with a rank-2+ right
    // operand carrying leading batch axes is the batched
    // contraction (decode `[5,3,1,64]·[5,64,76]` and prefill
    // `[5,3,36,64]·[5,64,36]`). The argument's batch dims
    // must be a PREFIX of the receiver's (the GQA share:
    // K/V per kv-head broadcast over q-per-kv heads). The
    // batch folds into `dispatch_y` — one padded row-block
    // per batch — so the workgroup/tile grid laws are
    // unchanged and the rank-2 arm stays byte-identical.
    let batched_rank = left_dims.len() >= 3
        && left_dims.len() <= 4
        && right_dims.len() >= 2
        && right_dims.len() <= left_dims.len();
    if batched_rank {
        let batch_axis_count = right_dims.len() - 2;
        for axis in 0..batch_axis_count {
            if left_dims[axis] != right_dims[axis] {
                return Err(MirAbiError::new(
                    "batched matmul requires the right operand's leading batch dims to be a prefix of the left operand's batch dims",
                ));
            }
        }
        if left_dims[left_dims.len() - 1] != right_dims[right_dims.len() - 2] {
            return Err(MirAbiError::new("matmul inner dimensions must match"));
        }
        let m = left_dims[left_dims.len() - 2];
        let n = right_dims[right_dims.len() - 1];
        let batch = left_dims[..left_dims.len() - 2].iter().product::<u64>();
        return Ok(Some(CollectionOpContract::MatMul {
            dispatch_x: n.div_ceil(tile),
            dispatch_y: batch * m.div_ceil(tile),
        }));
    }
    if left_dims.len() != 2 || !(1..=2).contains(&right_dims.len()) {
        return Err(MirAbiError::new(
            "matmul dispatch contract requires a rank-2 left tensor and rank-1 or rank-2 right tensor",
        ));
    }
    if left_dims[1] != right_dims[0] {
        return Err(MirAbiError::new("matmul inner dimensions must match"));
    }
    let right_columns = right_dims.get(1).copied().unwrap_or(1);
    Ok(Some(CollectionOpContract::MatMul {
        dispatch_x: right_columns.div_ceil(tile),
        dispatch_y: left_dims[0].div_ceil(tile),
    }))
}

/// Transpose contract for the current `TensorTranspose`: the rank-2 remap or
/// the rank-3/4 batched fold.
fn transpose_contract(
    function: &MirFunction,
    args: &[MirOperand],
    validation: &MirValidationContext<'_>,
) -> Result<Option<CollectionOpContract>, MirAbiError> {
    let Some(operand_ty) = args
        .first()
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
    else {
        return Ok(None);
    };
    let dims = crate::tensor_type::tensor_literal_dims(operand_ty, validation)
        .map_err(MirAbiError::new)?;
    if dims.len() == 3 || dims.len() == 4 {
        // U3 head-axis: a rank-3/4 operand is the batched
        // transpose — leading batch (head) axes fold into
        // the batch count, the trailing `[M, N]` pair swaps.
        // Rank 5+ stays closed at this typed gate (the
        // device-view family admits at most rank 4).
        let m = dims[dims.len() - 2];
        let n = dims[dims.len() - 1];
        let batch = dims[..dims.len() - 2]
            .iter()
            .copied()
            .try_fold(1_u64, u64::checked_mul)
            .ok_or_else(|| MirAbiError::new("batched transpose batch count overflows u64"))?;
        if m == 0 || n == 0 || batch == 0 {
            return Err(MirAbiError::new(
                "transpose dispatch contract requires non-zero dimensions",
            ));
        }
        return Ok(Some(CollectionOpContract::BatchedTranspose { batch, m, n }));
    }
    if dims.len() != 2 {
        return Err(MirAbiError::new(
            "transpose dispatch contract requires a rank-2 tensor operand, or rank-3/4 for the batched transpose (rank 5+ is closed)",
        ));
    }
    if dims[0] == 0 || dims[1] == 0 {
        return Err(MirAbiError::new(
            "transpose dispatch contract requires non-zero dimensions",
        ));
    }
    Ok(Some(CollectionOpContract::Transpose {
        m: dims[0],
        n: dims[1],
    }))
}

/// Quantized-matmul contract for the current `TensorQuantizedMatMul`.
fn quantized_matmul_contract(
    function: &MirFunction,
    args: &[MirOperand],
    validation: &MirValidationContext<'_>,
) -> Result<CollectionOpContract, MirAbiError> {
    let (left_dims, right_dims, layout) = packed_rank2_matmul_operands(function, args, validation)?;
    let tile = u64::from(crate::kernel_plan::MATMUL_TILE);
    let m = left_dims[0];
    let n = right_dims[1];
    // M=1 quantized dispatch exists as a shared, lane-neutral
    // contract: a single flat grid row (same `ceil(N/8)`
    // workgroups along x). Whether a device lane carries a
    // dedicated GEMV body — and its thread geometry — is a
    // per-lane recipe fact (Metal: `radix-mir-metal`
    // quantized_matmul emit); the ELP-02 lane admission map
    // is the handoff for stating it per lane. Prefill keeps
    // the tiled GEMM grid.
    let (dispatch_x, dispatch_y) = if m == 1 {
        (n.div_ceil(tile), 1)
    } else {
        (n.div_ceil(tile), m.div_ceil(tile))
    };
    Ok(CollectionOpContract::QuantizedMatMul {
        dispatch_x,
        dispatch_y,
        m,
        k: left_dims[1],
        n,
        layout,
    })
}

/// Reduction-family contract for the current `Sum`/`Mean`/`Max`/`Min` op.
fn reduction_contract(
    op: MirCollectionOp,
    function: &MirFunction,
    args: &[MirOperand],
    destination: &Option<MirPlace>,
    validation: &MirValidationContext<'_>,
) -> Result<Option<CollectionOpContract>, MirAbiError> {
    // PGC-R5: skip the DEAD stability-max idiom — a
    // `TensorMax`/`TensorMin` whose temp destination only
    // copies into scalar locals nothing ever reads (the
    // `_stability_max` coalesce const of the causal-softmax
    // idiom is the standing example). Narrow by
    // construction: Sum/Mean reductions and any
    // tensor-typed copy stay live (a later subchain may
    // read them — the S3-A5 cross-subchain blind spot),
    // and the emitters' dead skips are the precedent.
    if matches!(op, MirCollectionOp::TensorMax | MirCollectionOp::TensorMin)
        && destination
            .as_ref()
            .is_some_and(|dest| reduction_destination_is_dead(function, dest))
    {
        return Ok(None);
    }
    let Some(operand_ty) = args
        .first()
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
    else {
        return Ok(None);
    };
    let length = device_view_element_count(operand_ty, validation)?;
    if length == 0 {
        return Err(MirAbiError::new("reduction tensor length must be non-zero"));
    }
    let workgroup_x = reduction_workgroup_x(length);
    let partials = length.div_ceil(u64::from(workgroup_x));
    Ok(Some(CollectionOpContract::Reduction {
        length,
        workgroup_x,
        partials,
    }))
}

/// PGC-R5: resolve the `RowReduction` contract for a row-reduction-family
/// op from the recipe's typed shape facts (the same
/// [`crate::kernel_plan::transformer_recipe_shape`] scan the plan pass
/// uses), under the op's own admission predicate. `None` when the
/// shape is not admitted or the recipe facts do not resolve — the caller
/// keeps scanning and the kernel keeps the generic 1D contract.
fn row_reduction_contract(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    op: MirCollectionOp,
    admission: fn(u64, u64) -> Option<crate::kernel_plan::RowReductionFacts>,
) -> Option<CollectionOpContract> {
    // `transformer_recipe_shape` resolves the additive-idiom softmax's
    // elementwise temp (`(x + mask)`) as well as a direct operand, so its
    // dims are the row-reduction dims here.
    let (dims, _) = crate::kernel_plan::transformer_recipe_shape(function, validation, op)?;
    if dims.len() != 2 {
        return None;
    }
    let (rows, width) = (dims[0], dims[1]);
    admission(rows, width)?;
    Some(CollectionOpContract::RowReduction { rows, width })
}

/// PGC-R5: whether a `TensorMax`/`TensorMin` destination is dead — the
/// `_stability_max` idiom whose result never reaches the kernel's
/// dataflow. A LOCAL destination is dead when the shared read analysis
/// (the S3-A5 authority) never reads it and it is not a param; a TEMP
/// destination is dead only when every reader is a pure copy into a local
/// that is itself dead by the same rule. The call site additionally gates
/// the skip to `TensorMax`/`TensorMin`, so live reductions and Sum/Mean
/// consumers can never be skipped.
fn reduction_destination_is_dead(function: &MirFunction, destination: &MirPlace) -> bool {
    let read_locals = crate::read_analysis::function_read_locals(function);
    let local_is_dead = |local: &MirLocalId| {
        !function.params.iter().any(|param| param.local == *local) && !read_locals.contains(local)
    };
    match destination.base {
        MirPlaceBase::Local(local) => local_is_dead(&local),
        MirPlaceBase::Temp(temp) => {
            // Every reader of the temp — in any statement or terminator,
            // by the shared slot-level read analysis — must be a pure copy
            // into a dead local; any other reader keeps the reduction live.
            let reads_temp = |slots: Vec<MirPlaceBase>| {
                slots
                    .iter()
                    .any(|slot| matches!(slot, MirPlaceBase::Temp(read) if *read == temp))
            };
            let mut saw_reader = false;
            for block in &function.blocks {
                for statement in &block.statements {
                    if !reads_temp(crate::read_analysis::statement_read_slots(statement)) {
                        continue;
                    }
                    saw_reader = true;
                    // A pure copy: the whole value is exactly the temp read.
                    let is_dead_copy = matches!(
                        &statement.kind,
                        MirStatementKind::Assign { place, value, .. }
                            if matches!(value.kind, MirValueKind::Operand(_))
                                && matches!(place.base, MirPlaceBase::Local(local) if local_is_dead(&local))
                    );
                    if !is_dead_copy {
                        return false;
                    }
                }
                if reads_temp(crate::read_analysis::terminator_read_slots(
                    &block.terminator.kind,
                )) {
                    return false;
                }
            }
            saw_reader
        }
    }
}

/// PGC-R5: whether EVERY `TensorMax`/`TensorMin` statement in the function
/// body is the DEAD stability-max idiom (a destination that never reaches
/// the kernel's dataflow — the `_stability_max` coalesce const of the
/// causal-softmax idiom is the standing example). The contract resolver
/// skips these statements when scanning for a dispatch contract; published
/// so the plan and subchain scans apply the SAME skip (a dead reduction is
/// not a recipe — the emitters' dead skips are the precedent). A body with
/// no such statement, or with any live one, yields `false`.
#[must_use]
pub fn stability_max_is_dead(function: &MirFunction) -> bool {
    let mut saw_stability = false;
    for block in &function.blocks {
        for statement in &block.statements {
            if !matches!(
                crate::collection_ops::statement_collection_op(statement),
                Some(MirCollectionOp::TensorMax | MirCollectionOp::TensorMin)
            ) {
                continue;
            }
            let destination = match &statement.kind {
                MirStatementKind::RuntimeCall { destination, .. } => destination.as_ref(),
                _ => None,
            };
            let dead = destination
                .as_ref()
                .is_some_and(|dest| reduction_destination_is_dead(function, dest));
            if !dead {
                return false;
            }
            saw_stability = true;
        }
    }
    saw_stability
}

/// PGC-R5: whether a softmax operand is the additive-mask idiom — a temp
/// bound to `lhs + rhs` over two place operands (the GEA2-U5e causal
/// surface: the softmax runs over `(scores + causal_mask)` and the mask
/// carries the causality as data). Structural, name-free, shared by the
/// ABI contract resolver and the Metal emitter so the two can never
/// disagree (A3).
#[must_use]
pub fn softmax_operand_is_additive_mask_temp(function: &MirFunction, operand: &MirOperand) -> bool {
    /// The softmax operand's binding identity — a temp or a local.
    enum DestinationBase {
        Temp(MirTempId),
        Local(MirLocalId),
    }
    impl DestinationBase {
        fn matches(&self, base: &MirPlaceBase) -> bool {
            matches!(
                (self, base),
                (Self::Temp(temp), MirPlaceBase::Temp(dest)) if temp == dest
            ) || matches!(
                (self, base),
                (Self::Local(local), MirPlaceBase::Local(dest)) if local == dest
            )
        }
    }
    let temp = match operand {
        MirOperand::Place(place) => match place.base {
            MirPlaceBase::Temp(temp) => DestinationBase::Temp(temp),
            MirPlaceBase::Local(local) => DestinationBase::Local(local),
        },
        MirOperand::Temp(temp) => DestinationBase::Temp(*temp),
        _ => return false,
    };
    let place_bases_are_locals = |operand: &MirOperand| {
        matches!(
            operand,
            MirOperand::Place(addend)
                if matches!(addend.base, MirPlaceBase::Local(_))
                    && addend.projections.is_empty()
        )
    };
    // The elementwise add lowers as a `TensorAdd` collection runtime call
    // whose destination is the temp (the S3-A5 tensor-expr model).
    function.blocks.iter().any(|block| {
        block
            .statements
            .iter()
            .any(|statement| match &statement.kind {
                MirStatementKind::RuntimeCall {
                    destination: Some(destination),
                    call:
                        MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
                            args,
                            ..
                        },
                } => {
                    temp.matches(&destination.base)
                        && args.len() == 2
                        && place_bases_are_locals(&args[0])
                        && place_bases_are_locals(&args[1])
                }
                _ => false,
            })
    })
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
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
    else {
        return Err(MirAbiError::new(
            "quantized matmul requires a rank-2 left tensor operand",
        ));
    };
    let Some(right_ty) = args
        .get(1)
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
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

/// Flat-dispatch expansion for single-row (M=1) projection GEMV.
///
/// The dispatch domain pads the logical output columns by this multiplier;
/// the concrete thread geometry is a per-lane recipe fact (Metal states it
/// in `radix-mir-metal` `quantized_matmul` emit). The ELP-02 lane admission
/// map is the handoff for owning it per lane.
///
/// This is also the decode (M=1) body's outputs-per-workgroup fact: one
/// 64-thread workgroup partitions into this many eight-lane reductions, so
/// the launch grid carries one workgroup per `lanes` LOGICAL outputs (R4
/// repair — `BatchedAttentionPlan::decode_outputs_per_workgroup`). The
/// logical output span itself stays unpadded.
pub(crate) const PROJECTION_GEMV_DISPATCH_LANES: u64 = 8;

fn batched_attention_contract(
    function: &MirFunction,
    args: &[MirOperand],
    destination: Option<&crate::MirPlace>,
    validation: &MirValidationContext<'_>,
) -> Result<CollectionOpContract, MirAbiError> {
    if !(7..=16).contains(&args.len()) {
        return Err(MirAbiError::new(
            "batched attention has an unsupported operand/const arity",
        ));
    }
    // The final five operands are always the explicit axes. The operation
    // kind immediately precedes them; scores additionally carry the scale,
    // and projections carry their packed layout immediately before the kind.
    let axes_base = args.len() - 5;
    let kind_index = axes_base - 1;
    let kind_value = const_nonnegative_u64(args, kind_index, "attention kind")?;
    let (kind, rotate_half) = batched_attention_kind(kind_value)?;
    let kv_heads = const_u64(args, axes_base, "kv_heads")?;
    let q_per_kv = const_u64(args, axes_base + 1, "q_per_kv")?;
    let seq = const_u64(args, axes_base + 2, "sequence")?;
    let head_dim = const_u64(args, axes_base + 3, "head_dim")?;
    let hidden = const_u64(args, axes_base + 4, "hidden")?;
    if kv_heads == 0 || q_per_kv == 0 || seq == 0 || head_dim == 0 || hidden == 0 {
        return Err(MirAbiError::new("batched attention axes must be non-zero"));
    }
    let is_projection = matches!(
        kind,
        crate::kernel_plan::BatchedAttentionKind::ProjectionQ
            | crate::kernel_plan::BatchedAttentionKind::ProjectionK
            | crate::kernel_plan::BatchedAttentionKind::ProjectionV
    );
    // Projection GEMV is selected by THIS step's input row count (M), not
    // the seq const (which may be the N+1 conversation position on a
    // dense continuation while the step itself is a single decode row).
    let step_rows = if is_projection {
        projection_step_rows(function, args, validation, hidden)?
    } else {
        seq
    };
    let output_ty = destination
        .and_then(crate::read_analysis::place_local_base)
        .and_then(|local| crate::local_ty::local_ty(function, local))
        .ok_or_else(|| MirAbiError::new("batched attention output has no typed local"))?;
    let output_dims =
        crate::tensor_type::tensor_literal_dims(output_ty, validation).map_err(MirAbiError::new)?;
    let expected = match kind {
        crate::kernel_plan::BatchedAttentionKind::ProjectionQ
        | crate::kernel_plan::BatchedAttentionKind::ProjectionK
        | crate::kernel_plan::BatchedAttentionKind::ProjectionV => {
            // Keep Q explicit as [kv_group, query_head, M, head_dim]
            // even when the physical projection has one contiguous head
            // axis. The sentinel and context contract depend on this rank-4
            // view, so no emitter may flatten the GQA mapping implicitly.
            if matches!(kind, crate::kernel_plan::BatchedAttentionKind::ProjectionQ) {
                vec![kv_heads, q_per_kv, step_rows, head_dim]
            } else {
                vec![kv_heads, step_rows, head_dim]
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
            "batched attention {kind:?} output dims {output_dims:?} do not match {expected:?}"
        )));
    }
    let layout = if is_projection {
        Some(packed_layout_from_operand(
            args.get(kind_index - 1).ok_or_else(|| {
                MirAbiError::new("batched projection requires a GGML layout const")
            })?,
        )?)
    } else {
        None
    };
    let scale_bits = if matches!(kind, crate::kernel_plan::BatchedAttentionKind::Scores) {
        #[allow(clippy::cast_possible_truncation)] // WGSL scale carrier is f32
        let value = const_f64(args, kind_index - 1, "attention scale")? as f32;
        if !value.is_finite() || value == 0.0 {
            return Err(MirAbiError::new(
                "attention scale must be finite and non-zero",
            ));
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
    // Single-row Q/K/V projections expand the flat output domain by the
    // per-lane GEMV dispatch multiplier. The logical output shape stays
    // unchanged. GEMV selection off (CTO `f083c057`) keeps the logical,
    // unpadded output: the batched body needs no padded dispatch domain.
    let output_elements = if is_projection
        && step_rows == 1
        && crate::device_program_plans::DECODE_GEMV_SELECTION_ENABLED
    {
        output_elements
            .checked_mul(PROJECTION_GEMV_DISPATCH_LANES)
            .ok_or_else(|| MirAbiError::new("decode GEMV dispatch size overflow"))?
    } else {
        output_elements
    };
    Ok(CollectionOpContract::BatchedAttention {
        kind,
        kv_heads,
        q_per_kv,
        seq: step_rows,
        head_dim,
        hidden,
        layout,
        rotate_half,
        scale_bits,
        output_elements,
    })
}

/// Decode the batched-attention operation kind and its rotate-half flag.
fn batched_attention_kind(
    kind_value: u64,
) -> Result<(crate::kernel_plan::BatchedAttentionKind, bool), MirAbiError> {
    Ok(match kind_value {
        0 => (crate::kernel_plan::BatchedAttentionKind::ProjectionQ, false),
        1 => (crate::kernel_plan::BatchedAttentionKind::ProjectionK, false),
        2 => (crate::kernel_plan::BatchedAttentionKind::ProjectionV, false),
        // 6/7 are the same Q/K projection contracts with the explicit
        // Qwen/NeoX rotate-half mapping carried as a plan fact.
        6 => (crate::kernel_plan::BatchedAttentionKind::ProjectionQ, true),
        7 => (crate::kernel_plan::BatchedAttentionKind::ProjectionK, true),
        3 => (crate::kernel_plan::BatchedAttentionKind::Scores, false),
        4 => (crate::kernel_plan::BatchedAttentionKind::Softmax, false),
        5 => (
            crate::kernel_plan::BatchedAttentionKind::ContextSequenceMajor,
            false,
        ),
        other => {
            return Err(MirAbiError::new(format!(
                "batched attention kind {other} is not admitted"
            )));
        }
    })
}

/// Row count of THIS projection step: the input tensor's leading dim.
///
/// Distinct from the seq const, which may carry a longer context length
/// (decode at conversation position N+1) while the step itself is M=1.
fn projection_step_rows(
    function: &MirFunction,
    args: &[MirOperand],
    validation: &MirValidationContext<'_>,
    hidden: u64,
) -> Result<u64, MirAbiError> {
    let Some(input_ty) = args
        .first()
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
    else {
        return Err(MirAbiError::new(
            "batched projection requires a rank-2 input tensor",
        ));
    };
    let dims =
        crate::tensor_type::tensor_literal_dims(input_ty, validation).map_err(MirAbiError::new)?;
    if dims.len() != 2 {
        return Err(MirAbiError::new(
            "batched projection input must be rank-2 [M, hidden]",
        ));
    }
    if dims[0] == 0 {
        return Err(MirAbiError::new(
            "batched projection row count must be non-zero",
        ));
    }
    if dims[1] != hidden {
        return Err(MirAbiError::new(format!(
            "batched projection input hidden {} does not match axis {hidden}",
            dims[1]
        )));
    }
    Ok(dims[0])
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
        _ => Err(MirAbiError::new(format!(
            "batched attention {what} must be a positive const"
        ))),
    }
}

fn const_f64(args: &[MirOperand], index: usize, what: &str) -> Result<f64, MirAbiError> {
    match args.get(index) {
        Some(MirOperand::Constant(MirConstant::Float(value))) => Ok(*value),
        _ => Err(MirAbiError::new(format!(
            "batched attention {what} must be a float const"
        ))),
    }
}

fn grouped_matmul_contract(
    function: &MirFunction,
    args: &[MirOperand],
    validation: &MirValidationContext<'_>,
) -> Result<CollectionOpContract, MirAbiError> {
    let Some(left_ty) = args
        .first()
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
    else {
        return Err(MirAbiError::new(
            "grouped matmul requires a left tensor operand",
        ));
    };
    let Some(right_ty) = args
        .get(1)
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
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
    let tile = u64::from(crate::kernel_plan::MATMUL_TILE);
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
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
    else {
        return Err(MirAbiError::new("ssm conv1d requires an input tensor"));
    };
    let Some(kernel_ty) = args
        .get(1)
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
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
        .and_then(|arg| crate::local_ty::operand_ty(function, arg))
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
        let builtin = if param.mode == MirParamMode::Lane {
            MirKernelBuiltin::GlobalInvocationX
        } else {
            named_builtin_for_param(param, interner)
                .ok_or_else(|| MirAbiError::new(ERR_KERNEL_BUILTIN_UNKNOWN_NAME))?
        };
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
) -> MirKernelBufferBindingKind {
    match shader_access {
        MirKernelShaderAccess::Read => MirKernelBufferBindingKind::ReadOnlyStorage,
        MirKernelShaderAccess::ReadWrite => MirKernelBufferBindingKind::Storage,
    }
}

pub(crate) fn param_source_name(param: &MirParam, interner: Option<&Interner>) -> Option<String> {
    let name = param.name?;
    let interner = interner?;
    Some(interner.resolve(name).to_owned())
}

pub(crate) fn named_builtin_for_param(
    param: &MirParam,
    interner: &Interner,
) -> Option<MirKernelBuiltin> {
    param
        .name
        .and_then(|name| MirKernelBuiltin::from_spelling(interner.resolve(name)))
}
