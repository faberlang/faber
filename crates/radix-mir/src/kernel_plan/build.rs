use super::plan::{
    BarrierPhase, BarrierPoint, CollectionKernelPlan, MatMulPlan, MatMulShapes, MatMulSharedMemory,
    OobPaddingPolicy, ReduceOp, ReductionPlan, SharedMemoryLayout, TransposePlan,
    F32_ELEMENT_BYTE_WIDTH, MATMUL_SHARED_A_BUFFER_NAME, MATMUL_SHARED_B_BUFFER_NAME, MATMUL_TILE,
    REDUCTION_SHARED_BUFFER_NAME, TRANSPOSE_WORKGROUP_X,
};
use super::validate::{
    validate_matmul_plan, validate_reduction_plan, validate_transpose_plan, PlanValidationError,
};
use crate::abi::{CollectionOpContract, MirKernelSignature};
use crate::MirCollectionOp;

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
