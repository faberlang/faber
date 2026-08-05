use super::plan::{
    AxisReductionPlan, BarrierPhase, BarrierPoint, CollectionKernelPlan, LayerNormalizationPlan,
    MatMulPlan, MatMulShapes, MatMulSharedMemory, OobPaddingPolicy, ReduceOp, ReductionPlan,
    RowSoftmaxPlan, SharedMemoryLayout, TransposePlan, F32_ELEMENT_BYTE_WIDTH,
    MATMUL_SHARED_A_BUFFER_NAME, MATMUL_SHARED_B_BUFFER_NAME, MATMUL_TILE,
    REDUCTION_SHARED_BUFFER_NAME, TRANSPOSE_WORKGROUP_X,
};
use super::validate::{
    validate_axis_reduction_plan, validate_matmul_plan, validate_reduction_plan,
    validate_transpose_plan, PlanValidationError,
};
use crate::abi::{CollectionOpContract, MirKernelSignature};
use crate::{
    MirCollectionOp, MirConstant, MirFunction, MirOperand, MirRuntimeCall, MirStatementKind,
    MirValidationContext,
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
