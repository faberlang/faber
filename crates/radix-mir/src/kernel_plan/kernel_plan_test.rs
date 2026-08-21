use super::build::{matmul_barrier_schedule, reduction_barrier_schedule};
use super::*;
use crate::abi::{
    CollectionOpContract, MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess,
    MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature, MirWorkgroupSize,
    ReducedProjection,
};
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirRuntimeCall,
    MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};

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
            Some(MatMulShapes { m: 4, k: 4, n: 4 }),
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
    CollectionKernelPlan::RowSoftmax(RowSoftmaxPlan { axis: 1 })
        .validate()
        .expect("row-softmax plan validates");
    CollectionKernelPlan::LayerNormalization(LayerNormalizationPlan { axis: 1 })
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
    matmul_signature(MatMulShapes { m, k, n }, n.div_ceil(8), m.div_ceil(8))
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

fn section5_packed_layouts() -> [(
    crate::abi::PackedStorageLayout,
    PackedDequantClass,
    u32,
    u32,
); 8] {
    use crate::abi::PackedStorageLayout;
    [
        (
            PackedStorageLayout::F16,
            PackedDequantClass::NativeF16Convert,
            1,
            2,
        ),
        (
            PackedStorageLayout::Bf16,
            PackedDequantClass::NativeBf16Convert,
            1,
            2,
        ),
        (PackedStorageLayout::Q8_0, PackedDequantClass::Block, 32, 34),
        (
            PackedStorageLayout::Q4_K,
            PackedDequantClass::Superblock,
            256,
            144,
        ),
        (
            PackedStorageLayout::Q5_K,
            PackedDequantClass::Superblock,
            256,
            176,
        ),
        (
            PackedStorageLayout::Q6_K,
            PackedDequantClass::Superblock,
            256,
            210,
        ),
        (PackedStorageLayout::Q5_0, PackedDequantClass::Block, 32, 22),
        (
            PackedStorageLayout::MXFP4,
            PackedDequantClass::ScaledE8M0Group,
            32,
            17,
        ),
    ]
}

#[test]
fn packed_dequant_resolution_binds_section5_layout_table() {
    use crate::abi::PackedStorageLayout;
    let f32 = PackedDequantResolution::for_layout(PackedStorageLayout::F32);
    assert_eq!(f32.class, PackedDequantClass::PassthroughF32);
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
fn admit_quantized_matmul_falls_back_f32_to_tiled_matmul() {
    use crate::abi::PackedStorageLayout;
    let shapes = MatMulShapes { m: 8, k: 16, n: 16 };
    let plan = admit_quantized_matmul(PackedStorageLayout::F32, shapes);
    assert!(
        matches!(plan, CollectionKernelPlan::TiledMatMul(_)),
        "F32 must fall back to TiledMatMul, got {plan:?}"
    );
    plan.validate().expect("F32 fallback TiledMatMul validates");
}

#[test]
fn admit_quantized_matmul_resolves_every_packed_section5_layout() {
    let shapes = MatMulShapes { m: 8, k: 16, n: 16 };
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
    let shapes = MatMulShapes { m: 8, k: 16, n: 16 };
    let plan = admit_quantized_matmul_from_ggml_type_id(12, shapes)
        .expect("Q4_K type id 12 is an admitted packed layout");
    let CollectionKernelPlan::QuantizedMatMul(quantized) = &plan else {
        panic!("Q4_K must admit QuantizedMatMul, got {plan:?}");
    };
    assert_eq!(quantized.layout, crate::abi::PackedStorageLayout::Q4_K);
    assert_eq!(
        quantized.dequant_resolution().class,
        PackedDequantClass::Superblock
    );
}

#[test]
fn f16_ggml_id_admits_quantized_matmul_with_native_f16_convert() {
    // W1-U1 first-failing oracle: GGML type id 1 (F16) is outside the
    // closed set today. After admission it must resolve NativeF16Convert
    // and a QuantizedMatMul plan (same shape as NativeBf16Convert).
    let shapes = MatMulShapes { m: 8, k: 16, n: 16 };
    let plan = admit_quantized_matmul_from_ggml_type_id(1, shapes)
        .expect("F16 type id 1 is an admitted packed layout");
    let CollectionKernelPlan::QuantizedMatMul(quantized) = &plan else {
        panic!("F16 must admit QuantizedMatMul, got {plan:?}");
    };
    assert_eq!(quantized.layout, crate::abi::PackedStorageLayout::F16);
    assert_eq!(
        quantized.dequant_resolution().class,
        PackedDequantClass::NativeF16Convert
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
    let shapes = MatMulShapes { m: 8, k: 16, n: 16 };
    let plan = admit_quantized_matmul_from_ggml_type_id(39, shapes)
        .expect("MXFP4 type id 39 is an admitted packed layout");
    let CollectionKernelPlan::QuantizedMatMul(quantized) = &plan else {
        panic!("MXFP4 must admit QuantizedMatMul, got {plan:?}");
    };
    assert_eq!(quantized.layout, crate::abi::PackedStorageLayout::MXFP4);
    assert_eq!(
        quantized.dequant_resolution().class,
        PackedDequantClass::ScaledE8M0Group
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
    let shapes = MatMulShapes { m: 8, k: 16, n: 16 };
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

fn gi2_first_block_fixture(layout: &str) -> (Vec<u8>, Vec<f32>) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/factory/gpu-inference-gguf/evidence/gi2-dequant-goldens.json"
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("GI2-1 goldens")).expect("json");
    let fixture = json["block_fixtures"]
        .as_array()
        .expect("block_fixtures")
        .iter()
        .find(|block| block["type"] == layout)
        .unwrap_or_else(|| panic!("GI2-1 goldens must include a {layout} block"));
    let packed = parse_hex_bytes(fixture["bytes_hex"].as_str().expect("bytes_hex"));
    let expected = fixture["output_hex"]
        .as_array()
        .expect("output_hex")
        .iter()
        .map(|word| {
            f32::from_bits(u32::from_str_radix(word.as_str().expect("hex word"), 16).expect("hex"))
        })
        .collect();
    (packed, expected)
}

#[test]
fn q4_k_block_dequant_matches_gi2_1_golden() {
    let (packed, expected) = gi2_first_block_fixture("Q4_K");
    assert_eq!(packed.len(), 144);
    assert_eq!(expected.len(), 256);
    let got = dequant_q4_k_block(&packed).expect("Q4_K block is 144 bytes");
    assert_eq!(got.as_slice(), expected.as_slice());
    assert_eq!(
        dequant_packed_block(crate::abi::PackedStorageLayout::Q4_K, &packed).expect("Q4_K arm"),
        expected
    );
}

#[test]
fn q4_k_packed_gemm_matches_cpu_reference_without_whole_model_f32() {
    // One Q4_K superblock as B[K=256, N=1]; A is a single row of ones.
    let (packed, _) = gi2_first_block_fixture("Q4_K");
    let right = dequant_q4_k_block(&packed).expect("Q4_K block");
    let left = [1.0f32; 256];
    let out = gemm_f32(&left, 1, 256, &right, 1);
    let expected: f32 = right.iter().sum();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].to_bits(), expected.to_bits());
    assert!(out[0].is_finite());
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
