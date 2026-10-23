//! Launch geometry computed from a call site's runtime argument shapes.
//!
//! [`KernelLaunchPlan`] is derived at compile time from the static signature
//! plan. A runtime-extent tensor has no static shape, so a launch must derive
//! its geometry from the shapes of the actual call arguments.
//! [`geometry_for_call`] is that pure derivation. It owns no geometry rule of
//! its own: the generic one-dimensional workgroup policy is
//! [`generic_1d_workgroup`] (with its [`GENERIC_1D_MAX_ELEMENTS`] bound), the
//! per-axis grid is the shared `ceil(dispatch / workgroup)` rule, and a matmul
//! plan launches its tile grid verbatim (the rule
//! [`collection_op_workgroup_count`](crate::abi::collection_op_workgroup_count)
//! applies to matmul bodies). A missing fact fails closed with a
//! [`GeometryError`]; no plan is special-cased and nothing is guessed.
//!
//! Plan coverage:
//!
//! | plan | geometry |
//! | --- | --- |
//! | `Elementwise`, unbatched `RowSoftmax` | generic 1-D over the output element count |
//! | `Gather` | generic 1-D over the output element count |
//! | `TiledMatMul` | tile grid `(ceil(N/tile), batch · ceil(M/tile))`, verbatim; the batch is `1` for a rank-2 contraction |
//! | `LaneGrid` | generic 1-D over the element count of the source resource's call-site shape |
//! | `Transpose` | `plan.workgroup_x` lanes over the input element count |
//! | `GridReduce` | two stages ([`LaunchStages::Staged`], via [`launch_stages_for_call`]): `p = min(ceil(n / workgroup), partials_cap)` workgroups, then one |
//! | every other plan | [`GeometryError`] `geometry_plan_not_covered` |

use crate::abi::{
    MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind,
    MirKernelResourceRole, MirKernelSignature, MirWorkgroupSize, workgroup_count_for_dispatch,
};
use crate::kernel_plan::{CollectionKernelPlan, MatMulDims, matmul_class_for_extents};

use super::types::{GENERIC_1D_MAX_ELEMENTS, KernelUnit, generic_1d_workgroup};

/// Launch geometry of one call: grid (workgroup count) and block (threads per
/// workgroup), per axis. Target-neutral; consumers convert to their own type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallGeometry {
    pub grid: [u32; 3],
    pub block: [u32; 3],
}

/// The launches one call makes: one for every plan but a whole-grid
/// reduction, which makes two back to back (stage 1 on `<entry>`, stage 2 on
/// [`grid_reduce_combine_entry`]) through a dispatcher-owned partials scratch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchStages {
    /// One launch.
    Single(CallGeometry),
    /// A `GridReduce` call: stage 1 then stage 2, in this order.
    Staged([CallGeometry; 2]),
}

/// The entry-point name of a `GridReduce` kernel's stage 2, given the name of
/// its stage 1 entry. The one place the convention lives: the Metal emitter
/// names the second entry point with it and the dispatcher launches it.
#[must_use]
pub fn grid_reduce_combine_entry(entry: &str) -> String {
    format!("{entry}_combine")
}

/// A failed geometry derivation: a stable issue code plus named arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeometryError {
    /// Stable issue code (`geometry_*`).
    pub code: &'static str,
    /// Named arguments, in a stable order.
    pub args: Vec<(&'static str, String)>,
}

pub const GEOMETRY_SHAPE_COUNT: &str = "geometry_shape_count";
pub const GEOMETRY_RANK_MISMATCH: &str = "geometry_rank_mismatch";
pub const GEOMETRY_ZERO_EXTENT: &str = "geometry_zero_extent";
pub const GEOMETRY_EXTENT_OVERFLOW: &str = "geometry_extent_overflow";
pub const GEOMETRY_PLAN_NOT_COVERED: &str = "geometry_plan_not_covered";
pub const GEOMETRY_SHAPE_NOT_COVERED: &str = "geometry_shape_not_covered";

impl GeometryError {
    fn new(code: &'static str, args: Vec<(&'static str, String)>) -> Self {
        Self { code, args }
    }
}

/// Compute the launch geometry of `unit` for a call whose arguments have the
/// given runtime `shapes`, one per entry of `unit.resources` in resource order
/// (inputs, then outputs, exactly as the unit lists them).
///
/// # Errors
///
/// Returns a [`GeometryError`] when a shape does not match its resource or the
/// launch dimensions cannot be derived.
pub fn geometry_for_call(
    unit: &KernelUnit,
    shapes: &[Vec<u64>],
) -> Result<CallGeometry, GeometryError> {
    let counts = element_counts(unit, shapes)?;
    match &unit.plan {
        CollectionKernelPlan::Elementwise => generic_1d(unit, output_count(unit, &counts)?),
        // The gather body is one thread per output row element, so its launch
        // is the output element count from the call-site shape. The plan's own
        // `T`/`D` facts may be runtime and are not read here.
        CollectionKernelPlan::Gather(_) => generic_1d(unit, output_count(unit, &counts)?),
        CollectionKernelPlan::RowSoftmax(softmax) if softmax.batched.is_none() => {
            generic_1d(unit, output_count(unit, &counts)?)
        }
        CollectionKernelPlan::GridReduce { .. } => Err(GeometryError::new(
            GEOMETRY_PLAN_NOT_COVERED,
            vec![(
                "plan",
                "GridReduce (two stages: use launch_stages_for_call)".to_owned(),
            )],
        )),
        CollectionKernelPlan::LaneGrid {
            source_resource,
            rank,
        } => {
            // The lane domain is the call-site shape of the source resource,
            // `rank` axes, through the generic 1-D rule. Routed directly: the
            // kind is deliberately not in `is_generic_1d_plan` (ruling P2).
            let Some(shape) = shapes.get(*source_resource) else {
                return Err(GeometryError::new(
                    GEOMETRY_SHAPE_NOT_COVERED,
                    vec![("reason", "lane source resource out of range".to_owned())],
                ));
            };
            if shape.len() != usize::from(*rank) {
                return Err(rank_mismatch(
                    *source_resource,
                    usize::from(*rank),
                    shape.len(),
                ));
            }
            generic_1d(unit, counts[*source_resource])
        }
        // One thread, one group: the body is sequential. The call-site shapes
        // do not shape the launch.
        CollectionKernelPlan::SingleLane => finish(
            MirKernelDispatchSize { x: 1, y: 1, z: 1 },
            MirWorkgroupSize { x: 1, y: 1, z: 1 },
        ),
        CollectionKernelPlan::TiledMatMul(matmul) => tiled_matmul(matmul, shapes),
        CollectionKernelPlan::Transpose(transpose) => {
            let input = first_input(unit, &counts)?;
            let workgroup = MirWorkgroupSize {
                x: transpose.workgroup_x,
                y: 1,
                z: 1,
            };
            finish(
                MirKernelDispatchSize {
                    x: input,
                    y: 1,
                    z: 1,
                },
                workgroup,
            )
        }
        other => Err(GeometryError::new(
            GEOMETRY_PLAN_NOT_COVERED,
            vec![("plan", plan_name(other).to_owned())],
        )),
    }
}

/// Per-resource element counts of a call, after the shape-count, rank and
/// zero-extent checks every plan shares.
fn element_counts(unit: &KernelUnit, shapes: &[Vec<u64>]) -> Result<Vec<u64>, GeometryError> {
    if shapes.len() != unit.resources.len() {
        return Err(GeometryError::new(
            GEOMETRY_SHAPE_COUNT,
            vec![
                ("expected", unit.resources.len().to_string()),
                ("actual", shapes.len().to_string()),
            ],
        ));
    }
    let mut counts = Vec::with_capacity(shapes.len());
    for (index, (resource, shape)) in unit.resources.iter().zip(shapes).enumerate() {
        if let Some(declared) = &resource.version.shape
            && declared.len() != shape.len()
        {
            return Err(rank_mismatch(index, declared.len(), shape.len()));
        }
        counts.push(element_count(index, shape)?);
    }
    Ok(counts)
}

/// Compute every launch of a call: [`geometry_for_call`]'s single launch for
/// each plan but `GridReduce`, whose two stages are
/// `p = min(ceil(n / workgroup), partials_cap)` workgroups of `workgroup`
/// lanes, then one workgroup of `workgroup` lanes. `n` is the element count of
/// the reduced source's call-site shape; a zero extent is refused as for every
/// plan.
///
/// # Errors
///
/// Returns a [`GeometryError`] when a shape does not match its resource or the
/// launch dimensions cannot be derived.
pub fn launch_stages_for_call(
    unit: &KernelUnit,
    shapes: &[Vec<u64>],
) -> Result<LaunchStages, GeometryError> {
    let CollectionKernelPlan::GridReduce {
        source_resource,
        rank,
        workgroup,
        partials_cap,
        ..
    } = &unit.plan
    else {
        return geometry_for_call(unit, shapes).map(LaunchStages::Single);
    };
    let counts = element_counts(unit, shapes)?;
    let uncovered = |reason: &str| {
        GeometryError::new(
            GEOMETRY_SHAPE_NOT_COVERED,
            vec![("reason", reason.to_owned())],
        )
    };
    let Some(shape) = shapes.get(*source_resource) else {
        return Err(uncovered("reduce source resource out of range"));
    };
    if shape.len() != usize::from(*rank) {
        return Err(rank_mismatch(
            *source_resource,
            usize::from(*rank),
            shape.len(),
        ));
    }
    let n = counts[*source_resource];
    if n > i32::MAX as u64 {
        return Err(GeometryError::new(
            GEOMETRY_SHAPE_NOT_COVERED,
            vec![
                ("reason", "elements exceed 32-bit index".to_owned()),
                ("elements", n.to_string()),
            ],
        ));
    }
    let lanes = u64::from(*workgroup);
    if lanes == 0 {
        return Err(uncovered("zero workgroup"));
    }
    let partials = n.div_ceil(lanes).min(u64::from(*partials_cap));
    let block = MirWorkgroupSize {
        x: *workgroup,
        y: 1,
        z: 1,
    };
    let stage1 = finish(
        MirKernelDispatchSize {
            x: partials * lanes,
            y: 1,
            z: 1,
        },
        block,
    )?;
    let stage2 = finish(
        MirKernelDispatchSize {
            x: lanes,
            y: 1,
            z: 1,
        },
        block,
    )?;
    Ok(LaunchStages::Staged([stage1, stage2]))
}

fn element_count(resource: usize, shape: &[u64]) -> Result<u64, GeometryError> {
    let mut product: u64 = 1;
    for &extent in shape {
        if extent == 0 {
            return Err(GeometryError::new(
                GEOMETRY_ZERO_EXTENT,
                vec![("resource", resource.to_string())],
            ));
        }
        product = product.checked_mul(extent).ok_or_else(|| {
            GeometryError::new(
                GEOMETRY_EXTENT_OVERFLOW,
                vec![("resource", resource.to_string())],
            )
        })?;
    }
    Ok(product)
}

/// The shared rank-mismatch error: `resource`'s shape has `actual` axes where
/// `expected` were declared.
fn rank_mismatch(resource: usize, expected: usize, actual: usize) -> GeometryError {
    GeometryError::new(
        GEOMETRY_RANK_MISMATCH,
        vec![
            ("resource", resource.to_string()),
            ("expected", expected.to_string()),
            ("actual", actual.to_string()),
        ],
    )
}

fn output_count(unit: &KernelUnit, counts: &[u64]) -> Result<u64, GeometryError> {
    unit.resources
        .iter()
        .position(|resource| resource.access != MirKernelResourceAccess::Read)
        .map(|index| counts[index])
        .ok_or_else(|| {
            GeometryError::new(
                GEOMETRY_SHAPE_NOT_COVERED,
                vec![("reason", "no written resource".to_owned())],
            )
        })
}

fn first_input(unit: &KernelUnit, counts: &[u64]) -> Result<u64, GeometryError> {
    unit.resources
        .iter()
        .position(|resource| resource.access == MirKernelResourceAccess::Read)
        .map(|index| counts[index])
        .ok_or_else(|| {
            GeometryError::new(
                GEOMETRY_SHAPE_NOT_COVERED,
                vec![("reason", "no read resource".to_owned())],
            )
        })
}

/// Generic 1-D launch over `elements` threads: the shared workgroup policy,
/// refused above the 32-bit index bound the generated bodies address with.
fn generic_1d(unit: &KernelUnit, elements: u64) -> Result<CallGeometry, GeometryError> {
    if elements > i32::MAX as u64 {
        return Err(GeometryError::new(
            GEOMETRY_SHAPE_NOT_COVERED,
            vec![
                ("reason", "elements exceed 32-bit index".to_owned()),
                ("elements", elements.to_string()),
                ("max", GENERIC_1D_MAX_ELEMENTS.to_string()),
            ],
        ));
    }
    let dispatch = MirKernelDispatchSize {
        x: elements,
        y: 1,
        z: 1,
    };
    let signature = default_signature(unit, dispatch);
    // A thread-per-output recipe the wide-workgroup policy has measured
    // (`Elementwise`, unbatched `RowSoftmax`) widens its block; an unmeasured
    // recipe (`Gather`) keeps the signature workgroup. `LaneGrid` is not in
    // `is_generic_1d_plan` yet widens by ruling P2, so it keeps naming the
    // rule directly with `None`.
    let plan = match &unit.plan {
        CollectionKernelPlan::LaneGrid { .. } => None,
        plan => Some(plan),
    };
    let workgroup = generic_1d_workgroup(&signature, plan);
    finish(dispatch, workgroup)
}

/// The matmul plan a call launches. A literal plan is launched as built. A
/// run-time-extent plan (design note item 7) is the one `Tiled` plan
/// `from_contract` built, cloned with its class swapped to the one the
/// call-site shapes select, [`matmul_class_for_extents`]: the same selection
/// the literal path makes for the same numbers. The K == 1 outer update stays
/// literal-only and is refused for run-time extents.
///
/// # Errors
///
/// Returns a [`GeometryError`] when the operand shapes are not a rank 2..=4
/// left with a rank 2..=left-rank (or rank-1, rank-2 left) right and agreeing
/// inner dimensions, or the shapes select a literal-only body.
pub fn matmul_launch_plan(
    matmul: &crate::kernel_plan::MatMulPlan,
    shapes: &[Vec<u64>],
) -> Result<crate::kernel_plan::MatMulPlan, GeometryError> {
    let extents = matmul_extents(shapes)?;
    let k1_refusal = || {
        GeometryError::new(
            GEOMETRY_SHAPE_NOT_COVERED,
            vec![(
                "reason",
                "matmul with run-time extents does not cover the K == 1 outer update (literal-only)"
                    .to_owned(),
            )],
        )
    };
    let mut plan = matmul.clone();
    if let MatMulDims::Runtime { .. } = plan.dims {
        // A batched run-time call always launches the batched Tiled body: the
        // OneRow, Gemv and Simdgroup bodies are rank-2-only. The K == 1 outer
        // update stays literal-only for both forms.
        if extents.batch > 1 {
            if extents.k == 1 {
                return Err(k1_refusal());
            }
            plan.dims = MatMulDims::Runtime {
                class: crate::kernel_plan::MatMulClass::Tiled,
            };
            return Ok(plan);
        }
        let Some(class) = matmul_class_for_extents(extents.m, extents.k, extents.n) else {
            return Err(k1_refusal());
        };
        plan.dims = MatMulDims::Runtime { class };
    }
    Ok(plan)
}

/// The classes a run-time-extent matmul unit emits an entry for, Tiled first.
pub const MATMUL_CLASSES: [crate::kernel_plan::MatMulClass; 4] = {
    use crate::kernel_plan::MatMulClass;
    [
        MatMulClass::Tiled,
        MatMulClass::OneRow,
        MatMulClass::Gemv,
        MatMulClass::Simdgroup,
    ]
};

/// The device-artifact entry symbol of `class` for a run-time-extent matmul
/// unit named `entry` (design note section 5 rule 1: one entry per class).
/// The Tiled class keeps the unit's own entry; the other three are
/// class-suffixed siblings. The emitter names its class entries with this
/// function and the dispatcher launches the one [`matmul_launch_plan`]
/// selects, so the two cannot disagree.
#[must_use]
pub fn matmul_class_entry(entry: &str, class: crate::kernel_plan::MatMulClass) -> String {
    use crate::kernel_plan::MatMulClass;
    match class {
        MatMulClass::Tiled => entry.to_owned(),
        MatMulClass::OneRow => format!("{entry}__one_row"),
        MatMulClass::Gemv => format!("{entry}__gemv"),
        MatMulClass::Simdgroup => format!("{entry}__simdgroup"),
    }
}

/// The extents of one matmul call: the contraction `(m, k, n)` plus the batch
/// facts the launch folds into `dispatch_y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MatMulExtents {
    m: u64,
    k: u64,
    n: u64,
    /// Product of the left operand's leading axes, row-major; `1` for a rank-2
    /// contraction.
    batch: u64,
    /// Product of the right operand's leading axes, row-major. The right batch
    /// axes are an elementwise prefix of the left's (the GQA share); `1` for a
    /// rank-2 right.
    right_batch: u64,
}

/// The extents and batch facts of the two operand call-site shapes: the left
/// operand is rank 2..=4 (`l_batch, m, k`), the right one rank 1 (only with a
/// rank-2 left) or rank 2..=left-rank (`r_batch, k, n`).
fn matmul_extents(shapes: &[Vec<u64>]) -> Result<MatMulExtents, GeometryError> {
    let uncovered = |reason: &str| {
        GeometryError::new(
            GEOMETRY_SHAPE_NOT_COVERED,
            vec![("reason", reason.to_owned())],
        )
    };
    let product = |axes: &[u64]| {
        axes.iter()
            .try_fold(1_u64, |acc, &extent| acc.checked_mul(extent))
    };
    let (Some(left), Some(right)) = (shapes.first(), shapes.get(1)) else {
        return Err(uncovered("matmul needs two operands"));
    };
    if !(2..=4).contains(&left.len()) {
        return Err(uncovered("matmul left operand is not rank 2 to 4"));
    }
    let (l_batch, rows) = left.split_at(left.len() - 2);
    let (m, k) = (rows[0], rows[1]);
    let (r_batch, k_right, n): (&[u64], u64, u64) = match right.as_slice() {
        [k_right] if left.len() == 2 => (&[], *k_right, 1),
        _ if (2..=left.len()).contains(&right.len()) => {
            let (r_batch, tail) = right.split_at(right.len() - 2);
            (r_batch, tail[0], tail[1])
        }
        _ => {
            return Err(uncovered(
                "matmul right operand is not rank 1 or 2 to the left's rank",
            ));
        }
    };
    if k != k_right {
        return Err(uncovered("matmul inner dimensions differ"));
    }
    let batch = product(l_batch).ok_or_else(|| uncovered("matmul batch overflows 64 bits"))?;
    let right_batch =
        product(r_batch).ok_or_else(|| uncovered("matmul batch overflows 64 bits"))?;
    if batch == 0 || right_batch == 0 || m == 0 || k == 0 || n == 0 {
        return Err(uncovered("matmul extents are zero"));
    }
    if !l_batch.starts_with(r_batch) {
        return Err(uncovered("matmul batch axes are not a prefix"));
    }
    if !batch.is_multiple_of(right_batch) {
        return Err(uncovered("matmul right batch does not divide the batch"));
    }
    Ok(MatMulExtents {
        m,
        k,
        n,
        batch,
        right_batch,
    })
}

/// Tile grid `(ceil(N/tile), batch · ceil(M/tile))`, launched verbatim: one
/// padded row-block per flattened batch index.
fn tiled_matmul(
    matmul: &crate::kernel_plan::MatMulPlan,
    shapes: &[Vec<u64>],
) -> Result<CallGeometry, GeometryError> {
    let plan = matmul_launch_plan(matmul, shapes)?;
    let extents = matmul_extents(shapes)?;
    // A literal plan carries its batch facts; the call-site shapes must agree
    // with them (a run-time plan has no batch of its own to compare against).
    if matches!(plan.dims, MatMulDims::Literal { .. })
        && (extents.batch != matmul.batch || extents.right_batch != matmul.right_batch)
    {
        return Err(GeometryError::new(
            GEOMETRY_SHAPE_NOT_COVERED,
            vec![(
                "reason",
                "matmul batch disagrees with the literal plan".to_owned(),
            )],
        ));
    }
    let tile = u64::from(plan.tile.max(1));
    let workgroup = MirWorkgroupSize {
        x: plan.workgroup_x,
        y: plan.workgroup_y,
        z: 1,
    };
    let narrow = |count: u64| {
        u32::try_from(count).map_err(|_| {
            GeometryError::new(
                GEOMETRY_EXTENT_OVERFLOW,
                vec![("extent", count.to_string())],
            )
        })
    };
    let row_blocks = extents
        .batch
        .checked_mul(extents.m.div_ceil(tile))
        .ok_or_else(|| {
            GeometryError::new(
                GEOMETRY_EXTENT_OVERFLOW,
                vec![("extent", extents.batch.to_string())],
            )
        })?;
    Ok(CallGeometry {
        grid: [narrow(extents.n.div_ceil(tile))?, narrow(row_blocks)?, 1],
        block: [workgroup.x, workgroup.y, workgroup.z],
    })
}

/// Derive the grid from `dispatch` and `workgroup` by the shared
/// `ceil(dispatch / workgroup)` rule and narrow it to 32 bits.
fn finish(
    dispatch: MirKernelDispatchSize,
    workgroup: MirWorkgroupSize,
) -> Result<CallGeometry, GeometryError> {
    let count = workgroup_count_for_dispatch(dispatch, workgroup).map_err(|_| {
        GeometryError::new(
            GEOMETRY_SHAPE_NOT_COVERED,
            vec![("reason", "zero workgroup".to_owned())],
        )
    })?;
    let narrow = |value: u64| {
        u32::try_from(value).map_err(|_| {
            GeometryError::new(GEOMETRY_EXTENT_OVERFLOW, vec![("grid", value.to_string())])
        })
    };
    Ok(CallGeometry {
        grid: [narrow(count.x)?, narrow(count.y)?, narrow(count.z)?],
        block: [workgroup.x, workgroup.y, workgroup.z],
    })
}

/// A signature carrying only what [`generic_1d_workgroup`] reads: the ABI
/// default `1x1x1` workgroup and the call's dispatch.
fn default_signature(
    unit: &KernelUnit,
    dispatch_size: MirKernelDispatchSize,
) -> MirKernelSignature {
    let resource = |binding: u32, role: MirKernelResourceRole, access| MirKernelResource {
        group: 0,
        binding,
        kind: MirKernelResourceKind::StorageBuffer,
        role,
        access,
        element_ty: unit.resources[0].version.element_ty,
        element_count: dispatch_size.x,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    };
    MirKernelSignature {
        function: unit.function,
        input: resource(
            0,
            MirKernelResourceRole::Input,
            MirKernelResourceAccess::Read,
        ),
        extra_inputs: Vec::new(),
        output: resource(
            1,
            MirKernelResourceRole::Output,
            MirKernelResourceAccess::Write,
        ),
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size,
    }
}

fn plan_name(plan: &CollectionKernelPlan) -> &'static str {
    match plan {
        CollectionKernelPlan::Elementwise => "Elementwise",
        CollectionKernelPlan::TiledMatMul(_) => "TiledMatMul",
        CollectionKernelPlan::TreeReduction(_) => "TreeReduction",
        CollectionKernelPlan::Transpose(_) => "Transpose",
        CollectionKernelPlan::AxisReduction(_) => "AxisReduction",
        CollectionKernelPlan::RowSoftmax(_) => "RowSoftmax(batched)",
        CollectionKernelPlan::LayerNormalization(_) => "LayerNormalization",
        CollectionKernelPlan::Gather(_) => "Gather",
        CollectionKernelPlan::RmsNormalization(_) => "RmsNormalization",
        CollectionKernelPlan::Rope(_) => "Rope",
        CollectionKernelPlan::CausalMaskedSoftmax(_) => "CausalMaskedSoftmax",
        CollectionKernelPlan::QuantizedMatMul(_) => "QuantizedMatMul",
        CollectionKernelPlan::GroupedMatMul(_) => "GroupedMatMul",
        CollectionKernelPlan::BatchedAttention(_) => "BatchedAttention",
        CollectionKernelPlan::SsmConv1d(_) => "SsmConv1d",
        CollectionKernelPlan::SsmScan(_) => "SsmScan",
        CollectionKernelPlan::DistributedSum(_) => "DistributedSum",
        CollectionKernelPlan::LaneGrid { .. } => "LaneGrid",
        CollectionKernelPlan::SingleLane => "SingleLane",
        CollectionKernelPlan::GridReduce { .. } => "GridReduce",
    }
}

#[cfg(test)]
#[path = "launch_geometry_test.rs"]
mod tests;
