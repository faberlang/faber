use super::*;
use crate::abi::{
    MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind,
    MirKernelResourceRole, MirWorkgroupSize,
};
use crate::{MirFunctionId, MirLocalId, MirType};
use radix_types::{NumericWidth, Primitive, TypeTable};

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
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
