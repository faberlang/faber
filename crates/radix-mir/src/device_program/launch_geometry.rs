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
//! | `TiledMatMul` (rank-2, unbatched) | tile grid `(ceil(N/tile), ceil(M/tile))`, verbatim |
//! | `LaneGrid` | generic 1-D over the element count of the source resource's call-site shape |
//! | `Transpose` | `plan.workgroup_x` lanes over the input element count |
//! | every other plan | [`GeometryError`] `geometry_plan_not_covered` |

use crate::abi::{
    MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind,
    MirKernelResourceRole, MirKernelSignature, MirWorkgroupSize, workgroup_count_for_dispatch,
};
use crate::kernel_plan::CollectionKernelPlan;

use super::types::{GENERIC_1D_MAX_ELEMENTS, KernelUnit, generic_1d_workgroup};

/// Launch geometry of one call: grid (workgroup count) and block (threads per
/// workgroup), per axis. Target-neutral; consumers convert to their own type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallGeometry {
    pub grid: [u32; 3],
    pub block: [u32; 3],
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
            return Err(GeometryError::new(
                GEOMETRY_RANK_MISMATCH,
                vec![
                    ("resource", index.to_string()),
                    ("expected", declared.len().to_string()),
                    ("actual", shape.len().to_string()),
                ],
            ));
        }
        counts.push(element_count(index, shape)?);
    }
    match &unit.plan {
        CollectionKernelPlan::Elementwise => generic_1d(unit, output_count(unit, &counts)?),
        CollectionKernelPlan::RowSoftmax(softmax) if softmax.batched.is_none() => {
            generic_1d(unit, output_count(unit, &counts)?)
        }
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
                return Err(GeometryError::new(
                    GEOMETRY_RANK_MISMATCH,
                    vec![
                        ("resource", source_resource.to_string()),
                        ("expected", rank.to_string()),
                        ("actual", shape.len().to_string()),
                    ],
                ));
            }
            generic_1d(unit, counts[*source_resource])
        }
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
    // Every caller (Elementwise, unbatched RowSoftmax, LaneGrid) takes the
    // generic one-dimensional rule; `None` names that rule directly, so a
    // plan not admitted by `is_generic_1d_plan` (LaneGrid, ruling P2) still
    // routes here without widening the admission list.
    let workgroup = generic_1d_workgroup(&signature, None);
    finish(dispatch, workgroup)
}

/// Tile grid `(ceil(N/tile), ceil(M/tile))`, launched verbatim.
fn tiled_matmul(
    matmul: &crate::kernel_plan::MatMulPlan,
    shapes: &[Vec<u64>],
) -> Result<CallGeometry, GeometryError> {
    let uncovered = |reason: &str| {
        GeometryError::new(
            GEOMETRY_SHAPE_NOT_COVERED,
            vec![("reason", reason.to_owned())],
        )
    };
    if matmul.batch != 1 || matmul.right_batch != 1 {
        return Err(GeometryError::new(
            GEOMETRY_PLAN_NOT_COVERED,
            vec![("plan", "TiledMatMul(batched)".to_owned())],
        ));
    }
    let (Some(left), Some(right)) = (shapes.first(), shapes.get(1)) else {
        return Err(uncovered("matmul needs two operands"));
    };
    let (m, k) = match left.as_slice() {
        [m, k] => (*m, *k),
        _ => return Err(uncovered("matmul left operand is not rank 2")),
    };
    let (k_right, n) = match right.as_slice() {
        [k, n] => (*k, *n),
        [k] => (*k, 1),
        _ => return Err(uncovered("matmul right operand is not rank 1 or 2")),
    };
    if k != k_right {
        return Err(uncovered("matmul inner dimensions differ"));
    }
    let tile = u64::from(matmul.tile.max(1));
    let workgroup = MirWorkgroupSize {
        x: matmul.workgroup_x,
        y: matmul.workgroup_y,
        z: 1,
    };
    let grid = |extent: u64| {
        u32::try_from(extent.div_ceil(tile)).map_err(|_| {
            GeometryError::new(
                GEOMETRY_EXTENT_OVERFLOW,
                vec![("extent", extent.to_string())],
            )
        })
    };
    Ok(CallGeometry {
        grid: [grid(n)?, grid(m)?, 1],
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
    }
}

#[cfg(test)]
#[path = "launch_geometry_test.rs"]
mod tests;
