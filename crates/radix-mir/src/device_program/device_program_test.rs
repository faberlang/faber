use super::*;
use crate::abi::{
    CollectionOpContract, MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess,
    MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature, MirKernelWorkgroupCount,
    MirWorkgroupSize,
};
use crate::device_semantics::{
    DeviceSemantics, InitializationFact, InitializationPolicy, ObservationFact, SemanticValue,
    SemanticValueId, SemanticValueOrigin, ValueBinding, ValueGeneration,
};
use crate::kernel_plan::{CollectionKernelPlan, ReduceOp};
use crate::layout::MirTensorStorageLayout;
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirLocalId,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
};
use radix_lexer::Span;
use radix_types::{NumericWidth, Primitive, TypeTable};

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
fn test_program_fields_and_ordered_kernels() {
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
fn test_reuses_collection_kernel_plan() {
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
fn test_elementwise_plan_accepted() {
    let signature = elementwise_signature(16, 8);
    let kernel = KernelUnit {
        function: signature.function,
        entry: "elementwise_kernel".to_owned(),
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
fn test_launch_plan_from_signature_matmul() {
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
fn test_launch_plan_from_signature_reduction() {
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

#[test]
fn test_resources_map_from_kernel_resource() {
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
    };
    assert!(
        DeviceResource::from_kernel_resource(buffer, version(1, element_ty, 1), &extent).is_none()
    );
}

#[test]
fn test_buffer_role_abi_mapping() {
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
fn test_multi_output_results() {
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
fn test_shape_change_is_new_version() {
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
fn test_in_place_reinterpretation_rejected() {
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
fn test_identity_conflict_rejected() {
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
fn test_empty_kernels_and_launches_rejected() {
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
fn test_empty_entry_name_rejected() {
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
fn test_plan_validation_propagates() {
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
fn test_duplicate_binding_rejected() {
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
        Err(DeviceProgramError::DuplicateBinding {
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
fn test_zero_version_rejected() {
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
fn test_role_access_mismatch_rejected() {
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
fn test_launch_out_of_range_rejected() {
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
fn test_duplicate_launch_id_rejected() {
    let mut program = reduction_program();
    program.launches.push(LaunchUnit {
        id: LaunchId(0),
        kernel_index: 0,
    });
    assert!(matches!(
        program.validate(),
        Err(DeviceProgramError::DuplicateLaunchId {
            launch: LaunchId(0)
        })
    ));
}

#[test]
fn test_result_role_rejected() {
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
fn test_result_role_buffer_role_mismatch_rejected() {
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
fn test_unknown_producing_launch_rejected() {
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
    });
    assert!(matches!(
        program.validate(),
        Err(DeviceProgramError::UnknownProducingLaunch {
            result: 0,
            produced_by: LaunchId(9)
        })
    ));
}

#[test]
fn test_result_not_produced_rejected() {
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

// -----------------------------------------------------------------------
// Registry: inter-kernel data flow (the generalized chain concept)
// -----------------------------------------------------------------------

#[test]
fn test_registry_chain_and_data_flow_pairs() {
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
