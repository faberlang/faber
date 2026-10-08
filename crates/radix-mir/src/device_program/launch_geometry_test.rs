use super::*;
use crate::abi::{MirKernelResourceAccess, MirKernelWorkgroupCount, MirWorkgroupSize};
use crate::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceResource,
    KernelLaunchPlan,
};
use crate::kernel_plan::{
    CollectionKernelPlan, F32_ELEMENT_BYTE_WIDTH, MATMUL_SHARED_A_BUFFER_NAME,
    MATMUL_SHARED_B_BUFFER_NAME, MATMUL_TILE, MatMulPlan, MatMulSharedMemory, OobPaddingPolicy,
    SharedMemoryLayout, TRANSPOSE_WORKGROUP_X, TransposePlan,
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
        (wg as u64, 1, wg),
        (wg as u64 + 1, 2, wg),
        (1000, 8, wg),
        // The bound itself still gets the wide workgroup.
        (max, max.div_ceil(wg as u64) as u32, wg),
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
    let plan = CollectionKernelPlan::Gather(crate::kernel_plan::GatherPlan {
        table_rows: 0,
        table_cols: 0,
        id_count: 0,
    });
    let unit = unit(
        plan,
        vec![resource(0, Read, None), resource(1, Write, None)],
    );
    assert_eq!(
        geometry_for_call(&unit, &[vec![4], vec![4]]),
        Err(error(GEOMETRY_PLAN_NOT_COVERED, &[("plan", "Gather")]))
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
