use super::plan::{
    AxisReductionPlan, BarrierPhase, BarrierPoint, CausalMaskedSoftmaxPlan, CollectionKernelPlan,
    GatherPlan, LayerNormalizationPlan, MatMulPlan, MatMulShapes, MatMulSharedMemory,
    OobPaddingPolicy, ReduceOp, ReductionPlan, RmsNormalizationPlan, RopePlan, RowSoftmaxPlan,
    SharedMemoryLayout, TransposePlan, F32_ELEMENT_BYTE_WIDTH, MATMUL_SHARED_A_BUFFER_NAME,
    MATMUL_SHARED_B_BUFFER_NAME, MATMUL_TILE, REDUCTION_SHARED_BUFFER_NAME, TRANSPOSE_WORKGROUP_X,
};
use super::validate::{
    validate_axis_reduction_plan, validate_causal_masked_softmax_plan, validate_gather_plan,
    validate_matmul_plan, validate_reduction_plan, validate_rms_normalization_plan,
    validate_rope_plan, validate_transpose_plan, PlanValidationError,
};
use crate::abi::{CollectionOpContract, MirKernelSignature};
use crate::{
    MirCollectionOp, MirConstant, MirFunction, MirLocalId, MirOperand, MirParam, MirPlaceBase,
    MirRuntimeCall, MirStatementKind, MirValidationContext,
};
use radix_types::Type;

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
            CollectionOpContract::Reduction {
                length,
                workgroup_x,
                partials,
            } => {
                let reduce_op = match op {
                    MirCollectionOp::Sum => ReduceOp::Sum,
                    MirCollectionOp::TensorMean => ReduceOp::Mean,
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
                    shared_memory: SharedMemoryLayout {
                        element_byte_width: F32_ELEMENT_BYTE_WIDTH,
                        slot_count: workgroup_x,
                        buffer_name: REDUCTION_SHARED_BUFFER_NAME.to_owned(),
                    },
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
                if !shapes_match_matmul_dispatch(shapes, dispatch_x, dispatch_y, tile) {
                    return None;
                }
                if !signature_matches_matmul(signature, dispatch_x, dispatch_y, shapes) {
                    return None;
                }
                let tile_slots = tile * tile;
                Some(CollectionKernelPlan::TiledMatMul(MatMulPlan {
                    m: shapes.m,
                    k: shapes.k,
                    n: shapes.n,
                    tile,
                    workgroup_x: signature.workgroup.x,
                    workgroup_y: signature.workgroup.y,
                    shared_memory: MatMulSharedMemory {
                        shared_a: SharedMemoryLayout {
                            element_byte_width: F32_ELEMENT_BYTE_WIDTH,
                            slot_count: tile_slots,
                            buffer_name: MATMUL_SHARED_A_BUFFER_NAME.to_owned(),
                        },
                        shared_b: SharedMemoryLayout {
                            element_byte_width: F32_ELEMENT_BYTE_WIDTH,
                            slot_count: tile_slots,
                            buffer_name: MATMUL_SHARED_B_BUFFER_NAME.to_owned(),
                        },
                    },
                    barriers: matmul_barrier_schedule(),
                    oob_padding: OobPaddingPolicy::ZeroFill,
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
                }))
            }
        }
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
    ///   `op: Sum` and the axis from the trailing `MirConstant::Int` const
    ///   operand (the S6-N1-frozen `AxisReductionPlan`);
    /// - `TensorSoftmax` → [`CollectionKernelPlan::RowSoftmax`] with the
    ///   last axis (the row-wise, numerically stable softmax recipe);
    /// - `TensorLayerNorm` → [`CollectionKernelPlan::LayerNormalization`]
    ///   with the last axis, epsilon from the const operand, and the
    ///   optional affine operand shapes cross-checked.
    ///
    /// The resolved plan is cross-checked against the signature's element
    /// counts (input/output buffers must carry the flattened input/output
    /// dims) — the A3 typed cross-check: recipe facts are carried facts, never
    /// inferred from names or bindings. Rank 1-2 only (the stepper's v1
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
                let plan = if op == MirCollectionOp::TensorSoftmax {
                    CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan { axis })
                } else {
                    CollectionKernelPlan::LayerNormalization(LayerNormalizationPlan { axis })
                };
                Some(plan)
            }
            MirCollectionOp::TensorRmsNorm => {
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
                let (epsilon_bits, width) =
                    rms_norm_const_facts(function, validation, &dims, axis)?;
                Some(CollectionKernelPlan::RmsNormalization(
                    RmsNormalizationPlan {
                        axis,
                        epsilon_bits,
                        width,
                    },
                ))
            }
            MirCollectionOp::TensorRope => {
                // GI3-1: the RoPE recipe resolves pos/dim from the trailing
                // const operands and the row width from the operand dims
                // (the last-axis extent; elements beyond `dim` are
                // preserved unrotated). Per-position mode (the GI3-1
                // contract amendment) is carried by the cos/sin table
                // operand shapes — rank-2 `[rows, dim/2]` tables rotate
                // every row at its own position.
                let (dims, axis) = transformer_recipe_shape(function, validation, op)?;
                let count = dims_product(&dims)?;
                if !signature_matches_transformer_counts(signature, count, count) {
                    return None;
                }
                let (pos, dim, width, per_row, rows) =
                    rope_plan_facts(function, validation, &dims, axis)?;
                Some(CollectionKernelPlan::Rope(RopePlan {
                    pos,
                    dim,
                    width,
                    per_row,
                    rows,
                }))
            }
            MirCollectionOp::TensorGather => {
                // GI3-1: the row-gather recipe resolves the rank-2 `[V, D]`
                // table dims and the id-vector length `T` (from the output
                // row count) and cross-checks the signature's input/output
                // element counts plus the ids extra-input count.
                let (table_rows, table_cols, id_count) = gather_recipe_facts(function, validation)?;
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
            CollectionKernelPlan::Elementwise => Ok(()),
            CollectionKernelPlan::TreeReduction(plan) => validate_reduction_plan(plan),
            CollectionKernelPlan::TiledMatMul(plan) => validate_matmul_plan(plan),
            CollectionKernelPlan::Transpose(plan) => validate_transpose_plan(plan),
            CollectionKernelPlan::AxisReduction(plan) => validate_axis_reduction_plan(plan),
            // S6-P1: the row-softmax and layer-normalization recipes carry
            // no static grid/shared-memory structure — every algorithmic
            // fact (last-axis constraint, epsilon const/finite, affine
            // operand shapes, element counts) is a resolution-time typed
            // check in [`CollectionKernelPlan::from_transformer_op`].
            CollectionKernelPlan::RowSoftmax(_) | CollectionKernelPlan::LayerNormalization(_) => {
                Ok(())
            }
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
        }
    }
}

/// Barrier schedule for the tree-reduction recipe: one post-load barrier
/// (publishes each lane's partial to its own shared slot), then one barrier
/// after every tree halving step (`log2(workgroup_x)` steps for the
/// power-of-two workgroup).
pub(super) fn reduction_barrier_schedule(workgroup_x: u32) -> Vec<BarrierPoint> {
    let mut barriers = Vec::new();
    barriers.push(BarrierPoint {
        after: BarrierPhase::SharedMemoryLoad,
    });
    for _ in 0..workgroup_x.trailing_zeros() {
        barriers.push(BarrierPoint {
            after: BarrierPhase::ReductionStep,
        });
    }
    barriers
}

/// Per-K-tile barrier schedule for the tiled-matmul recipe: the
/// post-cooperative-load barrier (publishes the loaded A/B tiles to the
/// inner product), then the post-inner-product-step barrier (publishes the
/// read completion before the next K tile's cooperative load overwrites the
/// buffers). The K-tile loop repeats this pair `ceil(k / tile)` times.
pub(super) fn matmul_barrier_schedule() -> Vec<BarrierPoint> {
    vec![
        BarrierPoint {
            after: BarrierPhase::SharedMemoryLoad,
        },
        BarrierPoint {
            after: BarrierPhase::InnerProductStep,
        },
    ]
}

/// The matmul shapes must reproduce the contract's tile grid: `dispatch_y`
/// tiles the `M` rows and `dispatch_x` tiles the `N` columns. A
/// shapes/dispatch mismatch means the caller resolved shapes that disagree
/// with the contract the signature carries.
fn shapes_match_matmul_dispatch(
    shapes: MatMulShapes,
    dispatch_x: u64,
    dispatch_y: u64,
    tile: u32,
) -> bool {
    let tile = u64::from(tile);
    dispatch_y == shapes.m.div_ceil(tile) && dispatch_x == shapes.n.div_ceil(tile)
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

/// The matmul signature must already carry the contract it was built from:
/// `apply_collection_op_contract` sets workgroup `(8, 8, 1)`, the tile-grid
/// dispatch, and the operand buffers hold the flat `M·K` (left) / `K·N`
/// (right) element counts with the `M·N` output.
fn signature_matches_matmul(
    signature: &MirKernelSignature,
    dispatch_x: u64,
    dispatch_y: u64,
    shapes: MatMulShapes,
) -> bool {
    signature.workgroup.x == MATMUL_TILE
        && signature.workgroup.y == MATMUL_TILE
        && signature.workgroup.z == 1
        && signature.dispatch_size.x == dispatch_x
        && signature.dispatch_size.y == dispatch_y
        && signature.dispatch_size.z == 1
        && signature.input.element_count == shapes.m.saturating_mul(shapes.k)
        && signature.output.element_count == shapes.m.saturating_mul(shapes.n)
        && signature.extra_inputs.len() == 1
        && signature.extra_inputs[0].element_count == shapes.k.saturating_mul(shapes.n)
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
                // v1: rank 1-2 only (the stepper's constraint).
                return None;
            }
            let axis = const_axis(args.get(1)?)?;
            if axis >= rank as u64 {
                return None;
            }
            Some((dims, axis))
        }
        MirCollectionOp::TensorSoftmax => {
            let args = transformer_statement_args(function, MirCollectionOp::TensorSoftmax)?;
            let dims = transformer_operand_dims(function, &args[0], validation)?;
            let rank = dims.len();
            if rank == 0 || rank > 2 {
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
            if const_axis(args.get(1)?)? != axis {
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
            if const_axis(args.get(1)?)? != axis {
                return None;
            }
            Some((dims, axis))
        }
        MirCollectionOp::TensorRope => {
            // GI3-1: RoPE rotates consecutive pairs on the last axis.
            let args = transformer_statement_args(function, MirCollectionOp::TensorRope)?;
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
/// dimension when present (the stepper's v1 constraint).
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
    let MirOperand::Constant(MirConstant::Float(epsilon)) = &args[2] else {
        return false;
    };
    if !epsilon.is_finite() || *epsilon < 0.0 {
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

/// The `TensorRmsNorm` const-operand facts (GI3-1): the epsilon must be a
/// `MirConstant::Float` (a const — never a value that would require
/// inference, A3) that is finite and positive, and the mandatory affine
/// gamma peer must be a rank-1 tensor of the normalization dimension (the
/// CCI-1 source contract). Returns the f32 epsilon bits (the CCI-1
/// `epsilon_bits` precedent — the op-precision f32 bits) and the
/// normalization width; `None` when a const constraint fails.
fn rms_norm_const_facts(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    dims: &[u64],
    axis: u64,
) -> Option<(u32, u64)> {
    let args = transformer_statement_args(function, MirCollectionOp::TensorRmsNorm)?;
    if args.len() != 4 {
        return None;
    }
    let MirOperand::Constant(MirConstant::Float(epsilon)) = &args[2] else {
        return None;
    };
    // WHY: the device recipe stores the operation-precision f32 bits; reject
    // overflow and underflow after the IEEE-754 narrowing below.
    #[allow(clippy::cast_possible_truncation)]
    let epsilon = *epsilon as f32;
    if !epsilon.is_finite() || epsilon <= 0.0 {
        return None;
    }
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
    Some((epsilon.to_bits(), norm_dim))
}

/// The `TensorRope` const-operand facts (GI3-1): `pos` must be a
/// non-negative `MirConstant::Int`/`UInt` and `dim` a positive, even
/// `MirConstant::Int`/`UInt` not exceeding the last-axis extent (the CCI-1
/// source contract; the MIR validator re-checks). Returns
/// `(pos, dim, width)`; `None` when a const constraint fails.
fn rope_plan_facts(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    dims: &[u64],
    axis: u64,
) -> Option<(u64, u64, u64, bool, u64)> {
    let args = transformer_statement_args(function, MirCollectionOp::TensorRope)?;
    if args.len() != 3 {
        return None;
    }
    let pos = const_non_negative(args.get(1)?)?;
    let dim = const_non_negative(args.get(2)?)?;
    if dim == 0 || dim % 2 != 0 {
        return None;
    }
    let axis_index = usize::try_from(axis)
        .ok()
        .filter(|index| *index < dims.len())?;
    let width = dims[axis_index];
    if dim > width {
        return None;
    }
    // Per-position mode is a typed fact of the kernel's table operands: the
    // cos/sin tables are the two tensor params beyond the x operand and the
    // statement destination. Rank-2 `[rows, dim/2]` tables select per-row
    // rotation; rank-1 `[dim/2]` tables are the single-position mode.
    let x_local = crate::read_analysis::operand_local(args.first()?)?;
    let destination = rope_destination_local(function)?;
    let tables: Vec<&MirParam> = function
        .params
        .iter()
        .filter(|param| {
            param.local != x_local
                && param.local != destination
                && crate::local_ty::local_ty(function, param.local).is_some_and(|ty| {
                    matches!(validation.types.get(ty.semantic_id()), Type::Tensor(..))
                })
        })
        .collect();
    let (per_row, rows) = if tables.len() == 2 {
        let first_dims = crate::tensor_type::tensor_literal_dims(
            crate::local_ty::local_ty(function, tables[0].local)?,
            validation,
        )
        .ok()?;
        match first_dims.as_slice() {
            [_] => (false, 0),
            [rows, _] => (true, *rows),
            _ => return None,
        }
    } else {
        (false, 0)
    };
    Some((pos, dim, width, per_row, rows))
}

/// The `TensorRope` statement's destination local (the rotated output).
fn rope_destination_local(function: &MirFunction) -> Option<MirLocalId> {
    for block in &function.blocks {
        for statement in &block.statements {
            if crate::collection_ops::statement_collection_op(statement)
                != Some(MirCollectionOp::TensorRope)
            {
                continue;
            }
            let MirStatementKind::RuntimeCall { destination, .. } = &statement.kind else {
                continue;
            };
            let Some(place) = destination else {
                return None;
            };
            let MirPlaceBase::Local(local) = place.base else {
                return None;
            };
            return Some(local);
        }
    }
    None
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
/// source contract) divided by the table row width — the ids' literal
/// values were statically validated at the source surface, so the plan
/// carries the count as a typed fact, never inferred from text (A3).
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
    let ty = crate::local_ty::operand_local_ty(function, operand)?;
    crate::tensor_type::tensor_literal_dims(ty, validation).ok()
}

/// The axis const operand: a non-negative `MirConstant::Int` or a
/// `MirConstant::UInt` (the trailing const operands of the transformer
/// lowerings).
fn const_axis(operand: &MirOperand) -> Option<u64> {
    match operand {
        MirOperand::Constant(MirConstant::Int(axis)) => u64::try_from(*axis).ok(),
        MirOperand::Constant(MirConstant::UInt(axis)) => Some(*axis),
        _ => None,
    }
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
