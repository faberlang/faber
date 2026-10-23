use super::*;
use crate::abi::{
    CollectionOpContract, MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess,
    MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature, MirKernelWorkgroupCount,
    MirWorkgroupSize,
};
use crate::device::MirCompanionDerivativeKind;
use crate::device_semantics::{
    DependencyEdge, DeviceSemantics, InitializationFact, InitializationPolicy,
    LosslessMirCompanionEntry, MirCompanionSelectedInput, MirCompanionSelectedOutput,
    ObservationFact, SemanticValue, SemanticValueId, SemanticValueOrigin, ValueBinding,
    ValueGeneration,
};
use crate::kernel_plan::{CollectionKernelPlan, ReduceOp};
use crate::layout::MirTensorStorageLayout;
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirLocalId,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
};
use radix_hir::DefId;
use radix_lexer::Span;
use radix_types::{NumericWidth, Primitive, TypeId, TypeTable};

// -----------------------------------------------------------------------
// Fixtures
// -----------------------------------------------------------------------

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn identity(id: u32, name: &str, role: BufferRole, lifetime: BufferLifetime) -> BufferIdentity {
    BufferIdentity {
        id: BufferId(id),
        name: name.to_owned(),
        role,
        storage: MirTensorStorageLayout::DeviceHandle,
        lifetime,
    }
}

fn version(number: u32, ty: MirType, count: u64) -> BufferVersion {
    BufferVersion {
        version: number,
        element_ty: ty,
        element_count: count,
        shape: None,
        reduced_projection: None,
    }
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

/// A generic elementwise signature: workgroup `(ws, 1, 1)`, dispatch
/// `(length, 1, 1)`, output sized to `length`.
fn elementwise_signature(length: u64, workgroup_x: u32) -> MirKernelSignature {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    MirKernelSignature {
        function: MirFunctionId(1),
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
            element_count: length,
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
/// element counts and an `M·N` output.
fn matmul_signature(m: u64, k: u64, n: u64) -> MirKernelSignature {
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
            element_count: m * k,
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
            element_count: k * n,
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
            element_count: m * n,
            source_local: None,
            source_name: None,
            reduced_projection: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 8, y: 8, z: 1 },
        dispatch_size: MirKernelDispatchSize {
            x: n.div_ceil(8),
            y: m.div_ceil(8),
            z: 1,
        },
    }
}

/// Build the tree-reduction plan for a matching reduction signature (TB5:
/// `MirCollectionOp → CollectionKernelPlan`).
fn reduction_plan(
    signature: &MirKernelSignature,
    length: u64,
    workgroup_x: u32,
    partials: u64,
) -> CollectionKernelPlan {
    CollectionKernelPlan::from_contract(
        signature,
        &CollectionOpContract::Reduction {
            length,
            workgroup_x,
            partials,
        },
        MirCollectionOp::Sum,
        None,
    )
    .expect("reduction signature matches the contract")
}

/// A minimal MIR function whose body contains one collection runtime call,
/// used to exercise `collection_op_workgroup_count` through the launch-plan
/// constructor.
fn body_function(op: MirCollectionOp) -> MirFunction {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(op),
                        args: Vec::new(),
                        return_ty: element_ty,
                    },
                },
                span: Span::default(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: element_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

/// A reduction kernel unit with the given input/output buffer identities.
fn reduction_kernel(
    signature: &MirKernelSignature,
    plan: CollectionKernelPlan,
    input: BufferIdentity,
    output: BufferIdentity,
) -> KernelUnit {
    let input_resource = DeviceResource::from_kernel_resource(
        input,
        version(1, signature.input.element_ty, signature.input.element_count),
        &signature.input,
    )
    .expect("input resource maps from signature");
    let output_resource = DeviceResource::from_kernel_resource(
        output,
        version(
            1,
            signature.output.element_ty,
            signature.output.element_count,
        ),
        &signature.output,
    )
    .expect("output resource maps from signature");
    KernelUnit {
        function: signature.function,
        entry: "reduction_kernel".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan,
        resources: vec![input_resource, output_resource],
        // dispatch 16 / workgroup 8 -> grid 2 for the standard fixture.
        launch: KernelLaunchPlan {
            workgroup: signature.workgroup,
            dispatch_size: signature.dispatch_size,
            workgroup_count: MirKernelWorkgroupCount { x: 2, y: 1, z: 1 },
        },
    }
}

fn reduction_kernel_default(
    signature: &MirKernelSignature,
    plan: CollectionKernelPlan,
) -> KernelUnit {
    reduction_kernel(
        signature,
        plan,
        identity(
            0,
            "reduction.input",
            BufferRole::Input,
            BufferLifetime::PerStep,
        ),
        identity(
            1,
            "reduction.output",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
    )
}

/// The standard single-kernel reduction program: 16 f32 elements reduced by a
/// power-of-two workgroup of 8 lanes into 2 partial slots.
fn reduction_program() -> DeviceProgram {
    let signature = reduction_signature(16, 8, 2);
    let plan = reduction_plan(&signature, 16, 8, 2);
    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program
        .kernels
        .push(reduction_kernel_default(&signature, plan));
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    program
}

// -----------------------------------------------------------------------
// Structure: the frozen four-field shape and ordered kernels
// -----------------------------------------------------------------------

#[test]
fn program_fields_and_ordered_kernels() {
    let mut program = reduction_program();

    // Append an elementwise kernel as the second declaration.
    let ew_signature = elementwise_signature(16, 8);
    let ew_input = identity(
        2,
        "elementwise.input",
        BufferRole::Input,
        BufferLifetime::PerStep,
    );
    let ew_output = identity(
        3,
        "elementwise.output",
        BufferRole::Output,
        BufferLifetime::ObservationPoint,
    );
    let ew_kernel = KernelUnit {
        function: ew_signature.function,
        entry: "elementwise_kernel".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![
            DeviceResource::from_kernel_resource(
                ew_input,
                version(
                    1,
                    ew_signature.input.element_ty,
                    ew_signature.input.element_count,
                ),
                &ew_signature.input,
            )
            .expect("elementwise input maps"),
            DeviceResource::from_kernel_resource(
                ew_output,
                version(
                    1,
                    ew_signature.output.element_ty,
                    ew_signature.output.element_count,
                ),
                &ew_signature.output,
            )
            .expect("elementwise output maps"),
        ],
        launch: KernelLaunchPlan {
            workgroup: ew_signature.workgroup,
            dispatch_size: ew_signature.dispatch_size,
            workgroup_count: MirKernelWorkgroupCount { x: 2, y: 1, z: 1 },
        },
    };
    program.kernels.push(ew_kernel);
    program.launches.push(LaunchUnit {
        id: LaunchId(1),
        kernel_index: 1,
    });

    assert_eq!(program.kernels.len(), 2);
    assert_eq!(program.kernels[0].entry, "reduction_kernel");
    assert_eq!(program.kernels[1].entry, "elementwise_kernel");
    assert_eq!(program.launches.len(), 2);
    assert_eq!(program.launches[0].kernel_index, 0);
    assert_eq!(program.launches[1].kernel_index, 1);
    assert_eq!(program.lifetime, DeviceProgramLifetime::SingleRun);
    assert!(program.results.is_empty());
    assert_eq!(program.validate(), Ok(()));
}

#[test]
fn reuses_collection_kernel_plan() {
    let signature = reduction_signature(16, 8, 2);
    let plan = reduction_plan(&signature, 16, 8, 2);
    let kernel = reduction_kernel_default(&signature, plan);

    // The plan carries the live recipe built from the frozen ABI contract.
    match &kernel.plan {
        CollectionKernelPlan::TreeReduction(reduction) => {
            assert_eq!(reduction.op, ReduceOp::Sum);
            assert_eq!(reduction.length, 16);
            assert_eq!(reduction.workgroup_x, 8);
            assert_eq!(reduction.partials, 2);
            assert_eq!(reduction.shared_memory.slot_count, 8);
            assert_eq!(
                reduction.shared_memory.buffer_name,
                crate::kernel_plan::REDUCTION_SHARED_BUFFER_NAME,
                "shared memory name is pinned by the recipe"
            );
        }
        other => panic!("expected a tree-reduction plan, got {other:?}"),
    }
    assert_eq!(kernel.function, MirFunctionId(0));
    assert_eq!(kernel.resources.len(), 2);

    // Typed resources map from the live signature resources.
    let input = &kernel.resources[0];
    assert_eq!(
        input.binding,
        Binding {
            group: 0,
            binding: 0
        }
    );
    assert_eq!(input.buffer.role, BufferRole::Input);
    assert_eq!(input.buffer.storage, MirTensorStorageLayout::DeviceHandle);
    assert_eq!(input.buffer.lifetime, BufferLifetime::PerStep);
    assert_eq!(input.access, MirKernelResourceAccess::Read);
    assert_eq!(input.version.element_count, 16);
    let output = &kernel.resources[1];
    assert_eq!(
        output.binding,
        Binding {
            group: 0,
            binding: 1
        }
    );
    assert_eq!(output.buffer.role, BufferRole::Output);
    assert_eq!(output.buffer.lifetime, BufferLifetime::ObservationPoint);
    assert_eq!(output.access, MirKernelResourceAccess::Write);
    assert_eq!(output.version.element_count, 2);
}

#[test]
fn elementwise_plan_accepted() {
    let signature = elementwise_signature(16, 8);
    let kernel = KernelUnit {
        function: signature.function,
        entry: "elementwise_kernel".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![
            DeviceResource::from_kernel_resource(
                identity(0, "x", BufferRole::Input, BufferLifetime::PerStep),
                version(1, signature.input.element_ty, signature.input.element_count),
                &signature.input,
            )
            .expect("input maps"),
            DeviceResource::from_kernel_resource(
                identity(1, "y", BufferRole::Output, BufferLifetime::ObservationPoint),
                version(
                    1,
                    signature.output.element_ty,
                    signature.output.element_count,
                ),
                &signature.output,
            )
            .expect("output maps"),
        ],
        launch: KernelLaunchPlan {
            workgroup: signature.workgroup,
            dispatch_size: signature.dispatch_size,
            workgroup_count: MirKernelWorkgroupCount { x: 2, y: 1, z: 1 },
        },
    };
    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels.push(kernel);
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    assert_eq!(program.validate(), Ok(()));
}

// -----------------------------------------------------------------------
// Live-surface adapters (the constructor building blocks)
// -----------------------------------------------------------------------

#[test]
fn launch_plan_from_signature_matmul() {
    // The shared ABI rule: matmul launches the tile grid verbatim.
    let signature = matmul_signature(16, 16, 16);
    let function = body_function(MirCollectionOp::TensorMatMul);
    let plan = KernelLaunchPlan::from_signature_and_function(&signature, &function);
    assert_eq!(plan.workgroup, MirWorkgroupSize { x: 8, y: 8, z: 1 });
    assert_eq!(
        plan.dispatch_size,
        MirKernelDispatchSize { x: 2, y: 2, z: 1 }
    );
    assert_eq!(
        plan.workgroup_count,
        MirKernelWorkgroupCount { x: 2, y: 2, z: 1 }
    );
}

#[test]
fn launch_plan_from_signature_quantized_matmul_is_verbatim_tile_grid() {
    // Packed QMM uses the same tile-grid dispatch as TensorMatMul. The
    // generic ceil(dispatch/workgroup) path under-launches (1×1 instead
    // of 2×2 here) and leaves the last output row zero.
    let signature = matmul_signature(16, 16, 16);
    let function = body_function(MirCollectionOp::TensorQuantizedMatMul);
    let plan = KernelLaunchPlan::from_signature_and_function(&signature, &function);
    assert_eq!(plan.workgroup, MirWorkgroupSize { x: 8, y: 8, z: 1 });
    assert_eq!(
        plan.dispatch_size,
        MirKernelDispatchSize { x: 2, y: 2, z: 1 }
    );
    assert_eq!(
        plan.workgroup_count,
        MirKernelWorkgroupCount { x: 2, y: 2, z: 1 }
    );
}

#[test]
fn launch_plan_from_signature_reduction() {
    // Everything else: ceil(dispatch / workgroup) per axis.
    let signature = reduction_signature(16, 8, 2);
    let function = body_function(MirCollectionOp::Sum);
    let plan = KernelLaunchPlan::from_signature_and_function(&signature, &function);
    assert_eq!(plan.workgroup, MirWorkgroupSize { x: 8, y: 1, z: 1 });
    assert_eq!(
        plan.dispatch_size,
        MirKernelDispatchSize { x: 16, y: 1, z: 1 }
    );
    assert_eq!(
        plan.workgroup_count,
        MirKernelWorkgroupCount { x: 2, y: 1, z: 1 }
    );
}

// -----------------------------------------------------------------------
// Generic 1-D workgroup policy
// -----------------------------------------------------------------------

fn plan_launch(
    signature: &MirKernelSignature,
    plan: &CollectionKernelPlan,
    op: MirCollectionOp,
) -> KernelLaunchPlan {
    KernelLaunchPlan::from_signature_plan_and_function(signature, plan, &body_function(op))
}

#[test]
fn a_generic_elementwise_kernel_launches_a_real_workgroup_not_one_thread() {
    // The ABI default is workgroup 1x1x1 with dispatch = element count, which
    // a host launches as one thread per block (17.5 GB/s against ~596 GB/s on
    // an RTX 5070). The launch plan widens it; the dispatch extent is the
    // element count and the grid is the rounded-up cover.
    let signature = elementwise_signature(1000, 1);
    let launch = plan_launch(
        &signature,
        &CollectionKernelPlan::Elementwise,
        MirCollectionOp::TensorAdd,
    );
    assert_eq!(launch.workgroup, MirWorkgroupSize { x: 128, y: 1, z: 1 });
    assert_eq!(
        launch.dispatch_size,
        MirKernelDispatchSize {
            x: 1000,
            y: 1,
            z: 1
        }
    );
    assert_eq!(
        launch.workgroup_count,
        MirKernelWorkgroupCount { x: 8, y: 1, z: 1 },
        "8 * 128 covers 1000; the kernel's own index guard handles the tail"
    );
    assert_eq!(
        launch.work_items_x(&CollectionKernelPlan::Elementwise),
        1000,
        "the work-item extent stays the element count, not the rounded-up grid"
    );
}

/// Bodies index in 32 bits and address with the index as a signed offset. At 2³¹
/// elements or more the launched index passes `i32::MAX`, so the wide geometry
/// would address the wrong elements where the one-thread geometry was refused by
/// the host's grid limit. The kernel keeps its signature workgroup past the bound.
#[test]
fn a_generic_kernel_too_large_for_32_bit_indexing_keeps_one_thread_per_block() {
    let widest = GENERIC_1D_MAX_ELEMENTS;
    let signature = elementwise_signature(widest, 1);
    let launch = plan_launch(
        &signature,
        &CollectionKernelPlan::Elementwise,
        MirCollectionOp::TensorAdd,
    );
    assert_eq!(launch.workgroup, MirWorkgroupSize { x: 128, y: 1, z: 1 });
    let last_index = launch.workgroup_count.x * 128 - 1;
    assert!(
        i32::try_from(last_index).is_ok(),
        "the last launched thread's index {last_index} must fit a signed 32-bit offset"
    );

    for elements in [widest + 1, 1 << 31, 1 << 32, 5_000_000_000] {
        let signature = elementwise_signature(elements, 1);
        let launch = plan_launch(
            &signature,
            &CollectionKernelPlan::Elementwise,
            MirCollectionOp::TensorAdd,
        );
        assert_eq!(
            launch.workgroup,
            MirWorkgroupSize { x: 1, y: 1, z: 1 },
            "{elements} elements must not take the wide geometry"
        );
    }
}

#[test]
fn an_exact_multiple_needs_no_tail_workgroup() {
    let signature = elementwise_signature(16_777_216, 1);
    let launch = plan_launch(
        &signature,
        &CollectionKernelPlan::Elementwise,
        MirCollectionOp::TensorAdd,
    );
    assert_eq!(launch.workgroup_count.x, 131_072);
}

#[test]
fn a_single_work_item_stays_a_single_thread() {
    let signature = elementwise_signature(1, 1);
    let launch = plan_launch(
        &signature,
        &CollectionKernelPlan::Elementwise,
        MirCollectionOp::TensorAdd,
    );
    assert_eq!(launch.workgroup, MirWorkgroupSize { x: 1, y: 1, z: 1 });
    assert_eq!(launch.workgroup_count.x, 1);
}

#[test]
fn the_per_element_row_softmax_body_widens_too() {
    let signature = elementwise_signature(16_384, 1);
    let softmax = CollectionKernelPlan::RowSoftmax(crate::kernel_plan::RowSoftmaxPlan {
        dims: crate::kernel_plan::RowOpDims::Literal {
            rows: 16_384,
            width: 1,
        },
        axis: 1,
        batched: None,
    });
    let launch = plan_launch(&signature, &softmax, MirCollectionOp::TensorSoftmax);
    assert_eq!(launch.workgroup.x, 128);
    assert_eq!(launch.workgroup_count.x, 128);
    assert_eq!(launch.work_items_x(&softmax), 16_384);
}

#[test]
fn recipes_with_their_own_contracts_keep_their_geometry() {
    // A contract-sized workgroup is never overridden.
    let signature = elementwise_signature(1000, 32);
    let launch = plan_launch(
        &signature,
        &CollectionKernelPlan::Elementwise,
        MirCollectionOp::TensorAdd,
    );
    assert_eq!(launch.workgroup.x, 32);
    assert_eq!(launch.workgroup_count.x, 32, "ceil(1000 / 32)");

    // A plan outside the admitted list keeps its signature workgroup, and its
    // work-item extent stays the established workgroups x lanes product.
    let batched = CollectionKernelPlan::RowSoftmax(crate::kernel_plan::RowSoftmaxPlan {
        dims: crate::kernel_plan::RowOpDims::Literal {
            rows: 64,
            width: 256,
        },
        axis: 3,
        batched: Some(crate::kernel_plan::RowSoftmaxBatch {
            batch: 64,
            width: 256,
            mask: None,
        }),
    });
    let reduction = reduction_signature(1000, 8, 125);
    let launch = plan_launch(&reduction, &batched, MirCollectionOp::TensorSoftmax);
    assert_eq!(launch.workgroup.x, 8);
    assert_eq!(launch.work_items_x(&batched), 125 * 8);
    let plain_widened = elementwise_signature(1000, 1);
    let launch = plan_launch(&plain_widened, &batched, MirCollectionOp::TensorSoftmax);
    assert_eq!(
        launch.workgroup.x, 1,
        "an unadmitted plan is never widened, whatever its signature says"
    );

    // A matmul tile grid is verbatim.
    let matmul = matmul_signature(16, 16, 16);
    let launch = plan_launch(
        &matmul,
        &CollectionKernelPlan::Elementwise,
        MirCollectionOp::TensorMatMul,
    );
    assert_eq!(launch.workgroup, MirWorkgroupSize { x: 8, y: 8, z: 1 });
}

#[test]
fn a_two_dimensional_dispatch_is_not_widened() {
    let mut signature = elementwise_signature(64, 1);
    signature.dispatch_size = MirKernelDispatchSize { x: 8, y: 8, z: 1 };
    let launch = plan_launch(
        &signature,
        &CollectionKernelPlan::Elementwise,
        MirCollectionOp::TensorAdd,
    );
    assert_eq!(launch.workgroup, MirWorkgroupSize { x: 1, y: 1, z: 1 });
}

#[test]
fn the_batched_row_softmax_variant_is_not_admitted() {
    // Its body is not covered by the block-size-independence evidence.
    assert!(!is_generic_1d_plan(&CollectionKernelPlan::RowSoftmax(
        crate::kernel_plan::RowSoftmaxPlan {
            dims: crate::kernel_plan::RowOpDims::Literal { rows: 8, width: 64 },
            axis: 3,
            batched: Some(crate::kernel_plan::RowSoftmaxBatch {
                batch: 8,
                width: 64,
                mask: None,
            }),
        }
    )));
    assert!(is_generic_1d_plan(&CollectionKernelPlan::Elementwise));
}

#[test]
fn resources_map_from_kernel_resource() {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let resource = MirKernelResource {
        group: 1,
        binding: 2,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty,
        element_count: 16,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    };
    let buffer = identity(0, "x", BufferRole::Input, BufferLifetime::PerStep);
    let mapped =
        DeviceResource::from_kernel_resource(buffer.clone(), version(1, element_ty, 16), &resource)
            .expect("storage-buffer resource maps");
    assert_eq!(
        mapped.binding,
        Binding {
            group: 1,
            binding: 2
        }
    );
    assert_eq!(mapped.access, MirKernelResourceAccess::Read);
    assert_eq!(mapped.buffer, buffer);

    // The runtime-extent count channel is not a device data buffer.
    let extent = MirKernelResource {
        group: 1,
        binding: 3,
        kind: MirKernelResourceKind::RuntimeExtent,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty,
        element_count: 1,
        source_local: None,
        source_name: Some("runtime_extent".to_owned()),
        reduced_projection: None,
    };
    assert!(
        DeviceResource::from_kernel_resource(buffer, version(1, element_ty, 1), &extent).is_none()
    );
}

#[test]
fn buffer_role_abi_mapping() {
    assert_eq!(
        BufferRole::from_abi_role(MirKernelResourceRole::Input, MirKernelResourceAccess::Read),
        Some(BufferRole::Input)
    );
    assert_eq!(
        BufferRole::from_abi_role(
            MirKernelResourceRole::Output,
            MirKernelResourceAccess::Write
        ),
        Some(BufferRole::Output)
    );
    // Read-write on either ABI role is an in-out buffer.
    assert_eq!(
        BufferRole::from_abi_role(
            MirKernelResourceRole::Input,
            MirKernelResourceAccess::ReadWrite
        ),
        Some(BufferRole::InOut)
    );
    assert_eq!(
        BufferRole::from_abi_role(
            MirKernelResourceRole::Output,
            MirKernelResourceAccess::ReadWrite
        ),
        Some(BufferRole::InOut)
    );
    // Coherent pairs cannot express a program role.
    assert_eq!(
        BufferRole::from_abi_role(MirKernelResourceRole::Input, MirKernelResourceAccess::Write),
        None
    );
    assert_eq!(
        BufferRole::from_abi_role(MirKernelResourceRole::Output, MirKernelResourceAccess::Read),
        None
    );

    // Round-trip through the ABI pair reproduces the role.
    for role in [BufferRole::Input, BufferRole::Output, BufferRole::InOut] {
        let pair = (role.abi_role(), role.abi_access());
        assert_eq!(BufferRole::from_abi_role(pair.0, pair.1), Some(role));
    }
}

// -----------------------------------------------------------------------
// Multi-output results and shape-changing versioning
// -----------------------------------------------------------------------

#[test]
fn multi_output_results() {
    let mut program = reduction_program();

    // Second kernel: a second reduction over a larger tensor.
    let signature2 = reduction_signature(32, 8, 4);
    let plan2 = reduction_plan(&signature2, 32, 8, 4);
    program.kernels.push(reduction_kernel(
        &signature2,
        plan2,
        identity(
            2,
            "second.input",
            BufferRole::Input,
            BufferLifetime::PerStep,
        ),
        identity(
            3,
            "second.output",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
    ));
    program.launches.push(LaunchUnit {
        id: LaunchId(1),
        kernel_index: 1,
    });

    program.results.push(ResultBuffer {
        buffer: identity(
            1,
            "reduction.output",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
        version: version(1, program.kernels[0].resources[1].version.element_ty, 2),
        role: BufferRole::Output,
        produced_by: LaunchId(0),
        cadence: ObservationCadence::PerStep,
    });
    program.results.push(ResultBuffer {
        buffer: identity(
            3,
            "second.output",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
        version: version(1, program.kernels[1].resources[1].version.element_ty, 4),
        role: BufferRole::Output,
        produced_by: LaunchId(1),
        cadence: ObservationCadence::PerStep,
    });

    assert_eq!(program.validate(), Ok(()));
    let registry = program.buffer_registry();
    assert_eq!(registry.buffers.len(), 4);
    assert_eq!(
        registry.buffer(BufferId(1)).expect("first result").versions[0].produced_by,
        vec![LaunchId(0)]
    );
    assert_eq!(
        registry
            .buffer(BufferId(3))
            .expect("second result")
            .versions[0]
            .produced_by,
        vec![LaunchId(1)]
    );
}

#[test]
fn shape_change_is_new_version() {
    // Buffer B: version 1 with 8 elements written by kernel 0, then a new
    // version 2 with 16 elements written by kernel 1. A new shape is a new
    // version — this is the shape-changing contract, and it validates.
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let b_identity = identity(1, "resized", BufferRole::InOut, BufferLifetime::PerStep);

    let mut program = DeviceProgram::new(DeviceProgramLifetime::RepeatingStep);
    program.kernels.push(KernelUnit {
        function: MirFunctionId(0),
        entry: "write_v1".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![DeviceResource {
            buffer: b_identity.clone(),
            version: version(1, element_ty, 8),
            binding: Binding {
                group: 0,
                binding: 0,
            },
            access: MirKernelResourceAccess::Write,
        }],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 8, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 8, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        },
    });
    program.kernels.push(KernelUnit {
        function: MirFunctionId(1),
        entry: "write_v2".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![DeviceResource {
            buffer: b_identity.clone(),
            version: version(2, element_ty, 16),
            binding: Binding {
                group: 0,
                binding: 0,
            },
            access: MirKernelResourceAccess::Write,
        }],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 16, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 16, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        },
    });
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    program.launches.push(LaunchUnit {
        id: LaunchId(1),
        kernel_index: 1,
    });

    assert_eq!(program.validate(), Ok(()));

    let registry = program.buffer_registry();
    let b = registry
        .buffer(BufferId(1))
        .expect("resized buffer in registry");
    assert_eq!(b.versions.len(), 2);
    assert_eq!(b.versions[0].version.element_count, 8);
    assert_eq!(b.versions[1].version.element_count, 16);
    assert_eq!(b.versions[0].produced_by, vec![LaunchId(0)]);
    assert_eq!(b.versions[1].produced_by, vec![LaunchId(1)]);
}

#[test]
fn in_place_reinterpretation_rejected() {
    // Rebinding an existing version to a different shape is in-place
    // reinterpretation and must be rejected.
    let signature = reduction_signature(16, 8, 2);
    let plan = reduction_plan(&signature, 16, 8, 2);
    let mut kernel = reduction_kernel_default(&signature, plan);
    kernel.resources.push(DeviceResource {
        buffer: identity(
            1,
            "reduction.output",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
        version: version(1, signature.output.element_ty, 999),
        binding: Binding {
            group: 0,
            binding: 2,
        },
        access: MirKernelResourceAccess::Write,
    });

    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels.push(kernel);
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });

    match program.validate() {
        Err(DeviceProgramError::VersionShapeConflict {
            buffer, version, ..
        }) => {
            assert_eq!(buffer, BufferId(1));
            assert_eq!(version, 1);
        }
        other => panic!("expected a version-shape conflict, got {other:?}"),
    }
}

#[test]
fn identity_conflict_rejected() {
    let signature = reduction_signature(16, 8, 2);
    let plan = reduction_plan(&signature, 16, 8, 2);
    let mut kernel = reduction_kernel_default(&signature, plan);
    // Same buffer id, same shape, but a conflicting lifetime.
    kernel.resources.push(DeviceResource {
        buffer: identity(
            1,
            "reduction.output",
            BufferRole::Output,
            BufferLifetime::PerProgram,
        ),
        version: version(1, signature.output.element_ty, 2),
        binding: Binding {
            group: 0,
            binding: 2,
        },
        access: MirKernelResourceAccess::Write,
    });

    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels.push(kernel);
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });

    match program.validate() {
        Err(DeviceProgramError::BufferIdentityConflict { buffer, field, .. }) => {
            assert_eq!(buffer, BufferId(1));
            assert_eq!(field, "lifetime");
        }
        other => panic!("expected a buffer-identity conflict, got {other:?}"),
    }
}

// -----------------------------------------------------------------------
// Validation negatives
// -----------------------------------------------------------------------

#[test]
fn empty_kernels_and_launches_rejected() {
    let empty = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    assert!(matches!(
        empty.validate(),
        Err(DeviceProgramError::EmptyKernels)
    ));

    let signature = reduction_signature(16, 8, 2);
    let plan = reduction_plan(&signature, 16, 8, 2);
    let mut no_launches = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    no_launches
        .kernels
        .push(reduction_kernel_default(&signature, plan));
    assert!(matches!(
        no_launches.validate(),
        Err(DeviceProgramError::EmptyLaunches)
    ));
}

#[test]
fn empty_entry_name_rejected() {
    let signature = reduction_signature(16, 8, 2);
    let plan = reduction_plan(&signature, 16, 8, 2);
    let mut kernel = reduction_kernel_default(&signature, plan);
    kernel.entry.clear();
    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels.push(kernel);
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    assert!(matches!(
        program.validate(),
        Err(DeviceProgramError::EmptyEntryName { kernel: 0 })
    ));
}

#[test]
fn plan_validation_propagates() {
    // A reduction plan over a non-power-of-two workgroup passes the contract
    // match but fails the recipe validation; the program surfaces it.
    let signature = reduction_signature(16, 3, 6);
    let plan = CollectionKernelPlan::from_contract(
        &signature,
        &CollectionOpContract::Reduction {
            length: 16,
            workgroup_x: 3,
            partials: 6,
        },
        MirCollectionOp::Sum,
        None,
    )
    .expect("signature matches the contract");
    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program
        .kernels
        .push(reduction_kernel_default(&signature, plan));
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    match program.validate() {
        Err(DeviceProgramError::Plan { error }) => {
            assert!(matches!(
                error,
                crate::kernel_plan::PlanValidationError::NonPowerOfTwoWorkgroup { workgroup_x: 3 }
            ));
        }
        other => panic!("expected a plan-validation error, got {other:?}"),
    }
}

#[test]
fn duplicate_binding_rejected() {
    let signature = reduction_signature(16, 8, 2);
    let plan = reduction_plan(&signature, 16, 8, 2);
    let mut kernel = reduction_kernel_default(&signature, plan);
    // Same (group, binding) as the input resource.
    kernel.resources.push(DeviceResource {
        buffer: identity(
            1,
            "reduction.output",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
        version: version(1, signature.output.element_ty, 2),
        binding: Binding {
            group: 0,
            binding: 0,
        },
        access: MirKernelResourceAccess::Write,
    });
    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels.push(kernel);
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    match program.validate() {
        Err(DeviceProgramError::RepeatedBinding {
            kernel,
            group,
            binding,
        }) => {
            assert_eq!(kernel, 0);
            assert_eq!(group, 0);
            assert_eq!(binding, 0);
        }
        other => panic!("expected a duplicate-binding error, got {other:?}"),
    }
}

#[test]
fn zero_version_rejected() {
    let signature = reduction_signature(16, 8, 2);
    let plan = reduction_plan(&signature, 16, 8, 2);
    let mut kernel = reduction_kernel_default(&signature, plan);
    kernel.resources.push(DeviceResource {
        buffer: identity(
            1,
            "reduction.output",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
        version: version(0, signature.output.element_ty, 2),
        binding: Binding {
            group: 0,
            binding: 2,
        },
        access: MirKernelResourceAccess::Write,
    });
    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels.push(kernel);
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    assert!(matches!(
        program.validate(),
        Err(DeviceProgramError::ZeroVersion {
            buffer: BufferId(1),
            version: 0
        })
    ));
}

#[test]
fn role_access_mismatch_rejected() {
    let signature = reduction_signature(16, 8, 2);
    let plan = reduction_plan(&signature, 16, 8, 2);
    let mut kernel = reduction_kernel_default(&signature, plan);
    // An input buffer written by a slot: the program role is read-only.
    kernel.resources.push(DeviceResource {
        buffer: identity(
            0,
            "reduction.input",
            BufferRole::Input,
            BufferLifetime::PerStep,
        ),
        version: version(1, signature.input.element_ty, 16),
        binding: Binding {
            group: 0,
            binding: 2,
        },
        access: MirKernelResourceAccess::Write,
    });
    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels.push(kernel);
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    match program.validate() {
        Err(DeviceProgramError::RoleAccessMismatch { buffer, role, .. }) => {
            assert_eq!(buffer, BufferId(0));
            assert_eq!(role, BufferRole::Input);
        }
        other => panic!("expected a role/access mismatch, got {other:?}"),
    }
}

#[test]
fn launch_out_of_range_rejected() {
    let mut program = reduction_program();
    program.launches.push(LaunchUnit {
        id: LaunchId(1),
        kernel_index: 3,
    });
    match program.validate() {
        Err(DeviceProgramError::LaunchOutOfRange {
            launch,
            kernel_index,
            kernel_count,
        }) => {
            assert_eq!(launch, LaunchId(1));
            assert_eq!(kernel_index, 3);
            assert_eq!(kernel_count, 1);
        }
        other => panic!("expected a launch out-of-range error, got {other:?}"),
    }
}

#[test]
fn duplicate_launch_id_rejected() {
    let mut program = reduction_program();
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    assert!(matches!(
        program.validate(),
        Err(DeviceProgramError::RepeatedLaunchId {
            launch: LaunchId(0)
        })
    ));
}

#[test]
fn result_role_rejected() {
    let mut program = reduction_program();
    program.results.push(ResultBuffer {
        buffer: identity(
            1,
            "reduction.output",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
        version: version(1, program.kernels[0].resources[1].version.element_ty, 2),
        role: BufferRole::Input,
        produced_by: LaunchId(0),
        cadence: ObservationCadence::PerStep,
    });
    assert!(matches!(
        program.validate(),
        Err(DeviceProgramError::ResultRole {
            result: 0,
            role: BufferRole::Input
        })
    ));
}

#[test]
fn result_role_buffer_role_mismatch_rejected() {
    // A result's observation role must agree with the buffer's identity role.
    // `InOut` on an `Output` buffer (or vice versa) is a contradictory schema
    // fact: each role is individually valid, so only the cross-check catches
    // it.
    for (role, buffer_role) in [
        (BufferRole::InOut, BufferRole::Output),
        (BufferRole::Output, BufferRole::InOut),
    ] {
        let mut program = reduction_program();
        program.results.push(ResultBuffer {
            buffer: identity(
                1,
                "reduction.output",
                buffer_role,
                BufferLifetime::ObservationPoint,
            ),
            version: version(1, program.kernels[0].resources[1].version.element_ty, 2),
            role,
            produced_by: LaunchId(0),
            cadence: ObservationCadence::PerStep,
        });
        match program.validate() {
            Err(DeviceProgramError::ResultBufferRoleMismatch {
                result,
                role: got_role,
                buffer_role: got_buffer_role,
            }) => {
                assert_eq!(result, 0);
                assert_eq!(got_role, role);
                assert_eq!(got_buffer_role, buffer_role);
            }
            other => panic!("expected a result role/buffer-role mismatch, got {other:?}"),
        }
    }
}

#[test]
fn unknown_producing_launch_rejected() {
    let mut program = reduction_program();
    program.results.push(ResultBuffer {
        buffer: identity(
            1,
            "reduction.output",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
        version: version(1, program.kernels[0].resources[1].version.element_ty, 2),
        role: BufferRole::Output,
        produced_by: LaunchId(9),
        cadence: ObservationCadence::PerStep,
    });
    assert!(matches!(
        program.validate(),
        Err(DeviceProgramError::AbsentProducingLaunch {
            result: 0,
            produced_by: LaunchId(9)
        })
    ));
}

#[test]
fn result_not_produced_rejected() {
    let mut program = reduction_program();
    // The launch exists, but its kernel does not write buffer 5 version 1.
    program.results.push(ResultBuffer {
        buffer: identity(
            5,
            "ghost",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
        version: version(1, program.kernels[0].resources[1].version.element_ty, 2),
        role: BufferRole::Output,
        produced_by: LaunchId(0),
        cadence: ObservationCadence::PerStep,
    });
    match program.validate() {
        Err(DeviceProgramError::ResultNotProduced {
            result,
            produced_by,
            buffer,
            version,
        }) => {
            assert_eq!(result, 0);
            assert_eq!(produced_by, LaunchId(0));
            assert_eq!(buffer, BufferId(5));
            assert_eq!(version, 1);
        }
        other => panic!("expected a result-not-produced error, got {other:?}"),
    }
}

/// S5A-U1: the observation cadence is a declared fact consistent with the
/// buffer's lifetime class. A `PerStep` readback names an `ObservationPoint`
/// buffer; an `EndOfRun` readback names a `PerStep` or `PerProgram` buffer. A
/// cadence that contradicts the lifetime (a per-step readback of a per-step
/// final, or an end-of-run readback of an observation point) fails closed.
#[test]
fn result_cadence_must_match_buffer_lifetime() {
    // A one-kernel program whose output buffer carries the requested
    // lifetime; validating it with the requested cadence exercises exactly
    // the cadence/lifetime coupling (the kernel slot and the result row
    // share one identity, so only the cadence can contradict).
    fn validate_with(
        lifetime: BufferLifetime,
        cadence: ObservationCadence,
    ) -> Result<(), DeviceProgramError> {
        let mut types = TypeTable::new();
        let element_ty = f32_ty(&mut types);
        let output = identity(2, "y", BufferRole::Output, lifetime);
        let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
        program.kernels.push(KernelUnit {
            function: MirFunctionId(0),
            entry: "addita".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: identity(1, "x", BufferRole::Input, BufferLifetime::PerProgram),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: output,
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: KernelLaunchPlan {
                workgroup: MirWorkgroupSize { x: 8, y: 1, z: 1 },
                dispatch_size: MirKernelDispatchSize { x: 4, y: 1, z: 1 },
                workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
            },
        });
        program.launches.push(LaunchUnit {
            id: LaunchId(1),
            kernel_index: 0,
        });
        program.results.push(ResultBuffer {
            buffer: identity(2, "y", BufferRole::Output, lifetime),
            version: version(1, element_ty, 4),
            role: BufferRole::Output,
            produced_by: LaunchId(1),
            cadence,
        });
        program.validate()
    }

    for (lifetime, cadence, rejected) in [
        // Consistent pairings admitted.
        (
            BufferLifetime::ObservationPoint,
            ObservationCadence::PerStep,
            false,
        ),
        (BufferLifetime::PerStep, ObservationCadence::EndOfRun, false),
        (
            BufferLifetime::PerProgram,
            ObservationCadence::EndOfRun,
            false,
        ),
        // Contradictory pairings rejected.
        (BufferLifetime::PerStep, ObservationCadence::PerStep, true),
        (
            BufferLifetime::PerProgram,
            ObservationCadence::PerStep,
            true,
        ),
        (
            BufferLifetime::ObservationPoint,
            ObservationCadence::EndOfRun,
            true,
        ),
    ] {
        match validate_with(lifetime, cadence) {
            Ok(()) => assert!(
                !rejected,
                "cadence {cadence:?} with lifetime {lifetime:?} must be rejected"
            ),
            Err(DeviceProgramError::ResultCadenceMismatch {
                result,
                buffer,
                cadence: got_cadence,
                lifetime: got_lifetime,
            }) => {
                assert!(
                    rejected,
                    "cadence {cadence:?} with lifetime {lifetime:?} must be admitted"
                );
                assert_eq!(result, 0);
                assert_eq!(buffer, BufferId(2));
                assert_eq!(got_cadence, cadence);
                assert_eq!(got_lifetime, lifetime);
            }
            other => panic!("expected a cadence/lifetime mismatch, got {other:?}"),
        }
    }
}

// -----------------------------------------------------------------------
// Registry: inter-kernel data flow (the generalized chain concept)
// -----------------------------------------------------------------------

#[test]
fn registry_chain_and_data_flow_pairs() {
    // kernel 0 (producer) reads A and writes intermediate B; kernel 1
    // (consumer) reads B and writes result C.
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let mut program = DeviceProgram::new(DeviceProgramLifetime::RepeatingStep);

    let a = identity(0, "input", BufferRole::Input, BufferLifetime::PerStep);
    let b = identity(
        1,
        "intermediate",
        BufferRole::InOut,
        BufferLifetime::PerStep,
    );
    let c = identity(
        2,
        "result",
        BufferRole::Output,
        BufferLifetime::ObservationPoint,
    );

    program.kernels.push(KernelUnit {
        function: MirFunctionId(0),
        entry: "producer".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![
            DeviceResource {
                buffer: a,
                version: version(1, element_ty, 8),
                binding: Binding {
                    group: 0,
                    binding: 0,
                },
                access: MirKernelResourceAccess::Read,
            },
            DeviceResource {
                buffer: b.clone(),
                version: version(1, element_ty, 8),
                binding: Binding {
                    group: 0,
                    binding: 1,
                },
                access: MirKernelResourceAccess::Write,
            },
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 8, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 8, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        },
    });
    program.kernels.push(KernelUnit {
        function: MirFunctionId(1),
        entry: "consumer".to_owned(),
        library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![
            DeviceResource {
                buffer: b,
                version: version(1, element_ty, 8),
                binding: Binding {
                    group: 0,
                    binding: 0,
                },
                access: MirKernelResourceAccess::Read,
            },
            DeviceResource {
                buffer: c.clone(),
                version: version(1, element_ty, 8),
                binding: Binding {
                    group: 0,
                    binding: 1,
                },
                access: MirKernelResourceAccess::Write,
            },
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 8, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 8, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        },
    });
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    program.launches.push(LaunchUnit {
        id: LaunchId(1),
        kernel_index: 1,
    });
    program.results.push(ResultBuffer {
        buffer: c,
        version: version(1, element_ty, 8),
        role: BufferRole::Output,
        produced_by: LaunchId(1),
        cadence: ObservationCadence::PerStep,
    });

    assert_eq!(program.validate(), Ok(()));

    let registry = program.buffer_registry();
    let b_entry = registry
        .buffer(BufferId(1))
        .expect("intermediate in registry");
    assert_eq!(b_entry.identity.name, "intermediate");
    assert_eq!(b_entry.identity.role, BufferRole::InOut);
    assert_eq!(b_entry.versions.len(), 1);
    assert_eq!(b_entry.versions[0].produced_by, vec![LaunchId(0)]);
    assert_eq!(b_entry.versions[0].consumed_by, vec![LaunchId(1)]);

    // The generalized inter-kernel identity: buffer 1 version 1 flows from
    // launch 0 to launch 1.
    assert_eq!(
        registry.data_flow_pairs(),
        vec![DataFlowPair {
            buffer: BufferId(1),
            version: 1,
            producer: LaunchId(0),
            consumer: LaunchId(1),
        }]
    );
}

// -----------------------------------------------------------------------
// Stage 3R F5/F6: independent resource-state axes + the F6 coupling
// (DeviceProgram::validate_with_semantics)
// -----------------------------------------------------------------------

/// The standard reduction program plus a declared result row for its output,
/// paired with the carried semantics that make the result an explicit
/// observation point (F6).
fn reduction_program_with_semantics() -> (DeviceProgram, DeviceSemantics) {
    let mut program = reduction_program();
    let signature = reduction_signature(16, 8, 2);
    program.results.push(ResultBuffer {
        buffer: identity(
            1,
            "reduction.output",
            BufferRole::Output,
            BufferLifetime::ObservationPoint,
        ),
        version: version(
            1,
            signature.output.element_ty,
            signature.output.element_count,
        ),
        role: BufferRole::Output,
        produced_by: LaunchId(0),
        cadence: ObservationCadence::PerStep,
    });
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);
    let semantics = DeviceSemantics {
        values: vec![
            SemanticValue {
                id: SemanticValueId(1),
                name: "reduction.input".to_owned(),
                element_ty,
                element_count: 16,
                origin: SemanticValueOrigin::HostInput,
            },
            SemanticValue {
                id: SemanticValueId(2),
                name: "reduction.output".to_owned(),
                element_ty,
                element_count: 2,
                origin: SemanticValueOrigin::Synthetic {
                    label: "reduction-output".to_owned(),
                },
            },
        ],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(0),
            },
            ValueBinding {
                value: SemanticValueId(2),
                buffer: BufferId(1),
            },
        ],
        generations: vec![ValueGeneration {
            value: SemanticValueId(2),
            generation: 1,
            element_ty,
            element_count: 2,
            produced_by: LaunchId(0),
        }],
        roots: vec![LaunchId(0)],
        // The five axes are declared independently (DDCP1-U3): the input is
        // host-provided, the output kernel-initialized — each buffer's read
        // is preceded by a declared initialization fact.
        initializations: vec![
            InitializationFact {
                buffer: BufferId(0),
                policy: InitializationPolicy::HostProvided,
            },
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::KernelInitialized,
            },
        ],
        observations: vec![ObservationFact {
            buffer: BufferId(1),
            version: 1,
            at_launch: LaunchId(0),
        }],
        ..DeviceSemantics::default()
    };
    (program, semantics)
}

/// F6 positive: the frozen program and its carried semantics validate
/// together when the result IS the declared observation point.
#[test]
fn program_and_semantics_validate_together() {
    let (program, semantics) = reduction_program_with_semantics();
    program
        .validate_with_semantics(&semantics)
        .expect("an explicit observation fact backs the result (F6)");
}

/// F6 red test: a writable intermediate exposed as a result WITHOUT an
/// explicit observation fact is rejected — results name declared observation
/// points only, never buffers merely because they are writable.
#[test]
fn writable_intermediate_as_result_without_observation_fact_fails_closed() {
    let (program, mut semantics) = reduction_program_with_semantics();
    semantics.observations.clear();
    assert!(matches!(
        program.validate_with_semantics(&semantics),
        Err(DeviceProgramError::ResultWithoutObservationFact {
            result: 0,
            buffer: BufferId(1),
            version: 1,
        })
    ));
}

/// F6 red test: an observation fact the host would never be told to read
/// back (no declared result row) fails closed.
#[test]
fn observation_fact_without_declared_result_fails_closed() {
    let (mut program, semantics) = reduction_program_with_semantics();
    program.results.clear();
    assert!(matches!(
        program.validate_with_semantics(&semantics),
        Err(DeviceProgramError::UndeclaredObservationFact {
            buffer: BufferId(1),
            version: 1,
            at_launch: LaunchId(0),
        })
    ));
}

/// F5 coupling proof: initialization, allocation lifetime, observation, and
/// value generation are independent facts — each can change alone without
/// silently rewriting the others and without invalidating the pair.
#[test]
fn axes_vary_independently_without_cross_rewrite() {
    let (program, mut semantics) = reduction_program_with_semantics();
    program
        .validate_with_semantics(&semantics)
        .expect("baseline pair validates");

    // 1. Initialization axis: a host-provided init fact for the input buffer
    // rides alongside the existing lifetime / observation / generation facts
    // — no axis rewrites another.
    semantics.initializations = vec![InitializationFact {
        buffer: BufferId(0),
        policy: InitializationPolicy::HostProvided,
    }];
    program
        .validate_with_semantics(&semantics)
        .expect("init fact is an independent axis (F5)");
    assert_eq!(
        semantics.observations,
        vec![ObservationFact {
            buffer: BufferId(1),
            version: 1,
            at_launch: LaunchId(0),
        }],
        "adding an init fact must not rewrite the observation fact"
    );
    assert_eq!(
        semantics.generations[0].generation, 1,
        "adding an init fact must not rewrite the generation"
    );

    // 2. Generation axis: advancing a generation is a new logical write and
    // must not touch init or observation.
    semantics.generations[0].generation = 2;
    program
        .validate_with_semantics(&semantics)
        .expect("a new generation is an independent axis (F2/F5)");
    assert_eq!(
        semantics.initializations[0].policy,
        InitializationPolicy::HostProvided,
        "advancing a generation must not rewrite the init policy"
    );

    // 3. Allocation-lifetime axis: relabeling a buffer's storage lifetime on
    // the program side is independent of the carried init/observation facts.
    let mut lifetime_relabeled = program.clone();
    let input = &mut lifetime_relabeled.kernels[0].resources[0].buffer.lifetime;
    *input = BufferLifetime::PerProgram;
    lifetime_relabeled
        .validate_with_semantics(&semantics)
        .expect("lifetime is an independent axis (F5)");
    assert_eq!(
        semantics.observations[0].at_launch,
        LaunchId(0),
        "relabeling a lifetime must not rewrite the observation boundary"
    );
    assert_eq!(
        semantics.generations[0].generation, 2,
        "relabeling a lifetime must not rewrite the generation"
    );
}

// -----------------------------------------------------------------------
// DDCP1-U2: consumer binds identity + generation from its producer edge
// (contract rule 3; scheduling rules 2/3) — the edge↔value↔generation
// cross-check in DeviceProgram::validate_with_semantics.
// -----------------------------------------------------------------------

/// A two-kernel chain: kernel 0 writes intermediate buffer 0 version 1;
/// kernel 1 reads it and writes output buffer 1 version 1. Launches
/// l0 → l1. The carried semantics bind value 1 to buffer 0 (generation 1
/// produced by l0, shape (f32, 4)) and value 2 to buffer 1 (generation 1
/// produced by l1, shape (f32, 2)); root l0; one edge l0→l1 flowing
/// buffer 0 version 1. The pair validates together; tests mutate exactly
/// one fact to prove each fail-closed rule.
fn chain_program_with_semantics() -> (DeviceProgram, DeviceSemantics) {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);

    let launch_plan = |x: u64| KernelLaunchPlan {
        workgroup: MirWorkgroupSize {
            x: u32::try_from(x).expect("workgroup fits u32"),
            y: 1,
            z: 1,
        },
        dispatch_size: MirKernelDispatchSize { x, y: 1, z: 1 },
        workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
    };

    let intermediate = identity(
        0,
        "intermediate",
        BufferRole::InOut,
        BufferLifetime::PerStep,
    );
    let output = identity(1, "output", BufferRole::Output, BufferLifetime::PerStep);

    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels = vec![
        KernelUnit {
            function: MirFunctionId(0),
            entry: "chain_producer".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![DeviceResource {
                buffer: intermediate.clone(),
                version: version(1, element_ty, 4),
                binding: Binding {
                    group: 0,
                    binding: 0,
                },
                access: MirKernelResourceAccess::Write,
            }],
            launch: launch_plan(4),
        },
        KernelUnit {
            function: MirFunctionId(1),
            entry: "chain_consumer".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: intermediate.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: output.clone(),
                    version: version(1, element_ty, 2),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(2),
        },
    ];
    program.launches = vec![
        LaunchUnit {
            id: LaunchId(0),
            kernel_index: 0,
        },
        LaunchUnit {
            id: LaunchId(1),
            kernel_index: 1,
        },
    ];

    let semantics = DeviceSemantics {
        values: vec![
            SemanticValue {
                id: SemanticValueId(1),
                name: "intermediate".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::MirLocal {
                    function: MirFunctionId(0),
                    local: 0,
                },
            },
            SemanticValue {
                id: SemanticValueId(2),
                name: "output".to_owned(),
                element_ty,
                element_count: 2,
                origin: SemanticValueOrigin::Synthetic {
                    label: "chain-output".to_owned(),
                },
            },
        ],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(0),
            },
            ValueBinding {
                value: SemanticValueId(2),
                buffer: BufferId(1),
            },
        ],
        generations: vec![
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 1,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(0),
            },
            ValueGeneration {
                value: SemanticValueId(2),
                generation: 1,
                element_ty,
                element_count: 2,
                produced_by: LaunchId(1),
            },
        ],
        roots: vec![LaunchId(0)],
        dependencies: vec![DependencyEdge {
            producer: LaunchId(0),
            consumer: LaunchId(1),
            buffer: BufferId(0),
            version: 1,
        }],
        // Independent initialization axis (DDCP1-U3): the intermediate is
        // kernel-initialized (launch 0 writes it before launch 1 reads it),
        // the output kernel-initialized by launch 1.
        initializations: vec![
            InitializationFact {
                buffer: BufferId(0),
                policy: InitializationPolicy::KernelInitialized,
            },
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::KernelInitialized,
            },
        ],
        ..DeviceSemantics::default()
    };

    program
        .validate_with_semantics(&semantics)
        .expect("chain program + carried semantics validate together");
    (program, semantics)
}

#[test]
fn consumer_edge_binds_value_and_generation() {
    // Contract rule 3 (positive): the consumer edge inherits the value
    // identity (buffer 0 → value 1) and the generation produced by its
    // producer edge (generation 1, produced by launch 0) unambiguously.
    let (program, semantics) = chain_program_with_semantics();
    program
        .validate_with_semantics(&semantics)
        .expect("the consumer binding is unambiguous and consistent");
}

#[test]
fn consumer_edge_missing_producer_fails_closed() {
    // Scheduling rule 3: an edge flowing a buffer version the program never
    // produces is a missing producer.
    let (program, mut semantics) = chain_program_with_semantics();
    semantics.dependencies[0].version = 2;
    assert!(matches!(
        program.validate_with_semantics(&semantics),
        Err(DeviceProgramError::AbsentProducer {
            producer: LaunchId(0),
            value: SemanticValueId(1),
        })
    ));
}

#[test]
fn edge_generation_shape_mismatch_fails_closed() {
    // Contract rule 3: the producer's carried generation must match the
    // flowing buffer version's shape; a mismatch cannot bind a consistent
    // generation.
    let (program, mut semantics) = chain_program_with_semantics();
    semantics.generations[0].element_count = 8;
    assert!(matches!(
        program.validate_with_semantics(&semantics),
        Err(DeviceProgramError::EdgeGenerationShapeMismatch {
            value: SemanticValueId(1),
            producer: LaunchId(0),
            version: 1,
        })
    ));
}

#[test]
fn ambiguous_edge_generation_fails_closed() {
    // Contract rule 3: two generations of the value written by the producer
    // both matching the flowing version make the consumer binding ambiguous.
    let (program, mut semantics) = chain_program_with_semantics();
    semantics.generations.push(ValueGeneration {
        value: SemanticValueId(1),
        generation: 2,
        element_ty: semantics.generations[0].element_ty,
        element_count: 4,
        produced_by: LaunchId(0),
    });
    assert!(matches!(
        program.validate_with_semantics(&semantics),
        Err(DeviceProgramError::AmbiguousEdgeGeneration {
            value: SemanticValueId(1),
            producer: LaunchId(0),
            version: 1,
        })
    ));
}

#[test]
fn scheduling_is_driven_by_edges_not_launch_declaration_order() {
    // Scheduling rule 1 (regression): the same edge set with a permuted
    // launch declaration order and a permuted edge declaration order
    // validates identically — declaration order never decides scheduling.
    let (mut program, mut semantics) = chain_program_with_semantics();
    program.launches.swap(0, 1);
    semantics.dependencies.reverse();
    program
        .validate_with_semantics(&semantics)
        .expect("permuted launch/edge declaration order validates identically");
}

// -----------------------------------------------------------------------
// DDCP1-U3: independent resource axes + persistent-state rule + per-output
// domains (contract §Resource axes, §Persistent state, §Per-output bounds).
// -----------------------------------------------------------------------

#[test]
fn first_read_before_initialization_fails_closed() {
    // Resource axes rule 1: a buffer read by a kernel must have a declared
    // initialization fact — the initialization axis is separate and never
    // implied by access, lifetime, generation, or observation facts.
    let (program, mut semantics) = chain_program_with_semantics();
    semantics.initializations.clear();
    assert!(matches!(
        program.validate_with_semantics(&semantics),
        Err(DeviceProgramError::UninitializedBufferRead {
            buffer: BufferId(0),
            launch: LaunchId(1),
        })
    ));
}

#[test]
fn initialization_axis_is_independent_of_observation() {
    // Resource axes: the initialization policy changes alone (kernel- to
    // host-provided) without touching access, lifetime, generation, or
    // observation facts, and no observation is implied by any initialization
    // — a write never implies observability.
    let (program, mut semantics) = chain_program_with_semantics();
    semantics.initializations[0].policy = InitializationPolicy::HostProvided;
    program
        .validate_with_semantics(&semantics)
        .expect("the initialization axis varies alone; observability is a separate declared fact");
    assert!(
        semantics.observations.is_empty(),
        "initialization never implies an observation fact"
    );
}

#[test]
fn persistent_writable_state_never_automatically_results() {
    // Persistent-state rule: a per-program writable buffer with no declared
    // observation produces NO result — results are never inferred from
    // writable buffers or `results` presence alone.
    let (mut program, mut semantics) = chain_program_with_semantics();
    program.kernels[0].resources[0].buffer.lifetime = BufferLifetime::PerProgram;
    program.kernels[1].resources[0].buffer.lifetime = BufferLifetime::PerProgram;
    semantics.initializations.clear();
    semantics.initializations.push(InitializationFact {
        buffer: BufferId(0),
        policy: InitializationPolicy::HostProvided,
    });
    assert!(
        program.results.is_empty(),
        "precondition: no result rows are declared"
    );
    program.validate_with_semantics(&semantics).expect(
        "a writable persistent buffer with no declared observation is never an automatic result",
    );
    assert!(
        program.buffer_registry().buffer(BufferId(0)).is_some(),
        "the persistent writable buffer stays in the program"
    );
}

#[test]
fn observation_point_buffer_must_be_declared_observed() {
    // Resource axes rule 3: a buffer declaring the read-back lifetime class
    // (ObservationPoint) but never declared observed is a use outside its
    // declared allocation lifetime.
    let (mut program, semantics) = reduction_program_with_semantics();
    program.kernels[0].resources[0].buffer.lifetime = BufferLifetime::ObservationPoint;
    assert!(matches!(
        program.validate_with_semantics(&semantics),
        Err(DeviceProgramError::ObservationPointBufferNotObserved {
            buffer: BufferId(0)
        })
    ));
}

// -----------------------------------------------------------------------
// DDCP1-U4: companion identity + forward/backward coexistence without a
// runtime tape (contract §Kernel and companion identity rules 2/5/6/7/8).
// -----------------------------------------------------------------------

/// The chain program paired with one carried companion relation: primal
/// `DefId(1)` (launch 0, writes value 1) has a generated backward companion
/// `DefId(2)` (launch 1) that consumes the primal output value 1 and writes
/// one gradient (value 2). The graph + the relation are the only accounting
/// — there is no runtime-tape carrier anywhere in the model.
fn forward_backward_program_with_semantics() -> (DeviceProgram, DeviceSemantics) {
    let (program, mut semantics) = chain_program_with_semantics();
    semantics.relations = vec![LosslessMirCompanionEntry {
        primal: DefId(1),
        companion: DefId(2),
        derivative: MirCompanionDerivativeKind::ReverseModeVjp,
        device_resident: true,
        selected_inputs: vec![MirCompanionSelectedInput {
            param: DefId(1),
            position: 0,
            ty: TypeId(1),
            gradient_slot: 0,
        }],
        selected_outputs: vec![MirCompanionSelectedOutput {
            position: 0,
            ty: TypeId(1),
            upstream_gradient_ty: TypeId(1),
        }],
    }];
    program
        .validate_with_semantics(&semantics)
        .expect("forward + generated-backward kernels coexist");
    (program, semantics)
}

#[test]
fn forward_and_backward_coexist_without_runtime_tape() {
    // Contract rule 8: forward and generated-backward kernels coexist in one
    // program WITHOUT a runtime tape — the dependency graph and the
    // companion relations are the only accounting. The frozen four-field
    // program shape carries no tape, no autograd runtime, and no history
    // buffer; the semantics carry only values/generations/edges/relations.
    let (program, semantics) = forward_backward_program_with_semantics();
    program
        .validate_with_semantics(&semantics)
        .expect("the pair validates with graph + relations as the only accounting");
    assert_eq!(
        program.kernels.len(),
        2,
        "one forward + one backward kernel"
    );
    assert_eq!(program.launches.len(), 2);
    assert_eq!(semantics.relations.len(), 1);
}

#[test]
fn stale_companion_consumption_fails_closed() {
    // Contract rule 5: a consumer binding an OLDER generation while the
    // producer carries a newer one consumes superseded primal state — a
    // stale companion fact, fail-closed in companion-bearing programs.
    let (program, mut semantics) = forward_backward_program_with_semantics();
    semantics.generations.push(ValueGeneration {
        value: SemanticValueId(1),
        generation: 2,
        element_ty: semantics.generations[0].element_ty,
        element_count: 8,
        produced_by: LaunchId(0),
    });
    assert!(matches!(
        program.validate_with_semantics(&semantics),
        Err(DeviceProgramError::StaleCompanionFact {
            value: SemanticValueId(1),
            bound_generation: 1,
            latest_generation: 2,
        })
    ));
}

#[test]
fn forward_only_program_never_trips_stale_or_missing_rules() {
    // Contract rule 7: forward-only programs (no relations) are immune to
    // the stale/missing companion rules — a newer write of a consumed value
    // in a relation-free program is an ordinary superseding write, not a
    // stale companion.
    let (program, mut semantics) = chain_program_with_semantics();
    assert!(semantics.relations.is_empty());
    semantics.generations.push(ValueGeneration {
        value: SemanticValueId(1),
        generation: 2,
        element_ty: semantics.generations[0].element_ty,
        element_count: 8,
        produced_by: LaunchId(0),
    });
    program
        .validate_with_semantics(&semantics)
        .expect("a forward-only program needs no companion facts; newer writes are ordinary");
}

// -----------------------------------------------------------------------
// DDCP1-U5: prepared submission regions (contract §Prepared submission
// regions rules 1–4; NGAB0-R1 granularity) — the compiler-visible region
// surface over the carried dependency graph.
// -----------------------------------------------------------------------

/// A three-launch chain program with a declared observation at the middle
/// launch: kernel 0 writes buffer 0, kernel 1 reads it and writes buffer 1,
/// kernel 2 reads buffer 1 and writes buffer 2; launches l0 → l1 → l2 with
/// a result row + observation fact at l1 (the loss-trace pattern — a CPU
/// readback between l1 and l2). The derived prepared submission regions are
/// [l0, l1] and [l2].
fn three_chain_program_with_middle_observation() -> (DeviceProgram, DeviceSemantics) {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);

    let launch_plan = |x: u64| KernelLaunchPlan {
        workgroup: MirWorkgroupSize {
            x: u32::try_from(x).expect("workgroup fits u32"),
            y: 1,
            z: 1,
        },
        dispatch_size: MirKernelDispatchSize { x, y: 1, z: 1 },
        workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
    };

    let b0 = identity(0, "mid0", BufferRole::InOut, BufferLifetime::PerStep);
    let b1 = identity(
        1,
        "mid1",
        BufferRole::InOut,
        BufferLifetime::ObservationPoint,
    );
    let b2 = identity(2, "out", BufferRole::Output, BufferLifetime::PerStep);

    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels = vec![
        KernelUnit {
            function: MirFunctionId(0),
            entry: "k0".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![DeviceResource {
                buffer: b0.clone(),
                version: version(1, element_ty, 4),
                binding: Binding {
                    group: 0,
                    binding: 0,
                },
                access: MirKernelResourceAccess::Write,
            }],
            launch: launch_plan(4),
        },
        KernelUnit {
            function: MirFunctionId(1),
            entry: "k1".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: b0,
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: b1.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(4),
        },
        KernelUnit {
            function: MirFunctionId(2),
            entry: "k2".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: b1,
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: b2,
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(4),
        },
    ];
    program.launches = vec![
        LaunchUnit {
            id: LaunchId(0),
            kernel_index: 0,
        },
        LaunchUnit {
            id: LaunchId(1),
            kernel_index: 1,
        },
        LaunchUnit {
            id: LaunchId(2),
            kernel_index: 2,
        },
    ];
    program.results.push(ResultBuffer {
        buffer: identity(
            1,
            "mid1",
            BufferRole::InOut,
            BufferLifetime::ObservationPoint,
        ),
        version: version(1, element_ty, 4),
        role: BufferRole::InOut,
        produced_by: LaunchId(1),
        cadence: ObservationCadence::PerStep,
    });

    let semantics = DeviceSemantics {
        values: vec![
            SemanticValue {
                id: SemanticValueId(1),
                name: "mid0".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::MirLocal {
                    function: MirFunctionId(0),
                    local: 0,
                },
            },
            SemanticValue {
                id: SemanticValueId(2),
                name: "mid1".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::MirLocal {
                    function: MirFunctionId(1),
                    local: 0,
                },
            },
            SemanticValue {
                id: SemanticValueId(3),
                name: "out".to_owned(),
                element_ty,
                element_count: 4,
                origin: SemanticValueOrigin::Synthetic {
                    label: "three-chain-output".to_owned(),
                },
            },
        ],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(0),
            },
            ValueBinding {
                value: SemanticValueId(2),
                buffer: BufferId(1),
            },
            ValueBinding {
                value: SemanticValueId(3),
                buffer: BufferId(2),
            },
        ],
        generations: vec![
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 1,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(0),
            },
            ValueGeneration {
                value: SemanticValueId(2),
                generation: 1,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(1),
            },
            ValueGeneration {
                value: SemanticValueId(3),
                generation: 1,
                element_ty,
                element_count: 4,
                produced_by: LaunchId(2),
            },
        ],
        roots: vec![LaunchId(0)],
        dependencies: vec![
            DependencyEdge {
                producer: LaunchId(0),
                consumer: LaunchId(1),
                buffer: BufferId(0),
                version: 1,
            },
            DependencyEdge {
                producer: LaunchId(1),
                consumer: LaunchId(2),
                buffer: BufferId(1),
                version: 1,
            },
        ],
        initializations: vec![
            InitializationFact {
                buffer: BufferId(0),
                policy: InitializationPolicy::KernelInitialized,
            },
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::KernelInitialized,
            },
            InitializationFact {
                buffer: BufferId(2),
                policy: InitializationPolicy::KernelInitialized,
            },
        ],
        observations: vec![ObservationFact {
            buffer: BufferId(1),
            version: 1,
            at_launch: LaunchId(1),
        }],
        ..DeviceSemantics::default()
    };

    program
        .validate_with_semantics(&semantics)
        .expect("three-launch chain + carried semantics validate together");
    (program, semantics)
}

#[test]
fn chain_program_is_one_prepared_region_without_observation() {
    // NGAB0-R1: two dependent launches with no observation between them form
    // ONE prepared submission region — one host call enqueues both, with no
    // CPU observation between members.
    let (program, semantics) = chain_program_with_semantics();
    let launches: Vec<LaunchId> = program.launches.iter().map(|launch| launch.id).collect();
    let regions = semantics.derive_prepared_regions(&launches);
    assert_eq!(
        regions,
        vec![PreparedRegion {
            id: RegionId(0),
            launches: vec![LaunchId(0), LaunchId(1)],
        }]
    );
}

#[test]
fn prepared_regions_split_at_the_middle_observation() {
    // Rules 1/3/4 + NGAB0-R1: the connected chain l0 → l1 → l2 is one
    // candidate, and the declared observation at l1 (a CPU readback point)
    // is split at that point — [l0, l1] and [l2].
    let (program, semantics) = three_chain_program_with_middle_observation();
    let launches: Vec<LaunchId> = program.launches.iter().map(|launch| launch.id).collect();
    let regions = semantics.derive_prepared_regions(&launches);
    assert_eq!(
        regions,
        vec![
            PreparedRegion {
                id: RegionId(0),
                launches: vec![LaunchId(0), LaunchId(1)],
            },
            PreparedRegion {
                id: RegionId(1),
                launches: vec![LaunchId(2)],
            },
        ],
        "the observed launch l1 ends its region; l2 starts the next region"
    );
}

#[test]
fn one_kernel_program_is_the_minimal_prepared_region() {
    // NGAB0-R1: the one-kernel case is the minimal prepared region, not a
    // separate ABI shape — a single observed launch is one region of one
    // member.
    let (program, semantics) = reduction_program_with_semantics();
    let launches: Vec<LaunchId> = program.launches.iter().map(|launch| launch.id).collect();
    let regions = semantics.derive_prepared_regions(&launches);
    assert_eq!(
        regions,
        vec![PreparedRegion {
            id: RegionId(0),
            launches: vec![LaunchId(0)],
        }]
    );
    program
        .validate_with_semantics(&semantics)
        .expect("the observed single-kernel program still validates with regions");
}

#[test]
fn prepared_region_derivation_is_order_independent() {
    // Rule 4 fixture: permuted launch/edge/generation declaration order
    // derives identical regions and identical compiler-owned ids.
    let (mut program_a, semantics_a) = chain_program_with_semantics();
    let (mut program_b, mut semantics_b) = chain_program_with_semantics();
    program_a.launches.swap(0, 1);
    program_b.launches.swap(0, 1);
    semantics_b.dependencies.reverse();
    semantics_b.generations.reverse();
    let launches_a: Vec<LaunchId> = program_a.launches.iter().map(|launch| launch.id).collect();
    let launches_b: Vec<LaunchId> = program_b.launches.iter().map(|launch| launch.id).collect();
    let regions_a = semantics_a.derive_prepared_regions(&launches_a);
    let regions_b = semantics_b.derive_prepared_regions(&launches_b);
    assert_eq!(
        regions_a, regions_b,
        "declaration order never decides region formation or region ids"
    );
}

/// l0 → l1, l0 → l2, l2 → l3, l0 → l4, l4 → l5: one branched component
/// rooted at l0, with a declared observation at l1.
///
/// Rule 3 splits the component at the observed launch l1 — `[l0, l1]` and
/// `[l2, l3, l4, l5]`. The remainder is NOT path-connected: {l2, l3} and
/// {l4, l5} connect only through l0, which already ended the earlier
/// region. Rule 1 says disconnected launches are separate regions, so the
/// derivation re-splits the remainder into `[l2, l3]` and `[l4, l5]`.
fn branched_program_with_disconnecting_middle_observation() -> (DeviceProgram, DeviceSemantics) {
    let mut types = TypeTable::new();
    let element_ty = f32_ty(&mut types);

    let launch_plan = |x: u64| KernelLaunchPlan {
        workgroup: MirWorkgroupSize {
            x: u32::try_from(x).expect("workgroup fits u32"),
            y: 1,
            z: 1,
        },
        dispatch_size: MirKernelDispatchSize { x, y: 1, z: 1 },
        workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
    };

    let b0 = identity(0, "b0", BufferRole::InOut, BufferLifetime::PerStep);
    let b1 = identity(1, "b1", BufferRole::InOut, BufferLifetime::ObservationPoint);
    let b2 = identity(2, "b2", BufferRole::InOut, BufferLifetime::PerStep);
    let b3 = identity(3, "b3", BufferRole::InOut, BufferLifetime::PerStep);
    let b4 = identity(4, "b4", BufferRole::InOut, BufferLifetime::PerStep);
    let b5 = identity(5, "b5", BufferRole::Output, BufferLifetime::PerStep);

    let mut program = DeviceProgram::new(DeviceProgramLifetime::SingleRun);
    program.kernels = vec![
        // k0 writes b0 (the root).
        KernelUnit {
            function: MirFunctionId(0),
            entry: "k0".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![DeviceResource {
                buffer: b0.clone(),
                version: version(1, element_ty, 4),
                binding: Binding {
                    group: 0,
                    binding: 0,
                },
                access: MirKernelResourceAccess::Write,
            }],
            launch: launch_plan(4),
        },
        // k1 reads b0, writes b1 (the declared observation).
        KernelUnit {
            function: MirFunctionId(1),
            entry: "k1".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: b0.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: b1.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(4),
        },
        // k2 reads b0, writes b2.
        KernelUnit {
            function: MirFunctionId(2),
            entry: "k2".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: b0.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: b2.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(4),
        },
        // k3 reads b2, writes b3.
        KernelUnit {
            function: MirFunctionId(3),
            entry: "k3".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: b2.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: b3.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(4),
        },
        // k4 reads b0, writes b4.
        KernelUnit {
            function: MirFunctionId(4),
            entry: "k4".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: b0.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: b4.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(4),
        },
        // k5 reads b4, writes b5.
        KernelUnit {
            function: MirFunctionId(5),
            entry: "k5".to_owned(),
            library_operation: crate::device_plan_compiler::LibraryOperation::Generated,
            plan: CollectionKernelPlan::Elementwise,
            resources: vec![
                DeviceResource {
                    buffer: b4.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 0,
                    },
                    access: MirKernelResourceAccess::Read,
                },
                DeviceResource {
                    buffer: b5.clone(),
                    version: version(1, element_ty, 4),
                    binding: Binding {
                        group: 0,
                        binding: 1,
                    },
                    access: MirKernelResourceAccess::Write,
                },
            ],
            launch: launch_plan(4),
        },
    ];
    program.launches = (0..6)
        .map(|id| LaunchUnit {
            id: LaunchId(id),
            kernel_index: id as usize,
        })
        .collect();
    program.results.push(ResultBuffer {
        buffer: identity(1, "b1", BufferRole::InOut, BufferLifetime::ObservationPoint),
        version: version(1, element_ty, 4),
        role: BufferRole::InOut,
        produced_by: LaunchId(1),
        cadence: ObservationCadence::PerStep,
    });

    let values: Vec<SemanticValue> = (0..6)
        .map(|index| SemanticValue {
            id: SemanticValueId(index + 1),
            name: format!("b{index}"),
            element_ty,
            element_count: 4,
            origin: SemanticValueOrigin::Synthetic {
                label: format!("branched-b{index}"),
            },
        })
        .collect();
    let bindings: Vec<ValueBinding> = values
        .iter()
        .map(|value| ValueBinding {
            value: value.id,
            buffer: BufferId(value.id.0 - 1),
        })
        .collect();
    let generations: Vec<ValueGeneration> = (0..6)
        .map(|index| ValueGeneration {
            value: SemanticValueId(index + 1),
            generation: 1,
            element_ty,
            element_count: 4,
            produced_by: LaunchId(index),
        })
        .collect();
    let edge = |producer: u32, consumer: u32, buffer: u32| DependencyEdge {
        producer: LaunchId(producer),
        consumer: LaunchId(consumer),
        buffer: BufferId(buffer),
        version: 1,
    };
    let initializations: Vec<InitializationFact> = (0..6)
        .map(|buffer| InitializationFact {
            buffer: BufferId(buffer),
            policy: InitializationPolicy::KernelInitialized,
        })
        .collect();

    let semantics = DeviceSemantics {
        values,
        bindings,
        generations,
        roots: vec![LaunchId(0)],
        dependencies: vec![
            edge(0, 1, 0),
            edge(0, 2, 0),
            edge(2, 3, 2),
            edge(0, 4, 0),
            edge(4, 5, 4),
        ],
        initializations,
        observations: vec![ObservationFact {
            buffer: BufferId(1),
            version: 1,
            at_launch: LaunchId(1),
        }],
        ..DeviceSemantics::default()
    };

    program
        .validate_with_semantics(&semantics)
        .expect("branched program + carried semantics validate together");
    (program, semantics)
}

#[test]
fn prepared_regions_resplit_disconnected_remainder_after_observation() {
    // Rule 1/3 fixture: the observation at l1 splits the branched component
    // into [l0, l1] and a remainder [l2, l3, l4, l5] whose two chains
    // {l2, l3} and {l4, l5} connect only through the earlier region's l0.
    // Disconnected launches are separate regions, so the remainder must be
    // re-split — never a failing closed `RegionMembersDisconnected`.
    let (program, semantics) = branched_program_with_disconnecting_middle_observation();
    let launches: Vec<LaunchId> = program.launches.iter().map(|launch| launch.id).collect();
    let regions = semantics.derive_prepared_regions(&launches);
    assert_eq!(
        regions,
        vec![
            PreparedRegion {
                id: RegionId(0),
                launches: vec![LaunchId(0), LaunchId(1)],
            },
            PreparedRegion {
                id: RegionId(1),
                launches: vec![LaunchId(2), LaunchId(3)],
            },
            PreparedRegion {
                id: RegionId(2),
                launches: vec![LaunchId(4), LaunchId(5)],
            },
        ],
        "the disconnected remainder splits into its connected components"
    );
}
