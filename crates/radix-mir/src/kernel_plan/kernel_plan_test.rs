use super::build::{
    dims_product, dims_without_axis_product, matmul_barrier_schedule, reduction_barrier_schedule,
};
use super::*;
use crate::abi::{
    CollectionOpContract, MatMulDispatch, MirKernelDispatchSize, MirKernelResource,
    MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature,
    MirWorkgroupSize, RUNTIME_SHAPED_ELEMENT_COUNT, ReducedProjection,
};
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTemp, MirTempId, MirTerminator,
    MirTerminatorKind, MirType, MirValidationContext,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, RuntimeExtentId, Type, TypeTable};

use crate::tensor_type::Dim;

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn test_span() -> Span {
    Span::default()
}

/// A rank-2 `tf32[M, N]` tensor type with static literal dims.
fn rank2_tensor_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32_ty = f32_ty(types);
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32_ty.semantic_id(), shape)))
}

/// A rank-1 `tf32[n]` tensor type.
fn rank1_tensor_ty(types: &mut TypeTable, count: u64) -> MirType {
    let f32_ty = f32_ty(types);
    let len = types.intern_index(IndexExpr::Literal(count));
    MirType::semantic(types.intern(Type::Tensor(f32_ty.semantic_id(), len)))
}

/// A generic rank-2 tensor whose axes are runtime function-parameter extents.
fn runtime_rank2_tensor_ty(types: &mut TypeTable, parameter: u32) -> MirType {
    let element = f32_ty(types);
    let dims = (0..2)
        .map(|axis| {
            types.intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
                parameter,
                axis,
            }))
        })
        .collect();
    let shape = types.intern_index(IndexExpr::Tuple(dims));
    MirType::semantic(types.intern(Type::Tensor(element.semantic_id(), shape)))
}

/// A generic rank-1 affine tensor, with the extent supplied by its parameter.
fn runtime_rank1_tensor_ty(types: &mut TypeTable, parameter: u32) -> MirType {
    let element = f32_ty(types);
    let extent = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
        parameter,
        axis: 0,
    }));
    let shape = types.intern_index(IndexExpr::Tuple(vec![extent]));
    MirType::semantic(types.intern(Type::Tensor(element.semantic_id(), shape)))
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
            reduced_projection: None,
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
            reduced_projection: None,
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
fn matmul_signature(shapes: MatMulShapes, dispatch_x: u64, dispatch_y: u64) -> MirKernelSignature {
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
            reduced_projection: None,
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
            reduced_projection: None,
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
            reduced_projection: None,
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
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    }
}

/// A post-contract transpose signature: workgroup
/// `(TRANSPOSE_WORKGROUP_X, 1, 1)`, dispatch `(M·N, 1, 1)`, input and
/// output each `M·N` elements — the shape
/// `apply_collection_op_contract` produces for a rank-2 `[M,N] → [N,M]`
/// transpose kernel.
fn transpose_signature(m: u64, n: u64) -> MirKernelSignature {
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
            element_count: m * n,
            source_local: Some(MirLocalId(0)),
            source_name: None,
            reduced_projection: None,
        },
        extra_inputs: Vec::new(),
        output: MirKernelResource {
            group: 0,
            binding: 1,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Output,
            access: MirKernelResourceAccess::Write,
            element_ty,
            element_count: m * n,
            source_local: None,
            source_name: None,
            reduced_projection: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize {
            x: TRANSPOSE_WORKGROUP_X,
            y: 1,
            z: 1,
        },
        dispatch_size: MirKernelDispatchSize {
            x: m * n,
            y: 1,
            z: 1,
        },
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
        dims: crate::kernel_plan::MatMulDims::Literal {
            m: shapes.m,
            k: shapes.k,
            n: shapes.n,
        },
        m: shapes.m,
        k: shapes.k,
        n: shapes.n,
        left_layout: shapes.left_layout,
        right_layout: shapes.right_layout,
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
        batch: 1,
        right_batch: 1,
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
    assert!(
        reduction.barriers[1..]
            .iter()
            .all(|point| point.after == BarrierPhase::ReductionStep)
    );
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
fn max_and_min_reduction_plans_preserve_their_combine_ops() {
    let signature = reduction_signature(1024, 256, 4);
    let contract = CollectionOpContract::Reduction {
        length: 1024,
        workgroup_x: 256,
        partials: 4,
    };
    for (op, expected) in [
        (MirCollectionOp::TensorMax, ReduceOp::Max),
        (MirCollectionOp::TensorMin, ReduceOp::Min),
    ] {
        let plan = CollectionKernelPlan::from_contract(&signature, &contract, op, None)
            .expect("extremum contract builds a plan");
        let CollectionKernelPlan::TreeReduction(reduction) = &plan else {
            panic!("expected a tree-reduction plan");
        };
        assert_eq!(reduction.op, expected);
        plan.validate().expect("extremum plan validates");
    }
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
        CollectionKernelPlan::from_contract(&signature, &contract, MirCollectionOp::Append, None)
            .is_none(),
        "a non-reduction op must not produce a reduction plan"
    );
}

#[test]
fn matmul_plan_is_fully_populated() {
    let shapes = matmul_fixture_shapes();
    let signature = matmul_signature(shapes, 5, 3);
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 5,
            dispatch_y: 3,
        },
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

/// Design-note item 5: the run-time contract builds a `Runtime` plan (class
/// `Tiled`, the all-shapes body) from the signature's runtime marker alone,
/// the plan validates, and a signature without the marker builds nothing.
#[test]
fn matmul_runtime_contract_builds_a_runtime_plan_from_the_marker() {
    let shapes = matmul_fixture_shapes();
    let mut signature = matmul_signature(shapes, 0, 0);
    signature.input.element_count = crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;
    let mut extent = signature.output.clone();
    extent.binding = 3;
    extent.kind = MirKernelResourceKind::TensorAxisExtent {
        tensor_binding: 0,
        axis: 0,
    };
    extent.role = MirKernelResourceRole::Input;
    extent.access = MirKernelResourceAccess::Read;
    extent.element_count = 1;
    signature.extra_inputs.push(extent);
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &CollectionOpContract::MatMul {
            dispatch: MatMulDispatch::Runtime,
        },
        MirCollectionOp::TensorMatMul,
        None,
    )
    .expect("run-time matmul builds a plan");
    let CollectionKernelPlan::TiledMatMul(plan) = plan else {
        panic!("expected a tiled matmul plan");
    };
    assert_eq!(
        plan.dims,
        MatMulDims::Runtime {
            class: MatMulClass::Tiled
        }
    );
    assert_eq!(
        (plan.tile, plan.batch, plan.right_batch),
        (MATMUL_TILE, 1, 1)
    );
    assert_eq!(plan.workgroup_x, MATMUL_TILE);
    super::validate::validate_matmul_plan(&plan).expect("run-time plan validates");

    // No extent resource: not a run-time signature.
    signature.extra_inputs.pop();
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &CollectionOpContract::MatMul {
                dispatch: MatMulDispatch::Runtime
            },
            MirCollectionOp::TensorMatMul,
            None,
        )
        .is_none()
    );
    // Wrong op: no plan.
    signature.extra_inputs.push(signature.output.clone());
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &CollectionOpContract::MatMul {
                dispatch: MatMulDispatch::Runtime
            },
            MirCollectionOp::Sum,
            None,
        )
        .is_none()
    );
}

#[test]
fn matmul_contract_without_shapes_returns_none() {
    let shapes = MatMulShapes {
        m: 16,
        k: 8,
        n: 32,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let signature = matmul_signature(shapes, 4, 2);
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 4,
            dispatch_y: 2,
        },
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
    let signature = matmul_signature(
        MatMulShapes {
            m: 16,
            k: 8,
            n: 32,
            left_layout: crate::MirScalarLayout::F32,
            right_layout: crate::MirScalarLayout::F32,
        },
        4,
        2,
    );
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 4,
            dispatch_y: 2,
        },
    };
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::TensorMatMul,
            Some(MatMulShapes {
                m: 33,
                k: 8,
                n: 33,
                left_layout: crate::MirScalarLayout::F32,
                right_layout: crate::MirScalarLayout::F32
            }),
        )
        .is_none(),
        "shapes that disagree with the contract dispatch must not build a plan"
    );
}

#[test]
fn matmul_contract_with_non_matmul_op_returns_none() {
    let signature = matmul_signature(
        MatMulShapes {
            m: 16,
            k: 8,
            n: 32,
            left_layout: crate::MirScalarLayout::F32,
            right_layout: crate::MirScalarLayout::F32,
        },
        4,
        2,
    );
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 4,
            dispatch_y: 2,
        },
    };
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::Sum,
            Some(MatMulShapes {
                m: 16,
                k: 8,
                n: 32,
                left_layout: crate::MirScalarLayout::F32,
                right_layout: crate::MirScalarLayout::F32
            }),
        )
        .is_none(),
        "a reduction op must not produce a matmul plan"
    );
}

/// A post-contract batched-attention signature: the 64-thread flat workgroup
/// and a dispatch of `output_elements` — the shape
/// `apply_collection_op_contract` produces for the batched attention ops.
fn attention_signature(output_elements: u64) -> MirKernelSignature {
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
            element_count: output_elements,
            source_local: Some(MirLocalId(0)),
            source_name: None,
            reduced_projection: None,
        },
        extra_inputs: Vec::new(),
        output: MirKernelResource {
            group: 0,
            binding: 1,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Output,
            access: MirKernelResourceAccess::Write,
            element_ty,
            element_count: output_elements,
            source_local: None,
            source_name: None,
            reduced_projection: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 64, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: output_elements,
            y: 1,
            z: 1,
        },
    }
}

#[test]
fn batched_attention_plan_resolves_typed_strides_and_decode_lane_fact() {
    // A decode projection: seq=1 selects the R4 GEMV lane expansion, and the
    // q_proj output strides over `[heads, rows, head_dim]`.
    let signature = attention_signature(8);
    let contract = CollectionOpContract::BatchedAttention {
        kind: BatchedAttentionKind::ProjectionQ,
        kv_heads: 2,
        q_per_kv: 2,
        seq: 1,
        head_dim: 4,
        hidden: 16,
        layout: None,
        rotate_half: false,
        scale_bits: 0x3f80_0000,
        output_elements: 8,
    };
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &contract,
        MirCollectionOp::TensorBatchedAttention,
        None,
    )
    .expect("the batched attention contract resolves a plan");
    let CollectionKernelPlan::BatchedAttention(projection) = &plan else {
        panic!("expected a batched attention plan");
    };
    assert_eq!(
        projection.output_strides,
        [8, 4, 4, 1],
        "ProjectionQ strides over [q_per_kv*seq*head_dim, head_dim, ...]"
    );
    assert_eq!(
        projection.decode_outputs_per_workgroup,
        Some(crate::abi::PROJECTION_GEMV_DISPATCH_LANES),
        "the decode projection carries the GEMV lane fact"
    );
    plan.validate().expect("the resolved plan validates");

    // The sequence-major context keeps the contiguous hidden vector and no
    // GEMV lane fact.
    let context_contract = CollectionOpContract::BatchedAttention {
        kind: BatchedAttentionKind::ContextSequenceMajor,
        kv_heads: 2,
        q_per_kv: 2,
        seq: 1,
        head_dim: 4,
        hidden: 16,
        layout: None,
        rotate_half: false,
        scale_bits: 0x3f80_0000,
        output_elements: 8,
    };
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &context_contract,
        MirCollectionOp::TensorBatchedAttention,
        None,
    )
    .expect("the context attention contract resolves a plan");
    let CollectionKernelPlan::BatchedAttention(context) = &plan else {
        panic!("expected a batched attention plan");
    };
    assert_eq!(
        context.output_strides,
        [16, 1, 1, 1],
        "the sequence-major context keeps the contiguous hidden vector"
    );
    assert_eq!(
        context.decode_outputs_per_workgroup, None,
        "only Q/K/V projections carry the GEMV lane fact"
    );
    plan.validate().expect("the resolved plan validates");
}

// ── Transpose recipe (S5-U1) ──────────────────────────────────────────

#[test]
fn transpose_plan_is_fully_populated_for_mlp_4x4() {
    // The MLP 4×4 shape: `[4,4] → [4,4]` — the matmul-VJP transpose.
    let signature = transpose_signature(4, 4);
    let contract = CollectionOpContract::Transpose { m: 4, n: 4 };
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &contract,
        MirCollectionOp::TensorTranspose,
        None,
    )
    .expect("transpose contract builds a plan");
    let CollectionKernelPlan::Transpose(transpose) = &plan else {
        panic!("expected a transpose plan, got {plan:?}");
    };
    assert_eq!((transpose.m, transpose.n), (4, 4));
    assert_eq!(transpose.workgroup_x, TRANSPOSE_WORKGROUP_X);
    assert_eq!(transpose.dispatch_x, 16);
    // One thread per output element: dispatch == element count.
    assert_eq!(transpose.dispatch_x, transpose.m * transpose.n);
    plan.validate().expect("built transpose plan validates");
}

#[test]
fn transpose_plan_resolves_rectangular_shape() {
    // A rectangular `[M,N] → [N,M]`: the left-operand matmul VJP
    // (`xᵀ @ grad` needs `[K,M]` from a `[M,K]` input).
    let signature = transpose_signature(3, 4);
    let contract = CollectionOpContract::Transpose { m: 3, n: 4 };
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &contract,
        MirCollectionOp::TensorTranspose,
        None,
    )
    .expect("rectangular transpose contract builds a plan");
    let CollectionKernelPlan::Transpose(transpose) = &plan else {
        panic!("expected a transpose plan, got {plan:?}");
    };
    assert_eq!((transpose.m, transpose.n), (3, 4));
    assert_eq!(transpose.dispatch_x, 12);
    plan.validate()
        .expect("rectangular transpose plan validates");
}

#[test]
fn transpose_contract_with_non_transpose_op_returns_none() {
    let signature = transpose_signature(4, 4);
    let contract = CollectionOpContract::Transpose { m: 4, n: 4 };
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::TensorMatMul,
            Some(MatMulShapes {
                m: 4,
                k: 4,
                n: 4,
                left_layout: crate::MirScalarLayout::F32,
                right_layout: crate::MirScalarLayout::F32
            }),
        )
        .is_none(),
        "a matmul op must not produce a transpose plan"
    );
}

#[test]
fn transpose_signature_not_carrying_contract_returns_none() {
    // Dispatch disagrees with the contract dims: the signature was not
    // built from this contract.
    let signature = transpose_signature(4, 4);
    let contract = CollectionOpContract::Transpose { m: 4, n: 8 };
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::TensorTranspose,
            None,
        )
        .is_none(),
        "a signature that does not carry the contract must not build a plan"
    );
}

#[test]
fn validate_rejects_transpose_zero_dimension() {
    let plan = CollectionKernelPlan::Transpose(TransposePlan {
        m: 0,
        n: 4,
        workgroup_x: TRANSPOSE_WORKGROUP_X,
        dispatch_x: 0,
        batched: None,
    });
    assert_eq!(
        plan.validate().expect_err("zero-dim transpose must fail"),
        PlanValidationError::TransposeZeroDimension { m: 0, n: 4 }
    );
}

#[test]
fn validate_rejects_transpose_zero_workgroup() {
    let plan = CollectionKernelPlan::Transpose(TransposePlan {
        m: 4,
        n: 4,
        workgroup_x: 0,
        dispatch_x: 16,
        batched: None,
    });
    assert_eq!(
        plan.validate().expect_err("zero workgroup must fail"),
        PlanValidationError::TransposeZeroWorkgroup { workgroup_x: 0 }
    );
}

#[test]
fn validate_rejects_transpose_dispatch_mismatch() {
    let plan = CollectionKernelPlan::Transpose(TransposePlan {
        m: 4,
        n: 4,
        workgroup_x: TRANSPOSE_WORKGROUP_X,
        dispatch_x: 15,
        batched: None,
    });
    assert_eq!(
        plan.validate().expect_err("dispatch mismatch must fail"),
        PlanValidationError::TransposeDispatchMismatch {
            m: 4,
            n: 4,
            dispatch_x: 15,
        }
    );
}

// --- U3 head-axis: batched transpose selection (leading head axes) ---

/// A batched-transpose signature: the elementwise-style 1D contract with
/// the batch folded into the resource extents and dispatch — the operand's
/// full `batch·M·N` count, one thread per batched output element.
fn batched_transpose_signature(batch: u64, m: u64, n: u64) -> MirKernelSignature {
    let mut signature = transpose_signature(m, n);
    let count = batch * m * n;
    signature.input.element_count = count;
    signature.output.element_count = count;
    signature.dispatch_size.x = count;
    signature
}

/// U3 head-axis done-when: the decode fixture — K `[5,76,64] → Kᵀ
/// [5,64,76]` over the row-major `[76,320]` arena — selects ONE batched
/// transpose kernel: the plan carries the batch fact (`Some(5)`), the
/// per-head dims, and the batch-folded dispatch `5·76·64`.
#[test]
fn batched_transpose_decode_shape_selects_batched_plan() {
    let signature = batched_transpose_signature(5, 76, 64);
    let contract = CollectionOpContract::BatchedTranspose {
        batch: 5,
        m: 76,
        n: 64,
    };
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &contract,
        MirCollectionOp::TensorTranspose,
        None,
    )
    .expect("batched transpose contract builds a plan");
    let CollectionKernelPlan::Transpose(transpose) = &plan else {
        panic!("expected a transpose plan, got {plan:?}");
    };
    assert_eq!((transpose.m, transpose.n), (76, 64));
    assert_eq!(transpose.batched, Some(5));
    assert_eq!(transpose.workgroup_x, TRANSPOSE_WORKGROUP_X);
    assert_eq!(transpose.dispatch_x, 5 * 76 * 64);
    plan.validate().expect("batched transpose plan validates");
}

/// U3 head-axis done-when: the prefill fixture — K `[5,36,64] → Kᵀ
/// [5,64,36]` — selects the same single batched kernel.
#[test]
fn batched_transpose_prefill_shape_selects_batched_plan() {
    let signature = batched_transpose_signature(5, 36, 64);
    let contract = CollectionOpContract::BatchedTranspose {
        batch: 5,
        m: 36,
        n: 64,
    };
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &contract,
        MirCollectionOp::TensorTranspose,
        None,
    )
    .expect("batched transpose contract builds a plan");
    let CollectionKernelPlan::Transpose(transpose) = &plan else {
        panic!("expected a transpose plan, got {plan:?}");
    };
    assert_eq!((transpose.m, transpose.n), (36, 64));
    assert_eq!(transpose.batched, Some(5));
    assert_eq!(transpose.dispatch_x, 5 * 36 * 64);
    plan.validate().expect("batched transpose plan validates");
}

/// A signature whose resource extent contradicts the contract's batched
/// count resolves no plan — the batch is a typed-fact cross-check, never
/// a guess.
#[test]
fn batched_transpose_count_contradiction_returns_none() {
    let mut signature = batched_transpose_signature(5, 76, 64);
    signature.input.element_count = 5 * 76 * 64 + 1;
    let contract = CollectionOpContract::BatchedTranspose {
        batch: 5,
        m: 76,
        n: 64,
    };
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::TensorTranspose,
            None,
        )
        .is_none(),
        "a contradicting input extent must resolve no plan"
    );
}

/// A rank-2 contract on a rank-2 signature keeps the ordinary image: no
/// batch fact (the rank-2 path is unchanged).
#[test]
fn rank2_transpose_plan_carries_no_batch_fact() {
    let signature = transpose_signature(4, 4);
    let contract = CollectionOpContract::Transpose { m: 4, n: 4 };
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &contract,
        MirCollectionOp::TensorTranspose,
        None,
    )
    .expect("rank-2 transpose contract builds a plan");
    let CollectionKernelPlan::Transpose(transpose) = &plan else {
        panic!("expected a transpose plan, got {plan:?}");
    };
    assert_eq!(transpose.batched, None);
}

#[test]
fn validate_rejects_batched_transpose_zero_batch() {
    let plan = CollectionKernelPlan::Transpose(TransposePlan {
        m: 76,
        n: 64,
        workgroup_x: TRANSPOSE_WORKGROUP_X,
        dispatch_x: 76 * 64,
        batched: Some(0),
    });
    assert_eq!(
        plan.validate().expect_err("zero batch must fail"),
        PlanValidationError::TransposeBatchLaw {
            batch: 0,
            m: 76,
            n: 64,
            dispatch_x: 76 * 64,
        }
    );
}

#[test]
fn validate_rejects_batched_transpose_dispatch_mismatch() {
    // The dispatch tiles only one head's output: the batch-folded law
    // fails typed.
    let plan = CollectionKernelPlan::Transpose(TransposePlan {
        m: 76,
        n: 64,
        workgroup_x: TRANSPOSE_WORKGROUP_X,
        dispatch_x: 76 * 64,
        batched: Some(5),
    });
    assert_eq!(
        plan.validate()
            .expect_err("batched dispatch mismatch must fail"),
        PlanValidationError::TransposeDispatchMismatch {
            m: 76,
            n: 64,
            dispatch_x: 76 * 64,
        }
    );
}

#[test]
fn matmul_signature_not_carrying_contract_returns_none() {
    // The output element count disagrees with the shapes: the signature
    // was not built from this contract.
    let mut signature = matmul_signature(
        MatMulShapes {
            m: 16,
            k: 8,
            n: 32,
            left_layout: crate::MirScalarLayout::F32,
            right_layout: crate::MirScalarLayout::F32,
        },
        4,
        2,
    );
    signature.output.element_count = 1;
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 4,
            dispatch_y: 2,
        },
    };
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::TensorMatMul,
            Some(MatMulShapes {
                m: 16,
                k: 8,
                n: 32,
                left_layout: crate::MirScalarLayout::F32,
                right_layout: crate::MirScalarLayout::F32
            }),
        )
        .is_none(),
        "a signature that does not carry the contract must not build a plan"
    );
}

#[test]
fn matmul_signature_with_fused_tail_operands_builds_the_same_plan() {
    // `y ⇇ x.matmul(w) + b` carries the bias `b` as a third input. It is a
    // data-flow buffer of the fused elementwise tail, not a recipe operand:
    // the plan is the one the bare `x.matmul(w)` signature builds.
    let shapes = MatMulShapes {
        m: 16,
        k: 8,
        n: 32,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 4,
            dispatch_y: 2,
        },
    };
    let bare = matmul_signature(shapes, 4, 2);
    let mut with_tail = matmul_signature(shapes, 4, 2);
    let mut bias = with_tail.extra_inputs[0].clone();
    bias.binding = 2;
    bias.element_count = shapes.m * shapes.n;
    bias.source_local = Some(MirLocalId(2));
    with_tail.extra_inputs.push(bias);
    let want = CollectionKernelPlan::from_contract(
        &bare,
        &contract,
        MirCollectionOp::TensorMatMul,
        Some(shapes),
    )
    .expect("the bare matmul signature builds a plan");
    let got = CollectionKernelPlan::from_contract(
        &with_tail,
        &contract,
        MirCollectionOp::TensorMatMul,
        Some(shapes),
    )
    .expect("tail operands must not turn a matmul with a fused tail into a decomposition");
    assert_eq!(got, want);
}

#[test]
fn matmul_signature_without_a_right_operand_returns_none() {
    // The right operand is the first extra input; a signature with none has
    // no recipe operand to count, whatever else it carries.
    let shapes = MatMulShapes {
        m: 16,
        k: 8,
        n: 32,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let mut signature = matmul_signature(shapes, 4, 2);
    signature.extra_inputs.clear();
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 4,
            dispatch_y: 2,
        },
    };
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::TensorMatMul,
            Some(shapes),
        )
        .is_none(),
        "a matmul signature with no right operand must not build a plan"
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

/// PGC-B2 T=1 one-row exception: an M=1 plan may carry workgroup
/// `(tile, 1)` (the decode GEMV launch shape); every other multi-row
/// workgroup drift (including a one-row workgroup on an M>1 plan, and a
/// shrunk x lane) still rejects.
#[test]
fn validate_admits_matmul_one_row_workgroup_only_for_t1() {
    let mut plan = matmul_plan();
    plan.m = 1;
    plan.workgroup_y = 1;
    assert!(
        CollectionKernelPlan::TiledMatMul(plan.clone())
            .validate()
            .is_ok(),
        "an M=1 plan admits the one-row workgroup (tile, 1)"
    );
    let mut multi_row = matmul_plan();
    multi_row.workgroup_y = 1;
    assert_eq!(
        matmul_plan_validation_error(&multi_row),
        PlanValidationError::MatMulWorkgroupMismatch {
            workgroup_x: MATMUL_TILE,
            workgroup_y: 1,
            tile: MATMUL_TILE,
        }
    );
    let mut one_row_x = matmul_plan();
    one_row_x.m = 1;
    one_row_x.workgroup_y = 1;
    one_row_x.workgroup_x = MATMUL_TILE - 1;
    assert_eq!(
        matmul_plan_validation_error(&one_row_x),
        PlanValidationError::MatMulWorkgroupMismatch {
            workgroup_x: MATMUL_TILE - 1,
            workgroup_y: 1,
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

// ── S6-P1 transformer recipe resolution (`from_transformer_op`) ──────────

/// A single-transformer-op kernel fixture: `t = op(x, const_args…)` over a
/// rank-2 `[dims]` tensor. The fixture keeps the rank-2 type on the return
/// and locals (the plan resolution reads the operand dims from the args,
/// not the return).
fn transformer_function(
    types: &mut TypeTable,
    dims: [u64; 2],
    op: MirCollectionOp,
    const_args: Vec<MirOperand>,
) -> MirFunction {
    let r2 = rank2_tensor_ty(types, dims);
    let span = test_span();
    let mut args = vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))];
    args.extend(const_args);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: r2,
            mode: MirParamMode::Owned,
            span,
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: r2,
            mutable: false,
            span,
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(op),
                        args,
                        return_ty: r2,
                    },
                },
                span,
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(1),
                )))),
                span,
            },
            span,
        }],
        return_ty: r2,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

/// Runtime-extent counterpart to [`transformer_function`], with generic
/// function-parameter axes on the rank-2 operand.
fn runtime_transformer_function(
    types: &mut TypeTable,
    op: MirCollectionOp,
    const_args: Vec<MirOperand>,
) -> MirFunction {
    let r2 = runtime_rank2_tensor_ty(types, 0);
    let span = test_span();
    let mut args = vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))];
    args.extend(const_args);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: r2,
            mode: MirParamMode::Owned,
            span,
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: r2,
            mutable: false,
            span,
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(op),
                        args,
                        return_ty: r2,
                    },
                },
                span,
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(1),
                )))),
                span,
            },
            span,
        }],
        return_ty: r2,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

/// A generic-1D transformer signature: workgroup `(1,1,1)`, dispatch
/// `(output_count, 1, 1)`, input/output at the flattened counts — the shape
/// the ABI produces for kernels without a collection-op contract.
fn transformer_signature(input_count: u64, output_count: u64) -> MirKernelSignature {
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
            element_count: input_count,
            source_local: Some(MirLocalId(0)),
            source_name: None,
            reduced_projection: None,
        },
        extra_inputs: Vec::new(),
        output: MirKernelResource {
            group: 0,
            binding: 1,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Output,
            access: MirKernelResourceAccess::Write,
            element_ty,
            element_count: output_count,
            source_local: None,
            source_name: None,
            reduced_projection: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: output_count,
            y: 1,
            z: 1,
        },
    }
}

/// A runtime-shaped row-op signature; the output's source local names the
/// statement destination for the RowMapped eligibility proof.
fn runtime_transformer_signature() -> MirKernelSignature {
    let mut signature =
        transformer_signature(RUNTIME_SHAPED_ELEMENT_COUNT, RUNTIME_SHAPED_ELEMENT_COUNT);
    signature.output.source_local = Some(MirLocalId(1));
    signature
}

#[test]
fn rowop_runtime_extents_resolve_runtime_plans() {
    let mut types = TypeTable::new();
    let mut plain_softmax =
        runtime_transformer_function(&mut types, MirCollectionOp::TensorSoftmax, vec![]);
    plain_softmax.blocks[0].terminator.kind = MirTerminatorKind::Return(None);
    let returned_softmax =
        runtime_transformer_function(&mut types, MirCollectionOp::TensorSoftmax, vec![]);
    let layer_norm = runtime_transformer_function(
        &mut types,
        MirCollectionOp::TensorLayerNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
        ],
    );
    let causal = runtime_transformer_function(
        &mut types,
        MirCollectionOp::TensorCausalMaskedSoftmax,
        vec![],
    );
    let mut signature = runtime_transformer_signature();
    signature.output.source_local = None;
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &plain_softmax,
        &validation,
        MirCollectionOp::TensorSoftmax,
    )
    .expect("runtime softmax resolves");
    assert!(
        matches!(
            plan,
            CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan {
                dims: RowOpDims::Runtime {
                    class: RowOpClass::Generic1D
                },
                batched: None,
                ..
            })
        ),
        "unexpected plain softmax plan: {plan:?}"
    );

    let returned_signature = runtime_transformer_signature();
    let plan = CollectionKernelPlan::from_transformer_op(
        &returned_signature,
        &returned_softmax,
        &validation,
        MirCollectionOp::TensorSoftmax,
    )
    .expect("returned runtime softmax resolves");
    assert!(matches!(
        plan,
        CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan {
            dims: RowOpDims::Runtime {
                class: RowOpClass::RowMapped
            },
            batched: None,
            ..
        })
    ));

    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &layer_norm,
        &validation,
        MirCollectionOp::TensorLayerNorm,
    )
    .expect("runtime layer norm resolves");
    assert!(matches!(
        plan,
        CollectionKernelPlan::LayerNormalization(LayerNormalizationPlan {
            dims: RowOpDims::Runtime { .. },
            axis: 1,
            ..
        })
    ));

    let mut rms_types = TypeTable::new();
    let gamma_ty = runtime_rank1_tensor_ty(&mut rms_types, 1);
    let mut rms_norm = runtime_transformer_function(
        &mut rms_types,
        MirCollectionOp::TensorRmsNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
    );
    rms_norm.params.push(MirParam {
        local: MirLocalId(1),
        name: None,
        ty: gamma_ty,
        mode: MirParamMode::Owned,
        span: test_span(),
    });
    rms_norm.locals.push(MirLocal {
        id: MirLocalId(1),
        name: None,
        ty: gamma_ty,
        mutable: false,
        span: test_span(),
    });
    let mut rms_signature = runtime_transformer_signature();
    rms_signature.extra_inputs.push(MirKernelResource {
        group: 0,
        binding: 2,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty: rms_signature.input.element_ty,
        element_count: RUNTIME_SHAPED_ELEMENT_COUNT,
        source_local: Some(MirLocalId(1)),
        source_name: None,
        reduced_projection: None,
    });
    let rms_validation = MirValidationContext::new(&rms_types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &rms_signature,
        &rms_norm,
        &rms_validation,
        MirCollectionOp::TensorRmsNorm,
    )
    .expect("runtime RMS norm resolves");
    assert!(matches!(
        plan,
        CollectionKernelPlan::RmsNormalization(RmsNormalizationPlan {
            dims: RowOpDims::Runtime { .. },
            axis: 1,
            width: 0,
            ..
        })
    ));
    plan.validate()
        .expect("runtime RMS dimensions defer positivity to launch");

    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &causal,
        &validation,
        MirCollectionOp::TensorCausalMaskedSoftmax,
    )
    .expect("runtime causal softmax resolves");
    assert!(matches!(
        plan,
        CollectionKernelPlan::CausalMaskedSoftmax(CausalMaskedSoftmaxPlan {
            dims: RowOpDims::Runtime { .. },
            rows: 0,
            cols: 0,
        })
    ));
    plan.validate()
        .expect("runtime causal dimensions defer positivity to launch");
}

#[test]
fn rowop_runtime_batched_softmax_is_refused() {
    let mut types = TypeTable::new();
    let mut function =
        runtime_transformer_function(&mut types, MirCollectionOp::TensorSoftmax, vec![]);
    let element = f32_ty(&mut types);
    let axes = (0..3)
        .map(|axis| {
            types.intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
                parameter: 0,
                axis,
            }))
        })
        .collect();
    let shape = types.intern_index(IndexExpr::Tuple(axes));
    let rank3 = MirType::semantic(types.intern(Type::Tensor(element.semantic_id(), shape)));
    function.params[0].ty = rank3;
    function.locals[0].ty = rank3;
    function.return_ty = rank3;
    let MirStatementKind::RuntimeCall { call, .. } = &mut function.blocks[0].statements[0].kind
    else {
        unreachable!("softmax is a runtime call");
    };
    call.return_ty = rank3;

    let signature = runtime_transformer_signature();
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorSoftmax,
        )
        .is_none()
    );
}

#[test]
fn rowop_runtime_elementwise_operand_is_refused() {
    let mut types = TypeTable::new();
    let mut function =
        runtime_transformer_function(&mut types, MirCollectionOp::TensorSoftmax, vec![]);
    let tensor_ty = function.params[0].ty;
    let span = test_span();
    function.temps.push(MirTemp {
        id: MirTempId(0),
        ty: tensor_ty,
        span,
    });
    let other_ty = runtime_rank2_tensor_ty(&mut types, 1);
    function.params.push(MirParam {
        local: MirLocalId(2),
        name: None,
        ty: other_ty,
        mode: MirParamMode::Owned,
        span,
    });
    function.locals.push(MirLocal {
        id: MirLocalId(2),
        name: None,
        ty: other_ty,
        mutable: false,
        span,
    });
    let additive = MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace {
                base: MirPlaceBase::Temp(MirTempId(0)),
                projections: Vec::new(),
            }),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
                args: vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Place(MirPlace::local(MirLocalId(2))),
                ],
                return_ty: tensor_ty,
            },
        },
        span,
    };
    function.blocks[0].statements.insert(0, additive);
    let MirStatementKind::RuntimeCall { call, .. } = &mut function.blocks[0].statements[1].kind
    else {
        unreachable!("the softmax statement follows its additive producer");
    };
    call.args[0] = MirOperand::Temp(MirTempId(0));

    let signature = runtime_transformer_signature();
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorSoftmax,
        )
        .is_none()
    );
}

#[test]
fn transformer_operand_dims_typed_keeps_literal_axes() {
    // The typed helper keeps the operand's axes as `Dim`s (a run-time extent
    // stays `Dim::Runtime` instead of failing), and the typed recipe shape
    // reports the same dims with the softmax last axis.
    let mut types = TypeTable::new();
    let function = transformer_function(&mut types, [4, 4], MirCollectionOp::TensorSoftmax, vec![]);
    let validation = MirValidationContext::new(&types);
    let operand = MirOperand::Place(MirPlace::local(MirLocalId(0)));
    let dims = transformer_operand_dims_typed(&function, &operand, &validation)
        .expect("the typed helper reads the operand's axes");
    assert_eq!(dims, vec![Dim::Literal(4), Dim::Literal(4)]);
    let (shape, axis) =
        transformer_recipe_shape_typed(&function, &validation, MirCollectionOp::TensorSoftmax)
            .expect("the typed softmax shape resolves");
    assert_eq!(shape, dims);
    assert_eq!(axis, 1);
}

#[test]
fn tensor_sum_axis_resolves_axis_reduction_plan() {
    // S6-P1 admission: `t = sum_axis(x, 1)` over `[4,4]` resolves the
    // AxisReduction recipe with `op: Sum` and the axis from the trailing
    // const operand (S6-N1-frozen plan struct).
    let mut types = TypeTable::new();
    let function = transformer_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorSumAxis,
        vec![MirOperand::Constant(MirConstant::Int(1))],
    );
    let signature = transformer_signature(16, 4);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorSumAxis,
    )
    .expect("sum_axis typed facts resolve a plan");
    let CollectionKernelPlan::AxisReduction(axis_reduction) = &plan else {
        panic!("sum_axis must map to an axis-reduction plan, got {plan:?}");
    };
    assert_eq!(axis_reduction.op, ReduceOp::Sum);
    assert_eq!(axis_reduction.axis, 1);
    plan.validate().expect("the resolved plan validates");
}

#[test]
fn softmax_resolves_row_softmax_plan() {
    // S6-P1 admission: `t = softmax(x)` over `[4,4]` resolves the RowSoftmax
    // recipe over the last (row) axis.
    let mut types = TypeTable::new();
    let function = transformer_function(&mut types, [4, 4], MirCollectionOp::TensorSoftmax, vec![]);
    let signature = transformer_signature(16, 16);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorSoftmax,
    )
    .expect("softmax typed facts resolve a plan");
    let CollectionKernelPlan::RowSoftmax(row_softmax) = &plan else {
        panic!("softmax must map to a row-softmax plan, got {plan:?}");
    };
    assert_eq!(row_softmax.axis, 1);
    plan.validate().expect("the resolved plan validates");
}

#[test]
fn row_softmax_plan_carries_literal_row_and_width() {
    // The unbatched row-softmax plan carries the operand's literal extents:
    // `[4,4]` resolves `rows: 4, width: 4` over the last (row) axis.
    let mut types = TypeTable::new();
    let function = transformer_function(&mut types, [4, 4], MirCollectionOp::TensorSoftmax, vec![]);
    let signature = transformer_signature(16, 16);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorSoftmax,
    )
    .expect("softmax typed facts resolve a plan");
    let CollectionKernelPlan::RowSoftmax(row_softmax) = &plan else {
        panic!("softmax must map to a row-softmax plan, got {plan:?}");
    };
    assert_eq!(row_softmax.dims, RowOpDims::Literal { rows: 4, width: 4 });
}

#[test]
fn causal_masked_softmax_plan_carries_literal_dims() {
    // The causal masked-softmax plan carries the operand's literal extents:
    // `[8,8]` resolves dims `rows: 8, width: 8` alongside the typed
    // `rows`/`cols` fields.
    let mut types = TypeTable::new();
    let function = transformer_function(
        &mut types,
        [8, 8],
        MirCollectionOp::TensorCausalMaskedSoftmax,
        vec![],
    );
    let signature = transformer_signature(64, 64);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorCausalMaskedSoftmax,
    )
    .expect("causal masked-softmax typed facts resolve a plan");
    let CollectionKernelPlan::CausalMaskedSoftmax(causal) = &plan else {
        panic!("causal masked softmax must map to its plan, got {plan:?}");
    };
    assert_eq!(causal.dims, RowOpDims::Literal { rows: 8, width: 8 });
    assert_eq!(causal.rows, 8, "the row (query) extent");
    assert_eq!(causal.cols, 8, "the column (key) extent");
}

#[test]
fn layer_norm_resolves_layer_normalization_plan() {
    // S6-P1 admission: `t = layernorm(x, 1, 1e-5)` over `[4,4]` resolves the
    // LayerNormalization recipe with the axis from the const operand.
    let mut types = TypeTable::new();
    let function = transformer_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorLayerNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
        ],
    );
    let signature = transformer_signature(16, 16);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorLayerNorm,
    )
    .expect("layer-norm typed facts resolve a plan");
    let CollectionKernelPlan::LayerNormalization(layer_norm) = &plan else {
        panic!("layer-norm must map to a layer-normalization plan, got {plan:?}");
    };
    assert_eq!(layer_norm.axis, 1);
    plan.validate().expect("the resolved plan validates");
}

/// A rank-2 RMSNorm fixture with the mandatory rank-1 affine gamma peer:
/// `t = rmsnorm(x, 1, 1e-5, gamma)` over `[dims]` (the fourth operand is the
/// gamma local the plan resolution cross-checks against the last axis).
fn rms_norm_transformer_function(types: &mut TypeTable, dims: [u64; 2]) -> MirFunction {
    let mut function = transformer_function(
        types,
        dims,
        MirCollectionOp::TensorRmsNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
    );
    let gamma = rank1_tensor_ty(types, dims[1]);
    function.params.push(MirParam {
        local: MirLocalId(1),
        name: None,
        ty: gamma,
        mode: MirParamMode::Owned,
        span: test_span(),
    });
    function.locals.push(MirLocal {
        id: MirLocalId(1),
        name: None,
        ty: gamma,
        mutable: false,
        span: test_span(),
    });
    function
}

#[test]
fn layer_norm_dynamic_epsilon_resolves_dynamic_plan() {
    let mut types = TypeTable::new();
    let mut dynamic_function = transformer_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorLayerNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Place(MirPlace::local(MirLocalId(2))),
        ],
    );
    let epsilon_ty = f32_ty(&mut types);
    dynamic_function.params.push(MirParam {
        local: MirLocalId(2),
        name: None,
        ty: epsilon_ty,
        mode: MirParamMode::Owned,
        span: test_span(),
    });
    dynamic_function.locals.push(MirLocal {
        id: MirLocalId(2),
        name: None,
        ty: epsilon_ty,
        mutable: false,
        span: test_span(),
    });
    let mut dynamic_signature = transformer_signature(16, 16);
    dynamic_signature.extra_inputs.push(MirKernelResource {
        group: 0,
        binding: 2,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty: dynamic_signature.input.element_ty,
        element_count: 1,
        source_local: Some(MirLocalId(2)),
        source_name: None,
        reduced_projection: None,
    });
    let literal_function = transformer_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorLayerNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
        ],
    );
    let validation = MirValidationContext::new(&types);
    let dynamic = CollectionKernelPlan::from_transformer_op(
        &dynamic_signature,
        &dynamic_function,
        &validation,
        MirCollectionOp::TensorLayerNorm,
    )
    .expect("scalar-param epsilon resolves a layer-norm plan");
    let CollectionKernelPlan::LayerNormalization(dynamic) = dynamic else {
        panic!("layer norm must resolve a layer-normalization plan");
    };
    assert_eq!(dynamic.epsilon_bits, 0);

    let literal = CollectionKernelPlan::from_transformer_op(
        &transformer_signature(16, 16),
        &literal_function,
        &validation,
        MirCollectionOp::TensorLayerNorm,
    )
    .expect("literal epsilon resolves a layer-norm plan");
    let CollectionKernelPlan::LayerNormalization(literal) = literal else {
        panic!("layer norm must resolve a layer-normalization plan");
    };
    assert_eq!(literal.epsilon_bits, 1e-5_f32.to_bits());
}

#[test]
fn layer_norm_and_rms_plan_carry_literal_dims() {
    // Both normalization recipes carry the operand's literal extents: the
    // `[4,4]` fixtures resolve `dims == Literal { rows: 4, width: 4 }`.
    let mut types = TypeTable::new();
    let layer_function = transformer_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorLayerNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
        ],
    );
    let rms_function = rms_norm_transformer_function(&mut types, [4, 4]);
    let validation = MirValidationContext::new(&types);

    let layer_signature = transformer_signature(16, 16);
    let layer = CollectionKernelPlan::from_transformer_op(
        &layer_signature,
        &layer_function,
        &validation,
        MirCollectionOp::TensorLayerNorm,
    )
    .expect("layer-norm typed facts resolve a plan");
    let CollectionKernelPlan::LayerNormalization(layer_plan) = &layer else {
        panic!("layer-norm must map to a layer-normalization plan, got {layer:?}");
    };
    assert_eq!(
        layer_plan.dims,
        RowOpDims::Literal { rows: 4, width: 4 },
        "the layer-norm plan carries the operand's literal extents"
    );

    let mut rms_signature = transformer_signature(16, 16);
    let element_ty = rms_signature.input.element_ty;
    rms_signature.extra_inputs.push(MirKernelResource {
        group: 0,
        binding: 1,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty,
        element_count: 4,
        source_local: Some(MirLocalId(1)),
        source_name: None,
        reduced_projection: None,
    });
    let rms = CollectionKernelPlan::from_transformer_op(
        &rms_signature,
        &rms_function,
        &validation,
        MirCollectionOp::TensorRmsNorm,
    )
    .expect("rms-norm typed facts resolve a plan");
    let CollectionKernelPlan::RmsNormalization(rms_plan) = &rms else {
        panic!("rms-norm must map to an RMS-normalization plan, got {rms:?}");
    };
    assert_eq!(
        rms_plan.dims,
        RowOpDims::Literal { rows: 4, width: 4 },
        "the RMS-norm plan carries the operand's literal extents"
    );
}

#[test]
fn layer_norm_affine_operands_resolve_the_plan() {
    // The optional affine pair (gamma, beta) does not change the carried
    // recipe — the plan is LayerNormalization with the axis; the affine
    // tensors are data-flow inputs, not recipe facts.
    let mut types = TypeTable::new();
    let mut function = transformer_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorLayerNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
            MirOperand::Place(MirPlace::local(MirLocalId(2))),
        ],
    );
    // The affine pair must be typed rank-1 over the normalization dimension
    // (the v1 constraint the plan resolution cross-checks).
    let affine = rank1_tensor_ty(&mut types, 4);
    function.locals.push(MirLocal {
        id: MirLocalId(1),
        name: None,
        ty: affine,
        mutable: false,
        span: test_span(),
    });
    function.locals.push(MirLocal {
        id: MirLocalId(2),
        name: None,
        ty: affine,
        mutable: false,
        span: test_span(),
    });
    let signature = transformer_signature(16, 16);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorLayerNorm,
    )
    .expect("layer-norm with affine operands resolves a plan");
    let CollectionKernelPlan::LayerNormalization(layer_norm) = &plan else {
        panic!("layer-norm must map to a layer-normalization plan, got {plan:?}");
    };
    assert_eq!(layer_norm.axis, 1);
}

#[test]
fn layer_norm_wrong_affine_shape_fails_resolution() {
    // The v1 affine contract: gamma/beta must be rank-1 over the
    // normalization dimension. A mismatched affine shape yields no plan.
    let mut types = TypeTable::new();
    let mut function = transformer_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorLayerNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
    );
    function.locals.push(MirLocal {
        id: MirLocalId(1),
        name: None,
        ty: rank1_tensor_ty(&mut types, 8),
        mutable: false,
        span: test_span(),
    });
    let signature = transformer_signature(16, 16);
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorLayerNorm,
        )
        .is_none(),
        "a mismatched affine shape must not resolve a layer-norm plan"
    );
}

#[test]
fn layer_norm_non_last_axis_fails_resolution() {
    // v1 constraint: layer norm normalizes over the LAST axis. A const axis
    // disagreeing with the last axis yields no plan (fail-closed typed
    // diagnostic at the caller, never a guessed plan).
    let mut types = TypeTable::new();
    let function = transformer_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorLayerNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(0)),
            MirOperand::Constant(MirConstant::Float(1e-5)),
        ],
    );
    let signature = transformer_signature(16, 16);
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorLayerNorm,
        )
        .is_none(),
        "a non-last axis must not resolve a layer-norm plan"
    );
}

#[test]
fn layer_norm_non_const_epsilon_fails_resolution() {
    // A3: the plan never infers a fact. A non-const epsilon operand cannot
    // be resolved from the const operands and yields no plan.
    let mut types = TypeTable::new();
    let function = transformer_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorLayerNorm,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Place(MirPlace::local(MirLocalId(1))),
        ],
    );
    let signature = transformer_signature(16, 16);
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorLayerNorm,
        )
        .is_none(),
        "a non-const epsilon must not resolve a layer-norm plan"
    );
}

#[test]
fn transformer_signature_count_mismatch_returns_none() {
    // The signature must carry the flattened input/output dims — a
    // signature that does not carry the recipe's shape yields no plan (the
    // same fail-closed law as the contract families).
    let mut types = TypeTable::new();
    let function = transformer_function(&mut types, [4, 4], MirCollectionOp::TensorSoftmax, vec![]);
    let signature = transformer_signature(16, 8);
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorSoftmax,
        )
        .is_none(),
        "a signature that does not carry the recipe counts must not build a plan"
    );
}

#[test]
fn transformer_resolution_rejects_out_of_scope_ops() {
    // A non-transformer op (here the genuinely-unplannable
    // TensorCruxEntropia) never resolves through the transformer path.
    let mut types = TypeTable::new();
    let function = transformer_function(
        &mut types,
        [4, 4],
        MirCollectionOp::TensorCruxEntropia,
        vec![],
    );
    let signature = transformer_signature(16, 16);
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorCruxEntropia,
        )
        .is_none(),
        "a non-transformer op must not resolve a transformer plan"
    );
}

#[test]
fn axis_reduction_plan_validate_rejects_non_sum_op() {
    // The v1 axis-reduction recipe set is `Sum` only.
    let plan = CollectionKernelPlan::AxisReduction(AxisReductionPlan {
        op: ReduceOp::Mean,
        axis: 1,
        projection: ReducedProjection {
            axis_extent: 8,
            inner_stride: 1,
        },
    });
    assert_eq!(
        plan.validate().expect_err("mean-over-axis must fail"),
        PlanValidationError::AxisReductionUnsupportedOp { op: ReduceOp::Mean }
    );
}

#[test]
fn transformer_plans_validate() {
    CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan {
        dims: RowOpDims::Literal { rows: 1, width: 1 },
        axis: 1,
        batched: None,
    })
    .validate()
    .expect("row-softmax plan validates");
    CollectionKernelPlan::LayerNormalization(LayerNormalizationPlan {
        dims: RowOpDims::Literal { rows: 1, width: 1 },
        axis: 1,
        epsilon_bits: 0,
    })
    .validate()
    .expect("layer-normalization plan validates");
    CollectionKernelPlan::AxisReduction(AxisReductionPlan {
        op: ReduceOp::Sum,
        axis: 1,
        projection: ReducedProjection {
            axis_extent: 8,
            inner_stride: 1,
        },
    })
    .validate()
    .expect("axis-reduction plan validates");
    CollectionKernelPlan::AxisReduction(AxisReductionPlan {
        op: ReduceOp::Sum,
        axis: 1,
        projection: ReducedProjection {
            axis_extent: 0,
            inner_stride: 1,
        },
    })
    .validate()
    .expect_err("a zero axis extent makes the projection degenerate");
}

#[test]
fn equal_dim_reductions_select_different_reduced_buffer_indices() {
    // Council-8 CB-3 (the [4,4] proof): reducing `[4,4]` over axis 0 vs
    // axis 1 both produce a 4-element reduced buffer — element count cannot
    // decide the projection. The producer-defined reduced-resource
    // projection distinguishes them, and the keep-dims flat element at
    // `keep_dims_flat` maps to a DIFFERENT reduced-buffer index under each
    // reduction.
    let dims = [4u64, 4u64];
    let axis0 = super::build::axis_reduction_projection(&dims, 0).expect("axis-0 projection");
    let axis1 = super::build::axis_reduction_projection(&dims, 1).expect("axis-1 projection");
    assert_eq!(
        axis0,
        ReducedProjection {
            axis_extent: 4,
            inner_stride: 4
        }
    );
    assert_eq!(
        axis1,
        ReducedProjection {
            axis_extent: 4,
            inner_stride: 1
        }
    );
    assert_ne!(
        axis0, axis1,
        "equal dims [4,4] must not conflate the axis-0 and axis-1 projections"
    );
    // The keep-dims flat element (row 1, column 2) of the operand: axis-0
    // reduces columns (reduced index = column), axis-1 reduces rows
    // (reduced index = row) — different reduced-buffer indices.
    let keep_dims_flat = 4 + 2;
    assert_eq!(axis0.reduced_index(keep_dims_flat), 2);
    assert_eq!(axis1.reduced_index(keep_dims_flat), 1);
    assert_ne!(
        axis0.reduced_index(keep_dims_flat),
        axis1.reduced_index(keep_dims_flat),
        "[4,4] axis-0 vs axis-1 must select different reduced-buffer indices"
    );
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
            reduced_projection: None,
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
            reduced_projection: None,
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
            reduced_projection: None,
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

// ── R-PACK-01A packed plan/op surface freeze ─────────────────────────────

fn packed_layout_fixture(layout: crate::abi::PackedStorageLayout) -> CollectionOpContract {
    CollectionOpContract::QuantizedMatMul {
        dispatch_x: 2,
        dispatch_y: 1,
        m: 8,
        k: 16,
        n: 16,
        layout,
    }
}

fn quantized_matmul_signature(m: u64, k: u64, n: u64) -> MirKernelSignature {
    matmul_signature(
        MatMulShapes {
            m,
            k,
            n,
            left_layout: crate::MirScalarLayout::F32,
            right_layout: crate::MirScalarLayout::F32,
        },
        n.div_ceil(8),
        m.div_ceil(8),
    )
}

#[test]
fn packed_storage_layout_binds_pinned_ggml_block_geometry() {
    use crate::abi::PackedStorageLayout;
    let rows = [
        (PackedStorageLayout::F32, 0, 1, 4),
        (PackedStorageLayout::F16, 1, 1, 2),
        (PackedStorageLayout::Bf16, 30, 1, 2),
        (PackedStorageLayout::Q8_0, 8, 32, 34),
        (PackedStorageLayout::Q4_K, 12, 256, 144),
        (PackedStorageLayout::Q5_K, 13, 256, 176),
        (PackedStorageLayout::Q6_K, 14, 256, 210),
        (PackedStorageLayout::Q5_0, 6, 32, 22),
        (PackedStorageLayout::MXFP4, 39, 32, 17),
    ];
    for (layout, id, elements, bytes) in rows {
        assert_eq!(layout.ggml_type_id(), id);
        assert_eq!(PackedStorageLayout::from_ggml_type_id(id), Some(layout));
        assert_eq!(layout.block_elements(), elements);
        assert_eq!(layout.block_bytes(), bytes);
        assert_eq!(
            layout.packed_bytes_for_elements(u64::from(elements)),
            Some(u64::from(bytes))
        );
    }
    assert_eq!(PackedStorageLayout::from_ggml_type_id(99), None);
    assert_eq!(
        PackedStorageLayout::Q5_0.packed_bytes_for_elements(960 * 960),
        Some((960 * 960 / 32) * 22)
    );
    assert_eq!(
        PackedStorageLayout::Q4_K.packed_bytes_for_elements(255),
        None
    );
}

#[test]
fn q4k_quantized_matmul_resolves_typed_packed_plan() {
    use crate::abi::PackedStorageLayout;
    let signature = quantized_matmul_signature(8, 16, 16);
    let contract = packed_layout_fixture(PackedStorageLayout::Q4_K);
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &contract,
        MirCollectionOp::TensorQuantizedMatMul,
        None,
    )
    .expect("Q4_K quantized matmul resolves a typed packed plan");
    let CollectionKernelPlan::QuantizedMatMul(quantized) = &plan else {
        panic!("Q4_K fixture must resolve QuantizedMatMul, got {plan:?}");
    };
    assert_eq!(
        (quantized.m, quantized.k, quantized.n, quantized.layout),
        (8, 16, 16, PackedStorageLayout::Q4_K)
    );
    assert_eq!(quantized.tile, MATMUL_TILE);
    plan.validate().expect("Q4_K packed plan validates");
}

#[test]
fn grouped_and_ssm_plans_resolve_typed_variants() {
    use crate::abi::PackedStorageLayout;
    let mut signature = quantized_matmul_signature(8, 16, 16);
    signature.extra_inputs[0].element_count = 4 * 16 * 16;
    let grouped = CollectionKernelPlan::from_contract(
        &signature,
        &CollectionOpContract::GroupedMatMul {
            dispatch_x: 2,
            dispatch_y: 1,
            m: 8,
            k: 16,
            n: 16,
            groups: 4,
            layout: PackedStorageLayout::Q4_K,
        },
        MirCollectionOp::TensorGroupedMatMul,
        None,
    )
    .expect("grouped matmul resolves");
    assert!(matches!(grouped, CollectionKernelPlan::GroupedMatMul(_)));
    grouped.validate().expect("grouped plan validates");

    let ssm_signature = transformer_signature(32, 32);
    let mut ssm_signature = ssm_signature;
    ssm_signature.workgroup = MirWorkgroupSize {
        x: TRANSPOSE_WORKGROUP_X,
        y: 1,
        z: 1,
    };
    ssm_signature.dispatch_size = MirKernelDispatchSize { x: 32, y: 1, z: 1 };
    let conv = CollectionKernelPlan::from_contract(
        &ssm_signature,
        &CollectionOpContract::SsmConv1d {
            length: 8,
            channels: 4,
            kernel_width: 3,
            layout: PackedStorageLayout::F32,
        },
        MirCollectionOp::TensorSsmConv1d,
        None,
    )
    .expect("ssm conv1d resolves");
    assert!(matches!(conv, CollectionKernelPlan::SsmConv1d(_)));
    conv.validate().expect("ssm conv1d validates");

    let scan = CollectionKernelPlan::from_contract(
        &ssm_signature,
        &CollectionOpContract::SsmScan {
            length: 8,
            state_dim: 4,
            layout: PackedStorageLayout::F32,
        },
        MirCollectionOp::TensorSsmScan,
        None,
    )
    .expect("ssm scan resolves");
    assert!(matches!(scan, CollectionKernelPlan::SsmScan(_)));
    scan.validate().expect("ssm scan validates");
}

// ── R-PACK-02A packed admission / dequant resolution ──────────────────────

fn section5_packed_layouts() -> [(crate::abi::PackedStorageLayout, PackedDequantKind, u32, u32); 8]
{
    use crate::abi::PackedStorageLayout;
    [
        (
            PackedStorageLayout::F16,
            PackedDequantKind::NativeF16Convert,
            1,
            2,
        ),
        (
            PackedStorageLayout::Bf16,
            PackedDequantKind::NativeBf16Convert,
            1,
            2,
        ),
        (PackedStorageLayout::Q8_0, PackedDequantKind::Block, 32, 34),
        (
            PackedStorageLayout::Q4_K,
            PackedDequantKind::Superblock,
            256,
            144,
        ),
        (
            PackedStorageLayout::Q5_K,
            PackedDequantKind::Superblock,
            256,
            176,
        ),
        (
            PackedStorageLayout::Q6_K,
            PackedDequantKind::Superblock,
            256,
            210,
        ),
        (PackedStorageLayout::Q5_0, PackedDequantKind::Block, 32, 22),
        (
            PackedStorageLayout::MXFP4,
            PackedDequantKind::ScaledE8M0Group,
            32,
            17,
        ),
    ]
}

#[test]
fn packed_dequant_resolution_binds_section5_layout_table() {
    use crate::abi::PackedStorageLayout;
    let f32 = PackedDequantResolution::for_layout(PackedStorageLayout::F32);
    assert_eq!(f32.class, PackedDequantKind::PassthroughF32);
    assert_eq!((f32.block_elements, f32.block_bytes), (1, 4));
    for (layout, class, elements, bytes) in section5_packed_layouts() {
        let resolved = PackedDequantResolution::for_layout(layout);
        assert_eq!(resolved.class, class, "{layout:?}");
        assert_eq!(resolved.block_elements, elements, "{layout:?}");
        assert_eq!(resolved.block_bytes, bytes, "{layout:?}");
        assert_eq!(
            PackedDequantResolution::from_ggml_type_id(layout.ggml_type_id()).expect("admitted"),
            resolved
        );
    }
}

#[test]
fn bf16_plan_seam_pins_width_mapping_both_directions() {
    // U5: the `PackedStorageLayout::Bf16 ↔ NumericWidth::BF16` correspondence
    // is named and pinned at the plan seam, not incidental — GGML-30, the
    // 2-byte width, and the NativeBf16Convert admission receipt hold in both
    // directions.
    use crate::abi::PackedStorageLayout;
    use radix_types::NumericWidth;

    // GGML-30 → Bf16 layout → NumericWidth::BF16 → 2-byte width.
    let layout = PackedStorageLayout::from_ggml_type_id(30)
        .expect("GGML type id 30 is an admitted packed layout");
    assert_eq!(layout, PackedStorageLayout::Bf16);
    assert_eq!(layout.numeric_width(), Some(NumericWidth::BF16));
    assert_eq!(layout.block_bytes(), 2);

    // NumericWidth::BF16 → Bf16 layout → GGML-30 → 2-byte width.
    let layout = PackedStorageLayout::from_numeric_width(NumericWidth::BF16)
        .expect("NumericWidth::BF16 is an admitted packed layout");
    assert_eq!(layout, PackedStorageLayout::Bf16);
    assert_eq!(layout.ggml_type_id(), 30);
    assert_eq!(layout.block_bytes(), 2);

    // Receipts row pinned as-is: Bf16 admits QuantizedMatMul via
    // NativeBf16Convert — no new kernels, Metal body unchanged.
    let plan = admit_quantized_matmul_from_ggml_type_id(
        30,
        MatMulShapes {
            m: 8,
            k: 16,
            n: 16,
            left_layout: crate::MirScalarLayout::F32,
            right_layout: crate::MirScalarLayout::F32,
        },
    )
    .expect("BF16 type id 30 admits QuantizedMatMul");
    let CollectionKernelPlan::QuantizedMatMul(quantized) = &plan else {
        panic!("BF16 must admit QuantizedMatMul, got {plan:?}");
    };
    assert_eq!(
        quantized.dequant_resolution().class,
        PackedDequantKind::NativeBf16Convert
    );
    assert_eq!(
        (
            quantized.dequant_resolution().block_elements,
            quantized.dequant_resolution().block_bytes
        ),
        (1, 2)
    );
}

#[test]
fn admit_quantized_matmul_falls_back_f32_to_tiled_matmul() {
    use crate::abi::PackedStorageLayout;
    let shapes = MatMulShapes {
        m: 8,
        k: 16,
        n: 16,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let plan = admit_quantized_matmul(PackedStorageLayout::F32, shapes);
    assert!(
        matches!(plan, CollectionKernelPlan::TiledMatMul(_)),
        "F32 must fall back to TiledMatMul, got {plan:?}"
    );
    plan.validate().expect("F32 fallback TiledMatMul validates");
}

#[test]
fn admit_quantized_matmul_resolves_every_packed_section5_layout() {
    let shapes = MatMulShapes {
        m: 8,
        k: 16,
        n: 16,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    for (layout, class, elements, bytes) in section5_packed_layouts() {
        let plan = admit_quantized_matmul(layout, shapes);
        let CollectionKernelPlan::QuantizedMatMul(quantized) = &plan else {
            panic!("{layout:?} must admit QuantizedMatMul, got {plan:?}");
        };
        assert_eq!(quantized.layout, layout);
        let dequant = quantized.dequant_resolution();
        assert_eq!(dequant.class, class);
        assert_eq!(
            (dequant.block_elements, dequant.block_bytes),
            (elements, bytes)
        );
        plan.validate().expect("packed QuantizedMatMul validates");
    }
}

#[test]
fn q4k_ggml_id_admits_quantized_matmul_with_superblock_dequant() {
    // First-failing-oracle surface: a Q4_K GGML id (12) on the pre-02A
    // f32-only substrate had no admission helper and no dequant
    // resolution — `admit_quantized_matmul_from_ggml_type_id` did not exist.
    let shapes = MatMulShapes {
        m: 8,
        k: 16,
        n: 16,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let plan = admit_quantized_matmul_from_ggml_type_id(12, shapes)
        .expect("Q4_K type id 12 is an admitted packed layout");
    let CollectionKernelPlan::QuantizedMatMul(quantized) = &plan else {
        panic!("Q4_K must admit QuantizedMatMul, got {plan:?}");
    };
    assert_eq!(quantized.layout, crate::abi::PackedStorageLayout::Q4_K);
    assert_eq!(
        quantized.dequant_resolution().class,
        PackedDequantKind::Superblock
    );
}

#[test]
fn f16_ggml_id_admits_quantized_matmul_with_native_f16_convert() {
    // W1-U1 first-failing oracle: GGML type id 1 (F16) is outside the
    // closed set today. After admission it must resolve NativeF16Convert
    // and a QuantizedMatMul plan (same shape as NativeBf16Convert).
    let shapes = MatMulShapes {
        m: 8,
        k: 16,
        n: 16,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let plan = admit_quantized_matmul_from_ggml_type_id(1, shapes)
        .expect("F16 type id 1 is an admitted packed layout");
    let CollectionKernelPlan::QuantizedMatMul(quantized) = &plan else {
        panic!("F16 must admit QuantizedMatMul, got {plan:?}");
    };
    assert_eq!(quantized.layout, crate::abi::PackedStorageLayout::F16);
    assert_eq!(
        quantized.dequant_resolution().class,
        PackedDequantKind::NativeF16Convert
    );
    assert_eq!(
        (
            quantized.dequant_resolution().block_elements,
            quantized.dequant_resolution().block_bytes
        ),
        (1, 2)
    );
}

#[test]
fn mxfp4_ggml_id_admits_quantized_matmul_with_scaled_e8m0_group() {
    // W5c-U7 first-failing oracle: GGML type id 39 (MXFP4) is outside the
    // closed set today. After admission it must resolve ScaledE8M0Group
    // (one E8M0 scale over a 32-element group) and a QuantizedMatMul plan.
    // Geometry is llama.cpp block_mxfp4: 32 elems / 17 bytes
    // (ggml.h:429 GGML_TYPE_MXFP4=39; ggml-common.h:214-219).
    let shapes = MatMulShapes {
        m: 8,
        k: 16,
        n: 16,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let plan = admit_quantized_matmul_from_ggml_type_id(39, shapes)
        .expect("MXFP4 type id 39 is an admitted packed layout");
    let CollectionKernelPlan::QuantizedMatMul(quantized) = &plan else {
        panic!("MXFP4 must admit QuantizedMatMul, got {plan:?}");
    };
    assert_eq!(quantized.layout, crate::abi::PackedStorageLayout::MXFP4);
    assert_eq!(
        quantized.dequant_resolution().class,
        PackedDequantKind::ScaledE8M0Group
    );
    assert_eq!(
        (
            quantized.dequant_resolution().block_elements,
            quantized.dequant_resolution().block_bytes
        ),
        (32, 17)
    );
}

#[test]
fn unrepresentable_ggml_type_id_fails_closed() {
    let shapes = MatMulShapes {
        m: 8,
        k: 16,
        n: 16,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let error = admit_quantized_matmul_from_ggml_type_id(99, shapes)
        .expect_err("GGML type id 99 is unrepresentable");
    assert_eq!(
        error,
        PackedAdmissionError::UnrepresentableLayout { ggml_type_id: 99 }
    );
    let gather_error = admit_quantized_gather_from_ggml_type_id(99, 960, 64, 9)
        .expect_err("GGML type id 99 is unrepresentable for gather");
    assert_eq!(
        gather_error,
        PackedAdmissionError::UnrepresentableLayout { ggml_type_id: 99 }
    );
}

#[test]
fn admit_quantized_gather_resolves_packed_embedding_tables() {
    for (layout, class, ..) in section5_packed_layouts() {
        let admitted = admit_quantized_gather(layout, 960, 64, 9);
        let AdmittedGather::Quantized(gather) = admitted else {
            panic!("{layout:?} embedding table must admit QuantizedGather");
        };
        assert_eq!(gather.layout, layout);
        assert_eq!(gather.dequant.class, class);
        assert_eq!(
            (gather.table_rows, gather.table_cols, gather.id_count),
            (960, 64, 9)
        );
    }
    let dense = admit_quantized_gather(crate::abi::PackedStorageLayout::F32, 960, 64, 9);
    assert!(
        matches!(dense, AdmittedGather::Dense(_)),
        "F32 embedding table stays Gather, got {dense:?}"
    );
}

#[test]
fn gather_plan_dim_carries_runtime_extents() {
    assert!(PlanDim::Runtime.is_runtime());
    assert_eq!(PlanDim::Literal(7).literal(), Some(7));

    let plan = CollectionKernelPlan::Gather(GatherPlan {
        table_rows: PlanDim::Runtime,
        table_cols: PlanDim::Literal(64),
        id_count: PlanDim::Runtime,
    });
    plan.validate()
        .expect("runtime gather extents are not zero dimensions");
}

/// A tensor type over the given dims (`IndexExpr::Unspecified` is a run-time
/// extent). The shape interns as a dim tuple, the form the plan facts read for
/// every rank (a declared rank is always a tuple).
fn tensor_ty_with_dims(types: &mut TypeTable, dims: Vec<IndexExpr>) -> MirType {
    let element = f32_ty(types);
    let dim_ids = dims
        .into_iter()
        .map(|dim| types.intern_index(dim))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(element.semantic_id(), shape)))
}

/// A row-gather kernel over the given table/ids/return extents:
/// `t = gather(table, ids)` with the args and return type carrying the
/// extents the recipe facts read.
fn gather_typed_function(
    types: &mut TypeTable,
    table_dims: Vec<IndexExpr>,
    ids_dims: Vec<IndexExpr>,
    return_dims: Vec<IndexExpr>,
) -> MirFunction {
    let table = tensor_ty_with_dims(types, table_dims);
    let ids = tensor_ty_with_dims(types, ids_dims);
    let output = tensor_ty_with_dims(types, return_dims);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: table,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: ids,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: table,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: ids,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: output,
                mutable: false,
                span: test_span(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(2))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorGather),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        ],
                        return_ty: output,
                    },
                },
                span: test_span(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(2),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: output,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// A gather signature whose input, ids extra input and output all carry the
/// runtime-shaped marker (the ABI form for a launch-bound extent).
fn gather_runtime_signature(types: &mut TypeTable) -> MirKernelSignature {
    let element_ty = f32_ty(types);
    let runtime_count = crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;
    let resource = |binding: u32,
                    role: MirKernelResourceRole,
                    access: MirKernelResourceAccess,
                    local: Option<MirLocalId>| MirKernelResource {
        group: 0,
        binding,
        kind: MirKernelResourceKind::StorageBuffer,
        role,
        access,
        element_ty,
        element_count: runtime_count,
        source_local: local,
        source_name: None,
        reduced_projection: None,
    };
    MirKernelSignature {
        function: MirFunctionId(0),
        input: resource(
            0,
            MirKernelResourceRole::Input,
            MirKernelResourceAccess::Read,
            Some(MirLocalId(0)),
        ),
        extra_inputs: vec![resource(
            1,
            MirKernelResourceRole::Input,
            MirKernelResourceAccess::Read,
            Some(MirLocalId(1)),
        )],
        output: resource(
            2,
            MirKernelResourceRole::Output,
            MirKernelResourceAccess::Write,
            None,
        ),
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: runtime_count,
            y: 1,
            z: 1,
        },
    }
}

/// Rule 11: the gather glyph takes run-time sizes. A table `[N, 64]` with `N`
/// bound at launch, ids `[T]` and output `[T, 64]` resolve a Gather plan
/// carrying `Runtime` rows and id count with the literal width 64, instead of
/// the `requires literal dimensions` refusal. A run-time table width resolves
/// too (runtime-vs-runtime on both paired extents), while a literal-vs-runtime
/// pair still fails closed.
#[test]
fn gather_runtime_facts_resolve_runtime_dims() {
    let mut types = TypeTable::new();
    let function = gather_typed_function(
        &mut types,
        vec![IndexExpr::Unspecified, IndexExpr::Literal(64)],
        vec![IndexExpr::Unspecified],
        vec![IndexExpr::Unspecified, IndexExpr::Literal(64)],
    );
    let signature = gather_runtime_signature(&mut types);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorGather,
    )
    .expect("run-time gather extents resolve a plan");
    let CollectionKernelPlan::Gather(gather) = &plan else {
        panic!("gather must map to a gather plan, got {plan:?}");
    };
    assert_eq!(
        gather.table_rows,
        PlanDim::Runtime,
        "a launch-bound table row count is a run-time extent"
    );
    assert_eq!(
        gather.table_cols,
        PlanDim::Literal(64),
        "a written row width stays literal"
    );
    assert_eq!(
        gather.id_count,
        PlanDim::Runtime,
        "the id count comes from the run-time return rows"
    );
    plan.validate().expect("the run-time gather plan validates");

    // A run-time table width too: the return width and the table width are
    // both run-time, and K7c is runtime-vs-runtime.
    let mut types = TypeTable::new();
    let function = gather_typed_function(
        &mut types,
        vec![IndexExpr::Unspecified, IndexExpr::Unspecified],
        vec![IndexExpr::Unspecified],
        vec![IndexExpr::Unspecified, IndexExpr::Unspecified],
    );
    let signature = gather_runtime_signature(&mut types);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorGather,
    )
    .expect("a run-time table width resolves a plan");
    let CollectionKernelPlan::Gather(gather) = &plan else {
        panic!("gather must map to a gather plan, got {plan:?}");
    };
    assert_eq!(
        (gather.table_rows, gather.table_cols, gather.id_count),
        (PlanDim::Runtime, PlanDim::Runtime, PlanDim::Runtime)
    );

    // A literal width paired with a run-time return width cannot be proven
    // and fails closed (rule 11: a mismatch is a recoverable error, never a
    // silent reshape).
    let mut types = TypeTable::new();
    let function = gather_typed_function(
        &mut types,
        vec![IndexExpr::Unspecified, IndexExpr::Literal(64)],
        vec![IndexExpr::Unspecified],
        vec![IndexExpr::Unspecified, IndexExpr::Unspecified],
    );
    let signature = gather_runtime_signature(&mut types);
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorGather,
        )
        .is_none(),
        "a literal-vs-runtime width pair must not resolve a plan"
    );
}

/// A row-gather kernel whose ids operand is the `tensor<u32, [n]>` element
/// type the recipe requires (the shared fixture keeps the table's `tf32`
/// element type on every operand).
fn gather_u32_ids_function(
    types: &mut TypeTable,
    table_dims: Vec<IndexExpr>,
    ids_dims: Vec<IndexExpr>,
    return_dims: Vec<IndexExpr>,
) -> MirFunction {
    let mut function = gather_typed_function(types, table_dims, ids_dims.clone(), return_dims);
    let ids_element = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let dim_ids = ids_dims
        .into_iter()
        .map(|dim| types.intern_index(dim))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    let ids = MirType::semantic(types.intern(Type::Tensor(ids_element, shape)));
    function.params[1].ty = ids;
    function.locals[1].ty = ids;
    function
}

/// AUD-GATHER-001: the signature builder marks each gather resource from the
/// extents that resource is sized from — the table input from `[V, D]`, the
/// output from `[T, D]`, the ids extra input from `[T]` — so the planner must
/// check each resource against its own extents, not demand that every
/// resource be launch-bound because one extent is. A run-time table with
/// literal ids (`[Runtime, 64]` / `[1]` / `[1, 64]`) and a literal table with
/// run-time ids (`[7, 64]` / `[Runtime]` / `[Runtime, 64]`) resolve the Gather
/// recipe end to end (signature builder → planner), while a shape
/// incompatibility still fails closed.
#[test]
fn gather_mixed_runtime_and_literal_extents_resolve_a_plan() {
    use crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT;

    // Run-time table rows, literal ids and literal output rows.
    let mut types = TypeTable::new();
    let function = gather_u32_ids_function(
        &mut types,
        vec![IndexExpr::Unspecified, IndexExpr::Literal(64)],
        vec![IndexExpr::Literal(1)],
        vec![IndexExpr::Literal(1), IndexExpr::Literal(64)],
    );
    let validation = MirValidationContext::new(&types);
    let signature = crate::device_program_plans::transformer_shape_signature(
        &function,
        MirCollectionOp::TensorGather,
        &validation,
    )
    .expect("a run-time table row count builds a gather signature");
    assert_eq!(signature.input.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);
    assert_eq!(
        signature.output.element_count, 64,
        "a literal [1, 64] output carries its literal count"
    );
    assert_eq!(
        signature.extra_inputs[0].element_count, 1,
        "a literal [1] ids vector carries its literal count"
    );
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorGather,
    )
    .expect("a run-time table with literal ids resolves a plan");
    let CollectionKernelPlan::Gather(gather) = &plan else {
        panic!("gather must map to a gather plan, got {plan:?}");
    };
    assert_eq!(
        (gather.table_rows, gather.table_cols, gather.id_count),
        (PlanDim::Runtime, PlanDim::Literal(64), PlanDim::Literal(1)),
        "each extent keeps its own literal-or-run-time fact"
    );
    plan.validate().expect("the mixed-extent plan validates");

    // Literal table, run-time ids.
    let mut types = TypeTable::new();
    let function = gather_u32_ids_function(
        &mut types,
        vec![IndexExpr::Literal(7), IndexExpr::Literal(64)],
        vec![IndexExpr::Unspecified],
        vec![IndexExpr::Unspecified, IndexExpr::Literal(64)],
    );
    let validation = MirValidationContext::new(&types);
    let signature = crate::device_program_plans::transformer_shape_signature(
        &function,
        MirCollectionOp::TensorGather,
        &validation,
    )
    .expect("a run-time ids count builds a gather signature");
    assert_eq!(signature.input.element_count, 7 * 64);
    assert_eq!(signature.output.element_count, RUNTIME_SHAPED_ELEMENT_COUNT);
    assert_eq!(
        signature.extra_inputs[0].element_count,
        RUNTIME_SHAPED_ELEMENT_COUNT
    );
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorGather,
    )
    .expect("a literal table with run-time ids resolves a plan");
    let CollectionKernelPlan::Gather(gather) = &plan else {
        panic!("gather must map to a gather plan, got {plan:?}");
    };
    assert_eq!(
        (gather.table_rows, gather.table_cols, gather.id_count),
        (PlanDim::Literal(7), PlanDim::Literal(64), PlanDim::Runtime)
    );
    plan.validate().expect("the mixed-extent plan validates");

    // A table width that disagrees with the return width is a shape
    // incompatibility: the paired extents cannot be proven equal and the
    // plan stays failed closed.
    let mut types = TypeTable::new();
    let function = gather_u32_ids_function(
        &mut types,
        vec![IndexExpr::Unspecified, IndexExpr::Literal(64)],
        vec![IndexExpr::Literal(1)],
        vec![IndexExpr::Literal(1), IndexExpr::Literal(32)],
    );
    let validation = MirValidationContext::new(&types);
    let signature = crate::device_program_plans::transformer_shape_signature(
        &function,
        MirCollectionOp::TensorGather,
        &validation,
    )
    .expect("the signature builder is not the paired-extent cross-check");
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorGather,
        )
        .is_none(),
        "a table/return width mismatch must not resolve a plan"
    );

    // A signature whose resource counts contradict the recipe extents is
    // still refused: a literal table with literal ids cannot produce a
    // run-time-shaped output.
    let mut types = TypeTable::new();
    let function = gather_u32_ids_function(
        &mut types,
        vec![IndexExpr::Literal(7), IndexExpr::Literal(64)],
        vec![IndexExpr::Literal(1)],
        vec![IndexExpr::Literal(1), IndexExpr::Literal(64)],
    );
    let validation = MirValidationContext::new(&types);
    let mut signature = crate::device_program_plans::transformer_shape_signature(
        &function,
        MirCollectionOp::TensorGather,
        &validation,
    )
    .expect("the literal gather signature resolves");
    signature.output.element_count = RUNTIME_SHAPED_ELEMENT_COUNT;
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorGather,
        )
        .is_none(),
        "a run-time-shaped output over literal extents must not resolve a plan"
    );
}

#[test]
fn from_contract_f32_quantized_matmul_falls_back_to_tiled() {
    use crate::abi::PackedStorageLayout;
    let signature = quantized_matmul_signature(8, 16, 16);
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &packed_layout_fixture(PackedStorageLayout::F32),
        MirCollectionOp::TensorQuantizedMatMul,
        None,
    )
    .expect("F32 quantized-matmul contract admits");
    assert!(
        matches!(plan, CollectionKernelPlan::TiledMatMul(_)),
        "from_contract must fall back F32 QuantizedMatMul to TiledMatMul, got {plan:?}"
    );
}

#[test]
fn quantized_matmul_plan_validate_rejects_f32_layout() {
    let plan = CollectionKernelPlan::QuantizedMatMul(QuantizedMatMulPlan {
        m: 8,
        k: 16,
        n: 16,
        layout: crate::abi::PackedStorageLayout::F32,
        tile: MATMUL_TILE,
        workgroup_x: MATMUL_TILE,
        workgroup_y: MATMUL_TILE,
    });
    assert_eq!(
        plan.validate()
            .expect_err("F32 QuantizedMatMulPlan must fail"),
        PlanValidationError::QuantizedMatMulF32Fallback
    );
}

// ── R-PACK-02B Q8_0 dequant + GEMM probe (GI2-1 goldens) ──────────────────

fn parse_hex_bytes(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0, "hex byte string must be even");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
        .collect()
}

fn parse_hex_f32s(words: &[&str]) -> Vec<f32> {
    words
        .iter()
        .map(|word| f32::from_bits(u32::from_str_radix(word, 16).expect("hex f32")))
        .collect()
}

#[test]
fn q8_0_block_dequant_matches_gi2_1_golden() {
    // GI2-1 `gi2-dequant-goldens.json`: real blk.0.attn_v.weight block 0.
    let packed =
        parse_hex_bytes("7f13b2ef0081cff90a0316cfc70aea841ae908f1330d18e1cdce2309ffed0f33eba8");
    let expected = parse_hex_f32s(&[
        "bd922c80", "bc7ede00", "00000000", "bdee0040", "bd37a780", "bbd1e400", "3c15ec00",
        "3b33e800", "3ca4ea00", "bd37a780", "bd55a380", "3c15ec00", "bca4ea00", "bde86100",
        "3cc2e600", "bcac6900", "3befe000", "bc60e200", "3d3f2680", "3c42e600", "3cb3e800",
        "bce86100", "bd3f2680", "bd3b6700", "3d032e80", "3c06ee00", "ba6fe000", "bc8e6d00",
        "3c60e200", "3d3f2680", "bc9d6b00", "bda4ea00",
    ]);
    let got = dequant_q8_0_block(&packed).expect("Q8_0 block is 34 bytes");
    assert_eq!(got.as_slice(), expected.as_slice());
    assert_eq!(
        dequant_packed_block(crate::abi::PackedStorageLayout::Q8_0, &packed).expect("Q8_0 arm"),
        expected
    );
}

#[test]
fn q8_0_packed_gemm_matches_cpu_reference_without_whole_model_f32() {
    // One Q8_0 block as B[K=32, N=1]; A is a single row of ones.
    // Dequant is block-wise — the fixture is 34 packed bytes, never a
    // whole-model F32 expansion.
    let packed =
        parse_hex_bytes("7f13b2ef0081cff90a0316cfc70aea841ae908f1330d18e1cdce2309ffed0f33eba8");
    let right = dequant_q8_0_block(&packed).expect("Q8_0 block");
    let left = [1.0f32; 32];
    let out = gemm_f32(&left, 1, 32, &right, 1);
    let expected: f32 = right.iter().sum();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].to_bits(), expected.to_bits());
    assert!(out[0].is_finite());
}

#[test]
fn q8_0_dequant_rejects_truncated_block() {
    let error = dequant_q8_0_block(&[0u8; 8]).expect_err("truncated Q8_0 must fail closed");
    assert_eq!(
        error,
        PackedDequantError::BlockBytesMismatch {
            expected: 34,
            actual: 8
        }
    );
}

#[test]
fn q4_k_dequant_rejects_truncated_block() {
    let error = dequant_q4_k_block(&[0u8; 8]).expect_err("truncated Q4_K must fail closed");
    assert_eq!(
        error,
        PackedDequantError::BlockBytesMismatch {
            expected: 144,
            actual: 8
        }
    );
}

#[test]
fn packed_dequant_q5k_accepts_full_superblock() {
    let values = dequant_packed_block(crate::abi::PackedStorageLayout::Q5_K, &[0u8; 176])
        .expect("Q5_K body landed in R-PACK-02F");
    assert_eq!(values.len(), 256);
}

// ── PGC-R4 simdgroup fast-path admission (plan facts) ─────────────────

/// The prefill `M = 36` family: multi-row f32, `k` a tile multiple, `m` a
/// partial band — the padded-FMA class the fast path exists for.
#[test]
fn simdgroup_admission_covers_the_prefill_partial_band_class() {
    let mut plan = matmul_plan();
    plan.m = 36;
    plan.k = 960;
    plan.n = 2560;
    let facts = simdgroup_matmul_admission(&plan).expect("prefill gate_up admits");
    assert_eq!(facts.k_tiles, 120);
    assert_eq!(facts.staging_slots, SIMDGROUP_MATMUL_STAGING_SLOTS);
    assert_eq!(facts.partial_slots, SIMDGROUP_MATMUL_PARTIAL_SLOTS);
    assert_eq!(facts.staging_slots, 128);
    assert_eq!(facts.partial_slots, 128);
    // Same admission for the other prefill shapes.
    plan.n = 320;
    assert!(simdgroup_matmul_admission(&plan).is_some());
    plan.k = 2560;
    plan.n = 960;
    assert!(simdgroup_matmul_admission(&plan).is_some());
}

/// The fast path refuses every shape outside its class: K tails (the
/// direct device `simdgroup_load` admits no K tail), tile-multiple `m`
/// (no padding class; keeps the frozen canonical scalar body), one-row
/// launches, non-f32 layouts, and workgroup drift.
#[test]
fn simdgroup_admission_refuses_non_class_shapes() {
    // Fixture: m=17 (partial band) but k=12 — a K tail refuses.
    let plan = matmul_plan();
    assert!(
        simdgroup_matmul_admission(&plan).is_none(),
        "k=12 carries a K tail and must keep the scalar body"
    );
    let mut square = matmul_plan();
    square.m = 16;
    square.k = 960;
    let facts = simdgroup_matmul_admission(&square)
        .expect("tile-multiple m (the square/tall full-band class) is admitted");
    assert_eq!(facts.k_tiles, 960 / u64::from(MATMUL_TILE));
    let mut one_row = square.clone();
    one_row.m = 1;
    one_row.workgroup_y = 1;
    assert!(simdgroup_matmul_admission(&one_row).is_none());
    let mut bf16 = square.clone();
    bf16.m = 36;
    bf16.right_layout = crate::MirScalarLayout::Bf16;
    assert!(simdgroup_matmul_admission(&bf16).is_none());
    let mut drifted = matmul_plan();
    drifted.m = 36;
    drifted.k = 960;
    drifted.workgroup_x = MATMUL_TILE - 1;
    assert!(simdgroup_matmul_admission(&drifted).is_none());
    // Vocab-scale output widths are outside the projection-class wave
    // (the frozen `prefill_lm_head_gemv` emission stays scalar).
    let mut vocab = matmul_plan();
    vocab.m = 36;
    vocab.k = 960;
    vocab.n = 49_152;
    assert!(
        simdgroup_matmul_admission(&vocab).is_none(),
        "the vocab-scale lm_head twin must keep the frozen scalar emission"
    );
}

/// PGC-R5: the row-reduction admission facts. The pinned family —
/// `prefill_rmsnorm`/`head_rmsnorm` at `[·, 960]` (a full simdgroup of
/// strided columns, float4-aligned width) and `prefill_causal_softmax` at
/// `[36, 36]` — admits the one-workgroup-per-row launch; the decode T=1
/// masked softmax (B1/B2-landed extents and body) refuses, and shapes
/// without a full lane count or without float4 alignment keep the frozen
/// per-element 1D contract.
#[test]
fn row_reduction_admission_covers_pinned_family_and_refuses_others() {
    use crate::{causal_softmax_row_reduction_admission, rmsnorm_row_reduction_admission};

    let prefill = rmsnorm_row_reduction_admission(36, 960).expect("prefill_rmsnorm admits");
    assert_eq!(prefill.rows, 36);
    assert_eq!(prefill.width, 960);
    assert_eq!(prefill.workgroup_x, ROW_REDUCTION_WORKGROUP_X);
    // head_rmsnorm is T=1 — the recipe is row-wise at any row count (the
    // decode rmsnorm twin shares the MIR shape and the family).
    let head = rmsnorm_row_reduction_admission(1, 960).expect("head_rmsnorm admits");
    assert_eq!(head.rows, 1);

    let softmax = causal_softmax_row_reduction_admission(36, 36).expect("prefill softmax admits");
    assert_eq!(softmax.rows, 36);
    assert_eq!(softmax.width, 36);

    // The decode T=1 masked softmax keeps the frozen per-element body.
    assert!(
        causal_softmax_row_reduction_admission(1, 76).is_none(),
        "decode_masked_softmax must keep the generic 1D contract"
    );
    // Below a full lane count: no row reduction.
    assert!(rmsnorm_row_reduction_admission(4, 8).is_none());
    assert!(causal_softmax_row_reduction_admission(9, 9).is_none());
    // float4-unaligned width: no vectorized scale pass, no admission.
    assert!(rmsnorm_row_reduction_admission(2, 34).is_none());
    // Zero extents fail closed.
    assert!(rmsnorm_row_reduction_admission(0, 960).is_none());
    assert!(rmsnorm_row_reduction_admission(36, 0).is_none());
}

// --- U1 head-axis: batched tiled-matmul selection (leading head axes) ---

/// A post-contract BATCHED matmul signature: workgroup `(8, 8, 1)`, the
/// batch-folded tile-grid dispatch (`dispatch_y = batch · ceil(M/8)`), and
/// operand buffers sized to the FULL tensor extents — `batch·M·K` left,
/// `right_batch·K·N` right, `batch·M·N` output. The shape the kernel ABI
/// produces for a head-axis batched matmul kernel.
fn batched_matmul_signature(
    shapes: MatMulShapes,
    batch: u64,
    right_batch: u64,
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
            element_count: batch * shapes.m * shapes.k,
            source_local: Some(MirLocalId(0)),
            source_name: None,
            reduced_projection: None,
        },
        extra_inputs: vec![MirKernelResource {
            group: 0,
            binding: 1,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty,
            element_count: right_batch * shapes.k * shapes.n,
            source_local: Some(MirLocalId(1)),
            source_name: None,
            reduced_projection: None,
        }],
        output: MirKernelResource {
            group: 0,
            binding: 2,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Output,
            access: MirKernelResourceAccess::Write,
            element_ty,
            element_count: batch * shapes.m * shapes.n,
            source_local: None,
            source_name: None,
            reduced_projection: None,
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

/// U1 head-axis done-when: the decode attention shape — rectangular
/// `[5,3,1,64] · [5,64,76] → [5,3,1,76]` (`query_seq` 1, `key_seq` 76, never
/// square) — selects a batch-extended `TiledMatMul` plan (batch 15 over the
/// `[5,3]` leading axes; the `[5]` kv-head share is the right batch) and
/// validates.
#[test]
fn batched_matmul_decode_shape_selects_batched_plan() {
    let shapes = MatMulShapes {
        m: 1,
        k: 64,
        n: 76,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    // dispatch_x = ceil(76/8) = 10; dispatch_y = 15 · ceil(1/8) = 15.
    let signature = batched_matmul_signature(shapes, 15, 5, 10, 15);
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 10,
            dispatch_y: 15,
        },
    };
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &contract,
        MirCollectionOp::TensorMatMul,
        Some(shapes),
    )
    .expect("the batched decode contract builds a plan");
    let CollectionKernelPlan::TiledMatMul(matmul) = &plan else {
        panic!("expected a tiled-matmul plan");
    };
    assert_eq!((matmul.m, matmul.k, matmul.n), (1, 64, 76));
    assert_eq!(
        (matmul.batch, matmul.right_batch),
        (15, 5),
        "the batch facts derive from the signature's operand extents"
    );
    assert_eq!(
        (matmul.workgroup_x, matmul.workgroup_y),
        (MATMUL_TILE, MATMUL_TILE)
    );
    plan.validate().expect("the batched decode plan validates");
}

/// U1 head-axis done-when: the prefill attention shape —
/// `[5,3,36,64] · [5,64,36] → [5,3,36,36]` — selects the batch-extended
/// plan with the batch folded into the row-block dispatch
/// (`dispatch_y = 15 · ceil(36/8) = 75`).
#[test]
fn batched_matmul_prefill_shape_selects_batched_plan() {
    let shapes = MatMulShapes {
        m: 36,
        k: 64,
        n: 36,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let signature = batched_matmul_signature(shapes, 15, 5, 5, 75);
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 5,
            dispatch_y: 75,
        },
    };
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &contract,
        MirCollectionOp::TensorMatMul,
        Some(shapes),
    )
    .expect("the batched prefill contract builds a plan");
    let CollectionKernelPlan::TiledMatMul(matmul) = &plan else {
        panic!("expected a tiled-matmul plan");
    };
    assert_eq!((matmul.m, matmul.k, matmul.n), (36, 64, 36));
    assert_eq!((matmul.batch, matmul.right_batch), (15, 5));
    plan.validate().expect("the batched prefill plan validates");
}

/// The batch derivation is a typed-fact law, not a fallback: a signature
/// whose operand extents contradict the shapes (an output count that is
/// not `batch·M·N`) yields no plan.
#[test]
fn batched_matmul_count_contradiction_returns_none() {
    let shapes = MatMulShapes {
        m: 1,
        k: 64,
        n: 76,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let mut signature = batched_matmul_signature(shapes, 15, 5, 10, 15);
    signature.output.element_count += 1;
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 10,
            dispatch_y: 15,
        },
    };
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::TensorMatMul,
            Some(shapes),
        )
        .is_none(),
        "an output extent that contradicts the batch law must fail closed"
    );
}

/// The dispatch law folds the batch into `dispatch_y`: a rank-2-shaped
/// `dispatch_y` under a batched signature yields no plan.
#[test]
fn batched_matmul_dispatch_without_batch_fold_returns_none() {
    let shapes = MatMulShapes {
        m: 36,
        k: 64,
        n: 36,
        left_layout: crate::MirScalarLayout::F32,
        right_layout: crate::MirScalarLayout::F32,
    };
    let signature = batched_matmul_signature(shapes, 15, 5, 5, 5);
    let contract = CollectionOpContract::MatMul {
        dispatch: MatMulDispatch::Literal {
            dispatch_x: 5,
            dispatch_y: 5,
        },
    };
    assert!(
        CollectionKernelPlan::from_contract(
            &signature,
            &contract,
            MirCollectionOp::TensorMatMul,
            Some(shapes),
        )
        .is_none(),
        "a batched signature requires the batch-folded dispatch_y"
    );
}

/// A hand-broken plan violating the batch law (`right_batch` does not divide
/// `batch`) fails validation with the typed `MatMulBatchLaw` error.
#[test]
fn batched_matmul_batch_law_validation_fails_closed() {
    let mut plan = matmul_plan();
    plan.batch = 15;
    plan.right_batch = 4;
    assert_eq!(
        matmul_plan_validation_error(&plan),
        PlanValidationError::MatMulBatchLaw {
            batch: 15,
            right_batch: 4
        }
    );
}

// --- U2 head-axis: batched additive-masked row-softmax (rank-3/4) ---

/// A `tf32[dims]` tensor type with static literal dims (any rank).
fn literal_dims_tensor_ty(types: &mut TypeTable, dims: &[u64]) -> MirType {
    let element = f32_ty(types);
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(element.semantic_id(), shape)))
}

/// The batched masked-softmax kernel body — the decode/prefill entry
/// shape: params `(scores, mask, out, lane)`, the additive idiom's
/// `TensorAdd(scores, mask)` temp, and one `TensorSoftmax(temp) → out`.
fn batched_masked_softmax_function(
    types: &mut TypeTable,
    score_dims: &[u64],
    mask_dims: &[u64],
) -> MirFunction {
    let scores_ty = literal_dims_tensor_ty(types, score_dims);
    let mask_ty = literal_dims_tensor_ty(types, mask_dims);
    let out_ty = literal_dims_tensor_ty(types, score_dims);
    let u32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let tensor_param = |local: MirLocalId, ty: MirType| MirParam {
        local,
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span: test_span(),
    };
    MirFunction {
        id: MirFunctionId(0),
        source: Some(radix_hir::DefId(1)),
        name: None,
        params: vec![
            tensor_param(MirLocalId(0), scores_ty),
            tensor_param(MirLocalId(1), mask_ty),
            tensor_param(MirLocalId(2), out_ty),
            tensor_param(MirLocalId(3), u32_ty),
        ],
        locals: Vec::new(),
        temps: vec![crate::MirTemp {
            id: crate::MirTempId(0),
            ty: scores_ty,
            span: test_span(),
        }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::temp(crate::MirTempId(0))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                                MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            ],
                            return_ty: scores_ty,
                        },
                    },
                    span: test_span(),
                },
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(2))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSoftmax),
                            args: vec![MirOperand::Temp(crate::MirTempId(0))],
                            return_ty: out_ty,
                        },
                    },
                    span: test_span(),
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: vacuum_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// The whole-function storage-buffer signature the ABI builds for the
/// batched masked-softmax entry: input `scores` (binding 0), extra input
/// `mask` (binding 1), output (binding 2), all at their FULL tensor
/// extents under the generic 1D contract (workgroup `(1,1,1)`, dispatch
/// = element count).
fn batched_masked_softmax_signature(
    types: &mut TypeTable,
    score_count: u64,
    mask_count: u64,
) -> MirKernelSignature {
    let element_ty = f32_ty(types);
    let resource = |binding: u32,
                    role: MirKernelResourceRole,
                    access: MirKernelResourceAccess,
                    count: u64,
                    local: Option<MirLocalId>| MirKernelResource {
        group: 0,
        binding,
        kind: MirKernelResourceKind::StorageBuffer,
        role,
        access,
        element_ty,
        element_count: count,
        source_local: local,
        source_name: None,
        reduced_projection: None,
    };
    MirKernelSignature {
        function: MirFunctionId(0),
        input: resource(
            0,
            MirKernelResourceRole::Input,
            MirKernelResourceAccess::Read,
            score_count,
            Some(MirLocalId(0)),
        ),
        extra_inputs: vec![resource(
            1,
            MirKernelResourceRole::Input,
            MirKernelResourceAccess::Read,
            mask_count,
            Some(MirLocalId(1)),
        )],
        output: resource(
            2,
            MirKernelResourceRole::Output,
            MirKernelResourceAccess::Write,
            score_count,
            Some(MirLocalId(2)),
        ),
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: score_count,
            y: 1,
            z: 1,
        },
    }
}

/// U2 head-axis done-when (decode): rectangular scores `[5,3,1,76]` with
/// the declared-length additive mask `[1,76]` (broadcast over heads) —
/// the last two axes are `query_seq` × `key_seq`, NOT square seq×seq —
/// resolve one batched row-softmax plan (batch 15 rows of width 76; mask
/// count 76) and validate.
#[test]
fn batched_softmax_decode_shape_selects_batched_plan() {
    let mut types = TypeTable::new();
    let function = batched_masked_softmax_function(&mut types, &[5, 3, 1, 76], &[1, 76]);
    let signature = batched_masked_softmax_signature(&mut types, 15 * 76, 76);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorSoftmax,
    )
    .expect("the batched decode softmax resolves a plan");
    let CollectionKernelPlan::RowSoftmax(softmax) = &plan else {
        panic!("expected the row-softmax recipe, got: {plan:?}");
    };
    let batch = softmax.batched.as_ref().expect("batched facts");
    assert_eq!(softmax.axis, 3, "the row axis stays the last axis");
    assert_eq!((batch.batch, batch.width), (15, 76));
    assert_eq!(
        batch.mask.as_ref().map(|mask| mask.count),
        Some(76),
        "the declared-length [1,76] mask is the broadcast operand"
    );
    plan.validate().expect("the batched decode plan validates");
}

/// U2 head-axis done-when (prefill): `[5,3,36,36]` scores with the causal
/// additive mask `[36,36]` resolve the batched plan (batch 15 rows of
/// width 36; mask count 1296 — the trailing [36,36] block).
#[test]
fn batched_softmax_prefill_shape_selects_batched_plan() {
    let mut types = TypeTable::new();
    let function = batched_masked_softmax_function(&mut types, &[5, 3, 36, 36], &[36, 36]);
    let signature = batched_masked_softmax_signature(&mut types, 15 * 36 * 36, 36 * 36);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorSoftmax,
    )
    .expect("the batched prefill softmax resolves a plan");
    let CollectionKernelPlan::RowSoftmax(softmax) = &plan else {
        panic!("expected the row-softmax recipe, got: {plan:?}");
    };
    let batch = softmax.batched.as_ref().expect("batched facts");
    // 15 head rows × 36 query rows = 540 rows, each softmaxing 36 keys.
    assert_eq!((batch.batch, batch.width), (15 * 36, 36));
    assert_eq!(
        batch.mask.as_ref().map(|mask| mask.count),
        Some(36 * 36),
        "the causal [36,36] mask is the broadcast operand"
    );
    plan.validate().expect("the batched prefill plan validates");
}

/// A rank-3/4 softmax over a plain local operand (no additive idiom)
/// resolves the batched facts with NO mask — the plain batched softmax.
#[test]
fn batched_softmax_plain_operand_resolves_without_mask() {
    let mut types = TypeTable::new();
    let mut function = batched_masked_softmax_function(&mut types, &[5, 3, 1, 76], &[1, 76]);
    // Drop the additive temp: softmax over the raw scores local.
    function.temps.clear();
    let scores_ty = function.params[0].ty;
    let softmax = &mut function.blocks[0].statements[1];
    let MirStatementKind::RuntimeCall { call, .. } = &mut softmax.kind else {
        panic!("fixture");
    };
    call.args = vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))];
    call.return_ty = scores_ty;
    function.blocks[0].statements.remove(0);
    let signature = batched_masked_softmax_signature(&mut types, 15 * 76, 76);
    let validation = MirValidationContext::new(&types);
    let plan = CollectionKernelPlan::from_transformer_op(
        &signature,
        &function,
        &validation,
        MirCollectionOp::TensorSoftmax,
    )
    .expect("the plain batched softmax resolves a plan");
    let CollectionKernelPlan::RowSoftmax(softmax) = &plan else {
        panic!("expected the row-softmax recipe, got: {plan:?}");
    };
    let batch = softmax.batched.as_ref().expect("batched facts");
    assert_eq!((batch.batch, batch.width), (15, 76));
    assert!(
        batch.mask.is_none(),
        "no additive idiom means no mask operand"
    );
    plan.validate().expect("the plain batched plan validates");
}

/// The mask broadcast law is a trailing-block law: a mask dim that
/// contradicts its aligned score dim (`[2,38]` against the trailing
/// `[1,76]`) fails closed — no plan, never a reshaped guess.
#[test]
fn batched_softmax_mask_broadcast_law_violation_returns_none() {
    let mut types = TypeTable::new();
    let function = batched_masked_softmax_function(&mut types, &[5, 3, 1, 76], &[2, 38]);
    let signature = batched_masked_softmax_signature(&mut types, 15 * 76, 2 * 38);
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorSoftmax,
        )
        .is_none(),
        "a mask that is not a trailing block of the score dims must fail closed"
    );
}

/// The signature's mask resource must carry the mask's own element count
/// (the A3 cross-check) — a contradicting extent yields no plan.
#[test]
fn batched_softmax_mask_count_contradiction_returns_none() {
    let mut types = TypeTable::new();
    let function = batched_masked_softmax_function(&mut types, &[5, 3, 1, 76], &[1, 76]);
    // The signature lies about the mask buffer size (152 ≠ 76).
    let signature = batched_masked_softmax_signature(&mut types, 15 * 76, 152);
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorSoftmax,
        )
        .is_none(),
        "a mask resource extent that contradicts the mask dims must fail closed"
    );
}

/// Rank admission is 3/4 (the settled GEA3 shapes); rank-5 still fails
/// closed.
#[test]
fn batched_softmax_rank5_still_fails_closed() {
    let mut types = TypeTable::new();
    let function = batched_masked_softmax_function(&mut types, &[2, 2, 2, 2, 2], &[1, 2]);
    let signature = batched_masked_softmax_signature(&mut types, 32, 2);
    let validation = MirValidationContext::new(&types);
    assert!(
        CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorSoftmax,
        )
        .is_none(),
        "rank-5 softmax stays outside the admitted surface"
    );
}

/// A hand-broken batched plan whose mask count does not divide
/// `batch · width` fails validation with the typed `RowSoftmaxBatchLaw`.
#[test]
fn batched_softmax_batch_law_validation_fails_closed() {
    let plan = CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan {
        dims: RowOpDims::Literal {
            rows: 15,
            width: 76,
        },
        axis: 3,
        batched: Some(RowSoftmaxBatch {
            batch: 15,
            width: 76,
            mask: Some(RowSoftmaxMask { count: 75 }),
        }),
    });
    let error = plan
        .validate()
        .expect_err("a non-dividing mask count must fail validation");
    assert_eq!(
        error,
        PlanValidationError::RowSoftmaxBatchLaw {
            batch: 15,
            width: 76,
            mask_count: 75,
        }
    );
}

#[test]
fn batched_softmax_expanded_mask_requires_trailing_mapping() {
    for (score_dims, mask_dims, resource_count, admitted) in [
        (vec![5, 3, 1, 76], vec![1, 76], 76, true),
        (vec![5, 3, 36, 36], vec![36, 36], 1296, true),
        (vec![5, 3, 2, 76], vec![3, 76], 228, false),
        (vec![5, 3, 1, 76], vec![1, 76], 152, false),
    ] {
        let mut types = TypeTable::new();
        let mut function = batched_masked_softmax_function(&mut types, &score_dims, &mask_dims);
        let scores_ty = function.params[0].ty;
        function.temps.push(crate::MirTemp {
            id: crate::MirTempId(1),
            ty: scores_ty,
            span: test_span(),
        });
        let MirStatementKind::RuntimeCall { call, .. } = &mut function.blocks[0].statements[0].kind
        else {
            unreachable!()
        };
        call.args[1] = MirOperand::Temp(crate::MirTempId(1));
        function.blocks[0].statements.insert(
            0,
            MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::temp(crate::MirTempId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorExpand),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                        ],
                        return_ty: scores_ty,
                    },
                },
                span: test_span(),
            },
        );
        let signature = batched_masked_softmax_signature(
            &mut types,
            score_dims.iter().product(),
            resource_count,
        );
        let validation = MirValidationContext::new(&types);
        let plan = CollectionKernelPlan::from_transformer_op(
            &signature,
            &function,
            &validation,
            MirCollectionOp::TensorSoftmax,
        );
        assert_eq!(plan.is_some(), admitted, "{score_dims:?} <- {mask_dims:?}");
        if admitted {
            assert_eq!(
                softmax_additive_mask_locals(
                    &function,
                    &MirOperand::Temp(crate::MirTempId(0)),
                    &validation,
                ),
                Some((MirLocalId(0), MirLocalId(1), resource_count))
            );
            plan.unwrap().validate().unwrap();
        }
    }
}

// --- D-29: literal_const_operand writer scan ---

/// A one-block function with a single temp and no locals, for the
/// `literal_const_operand` writer-scan tests.
fn literal_const_scan_function(
    statements: Vec<MirStatement>,
    terminator: MirTerminatorKind,
) -> MirFunction {
    let ty = MirType::semantic(radix_types::TypeId(0));
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: vec![crate::MirTemp {
            id: crate::MirTempId(0),
            ty,
            span: test_span(),
        }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: terminator,
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

/// `place ← float` as a constant `Assign` statement.
fn literal_const_assign(place: MirPlace, value: f64) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place,
            value: crate::MirValue {
                id: crate::MirValueId(0),
                kind: crate::MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(value))),
                ty: MirType::semantic(radix_types::TypeId(0)),
                span: test_span(),
            },
        },
        span: test_span(),
    }
}

/// D-29: a temp assigned a constant but also written as a `Call`
/// destination is ambiguous and must fail closed.
#[test]
fn literal_const_operand_rejects_temp_with_second_writer() {
    let temp = crate::MirTempId(0);
    let function = literal_const_scan_function(
        vec![
            literal_const_assign(MirPlace::temp(temp), 1.0),
            MirStatement {
                kind: MirStatementKind::Call {
                    destination: Some(MirPlace::temp(temp)),
                    callee: crate::MirCallee::Function(MirFunctionId(0)),
                    args: Vec::new(),
                },
                span: test_span(),
            },
        ],
        MirTerminatorKind::Return(None),
    );
    assert_eq!(
        literal_const_operand(&function, &MirOperand::Temp(temp)),
        None
    );
}

/// The control case: exactly one constant `Assign` still resolves.
#[test]
fn literal_const_operand_resolves_single_assign() {
    let temp = crate::MirTempId(0);
    let function = literal_const_scan_function(
        vec![literal_const_assign(MirPlace::temp(temp), 1.0)],
        MirTerminatorKind::Return(None),
    );
    assert!(matches!(
        literal_const_operand(&function, &MirOperand::Temp(temp)),
        Some(MirConstant::Float(value)) if value.to_bits() == 1.0f64.to_bits()
    ));
}

/// A literal dim product that does not fit `u64` fails closed (no plan)
/// instead of saturating to a near-`u64::MAX` element count that the plan
/// would carry as a real buffer size.
#[test]
fn dims_product_overflow_fails_closed() {
    assert_eq!(dims_product(&[u64::MAX, 2]), None);
    assert_eq!(dims_product(&[2u64.pow(32), 2u64.pow(32), 2]), None);
    assert_eq!(dims_product(&[3, 5]), Some(15));
    // Zero still flows on: the per-plan zero validators name it.
    assert_eq!(dims_product(&[3, 0, 5]), Some(0));
}

/// The axis-removed product has the same fail-closed overflow law.
#[test]
fn dims_without_axis_product_overflow_fails_closed() {
    assert_eq!(dims_without_axis_product(&[u64::MAX, 2, 1], 2), None);
    assert_eq!(dims_without_axis_product(&[3, 5], 0), Some(5));
    assert_eq!(dims_without_axis_product(&[3, 5], 1), Some(3));
    assert_eq!(dims_without_axis_product(&[3, 0, 5], 2), Some(0));
    // An out-of-range axis stays fail-closed.
    assert_eq!(dims_without_axis_product(&[3, 5], 2), None);
}

/// The literal and run-time row-op extent forms are distinct values, and the
/// run-time form is distinct per class. A copy of either compares equal to
/// its source, so a plan can be cloned without changing its identity.
#[test]
fn rowop_dims_literal_and_runtime_are_distinct() {
    let literal = RowOpDims::Literal { rows: 2, width: 5 };
    let generic = RowOpDims::Runtime {
        class: RowOpClass::Generic1D,
    };
    let mapped = RowOpDims::Runtime {
        class: RowOpClass::RowMapped,
    };
    assert_ne!(literal, generic);
    assert_ne!(generic, mapped);

    let literal_copy = literal;
    let generic_copy = generic;
    let mapped_copy = mapped;
    assert_eq!(literal, literal_copy);
    assert_eq!(generic, generic_copy);
    assert_eq!(mapped, mapped_copy);
}
