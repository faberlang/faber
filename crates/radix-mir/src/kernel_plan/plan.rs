//! Portable collective floor for backend-neutral kernel recipes.
//!
//! Every lane must be able to emit a collective as a tree reduction over
//! shared memory plus a barrier. Lane-ISA collectives such as simdgroup/warp
//! shuffle, reduce, or scan are recipe-local optimizations, not portable MIR
//! semantics: each optimization carries a portable fallback or an explicit
//! lane admission. The one admitted collective caller is the declared
//! `summa ex … filum …` distributed sum-reduce (SFR-5,
//! [`CollectionKernelPlan::DistributedSum`]): backends without a native arm
//! lower it through the shared-memory tree floor this plan pins, so the plan
//! law is satisfied by construction.

use crate::MirScalarLayout;
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
    /// Row-gather recipe (GI3-1: name frozen in this unit, S6-N1 precedent):
    /// `out[t] = table[ids[t]]` over a rank-2 `[V, D]` table with an id
    /// vector `[T]` → `[T, D]`. See [`GatherPlan`].
    Gather(GatherPlan),
    /// Root-mean-square-normalization recipe (GI3-1: name frozen in this
    /// unit, S6-N1 precedent): `x/sqrt(mean(x²)+eps)·γ`, no mean
    /// subtraction, eps from the const operand. See
    /// [`RmsNormalizationPlan`].
    RmsNormalization(RmsNormalizationPlan),
    /// Rotary-position-embedding recipe (GI3-1: name frozen in this unit,
    /// S6-N1 precedent): llama-arch NORM consecutive-pair rotation with
    /// host-precomputed cos/sin table inputs. See [`RopePlan`].
    Rope(RopePlan),
    /// Causal masked-row softmax recipe (GI3-1: name frozen in this unit,
    /// S6-N1 precedent): numerically stable row softmax over the causal
    /// scores, zero probability above the diagonal. See
    /// [`CausalMaskedSoftmaxPlan`].
    CausalMaskedSoftmax(CausalMaskedSoftmaxPlan),
    /// Packed quantized GEMM recipe (R-PACK-01A: name frozen; R-PACK-02A
    /// admits per-layout dequant resolution). In-kernel block/superblock
    /// dequant, f32 accumulate — never a whole-model F32 materialization.
    /// F32 falls back to [`CollectionKernelPlan::TiledMatMul`].
    /// See [`QuantizedMatMulPlan`].
    QuantizedMatMul(QuantizedMatMulPlan),
    /// Grouped/segmented packed GEMM over rank-3 expert slices (R-PACK-01A:
    /// name frozen; admission/resolution is R-PACK-03A). See
    /// [`GroupedMatMulPlan`].
    GroupedMatMul(GroupedMatMulPlan),
    /// Head-batched attention materialization (M3-U1). This is deliberately a
    /// distinct recipe from `GroupedMatMul`: attention carries explicit
    /// group/head axes, GQA mapping, and a sequence-major context output.
    BatchedAttention(BatchedAttentionPlan),
    /// Hybrid SSM conv1d recipe (R-PACK-01A: name frozen; admission is
    /// R-PACK-04A). See [`SsmConv1dPlan`].
    SsmConv1d(SsmConv1dPlan),
    /// Hybrid SSM linear-recurrence scan / state-update recipe (R-PACK-01A:
    /// name frozen; admission is R-PACK-04A). See [`SsmScanPlan`].
    SsmScan(SsmScanPlan),
    /// Declared distributed sum-reduce (`summa ex … filum …`, SFR-5): the
    /// first author-stated recipe — declaration, not pattern recognition.
    /// Carries the filum metadata, the term-body MIR, and the coordinate
    /// facts; backends without a native arm lower it through the
    /// shared-memory tree floor the plan pins (plan law by construction).
    /// See [`DistributedSumPlan`].
    DistributedSum(DistributedSumPlan),
    /// Lane-form kernel (gpu-reset C4): a compute function with a `Lane`
    /// builtin parameter and a written tensor resource, one thread per lane
    /// of the lane domain. The domain is the call-site shape of the exact
    /// source resource `source_resource`, `rank` axes; the launch
    /// geometry is the generic 1-D workgroup rule over its element count.
    LaneGrid { source_resource: usize, rank: u8 },
    /// Single-lane kernel (gpu-reset A2 rung 3): a compute function with no
    /// `thread lane` loop and no recipe is sequential, so it runs as exactly
    /// one lane. The launch is one thread (1x1x1); there is no `Lane`
    /// builtin and no over-launch guard, and a `mut` tensor the body writes
    /// may also be read in the same body.
    SingleLane,
    /// Whole-grid reduction (gpu-reset C4-8): the function's lane region is
    /// one `reduce via Op from t at [...] thread lane ...` node
    /// ([`MirFilumForm::Grid`](crate::MirFilumForm::Grid)). The dispatcher
    /// runs two launches of two entry points emitted from the one function
    /// (stage 1: grid-stride fold plus a workgroup tree writing one partial
    /// per workgroup; stage 2: one workgroup combines the partials, then the
    /// destination store runs) through a dispatcher-owned partials scratch
    /// ([`MirKernelResourceKind::PartialsScratch`](crate::MirKernelResourceKind::PartialsScratch)).
    /// `source_resource` names the reduced tensor's storage resource (the
    /// node's source, never "the first read buffer"); `rank` is its axis
    /// count.
    GridReduce {
        source_resource: usize,
        rank: u8,
        /// Literal output bucket shape for grouped sum/count. `None` keeps
        /// the scalar whole-grid reduction layout.
        group_shape: Option<Vec<u64>>,
        /// Workgroup lane count of both stages (a power of two).
        workgroup: u32,
        /// Maximum source workgroups; scratch holds this many records per bucket.
        partials_cap: u32,
        /// The byte layout of one partial.
        partial_layout: GridReducePartialLayout,
    },
    /// Ordered prefix scan. Stage 1 writes chunk-local prefixes and summaries,
    /// stage 2 turns summaries into carries, and stage 3 applies each carry
    /// into a fresh source-shaped result tensor.
    Scan {
        source_resource: usize,
        rank: u8,
        workgroup: u32,
        blocks_cap: u32,
        group_count: u32,
        partial_layout: GridReducePartialLayout,
        /// Byte width of one output prefix value in `PrefixScratch`.
        element_bytes: u32,
        reverse: bool,
    },
}

impl CollectionKernelPlan {
    /// Number of buckets in a whole-grid reduction or scan.
    #[must_use]
    pub fn group_count(&self) -> Option<u64> {
        match self {
            CollectionKernelPlan::GridReduce { group_shape, .. } => match group_shape {
                None => Some(1),
                Some(shape) => shape
                    .iter()
                    .try_fold(1_u64, |count, extent| count.checked_mul(*extent)),
            },
            CollectionKernelPlan::Scan { group_count, .. } => Some(u64::from(*group_count)),
            _ => None,
        }
    }

    /// Total partial records in a whole-grid reduction's workgroup-by-bucket
    /// scratch. The multiplication is checked before a dispatcher allocates.
    #[must_use]
    pub fn grid_reduce_partial_slots(&self) -> Option<u64> {
        let CollectionKernelPlan::GridReduce { partials_cap, .. } = self else {
            return None;
        };
        u64::from(*partials_cap).checked_mul(self.group_count()?)
    }
}

/// Workgroup lane count of a [`CollectionKernelPlan::GridReduce`] (both
/// stages).
pub const GRID_REDUCE_WORKGROUP: u32 = 256;

/// Partial slots in the [`CollectionKernelPlan::GridReduce`] scratch.
pub const GRID_REDUCE_PARTIALS_CAP: u32 = 1024;

/// Source workgroups (partial rows) of a grouped grid reduction's first
/// stage. That stage launches `rows * buckets` workgroups of `workgroup`
/// lanes, and Metal addresses a lane's grid position as a 32-bit integer, so
/// the rows shrink until the launch stays below 2^32 lanes. Host geometry and
/// every device emitter use this one value; the scratch keeps `cap * buckets`
/// slots, of which the first `rows * buckets` are used.
#[must_use]
pub fn grouped_partial_rows(cap: u32, workgroup: u32, buckets: u64) -> u32 {
    let per_row = u64::from(workgroup.max(1)).saturating_mul(buckets.max(1));
    let rows = u64::from(u32::MAX) / per_row;
    u32::try_from(rows).unwrap_or(u32::MAX).clamp(1, cap.max(1))
}

/// The byte layout of one partial of a [`CollectionKernelPlan::GridReduce`].
///
/// Algebraic operators (sum, product, count, all, any) carry the value only.
/// Ordered operators (max, min, argmax, argmin) also carry the `seen` flag
/// (the combine order is "(value desc, coord asc); a number beats NaN; NaN
/// beats unseen"), and the arg operators carry one `u32` coordinate per
/// source axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridReducePartialLayout {
    /// Byte width of the value (at least four: counts and flags widen to a
    /// `u32`).
    pub value_bytes: u32,
    /// `u32` coordinates carried after the value (the source rank for
    /// argmax/argmin, otherwise zero).
    pub coordinates: u8,
    /// Whether a `u32` `seen` flag follows.
    pub seen: bool,
}

impl GridReducePartialLayout {
    /// The layout for `op` over a `rank`-axis source whose value is
    /// `value_bytes` wide.
    #[must_use]
    pub fn for_op(op: crate::MirReduceOp, value_bytes: u32, rank: u8) -> Self {
        use crate::MirReduceOp as Op;
        let ordered = matches!(op, Op::Max | Op::Min | Op::ArgMax | Op::ArgMin);
        Self {
            value_bytes: value_bytes.max(4),
            coordinates: if matches!(op, Op::ArgMax | Op::ArgMin) {
                rank
            } else {
                0
            },
            seen: ordered,
        }
    }

    /// Size of one partial in bytes (a multiple of four).
    #[must_use]
    pub fn element_bytes(self) -> u32 {
        self.value_bytes.div_ceil(4) * 4
            + u32::from(self.coordinates) * 4
            + if self.seen { 4 } else { 0 }
    }
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
    /// The producer-defined reduced-resource projection (council-8 CB-3):
    /// `axis_extent` (`dims[axis]`) and `inner_stride` (the dim product
    /// after the axis). Carried into each consuming
    /// [`MirKernelResource`](crate::abi::MirKernelResource) /
    /// `WireBufferVersion` so consumers never reconstruct the reduced-buffer
    /// mapping from element count — ambiguous for equal dims (a `[4,4]`
    /// axis-0 vs axis-1 reduction both produce 4 elements but map
    /// differently).
    pub projection: crate::abi::ReducedProjection,
}

/// Row-wise softmax recipe plan (name frozen by S6-N1; admission, resolution,
/// and decomposition implemented by S6-P1). Operates over the last (row)
/// axis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSoftmaxPlan {
    /// Whether the extents are compile-time literals or bound at run time.
    pub dims: RowOpDims,
    /// Row axis (the last axis in v1).
    pub axis: u64,
    /// U2 head-axis batch facts; `None` is the ordinary rank-1/2 image
    /// (rank-2 resolution and the legacy wire decode) — a batched fact can
    /// never decode as rank-2 by omission.
    pub batched: Option<RowSoftmaxBatch>,
}

/// U2 head-axis batch facts on the row-softmax recipe: the softmax operand
/// carries leading head/query axes, so the last-axis row law runs once per
/// flattened leading-axis row. The last two axes are `query_seq × key_seq`
/// and are NOT required to be square (decode is rectangular `1 × L`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSoftmaxBatch {
    /// Product of the leading head/query axes — the row count the body's
    /// row-wise law runs over.
    pub batch: u64,
    /// Last-axis (row) width — the key extent each row softmaxes over.
    pub width: u64,
    /// Additive mask operand facts (`None` for a plain batched softmax).
    /// The mask is data (0 at admitted positions, negative beyond), kept
    /// as an operand — never folded into the softmax or the scale.
    pub mask: Option<RowSoftmaxMask>,
}

/// U2 head-axis additive-mask facts: the mask buffer's element count under
/// the right-aligned broadcast (the mask dims are a trailing block of the
/// score dims; each mask dim equals the score dim or is a leading 1). The
/// body reads `mask[flat % count]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSoftmaxMask {
    /// The mask buffer's element count; positive and divides
    /// `batch * width`.
    pub count: u64,
}

/// Layer-normalization recipe plan (name frozen by S6-N1; admission,
/// resolution, and decomposition implemented by S6-P1). The full field set
/// (epsilon, affine) is resolved by P1 from the const operands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerNormalizationPlan {
    /// Whether the extents are compile-time literals or bound at run time.
    pub dims: RowOpDims,
    /// Normalization axis (from the trailing const operand).
    pub axis: u64,
    /// Epsilon as f32 bits. A zero value is reserved for a dynamic epsilon
    /// operand, which remains explicit in the MIR call and kernel signature.
    pub epsilon_bits: u32,
}

/// Row-gather recipe plan (GI3-1: name frozen in this unit, S6-N1
/// precedent). The typed facts resolve from the `TensorGather` statement:
/// the rank-2 `[V, D]` table dims and the id-vector length `T` (the output
/// `[T, D]` row count, resolved from the statement's return-type dims). The
/// CCI-1 source contract validated the ids' *literal* values only, so the
/// emitted body bounds-checks the runtime token against `table_rows`
/// (SEC-G1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatherPlan {
    /// Table rows `V` (the first axis of `token_embd.weight` for the
    /// pinned row).
    pub table_rows: PlanDim,
    /// Table columns `D` (the row width; also the output row width).
    pub table_cols: PlanDim,
    /// Id-vector length `T` (the output row count).
    pub id_count: PlanDim,
}

/// Root-mean-square-normalization recipe plan (GI3-1: name frozen in this
/// unit, S6-N1 precedent). `y = x·(1/sqrt(mean(x²)+eps))·gamma`, no mean
/// subtraction, eps resolved from the const operand and the affine gamma
/// peer's width cross-checked against the normalization dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RmsNormalizationPlan {
    /// Whether the extents are compile-time literals or bound at run time.
    pub dims: RowOpDims,
    /// Normalization axis (from the trailing const operand; the last axis
    /// in v1).
    pub axis: u64,
    /// Epsilon as f32 bits. A zero value is reserved for a dynamic epsilon
    /// operand, which remains explicit in the MIR call and kernel signature.
    pub epsilon_bits: u32,
    /// Normalization dimension width (the row extent gamma spans).
    pub width: u64,
}

/// Rotary-position-embedding recipe plan (GI3-1: name frozen in this unit,
/// S6-N1 precedent). llama-arch NORM consecutive-pair rotation: pairs
/// `(x[2k], x[2k+1])` with `theta[k] = pos·freq_base^(−2k/dim)`, where
/// `freq_base` is whatever the caller used to build the explicit rope table
/// operand (the operand carries the base; the compiler holds no constant);
/// elements beyond `dim` on the last axis are preserved unrotated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RopePlan {
    /// Position (from the const operand). Single-position mode only — in
    /// per-row mode the device body never reads it (the per-row positions
    /// ride the rank-2 tables).
    pub pos: u64,
    /// Rotated width (from the const operand; positive and even).
    pub dim: u64,
    /// Row width (the last-axis extent; elements beyond `dim` preserved).
    pub width: u64,
    /// Per-position rotation (GI3-1 contract amendment, task): each
    /// row rotates at its own position via the rank-2 `[rows, dim/2]`
    /// cos/sin tables. The mode is a typed fact of the kernel — the table
    /// operand shapes (rank-2 = per-row, rank-1 `[dim/2]` = single-position),
    /// never inferred from text (A3). Always `true` today because
    /// single-position mode is not built yet; the flag is an unfinished
    /// feature, not a dead flag (operator ruling 2026-10-13, need,
    /// finding G-04). Do not delete it; gpu-reset decides whether
    /// single-position mode exists.
    pub per_row: bool,
    /// The row count (per-row mode; the table row stride — each row's
    /// `dim/2`-entry angle segment). Zero in single-position mode.
    pub rows: u64,
    /// Qwen2 / GPT-NeoX `rotate_half` pairing (`TensorRopeHalf`). False is
    /// the llama consecutive-pair default (`TensorRope`); the field is a
    /// typed fact of which MIR op resolved the plan, never inferred from
    /// text (A3).
    pub rotate_half: bool,
}

/// PGC-R5 row-reduction launch lane width: one simdgroup (32 lanes) per
/// row under the `RowReduction` contract. The tree reduction over the
/// workgroup's lanes and the `float4` scale pass both key off this width.
pub const ROW_REDUCTION_WORKGROUP_X: u32 = 32;

/// PGC-R5 row-reduction admission facts for the `RowReduction` contract
/// (`TensorRmsNorm` / `TensorCausalMaskedSoftmax`): one workgroup per row,
/// 32 lanes, tree-reduced in threadgroup memory. Shared by the ABI
/// contract resolver (`abi/contract.rs`) and the Metal emitter arms so the
/// resolved geometry and the emitted body can never disagree (A3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowReductionFacts {
    /// Row count (the dispatch extent — one workgroup per row).
    pub rows: u64,
    /// Row width (the reduction extent each lane strides over).
    pub width: u64,
    /// Lanes per row workgroup (`ROW_REDUCTION_WORKGROUP_X`).
    pub workgroup_x: u32,
}

/// PGC-R5: admit the row-reduction contract for an RMSNorm-shaped operand
/// `[rows, width]`. Admission requires a full simdgroup's worth of strided
/// columns (`width >= 32`) and a `float4`-aligned width for the vectorized
/// scale pass; any other shape keeps the generic per-element 1D contract
/// and its frozen emission. Unlike the causal-softmax twin this admits
/// single-row shapes (`head_rmsnorm` is T=1) — the recipe is row-wise at
/// any row count and the decode twin (`decode_rmsnorm`, the same MIR
/// shape) rides the same family, declared Class B in the R5 receipt.
#[must_use]
pub fn rmsnorm_row_reduction_admission(rows: u64, width: u64) -> Option<RowReductionFacts> {
    row_reduction_admission(rows, width)
}

/// PGC-R5: admit the row-reduction contract for a causal masked-softmax
/// operand `[rows, cols]`. Multi-row only: the decode T=1 masked softmax
/// (`decode_masked_softmax`, B1/B2-landed extents and body) keeps the
/// generic per-element 1D contract byte-for-byte.
#[must_use]
pub fn causal_softmax_row_reduction_admission(rows: u64, cols: u64) -> Option<RowReductionFacts> {
    if rows <= 1 {
        return None;
    }
    row_reduction_admission(rows, cols)
}

fn row_reduction_admission(rows: u64, width: u64) -> Option<RowReductionFacts> {
    if rows == 0 || width < u64::from(ROW_REDUCTION_WORKGROUP_X) || !width.is_multiple_of(4) {
        return None;
    }
    Some(RowReductionFacts {
        rows,
        width,
        workgroup_x: ROW_REDUCTION_WORKGROUP_X,
    })
}

/// Causal masked-row softmax recipe plan (GI3-1: name frozen in this unit,
/// S6-N1 precedent). Row `i` of the causal score matrix masks columns
/// `j > i` (zero probability above the diagonal; the diagonal is included)
/// before the numerically stable row softmax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalMaskedSoftmaxPlan {
    /// Whether the extents are compile-time literals or bound at run time.
    pub dims: RowOpDims,
    /// Row (query) extent.
    pub rows: u64,
    /// Column (key) extent.
    pub cols: u64,
}

/// Packed quantized GEMM plan (R-PACK-01A: name frozen). The layout class
/// is a typed fact from the trailing GGML type-id const — never inferred
/// from a filename or wire text (A3). Dequant happens in-kernel per
/// block/tile; the plan does not admit a whole-model F32 expansion path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantizedMatMulPlan {
    /// Left matrix rows `M`.
    pub m: u64,
    /// Contracted inner dimension `K`.
    pub k: u64,
    /// Right matrix columns `N`.
    pub n: u64,
    /// Stored weight layout (the packed operand).
    pub layout: crate::abi::PackedStorageLayout,
    /// Square tile edge; the contract dispatches one workgroup per
    /// `tile × tile` output tile (same grid as [`MatMulPlan`]).
    pub tile: u32,
    /// Workgroup lane count on `x`; must equal `tile`.
    pub workgroup_x: u32,
    /// Workgroup lane count on `y`; must equal `tile`.
    pub workgroup_y: u32,
}

impl QuantizedMatMulPlan {
    /// Typed per-layout dequant facts (R-PACK-02A). Derived from
    /// [`Self::layout`] — not a stored field (wire-stable).
    #[must_use]
    pub fn dequant_resolution(&self) -> crate::kernel_plan::PackedDequantResolution {
        crate::kernel_plan::PackedDequantResolution::for_layout(self.layout)
    }
}

/// Grouped packed GEMM over rank-3 expert slices (R-PACK-01A: name frozen).
/// `groups` is the expert-axis extent resolved from the rank-3 operand —
/// never assumed from a model name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedMatMulPlan {
    /// Left matrix rows `M` (the ungrouped activation rows).
    pub m: u64,
    /// Contracted inner dimension `K`.
    pub k: u64,
    /// Right matrix columns `N`.
    pub n: u64,
    /// Expert / group count (the leading rank-3 axis).
    pub groups: u64,
    /// Stored expert-weight layout.
    pub layout: crate::abi::PackedStorageLayout,
    /// Square tile edge (same grid as [`MatMulPlan`]).
    pub tile: u32,
    /// Workgroup lane count on `x`; must equal `tile`.
    pub workgroup_x: u32,
    /// Workgroup lane count on `y`; must equal `tile`.
    pub workgroup_y: u32,
}

/// The target-neutral head-batched attention operation carried by M3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchedAttentionKind {
    /// Project the sequence-major hidden state into query heads.
    ProjectionQ,
    /// Project the sequence-major hidden state into key heads.
    ProjectionK,
    /// Project the sequence-major hidden state into value heads.
    ProjectionV,
    /// Compute grouped Q·Kᵀ scores and apply the frozen attention scale.
    Scores,
    /// Apply independent causal row softmax to every `(group, head)` row.
    Softmax,
    /// Compute grouped probabilities·V directly into sequence-major hidden
    /// storage. This is the sentinel-proven layout seam: no scatter/permute.
    ContextSequenceMajor,
}

/// M3 batched attention contract. All axes and strides are explicit facts;
/// an emitter must not infer them from a physical byte length or a model name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchedAttentionPlan {
    /// Operation family being materialized.
    pub kind: BatchedAttentionKind,
    /// Number of KV groups.
    pub kv_heads: u64,
    /// Query heads sharing each KV head (GQA mapping).
    pub q_per_kv: u64,
    /// Sequence extent of THIS op (projection input rows M for Q/K/V).
    pub seq: u64,
    /// Per-head feature width.
    pub head_dim: u64,
    /// Hidden-state width (used by projections and sequence-major context).
    pub hidden: u64,
    /// Packed weight layout for projection operations; `None` otherwise.
    pub layout: Option<crate::abi::PackedStorageLayout>,
    /// Explicit `NeoX` rotate-half projection mapping, when fused into Q/K.
    pub rotate_half: bool,
    /// Row-major physical strides for the logical output view.
    pub output_strides: [u64; 4],
    /// IEEE-754 bits of the scale applied by scores before softmax.
    pub scale_bits: u32,
    /// Typed launch fact for the decode (M=1) projection body: the number
    /// of logical outputs one 64-thread workgroup computes (the body
    /// partitions the workgroup into this many eight-lane reductions and
    /// guards each output with `output_valid`). `Some(n)` for single-row
    /// Q/K/V projections; `None` keeps the generic one-thread-per-output
    /// launch law. The launch grid must be `ceil(logical / n)` workgroups —
    /// deriving it from `ceil(logical / 64)` under-dispatches by `64 / n`
    /// (R4 repair: 15 workgroups published 120 of 960 logical Q). The
    /// LOGICAL output span is unchanged by this fact.
    pub decode_outputs_per_workgroup: Option<u64>,
}

/// SSM conv1d plan (R-PACK-01A: name frozen). Length, channel, and kernel
/// width come from the operand tensor dims — never invented (MODEL-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsmConv1dPlan {
    /// Sequence length.
    pub length: u64,
    /// Channel extent.
    pub channels: u64,
    /// Convolution kernel width.
    pub kernel_width: u64,
    /// Stored kernel/state layout.
    pub layout: crate::abi::PackedStorageLayout,
}

/// SSM linear-recurrence scan / state-update plan (R-PACK-01A: name frozen).
/// Length and state dim come from the operand tensor dims — never invented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsmScanPlan {
    /// Sequence length.
    pub length: u64,
    /// Recurrence state dimension.
    pub state_dim: u64,
    /// Stored state/weight layout.
    pub layout: crate::abi::PackedStorageLayout,
}

/// Declared distributed sum-reduce plan (`summa ex … filum …`, SFR-5). The
/// recipe is stated by the author, not recognized from a source pattern:
/// `filum` carries the filum metadata (lane binder, frozen lane width),
/// the term-body MIR, and the coordinate facts the per-lane inliner binds.
///
/// The floor facts (`workgroup_x`, `shared_memory`, `barriers`,
/// `oob_padding`) mirror the [`ReductionPlan`] tree recipe exactly: a
/// backend with no native collective arm emits the shared-memory tree
/// floor from these carried facts, so the portable-collective law of this
/// module is satisfied by construction. `length` is the source tensor's
/// element count `K`, resolved from the kernel signature's source buffer
/// (never guessed); `partials` is the per-workgroup partial slot count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributedSumPlan {
    /// The filum declaration facts: filum metadata, term-body MIR, and
    /// coordinate binders (the module lowering's captured term body).
    pub filum: crate::MirFilumReduceOp,
    /// True flat source length `K` (the source buffer's element count).
    pub length: u64,
    /// Workgroup lane count on `x` — equals the filum lane width `W`.
    pub workgroup_x: u32,
    /// Per-workgroup partial slot count `ceil(length / workgroup_x)`.
    pub partials: u64,
    /// Shared-memory buffer the tree floor reduces through.
    pub shared_memory: SharedMemoryLayout,
    /// Ordered barrier schedule of the tree floor: the post-load barrier
    /// first, then one barrier after every tree halving step.
    pub barriers: Vec<BarrierPoint>,
    /// Zero-fill policy for tail elements beyond `length`.
    pub oob_padding: OobPaddingPolicy,
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
    /// Right matrix columns `N` (one for a promoted rank-1 RHS).
    pub n: u64,
    /// Element layout of the left operand; dense tiles widen into f32.
    pub left_layout: MirScalarLayout,
    /// Element layout of the right operand; dense tiles widen into f32.
    pub right_layout: MirScalarLayout,
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
    /// Whether the extents are compile-time literals or bound at run time.
    pub dims: MatMulDims,
    /// Left matrix rows `M` (the literal arm of [`MatMulPlan::dims`]).
    pub m: u64,
    /// Contracted inner dimension `K` (left columns / right rows; the
    /// literal arm of [`MatMulPlan::dims`]).
    pub k: u64,
    /// Right matrix columns `N` (one for a promoted rank-1 RHS; the literal
    /// arm of [`MatMulPlan::dims`]).
    pub n: u64,
    /// Element layout of the left operand; dense tiles stay f32 after load.
    pub left_layout: MirScalarLayout,
    /// Element layout of the right operand; dense tiles stay f32 after load.
    pub right_layout: MirScalarLayout,
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
    /// Product of the receiver's leading batch axes (head-axis batched
    /// matmul, U1 head-axis): `1` for an ordinary rank-2 contraction. The
    /// launch folds the batch into `dispatch_y` — one padded row-block per
    /// batch — so the workgroup/tile laws are unchanged.
    pub batch: u64,
    /// Product of the argument's leading batch axes. The argument batch
    /// dims are a PREFIX of the receiver's (the GQA share: K/V per kv-head
    /// broadcast over q-per-kv heads); `right_batch` divides `batch`.
    pub right_batch: u64,
}

/// How a [`MatMulPlan`] knows its extents: literal numbers, or run-time
/// extents selected by shape class. Nothing produces `Runtime` yet; it is
/// only representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatMulDims {
    /// Compile-time extents `M`, `K`, `N`.
    Literal { m: u64, k: u64, n: u64 },
    /// Extents bound at run time; the recipe body is the one for `class`.
    Runtime { class: MatMulClass },
}

/// A planned tensor extent: a compile-time literal or a resource-bound
/// extent. `Runtime` means the extent is bound at launch from a
/// `TensorAxisExtent` resource, never a number and never a silent default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanDim {
    /// A compile-time extent.
    Literal(u64),
    /// Bound at launch from a `TensorAxisExtent` resource.
    Runtime,
}

impl PlanDim {
    /// Return the compile-time extent, if this dimension is literal.
    #[must_use]
    pub const fn literal(self) -> Option<u64> {
        match self {
            Self::Literal(value) => Some(value),
            Self::Runtime => None,
        }
    }

    /// Whether this dimension is bound at run time.
    #[must_use]
    pub const fn is_runtime(self) -> bool {
        matches!(self, Self::Runtime)
    }
}

/// The matmul body a run-time-extent plan selects (one per body the literal
/// form chooses by value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatMulClass {
    /// `M == 1` one-row GEMV.
    OneRow,
    /// `N == 1` direct GEMV.
    Gemv,
    /// Simdgroup fast path.
    Simdgroup,
    /// Scalar tiled body.
    Tiled,
}

/// How a row operation plan (row softmax, layer norm, RMS norm, causal
/// softmax) knows its extents: literal numbers, or run-time extents selected
/// by shape class. The row-op twin of [`MatMulDims`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowOpDims {
    /// Compile-time extents. `rows` is the product of the leading axes (`1`
    /// at rank 1) and `width` the last axis.
    Literal { rows: u64, width: u64 },
    /// Extents bound at run time. The literal numbers are zero and `class` is
    /// the plan-time placeholder ([`RowOpClass::Generic1D`]) until the launch
    /// selects the real class from the call-site shape.
    Runtime { class: RowOpClass },
}

/// The row-op body a run-time-extent plan selects (one per body the literal
/// form chooses by value). The class follows from the call-site
/// `[rows, width]` at launch: row reduction for `width >= 32 && width % 4 ==
/// 0`, row-mapped for the wide flat shapes, generic 1D otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowOpClass {
    /// Flat one-thread-per-element body.
    Generic1D,
    /// One workgroup per row tree reduction.
    RowReduction,
    /// Row-mapped flat body (guards `if o < rows`).
    RowMapped,
}

/// The two shared-memory operand tiles of the tiled-matmul recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatMulSharedMemory {
    /// Left-matrix tile (`A[row][k_start + tx]`), `tile × tile` slots.
    pub shared_a: SharedMemoryLayout,
    /// Right-matrix tile (`B[k_start + ty][col]`), `tile × tile` slots.
    pub shared_b: SharedMemoryLayout,
}

/// PGC-R4 simdgroup fast-path staging facts (Metal override of the
/// `TiledMatMul` recipe). The fast path double-buffers its two operand
/// staging tiles so a partial output band needs only ONE barrier per K
/// slice (the scalar body's post-load + post-inner-product pair is gone),
/// and combines the two simdgroups' K-parity partial accumulators through
/// one `SIMDGROUP_PARTIAL_SLOTS` threadgroup buffer at kernel end.
pub const SIMDGROUP_MATMUL_STAGING_SLOTS: u32 = 128;

/// PGC-R4 wave bound: the fast path admits projection-class output
/// widths only. The vocab-scale `prefill_lm_head_gemv` twin carries a
/// frozen scalar-emission pin (the terminal-row narrowing family is
/// PGC-R2/C2 territory, not this recipe wave), so its 49,152-column
/// launch keeps the canonical body until a follow-up card owns it.
pub const SIMDGROUP_MATMUL_MAX_N: u64 = 8192;

/// PGC-R4: the K-parity partial-accumulator combine buffer slots (two
/// simdgroups × 64 elements).
pub const SIMDGROUP_MATMUL_PARTIAL_SLOTS: u32 = 128;

/// PGC-R4: the simdgroup fast-path admission facts for a validated
/// `TiledMatMul` plan. This is NOT a new `CollectionKernelPlan` variant —
/// the module law ("a backend either lowers a recipe to target syntax or
/// overrides the whole body with a target fast path") makes the
/// simdgroup-matrix body a backend-local override of the SAME recipe: the
/// dispatch geometry, signature, and wire plan facts stay byte-identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimdgroupMatMulFacts {
    /// `ceil(k / tile)` K slices the two simdgroups split by parity.
    pub k_tiles: u64,
    /// Staging slots per operand tile pair (double-buffered `tile × tile`).
    pub staging_slots: u32,
    /// Partial-accumulator combine buffer slots.
    pub partial_slots: u32,
}

/// PGC-R4: admit the Metal simdgroup-matrix fast path for a tiled-matmul
/// plan. Plan-level gates: the multi-row law (`m > 1` with the canonical
/// `(tile, tile)` workgroup), the rank-2 law (`batch == 1` — a batched
/// plan keeps the scalar cooperative-tile body and its batch decomposition),
/// `k` a tile multiple (the direct device `simdgroup_load` path admits no
/// K tail), f32 operands, and `n` under the projection-class cap. Every
/// `m` is admitted — the full output bands take the zero-barrier direct
/// device loads and the `m`/`n` tail bands the guarded float4 staging
/// path (tile-multiple square and tall GEMMs are the Whisper-encoder
/// class; the prefill `M = 36` family keeps its partial-band staging).
/// Backend-local declarations (strided/chunked/gathered operand windows,
/// launch output windows) are checked at the emitter and fall back to the
/// scalar body.
#[must_use]
pub fn simdgroup_matmul_admission(plan: &MatMulPlan) -> Option<SimdgroupMatMulFacts> {
    if plan.tile != MATMUL_TILE
        || plan.workgroup_x != MATMUL_TILE
        || plan.workgroup_y != MATMUL_TILE
        || plan.batch != 1
        || plan.left_layout != MirScalarLayout::F32
        || plan.right_layout != MirScalarLayout::F32
        || !simdgroup_extents_admitted(plan.m, plan.k, plan.n)
    {
        return None;
    }
    Some(SimdgroupMatMulFacts {
        k_tiles: plan.k / u64::from(plan.tile),
        staging_slots: SIMDGROUP_MATMUL_STAGING_SLOTS,
        partial_slots: SIMDGROUP_MATMUL_PARTIAL_SLOTS,
    })
}

/// The extent half of the simdgroup admission: multi-row, `k` a nonzero tile
/// multiple, `n` under the projection-class cap. Shared
/// by [`simdgroup_matmul_admission`] (literal plans) and
/// [`matmul_class_for_extents`] (run-time extents) so the two cannot drift.
fn simdgroup_extents_admitted(m: u64, k: u64, n: u64) -> bool {
    let tile = u64::from(MATMUL_TILE);
    m > 1 && k != 0 && k.is_multiple_of(tile) && n <= SIMDGROUP_MATMUL_MAX_N
}

/// The matmul class the literal path selects for these extents (design note
/// item 7, section 5 rule 3): `OneRow` when `m == 1`, else `Gemv` when
/// `n == 1`, else `Simdgroup` when the simdgroup admission holds and
/// `n % 4 == 0` (the `float4` stage read of a row-major right operand), else
/// `Tiled`. `None` for `k == 1`: the rank-1 outer update stays literal-only.
#[must_use]
pub fn matmul_class_for_extents(m: u64, k: u64, n: u64) -> Option<MatMulClass> {
    if k == 1 {
        return None;
    }
    Some(if m == 1 {
        MatMulClass::OneRow
    } else if n == 1 {
        MatMulClass::Gemv
    } else if simdgroup_extents_admitted(m, k, n) && n.is_multiple_of(4) {
        MatMulClass::Simdgroup
    } else {
        MatMulClass::Tiled
    })
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
    /// Maximum fold over the tensor elements.
    Max,
    /// Minimum fold over the tensor elements.
    Min,
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
    /// U3 head-axis batch count; `None` is the unchanged rank-2 image
    /// (rank-2 resolution and the legacy wire decode) — a batched fact can
    /// never decode as rank-2 by omission. The leading head axes fold into
    /// the same 1D contract: `dispatch_x` is `batch·M·N`.
    pub batched: Option<u64>,
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
