use super::*;
use crate::abi::{
    CollectionOpContract, MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess,
    MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature, MirKernelWorkgroupCount,
    MirWorkgroupSize,
};
use crate::device_program::{
    BufferId, BufferIdentity, BufferLifetime, BufferVersion, DeviceProgram, DeviceProgramLifetime,
    DeviceResource, KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit, ResultBuffer,
};
use crate::kernel_plan::{
    MatMulShapes, MatMulSharedMemory, OobPaddingPolicy, ReduceOp, SharedMemoryLayout,
    MATMUL_SHARED_A_BUFFER_NAME, MATMUL_SHARED_B_BUFFER_NAME,
};
use crate::layout::MirTensorStorageLayout;
use crate::{
    MirBlock, MirBlockId, MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirLocalId,
    MirOperand, MirParam, MirParamMode, MirPlace, MirRuntimeCall, MirStatement, MirStatementKind,
    MirTerminator, MirTerminatorKind, MirType,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};

fn test_span() -> Span {
    Span::default()
}

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn tensor_ty(types: &mut TypeTable, count: u64) -> MirType {
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let len = types.intern_index(IndexExpr::Literal(count));
    MirType::semantic(types.intern(Type::Tensor(f32_ty, len)))
}

/// A rank-2 `tf32[M, N]` tensor type with static literal dims (the matmul
/// operand shape the contract/plan passes read).
fn rank2_tensor_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32_ty, shape)))
}

// -----------------------------------------------------------------------
// Signature fixtures (the post-contract shapes `apply_collection_op_contract`
// produces — the shape `CollectionKernelPlan::from_contract` requires).
// -----------------------------------------------------------------------

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
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 8, y: 8, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: dispatch_x,
            y: dispatch_y,
            z: 1,
        },
    }
}

// -----------------------------------------------------------------------
// Function fixtures (kernel bodies the typed plan mapping scans).
// -----------------------------------------------------------------------

fn reduction_function(types: &mut TypeTable, length: u64) -> MirFunction {
    let tensor = tensor_ty(types, length);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: tensor,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: tensor,
            mutable: false,
            span: test_span(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Sum),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                        return_ty: tensor,
                    },
                },
                span: test_span(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(1),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

fn matmul_function(types: &mut TypeTable) -> MirFunction {
    let left = rank2_tensor_ty(types, [17, 12]);
    let right = rank2_tensor_ty(types, [12, 33]);
    let out = rank2_tensor_ty(types, [17, 33]);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: left,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: right,
                mode: MirParamMode::Owned,
                span: test_span(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: left,
                mutable: false,
                span: test_span(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: right,
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
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        ],
                        return_ty: out,
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
        return_ty: out,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

fn elementwise_function(types: &mut TypeTable) -> MirFunction {
    let tensor = tensor_ty(types, 4);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: tensor,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: tensor,
            mutable: false,
            span: test_span(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(0),
                )))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

// -----------------------------------------------------------------------
// Device-program fixtures (kernel units the launch descriptors derive from).
// -----------------------------------------------------------------------

fn identity(id: u32, name: &str, role: BufferRole) -> BufferIdentity {
    BufferIdentity {
        id: BufferId(id),
        name: name.to_owned(),
        role,
        storage: MirTensorStorageLayout::DeviceHandle,
        lifetime: BufferLifetime::PerProgram,
    }
}

fn version(number: u32, ty: MirType, count: u64) -> BufferVersion {
    BufferVersion {
        version: number,
        element_ty: ty,
        element_count: count,
    }
}

fn resource(
    identity: BufferIdentity,
    version: BufferVersion,
    binding: u32,
    access: MirKernelResourceAccess,
) -> DeviceResource {
    DeviceResource {
        buffer: identity,
        version,
        binding: Binding { group: 0, binding },
        access,
    }
}

/// The carried plan a materializer stores on a kernel unit, built through the
/// same typed mapping the emitters cross-check against.
fn carried_reduction_plan(length: u64, workgroup_x: u32, partials: u64) -> CollectionKernelPlan {
    let signature = reduction_signature(length, workgroup_x, partials);
    let contract = CollectionOpContract::Reduction {
        length,
        workgroup_x,
        partials,
    };
    plan_for_collection_op(&signature, &contract, MirCollectionOp::Sum, None)
}

fn elementwise_device_program() -> DeviceProgram {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let input = identity(1, "x", BufferRole::Input);
    let output = identity(2, "y", BufferRole::Output);
    let kernel = KernelUnit {
        function: MirFunctionId(0),
        entry: "addita".to_owned(),
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![
            resource(
                input.clone(),
                version(1, element_ty, 4),
                0,
                MirKernelResourceAccess::Read,
            ),
            resource(
                output.clone(),
                version(1, element_ty, 4),
                1,
                MirKernelResourceAccess::Write,
            ),
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 8, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 4, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        },
    };
    DeviceProgram {
        kernels: vec![kernel],
        launches: vec![LaunchUnit {
            id: LaunchId(1),
            kernel_index: 0,
        }],
        lifetime: DeviceProgramLifetime::SingleRun,
        results: vec![ResultBuffer {
            buffer: output,
            version: version(1, element_ty, 4),
            role: BufferRole::Output,
            produced_by: LaunchId(1),
        }],
    }
}

// -----------------------------------------------------------------------
// Plan mapping (typed op → plan).
// -----------------------------------------------------------------------

#[test]
fn sum_maps_to_tree_reduction() {
    let signature = reduction_signature(1024, 256, 4);
    let contract = CollectionOpContract::Reduction {
        length: 1024,
        workgroup_x: 256,
        partials: 4,
    };
    let plan = plan_for_collection_op(&signature, &contract, MirCollectionOp::Sum, None);
    let CollectionKernelPlan::TreeReduction(reduction) = &plan else {
        panic!("Sum must map to a tree-reduction plan, got {plan:?}");
    };
    assert_eq!(reduction.op, ReduceOp::Sum);
    assert_eq!(reduction.length, 1024);
    assert_eq!(reduction.workgroup_x, 256);
    assert_eq!(reduction.partials, 4);
}

#[test]
fn tensor_mean_maps_to_tree_reduction() {
    let signature = reduction_signature(1024, 256, 4);
    let contract = CollectionOpContract::Reduction {
        length: 1024,
        workgroup_x: 256,
        partials: 4,
    };
    let plan = plan_for_collection_op(&signature, &contract, MirCollectionOp::TensorMean, None);
    let CollectionKernelPlan::TreeReduction(reduction) = &plan else {
        panic!("TensorMean must map to a tree-reduction plan, got {plan:?}");
    };
    assert_eq!(reduction.op, ReduceOp::Mean);
}

#[test]
fn matmul_maps_to_tiled_matmul() {
    let shapes = MatMulShapes {
        m: 17,
        k: 12,
        n: 33,
    };
    let signature = matmul_signature(shapes, 5, 3);
    let contract = CollectionOpContract::MatMul {
        dispatch_x: 5,
        dispatch_y: 3,
    };
    let plan = plan_for_collection_op(
        &signature,
        &contract,
        MirCollectionOp::TensorMatMul,
        Some(shapes),
    );
    let CollectionKernelPlan::TiledMatMul(matmul) = &plan else {
        panic!("TensorMatMul must map to a tiled-matmul plan, got {plan:?}");
    };
    assert_eq!(matmul.m, 17);
    assert_eq!(matmul.k, 12);
    assert_eq!(matmul.n, 33);
    assert_eq!(
        matmul.shared_memory.shared_a.buffer_name,
        MATMUL_SHARED_A_BUFFER_NAME
    );
    assert_eq!(
        matmul.shared_memory.shared_b.buffer_name,
        MATMUL_SHARED_B_BUFFER_NAME
    );
}

#[test]
fn elementwise_op_maps_to_elementwise_plan() {
    // A non-recipe op keeps the no-recipe fallback even under a reduction
    // contract: the mapping never invents a recipe for an elementwise op.
    let signature = reduction_signature(1024, 256, 4);
    let contract = CollectionOpContract::Reduction {
        length: 1024,
        workgroup_x: 256,
        partials: 4,
    };
    let plan = plan_for_collection_op(&signature, &contract, MirCollectionOp::TensorRelu, None);
    assert_eq!(plan, CollectionKernelPlan::Elementwise);
}

// -----------------------------------------------------------------------
// Function-level plan re-derivation (the emitters' cross-check input).
// -----------------------------------------------------------------------

#[test]
fn kernel_plan_for_function_derives_reduction_plan() {
    let mut types = TypeTable::new();
    let function = reduction_function(&mut types, 1024);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("reduction function facts resolve")
        .expect("reduction function carries a recipe plan");
    // A carried plan built by the same typed mapping agrees exactly.
    let carried = carried_reduction_plan(1024, 256, 4);
    assert!(carried_plan_agrees(&carried, Some(&derived)));
    assert_eq!(carried, derived);
}

#[test]
fn kernel_plan_for_function_derives_matmul_plan() {
    let mut types = TypeTable::new();
    let function = matmul_function(&mut types);
    let shapes = MatMulShapes {
        m: 17,
        k: 12,
        n: 33,
    };
    let signature = matmul_signature(shapes, 5, 3);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("matmul function facts resolve")
        .expect("matmul function carries a recipe plan");
    assert!(matches!(derived, CollectionKernelPlan::TiledMatMul(_)));
}

#[test]
fn kernel_plan_for_function_is_none_for_elementwise_function() {
    let mut types = TypeTable::new();
    let function = elementwise_function(&mut types);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let derived = kernel_plan_for_function(&function, &signature, &validation)
        .expect("elementwise facts resolve");
    assert!(derived.is_none());
    // An elementwise kernel unit carries the no-recipe plan; the agreement
    // predicate matches it to the `None` derivation.
    assert!(carried_plan_agrees(
        &CollectionKernelPlan::Elementwise,
        derived.as_ref()
    ));
    // A carried recipe on a function without one must fail the cross-check.
    let carried = carried_reduction_plan(1024, 256, 4);
    assert!(!carried_plan_agrees(&carried, derived.as_ref()));
}

#[test]
fn carried_plan_agrees_rejects_recipe_mismatch() {
    let mut types = TypeTable::new();
    let function = reduction_function(&mut types, 1024);
    let signature = reduction_signature(1024, 256, 4);
    let validation = MirValidationContext::new(&types);
    let derived =
        kernel_plan_for_function(&function, &signature, &validation).expect("facts resolve");
    // A no-recipe carried plan on a function that carries a recipe is a
    // contradiction (and vice versa).
    assert!(!carried_plan_agrees(
        &CollectionKernelPlan::Elementwise,
        derived.as_ref()
    ));
}

// -----------------------------------------------------------------------
// Launch-descriptor construction.
// -----------------------------------------------------------------------

#[test]
fn elementwise_kernel_descriptor_carries_typed_facts() {
    let program = elementwise_device_program();
    let descriptors = launch_descriptors(&program);
    assert_eq!(descriptors.len(), 1);
    let descriptor = &descriptors[0];
    assert_eq!(descriptor.kernel_index, 0);
    assert_eq!(descriptor.entry, "addita");
    assert_eq!(descriptor.recipe, RecipeKind::Elementwise);
    assert_eq!(descriptor.workgroup, MirWorkgroupSize { x: 8, y: 1, z: 1 });
    assert_eq!(
        descriptor.dispatch,
        MirKernelDispatchSize { x: 4, y: 1, z: 1 }
    );
    assert_eq!(
        descriptor.workgroup_count,
        MirKernelWorkgroupCount { x: 1, y: 1, z: 1 }
    );
    assert_eq!(descriptor.shared_memory_bytes, 0);
    assert_eq!(descriptor.slots.len(), 2);
    assert_eq!(
        descriptor.slots[0].binding,
        Binding {
            group: 0,
            binding: 0
        }
    );
    assert_eq!(descriptor.slots[0].element_count, 4);
    assert_eq!(descriptor.slots[0].role, BufferRole::Input);
    assert_eq!(
        descriptor.slots[1].binding,
        Binding {
            group: 0,
            binding: 1
        }
    );
    assert_eq!(descriptor.slots[1].role, BufferRole::Output);
}

#[test]
fn reduction_kernel_descriptor_reports_recipe_and_shared_memory() {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let input = identity(1, "summa_in", BufferRole::Input);
    let output = identity(2, "summa_out", BufferRole::Output);
    let kernel = KernelUnit {
        function: MirFunctionId(0),
        entry: "summa".to_owned(),
        plan: carried_reduction_plan(1024, 256, 4),
        resources: vec![
            resource(
                input,
                version(1, element_ty, 1024),
                0,
                MirKernelResourceAccess::Read,
            ),
            resource(
                output,
                version(1, element_ty, 4),
                1,
                MirKernelResourceAccess::Write,
            ),
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 256, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize {
                x: 1024,
                y: 1,
                z: 1,
            },
            workgroup_count: MirKernelWorkgroupCount { x: 4, y: 1, z: 1 },
        },
    };
    let descriptor = KernelLaunchDescriptor::from_kernel_unit(&kernel, 0);
    assert_eq!(descriptor.entry, "summa");
    assert_eq!(descriptor.recipe, RecipeKind::TreeReduction);
    assert_eq!(descriptor.shared_memory_bytes, 256 * 4);
    assert_eq!(
        descriptor.workgroup_count,
        MirKernelWorkgroupCount { x: 4, y: 1, z: 1 }
    );
    assert_eq!(descriptor.slots.len(), 2);
}

#[test]
fn matmul_kernel_descriptor_reports_tiled_matmul_recipe() {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let left = identity(1, "left", BufferRole::Input);
    let right = identity(2, "right", BufferRole::Input);
    let out = identity(3, "out", BufferRole::Output);
    let tile = 8;
    let tile_slots = tile * tile;
    let matmul_plan = crate::kernel_plan::MatMulPlan {
        m: 17,
        k: 12,
        n: 33,
        tile,
        workgroup_x: tile,
        workgroup_y: tile,
        shared_memory: MatMulSharedMemory {
            shared_a: SharedMemoryLayout {
                element_byte_width: 4,
                slot_count: tile_slots,
                buffer_name: MATMUL_SHARED_A_BUFFER_NAME.to_owned(),
            },
            shared_b: SharedMemoryLayout {
                element_byte_width: 4,
                slot_count: tile_slots,
                buffer_name: MATMUL_SHARED_B_BUFFER_NAME.to_owned(),
            },
        },
        barriers: Vec::new(),
        oob_padding: OobPaddingPolicy::ZeroFill,
    };
    let kernel = KernelUnit {
        function: MirFunctionId(0),
        entry: "multiplico".to_owned(),
        plan: CollectionKernelPlan::TiledMatMul(matmul_plan),
        resources: vec![
            resource(
                left,
                version(1, element_ty, 17 * 12),
                0,
                MirKernelResourceAccess::Read,
            ),
            resource(
                right,
                version(1, element_ty, 12 * 33),
                1,
                MirKernelResourceAccess::Read,
            ),
            resource(
                out,
                version(1, element_ty, 17 * 33),
                2,
                MirKernelResourceAccess::Write,
            ),
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 8, y: 8, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 5, y: 3, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 5, y: 3, z: 1 },
        },
    };
    let descriptor = KernelLaunchDescriptor::from_kernel_unit(&kernel, 0);
    assert_eq!(descriptor.recipe, RecipeKind::TiledMatMul);
    // Two `tile × tile` operand tiles: 2 × (64 slots × 4 bytes).
    assert_eq!(descriptor.shared_memory_bytes, 2 * 64 * 4);
    assert_eq!(descriptor.slots.len(), 3);
}
