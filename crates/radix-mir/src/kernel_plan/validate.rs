use super::plan::{
    AxisReductionPlan, BarrierPhase, BarrierPoint, CausalMaskedSoftmaxPlan, GatherPlan, MatMulPlan,
    ReduceOp, ReductionPlan, RmsNormalizationPlan, RopePlan, SharedMemoryLayout, TransposePlan,
    MATMUL_SHARED_A_BUFFER_NAME, MATMUL_SHARED_B_BUFFER_NAME, WORKGROUP_SHARED_MEMORY_LIMIT_BYTES,
};
use std::fmt;

pub(super) fn validate_reduction_plan(plan: &ReductionPlan) -> Result<(), PlanValidationError> {
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

pub(super) fn validate_matmul_plan(plan: &MatMulPlan) -> Result<(), PlanValidationError> {
    validate_matmul_workgroup(plan)?;
    validate_matmul_shared_memory(plan)?;
    validate_matmul_barrier_coverage(plan)?;
    validate_matmul_oob_padding_coverage(plan)?;
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
    if plan.dispatch_x != plan.m.saturating_mul(plan.n) {
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
    /// Transpose requires positive source dims (a zero `M` or `N` dispatches
    /// an empty grid and the column-major remap would divide by zero).
    TransposeZeroDimension { m: u64, n: u64 },
    /// Transpose requires a positive workgroup lane count.
    TransposeZeroWorkgroup { workgroup_x: u32 },
    /// The transpose dispatch extent must tile exactly `M·N` output
    /// elements (one thread per output element).
    TransposeDispatchMismatch { m: u64, n: u64, dispatch_x: u64 },
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
            Self::TransposeZeroDimension { m, n } => write!(
                f,
                "transpose requires positive source dims, got M={m}, N={n}"
            ),
            Self::TransposeZeroWorkgroup { workgroup_x } => write!(
                f,
                "transpose requires a positive workgroup lane count, got {workgroup_x}"
            ),
            Self::TransposeDispatchMismatch {
                m,
                n,
                dispatch_x,
            } => write!(
                f,
                "transpose dispatch extent {dispatch_x} must tile exactly {m}×{n} = {} output elements",
                m.saturating_mul(*n)
            ),
            Self::AxisReductionUnsupportedOp { op } => write!(
                f,
                "axis-reduction recipe v1 admits only Sum, got {op:?}"
            ),
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
        }
    }
}

impl std::error::Error for PlanValidationError {}
