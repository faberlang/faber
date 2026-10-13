use super::plan::{
    AxisReductionPlan, BarrierPhase, BarrierPoint, BatchedAttentionKind, BatchedAttentionPlan,
    CausalMaskedSoftmaxPlan, CollectionKernelPlan, DistributedSumPlan, F32_ELEMENT_BYTE_WIDTH,
    GatherPlan, GroupedMatMulPlan, LayerNormalizationPlan, MATMUL_SHARED_A_BUFFER_NAME,
    MATMUL_SHARED_B_BUFFER_NAME, MATMUL_TILE, MatMulPlan, MatMulShapes, MatMulSharedMemory,
    OobPaddingPolicy, REDUCTION_SHARED_BUFFER_NAME, ReduceOp, ReductionPlan, RmsNormalizationPlan,
    RopePlan, RowSoftmaxBatch, RowSoftmaxMask, RowSoftmaxPlan, SharedMemoryLayout, SsmConv1dPlan,
    SsmScanPlan, TRANSPOSE_WORKGROUP_X, TransposePlan,
};
use super::validate::{
    PlanValidationError, validate_axis_reduction_plan, validate_batched_attention_plan,
    validate_causal_masked_softmax_plan, validate_distributed_sum_plan, validate_gather_plan,
    validate_grouped_matmul_plan, validate_matmul_plan, validate_quantized_matmul_plan,
    validate_reduction_plan, validate_rms_normalization_plan, validate_rope_plan,
    validate_row_softmax_plan, validate_ssm_conv1d_plan, validate_ssm_scan_plan,
    validate_transpose_plan,
};
use crate::abi::{CollectionOpContract, MirKernelSignature};
use crate::{
    MirCollectionOp, MirConstant, MirFunction, MirKernelResourceAccess, MirKernelResourceRole,
    MirLayoutKind, MirLocalId, MirOperand, MirPlaceBase, MirRuntimeCall, MirScalarLayout,
    MirStatementKind, MirValidationContext, MirValueKind, MirWorkgroupSize,
};

/// Whether a kernel signature's **result-tuple contract** is coherent for
/// plan resolution (S3-A1): every output resource (the first plus the extra
/// tuple elements) is a write storage buffer bound to a DISTINCT slot — two
/// gradient outputs must never alias a binding. The accumulation buffers are
/// not outputs and are not part of this check.
///
/// The plan pass resolves one plan per kernel; the multi-output contract
/// guarantees the kernel body's outputs bind to separate device slots, so a
/// backward companion's N gradient outputs each land in their own buffer.
#[must_use = "the result-tuple contract is either coherent or not; callers must act on it"]
pub fn multi_output_contract_valid(signature: &MirKernelSignature) -> bool {
    use crate::abi::{MirKernelResourceAccess, MirKernelResourceKind};
    let mut bindings = std::collections::BTreeSet::new();
    for output in signature.output_resources() {
        if output.kind != MirKernelResourceKind::StorageBuffer
            || output.access != MirKernelResourceAccess::Write
            || !bindings.insert(output.binding)
        {
            return false;
        }
    }
    true
}

/// R6/P1: per-output write extents for a result-tuple (multi-output)
/// signature, in binding order (the first output resource, then the extra
/// tuple elements). The plan pass records one extent per output resource; the
/// emitters guard each output's write with its own extent, so a smaller
/// buffer is never overrun and a larger one is never left partial under the
/// union dispatch.
#[must_use]
pub fn multi_output_output_extents(signature: &MirKernelSignature) -> Vec<u64> {
    signature
        .output_resources()
        .map(|output| output.element_count)
        .collect()
}

/// R6/P1: the dispatch extent for a result-tuple (multi-output) kernel — the
/// max (union) of every output resource's element count, so one launch covers
/// ALL output buffers. Never derive the dispatch from the first output's
/// count alone (the P1 shortcut): with unequal extents that either overruns a
/// smaller buffer or leaves a larger one incomplete. Each output's write is
/// still guarded by its own per-output extent in the emitters.
#[must_use]
pub fn multi_output_dispatch_extent(signature: &MirKernelSignature) -> u64 {
    multi_output_output_extents(signature)
        .into_iter()
        .max()
        .unwrap_or(0)
}

impl CollectionKernelPlan {
    /// Build the plan from a resolved kernel signature and collection-op
    /// contract — the plan pass, a sibling of `abi::collection_op_contract`
    /// (L5: two passes, never merged).
    ///
    /// `op` is the collection intrinsic resolved from the kernel body's
    /// statement scan (the same scan `abi::collection_op_contract` runs); the
    /// contract itself does not distinguish `Sum` from `TensorMean`, so the
    /// op is threaded through here. `matmul_shapes` carries the rank-2
    /// operand dims `(M, K, N)` resolved from the matmul operands' tensor
    /// types in the same scan — the contract stops at the tile grid, the
    /// recipe needs the exact dims (K-tile loop count, output guard, OOB
    /// tails). `signature` must already carry the contract it was built from
    /// (the shape `apply_collection_op_contract` produces: corrected
    /// workgroup, dispatch, and output count) — a mismatched signature yields
    /// `None` rather than a plan that assumes a dispatch the host will not
    /// honor.
    ///
    /// Returns `None` for elementwise kernels (no plan needed) and for
    /// contract/op/shape mismatches (the kernel keeps the elementwise
    /// fallback).
    #[must_use]
    pub fn from_contract(
        signature: &MirKernelSignature,
        contract: &CollectionOpContract,
        op: MirCollectionOp,
        matmul_shapes: Option<MatMulShapes>,
    ) -> Option<CollectionKernelPlan> {
        match *contract {
            // PGC-R5 (verdict 91fe3d59): the row-reduction family. The
            // contract owns only launch geometry (one workgroup per row),
            // never plan facts: both ops are transformer recipe ops, so
            // every live resolution path (the whole-function scan, the
            // subchain seam, the emitters' cross-check) resolves the frozen
            // RmsNormalization/CausalMaskedSoftmax plans through
            // `from_transformer_op` with the SAME signature (element
            // counts are the generic 1D facts). This arm exists only to
            // keep the contract match exhaustive; a caller that reaches it
            // has routed a row-reduction op through the contract plan pass
            // and fails closed (None) instead of guessing plan facts.
            CollectionOpContract::RowReduction { .. } => None,
            CollectionOpContract::Reduction {
                length,
                workgroup_x,
                partials,
            } => {
                let reduce_op = match op {
                    MirCollectionOp::Sum => ReduceOp::Sum,
                    MirCollectionOp::TensorMean => ReduceOp::Mean,
                    MirCollectionOp::TensorMax => ReduceOp::Max,
                    MirCollectionOp::TensorMin => ReduceOp::Min,
                    _ => return None,
                };
                if !signature_matches_reduction(signature, length, workgroup_x, partials) {
                    return None;
                }
                Some(CollectionKernelPlan::TreeReduction(ReductionPlan {
                    op: reduce_op,
                    length,
                    workgroup_x,
                    partials,
                    shared_memory: reduction_shared_memory(workgroup_x),
                    barriers: reduction_barrier_schedule(workgroup_x),
                    oob_padding: OobPaddingPolicy::ZeroFill,
                }))
            }
            CollectionOpContract::MatMul {
                dispatch_x,
                dispatch_y,
            } => {
                if op != MirCollectionOp::TensorMatMul {
                    return None;
                }
                let Some(shapes) = matmul_shapes else {
                    // The recipe needs the exact operand dims; the contract
                    // alone cannot reconstruct them from the tile grid.
                    return None;
                };
                let tile = MATMUL_TILE;
                // U1 head-axis: the batch facts derive from the signature's
                // own element counts — the ABI resource extents of the
                // operand tensors (`batch·M·K` left, `right_batch·K·N`
                // right, `batch·M·N` output). An exact division with a
                // positive quotient is a typed-fact derivation, never a
                // guess: anything else fails closed below.
                let (batch, right_batch) = matmul_batch_from_signature(signature, shapes)?;
                if !shapes_match_matmul_dispatch(shapes, dispatch_x, dispatch_y, tile, batch) {
                    return None;
                }
                if !signature_matches_matmul(signature, dispatch_x, dispatch_y, shapes) {
                    return None;
                }
                Some(CollectionKernelPlan::TiledMatMul(MatMulPlan {
                    dims: crate::kernel_plan::MatMulDims::Literal {
                        m: shapes.m,
                        k: shapes.k,
                        n: shapes.n,
                    },
                    m: shapes.m,
                    k: shapes.k,
                    n: shapes.n,
                    left_layout: shapes.left_layout,
                    right_layout: shapes.right_layout,
                    tile,
                    workgroup_x: signature.workgroup.x,
                    workgroup_y: signature.workgroup.y,
                    shared_memory: matmul_shared_memory(tile),
                    barriers: matmul_barrier_schedule(),
                    oob_padding: OobPaddingPolicy::ZeroFill,
                    batch,
                    right_batch,
                }))
            }
            // Run-time extents (design note item 5). The plan is built at emit
            // time but the shape class is chosen at launch (item 7), so the
            // plan records the one class that is correct for every shape,
            // `Tiled` (its guards express the M, N and K tails); item 7
            // substitutes the launch-selected class on a copy of this plan.
            // The literal `m`/`k`/`n` fields are unused (zero) in this arm.
            CollectionOpContract::MatMulRuntime => {
                if op != MirCollectionOp::TensorMatMul {
                    return None;
                }
                if !signature_matches_matmul_runtime(signature) {
                    return None;
                }
                let tile = MATMUL_TILE;
                Some(CollectionKernelPlan::TiledMatMul(MatMulPlan {
                    dims: crate::kernel_plan::MatMulDims::Runtime {
                        class: crate::kernel_plan::MatMulClass::Tiled,
                    },
                    m: 0,
                    k: 0,
                    n: 0,
                    left_layout: MirScalarLayout::F32,
                    right_layout: MirScalarLayout::F32,
                    tile,
                    workgroup_x: signature.workgroup.x,
                    workgroup_y: signature.workgroup.y,
                    shared_memory: matmul_shared_memory(tile),
                    barriers: matmul_barrier_schedule(),
                    oob_padding: OobPaddingPolicy::ZeroFill,
                    batch: 1,
                    right_batch: 1,
                }))
            }
            CollectionOpContract::Transpose { m, n } => {
                if op != MirCollectionOp::TensorTranspose {
                    return None;
                }
                if !signature_matches_transpose(signature, m, n) {
                    return None;
                }
                Some(CollectionKernelPlan::Transpose(TransposePlan {
                    m,
                    n,
                    workgroup_x: signature.workgroup.x,
                    dispatch_x: signature.dispatch_size.x,
                    batched: None,
                }))
            }
            CollectionOpContract::BatchedTranspose { batch, m, n } => {
                if op != MirCollectionOp::TensorTranspose {
                    return None;
                }
                // U3 head-axis: the batch fact derives from the signature's
                // own element counts — the operand's full extent
                // (`batch·M·N`) with the trailing dims already pinned by the
                // contract. An exact division with a positive quotient
                // agreeing with the carried batch is a typed-fact
                // cross-check, never a guess; anything else fails closed.
                if !signature_matches_batched_transpose(signature, batch, m, n) {
                    return None;
                }
                Some(CollectionKernelPlan::Transpose(TransposePlan {
                    m,
                    n,
                    workgroup_x: signature.workgroup.x,
                    dispatch_x: signature.dispatch_size.x,
                    batched: Some(batch),
                }))
            }
            CollectionOpContract::QuantizedMatMul {
                dispatch_x,
                dispatch_y,
                m,
                k,
                n,
                layout,
            } => {
                if op != MirCollectionOp::TensorQuantizedMatMul {
                    return None;
                }
                let shapes = MatMulShapes {
                    m,
                    k,
                    n,
                    left_layout: MirScalarLayout::F32,
                    right_layout: MirScalarLayout::F32,
                };
                if !shapes_match_matmul_dispatch(shapes, dispatch_x, dispatch_y, MATMUL_TILE, 1) {
                    return None;
                }
                if !signature_matches_matmul(signature, dispatch_x, dispatch_y, shapes) {
                    return None;
                }
                // R-PACK-02A: F32 falls back to TiledMatMul; packed layouts
                // keep QuantizedMatMul. The signature already matches the
                // shared tile grid.
                Some(super::admit_quantized_matmul(layout, shapes))
            }
            CollectionOpContract::GroupedMatMul {
                dispatch_x,
                dispatch_y,
                m,
                k,
                n,
                groups,
                layout,
            } => {
                if op != MirCollectionOp::TensorGroupedMatMul {
                    return None;
                }
                let shapes = MatMulShapes {
                    m,
                    k,
                    n,
                    left_layout: MirScalarLayout::F32,
                    right_layout: MirScalarLayout::F32,
                };
                if !shapes_match_matmul_dispatch(shapes, dispatch_x, dispatch_y, MATMUL_TILE, 1) {
                    return None;
                }
                if !signature_matches_grouped_matmul(
                    signature, dispatch_x, dispatch_y, shapes, groups,
                ) {
                    return None;
                }
                Some(CollectionKernelPlan::GroupedMatMul(GroupedMatMulPlan {
                    m,
                    k,
                    n,
                    groups,
                    layout,
                    tile: MATMUL_TILE,
                    workgroup_x: signature.workgroup.x,
                    workgroup_y: signature.workgroup.y,
                }))
            }
            CollectionOpContract::BatchedAttention {
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
            } => {
                if op != MirCollectionOp::TensorBatchedAttention
                    || signature.dispatch_size.x != output_elements
                    || signature.dispatch_size.y != 1
                    || signature.dispatch_size.z != 1
                    || signature.workgroup != (MirWorkgroupSize { x: 64, y: 1, z: 1 })
                {
                    return None;
                }
                Some(CollectionKernelPlan::BatchedAttention(
                    BatchedAttentionPlan {
                        kind,
                        kv_heads,
                        q_per_kv,
                        seq,
                        head_dim,
                        hidden,
                        layout,
                        rotate_half,
                        output_strides: attention_output_strides(
                            kind, q_per_kv, seq, head_dim, hidden,
                        ),
                        scale_bits,
                        decode_outputs_per_workgroup: attention_decode_outputs_per_workgroup(
                            kind, seq,
                        ),
                    },
                ))
            }
            CollectionOpContract::SsmConv1d {
                length,
                channels,
                kernel_width,
                layout,
            } => {
                if op != MirCollectionOp::TensorSsmConv1d {
                    return None;
                }
                if !signature_matches_ssm_extent(signature, length.saturating_mul(channels)) {
                    return None;
                }
                Some(CollectionKernelPlan::SsmConv1d(SsmConv1dPlan {
                    length,
                    channels,
                    kernel_width,
                    layout,
                }))
            }
            CollectionOpContract::SsmScan {
                length,
                state_dim,
                layout,
            } => {
                if op != MirCollectionOp::TensorSsmScan {
                    return None;
                }
                if !signature_matches_ssm_extent(signature, length.saturating_mul(state_dim)) {
                    return None;
                }
                Some(CollectionKernelPlan::SsmScan(SsmScanPlan {
                    length,
                    state_dim,
                    layout,
                }))
            }
        }
    }

    /// Resolve the declared distributed sum-reduce (`summa ex … filum …`,
    /// SFR-5) — the declared-recipe admission path.
    ///
    /// Like the transformer recipes, the filum sum has **no**
    /// `CollectionOpContract` variant: the declaration facts come from the
    /// [`MirIntrinsic::FilumReduce`] runtime call the module lowering emitted
    /// (filum metadata, term-body MIR, coordinate binders), and `length` is
    /// the source buffer's element count resolved from the kernel signature
    /// — never guessed from the body. The tree-floor facts
    /// (`workgroup_x`, `shared_memory`, `barriers`, `oob_padding`) mirror
    /// the [`CollectionKernelPlan::TreeReduction`] recipe exactly, so a
    /// backend with no native collective arm emits the shared-memory tree
    /// floor by construction (the plan law of this module).
    ///
    /// Returns `None` on a degenerate declaration (zero lane width or
    /// length); the caller fails closed with a typed diagnostic.
    #[must_use]
    pub fn from_filum_summa(
        filum: &crate::MirFilumReduceOp,
        length: u64,
    ) -> Option<CollectionKernelPlan> {
        let workgroup_x = filum.lane_width;
        if workgroup_x == 0 || !workgroup_x.is_power_of_two() || length == 0 {
            return None;
        }
        let partials = length.div_ceil(u64::from(workgroup_x));
        Some(CollectionKernelPlan::DistributedSum(DistributedSumPlan {
            filum: filum.clone(),
            length,
            workgroup_x,
            partials,
            shared_memory: reduction_shared_memory(workgroup_x),
            barriers: reduction_barrier_schedule(workgroup_x),
            oob_padding: OobPaddingPolicy::ZeroFill,
        }))
    }

    /// Resolve the transformer-op recipe plans (`AxisReduction` /
    /// `RowSoftmax` / `LayerNormalization`) — the S6-P1 admission path.
    ///
    /// The transformer ops have **no** `CollectionOpContract` variant: the
    /// ABI's `collection_op_contract` resolves none for them, so their
    /// signatures keep the generic 1D contract the ABI applies to
    /// non-collection kernels. The plan pass therefore resolves these
    /// recipes directly from the kernel function's typed facts:
    ///
    /// - `TensorSumAxis` → [`CollectionKernelPlan::AxisReduction`] with
    ///   `op: Sum` and the axis from the trailing integer const operand (the
    ///   S6-N1-frozen `AxisReductionPlan`);
    /// - `TensorSoftmax` → [`CollectionKernelPlan::RowSoftmax`] with the
    ///   last axis (the row-wise, numerically stable softmax recipe);
    /// - `TensorLayerNorm` → [`CollectionKernelPlan::LayerNormalization`]
    ///   with the last axis, epsilon from the const operand, and the
    ///   optional affine operand shapes cross-checked.
    ///
    /// The resolved plan is cross-checked against the signature's element
    /// counts (input/output buffers must carry the flattened input/output
    /// dims) — the A3 typed cross-check: recipe facts are carried facts, never
    /// inferred from names or bindings. Rank 1-2 only (the runner's v1
    /// constraint for all three ops); anything else yields `None` and the
    /// caller fails closed with a typed diagnostic.
    ///
    /// Returns `None` on any resolution or signature mismatch (the kernel
    /// keeps the fail-closed typed diagnostic of the caller).
    #[must_use]
    pub fn from_transformer_op(
        signature: &MirKernelSignature,
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        op: MirCollectionOp,
    ) -> Option<CollectionKernelPlan> {
        match op {
            MirCollectionOp::TensorSumAxis => {
                let (dims, axis) = transformer_recipe_shape(function, validation, op)?;
                let input_count = dims_product(&dims)?;
                let output_count = dims_without_axis_product(&dims, axis)?;
                if !signature_matches_transformer_counts(signature, input_count, output_count) {
                    return None;
                }
                Some(CollectionKernelPlan::AxisReduction(AxisReductionPlan {
                    op: ReduceOp::Sum,
                    axis,
                    projection: axis_reduction_projection(&dims, axis)?,
                }))
            }
            MirCollectionOp::TensorSoftmax | MirCollectionOp::TensorLayerNorm => {
                let (dims, axis) = transformer_recipe_shape(function, validation, op)?;
                let count = dims_product(&dims)?;
                if !signature_matches_transformer_counts(signature, count, count) {
                    return None;
                }
                if op == MirCollectionOp::TensorSoftmax && dims.len() > 2 {
                    // U2 head-axis: a rank-3/4 softmax is the batched row
                    // law — leading head/query axes fold into the row
                    // count, the last axis stays the row width, and the
                    // additive-idiom mask (when present) rides the plan as
                    // an operand fact.
                    return batched_row_softmax_plan(signature, function, validation, &dims, axis);
                }
                let plan = if op == MirCollectionOp::TensorSoftmax {
                    CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan {
                        axis,
                        batched: None,
                    })
                } else {
                    CollectionKernelPlan::LayerNormalization(LayerNormalizationPlan { axis })
                };
                Some(plan)
            }
            MirCollectionOp::TensorRmsNorm => rms_norm_plan(signature, function, validation, op),
            MirCollectionOp::TensorRope | MirCollectionOp::TensorRopeHalf => {
                rope_plan(signature, function, validation, op)
            }
            MirCollectionOp::TensorGather => {
                // GI3-1: the row-gather recipe resolves the rank-2 `[V, D]`
                // table dims and the id-vector length `T` (from the output
                // row count) and cross-checks the signature's input/output
                // element counts plus the ids extra-input count.
                let (table_rows, table_cols, id_count) = gather_recipe_facts(function, validation)?;
                // R-PACK-02A: optional trailing GGML type-id const. Packed
                // tables admit QuantizedGather; unrepresentable ids fail
                // closed. The carried plan stays Gather (no wire change).
                if let Some(operand) =
                    super::trailing_layout_operand(function, MirCollectionOp::TensorGather)
                {
                    super::packed_layout_from_operand(operand).ok()?;
                }
                let input_count = table_rows.saturating_mul(table_cols);
                let output_count = id_count.saturating_mul(table_cols);
                if !signature_matches_transformer_counts(signature, input_count, output_count) {
                    return None;
                }
                if signature.extra_inputs.len() != 1
                    || signature.extra_inputs[0].element_count != id_count
                {
                    return None;
                }
                Some(CollectionKernelPlan::Gather(GatherPlan {
                    table_rows,
                    table_cols,
                    id_count,
                }))
            }
            MirCollectionOp::TensorCausalMaskedSoftmax => {
                // GI3-1: the causal masked-row softmax recipe resolves the
                // rank-2 causal score matrix dims (row = query extent, col =
                // key extent) and keeps the same shape end to end.
                let (dims, _axis) = transformer_recipe_shape(function, validation, op)?;
                if dims.len() != 2 {
                    return None;
                }
                let count = dims_product(&dims)?;
                if !signature_matches_transformer_counts(signature, count, count) {
                    return None;
                }
                Some(CollectionKernelPlan::CausalMaskedSoftmax(
                    CausalMaskedSoftmaxPlan {
                        rows: dims[0],
                        cols: dims[1],
                    },
                ))
            }
            _ => None,
        }
    }

    /// Validate the plan before any emitter consumes it (L12).
    ///
    /// Checks, per recipe:
    /// - barrier dominance: every shared-memory read is dominated by a
    ///   barrier after the corresponding write;
    /// - output-write exclusivity: each output element is written exactly
    ///   once across the grid;
    /// - shared-memory sizing: `slot_count * element_byte_width` within the
    ///   device workgroup-memory limit;
    /// - OOB-padding coverage: the padding policy covers the tail range of
    ///   the last workgroup.
    ///
    /// Matmul adds: the workgroup equals the `tile × tile` grid (PRP-004
    /// U2), both operand tiles are sized `tile × tile` and within the
    /// workgroup-memory limit, the per-K-tile barrier pair is present and
    /// ordered, and the zero-fill policy covers both operand tile tails.
    ///
    /// # Errors
    /// Returns [`PlanValidationError`] when any check fails. Elementwise
    /// kernels always validate.
    pub fn validate(&self) -> Result<(), PlanValidationError> {
        match self {
            CollectionKernelPlan::TreeReduction(plan) => validate_reduction_plan(plan),
            CollectionKernelPlan::TiledMatMul(plan) => validate_matmul_plan(plan),
            CollectionKernelPlan::Transpose(plan) => validate_transpose_plan(plan),
            CollectionKernelPlan::AxisReduction(plan) => validate_axis_reduction_plan(plan),
            // S6-P1: the row-softmax and layer-normalization recipes carry
            // no static grid/shared-memory structure — every algorithmic
            // fact (last-axis constraint, epsilon const/finite, affine
            // operand shapes, element counts) is a resolution-time typed
            // check in [`CollectionKernelPlan::from_transformer_op`].
            // U2 head-axis: a batched row-softmax image additionally pins
            // the batch/mask counts law.
            CollectionKernelPlan::RowSoftmax(plan) => validate_row_softmax_plan(plan),
            // GI3-1: the inference recipes carry no static grid/shared-memory
            // structure — every algorithmic fact (axis/epsilon/gamma-width,
            // pos/dim/row-width, table/id dims, causal extents) is a
            // resolution-time typed check in
            // [`CollectionKernelPlan::from_transformer_op`]; the static
            // validators pin the positive-dimension floors.
            CollectionKernelPlan::Gather(plan) => validate_gather_plan(plan),
            CollectionKernelPlan::RmsNormalization(plan) => validate_rms_normalization_plan(plan),
            CollectionKernelPlan::Rope(plan) => validate_rope_plan(plan),
            CollectionKernelPlan::CausalMaskedSoftmax(plan) => {
                validate_causal_masked_softmax_plan(plan)
            }
            CollectionKernelPlan::QuantizedMatMul(plan) => validate_quantized_matmul_plan(plan),
            CollectionKernelPlan::GroupedMatMul(plan) => validate_grouped_matmul_plan(plan),
            CollectionKernelPlan::BatchedAttention(plan) => validate_batched_attention_plan(plan),
            CollectionKernelPlan::SsmConv1d(plan) => validate_ssm_conv1d_plan(plan),
            CollectionKernelPlan::SsmScan(plan) => validate_ssm_scan_plan(plan),
            CollectionKernelPlan::DistributedSum(plan) => validate_distributed_sum_plan(plan),
            // gpu-reset C4: a lane grid carries no static grid structure; the
            // lane domain is a call-site fact checked by `geometry_for_call`.
            CollectionKernelPlan::Elementwise
            | CollectionKernelPlan::LayerNormalization(_)
            | CollectionKernelPlan::LaneGrid { .. }
            | CollectionKernelPlan::SingleLane => Ok(()),
            CollectionKernelPlan::GridReduce {
                workgroup,
                partials_cap,
                ..
            } => {
                if *workgroup == 0 || !workgroup.is_power_of_two() {
                    return Err(PlanValidationError::NonPowerOfTwoWorkgroup {
                        workgroup_x: *workgroup,
                    });
                }
                if *partials_cap == 0 {
                    return Err(PlanValidationError::GridReduceZeroPartialsCap);
                }
                Ok(())
            }
        }
    }
}

/// The batched-attention plan's row-major output strides (the last element of
/// each tuple is the contiguous axis). The projection and score families
/// stride over `[rows, cols, ...]`; the sequence-major context keeps the
/// `hidden`-wide contiguous vector.
fn attention_output_strides(
    kind: BatchedAttentionKind,
    q_per_kv: u64,
    seq: u64,
    head_dim: u64,
    hidden: u64,
) -> [u64; 4] {
    match kind {
        BatchedAttentionKind::ProjectionQ => [
            q_per_kv.saturating_mul(seq).saturating_mul(head_dim),
            seq.saturating_mul(head_dim),
            head_dim,
            1,
        ],
        BatchedAttentionKind::ProjectionK | BatchedAttentionKind::ProjectionV => {
            [seq.saturating_mul(head_dim), head_dim, 1, 1]
        }
        BatchedAttentionKind::Scores | BatchedAttentionKind::Softmax => [
            q_per_kv.saturating_mul(seq).saturating_mul(seq),
            seq.saturating_mul(seq),
            seq,
            1,
        ],
        BatchedAttentionKind::ContextSequenceMajor => [hidden, 1, 1, 1],
    }
}

/// R4 repair: the M=1 projection body computes this many outputs per
/// 64-thread workgroup, so the same typed fact must drive the launch grid.
/// `seq` is the contract's step-row count (decode = 1); the score, softmax
/// and context kinds keep the generic one-thread-per-output law (`None`).
fn attention_decode_outputs_per_workgroup(kind: BatchedAttentionKind, seq: u64) -> Option<u64> {
    if seq == 1
        && matches!(
            kind,
            BatchedAttentionKind::ProjectionQ
                | BatchedAttentionKind::ProjectionK
                | BatchedAttentionKind::ProjectionV
        )
    {
        Some(crate::abi::PROJECTION_GEMV_DISPATCH_LANES)
    } else {
        None
    }
}

fn rope_plan(
    signature: &MirKernelSignature,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    op: MirCollectionOp,
) -> Option<CollectionKernelPlan> {
    // GI3-1: the RoPE recipe resolves pos/dim from the trailing
    // const operands and the row width from the operand dims.
    // The caller-provided interleaved `[rows, dim/2, 3]` table
    // is the explicit fourth MIR operand; no unrelated kernel
    // parameter is eligible as a table. `TensorRopeHalf` selects
    // Qwen/NeoX rotate_half pairing; `TensorRope` stays llama
    // consecutive-pair.
    let (dims, axis) = transformer_recipe_shape(function, validation, op)?;
    let count = dims_product(&dims)?;
    if !signature_matches_transformer_counts(signature, count, count) {
        return None;
    }
    let (pos, dim, width, per_row, rows) = rope_plan_facts(function, validation, op, &dims, axis)?;
    Some(CollectionKernelPlan::Rope(RopePlan {
        pos,
        dim,
        width,
        per_row,
        rows,
        rotate_half: matches!(op, MirCollectionOp::TensorRopeHalf),
    }))
}

fn rms_norm_plan(
    signature: &MirKernelSignature,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    op: MirCollectionOp,
) -> Option<CollectionKernelPlan> {
    // GI3-1: the RMSNorm recipe resolves the typed facts like the
    // S6-P1 transformer recipes — dims from the operand tensor,
    // axis from the trailing const operand (must be the last
    // axis for the v1 row-wise recipe), epsilon from the const
    // operand, and the affine gamma peer width cross-checked.
    let (dims, axis) = transformer_recipe_shape(function, validation, op)?;
    let count = dims_product(&dims)?;
    if !signature_matches_transformer_counts(signature, count, count) {
        return None;
    }
    let (epsilon_bits, width, gamma_local) =
        rms_norm_const_facts(function, validation, &dims, axis)?;
    // Gamma is data, but it must still be present in the typed
    // kernel resource signature. This keeps plan admission tied
    // to the source ABI without adding gamma to the wire plan.
    let gamma_resource = signature.extra_inputs.iter().find(|resource| {
        resource.source_local == Some(gamma_local) && resource.element_count == width
    })?;
    if gamma_resource.role != MirKernelResourceRole::Input
        || gamma_resource.access != MirKernelResourceAccess::Read
    {
        return None;
    }
    Some(CollectionKernelPlan::RmsNormalization(
        RmsNormalizationPlan {
            axis,
            epsilon_bits,
            width,
        },
    ))
}

/// Barrier schedule for the tree-reduction recipe: one post-load barrier
/// (publishes each lane's partial to its own shared slot), then one barrier
/// after every tree halving step (`log2(workgroup_x)` steps for the
/// power-of-two workgroup).
pub(super) fn reduction_barrier_schedule(workgroup_x: u32) -> Vec<BarrierPoint> {
    let mut barriers = vec![BarrierPoint {
        after: BarrierPhase::SharedMemoryLoad,
    }];
    barriers.extend((0..workgroup_x.trailing_zeros()).map(|_| BarrierPoint {
        after: BarrierPhase::ReductionStep,
    }));
    barriers
}

/// Shared-memory layout of the tree-floor reduction: one f32 slot per
/// workgroup lane, the slots the tree halving steps combine in place.
fn reduction_shared_memory(workgroup_x: u32) -> SharedMemoryLayout {
    SharedMemoryLayout {
        element_byte_width: F32_ELEMENT_BYTE_WIDTH,
        slot_count: workgroup_x,
        buffer_name: REDUCTION_SHARED_BUFFER_NAME.to_owned(),
    }
}

/// Per-K-tile barrier schedule for the tiled-matmul recipe: the
/// post-cooperative-load barrier (publishes the loaded A/B tiles to the
/// inner product), then the post-inner-product-step barrier (publishes the
/// read completion before the next K tile's cooperative load overwrites the
/// buffers). The K-tile loop repeats this pair `ceil(k / tile)` times.
pub(crate) fn matmul_barrier_schedule() -> Vec<BarrierPoint> {
    vec![
        BarrierPoint {
            after: BarrierPhase::SharedMemoryLoad,
        },
        BarrierPoint {
            after: BarrierPhase::InnerProductStep,
        },
    ]
}

/// The matmul recipe's two shared-memory operand tiles: `tile` × `tile` f32
/// slots each, the cooperative-load targets the per-K-tile barrier pair
/// guards.
fn matmul_shared_memory(tile: u32) -> MatMulSharedMemory {
    MatMulSharedMemory {
        shared_a: SharedMemoryLayout {
            element_byte_width: F32_ELEMENT_BYTE_WIDTH,
            slot_count: tile * tile,
            buffer_name: MATMUL_SHARED_A_BUFFER_NAME.to_owned(),
        },
        shared_b: SharedMemoryLayout {
            element_byte_width: F32_ELEMENT_BYTE_WIDTH,
            slot_count: tile * tile,
            buffer_name: MATMUL_SHARED_B_BUFFER_NAME.to_owned(),
        },
    }
}

/// The matmul shapes must reproduce the contract's tile grid: `dispatch_y`
/// tiles the `M` rows and `dispatch_x` tiles the `N` columns. A
/// shapes/dispatch mismatch means the caller resolved shapes that disagree
/// with the contract the signature carries. A batched plan (U1 head-axis)
/// folds the batch into `dispatch_y` — one padded row-block of
/// `ceil(M/tile)` tiles per batch — so an ordinary rank-2 launch
/// (`batch == 1`) keeps the legacy law byte-identically.
fn shapes_match_matmul_dispatch(
    shapes: MatMulShapes,
    dispatch_x: u64,
    dispatch_y: u64,
    tile: u32,
    batch: u64,
) -> bool {
    let tile = u64::from(tile);
    if batch == 1 {
        if shapes.m == 1 {
            dispatch_y == 1 && dispatch_x == shapes.n.div_ceil(tile)
        } else {
            dispatch_y == shapes.m.div_ceil(tile) && dispatch_x == shapes.n.div_ceil(tile)
        }
    } else {
        dispatch_y == batch * shapes.m.div_ceil(tile) && dispatch_x == shapes.n.div_ceil(tile)
    }
}

/// U1 head-axis: derive the batch counts from the signature's own resource
/// extents. The left operand buffer holds `batch·M·K` elements and the
/// right `right_batch·K·N`; both quotients must divide exactly and be
/// positive, and the output must hold exactly `batch·M·N` — any
/// contradiction fails closed (`None`) rather than yielding a plan that
/// disagrees with the buffers the launch binds.
fn matmul_batch_from_signature(
    signature: &MirKernelSignature,
    shapes: MatMulShapes,
) -> Option<(u64, u64)> {
    if shapes.m == 0 || shapes.k == 0 || shapes.n == 0 {
        return None;
    }
    let left_count = signature.input.element_count;
    let right_count = signature
        .extra_inputs
        .first()
        .map_or(0, |resource| resource.element_count);
    if left_count == 0 || right_count == 0 {
        return None;
    }
    let batch = left_count / (shapes.m * shapes.k);
    let right_batch = right_count / (shapes.k * shapes.n);
    if batch == 0
        || right_batch == 0
        || batch * shapes.m * shapes.k != left_count
        || right_batch * shapes.k * shapes.n != right_count
        || !batch.is_multiple_of(right_batch)
        || signature.output.element_count != batch * shapes.m * shapes.n
    {
        return None;
    }
    Some((batch, right_batch))
}

/// The reduction signature must already carry the contract it was built
/// from: `apply_collection_op_contract` sets workgroup `(ws, 1, 1)`,
/// dispatch `(length, 1, 1)`, and output count `partials`.
fn signature_matches_reduction(
    signature: &MirKernelSignature,
    length: u64,
    workgroup_x: u32,
    partials: u64,
) -> bool {
    signature.workgroup.x == workgroup_x
        && signature.workgroup.y == 1
        && signature.workgroup.z == 1
        && signature.dispatch_size.x == length
        && signature.dispatch_size.y == 1
        && signature.dispatch_size.z == 1
        && signature.output.element_count == partials
}

/// The tiled-matmul recipes' launch workgroup: the `MATMUL_TILE` ×
/// `MATMUL_TILE` cooperative tile grid, one thread per tile slot.
///
/// WHY: three signature matchers (`signature_matches_matmul`,
/// `signature_matches_matmul_runtime`, `signature_matches_grouped_matmul`)
/// pin the same workgroup; one predicate keeps them from drifting apart.
fn matmul_workgroup_matches(signature: &MirKernelSignature) -> bool {
    signature.workgroup.x == MATMUL_TILE
        && signature.workgroup.y == MATMUL_TILE
        && signature.workgroup.z == 1
}

/// The matmul signature must already carry the contract it was built from:
/// `apply_collection_op_contract` sets workgroup `(8, 8, 1)`, the tile-grid
/// dispatch, and the operand buffers hold the flat `batch·M·K` (left) /
/// `right_batch·K·N` (right) element counts with the `batch·M·N` output —
/// `batch == 1` is the ordinary rank-2 law. The count law is the batch
/// derivation itself (`matmul_batch_from_signature`): a signature whose
/// extents contradict the shapes yields no batch facts and no plan.
fn signature_matches_matmul(
    signature: &MirKernelSignature,
    dispatch_x: u64,
    dispatch_y: u64,
    shapes: MatMulShapes,
) -> bool {
    if matmul_batch_from_signature(signature, shapes).is_none() {
        return false;
    }
    matmul_workgroup_matches(signature)
        && signature.dispatch_size.x == dispatch_x
        && signature.dispatch_size.y == dispatch_y
        && signature.dispatch_size.z == 1
        && signature.extra_inputs.len() == 1
}

/// The run-time-extent matmul signature must carry the runtime marker: an
/// 8x8x1 workgroup, the right operand as the first extra input, at least one
/// operand or output buffer that is runtime-shaped, and at least one
/// `TensorAxisExtent` resource (the extents the emitter reads). The dispatch
/// is a launch fact, so it is not checked.
fn signature_matches_matmul_runtime(signature: &MirKernelSignature) -> bool {
    use crate::abi::MirKernelResourceKind;
    let has_extent = signature.extra_inputs.iter().any(|resource| {
        matches!(
            resource.kind,
            MirKernelResourceKind::TensorAxisExtent { .. }
        )
    });
    let runtime_shaped = signature.input.is_runtime_shaped()
        || signature.output.is_runtime_shaped()
        || signature
            .extra_inputs
            .first()
            .is_some_and(crate::abi::MirKernelResource::is_runtime_shaped);
    matmul_workgroup_matches(signature) && has_extent && runtime_shaped
}

/// The grouped-matmul signature must already carry the contract it was built
/// from: workgroup `(MATMUL_TILE, MATMUL_TILE, 1)`, the tile-grid dispatch,
/// left operand `M·K`, output `M·N`, and the grouped right operand
/// `groups·K·N`.
fn signature_matches_grouped_matmul(
    signature: &MirKernelSignature,
    dispatch_x: u64,
    dispatch_y: u64,
    shapes: MatMulShapes,
    groups: u64,
) -> bool {
    matmul_workgroup_matches(signature)
        && signature.dispatch_size.x == dispatch_x
        && signature.dispatch_size.y == dispatch_y
        && signature.dispatch_size.z == 1
        && signature.input.element_count == shapes.m.saturating_mul(shapes.k)
        && signature.output.element_count == shapes.m.saturating_mul(shapes.n)
        && signature.extra_inputs.len() == 1
        && signature.extra_inputs[0].element_count
            == groups.saturating_mul(shapes.k).saturating_mul(shapes.n)
}

fn signature_matches_ssm_extent(signature: &MirKernelSignature, extent: u64) -> bool {
    signature.workgroup.x == TRANSPOSE_WORKGROUP_X
        && signature.workgroup.y == 1
        && signature.workgroup.z == 1
        && signature.dispatch_size.x == extent
        && signature.dispatch_size.y == 1
        && signature.dispatch_size.z == 1
        && signature.input.element_count == extent
        && signature.output.element_count == extent
}

/// The transpose signature must already carry the contract it was built
/// from: `apply_collection_op_contract` sets workgroup
/// `(TRANSPOSE_WORKGROUP_X, 1, 1)`, dispatch `(M·N, 1, 1)`, and the input and
/// output buffers each hold the flat `M·N` element count (the transposed
/// `[N,M]` tensor has the same element count as the `[M,N]` source).
fn signature_matches_transpose(signature: &MirKernelSignature, m: u64, n: u64) -> bool {
    let count = m.saturating_mul(n);
    signature.workgroup.x == TRANSPOSE_WORKGROUP_X
        && signature.workgroup.y == 1
        && signature.workgroup.z == 1
        && signature.dispatch_size.x == count
        && signature.dispatch_size.y == 1
        && signature.dispatch_size.z == 1
        && signature.input.element_count == count
        && signature.output.element_count == count
}

/// U3 head-axis: the batched transpose signature cross-check — the same
/// elementwise-style 1D contract over the batch-folded thread count, with
/// the input/output resources carrying the operands' full `batch·M·N`
/// extents and the carried batch agreeing with the input's own count.
fn signature_matches_batched_transpose(
    signature: &MirKernelSignature,
    batch: u64,
    m: u64,
    n: u64,
) -> bool {
    let Some(count) = batch.checked_mul(m).and_then(|count| count.checked_mul(n)) else {
        return false;
    };
    signature.workgroup.x == TRANSPOSE_WORKGROUP_X
        && signature.workgroup.y == 1
        && signature.workgroup.z == 1
        && signature.dispatch_size.x == count
        && signature.dispatch_size.y == 1
        && signature.dispatch_size.z == 1
        && signature.input.element_count == count
        && signature.output.element_count == count
}

/// The transformer-recipe signature cross-check: the input and output
/// buffers must carry the flattened input and output dims of the recipe
/// (the shape facts the ABI's generic 1D contract produces for these ops).
///
/// The workgroup/dispatch are deliberately not pinned here: the transformer
/// recipes have no `CollectionOpContract` variant, so the generic 1D
/// contract the ABI applies is the only dispatch the signature carries, and
/// the emitter units (S6-P2/P3) own the row-wise thread mapping under it.
fn signature_matches_transformer_counts(
    signature: &MirKernelSignature,
    input_count: u64,
    output_count: u64,
) -> bool {
    signature.input.element_count == input_count && signature.output.element_count == output_count
}

/// U2 head-axis: resolve the batched row-softmax facts from the softmax
/// statement's own typed facts. `dims` are the softmax operand's literal
/// dims (rank 3-4, already count-checked against the signature): the
/// leading head/query axes fold into the batch (the row count), the last
/// axis is the row width, and the additive-idiom mask — when the operand
/// is the `(scores + mask)` temp — rides the plan as an operand fact
/// cross-checked against the signature's own resources. A rank-3/4
/// operand that is a NON-additive elementwise temp fails closed (the
/// batched body reads raw buffers only, v1).
fn batched_row_softmax_plan(
    signature: &MirKernelSignature,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    dims: &[u64],
    axis: u64,
) -> Option<CollectionKernelPlan> {
    let width = *dims.last()?;
    let batch = dims[..dims.len() - 1]
        .iter()
        .copied()
        .try_fold(1_u64, u64::checked_mul)?;
    if batch == 0 || width == 0 {
        return None;
    }
    let args = transformer_statement_args(function, MirCollectionOp::TensorSoftmax)?;
    let operand = args.first()?;
    let operand_is_temp = match operand {
        MirOperand::Temp(_) => true,
        MirOperand::Place(place) => matches!(place.base, MirPlaceBase::Temp(_)),
        _ => false,
    };
    // A plain local operand is the unmasked batched softmax; a temp
    // operand must be the additive `(scores + mask)` idiom (anything else
    // fails closed — the batched body reads raw buffers only, v1).
    let mask = if operand_is_temp {
        Some(batched_softmax_mask_facts(
            signature, function, validation, dims, operand,
        )?)
    } else {
        None
    };
    Some(CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan {
        axis,
        batched: Some(RowSoftmaxBatch { batch, width, mask }),
    }))
}

/// The additive-mask operand facts of a batched softmax whose operand is
/// the `(scores + mask)` temp: the mask dims must satisfy the broadcast
/// law and the signature must carry both addend buffers at their own
/// extents (the A3 cross-check — the resource for each addend local must
/// exist, be read-only, and hold exactly the addend's element count).
/// `None` fails closed.
fn batched_softmax_mask_facts(
    signature: &MirKernelSignature,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    score_dims: &[u64],
    operand: &MirOperand,
) -> Option<RowSoftmaxMask> {
    let (scores_local, mask_local, count) =
        softmax_additive_mask_locals(function, operand, validation)?;
    let addends = additive_mask_add_args(function, operand)?;
    let receiver_dims = transformer_operand_dims(function, &addends[0], validation)?;
    if receiver_dims.as_slice() != score_dims {
        return None;
    }
    let total = dims_product(score_dims)?;
    // A3 cross-check: both addend locals resolve to read-only kernel
    // resources holding exactly their own element counts.
    let resource_count = |local: MirLocalId, expected: u64| {
        signature.resources().any(|resource| {
            resource.source_local == Some(local)
                && resource.access == MirKernelResourceAccess::Read
                && resource.element_count == expected
        })
    };
    if !resource_count(scores_local, total) || !resource_count(mask_local, count) {
        return None;
    }
    Some(RowSoftmaxMask { count })
}

/// Resolve the additive softmax's raw scores and mask buffers. Explicit
/// expansion is admitted only when its proven flat index is `flat % count`.
#[must_use]
pub fn softmax_additive_mask_locals(
    function: &MirFunction,
    operand: &MirOperand,
    validation: &MirValidationContext<'_>,
) -> Option<(MirLocalId, MirLocalId, u64)> {
    let args = additive_mask_add_args(function, operand)?;
    let scores = operand_local_id(&args[0])?;
    let score_dims = transformer_operand_dims(function, &args[0], validation)?;
    if let Some(mask) = operand_local_id(&args[1]) {
        let mask_dims = transformer_operand_dims(function, &args[1], validation)?;
        return Some((scores, mask, mask_broadcast_count(&score_dims, &mask_dims)?));
    }
    // Expansion producers in other blocks need an explicit dominance proof.
    if function.blocks.len() != 1 {
        return None;
    }
    let mask_base = match &args[1] {
        MirOperand::Temp(temp) => MirPlaceBase::Temp(*temp),
        MirOperand::Place(place) if place.projections.is_empty() => place.base.clone(),
        _ => return None,
    };
    let mut producer = None;
    for statement in function.blocks.iter().flat_map(|block| &block.statements) {
        if let Some(destination) = crate::read_analysis::statement_destination_place(statement)
            && destination.base == mask_base
        {
            // Multiple definitions or projected writes need a dominance proof.
            if producer.is_some() || !destination.projections.is_empty() {
                return None;
            }
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                return None;
            };
            producer = Some(call);
        }
    }
    let call = producer?;
    let expansion = crate::tensor_expansion::TensorExpansion::for_call(function, call, validation)?;
    if expansion.result_shape != score_dims {
        return None;
    }
    let count = mask_broadcast_count(&score_dims, &expansion.source_shape)?;
    // Every non-singleton digit must occupy the same row-major stride as
    // the source. Together with the trailing-full-block shape law, this
    // proves the expansion's exact flat mapping is modulo source count.
    if expansion
        .index_terms()
        .iter()
        .any(|(_, target_stride, _, source_stride)| target_stride != source_stride)
    {
        return None;
    }
    Some((scores, operand_local_id(call.args.first()?)?, count))
}

/// The `(scores, mask)` local operands of the `TensorAdd` statement whose
/// destination is the softmax's additive-idiom temp (the same statement
/// scan [`crate::abi::softmax_operand_is_additive_mask_temp`] admits).
fn additive_mask_add_args<'a>(
    function: &'a MirFunction,
    operand: &MirOperand,
) -> Option<&'a [MirOperand]> {
    let destination_base = match operand {
        MirOperand::Temp(temp) => MirPlaceBase::Temp(*temp),
        MirOperand::Place(place) => place.base.clone(),
        _ => return None,
    };
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall {
                destination: Some(destination),
                call:
                    MirRuntimeCall {
                        intrinsic: crate::MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
                        args,
                        ..
                    },
            } = &statement.kind
            else {
                continue;
            };
            if destination.base == destination_base && args.len() == 2 {
                return Some(args);
            }
        }
    }
    None
}

/// The local id of a plain local-place operand (`None` for temps,
/// constants, and projected places — the additive idiom's addends are
/// plain locals by construction).
fn operand_local_id(operand: &MirOperand) -> Option<MirLocalId> {
    match operand {
        MirOperand::Place(place) => match place.base {
            MirPlaceBase::Local(local) if place.projections.is_empty() => Some(local),
            _ => None,
        },
        _ => None,
    }
}

/// U2 head-axis mask broadcast law: the mask dims (right-aligned against
/// the score dims) must be a trailing block of FULL dims — leading 1s are
/// broadcast, and from the mask's first non-1 dim onward every mask dim
/// must equal its aligned score dim. Under that law (and only it) the
/// flat score index's trailing digits ARE the mask's flat index, so the
/// body reads `mask[flat % count]`; any other geometry fails closed
/// (`None`). Returns the mask's element count on admission.
fn mask_broadcast_count(score_dims: &[u64], mask_dims: &[u64]) -> Option<u64> {
    if mask_dims.is_empty() || mask_dims.len() > score_dims.len() {
        return None;
    }
    let offset = score_dims.len() - mask_dims.len();
    let cutoff = mask_dims
        .iter()
        .position(|&dim| dim != 1)
        .unwrap_or(mask_dims.len());
    for (index, &dim) in mask_dims.iter().enumerate() {
        if index >= cutoff && dim != score_dims[offset + index] {
            return None;
        }
    }
    mask_dims
        .iter()
        .copied()
        .try_fold(1_u64, u64::checked_mul)
        .filter(|count| *count > 0)
}

/// The transformer recipe's typed shape facts: the literal dims of the
/// operand tensor and the resolved axis, cross-checked against the op's
/// const-operand constraints.
///
/// - `TensorSumAxis`: axis from the trailing `MirConstant::Int` const
///   operand, in range for the rank (v1: rank 1-2);
/// - `TensorSoftmax`: the last axis (row-wise), rank 1-2;
/// - `TensorLayerNorm`: the last axis (v1), the const axis must equal it,
///   the epsilon const must be finite and non-negative, and the optional
///   affine pair must be rank-1 over the normalization dimension.
///
/// `None` when the op statement is absent, the operand has no static tensor
/// dims, a const constraint fails, or the shape is outside the v1 surface.
pub(crate) fn transformer_recipe_shape(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    op: MirCollectionOp,
) -> Option<(Vec<u64>, u64)> {
    match op {
        MirCollectionOp::TensorSumAxis => {
            let args = transformer_statement_args(function, MirCollectionOp::TensorSumAxis)?;
            let dims = transformer_operand_dims(function, &args[0], validation)?;
            let rank = dims.len();
            if rank == 0 || rank > 2 {
                // v1: rank 1-2 only (the runner's constraint).
                return None;
            }
            let axis = const_non_negative(args.get(1)?)?;
            if axis >= rank as u64 {
                return None;
            }
            Some((dims, axis))
        }
        MirCollectionOp::TensorSoftmax => {
            let args = transformer_statement_args(function, MirCollectionOp::TensorSoftmax)?;
            let dims = transformer_operand_dims(function, &args[0], validation)?;
            let rank = dims.len();
            // U2 head-axis: rank 1-4 — the batched softmax carries leading
            // head/query axes over the same last-axis row law; rank 5+
            // stays outside the admitted surface.
            if rank == 0 || rank > 4 {
                return None;
            }
            Some((dims, (rank - 1) as u64))
        }
        MirCollectionOp::TensorLayerNorm => {
            let args = transformer_statement_args(function, MirCollectionOp::TensorLayerNorm)?;
            let dims = transformer_operand_dims(function, &args[0], validation)?;
            let rank = dims.len();
            if rank == 0 || rank > 2 {
                return None;
            }
            let axis = (rank - 1) as u64;
            // v1: layer norm normalizes over the last axis; the const axis
            // must agree (the plan never infers facts, A3).
            if const_non_negative(args.get(1)?)? != axis {
                return None;
            }
            if !layer_norm_const_facts(function, validation, &dims, axis) {
                return None;
            }
            Some((dims, axis))
        }
        MirCollectionOp::TensorRmsNorm => {
            // GI3-1: RMSNorm normalizes over the last axis in v1 (row-wise,
            // like softmax/layer-norm); the const axis must agree.
            let args = transformer_statement_args(function, MirCollectionOp::TensorRmsNorm)?;
            let dims = transformer_operand_dims(function, &args[0], validation)?;
            let rank = dims.len();
            if rank == 0 || rank > 2 {
                return None;
            }
            let axis = (rank - 1) as u64;
            if const_non_negative(args.get(1)?)? != axis {
                return None;
            }
            Some((dims, axis))
        }
        MirCollectionOp::TensorRope | MirCollectionOp::TensorRopeHalf => {
            // GI3-1: RoPE rotates on the last axis. Pairing is a plan fact
            // (`rotate_half`); the shape contract is shared.
            let args = transformer_statement_args(function, op)?;
            let dims = transformer_operand_dims(function, &args[0], validation)?;
            let rank = dims.len();
            if rank == 0 || rank > 2 {
                return None;
            }
            Some((dims, (rank - 1) as u64))
        }
        MirCollectionOp::TensorCausalMaskedSoftmax => {
            // GI3-1: the causal masked-row softmax consumes a rank-2 causal
            // score matrix `[rows, cols]`.
            let args =
                transformer_statement_args(function, MirCollectionOp::TensorCausalMaskedSoftmax)?;
            let dims = transformer_operand_dims(function, &args[0], validation)?;
            if dims.len() != 2 {
                return None;
            }
            Some((dims, 1))
        }
        MirCollectionOp::TensorGather => {
            // GI3-1: the row-gather table is a rank-2 `[V, D]` tensor; the
            // id count resolves separately from the return-type dims (the
            // shape facts the plan pass cross-checks).
            let args = transformer_statement_args(function, MirCollectionOp::TensorGather)?;
            let dims = transformer_operand_dims(function, &args[0], validation)?;
            if dims.len() != 2 {
                return None;
            }
            Some((dims, 1))
        }
        _ => None,
    }
}

/// The `TensorLayerNorm` const-operand facts: the epsilon must be a
/// `MirConstant::Float` (a const — never a value that would require
/// inference, A3) that is finite and non-negative, and an optional affine
/// pair (gamma, beta) must each be rank-1 tensors of the normalization
/// dimension when present (the runner's v1 constraint).
fn layer_norm_const_facts(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    dims: &[u64],
    axis: u64,
) -> bool {
    let Some(args) = transformer_statement_args(function, MirCollectionOp::TensorLayerNorm) else {
        return false;
    };
    if args.len() < 3 || args.len() > 5 {
        return false;
    }
    // The epsilon is a const the source frontend may have materialized
    // through a single const temp (floating-point literals are), exactly as
    // the RMSNorm epsilon is read below; matching only a direct constant
    // rejected every source-level layer norm.
    let Some(MirConstant::Float(epsilon)) = literal_const_operand(function, &args[2]) else {
        return false;
    };
    if !epsilon.is_finite() || epsilon < 0.0 {
        return false;
    }
    let Some(norm_index) = usize::try_from(axis)
        .ok()
        .filter(|index| *index < dims.len())
    else {
        return false;
    };
    let norm_dim = dims[norm_index];
    for affine in args.iter().skip(3) {
        let Some(affine_ty) = crate::local_ty::operand_local_ty(function, affine) else {
            return false;
        };
        let Ok(affine_dims) = crate::tensor_type::tensor_literal_dims(affine_ty, validation) else {
            return false;
        };
        // v1: gamma/beta are rank-1 over the normalization dimension.
        if affine_dims.len() != 1 || affine_dims[0] != norm_dim {
            return false;
        }
    }
    true
}

/// Resolve a literal constant operand, including the single const-temp
/// materialization the source frontend uses for floating-point literals.
/// The resolver stays typed and local: it does not infer values from names or
/// arbitrary expressions, and ambiguous/multi-assignment temps fail closed.
#[must_use]
pub fn literal_const_operand(function: &MirFunction, operand: &MirOperand) -> Option<MirConstant> {
    if let MirOperand::Constant(constant) = operand {
        return Some(constant.clone());
    }
    let temp = match operand {
        MirOperand::Temp(temp) => *temp,
        MirOperand::Place(place) if place.projections.is_empty() => {
            let MirPlaceBase::Temp(temp) = place.base else {
                return None;
            };
            temp
        }
        _ => return None,
    };
    let mut found = None;
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            if place.projections.is_empty() && place.base == MirPlaceBase::Temp(temp) {
                let MirValueKind::Operand(MirOperand::Constant(constant)) = &value.kind else {
                    return None;
                };
                if found.is_some() {
                    return None;
                }
                found = Some(constant);
            }
        }
    }
    found.cloned()
}

/// The `TensorRmsNorm` const-operand facts (GI3-1): the epsilon must be a
/// finite, positive `MirConstant::Float`, either directly or through the
/// frontend's single literal-const temp (never an inferred value, A3), and
/// the mandatory affine
/// gamma peer must be a rank-1 tensor of the normalization dimension (the
/// CCI-1 source contract). Returns the f32 epsilon bits (the CCI-1
/// `epsilon_bits` precedent — the op-precision f32 bits) when literal, or
/// zero when the explicit scalar epsilon is dynamic; the normalization width,
/// and the gamma source local. `None` means a const or typed-peer constraint
/// failed.
fn rms_norm_const_facts(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    dims: &[u64],
    axis: u64,
) -> Option<(u32, u64, MirLocalId)> {
    let args = transformer_statement_args(function, MirCollectionOp::TensorRmsNorm)?;
    if args.len() != 4 {
        return None;
    }
    let epsilon_bits = match literal_const_operand(function, &args[2]) {
        Some(MirConstant::Float(epsilon)) => {
            // WHY: the device recipe stores the operation-precision f32 bits;
            // reject overflow and underflow after IEEE-754 narrowing.
            #[allow(clippy::cast_possible_truncation)]
            let epsilon = epsilon as f32;
            if !epsilon.is_finite() || epsilon <= 0.0 {
                return None;
            }
            epsilon.to_bits()
        }
        Some(_) => return None,
        None => {
            let epsilon_local = crate::read_analysis::operand_local(&args[2])?;
            let epsilon_ty = crate::local_ty::local_ty(function, epsilon_local)?;
            if !validation
                .layouts
                .layout_for_type(epsilon_ty)
                .ok()
                .is_some_and(|layout| matches!(layout.kind, MirLayoutKind::Scalar(_)))
            {
                return None;
            }
            0
        }
    };
    let norm_index = usize::try_from(axis)
        .ok()
        .filter(|index| *index < dims.len())?;
    let norm_dim = dims[norm_index];
    // Gamma is the mandatory affine peer (args[3]); the CCI-1 source
    // contract enforces a rank-1 tensor of the normalization dimension.
    let gamma_ty = crate::local_ty::operand_local_ty(function, &args[3])?;
    let Ok(gamma_dims) = crate::tensor_type::tensor_literal_dims(gamma_ty, validation) else {
        return None;
    };
    if gamma_dims.len() != 1 || gamma_dims[0] != norm_dim {
        return None;
    }
    let gamma_local = crate::read_analysis::operand_local(&args[3])?;
    Some((epsilon_bits, norm_dim, gamma_local))
}

/// The `TensorRope` / `TensorRopeHalf` const-operand facts (GI3-1): `pos`
/// must be a non-negative `MirConstant::Int`/`UInt` and `dim` a positive,
/// even `MirConstant::Int`/`UInt` not exceeding the last-axis extent (the
/// CCI-1 source contract; the MIR validator re-checks). The fourth operand
/// must be the caller-provided interleaved `[rows, dim/2, 3]` tensor. Returns
/// `(pos, dim, width)`; `None` when a const or table-shape constraint fails.
fn rope_plan_facts(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    op: MirCollectionOp,
    dims: &[u64],
    axis: u64,
) -> Option<(u64, u64, u64, bool, u64)> {
    if !op.is_rope() {
        return None;
    }
    let args = transformer_statement_args(function, op)?;
    if args.len() != 4 {
        return None;
    }
    let pos = const_non_negative(args.get(1)?)?;
    let dim = const_non_negative(args.get(2)?)?;
    if dim == 0 || !dim.is_multiple_of(2) {
        return None;
    }
    let axis_index = usize::try_from(axis)
        .ok()
        .filter(|index| *index < dims.len())?;
    let width = dims[axis_index];
    if dim > width {
        return None;
    }
    let table_local = crate::read_analysis::operand_local(args.get(3)?)?;
    let table_ty = crate::local_ty::local_ty(function, table_local)?;
    let table_dims = crate::tensor_type::tensor_literal_dims(table_ty, validation).ok()?;
    let expected_rows = dims
        .iter()
        .take(dims.len().saturating_sub(1))
        .copied()
        .product::<u64>();
    let expected_pairs = dim / 2;
    if table_dims != [expected_rows, expected_pairs, 3] {
        return None;
    }
    Some((pos, dim, width, true, expected_rows))
}

/// A non-negative integer const operand (the pos/dim consts of `RoPE` and the
/// axis consts of the transformer recipes).
fn const_non_negative(operand: &MirOperand) -> Option<u64> {
    match operand {
        MirOperand::Constant(MirConstant::Int(value)) => u64::try_from(*value).ok(),
        MirOperand::Constant(MirConstant::UInt(value)) => Some(*value),
        _ => None,
    }
}

/// The `TensorGather` recipe facts (GI3-1): the rank-2 `[V, D]` table dims
/// and the id-vector length `T`. `T` resolves from the gather statement's
/// return-type dims (`[T, D]`, the shape-transforming result of the CCI-1
/// source contract) divided by the table row width — the CCI-1 contract
/// validated the ids' *literal* values only, so the plan carries the count
/// as a typed fact, never inferred from text (A3); the emitted body
/// bounds-checks the runtime token against the table row count (SEC-G1).
/// `None` when the table is not rank-2, the return shape is not `[T, D]`,
/// or the width does not divide evenly.
fn gather_recipe_facts(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Option<(u64, u64, u64)> {
    let args = transformer_statement_args(function, MirCollectionOp::TensorGather)?;
    let dims = transformer_operand_dims(function, &args[0], validation)?;
    if dims.len() != 2 {
        return None;
    }
    let table_rows = dims[0];
    let table_cols = dims[1];
    if table_rows == 0 || table_cols == 0 {
        return None;
    }
    let return_dims =
        transformer_statement_return_dims(function, MirCollectionOp::TensorGather, validation)?;
    if return_dims.len() != 2 || return_dims[1] != table_cols {
        return None;
    }
    // K7c: the ids are a `tensor<u32, [n]>` whose own extent is the row
    // count, so it must agree with the return rows (the extent travels with
    // the value); a disagreeing pair fails closed.
    let ids_dims = transformer_operand_dims(function, &args[1], validation)?;
    if ids_dims != [return_dims[0]] {
        return None;
    }
    Some((table_rows, table_cols, return_dims[0]))
}

/// The literal dims of the first statement carrying `op`'s return type (the
/// shape-transforming result the gather recipe sizes its output from).
fn transformer_statement_return_dims(
    function: &MirFunction,
    op: MirCollectionOp,
    validation: &MirValidationContext<'_>,
) -> Option<Vec<u64>> {
    for block in &function.blocks {
        for statement in &block.statements {
            if crate::collection_ops::statement_collection_op(statement) != Some(op) {
                continue;
            }
            let MirStatementKind::RuntimeCall {
                call: MirRuntimeCall { return_ty, .. },
                ..
            } = &statement.kind
            else {
                continue;
            };
            return crate::tensor_type::tensor_literal_dims(*return_ty, validation).ok();
        }
    }
    None
}

/// The args of the first statement carrying `op` in the function body (the
/// typed-fact scan the transformer recipes resolve from).
fn transformer_statement_args(
    function: &MirFunction,
    op: MirCollectionOp,
) -> Option<&[MirOperand]> {
    for block in &function.blocks {
        for statement in &block.statements {
            if crate::collection_ops::statement_collection_op(statement) != Some(op) {
                continue;
            }
            let MirStatementKind::RuntimeCall {
                call: MirRuntimeCall { args, .. },
                ..
            } = &statement.kind
            else {
                continue;
            };
            return Some(args);
        }
    }
    None
}

/// The literal dims of a transformer recipe operand (the tensor the recipe
/// reads).
fn transformer_operand_dims(
    function: &MirFunction,
    operand: &MirOperand,
    validation: &MirValidationContext<'_>,
) -> Option<Vec<u64>> {
    // PGC-R5: the additive-idiom softmax reads an elementwise TEMP
    // (`(x + mask)` — the GEA2-U5e surface); its type lives in the temps
    // table, which the local-only `operand_local_ty` cannot read (the same
    // temp-aware lookup the ABI contract resolver applies).
    let ty = crate::local_ty::operand_local_ty(function, operand).or_else(|| {
        let temp = match operand {
            MirOperand::Temp(temp) => Some(*temp),
            MirOperand::Place(place) => match place.base {
                MirPlaceBase::Temp(temp) => Some(temp),
                MirPlaceBase::Local(_) => None,
            },
            _ => None,
        }?;
        function
            .temps
            .iter()
            .find(|entry| entry.id == temp)
            .map(|entry| entry.ty)
    })?;
    crate::tensor_type::tensor_literal_dims(ty, validation).ok()
}

/// The flattened element count of the literal dims.
fn dims_product(dims: &[u64]) -> Option<u64> {
    if dims.is_empty() {
        return None;
    }
    let mut product = 1u64;
    for &dim in dims {
        product = product.saturating_mul(dim);
    }
    Some(product)
}

/// The flattened element count of the dims with `axis` removed — the output
/// shape of an axis reduction (`result rank = input rank - 1`; a rank-1
/// reduction over its only axis yields the scalar 1-element output).
fn dims_without_axis_product(dims: &[u64], axis: u64) -> Option<u64> {
    let axis = usize::try_from(axis).ok()?;
    if axis >= dims.len() {
        return None;
    }
    let mut product = 1u64;
    for (index, &dim) in dims.iter().enumerate() {
        if index != axis {
            product = product.saturating_mul(dim);
        }
    }
    Some(product)
}

/// The producer-defined reduced-buffer projection of an axis reduction
/// (council-8 CB-3): `axis_extent` = `dims[axis]` (the per-row reduction
/// width) and `inner_stride` = the product of the dims after the axis (`1`
/// when the axis is last). Carried into each consuming resource/version so
/// consumers never reconstruct the projection from element count.
pub(crate) fn axis_reduction_projection(
    dims: &[u64],
    axis: u64,
) -> Option<crate::abi::ReducedProjection> {
    let axis = usize::try_from(axis).ok()?;
    if axis >= dims.len() {
        return None;
    }
    let mut inner_stride = 1u64;
    for &dim in &dims[axis + 1..] {
        inner_stride = inner_stride.saturating_mul(dim);
    }
    Some(crate::abi::ReducedProjection {
        axis_extent: dims[axis],
        inner_stride,
    })
}
