// Fixture casts are small known-in-range numbers.
#![allow(clippy::cast_possible_truncation)]
use super::*;
use crate::abi::{MirKernelResourceAccess, MirKernelWorkgroupCount, MirWorkgroupSize};
use crate::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceResource,
    KernelLaunchPlan,
};
use crate::kernel_plan::{
    CollectionKernelPlan, F32_ELEMENT_BYTE_WIDTH, MATMUL_SHARED_A_BUFFER_NAME,
    MATMUL_SHARED_B_BUFFER_NAME, MATMUL_TILE, MatMulPlan, MatMulSharedMemory, OobPaddingPolicy,
    ReduceOp, ReductionPlan, SharedMemoryLayout, TRANSPOSE_WORKGROUP_X, TransposePlan,
};
use crate::layout::MirTensorStorageLayout;
use crate::{MirFunctionId, MirScalarLayout, MirType};
use radix_types::{NumericWidth, Primitive, TypeTable};

fn resource(
    index: u32,
    access: MirKernelResourceAccess,
    shape: Option<Vec<u64>>,
) -> DeviceResource {
    let mut types = TypeTable::new();
    let element_ty = MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32));
    DeviceResource {
        buffer: BufferIdentity {
            id: BufferId(index),
            name: format!("b{index}"),
            role: if access == MirKernelResourceAccess::Read {
                BufferRole::Input
            } else {
                BufferRole::Output
            },
            storage: MirTensorStorageLayout::DeviceHandle,
            lifetime: BufferLifetime::PerProgram,
        },
        version: BufferVersion {
            version: 1,
            element_ty,
            element_count: 0,
            shape,
            reduced_projection: None,
        },
        binding: Binding {
            group: 0,
            binding: index,
        },
        access,
    }
}

fn unit(plan: CollectionKernelPlan, resources: Vec<DeviceResource>) -> KernelUnit {
    KernelUnit {
        function: MirFunctionId(0),
        entry: "k".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan,
        resources,
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        },
    }
}

fn elementwise() -> KernelUnit {
    use MirKernelResourceAccess::{Read, Write};
    unit(
        CollectionKernelPlan::Elementwise,
        vec![
            resource(0, Read, None),
            resource(1, Read, None),
            resource(2, Write, None),
        ],
    )
}

fn matmul() -> KernelUnit {
    use MirKernelResourceAccess::{Read, Write};
    let tile_slots = MATMUL_TILE * MATMUL_TILE;
    let slots = |name: &str| SharedMemoryLayout {
        element_byte_width: F32_ELEMENT_BYTE_WIDTH,
        slot_count: tile_slots,
        buffer_name: name.to_owned(),
    };
    let plan = CollectionKernelPlan::TiledMatMul(MatMulPlan {
        dims: crate::kernel_plan::MatMulDims::Literal { m: 0, k: 0, n: 0 },
        m: 0,
        k: 0,
        n: 0,
        left_layout: MirScalarLayout::F32,
        right_layout: MirScalarLayout::F32,
        tile: MATMUL_TILE,
        workgroup_x: MATMUL_TILE,
        workgroup_y: MATMUL_TILE,
        shared_memory: MatMulSharedMemory {
            shared_a: slots(MATMUL_SHARED_A_BUFFER_NAME),
            shared_b: slots(MATMUL_SHARED_B_BUFFER_NAME),
        },
        barriers: Vec::new(),
        oob_padding: OobPaddingPolicy::ZeroFill,
        batch: 1,
        right_batch: 1,
    });
    unit(
        plan,
        vec![
            resource(0, Read, Some(vec![0, 0])),
            resource(1, Read, Some(vec![0, 0])),
            resource(2, Write, Some(vec![0, 0])),
        ],
    )
}

fn error(code: &'static str, args: &[(&'static str, &str)]) -> GeometryError {
    GeometryError {
        code,
        args: args.iter().map(|(k, v)| (*k, (*v).to_owned())).collect(),
    }
}

fn flat(n: u64) -> Vec<Vec<u64>> {
    vec![vec![n]; 3]
}

#[test]
fn generic_1d_geometry_table() {
    let wg = crate::device_program::GENERIC_1D_WORKGROUP_X;
    let max = GENERIC_1D_MAX_ELEMENTS;
    // (elements, grid x, block x)
    let table: [(u64, u32, u32); 6] = [
        (1, 1, 1),
        (2, 1, wg),
        (u64::from(wg), 1, wg),
        (u64::from(wg) + 1, 2, wg),
        (1000, 8, wg),
        // The bound itself still gets the wide workgroup.
        (max, max.div_ceil(u64::from(wg)) as u32, wg),
    ];
    for (elements, grid_x, block_x) in table {
        let got = geometry_for_call(&elementwise(), &flat(elements)).expect("covered");
        assert_eq!(
            got,
            CallGeometry {
                grid: [grid_x, 1, 1],
                block: [block_x, 1, 1]
            },
            "elements {elements}"
        );
    }
}

#[test]
fn generic_1d_above_bound_keeps_signature_workgroup_until_index_limit() {
    // max + 1 loses the wide workgroup (existing rule): one thread per block.
    let got = geometry_for_call(&elementwise(), &flat(GENERIC_1D_MAX_ELEMENTS + 1)).expect("ok");
    assert_eq!(
        got,
        CallGeometry {
            grid: [(GENERIC_1D_MAX_ELEMENTS + 1) as u32, 1, 1],
            block: [1, 1, 1]
        }
    );
    // Past the 32-bit index the launch is refused.
    let beyond = i32::MAX as u64 + 1;
    assert_eq!(
        geometry_for_call(&elementwise(), &flat(beyond)),
        Err(error(
            GEOMETRY_SHAPE_NOT_COVERED,
            &[
                ("reason", "elements exceed 32-bit index"),
                ("elements", &beyond.to_string()),
                ("max", &GENERIC_1D_MAX_ELEMENTS.to_string()),
            ]
        ))
    );
}

#[test]
fn tiled_matmul_geometry_is_the_verbatim_tile_grid() {
    // [10,16] x [16,20] -> [10,20]: grid (ceil(20/8), ceil(10/8)).
    let shapes = vec![vec![10, 16], vec![16, 20], vec![10, 20]];
    assert_eq!(
        geometry_for_call(&matmul(), &shapes),
        Ok(CallGeometry {
            grid: [3, 2, 1],
            block: [8, 8, 1]
        })
    );
    // A different runtime extent through the same unit gives a different grid.
    let shapes = vec![vec![64, 8], vec![8, 64], vec![64, 64]];
    assert_eq!(
        geometry_for_call(&matmul(), &shapes),
        Ok(CallGeometry {
            grid: [8, 8, 1],
            block: [8, 8, 1]
        })
    );
    // Inner dimensions that disagree are not covered.
    let shapes = vec![vec![8, 8], vec![9, 8], vec![8, 8]];
    assert_eq!(
        geometry_for_call(&matmul(), &shapes),
        Err(error(
            GEOMETRY_SHAPE_NOT_COVERED,
            &[("reason", "matmul inner dimensions differ")]
        ))
    );
}

#[test]
fn transpose_geometry_uses_plan_lanes_over_input_elements() {
    use MirKernelResourceAccess::{Read, Write};
    let plan = CollectionKernelPlan::Transpose(TransposePlan {
        m: 0,
        n: 0,
        workgroup_x: TRANSPOSE_WORKGROUP_X,
        dispatch_x: 0,
        batched: None,
    });
    let unit = unit(
        plan,
        vec![resource(0, Read, None), resource(1, Write, None)],
    );
    assert_eq!(
        geometry_for_call(&unit, &[vec![5, 40], vec![40, 5]]),
        Ok(CallGeometry {
            grid: [4, 1, 1],
            block: [TRANSPOSE_WORKGROUP_X, 1, 1]
        })
    );
}

#[test]
fn shape_count_mismatch_is_an_error() {
    assert_eq!(
        geometry_for_call(&elementwise(), &[vec![4], vec![4]]),
        Err(error(
            GEOMETRY_SHAPE_COUNT,
            &[("expected", "3"), ("actual", "2")]
        ))
    );
}

#[test]
fn rank_mismatch_is_an_error() {
    let shapes = vec![vec![4, 4], vec![4, 4], vec![4]];
    assert_eq!(
        geometry_for_call(&matmul(), &shapes),
        Err(error(
            GEOMETRY_RANK_MISMATCH,
            &[("resource", "2"), ("expected", "2"), ("actual", "1")]
        ))
    );
}

#[test]
fn zero_extent_is_an_error() {
    let shapes = vec![vec![4], vec![4], vec![0]];
    assert_eq!(
        geometry_for_call(&elementwise(), &shapes),
        Err(error(GEOMETRY_ZERO_EXTENT, &[("resource", "2")]))
    );
}

#[test]
fn extent_product_overflow_is_an_error() {
    let shapes = vec![vec![4], vec![4], vec![u64::MAX, 2]];
    assert_eq!(
        geometry_for_call(&elementwise(), &shapes),
        Err(error(GEOMETRY_EXTENT_OVERFLOW, &[("resource", "2")]))
    );
}

#[test]
fn uncovered_plan_is_a_named_error() {
    use MirKernelResourceAccess::{Read, Write};
    let plan =
        CollectionKernelPlan::LayerNormalization(crate::kernel_plan::LayerNormalizationPlan {
            dims: crate::kernel_plan::RowOpDims::Literal { rows: 1, width: 4 },
            axis: 0,
            epsilon_bits: 0,
        });
    let unit = unit(
        plan,
        vec![resource(0, Read, None), resource(1, Write, None)],
    );
    assert_eq!(
        geometry_for_call(&unit, &[vec![4], vec![4]]),
        Err(error(
            GEOMETRY_PLAN_NOT_COVERED,
            &[("plan", "LayerNormalization")]
        ))
    );
}

#[test]
fn gather_geometry_uses_the_output_element_count() {
    use MirKernelResourceAccess::{Read, Write};
    // The plan carries no literal extents: the launch is derived from the
    // call-site output shape `[5, 4]` (20 elements), not from `T` or `D`.
    let plan = CollectionKernelPlan::Gather(crate::kernel_plan::GatherPlan {
        table_rows: crate::kernel_plan::PlanDim::Runtime,
        table_cols: crate::kernel_plan::PlanDim::Runtime,
        id_count: crate::kernel_plan::PlanDim::Runtime,
    });
    let unit = unit(
        plan,
        vec![
            resource(0, Read, None),
            resource(1, Read, None),
            resource(2, Write, None),
        ],
    );
    let got = geometry_for_call(&unit, &[vec![4, 4], vec![5], vec![5, 4]])
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        got,
        CallGeometry {
            grid: [20, 1, 1],
            block: [1, 1, 1],
        }
    );
}

fn lane_grid(rank: u8) -> KernelUnit {
    use MirKernelResourceAccess::{Read, Write};
    unit(
        CollectionKernelPlan::LaneGrid {
            source_resource: 0,
            rank,
        },
        vec![resource(0, Read, None), resource(1, Write, None)],
    )
}

#[test]
fn lane_grid_geometry_is_generic_1d_over_the_source_shape() {
    let wg = crate::device_program::GENERIC_1D_WORKGROUP_X;
    // (shape, grid x, block x): [7,5] is 35 lanes; [1,1] is one lane; one past
    // a workgroup multiple rounds the grid up.
    let single_row = u64::from(wg) + 1;
    let table: [(Vec<u64>, u32, u32); 3] = [
        (vec![7, 5], 1, wg),
        (vec![1, 1], 1, 1),
        (vec![single_row, 1], 2, wg),
    ];
    for (shape, grid_x, block_x) in table {
        let got = geometry_for_call(&lane_grid(2), &[shape.clone(), shape.clone()])
            .unwrap_or_else(|error| panic!("{shape:?}: {error:?}"));
        assert_eq!(
            got,
            CallGeometry {
                grid: [grid_x, 1, 1],
                block: [block_x, 1, 1]
            },
            "{shape:?}"
        );
    }
}

#[test]
fn rank1_singleton_lane_domain_dispatches_exactly_one_lane() {
    let got = geometry_for_call(&lane_grid(1), &[vec![1], vec![1]]).expect("one-lane shape");
    assert_eq!(
        got,
        CallGeometry {
            grid: [1, 1, 1],
            block: [1, 1, 1],
        }
    );
}

#[test]
fn lane_grid_geometry_names_its_failures() {
    let unit = lane_grid(2);
    let ok = vec![7, 5];
    assert_eq!(
        geometry_for_call(&unit, &[vec![7, 0], ok.clone()]),
        Err(error(GEOMETRY_ZERO_EXTENT, &[("resource", "0")]))
    );
    assert_eq!(
        geometry_for_call(&unit, &[vec![35], ok.clone()]),
        Err(error(
            GEOMETRY_RANK_MISMATCH,
            &[("resource", "0"), ("expected", "2"), ("actual", "1")]
        ))
    );
    assert_eq!(
        geometry_for_call(&unit, &[vec![u64::MAX, 2], ok.clone()]),
        Err(error(GEOMETRY_EXTENT_OVERFLOW, &[("resource", "0")]))
    );
    // Above the 32-bit index bound the generic rule refuses.
    let too_many = geometry_for_call(&unit, &[vec![1 << 20, 1 << 12], ok]).unwrap_err();
    assert_eq!(too_many.code, GEOMETRY_SHAPE_NOT_COVERED);
}

#[test]
fn lane_grid_is_not_admitted_to_the_generic_1d_list() {
    assert!(!crate::device_program::is_generic_1d_plan(
        &CollectionKernelPlan::LaneGrid {
            source_resource: 0,
            rank: 2
        }
    ));
    assert!(crate::device_program::is_generic_1d_plan(
        &CollectionKernelPlan::Elementwise
    ));
}

fn grid_reduce_unit() -> KernelUnit {
    use MirKernelResourceAccess::{Read, Write};
    unit(
        CollectionKernelPlan::GridReduce {
            source_resource: 0,
            rank: 1,
            group_shape: None,
            workgroup: 256,
            partials_cap: 1024,
            partial_layout: crate::kernel_plan::GridReducePartialLayout::for_op(
                crate::MirReduceOp::Max,
                4,
                1,
            ),
        },
        vec![resource(0, Read, None), resource(1, Write, None)],
    )
}

fn stages(n: u64) -> [CallGeometry; 2] {
    let unit = grid_reduce_unit();
    match launch_stages_for_call(&unit, &[vec![n], vec![24]]).expect("stages") {
        LaunchStages::Staged(stages) => stages,
        LaunchStages::Single(_) | LaunchStages::Scan(_) | LaunchStages::Empty => {
            panic!("a GridReduce call is two stages")
        }
    }
}

#[test]
fn grid_reduce_stage_grids_at_the_boundary_sizes() {
    for (n, p) in [
        (1, 1),
        (255, 1),
        (256, 1),
        (257, 2),
        (262_144, 1024),
        (262_145, 1024),
        (1_388_964, 1024),
    ] {
        let [first, second] = stages(n);
        assert_eq!(
            first,
            CallGeometry {
                grid: [p, 1, 1],
                block: [256, 1, 1]
            },
            "stage 1 at n = {n}"
        );
        assert_eq!(
            second,
            CallGeometry {
                grid: [1, 1, 1],
                block: [256, 1, 1]
            },
            "stage 2 at n = {n}"
        );
    }
}

#[test]
fn grid_reduce_n_is_the_source_tensor_not_the_destination() {
    // A [600, 500] source reduced into a 24-element destination.
    let unit = grid_reduce_unit();
    let rank2 = match unit.plan.clone() {
        CollectionKernelPlan::GridReduce {
            workgroup,
            partials_cap,
            partial_layout,
            ..
        } => CollectionKernelPlan::GridReduce {
            source_resource: 0,
            rank: 2,
            group_shape: None,
            workgroup,
            partials_cap,
            partial_layout,
        },
        _ => unreachable!(),
    };
    let unit = KernelUnit {
        plan: rank2,
        ..unit
    };
    let got = launch_stages_for_call(&unit, &[vec![600, 500], vec![24]]).expect("stages");
    let LaunchStages::Staged([first, _]) = got else {
        panic!("staged");
    };
    assert_eq!(first.grid, [1024, 1, 1]);
}

/// An empty ungrouped reduced source is no launch at all (operator ruling
/// 2026-10-14): there is nothing to fold, so the dispatcher stores the
/// reduction's `coalesce` fallback and skips both stages. Any other zero
/// extent still has no launch geometry.
#[test]
fn grid_reduce_over_an_empty_source_is_no_launch() {
    let unit = grid_reduce_unit();
    assert_eq!(
        launch_stages_for_call(&unit, &[vec![0], vec![24]]),
        Ok(LaunchStages::Empty)
    );
    // The same call with one element is the usual two stages.
    assert!(matches!(
        launch_stages_for_call(&unit, &[vec![1], vec![24]]),
        Ok(LaunchStages::Staged(_))
    ));
    // A zero extent in the written resource is not an empty reduction.
    assert_eq!(
        launch_stages_for_call(&unit, &[vec![8], vec![0]]),
        Err(error(GEOMETRY_ZERO_EXTENT, &[("resource", "1")]))
    );
}

/// A grouped reduction has one bucket per output cell and the plan carries no
/// per-bucket fallback, so its empty source stays refused.
#[test]
fn a_grouped_grid_reduce_still_refuses_an_empty_source() {
    let unit = grid_reduce_unit();
    let CollectionKernelPlan::GridReduce {
        source_resource,
        rank,
        workgroup,
        partials_cap,
        partial_layout,
        ..
    } = unit.plan
    else {
        unreachable!()
    };
    let grouped = KernelUnit {
        plan: CollectionKernelPlan::GridReduce {
            source_resource,
            rank,
            group_shape: Some(vec![24]),
            workgroup,
            partials_cap,
            partial_layout,
        },
        ..unit
    };
    assert_eq!(
        launch_stages_for_call(&grouped, &[vec![0], vec![24]]),
        Err(error(GEOMETRY_ZERO_EXTENT, &[("resource", "0")]))
    );
}

#[test]
fn grid_reduce_refuses_rank_mismatch_and_the_single_entry() {
    let unit = grid_reduce_unit();
    assert_eq!(
        launch_stages_for_call(&unit, &[vec![4, 4], vec![24]])
            .unwrap_err()
            .code,
        GEOMETRY_RANK_MISMATCH
    );
    // The single-launch entry point does not pretend to cover two stages.
    assert_eq!(
        geometry_for_call(&unit, &[vec![256], vec![24]])
            .unwrap_err()
            .code,
        GEOMETRY_PLAN_NOT_COVERED
    );
}

#[test]
fn every_other_plan_is_one_stage_identical_to_geometry_for_call() {
    let unit = elementwise();
    let shapes = [vec![35], vec![35], vec![35]];
    assert_eq!(
        launch_stages_for_call(&unit, &shapes),
        geometry_for_call(&unit, &shapes).map(LaunchStages::Single)
    );
}

#[test]
fn grid_reduce_combine_entry_is_the_suffix_convention() {
    assert_eq!(
        grid_reduce_combine_entry("wave_speed"),
        "wave_speed_combine"
    );
}

/// The `MatMulPlan` inside a matmul fixture unit.
fn matmul_plan_of(unit: &KernelUnit) -> MatMulPlan {
    match &unit.plan {
        CollectionKernelPlan::TiledMatMul(plan) => plan.clone(),
        other => panic!("not a matmul unit: {other:?}"),
    }
}

#[test]
fn runtime_matmul_class_equals_the_literal_selection_per_shape() {
    use crate::kernel_plan::{MatMulClass, MatMulDims, simdgroup_matmul_admission};
    // (m, k, n, class): one per class, the section-5 rule-4 shapes, and the
    // n > 8192 and n % 4 != 0 boundaries of the simdgroup predicate.
    let table = [
        (1, 960, 960, MatMulClass::OneRow),
        (16, 8, 1, MatMulClass::Gemv),
        (36, 64, 52, MatMulClass::Simdgroup),
        (2, 8, 4, MatMulClass::Simdgroup),
        (37, 65, 31, MatMulClass::Tiled),
        (36, 64, 8192, MatMulClass::Simdgroup),
        (36, 64, 8196, MatMulClass::Tiled),
        (36, 64, 50, MatMulClass::Tiled),
        (16, 64, 52, MatMulClass::Tiled),
    ];
    let runtime = {
        let mut unit = matmul();
        let mut plan = matmul_plan_of(&unit);
        plan.dims = MatMulDims::Runtime {
            class: MatMulClass::Tiled,
        };
        unit.plan = CollectionKernelPlan::TiledMatMul(plan);
        unit
    };
    for (m, k, n, want) in table {
        // The literal path's own selection for the same numbers: a literal
        // plan through the emitter's predicates (m == 1, n == 1, the
        // simdgroup admission plus the row-major `n % 4` stage-read rule).
        let mut literal = matmul_plan_of(&matmul());
        literal.dims = MatMulDims::Literal { m, k, n };
        (literal.m, literal.k, literal.n) = (m, k, n);
        let literal_class = if literal.m == 1 {
            MatMulClass::OneRow
        } else if literal.n == 1 {
            MatMulClass::Gemv
        } else if simdgroup_matmul_admission(&literal).is_some() && literal.n % 4 == 0 {
            MatMulClass::Simdgroup
        } else {
            MatMulClass::Tiled
        };
        assert_eq!(literal_class, want, "{m}x{k}x{n} literal");
        assert_eq!(
            crate::kernel_plan::matmul_class_for_extents(m, k, n),
            Some(literal_class),
            "{m}x{k}x{n} selection"
        );
        let shapes = vec![vec![m, k], vec![k, n], vec![m, n]];
        let launched = matmul_launch_plan(&matmul_plan_of(&runtime), &shapes).expect("launch plan");
        assert_eq!(
            launched.dims,
            MatMulDims::Runtime {
                class: literal_class
            },
            "{m}x{k}x{n} launch plan"
        );
        // The geometry is the literal tile grid for the same numbers.
        assert_eq!(
            geometry_for_call(&runtime, &shapes),
            geometry_for_call(&matmul(), &shapes),
            "{m}x{k}x{n} geometry"
        );
    }
}

#[test]
fn runtime_matmul_refuses_the_literal_only_outer_update() {
    use crate::kernel_plan::{MatMulClass, MatMulDims};
    let mut unit = matmul();
    let mut plan = matmul_plan_of(&unit);
    plan.dims = MatMulDims::Runtime {
        class: MatMulClass::Tiled,
    };
    unit.plan = CollectionKernelPlan::TiledMatMul(plan);
    let shapes = vec![vec![8, 1], vec![1, 8], vec![8, 8]];
    let error = geometry_for_call(&unit, &shapes).expect_err("K == 1 is literal-only");
    assert_eq!(error.code, GEOMETRY_SHAPE_NOT_COVERED);
    assert!(error.args[0].1.contains("K == 1 outer update"));
}

/// The [`matmul`] fixture with call-site-shape declarations that admit a
/// batched contraction: a rank-3 receiver, a rank-3 argument and a rank-3
/// output (`matmul()` declares `[2]` on every resource, which the shared rank
/// check would reject for the batched shapes). Its plan carries run-time
/// extents, the form the product dispatcher selects the class for.
fn batched_matmul() -> KernelUnit {
    let mut unit = matmul();
    for resource in &mut unit.resources {
        resource.version.shape = Some(vec![0, 0, 0]);
    }
    let mut plan = matmul_plan_of(&unit);
    plan.dims = crate::kernel_plan::MatMulDims::Runtime {
        class: crate::kernel_plan::MatMulClass::Tiled,
    };
    // A run-time plan carries no batch fact of its own: the call-site shapes
    // are the only source.
    plan.batch = 1;
    plan.right_batch = 1;
    unit.plan = CollectionKernelPlan::TiledMatMul(plan);
    unit
}

/// The batched launch is `dispatch_y = batch·ceil(M/tile)`: one padded
/// row-block per flattened batch index, `dispatch_x = ceil(N/tile)` unchanged.
#[test]
fn runtime_batched_matmul_geometry_is_the_padded_row_block_grid() {
    // [2,5,3] · [2,3,7]: ceil(7/8) = 1 on x, 2·ceil(5/8) = 2 on y.
    let shapes = vec![vec![2, 5, 3], vec![2, 3, 7], vec![2, 5, 7]];
    assert_eq!(
        geometry_for_call(&batched_matmul(), &shapes),
        Ok(CallGeometry {
            grid: [1, 2, 1],
            block: [8, 8, 1]
        })
    );
}

/// The right operand's batch axes must be an elementwise prefix of the
/// receiver's: `[3]` is not a prefix of `[2]`.
#[test]
fn runtime_batched_matmul_geometry_refuses_a_batch_prefix_mismatch() {
    let shapes = vec![vec![2, 5, 3], vec![3, 3, 7], vec![2, 5, 7]];
    let error = geometry_for_call(&batched_matmul(), &shapes).expect_err("prefix mismatch");
    assert_eq!(error.code, GEOMETRY_SHAPE_NOT_COVERED);
}

/// A batched call always launches the batched Tiled body, even where the same
/// extents would select OneRow unbatched (`m == 1`).
#[test]
fn runtime_batched_matmul_launch_plan_selects_the_tiled_class() {
    use crate::kernel_plan::{MatMulClass, MatMulDims};
    let shapes = vec![vec![2, 1, 3], vec![2, 3, 7], vec![2, 1, 7]];
    let launched = matmul_launch_plan(&matmul_plan_of(&batched_matmul()), &shapes)
        .expect("batched launch plan");
    assert_eq!(
        launched.dims,
        MatMulDims::Runtime {
            class: MatMulClass::Tiled
        }
    );
}

/// The K == 1 outer update stays literal-only, batched or not.
#[test]
fn runtime_batched_matmul_geometry_keeps_the_outer_update_refusal() {
    let shapes = vec![vec![2, 5, 1], vec![2, 1, 7], vec![2, 5, 7]];
    let error = geometry_for_call(&batched_matmul(), &shapes).expect_err("K == 1 outer update");
    assert_eq!(error.code, GEOMETRY_SHAPE_NOT_COVERED);
}

fn grouped_grid_reduce_unit(group_shape: Vec<u64>) -> KernelUnit {
    use MirKernelResourceAccess::{Read, Write};
    unit(
        CollectionKernelPlan::GridReduce {
            source_resource: 0,
            rank: 1,
            group_shape: Some(group_shape.clone()),
            workgroup: 4,
            partials_cap: 8,
            partial_layout: crate::kernel_plan::GridReducePartialLayout {
                value_bytes: 4,
                coordinates: 0,
                seen: false,
            },
        },
        vec![
            resource(0, Read, None),
            resource(1, Read, None),
            resource(2, Write, Some(group_shape)),
        ],
    )
}

#[test]
fn grouped_grid_reduce_stages_are_source_workgroups_times_buckets() {
    let unit = grouped_grid_reduce_unit(vec![4]);
    assert_eq!(unit.plan.group_count(), Some(4));
    assert_eq!(unit.plan.grid_reduce_partial_slots(), Some(32));

    // Nine source elements give P=3 with W=4; G=4 buckets gives P*G=12
    // stage-one workgroups and G=4 stage-two workgroups.
    let shapes = [vec![9], vec![9], vec![4]];
    let LaunchStages::Staged([stage1, stage2]) =
        launch_stages_for_call(&unit, &shapes).expect("grouped stages")
    else {
        panic!("GridReduce remains a two-stage plan");
    };
    assert_eq!(
        stage1,
        CallGeometry {
            grid: [12, 1, 1],
            block: [4, 1, 1],
        }
    );
    assert_eq!(
        stage2,
        CallGeometry {
            grid: [4, 1, 1],
            block: [4, 1, 1],
        }
    );
}

#[test]
fn grouped_grid_reduce_requires_the_output_shape_to_match_its_buckets() {
    let mut unit = grouped_grid_reduce_unit(vec![4]);
    unit.resources[2].version.shape = Some(vec![2, 2]);
    // Equal element counts do not make a rank-2 output the same bucket shape.
    let error = launch_stages_for_call(&unit, &[vec![9], vec![9], vec![2, 2]])
        .expect_err("output domain must match group shape");
    assert_eq!(error.code, GEOMETRY_SHAPE_NOT_COVERED);
}

#[test]
fn grouped_partial_rows_keep_stage_one_below_two_to_the_thirty_two_lanes() {
    use crate::kernel_plan::{
        GRID_REDUCE_PARTIALS_CAP, GRID_REDUCE_WORKGROUP, grouped_partial_rows,
    };
    let rows =
        |buckets| grouped_partial_rows(GRID_REDUCE_PARTIALS_CAP, GRID_REDUCE_WORKGROUP, buckets);
    // 1024 rows * 16383 buckets * 256 lanes still fits a 32-bit lane index;
    // 16384 buckets is exactly 2^32 lanes at 1024 rows, so one row goes.
    assert_eq!(rows(1), 1024);
    assert_eq!(rows(16_383), 1024);
    assert_eq!(rows(16_384), 1023);
    assert_eq!(rows(65_536), 255);
    assert_eq!(rows(1 << 30), 1, "never zero rows");
}

#[test]
fn grouped_grid_reduce_with_many_buckets_shrinks_its_source_rows() {
    use MirKernelResourceAccess::{Read, Write};
    let buckets = 16_384u64;
    let unit = unit(
        CollectionKernelPlan::GridReduce {
            source_resource: 0,
            rank: 1,
            group_shape: Some(vec![buckets]),
            workgroup: 256,
            partials_cap: 1024,
            partial_layout: crate::kernel_plan::GridReducePartialLayout {
                value_bytes: 4,
                coordinates: 0,
                seen: false,
            },
        },
        vec![
            resource(0, Read, None),
            resource(1, Read, None),
            resource(2, Write, Some(vec![buckets])),
        ],
    );
    // 2^20 source elements would give 1024 rows; the launch keeps 1023.
    let shapes = [vec![1 << 20], vec![1 << 20], vec![buckets]];
    let LaunchStages::Staged([stage1, _]) =
        launch_stages_for_call(&unit, &shapes).expect("grouped stages")
    else {
        panic!("GridReduce remains a two-stage plan");
    };
    assert_eq!(stage1.grid, [1023 * 16_384, 1, 1]);
    assert!(u64::from(stage1.grid[0]) * u64::from(stage1.block[0]) <= u64::from(u32::MAX));
}

fn tree_reduction(partials: u64, workgroup_x: u32) -> KernelUnit {
    use MirKernelResourceAccess::{Read, Write};
    let plan = CollectionKernelPlan::TreeReduction(ReductionPlan {
        op: ReduceOp::Sum,
        length: partials.saturating_mul(u64::from(workgroup_x)),
        workgroup_x,
        partials,
        shared_memory: SharedMemoryLayout {
            element_byte_width: F32_ELEMENT_BYTE_WIDTH,
            slot_count: workgroup_x,
            buffer_name: "partials".to_owned(),
        },
        barriers: Vec::new(),
        oob_padding: OobPaddingPolicy::ZeroFill,
    });
    unit(
        plan,
        vec![resource(0, Read, None), resource(1, Write, None)],
    )
}

#[test]
fn tree_reduction_launches_one_workgroup_per_partial() {
    for (partials, workgroup_x) in [(1_u64, 256_u32), (2, 256), (3, 64), (1024, 128)] {
        let unit = tree_reduction(partials, workgroup_x);
        // The dispatch is `partials * workgroup_x` threads, so the shared
        // ceil rule yields exactly `partials` workgroups of `workgroup_x`.
        assert_eq!(
            geometry_for_call(
                &unit,
                &[vec![partials * u64::from(workgroup_x)], vec![partials]]
            ),
            Ok(CallGeometry {
                grid: [partials as u32, 1, 1],
                block: [workgroup_x, 1, 1],
            }),
            "partials {partials}, workgroup {workgroup_x}"
        );
    }
}

#[test]
fn tree_reduction_dispatch_overflow_is_an_extent_overflow() {
    let unit = tree_reduction(u64::MAX, 2);
    let result = geometry_for_call(&unit, &[vec![8], vec![1]]);
    assert_eq!(
        result.map_err(|error| error.code),
        Err(GEOMETRY_EXTENT_OVERFLOW)
    );
}

#[test]
fn tree_reduction_grid_past_u32_is_an_extent_overflow() {
    // The product fits u64, but the workgroup count does not fit a grid axis.
    let unit = tree_reduction(u64::from(u32::MAX) + 1, 1);
    let result = geometry_for_call(&unit, &[vec![8], vec![1]]);
    assert_eq!(
        result.map_err(|error| error.code),
        Err(GEOMETRY_EXTENT_OVERFLOW)
    );
}
