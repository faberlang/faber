use super::plan::{
    AxisReductionPlan, BarrierPhase, BarrierPoint, CausalMaskedSoftmaxPlan, DistributedSumPlan,
    GatherPlan, GroupedMatMulPlan, MatMulPlan, QuantizedMatMulPlan, ReduceOp, ReductionPlan,
    RmsNormalizationPlan, RopePlan, RowSoftmaxPlan, SharedMemoryLayout, SsmConv1dPlan, SsmScanPlan,
    TransposePlan, WORKGROUP_SHARED_MEMORY_LIMIT_BYTES,
};
use std::fmt;

pub(super) fn validate_reduction_plan(plan: &ReductionPlan) -> Result<(), PlanValidationError> {
    validate_barrier_dominance(plan)?;
    validate_output_write_exclusivity(plan)?;
    validate_shared_memory_sizing(&plan.shared_memory)?;
    validate_oob_padding_coverage(plan)?;
    Ok(())
}

/// Declared distributed sum-reduce (SFR-5): the tree-floor facts must pass
/// the same checks as the [`ReductionPlan`] recipe (the floor IS the tree
/// recipe), plus the filum declaration coherence — lane width equals the
/// workgroup, and the term-body/coordinate facts name distinct locals.
pub(super) fn validate_distributed_sum_plan(
    plan: &DistributedSumPlan,
) -> Result<(), PlanValidationError> {
    let floor = ReductionPlan {
        op: ReduceOp::Sum,
        length: plan.length,
        workgroup_x: plan.workgroup_x,
        partials: plan.partials,
        shared_memory: plan.shared_memory.clone(),
        barriers: plan.barriers.clone(),
        oob_padding: plan.oob_padding,
    };
    validate_reduction_plan(&floor)?;
    if plan.filum.lane_width != plan.workgroup_x {
        return Err(PlanValidationError::DistributedSumLaneWidthMismatch {
            lane_width: plan.filum.lane_width,
            workgroup_x: plan.workgroup_x,
        });
    }
    if plan.length == 0 {
        return Err(PlanValidationError::DistributedSumZeroLength);
    }
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
    let mut cross_recipe_barriers = 0u32;
    for point in &plan.barriers {
        match point.after {
            BarrierPhase::SharedMemoryLoad => load_barriers += 1,
            BarrierPhase::ReductionStep => reduction_barriers += 1,
            // A matmul-only phase has no place in the tree recipe; the plan
            // is a closed recipe set (L2), so a cross-recipe barrier is a
            // malformed plan.
            BarrierPhase::InnerProductStep => cross_recipe_barriers += 1,
        }
    }
    if cross_recipe_barriers > 0 {
        return Err(PlanValidationError::BarrierDominance {
            phase: BarrierPhase::InnerProductStep,
            required_count: 0,
            present_count: cross_recipe_barriers,
        });
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

/// OOB-padding coverage (L12): the dispatched grid must not leave real
/// elements inside the "padding" range, so the partial count must tile the
/// tensor (`partials == ceil(length / workgroup_x)`). A zero workgroup or
/// zero length dispatches an empty/garbage grid and is rejected here.
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
    Ok(())
}

pub(super) fn validate_matmul_plan(plan: &MatMulPlan) -> Result<(), PlanValidationError> {
    validate_matmul_workgroup(plan)?;
    validate_matmul_batch_law(plan)?;
    validate_matmul_shared_memory(plan)?;
    validate_matmul_barrier_coverage(plan)?;
    validate_matmul_zero_dimension(plan)?;
    Ok(())
}

/// U1 head-axis batch law: both batch counts are positive and the argument
/// batch count divides the receiver's (the argument batch dims are a
/// prefix of the receiver's — the GQA share; the prefix itself is a
/// resolution-time typed fact, the plan carries only the counts).
fn validate_matmul_batch_law(plan: &MatMulPlan) -> Result<(), PlanValidationError> {
    if plan.batch == 0 || plan.right_batch == 0 || !plan.batch.is_multiple_of(plan.right_batch) {
        return Err(PlanValidationError::MatMulBatchLaw {
            batch: plan.batch,
            right_batch: plan.right_batch,
        });
    }
    Ok(())
}

/// U2 head-axis row-softmax batch law: positive batch/width and — when the
/// additive mask operand is carried — a positive mask count that divides
/// `batch · width` (the mask dims are a trailing block of the score dims,
/// so the broadcast read `flat % count` covers exactly the mask buffer).
/// The trailing-block geometry itself is a resolution-time typed fact; the
/// plan carries only the counts.
pub(super) fn validate_row_softmax_plan(plan: &RowSoftmaxPlan) -> Result<(), PlanValidationError> {
    let Some(batch) = &plan.batched else {
        // The ordinary rank-1/2 image: no static batch structure (every
        // algorithmic fact is a resolution-time typed check).
        return Ok(());
    };
    if batch.batch == 0 || batch.width == 0 {
        return Err(PlanValidationError::RowSoftmaxBatchLaw {
            batch: batch.batch,
            width: batch.width,
            mask_count: batch.mask.as_ref().map_or(0, |mask| mask.count),
        });
    }
    if let Some(mask) = &batch.mask {
        let total = batch.batch.saturating_mul(batch.width);
        if mask.count == 0 || !total.is_multiple_of(mask.count) {
            return Err(PlanValidationError::RowSoftmaxBatchLaw {
                batch: batch.batch,
                width: batch.width,
                mask_count: mask.count,
            });
        }
    }
    Ok(())
}

/// Transpose recipe validation (S5-U1): positive source dims (a zero dim
/// dispatches an empty grid and the remap arithmetic would divide by zero
/// when `M` or `N` is zero), a positive workgroup lane count, and a dispatch
/// extent that tiles exactly `M·N` output elements — one thread per output
/// element under the elementwise-style 1D contract. The recipe has no shared
/// memory or barriers, so those checks do not apply.
pub(super) fn validate_transpose_plan(plan: &TransposePlan) -> Result<(), PlanValidationError> {
    if plan.m == 0 || plan.n == 0 {
        return Err(PlanValidationError::TransposeZeroDimension {
            m: plan.m,
            n: plan.n,
        });
    }
    if plan.workgroup_x == 0 {
        return Err(PlanValidationError::TransposeZeroWorkgroup {
            workgroup_x: plan.workgroup_x,
        });
    }
    let count = match plan.batched {
        // U3 head-axis: the batch folds into the same 1D contract — the
        // dispatch tiles `batch·M·N` output elements, one thread per
        // element across the whole batched output.
        Some(batch) => {
            if batch == 0 {
                return Err(PlanValidationError::TransposeBatchLaw {
                    batch,
                    m: plan.m,
                    n: plan.n,
                    dispatch_x: plan.dispatch_x,
                });
            }
            batch.saturating_mul(plan.m).saturating_mul(plan.n)
        }
        None => plan.m.saturating_mul(plan.n),
    };
    if plan.dispatch_x != count {
        return Err(PlanValidationError::TransposeDispatchMismatch {
            m: plan.m,
            n: plan.n,
            dispatch_x: plan.dispatch_x,
        });
    }
    Ok(())
}

/// Axis-reduction recipe validation (S6-P1).
///
/// The v1 recipe set admits only `Sum` (a mean-over-axis is not part of the
/// transformer surface; the tree-reduction family already covers flat
/// means). The axis value, its in-range check, and the input/output element
/// counts are resolution-time typed-fact checks
/// ([`super::CollectionKernelPlan::from_transformer_op`] — the plan carries
/// only the resolved axis), so the static check pins the closed op set and
/// the carried reduced-resource projection (council-8 CB-3): a zero axis
/// extent or inner stride would make the producer-defined projection
/// degenerate.
pub(super) fn validate_axis_reduction_plan(
    plan: &AxisReductionPlan,
) -> Result<(), PlanValidationError> {
    if !matches!(plan.op, ReduceOp::Sum) {
        return Err(PlanValidationError::AxisReductionUnsupportedOp { op: plan.op });
    }
    if plan.projection.axis_extent == 0 || plan.projection.inner_stride == 0 {
        return Err(PlanValidationError::AxisReductionZeroProjection {
            axis_extent: plan.projection.axis_extent,
            inner_stride: plan.projection.inner_stride,
        });
    }
    Ok(())
}

/// Row-gather recipe validation (GI3-1). Positive dims only (a zero table
/// row/col or a zero id count dispatches an empty grid and the gather
/// remap would be degenerate); the id-range/table-shape facts are
/// resolution-time typed checks
/// ([`super::CollectionKernelPlan::from_transformer_op`]).
pub(super) fn validate_gather_plan(plan: &GatherPlan) -> Result<(), PlanValidationError> {
    if plan.table_rows == 0 || plan.table_cols == 0 || plan.id_count == 0 {
        return Err(PlanValidationError::GatherZeroDimension {
            table_rows: plan.table_rows,
            table_cols: plan.table_cols,
            id_count: plan.id_count,
        });
    }
    Ok(())
}

/// Root-mean-square-normalization recipe validation (GI3-1). A positive
/// normalization width (a zero width dispatches an empty row pass and the
/// rsqrt divisor would be degenerate); the axis-in-range / positive-finite
/// epsilon / gamma-width facts are resolution-time typed checks.
pub(super) fn validate_rms_normalization_plan(
    plan: &RmsNormalizationPlan,
) -> Result<(), PlanValidationError> {
    if plan.width == 0 {
        return Err(PlanValidationError::RmsNormalizationZeroWidth { width: plan.width });
    }
    Ok(())
}

/// Rotary-position-embedding recipe validation (GI3-1). A positive rotated
/// width (a zero `dim` admits no pair) and a positive row width; the
/// even-`dim` / in-extent facts are resolution-time typed checks.
pub(super) fn validate_rope_plan(plan: &RopePlan) -> Result<(), PlanValidationError> {
    if plan.dim == 0 || plan.width == 0 || (plan.per_row && plan.rows == 0) {
        return Err(PlanValidationError::RopeZeroDimension {
            pos: plan.pos,
            dim: plan.dim,
            width: plan.width,
            rows: plan.rows,
        });
    }
    Ok(())
}

/// Causal masked-row softmax recipe validation (GI3-1). Positive row/column
/// extents (a zero extent dispatches an empty grid and the row-mask remap
/// would be degenerate).
pub(super) fn validate_causal_masked_softmax_plan(
    plan: &CausalMaskedSoftmaxPlan,
) -> Result<(), PlanValidationError> {
    if plan.rows == 0 || plan.cols == 0 {
        return Err(PlanValidationError::CausalMaskedSoftmaxZeroDimension {
            rows: plan.rows,
            cols: plan.cols,
        });
    }
    Ok(())
}

/// Packed quantized GEMM validation (R-PACK-01A). Positive M/K/N and a
/// non-zero tile; the workgroup must equal the tile on both axes (same
/// coupling as [`validate_matmul_plan`]).
pub(super) fn validate_quantized_matmul_plan(
    plan: &QuantizedMatMulPlan,
) -> Result<(), PlanValidationError> {
    if plan.m == 0 || plan.k == 0 || plan.n == 0 || plan.tile == 0 {
        return Err(PlanValidationError::QuantizedMatMulZeroDimension {
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
    if plan.layout == crate::abi::PackedStorageLayout::F32 {
        return Err(PlanValidationError::QuantizedMatMulF32Fallback);
    }
    Ok(())
}

/// Grouped packed GEMM validation (R-PACK-01A). Positive M/K/N/groups and a
/// non-zero tile; the workgroup must equal the tile on both axes.
pub(super) fn validate_batched_attention_plan(
    plan: &super::plan::BatchedAttentionPlan,
) -> Result<(), PlanValidationError> {
    if plan.kv_heads == 0
        || plan.q_per_kv == 0
        || plan.seq == 0
        || plan.head_dim == 0
        || plan.hidden == 0
        || plan.output_strides.contains(&0)
    {
        return Err(PlanValidationError::BatchedAttentionInvalid);
    }
    // R4 repair: a typed decode launch fact of zero would divide by zero at
    // grid derivation; it must also partition the 64-thread workgroup.
    if plan
        .decode_outputs_per_workgroup
        .is_some_and(|outputs| outputs == 0 || outputs > 64 || 64 % outputs != 0)
    {
        return Err(PlanValidationError::BatchedAttentionInvalid);
    }
    if plan.kind == super::plan::BatchedAttentionKind::ContextSequenceMajor
        && plan.hidden
            != plan
                .kv_heads
                .saturating_mul(plan.q_per_kv)
                .saturating_mul(plan.head_dim)
    {
        return Err(PlanValidationError::BatchedAttentionInvalid);
    }
    Ok(())
}

pub(super) fn validate_grouped_matmul_plan(
    plan: &GroupedMatMulPlan,
) -> Result<(), PlanValidationError> {
    if plan.m == 0 || plan.k == 0 || plan.n == 0 || plan.groups == 0 || plan.tile == 0 {
        return Err(PlanValidationError::GroupedMatMulZeroDimension {
            m: plan.m,
            k: plan.k,
            n: plan.n,
            groups: plan.groups,
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

/// SSM conv1d validation (R-PACK-01A). Positive length, channels, and kernel
/// width (a zero extent dispatches an empty grid).
pub(super) fn validate_ssm_conv1d_plan(plan: &SsmConv1dPlan) -> Result<(), PlanValidationError> {
    if plan.length == 0 || plan.channels == 0 || plan.kernel_width == 0 {
        return Err(PlanValidationError::SsmConv1dZeroDimension {
            length: plan.length,
            channels: plan.channels,
            kernel_width: plan.kernel_width,
        });
    }
    Ok(())
}

/// SSM scan validation (R-PACK-01A). Positive length and state dim.
pub(super) fn validate_ssm_scan_plan(plan: &SsmScanPlan) -> Result<(), PlanValidationError> {
    if plan.length == 0 || plan.state_dim == 0 {
        return Err(PlanValidationError::SsmScanZeroDimension {
            length: plan.length,
            state_dim: plan.state_dim,
        });
    }
    Ok(())
}

/// Workgroup coupling (L12 / PRP-004 U2): tile-local addressing derives from
/// workgroup-local ids, so the shared-memory tile is only correct when the
/// workgroup equals the tile grid on both axes — the pre-fix `id % 8` aliased
/// shared slots for any other workgroup size. The one admitted exception is
/// the PGC-B2 T=1 one-row launch (see the check below): an M=1 plan may
/// carry workgroup `(tile, 1)` because its body never addresses a y tile.
/// A zero tile is degenerate (no
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
        // PGC-B2 T=1 one-row exception (B2-RETUNE width sweep): an M=1
        // plan (the decode GEMV / lm-head shapes) may carry workgroup
        // `(w, 1)` with `w` a tile multiple up to 64 — the one-row body
        // streams each output column independently (no shared tiles, no
        // barriers), so any admitted lane count is correct and the width
        // is an occupancy knob, not an addressing fact. A batched plan
        // (U1 head-axis) always keeps the cooperative-tile body, so the
        // exception stays rank-2-only.
        let one_row = plan.batch == 1
            && plan.m == 1
            && plan.workgroup_y == 1
            && plan.workgroup_x.is_multiple_of(plan.tile)
            && plan.workgroup_x <= 64;
        if !one_row {
            return Err(PlanValidationError::MatMulWorkgroupMismatch {
                workgroup_x: plan.workgroup_x,
                workgroup_y: plan.workgroup_y,
                tile: plan.tile,
            });
        }
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
    let cross_recipe_barriers = plan
        .barriers
        .iter()
        .filter(|point| matches!(point.after, BarrierPhase::ReductionStep))
        .count();
    if cross_recipe_barriers > 0 {
        // A reduction-only phase has no place in the matmul recipe; the
        // plan is a closed recipe set (L2), so a cross-recipe barrier is
        // a malformed plan.
        return Err(PlanValidationError::BarrierDominance {
            phase: BarrierPhase::ReductionStep,
            required_count: 0,
            present_count: u32::try_from(cross_recipe_barriers).unwrap_or(u32::MAX),
        });
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

/// Matmul zero-dimension guard: runtime extents have no literal numbers to
/// check; concrete plans must have positive M/K/N and a non-zero tile (a
/// zero dimension dispatches an empty grid).
fn validate_matmul_zero_dimension(plan: &MatMulPlan) -> Result<(), PlanValidationError> {
    // Run-time extents carry no literal numbers; the body's own guards
    // express the M, N and K bounds (design note section 5).
    if matches!(plan.dims, super::MatMulDims::Runtime { .. }) {
        return Ok(());
    }
    if plan.tile == 0 || plan.m == 0 || plan.k == 0 || plan.n == 0 {
        return Err(PlanValidationError::MatMulZeroDimension {
            m: plan.m,
            k: plan.k,
            n: plan.n,
            tile: plan.tile,
        });
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
    /// dispatches an empty grid).
    MatMulZeroDimension { m: u64, k: u64, n: u64, tile: u32 },
    /// U1 head-axis batch law: both batch counts positive and the argument
    /// batch count dividing the receiver's (the prefix share).
    MatMulBatchLaw { batch: u64, right_batch: u64 },
    /// U2 head-axis row-softmax batch law: positive batch/width and — when
    /// the additive mask operand is carried — a positive mask count that
    /// divides `batch · width`.
    RowSoftmaxBatchLaw {
        batch: u64,
        width: u64,
        mask_count: u64,
    },
    /// Transpose requires positive source dims (a zero `M` or `N` dispatches
    /// an empty grid and the column-major remap would divide by zero).
    TransposeZeroDimension { m: u64, n: u64 },
    /// Transpose requires a positive workgroup lane count.
    TransposeZeroWorkgroup { workgroup_x: u32 },
    /// The transpose dispatch extent must tile exactly `M·N` output
    /// elements (one thread per output element).
    TransposeDispatchMismatch { m: u64, n: u64, dispatch_x: u64 },
    /// U3 head-axis batched transpose law: the carried batch count must be
    /// positive and the dispatch extent must tile exactly `batch·M·N`
    /// output elements (the batch folds into the same 1D contract).
    TransposeBatchLaw {
        batch: u64,
        m: u64,
        n: u64,
        dispatch_x: u64,
    },
    /// The axis-reduction recipe's v1 closed op set is `Sum` only — a
    /// mean-over-axis is not part of the transformer surface.
    AxisReductionUnsupportedOp { op: ReduceOp },
    /// The carried reduced-resource projection (council-8 CB-3) must be
    /// positive in both facts — a zero axis extent or inner stride would
    /// make the producer-defined projection degenerate.
    AxisReductionZeroProjection { axis_extent: u64, inner_stride: u64 },
    /// The row-gather recipe requires positive table dims and a positive id
    /// count (a zero dimension dispatches an empty grid and the gather remap
    /// would be degenerate).
    GatherZeroDimension {
        table_rows: u64,
        table_cols: u64,
        id_count: u64,
    },
    /// The RMS-normalization recipe requires a positive normalization width
    /// (a zero width dispatches an empty row pass).
    RmsNormalizationZeroWidth { width: u64 },
    /// The `RoPE` recipe requires a positive rotated width and a positive row
    /// width (a zero width admits no rotation pair).
    RopeZeroDimension {
        pos: u64,
        dim: u64,
        width: u64,
        rows: u64,
    },
    /// The causal masked-softmax recipe requires positive row/column extents
    /// (a zero extent dispatches an empty grid).
    CausalMaskedSoftmaxZeroDimension { rows: u64, cols: u64 },
    /// Packed quantized GEMM requires positive M/K/N and a non-zero tile.
    QuantizedMatMulZeroDimension { m: u64, k: u64, n: u64, tile: u32 },
    /// F32 must fall back to `TiledMatMul` (R-PACK-02A); it is not a
    /// `QuantizedMatMul` layout.
    QuantizedMatMulF32Fallback,
    /// Grouped packed GEMM requires positive M/K/N/groups and a non-zero tile.
    GroupedMatMulZeroDimension {
        m: u64,
        k: u64,
        n: u64,
        groups: u64,
        tile: u32,
    },
    /// A batched attention contract has a zero axis or an invalid stride.
    BatchedAttentionInvalid,
    /// SSM conv1d requires positive length, channels, and kernel width.
    SsmConv1dZeroDimension {
        length: u64,
        channels: u64,
        kernel_width: u64,
    },
    /// SSM scan requires positive length and state dim.
    SsmScanZeroDimension { length: u64, state_dim: u64 },
    /// The filum lane width must equal the tree-floor workgroup lane count
    /// (SFR-5: one shared slot per lane).
    DistributedSumLaneWidthMismatch { lane_width: u32, workgroup_x: u32 },
    /// The filum source length `K` must be positive (SFR-5).
    DistributedSumZeroLength,
    /// A grid reduction needs at least one partial slot (C4-8).
    GridReduceZeroPartialsCap,
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
            Self::MatMulBatchLaw { batch, right_batch } => write!(
                f,
                "matmul batch law violated: batch={batch}, right_batch={right_batch} (both must be positive and right_batch must divide batch)"
            ),
            Self::RowSoftmaxBatchLaw {
                batch,
                width,
                mask_count,
            } => write!(
                f,
                "row-softmax batch law violated: batch={batch}, width={width}, mask_count={mask_count} (batch and width must be positive; a carried mask count must be positive and divide batch·width)"
            ),
            Self::TransposeZeroDimension { m, n } => write!(
                f,
                "transpose requires positive source dims, got M={m}, N={n}"
            ),
            Self::TransposeZeroWorkgroup { workgroup_x } => write!(
                f,
                "transpose requires a positive workgroup lane count, got {workgroup_x}"
            ),
            Self::TransposeDispatchMismatch { m, n, dispatch_x } => write!(
                f,
                "transpose dispatch extent {dispatch_x} must tile exactly {m}×{n} = {} output elements",
                m.saturating_mul(*n)
            ),
            Self::TransposeBatchLaw {
                batch,
                m,
                n,
                dispatch_x,
            } => write!(
                f,
                "batched transpose batch law violated: batch={batch}, m={m}, n={n}, dispatch_x={dispatch_x} (batch must be positive and dispatch_x must equal batch·M·N)"
            ),
            Self::AxisReductionUnsupportedOp { op } => {
                write!(f, "axis-reduction recipe v1 admits only Sum, got {op:?}")
            }
            Self::AxisReductionZeroProjection {
                axis_extent,
                inner_stride,
            } => write!(
                f,
                "axis-reduction reduced-resource projection must be positive, got axis extent {axis_extent}, inner stride {inner_stride}"
            ),
            Self::GatherZeroDimension {
                table_rows,
                table_cols,
                id_count,
            } => write!(
                f,
                "row-gather requires positive dims, got table {table_rows}×{table_cols}, id count {id_count}"
            ),
            Self::RmsNormalizationZeroWidth { width } => write!(
                f,
                "RMS-normalization requires a positive normalization width, got {width}"
            ),
            Self::RopeZeroDimension {
                pos,
                dim,
                width,
                rows,
            } => write!(
                f,
                "RoPE requires a positive rotated width, row width, and per-row count, got pos={pos}, dim={dim}, width={width}, rows={rows}"
            ),
            Self::CausalMaskedSoftmaxZeroDimension { rows, cols } => write!(
                f,
                "causal masked-softmax requires positive row/column extents, got rows={rows}, cols={cols}"
            ),
            Self::QuantizedMatMulZeroDimension { m, k, n, tile } => write!(
                f,
                "quantized matmul requires positive M/K/N and a non-zero tile, got M={m}, K={k}, N={n}, tile={tile}"
            ),
            Self::QuantizedMatMulF32Fallback => write!(
                f,
                "quantized matmul F32 layout must fall back to TiledMatMul"
            ),
            Self::GroupedMatMulZeroDimension {
                m,
                k,
                n,
                groups,
                tile,
            } => write!(
                f,
                "grouped matmul requires positive M/K/N/groups and a non-zero tile, got M={m}, K={k}, N={n}, groups={groups}, tile={tile}"
            ),
            Self::BatchedAttentionInvalid => write!(
                f,
                "batched attention requires positive axes, coherent strides, and a sequence-major context width"
            ),
            Self::SsmConv1dZeroDimension {
                length,
                channels,
                kernel_width,
            } => write!(
                f,
                "ssm conv1d requires positive length, channels, and kernel width, got length={length}, channels={channels}, kernel_width={kernel_width}"
            ),
            Self::SsmScanZeroDimension { length, state_dim } => write!(
                f,
                "ssm scan requires positive length and state dim, got length={length}, state_dim={state_dim}"
            ),
            Self::DistributedSumLaneWidthMismatch {
                lane_width,
                workgroup_x,
            } => write!(
                f,
                "distributed sum lane width {lane_width} must equal the tree-floor workgroup lane count {workgroup_x}"
            ),
            Self::DistributedSumZeroLength => {
                write!(f, "distributed sum requires a positive source length")
            }
            Self::GridReduceZeroPartialsCap => {
                write!(f, "grid reduction requires at least one partial slot")
            }
        }
    }
}

impl std::error::Error for PlanValidationError {}
