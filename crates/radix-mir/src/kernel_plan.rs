//! Collection-op kernel plan: the shared algorithmic recipe for compute
//! backends (WGSL, Metal, NVVM).
//!
//! TARGET: the compute-host emitters (`radix-mir-wgsl`, `radix-mir-metal`,
//! `radix-mir-llvm`). The plan is the Phase 0 design-council elevation
//! decision (ELEVATE-WITH-CONDITIONS, 2026-08-02, locks L1-L16): the tiled
//! matmul and tree-reduction bodies the WGSL emitter used to own are captured
//! here as a closed recipe set that every backend lowers to target syntax.
//! The plan never names a target keyword (L4); backends own syntax, the plan
//! owns the algorithm.
//!
//! WHY: three full lanes porting the same barrier-critical, shared-memory,
//! OOB-sensitive algorithm is a correctness defect waiting to happen — the
//! reference body is dense with fix markers (PRP-004 U1/U2/U3, F-035, C5/C6,
//! W6-A2). One authored recipe, checked once, consumed by every backend.
//!
//! The plan is a sibling pass output to [`crate::abi::CollectionOpContract`]
//! (L5): the contract stops at dispatch/workgroup/output-count; the plan adds
//! the algorithm structure — shared-memory layout, barrier schedule, OOB
//! padding policy. [`CollectionKernelPlan::from_contract`] consumes the
//! resolved contract, the statement-scan collection op, and the matmul
//! operand shapes from the MIR statement scan.
//!
//! Reference algorithm: `radix-mir-wgsl/src/lib.rs:1470-1555` (tree
//! reduction) and `:1610-1700` (tiled matmul). The WGSL emitter stays the
//! parity oracle this campaign (L13); it is not refactored to consume the
//! plan.

use crate::abi::{CollectionOpContract, MirKernelSignature};
use crate::MirCollectionOp;
use std::fmt;

/// Conservative device workgroup-memory limit in bytes (L12). Real devices
/// advertise more (the M5 Max and RTX 5070 exceed this); 32KB keeps every
/// emitted kernel portable until a per-device limit channel exists.
pub const WORKGROUP_SHARED_MEMORY_LIMIT_BYTES: u64 = 32 * 1024;

/// f32 element byte width. L7: this delivery lowers f32 shared-memory
/// algorithms only.
pub const F32_ELEMENT_BYTE_WIDTH: usize = 4;

/// Matmul tile edge (D1-A default). Mirrors the 8×8 tile grid the
/// [`CollectionOpContract::MatMul`] dispatch is built from; the plan keeps
/// the tile as a named constant so a backend never hardcodes it (L6).
pub const MATMUL_TILE: u32 = 8;

/// Logical shared-memory buffer name for the tree-reduction recipe.
///
/// Named `wg_shared`, not `shared`: `shared` is a WGSL reserved identifier —
/// Chrome rejects kernels that declare it (W6-A2 finding). The plan records
/// the logical identity; backends map it to their shared-memory declaration
/// syntax (`var<workgroup>` / `threadgroup` / `addrspace(3)`).
pub const REDUCTION_SHARED_BUFFER_NAME: &str = "wg_shared";

/// Logical shared-memory buffer name for the left-matrix operand tile (the
/// WGSL reference's `shared_a`). The matmul recipe has two distinct tiles;
/// `shared` alone is a WGSL reserved identifier (W6-A2 lesson), so backends
/// map these names onto their shared-memory declaration syntax.
pub const MATMUL_SHARED_A_BUFFER_NAME: &str = "shared_a";

/// Logical shared-memory buffer name for the right-matrix operand tile.
pub const MATMUL_SHARED_B_BUFFER_NAME: &str = "shared_b";

/// The closed recipe set for collection-op kernel bodies (L3).
///
/// Adding a variant is an explicit, reviewable change; the plan never carries
/// a backend tag. A backend either lowers a recipe to target syntax (default:
/// correct, portable) or overrides the whole body with a target fast path —
/// the override must still honor the [`CollectionOpContract`] dispatch (L4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionKernelPlan {
    /// No collection-op recipe: elementwise kernels keep the generic 1D
    /// contract. [`CollectionKernelPlan::from_contract`] returns `None` for
    /// these (the "no plan = elementwise" fallback, cto L5); the variant
    /// exists so consumers can represent a no-recipe kernel as a plan value
    /// when they prefer a uniform type.
    Elementwise,
    /// Tiled matmul recipe (cooperative K-tile load, barrier, inner-product
    /// accumulation). See [`MatMulPlan`].
    TiledMatMul(MatMulPlan),
    /// Sequential tree-reduction recipe (grid-stride partial + power-of-2
    /// halving + mean divisor).
    TreeReduction(ReductionPlan),
}

/// Matmul operand shapes `(M, K, N)`, resolved from the rank-2 tensor types
/// of the two matmul operands — the same literal dims
/// [`CollectionOpContract::MatMul`]'s dispatch is derived from
/// (`dispatch_y = ceil(M/8)`, `dispatch_x = ceil(N/8)`).
///
/// The contract carries only the tile grid; the recipe needs the exact dims
/// (K for the K-tile loop count, M/N for the output write guard and the OOB
/// tile tails), so the caller threads the shapes from the operand types in
/// the MIR statement scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatMulShapes {
    /// Left matrix rows `M`.
    pub m: u64,
    /// Contracted inner dimension `K` (left columns / right rows).
    pub k: u64,
    /// Right matrix columns `N`.
    pub n: u64,
}

/// Tiled-matmul recipe (WGSL `radix-mir-wgsl/src/lib.rs:1610-1700`).
///
/// Algorithmic invariants carried from the reference body:
/// - one workgroup per `tile × tile` output tile; the workgroup must equal
///   the tile on both axes (PRP-004 U2: tile-local addressing derives from
///   workgroup-local ids, so any other workgroup size aliases shared slots);
/// - `ceil(k / tile)` K-tile iterations; each iteration cooperatively loads
///   an A tile (`shared_a`) and a B tile (`shared_b`), barriers, then
///   accumulates the inner product `acc += shared_a[ty][kk] * shared_b[kk][tx]`;
/// - per-K-tile barrier schedule ([`MatMulPlan::barriers`]): the
///   post-cooperative-load barrier first, then the post-inner-product-step
///   barrier (the second publishes the read completion before the next K
///   tile's load overwrites the buffers);
/// - zero-filled tail tiles (L16): loads past `m×k` (A) / `k×n` (B) write 0;
/// - the output element is written from global row/col within `M×N`, once
///   per workgroup (F-035: each workgroup writes a distinct output tile).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatMulPlan {
    /// Left matrix rows `M`.
    pub m: u64,
    /// Contracted inner dimension `K` (left columns / right rows).
    pub k: u64,
    /// Right matrix columns `N`.
    pub n: u64,
    /// Square tile edge; the contract dispatches one workgroup per
    /// `tile × tile` output tile.
    pub tile: u32,
    /// Workgroup lane count on `x`; tile-local addressing is only correct
    /// when it equals `tile` (PRP-004 U2).
    pub workgroup_x: u32,
    /// Workgroup lane count on `y`; must equal `tile`.
    pub workgroup_y: u32,
    /// The two shared-memory operand tiles (`shared_a`, `shared_b`), each
    /// `tile × tile`.
    pub shared_memory: MatMulSharedMemory,
    /// Per-K-tile barrier schedule: the post-cooperative-load barrier first,
    /// then the post-inner-product-step barrier.
    pub barriers: Vec<BarrierPoint>,
    /// Zero-fill policy for the operand tail tiles (L16).
    pub oob_padding: OobPaddingPolicy,
}

/// The two shared-memory operand tiles of the tiled-matmul recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatMulSharedMemory {
    /// Left-matrix tile (`A[row][k_start + tx]`), `tile × tile` slots.
    pub shared_a: SharedMemoryLayout,
    /// Right-matrix tile (`B[k_start + ty][col]`), `tile × tile` slots.
    pub shared_b: SharedMemoryLayout,
}

/// Tree-reduction recipe (WGSL `radix-mir-wgsl/src/lib.rs:1470-1555`).
///
/// Algorithmic invariants carried from the reference body:
/// - grid-stride loop over the full tensor (`length` elements, stride
///   `workgroup_x * partials`) — every element in `[0, length)` is visited
///   exactly once by the union of all workgroups;
/// - one shared slot per lane ([`SharedMemoryLayout::slot_count`] ==
///   `workgroup_x`), named `wg_shared` (the W6-A2 reserved-word lesson);
/// - post-load barrier, then a sequential power-of-2 halving tree with a
///   barrier after every step ([`BarrierPhase::ReductionStep`]);
/// - [`ReduceOp::Mean`] divides the workgroup partial by the true `length`
///   (PRP-004 MEDIUM 1: the pre-fix kernel divided by `workgroup_x`);
/// - the per-workgroup partial lands in `output[workgroup_id.x]`, guarded by
///   `workgroup_id.x < partials` (PRP-004 U1: the pre-fix kernel wrote
///   `output[0]` from every workgroup);
/// - no OOB read is possible by construction (the grid-stride loop reads
///   only `g < length`); lanes past the tensor length accumulate nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReductionPlan {
    /// Sum or mean (mean divides by the true tensor length).
    pub op: ReduceOp,
    /// True flat tensor length `n` (PRP-004 U1: not the workgroup size).
    pub length: u64,
    /// Power-of-two workgroup lane count, from `reduction_workgroup_x()`
    /// (the contract resolves it; the plan copies it).
    pub workgroup_x: u32,
    /// Per-workgroup partial slot count `ceil(length / workgroup_x)`: one
    /// slot per workgroup; the host combines.
    pub partials: u64,
    /// Shared-memory buffer the tree reduces through.
    pub shared_memory: SharedMemoryLayout,
    /// Ordered barrier schedule: the post-load barrier first, then one
    /// barrier after every tree halving step.
    pub barriers: Vec<BarrierPoint>,
    /// Zero-fill policy for tail elements beyond `length` (L16).
    pub oob_padding: OobPaddingPolicy,
}

/// Reduction combination operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReduceOp {
    /// Plain accumulation.
    Sum,
    /// Accumulation divided by the true tensor length.
    Mean,
}

/// Shared-memory buffer layout for a recipe.
///
/// Logical only: the plan never names a target keyword (L4). `buffer_name`
/// is the logical buffer identity (e.g. `wg_shared`); backends map it to
/// their shared-memory declaration syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedMemoryLayout {
    /// Byte width of one element (f32 = 4; L7).
    pub element_byte_width: usize,
    /// Number of slots (workgroup lanes for the reduction recipe).
    pub slot_count: u32,
    /// Logical buffer name — backends map to target syntax.
    pub buffer_name: String,
}

/// A barrier program point in the recipe: the phase whose shared-memory
/// writes the barrier makes visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarrierPoint {
    /// The phase whose writes this barrier publishes.
    pub after: BarrierPhase,
}

/// What shared-memory writes a barrier publishes (L11: uniform-control-flow
/// common subset only — no divergent barriers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarrierPhase {
    /// After the cooperative load, before any shared-memory read.
    SharedMemoryLoad,
    /// After each tree halving step.
    ReductionStep,
    /// After the inner product over one K tile: every lane has finished
    /// reading `shared_a`/`shared_b`, so the next K tile's cooperative load
    /// may overwrite them.
    InnerProductStep,
}

/// Policy for out-of-bounds element reads in collection-op recipes.
///
/// The WGSL reduction body never reads OOB (the grid-stride loop is bounded
/// by the true length; lanes past the tensor length accumulate nothing). The
/// policy records the padding contract so every backend applies the same
/// zero-fill rule (L16) instead of reinventing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OobPaddingPolicy {
    /// Reads at element index ≥ `length` yield 0.
    ZeroFill,
}

impl OobPaddingPolicy {
    /// Whether reads at `index` (relative to the tensor) are covered: a
    /// zero-filled read is safe exactly when the index is at or beyond the
    /// tensor `length`.
    #[must_use]
    pub fn covers(&self, index: u64, length: u64) -> bool {
        match self {
            Self::ZeroFill => index >= length,
        }
    }
}

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
        }
    }
}

/// Barrier schedule for the tree-reduction recipe: one post-load barrier
/// (publishes each lane's partial to its own shared slot), then one barrier
/// after every tree halving step (`log2(workgroup_x)` steps for the
/// power-of-two workgroup).
fn reduction_barrier_schedule(workgroup_x: u32) -> Vec<BarrierPoint> {
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
fn matmul_barrier_schedule() -> Vec<BarrierPoint> {
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

fn validate_reduction_plan(plan: &ReductionPlan) -> Result<(), PlanValidationError> {
    validate_barrier_dominance(plan)?;
    validate_output_write_exclusivity(plan)?;
    validate_shared_memory_sizing(&plan.shared_memory)?;
    validate_oob_padding_coverage(plan)?;
    Ok(())
}

/// Barrier dominance (L12): every shared-memory read must be dominated by a
/// barrier after the corresponding write. The tree recipe reads shared
/// memory only inside the halving steps; the post-load barrier must precede
/// the first tree read, and every halving step must end with a barrier
/// before the next step's reads (and before the mean divide / output read).
/// The workgroup must be a power of two (the reference body's HIGH comment:
/// tree reduction requires it).
fn validate_barrier_dominance(plan: &ReductionPlan) -> Result<(), PlanValidationError> {
    if !plan.workgroup_x.is_power_of_two() {
        return Err(PlanValidationError::NonPowerOfTwoWorkgroup {
            workgroup_x: plan.workgroup_x,
        });
    }
    let tree_steps = plan.workgroup_x.trailing_zeros();
    let mut load_barriers = 0u32;
    let mut reduction_barriers = 0u32;
    for point in &plan.barriers {
        match point.after {
            BarrierPhase::SharedMemoryLoad => load_barriers += 1,
            BarrierPhase::ReductionStep => reduction_barriers += 1,
            // A matmul-only phase has no place in the tree recipe; the plan
            // is a closed recipe set (L2), so a cross-recipe barrier is a
            // malformed plan.
            BarrierPhase::InnerProductStep => {
                return Err(PlanValidationError::BarrierDominance {
                    phase: BarrierPhase::InnerProductStep,
                    required_count: 0,
                    present_count: 1,
                });
            }
        }
    }
    // The post-load barrier must dominate the first tree read: it is the
    // first barrier in the schedule.
    if plan.barriers.first()
        != Some(&BarrierPoint {
            after: BarrierPhase::SharedMemoryLoad,
        })
    {
        return Err(PlanValidationError::BarrierDominance {
            phase: BarrierPhase::SharedMemoryLoad,
            required_count: 1,
            present_count: load_barriers,
        });
    }
    if reduction_barriers < tree_steps {
        return Err(PlanValidationError::BarrierDominance {
            phase: BarrierPhase::ReductionStep,
            required_count: tree_steps,
            present_count: reduction_barriers,
        });
    }
    Ok(())
}

/// Output-write exclusivity (L12): each output element is written exactly
/// once across the grid. The reduction recipe writes one partial slot per
/// workgroup (`output[workgroup_id.x]`, guarded by
/// `workgroup_id.x < partials`); exclusivity holds exactly when the partial
/// count tiles the tensor (`partials == ceil(length / workgroup_x)`).
fn validate_output_write_exclusivity(plan: &ReductionPlan) -> Result<(), PlanValidationError> {
    let workgroup_x = u64::from(plan.workgroup_x);
    let expected = if workgroup_x == 0 {
        0
    } else {
        plan.length.div_ceil(workgroup_x)
    };
    if plan.partials != expected {
        return Err(PlanValidationError::OutputWriteExclusivity {
            output_slots: plan.partials,
            expected_partials: expected,
        });
    }
    Ok(())
}

/// Shared-memory sizing (L12): `slot_count * element_byte_width` within the
/// conservative device workgroup-memory limit (`32KB`). The reduction recipe
/// allocates one shared slot per workgroup lane; the matmul recipe allocates
/// two `tile × tile` operand tiles. An oversize footprint fails to
/// compile/launch on the target.
fn validate_shared_memory_sizing(layout: &SharedMemoryLayout) -> Result<(), PlanValidationError> {
    let element_width = u64::try_from(layout.element_byte_width).unwrap_or(u64::MAX);
    let bytes = u64::from(layout.slot_count).saturating_mul(element_width);
    if bytes > WORKGROUP_SHARED_MEMORY_LIMIT_BYTES {
        return Err(PlanValidationError::SharedMemorySizing {
            buffer_name: layout.buffer_name.clone(),
            bytes,
            limit: WORKGROUP_SHARED_MEMORY_LIMIT_BYTES,
        });
    }
    Ok(())
}

/// OOB-padding coverage (L12): the padding policy covers the tail of the
/// last workgroup — global element indices `[length, workgroup_x * partials)`
/// (at most `workgroup_x - 1` elements). `ZeroFill` covers every index at or
/// beyond `length`, so the check pins the arithmetic that defines the padded
/// range: the partial count must tile the tensor, or the dispatched grid
/// would leave real elements inside the "padding" range.
fn validate_oob_padding_coverage(plan: &ReductionPlan) -> Result<(), PlanValidationError> {
    let workgroup_x = u64::from(plan.workgroup_x);
    if workgroup_x == 0 || plan.length == 0 {
        return Err(PlanValidationError::OobPaddingCoverage {
            length: plan.length,
            workgroup_x: plan.workgroup_x,
            covered_until: 0,
        });
    }
    if plan.partials != plan.length.div_ceil(workgroup_x) {
        return Err(PlanValidationError::OobPaddingCoverage {
            length: plan.length,
            workgroup_x: plan.workgroup_x,
            covered_until: plan.partials.saturating_mul(workgroup_x),
        });
    }
    // `covered_until - length < workgroup_x` here: the tail is at most one
    // workgroup's lane span, so the loop is bounded by the workgroup size.
    let covered_until = plan.partials.saturating_mul(workgroup_x);
    for index in plan.length..covered_until {
        if !plan.oob_padding.covers(index, plan.length) {
            return Err(PlanValidationError::OobPaddingCoverage {
                length: plan.length,
                workgroup_x: plan.workgroup_x,
                covered_until,
            });
        }
    }
    Ok(())
}

fn validate_matmul_plan(plan: &MatMulPlan) -> Result<(), PlanValidationError> {
    validate_matmul_workgroup(plan)?;
    validate_matmul_shared_memory(plan)?;
    validate_matmul_barrier_coverage(plan)?;
    validate_matmul_oob_padding_coverage(plan)?;
    Ok(())
}

/// Workgroup coupling (L12 / PRP-004 U2): tile-local addressing derives from
/// workgroup-local ids, so the shared-memory tile is only correct when the
/// workgroup equals the tile grid on both axes — the pre-fix `id % 8` aliased
/// shared slots for any other workgroup size. A zero tile is degenerate (no
/// dispatch arithmetic could tile it) and is rejected here, before any
/// `div_ceil` runs.
fn validate_matmul_workgroup(plan: &MatMulPlan) -> Result<(), PlanValidationError> {
    if plan.tile == 0 {
        return Err(PlanValidationError::MatMulZeroDimension {
            m: plan.m,
            k: plan.k,
            n: plan.n,
            tile: plan.tile,
        });
    }
    if plan.workgroup_x != plan.tile || plan.workgroup_y != plan.tile {
        return Err(PlanValidationError::MatMulWorkgroupMismatch {
            workgroup_x: plan.workgroup_x,
            workgroup_y: plan.workgroup_y,
            tile: plan.tile,
        });
    }
    Ok(())
}

/// Shared-memory sizing (L12): both operand tiles must be exactly
/// `tile × tile` slots (the cooperative load fills the whole tile) and each
/// footprint must fit the conservative device workgroup-memory limit.
fn validate_matmul_shared_memory(plan: &MatMulPlan) -> Result<(), PlanValidationError> {
    let expected_slots = u64::from(plan.tile) * u64::from(plan.tile);
    for layout in [&plan.shared_memory.shared_a, &plan.shared_memory.shared_b] {
        if u64::from(layout.slot_count) != expected_slots {
            return Err(PlanValidationError::MatMulSharedMemoryLayout {
                buffer_name: layout.buffer_name.clone(),
                slot_count: u64::from(layout.slot_count),
                expected_slots,
            });
        }
        validate_shared_memory_sizing(layout)?;
    }
    Ok(())
}

/// Barrier coverage (L12): every shared-memory read must be dominated by a
/// barrier after the corresponding write. The matmul recipe reads the tiles
/// inside the inner-product step; the post-cooperative-load barrier must
/// head the per-K-tile schedule, and a post-inner-product-step barrier must
/// follow (it publishes the read completion before the next tile load
/// overwrites the buffers).
fn validate_matmul_barrier_coverage(plan: &MatMulPlan) -> Result<(), PlanValidationError> {
    let load = BarrierPoint {
        after: BarrierPhase::SharedMemoryLoad,
    };
    let step = BarrierPoint {
        after: BarrierPhase::InnerProductStep,
    };
    for point in &plan.barriers {
        if matches!(point.after, BarrierPhase::ReductionStep) {
            // A reduction-only phase has no place in the matmul recipe; the
            // plan is a closed recipe set (L2), so a cross-recipe barrier is
            // a malformed plan.
            return Err(PlanValidationError::BarrierDominance {
                phase: BarrierPhase::ReductionStep,
                required_count: 0,
                present_count: 1,
            });
        }
    }
    let load_count = plan.barriers.iter().filter(|point| **point == load).count();
    let step_count = plan.barriers.iter().filter(|point| **point == step).count();
    if plan.barriers.first() != Some(&load) {
        return Err(PlanValidationError::BarrierDominance {
            phase: BarrierPhase::SharedMemoryLoad,
            required_count: 1,
            present_count: u32::try_from(load_count).unwrap_or(u32::MAX),
        });
    }
    if step_count == 0 {
        return Err(PlanValidationError::BarrierDominance {
            phase: BarrierPhase::InnerProductStep,
            required_count: 1,
            present_count: u32::try_from(step_count).unwrap_or(u32::MAX),
        });
    }
    Ok(())
}

/// OOB-padding coverage (L12 / L16): the zero-fill policy must cover the
/// operand tile tails — the padded row/column/K spans the grid reaches past
/// `M`, `N`, and `K` (`shared_a` reads `A[row][k_start + tx]`, `shared_b`
/// reads `B[k_start + ty][col]`). The check pins the padded-extent
/// arithmetic (`ceil(d / tile) * tile`) against the policy; a policy that
/// leaves any index of the last tile uncovered fails here. Zero dimensions
/// are rejected before the arithmetic (a zero dim dispatches an empty grid,
/// and `div_ceil` by a zero tile must not run).
///
/// The shared-memory sizing check runs before this one and rejects tiles
/// larger than ~90 slots, so the four tail loops are each bounded by the
/// tile edge.
fn validate_matmul_oob_padding_coverage(plan: &MatMulPlan) -> Result<(), PlanValidationError> {
    if plan.tile == 0 || plan.m == 0 || plan.k == 0 || plan.n == 0 {
        return Err(PlanValidationError::MatMulZeroDimension {
            m: plan.m,
            k: plan.k,
            n: plan.n,
            tile: plan.tile,
        });
    }
    let tile = u64::from(plan.tile);
    // shared_a holds A[row][k_start + tx]: the tail rows past M and the tail
    // K columns past K are both out of bounds of the M×K left matrix.
    let padded_m = plan.m.div_ceil(tile) * tile;
    for row in plan.m..padded_m {
        if !plan.oob_padding.covers(row, plan.m) {
            return Err(PlanValidationError::MatMulOobPaddingCoverage {
                buffer_name: MATMUL_SHARED_A_BUFFER_NAME.to_owned(),
                length: plan.m,
                covered_until: padded_m,
            });
        }
    }
    let padded_k = plan.k.div_ceil(tile) * tile;
    for k_index in plan.k..padded_k {
        if !plan.oob_padding.covers(k_index, plan.k) {
            return Err(PlanValidationError::MatMulOobPaddingCoverage {
                buffer_name: MATMUL_SHARED_A_BUFFER_NAME.to_owned(),
                length: plan.k,
                covered_until: padded_k,
            });
        }
    }
    // shared_b holds B[k_start + ty][col]: the tail K rows past K and the
    // tail columns past N are both out of bounds of the K×N right matrix.
    for k_index in plan.k..padded_k {
        if !plan.oob_padding.covers(k_index, plan.k) {
            return Err(PlanValidationError::MatMulOobPaddingCoverage {
                buffer_name: MATMUL_SHARED_B_BUFFER_NAME.to_owned(),
                length: plan.k,
                covered_until: padded_k,
            });
        }
    }
    let padded_n = plan.n.div_ceil(tile) * tile;
    for col in plan.n..padded_n {
        if !plan.oob_padding.covers(col, plan.n) {
            return Err(PlanValidationError::MatMulOobPaddingCoverage {
                buffer_name: MATMUL_SHARED_B_BUFFER_NAME.to_owned(),
                length: plan.n,
                covered_until: padded_n,
            });
        }
    }
    Ok(())
}

/// Plan validation failure (L12 static plan checks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanValidationError {
    /// The barrier schedule does not dominate every shared-memory read.
    BarrierDominance {
        phase: BarrierPhase,
        required_count: u32,
        present_count: u32,
    },
    /// Tree reduction requires a power-of-two workgroup size.
    NonPowerOfTwoWorkgroup { workgroup_x: u32 },
    /// The output buffer is not written exactly once per grid position.
    OutputWriteExclusivity {
        output_slots: u64,
        expected_partials: u64,
    },
    /// The shared-memory footprint exceeds the device workgroup-memory limit.
    SharedMemorySizing {
        buffer_name: String,
        bytes: u64,
        limit: u64,
    },
    /// The OOB padding policy does not cover the last workgroup's tail.
    OobPaddingCoverage {
        length: u64,
        workgroup_x: u32,
        covered_until: u64,
    },
    /// Matmul requires the workgroup to equal the tile on both axes
    /// (PRP-004 U2).
    MatMulWorkgroupMismatch {
        workgroup_x: u32,
        workgroup_y: u32,
        tile: u32,
    },
    /// The shared-memory tile slot count is not `tile × tile`.
    MatMulSharedMemoryLayout {
        buffer_name: String,
        slot_count: u64,
        expected_slots: u64,
    },
    /// Matmul requires positive M/K/N and a non-zero tile (a zero dimension
    /// dispatches an empty grid and the padded-tile arithmetic would divide
    /// by a zero tile).
    MatMulZeroDimension { m: u64, k: u64, n: u64, tile: u32 },
    /// The OOB padding policy does not cover a matmul operand tile tail.
    MatMulOobPaddingCoverage {
        buffer_name: String,
        length: u64,
        covered_until: u64,
    },
}

impl fmt::Display for PlanValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BarrierDominance {
                phase,
                required_count,
                present_count,
            } => write!(
                f,
                "barrier dominance: {phase:?} barrier required {required_count} time(s), plan has {present_count}"
            ),
            Self::NonPowerOfTwoWorkgroup { workgroup_x } => write!(
                f,
                "tree reduction requires a power-of-two workgroup size, got {workgroup_x}"
            ),
            Self::OutputWriteExclusivity {
                output_slots,
                expected_partials,
            } => write!(
                f,
                "output-write exclusivity: {output_slots} partial slot(s) does not tile the tensor (expected {expected_partials})"
            ),
            Self::SharedMemorySizing {
                buffer_name,
                bytes,
                limit,
            } => write!(
                f,
                "shared-memory sizing: {buffer_name} is {bytes} bytes, exceeding the {limit}-byte workgroup-memory limit"
            ),
            Self::OobPaddingCoverage {
                length,
                workgroup_x,
                covered_until,
            } => write!(
                f,
                "OOB padding coverage: tensor length {length} with workgroup_x {workgroup_x} is not covered up to {covered_until}"
            ),
            Self::MatMulWorkgroupMismatch {
                workgroup_x,
                workgroup_y,
                tile,
            } => write!(
                f,
                "matmul workgroup ({workgroup_x}, {workgroup_y}) must equal the {tile}×{tile} tile grid (PRP-004 U2)"
            ),
            Self::MatMulSharedMemoryLayout {
                buffer_name,
                slot_count,
                expected_slots,
            } => write!(
                f,
                "matmul shared memory {buffer_name} has {slot_count} slots, expected {expected_slots} (tile × tile)"
            ),
            Self::MatMulZeroDimension { m, k, n, tile } => write!(
                f,
                "matmul requires positive M/K/N and a non-zero tile, got M={m}, K={k}, N={n}, tile={tile}"
            ),
            Self::MatMulOobPaddingCoverage {
                buffer_name,
                length,
                covered_until,
            } => write!(
                f,
                "matmul OOB padding coverage: {buffer_name} tail up to {covered_until} is not zero-filled against length {length}"
            ),
        }
    }
}

impl std::error::Error for PlanValidationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{
        MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind,
        MirKernelResourceRole, MirWorkgroupSize,
    };
    use crate::{MirFunctionId, MirLocalId, MirType};
    use radix_types::{NumericWidth, Primitive, TypeTable};

    fn f32_ty(types: &mut TypeTable) -> MirType {
        MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
    }

    /// A post-contract reduction signature: workgroup `(ws, 1, 1)`, dispatch
    /// `(length, 1, 1)`, output sized to `partials` — the shape
    /// `apply_collection_op_contract` produces.
    fn reduction_signature(length: u64, workgroup_x: u32, partials: u64) -> MirKernelSignature {
        let mut types = TypeTable::new();
        let element_ty = f32_ty(&mut types);
        MirKernelSignature {
            function: MirFunctionId(0),
            input: MirKernelResource {
                group: 0,
                binding: 0,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty,
                element_count: length,
                source_local: Some(MirLocalId(0)),
                source_name: None,
            },
            extra_inputs: Vec::new(),
            output: MirKernelResource {
                group: 0,
                binding: 1,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty,
                element_count: partials,
                source_local: None,
                source_name: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: Vec::new(),
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
        }
    }

    /// A post-contract matmul signature: workgroup `(8, 8, 1)`, tile-grid
    /// dispatch, left/right operand buffers sized to the flat `M·K` / `K·N`
    /// element counts and an `M·N` output — the shape the kernel ABI
    /// produces for a two-tensor-param matmul kernel.
    fn matmul_signature(
        shapes: MatMulShapes,
        dispatch_x: u64,
        dispatch_y: u64,
    ) -> MirKernelSignature {
        let mut types = TypeTable::new();
        let element_ty = f32_ty(&mut types);
        MirKernelSignature {
            function: MirFunctionId(0),
            input: MirKernelResource {
                group: 0,
                binding: 0,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty,
                element_count: shapes.m * shapes.k,
                source_local: Some(MirLocalId(0)),
                source_name: None,
            },
            extra_inputs: vec![MirKernelResource {
                group: 0,
                binding: 1,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty,
                element_count: shapes.k * shapes.n,
                source_local: Some(MirLocalId(1)),
                source_name: None,
            }],
            output: MirKernelResource {
                group: 0,
                binding: 2,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty,
                element_count: shapes.m * shapes.n,
                source_local: None,
                source_name: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: Vec::new(),
            workgroup: MirWorkgroupSize {
                x: MATMUL_TILE,
                y: MATMUL_TILE,
                z: 1,
            },
            dispatch_size: MirKernelDispatchSize {
                x: dispatch_x,
                y: dispatch_y,
                z: 1,
            },
        }
    }

    /// A tail-tile shapes fixture: `17×12` times `12×33` → dispatch `(5, 3)`
    /// — the padded grid reaches rows/cols/K the matrices do not have, so the
    /// zero-fill tails are non-empty.
    fn matmul_fixture_shapes() -> MatMulShapes {
        MatMulShapes {
            m: 17,
            k: 12,
            n: 33,
        }
    }

    /// Build the canonical tiled-matmul plan for the deliberately-broken-plan
    /// corpus: tail-tile fixture with the correct workgroup, operand tiles,
    /// and per-K-tile barrier pair.
    fn matmul_plan() -> MatMulPlan {
        let shapes = matmul_fixture_shapes();
        let tile = MATMUL_TILE;
        let tile_slots = tile * tile;
        MatMulPlan {
            m: shapes.m,
            k: shapes.k,
            n: shapes.n,
            tile,
            workgroup_x: tile,
            workgroup_y: tile,
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
        }
    }

    /// Build a reduction plan with an explicit barrier schedule (for the
    /// deliberately-broken-plan corpus, L12).
    fn reduction_plan_with_barriers(
        workgroup_x: u32,
        partials: u64,
        barriers: Vec<BarrierPoint>,
    ) -> ReductionPlan {
        ReductionPlan {
            op: ReduceOp::Sum,
            length: 1024,
            workgroup_x,
            partials,
            shared_memory: SharedMemoryLayout {
                element_byte_width: F32_ELEMENT_BYTE_WIDTH,
                slot_count: workgroup_x,
                buffer_name: REDUCTION_SHARED_BUFFER_NAME.to_owned(),
            },
            barriers,
            oob_padding: OobPaddingPolicy::ZeroFill,
        }
    }

    fn correct_barriers(workgroup_x: u32) -> Vec<BarrierPoint> {
        reduction_barrier_schedule(workgroup_x)
    }

    #[test]
    fn sum_reduction_plan_is_fully_populated() {
        let signature = reduction_signature(1024, 256, 4);
        let contract = CollectionOpContract::Reduction {
            length: 1024,
            workgroup_x: 256,
            partials: 4,
        };
        let plan =
            CollectionKernelPlan::from_contract(&signature, &contract, MirCollectionOp::Sum, None)
                .expect("reduction contract builds a plan");
        let CollectionKernelPlan::TreeReduction(reduction) = &plan else {
            panic!("expected a tree-reduction plan");
        };
        assert_eq!(reduction.op, ReduceOp::Sum);
        assert_eq!(reduction.length, 1024);
        assert_eq!(reduction.workgroup_x, 256);
        assert_eq!(reduction.partials, 4);
        assert_eq!(reduction.shared_memory.element_byte_width, 4);
        assert_eq!(reduction.shared_memory.slot_count, 256);
        assert_eq!(reduction.shared_memory.buffer_name, "wg_shared");
        assert_eq!(reduction.oob_padding, OobPaddingPolicy::ZeroFill);
        // One post-load barrier + log2(256) = 8 tree-step barriers.
        assert_eq!(reduction.barriers.len(), 9);
        assert_eq!(
            reduction.barriers.first(),
            Some(&BarrierPoint {
                after: BarrierPhase::SharedMemoryLoad
            })
        );
        assert!(reduction.barriers[1..]
            .iter()
            .all(|point| point.after == BarrierPhase::ReductionStep));
        plan.validate().expect("built plan validates");
    }

    #[test]
    fn mean_reduction_plan_sets_mean_op() {
        let signature = reduction_signature(1024, 256, 4);
        let contract = CollectionOpContract::Reduction {
            length: 1024,
            workgroup_x: 256,
            partials: 4,
        };
        let plan = CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::TensorMean,
            None,
        )
        .expect("mean contract builds a plan");
        let CollectionKernelPlan::TreeReduction(reduction) = &plan else {
            panic!("expected a tree-reduction plan");
        };
        assert_eq!(reduction.op, ReduceOp::Mean);
    }

    #[test]
    fn single_lane_reduction_has_load_barrier_only() {
        let signature = reduction_signature(1, 1, 1);
        let contract = CollectionOpContract::Reduction {
            length: 1,
            workgroup_x: 1,
            partials: 1,
        };
        let plan =
            CollectionKernelPlan::from_contract(&signature, &contract, MirCollectionOp::Sum, None)
                .expect("single-lane contract builds a plan");
        let CollectionKernelPlan::TreeReduction(reduction) = &plan else {
            panic!("expected a tree-reduction plan");
        };
        assert_eq!(reduction.barriers.len(), 1);
        assert_eq!(
            reduction.barriers.first(),
            Some(&BarrierPoint {
                after: BarrierPhase::SharedMemoryLoad
            })
        );
        plan.validate().expect("single-lane plan validates");
    }

    #[test]
    fn elementwise_op_returns_none() {
        let signature = reduction_signature(1024, 256, 4);
        let contract = CollectionOpContract::Reduction {
            length: 1024,
            workgroup_x: 256,
            partials: 4,
        };
        assert!(
            CollectionKernelPlan::from_contract(
                &signature,
                &contract,
                MirCollectionOp::Append,
                None
            )
            .is_none(),
            "a non-reduction op must not produce a reduction plan"
        );
    }

    #[test]
    fn matmul_plan_is_fully_populated() {
        let shapes = matmul_fixture_shapes();
        let signature = matmul_signature(shapes, 5, 3);
        let contract = CollectionOpContract::MatMul {
            dispatch_x: 5,
            dispatch_y: 3,
        };
        let plan = CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::TensorMatMul,
            Some(shapes),
        )
        .expect("matmul contract builds a plan");
        let CollectionKernelPlan::TiledMatMul(matmul) = &plan else {
            panic!("expected a tiled-matmul plan");
        };
        // Shapes come from the operand tensor types; the tile from the ABI
        // constant.
        assert_eq!((matmul.m, matmul.k, matmul.n), (17, 12, 33));
        assert_eq!(matmul.tile, MATMUL_TILE);
        assert_eq!(
            (matmul.workgroup_x, matmul.workgroup_y),
            (MATMUL_TILE, MATMUL_TILE)
        );
        // Two shared-memory operand tiles, each tile × tile f32 slots.
        assert_eq!(matmul.shared_memory.shared_a.element_byte_width, 4);
        assert_eq!(matmul.shared_memory.shared_a.slot_count, 64);
        assert_eq!(matmul.shared_memory.shared_a.buffer_name, "shared_a");
        assert_eq!(matmul.shared_memory.shared_b.element_byte_width, 4);
        assert_eq!(matmul.shared_memory.shared_b.slot_count, 64);
        assert_eq!(matmul.shared_memory.shared_b.buffer_name, "shared_b");
        // Per-K-tile barrier pair: post-cooperative-load, then
        // post-inner-product-step.
        assert_eq!(
            matmul.barriers,
            vec![
                BarrierPoint {
                    after: BarrierPhase::SharedMemoryLoad
                },
                BarrierPoint {
                    after: BarrierPhase::InnerProductStep
                }
            ]
        );
        assert_eq!(matmul.oob_padding, OobPaddingPolicy::ZeroFill);
        plan.validate().expect("built matmul plan validates");
    }

    #[test]
    fn matmul_contract_without_shapes_returns_none() {
        let shapes = MatMulShapes { m: 16, k: 8, n: 32 };
        let signature = matmul_signature(shapes, 4, 2);
        let contract = CollectionOpContract::MatMul {
            dispatch_x: 4,
            dispatch_y: 2,
        };
        assert!(
            CollectionKernelPlan::from_contract(
                &signature,
                &contract,
                MirCollectionOp::TensorMatMul,
                None,
            )
            .is_none(),
            "a matmul contract without operand shapes must not build a plan"
        );
    }

    #[test]
    fn matmul_plan_shapes_not_matching_dispatch_returns_none() {
        // Shapes say `33×8×33` → dispatch (5, 5), but the contract dispatches
        // (4, 2): the caller resolved shapes that disagree with the contract.
        let signature = matmul_signature(MatMulShapes { m: 16, k: 8, n: 32 }, 4, 2);
        let contract = CollectionOpContract::MatMul {
            dispatch_x: 4,
            dispatch_y: 2,
        };
        assert!(
            CollectionKernelPlan::from_contract(
                &signature,
                &contract,
                MirCollectionOp::TensorMatMul,
                Some(MatMulShapes { m: 33, k: 8, n: 33 }),
            )
            .is_none(),
            "shapes that disagree with the contract dispatch must not build a plan"
        );
    }

    #[test]
    fn matmul_contract_with_non_matmul_op_returns_none() {
        let signature = matmul_signature(MatMulShapes { m: 16, k: 8, n: 32 }, 4, 2);
        let contract = CollectionOpContract::MatMul {
            dispatch_x: 4,
            dispatch_y: 2,
        };
        assert!(
            CollectionKernelPlan::from_contract(
                &signature,
                &contract,
                MirCollectionOp::Sum,
                Some(MatMulShapes { m: 16, k: 8, n: 32 }),
            )
            .is_none(),
            "a reduction op must not produce a matmul plan"
        );
    }

    #[test]
    fn matmul_signature_not_carrying_contract_returns_none() {
        // The output element count disagrees with the shapes: the signature
        // was not built from this contract.
        let mut signature = matmul_signature(MatMulShapes { m: 16, k: 8, n: 32 }, 4, 2);
        signature.output.element_count = 1;
        let contract = CollectionOpContract::MatMul {
            dispatch_x: 4,
            dispatch_y: 2,
        };
        assert!(
            CollectionKernelPlan::from_contract(
                &signature,
                &contract,
                MirCollectionOp::TensorMatMul,
                Some(MatMulShapes { m: 16, k: 8, n: 32 }),
            )
            .is_none(),
            "a signature that does not carry the contract must not build a plan"
        );
    }

    #[test]
    fn signature_not_carrying_contract_returns_none() {
        // Output count disagrees with the contract's partial count: the
        // signature was not built from this contract.
        let signature = reduction_signature(1024, 256, 5);
        let contract = CollectionOpContract::Reduction {
            length: 1024,
            workgroup_x: 256,
            partials: 4,
        };
        assert!(
            CollectionKernelPlan::from_contract(&signature, &contract, MirCollectionOp::Sum, None)
                .is_none(),
            "a signature that does not carry the contract must not build a plan"
        );
    }

    #[test]
    fn elementwise_plan_validates() {
        CollectionKernelPlan::Elementwise
            .validate()
            .expect("elementwise needs no recipe checks");
    }

    #[test]
    fn validate_rejects_missing_load_barrier() {
        let barriers = (0..8)
            .map(|_| BarrierPoint {
                after: BarrierPhase::ReductionStep,
            })
            .collect();
        let plan = reduction_plan_with_barriers(256, 4, barriers);
        assert_eq!(
            plan_validation_error(&plan),
            PlanValidationError::BarrierDominance {
                phase: BarrierPhase::SharedMemoryLoad,
                required_count: 1,
                present_count: 0,
            }
        );
    }

    #[test]
    fn validate_rejects_short_barrier_schedule() {
        // ws = 256 needs log2(256) = 8 tree-step barriers; only the load
        // barrier is present.
        let plan = reduction_plan_with_barriers(
            256,
            4,
            vec![BarrierPoint {
                after: BarrierPhase::SharedMemoryLoad,
            }],
        );
        assert_eq!(
            plan_validation_error(&plan),
            PlanValidationError::BarrierDominance {
                phase: BarrierPhase::ReductionStep,
                required_count: 8,
                present_count: 0,
            }
        );
    }

    #[test]
    fn validate_rejects_non_power_of_two_workgroup() {
        let plan = reduction_plan_with_barriers(100, 11, correct_barriers(100));
        assert_eq!(
            plan_validation_error(&plan),
            PlanValidationError::NonPowerOfTwoWorkgroup { workgroup_x: 100 }
        );
    }

    #[test]
    fn validate_rejects_matmul_barrier_in_reduction() {
        // A matmul-only barrier phase in a tree-reduction schedule is a
        // cross-recipe plan (L2 closed recipe set).
        let plan = reduction_plan_with_barriers(
            256,
            4,
            vec![
                BarrierPoint {
                    after: BarrierPhase::SharedMemoryLoad,
                },
                BarrierPoint {
                    after: BarrierPhase::InnerProductStep,
                },
            ],
        );
        assert_eq!(
            plan_validation_error(&plan),
            PlanValidationError::BarrierDominance {
                phase: BarrierPhase::InnerProductStep,
                required_count: 0,
                present_count: 1,
            }
        );
    }

    #[test]
    fn validate_rejects_oversized_shared_memory() {
        // 16384 slots × 4 bytes = 64KB > the 32KB conservative limit.
        let plan = ReductionPlan {
            op: ReduceOp::Sum,
            length: 1 << 20,
            workgroup_x: 16384,
            partials: (1 << 20) / 16384,
            shared_memory: SharedMemoryLayout {
                element_byte_width: F32_ELEMENT_BYTE_WIDTH,
                slot_count: 16384,
                buffer_name: REDUCTION_SHARED_BUFFER_NAME.to_owned(),
            },
            barriers: correct_barriers(16384),
            oob_padding: OobPaddingPolicy::ZeroFill,
        };
        assert_eq!(
            plan_validation_error(&plan),
            PlanValidationError::SharedMemorySizing {
                buffer_name: REDUCTION_SHARED_BUFFER_NAME.to_owned(),
                bytes: 65_536,
                limit: WORKGROUP_SHARED_MEMORY_LIMIT_BYTES,
            }
        );
    }

    #[test]
    fn validate_rejects_partial_mismatch() {
        // partials = 3 instead of ceil(1024 / 256) = 4: one workgroup would
        // not own an output slot, so some output element is never written.
        let plan = reduction_plan_with_barriers(256, 3, correct_barriers(256));
        assert_eq!(
            plan_validation_error(&plan),
            PlanValidationError::OutputWriteExclusivity {
                output_slots: 3,
                expected_partials: 4,
            }
        );
    }

    #[test]
    fn validate_rejects_zero_length() {
        let plan = ReductionPlan {
            op: ReduceOp::Sum,
            length: 0,
            workgroup_x: 256,
            partials: 0,
            shared_memory: SharedMemoryLayout {
                element_byte_width: F32_ELEMENT_BYTE_WIDTH,
                slot_count: 256,
                buffer_name: REDUCTION_SHARED_BUFFER_NAME.to_owned(),
            },
            barriers: correct_barriers(256),
            oob_padding: OobPaddingPolicy::ZeroFill,
        };
        assert!(matches!(
            plan_validation_error(&plan),
            PlanValidationError::OobPaddingCoverage { .. }
        ));
    }

    #[test]
    fn zero_fill_covers_tail_not_real_elements() {
        let policy = OobPaddingPolicy::ZeroFill;
        assert!(policy.covers(1024, 1024));
        assert!(policy.covers(4096, 1024));
        assert!(!policy.covers(1023, 1024));
        assert!(!policy.covers(0, 1024));
    }

    // ── Matmul deliberately-broken-plan corpus (L12) ──────────────────────

    #[test]
    fn validate_rejects_matmul_workgroup_not_matching_tile() {
        let mut plan = matmul_plan();
        plan.workgroup_x = MATMUL_TILE - 1;
        assert_eq!(
            matmul_plan_validation_error(&plan),
            PlanValidationError::MatMulWorkgroupMismatch {
                workgroup_x: MATMUL_TILE - 1,
                workgroup_y: MATMUL_TILE,
                tile: MATMUL_TILE,
            }
        );
    }

    #[test]
    fn validate_rejects_matmul_shared_memory_slot_mismatch() {
        let mut plan = matmul_plan();
        plan.shared_memory.shared_b.slot_count = 32;
        assert_eq!(
            matmul_plan_validation_error(&plan),
            PlanValidationError::MatMulSharedMemoryLayout {
                buffer_name: MATMUL_SHARED_B_BUFFER_NAME.to_owned(),
                slot_count: 32,
                expected_slots: 64,
            }
        );
    }

    #[test]
    fn validate_rejects_matmul_oversized_shared_memory() {
        // tile 128 → 128×128 slots × 4 bytes = 64KB > the 32KB conservative
        // limit (a consistent tile whose footprint cannot launch).
        let tile = 128;
        let tile_slots = tile * tile;
        let mut plan = matmul_plan();
        plan.tile = tile;
        plan.workgroup_x = tile;
        plan.workgroup_y = tile;
        plan.shared_memory.shared_a.slot_count = tile_slots;
        plan.shared_memory.shared_b.slot_count = tile_slots;
        assert_eq!(
            matmul_plan_validation_error(&plan),
            PlanValidationError::SharedMemorySizing {
                buffer_name: MATMUL_SHARED_A_BUFFER_NAME.to_owned(),
                bytes: 65_536,
                limit: WORKGROUP_SHARED_MEMORY_LIMIT_BYTES,
            }
        );
    }

    #[test]
    fn validate_rejects_matmul_missing_load_barrier() {
        let mut plan = matmul_plan();
        plan.barriers = vec![BarrierPoint {
            after: BarrierPhase::InnerProductStep,
        }];
        assert_eq!(
            matmul_plan_validation_error(&plan),
            PlanValidationError::BarrierDominance {
                phase: BarrierPhase::SharedMemoryLoad,
                required_count: 1,
                present_count: 0,
            }
        );
    }

    #[test]
    fn validate_rejects_matmul_missing_inner_product_barrier() {
        let mut plan = matmul_plan();
        plan.barriers = vec![BarrierPoint {
            after: BarrierPhase::SharedMemoryLoad,
        }];
        assert_eq!(
            matmul_plan_validation_error(&plan),
            PlanValidationError::BarrierDominance {
                phase: BarrierPhase::InnerProductStep,
                required_count: 1,
                present_count: 0,
            }
        );
    }

    #[test]
    fn validate_rejects_matmul_barriers_out_of_order() {
        // Inner product before the load barrier: the inner product would
        // read tiles the load barrier has not yet published.
        let mut plan = matmul_plan();
        plan.barriers = vec![
            BarrierPoint {
                after: BarrierPhase::InnerProductStep,
            },
            BarrierPoint {
                after: BarrierPhase::SharedMemoryLoad,
            },
        ];
        assert_eq!(
            matmul_plan_validation_error(&plan),
            PlanValidationError::BarrierDominance {
                phase: BarrierPhase::SharedMemoryLoad,
                required_count: 1,
                present_count: 1,
            }
        );
    }

    #[test]
    fn validate_rejects_reduction_barrier_in_matmul() {
        // A reduction-only barrier phase in a matmul schedule is a
        // cross-recipe plan (L2 closed recipe set).
        let mut plan = matmul_plan();
        plan.barriers.push(BarrierPoint {
            after: BarrierPhase::ReductionStep,
        });
        assert_eq!(
            matmul_plan_validation_error(&plan),
            PlanValidationError::BarrierDominance {
                phase: BarrierPhase::ReductionStep,
                required_count: 0,
                present_count: 1,
            }
        );
    }

    #[test]
    fn validate_rejects_matmul_zero_dimension() {
        let mut plan = matmul_plan();
        plan.m = 0;
        assert_eq!(
            matmul_plan_validation_error(&plan),
            PlanValidationError::MatMulZeroDimension {
                m: 0,
                k: 12,
                n: 33,
                tile: MATMUL_TILE,
            }
        );
    }

    #[test]
    fn validate_rejects_matmul_zero_tile() {
        let mut plan = matmul_plan();
        plan.tile = 0;
        plan.workgroup_x = 0;
        plan.workgroup_y = 0;
        plan.shared_memory.shared_a.slot_count = 0;
        plan.shared_memory.shared_b.slot_count = 0;
        assert_eq!(
            matmul_plan_validation_error(&plan),
            PlanValidationError::MatMulZeroDimension {
                m: 17,
                k: 12,
                n: 33,
                tile: 0,
            }
        );
    }

    /// The first validation error a broken matmul plan raises (the corpus
    /// asserts on the exact error, L12).
    fn matmul_plan_validation_error(plan: &MatMulPlan) -> PlanValidationError {
        CollectionKernelPlan::TiledMatMul(plan.clone())
            .validate()
            .expect_err("broken matmul plan must fail validation")
    }

    /// The first validation error a broken plan raises (the corpus asserts
    /// on the exact error, L12: the checker gets deliberately-broken plans).
    fn plan_validation_error(plan: &ReductionPlan) -> PlanValidationError {
        CollectionKernelPlan::TreeReduction(plan.clone())
            .validate()
            .expect_err("broken plan must fail validation")
    }

    #[test]
    fn multi_output_contract_rejects_colliding_output_slots() {
        // S3-A1: the result-tuple contract is incoherent when two outputs
        // share a binding — two gradient outputs must never alias a slot.
        let mut types = TypeTable::new();
        let element_ty = f32_ty(&mut types);
        let mut signature = MirKernelSignature {
            function: MirFunctionId(0),
            input: MirKernelResource {
                group: 0,
                binding: 0,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty,
                element_count: 4,
                source_local: Some(MirLocalId(0)),
                source_name: None,
            },
            extra_inputs: Vec::new(),
            output: MirKernelResource {
                group: 0,
                binding: 1,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty,
                element_count: 4,
                source_local: None,
                source_name: None,
            },
            extra_outputs: vec![MirKernelResource {
                group: 0,
                binding: 1, // collides with the first output's slot
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty,
                element_count: 4,
                source_local: None,
                source_name: None,
            }],
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: Vec::new(),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 4, y: 1, z: 1 },
        };
        assert!(!multi_output_contract_valid(&signature));

        // A distinct second slot makes the contract coherent again.
        signature.extra_outputs[0].binding = 2;
        assert!(multi_output_contract_valid(&signature));
    }
}
