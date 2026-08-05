#[allow(unused_imports)] // intra-doc links only; the plan pass lives in build.rs
use crate::abi::CollectionOpContract;

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

/// Elementwise-style workgroup lane count for the rank-2 transpose recipe
/// (S5-U1): one thread per output element, no shared memory, no barriers —
/// the recipe is a copy with a column-major index remap, so any positive
/// lane count is correct and the conservative 64-thread default keeps the
/// dispatch portable. The [`CollectionOpContract::Transpose`] contract
/// applies this workgroup; [`CollectionKernelPlan::from_contract`] requires
/// the signature to carry it (the A3 typed cross-check).
pub const TRANSPOSE_WORKGROUP_X: u32 = 64;

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
    /// Rank-2 transpose recipe (`[M,N] → [N,M]`): one thread per output
    /// element with a column-major index remap (S5-U1). See [`TransposePlan`].
    Transpose(TransposePlan),
    /// Axis-reduction recipe (S6-N1: name frozen; admission/resolution is
    /// S6-P1): reduce a tensor over a single axis by `op`. See
    /// [`AxisReductionPlan`].
    AxisReduction(AxisReductionPlan),
    /// Row-wise softmax recipe (S6-N1: name frozen; admission/resolution is
    /// S6-P1): numerically stable softmax over the last (row) axis. See
    /// [`RowSoftmaxPlan`].
    RowSoftmax(RowSoftmaxPlan),
    /// Layer-normalization recipe (S6-N1: name frozen; admission/resolution
    /// is S6-P1): row mean/var with optional affine. See
    /// [`LayerNormalizationPlan`].
    LayerNormalization(LayerNormalizationPlan),
}

/// Axis-reduction recipe plan (name frozen by S6-N1; admission, resolution,
/// and decomposition implemented by S6-P1). The axis comes from the trailing
/// const operand of the lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxisReductionPlan {
    /// Reduction operator (`Sum` in v1).
    pub op: ReduceOp,
    /// Axis reduced over (from the trailing const operand).
    pub axis: u64,
}

/// Row-wise softmax recipe plan (name frozen by S6-N1; admission, resolution,
/// and decomposition implemented by S6-P1). Operates over the last (row)
/// axis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSoftmaxPlan {
    /// Row axis (the last axis in v1).
    pub axis: u64,
}

/// Layer-normalization recipe plan (name frozen by S6-N1; admission,
/// resolution, and decomposition implemented by S6-P1). The full field set
/// (epsilon, affine) is resolved by P1 from the const operands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerNormalizationPlan {
    /// Normalization axis (from the trailing const operand).
    pub axis: u64,
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

/// Rank-2 transpose recipe (`[M,N] → [N,M]`, S5-U1).
///
/// The recipe is a copy with a column-major index remap: the output element
/// at flat index `k` (row `j = k / M`, col `i = k % M`) reads
/// `input[i * N + j]` — the matmul-VJP product (`wᵀ @ grad`) and the MLP
/// 4×4 transpose both lower to this shape. One thread per output element
/// under the elementwise-style 1D contract: no shared memory, no barriers,
/// no OOB reads (every thread reads exactly one in-bounds input element).
///
/// The contract ([`CollectionOpContract::Transpose`]) resolves the literal
/// rank-2 dims from the operand tensor type; the plan carries the dims
/// plus the dispatch facts the contract applied (workgroup lane count and
/// `M·N` dispatch extent), so an emitter lowers the recipe without
/// re-deriving the shape and the A3 cross-check can verify the signature
/// carries the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransposePlan {
    /// Source rows `M` (output columns).
    pub m: u64,
    /// Source columns `N` (output rows).
    pub n: u64,
    /// Workgroup lane count on `x` (the elementwise-style 1D contract
    /// applied by [`CollectionOpContract::Transpose`]).
    pub workgroup_x: u32,
    /// Dispatch extent on `x` (`M·N` output elements, one thread per
    /// output element).
    pub dispatch_x: u64,
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
