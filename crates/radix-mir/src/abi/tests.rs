use super::*;
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirIntrinsic,
    MirLayoutKind, MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace,
    MirRuntimeCall, MirScalarLayout, MirStatement, MirStatementKind, MirTerminator,
    MirTerminatorKind, MirType, MirValidationContext,
};
use radix_hir::DefId;
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable};

/// One-param scalar kernel: `functio add_one(x: f32) -> f32 { x }`.
fn scalar_kernel_function(ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: Some(DefId(9)),
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty,
            mode: MirParamMode::Owned,
            span: Span::default(),
        }],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(0),
                )))),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

#[test]
fn scalar_shaped_kernel_carries_runtime_extent_in_launch_plan() {
    // U2 (DONE_WHEN d): a scalar-shaped kernel (static IO element count 1)
    // carries a RuntimeExtent resource appended to extra_inputs, and the
    // launch plan enumerates it with the next-free binding.
    let mut types = TypeTable::new();
    // The extent channel requires `numerus<u32>` to exist in the frozen
    // table (signature.rs `maybe_append_runtime_extent`); the registration
    // side effect is the point, not the returned id.
    let _extent_count_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let ty = MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32));
    let validation = MirValidationContext::new(&types);
    let function = scalar_kernel_function(ty);
    let signature =
        MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("signature");

    // Constructor wiring: input=0, output=1, extent=2 (next free binding).
    let extent = signature
        .runtime_extent()
        .expect("scalar kernel carries a runtime extent");
    assert_eq!(extent.kind, MirKernelResourceKind::RuntimeExtent);
    assert_eq!(extent.binding, 2);
    assert_eq!(extent.group, DEFAULT_KERNEL_RESOURCE_GROUP);
    assert_eq!(extent.role, MirKernelResourceRole::Input);
    assert_eq!(extent.access, MirKernelResourceAccess::Read);
    assert_eq!(extent.element_count, 1);
    assert_eq!(extent.source_local, None);
    assert_eq!(extent.source_name.as_deref(), Some("runtime_extent"));

    // Reflection enumerates the extent resource. `resources()` iterates
    // input → extra_inputs (the extent) → output, so the extent is at
    // index 1 in the reflection list.
    let reflection = MirKernelReflection::from_signature("add_one", &signature, &validation)
        .expect("reflection");
    assert_eq!(reflection.resources.len(), 3);
    assert_eq!(
        reflection.resources[1].kind,
        MirKernelResourceKind::RuntimeExtent
    );
    assert_eq!(reflection.resources[1].binding, 2);
    assert_eq!(reflection.resources[1].element_layout, MirScalarLayout::U32);
    assert_eq!(reflection.resources[1].element_byte_width, 4);
    assert_eq!(reflection.resources[1].buffer_byte_len, 4);

    // Launch plan enumerates the extent with its binding (L3: no parallel
    // field — it flows through the positional allocator).
    let launch = reflection.launch_plan();
    assert_eq!(launch.bind_groups.len(), 1);
    let bindings = &launch.bind_groups[0].bindings;
    assert_eq!(bindings.len(), 3);
    assert_eq!(bindings[2].binding, 2);
    assert_eq!(bindings[2].kind, MirKernelResourceKind::RuntimeExtent);
    assert_eq!(bindings[2].element_count, 1);
    assert_eq!(bindings[2].buffer_byte_len, 4);
    assert_eq!(bindings[2].source_name.as_deref(), Some("runtime_extent"));

    // dispatch_size remains the static shape hint (U2 done_when e).
    assert_eq!(signature.dispatch_size.x, 1);
}

#[test]
fn multi_element_shape_has_no_runtime_extent() {
    // U2 non-goals: statically-known multi-element shapes keep the static
    // limit — no extent resource.
    let mut types = TypeTable::new();
    let element_ty = MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32));
    let signature = MirKernelSignature {
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
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 4, y: 1, z: 1 },
    };
    assert!(signature.runtime_extent().is_none());
    // Static-axis fallback still drives the guard limit.
    assert_eq!(
        signature.oob_guard_limit(MirKernelBuiltin::GlobalInvocationX),
        4
    );
}

/// Build a `MirValidationContext` with device-tensor layouts so tensor
/// types resolve to device-handle device views (the kernel ABI requires
/// the device locus; mirrors the radix crate's `test_support` helper).
fn validation_with_device_tensor_layouts(types: &TypeTable) -> MirValidationContext<'_> {
    let mut validation = MirValidationContext::new(types);
    for index in 0..types.type_count() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = TypeId(index as u32);
        let Ok(layout) = validation.layouts.layout_for_type(MirType::semantic(ty)) else {
            continue;
        };
        let MirLayoutKind::DeviceView(view) = layout.kind else {
            continue;
        };
        validation.layouts.set_type_layout(
            ty,
            MirLayoutKind::DeviceView(crate::MirDeviceViewLayout::device_handle_row_major(
                view.element,
                view.rank,
            )),
        );
    }
    validation
}

/// Two-param tensor kernel: `functio addita(tf32[4] a, tf32[4] b) -> tf32[4]`.
fn multi_input_return_kernel_function(
    a_ty: MirType,
    b_ty: MirType,
    return_ty: MirType,
) -> MirFunction {
    MirFunction {
        id: MirFunctionId(10),
        source: Some(DefId(12)),
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: a_ty,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: b_ty,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

#[test]
fn multi_input_return_buffer_kernel_synthesizes_signature() {
    // C3-a: `addita(tf32[4] a, tf32[4] b) -> tf32[4]` matches the new
    // multi-input return-buffer arm instead of the fallthrough — one
    // input resource per param plus one output buffer, equal element
    // counts enforced.
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let element_ty = MirType::semantic(f32);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor_ty = MirType::semantic(types.tensor_with_shape(f32, shape));
    let validation = validation_with_device_tensor_layouts(&types);
    let function = multi_input_return_kernel_function(tensor_ty, tensor_ty, tensor_ty);
    let signature =
        MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("signature");

    assert_eq!(signature.input.binding, 0);
    assert_eq!(signature.input.role, MirKernelResourceRole::Input);
    assert_eq!(signature.input.access, MirKernelResourceAccess::Read);
    assert_eq!(signature.input.element_ty, element_ty);
    assert_eq!(signature.input.element_count, 4);
    assert_eq!(signature.extra_inputs.len(), 1);
    assert_eq!(signature.extra_inputs[0].binding, 1);
    assert_eq!(signature.extra_inputs[0].role, MirKernelResourceRole::Input);
    assert_eq!(
        signature.extra_inputs[0].access,
        MirKernelResourceAccess::Read
    );
    assert_eq!(signature.extra_inputs[0].element_ty, element_ty);
    assert_eq!(signature.extra_inputs[0].element_count, 4);
    assert_eq!(signature.output.binding, 2);
    assert_eq!(signature.output.role, MirKernelResourceRole::Output);
    assert_eq!(signature.output.access, MirKernelResourceAccess::Write);
    assert_eq!(signature.output.element_ty, element_ty);
    assert_eq!(signature.output.element_count, 4);
    assert_eq!(
        (
            signature.dispatch_size.x,
            signature.dispatch_size.y,
            signature.dispatch_size.z
        ),
        (4, 1, 1)
    );
    // Statically-known multi-element shape keeps the static limit — no
    // runtime-extent channel (U2 non-goals).
    assert!(signature.runtime_extent().is_none());
}

#[test]
fn multi_input_return_buffer_kernel_rejects_mismatched_counts() {
    // C3-a: mismatched element counts reject with the equality law's
    // error instead of silently producing a mismatched-shape signature.
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape4 = types.intern_index(IndexExpr::Literal(4));
    let shape8 = types.intern_index(IndexExpr::Literal(8));
    let tensor4 = MirType::semantic(types.tensor_with_shape(f32, shape4));
    let tensor8 = MirType::semantic(types.tensor_with_shape(f32, shape8));
    let validation = validation_with_device_tensor_layouts(&types);
    let function = multi_input_return_kernel_function(tensor4, tensor8, tensor4);
    let error = MirKernelSignature::storage_buffer_kernel(&function, &validation)
        .expect_err("mismatched element counts should reject");

    assert_eq!(error.message, ERR_KERNEL_IO_ELEMENT_MISMATCH);
}

/// Result-tuple kernel function:
/// `functio backward(tf32[4] x, tf32[4] w) → (tf32[4] gx, tf32[4] gw)`.
fn result_tuple_kernel_function(input_ty: MirType, tuple_return_ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(11),
        source: Some(DefId(13)),
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: input_ty,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: input_ty,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: tuple_return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

#[test]
fn result_tuple_kernel_synthesizes_multi_output_signature() {
    // S3-A1 done_when 1: a multi-output kernel signature (2+ outputs and
    // an accumulation-buffer resource) validates through the ABI contract
    // resolver and the plan pass.
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let element_ty = MirType::semantic(f32);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor_id = types.tensor_with_shape(f32, shape);
    let tensor_ty = MirType::semantic(tensor_id);
    let tuple_return_ty = MirType::semantic(types.intern(Type::Tuple(vec![tensor_id, tensor_id])));
    let validation = validation_with_device_tensor_layouts(&types);
    let function = result_tuple_kernel_function(tensor_ty, tuple_return_ty);
    let mut signature =
        MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("signature");

    // Two inputs (bindings 0, 1); two DISTINCT outputs (bindings 2, 3).
    assert_eq!(signature.input.binding, 0);
    assert_eq!(signature.extra_inputs.len(), 1);
    assert_eq!(signature.output.binding, 2);
    assert_eq!(signature.output.role, MirKernelResourceRole::Output);
    assert_eq!(signature.output.access, MirKernelResourceAccess::Write);
    assert_eq!(signature.output.element_ty, element_ty);
    assert_eq!(signature.output.element_count, 4);
    assert_eq!(signature.extra_outputs.len(), 1);
    assert_eq!(signature.extra_outputs[0].binding, 3);
    assert_eq!(
        signature.extra_outputs[0].role,
        MirKernelResourceRole::Output
    );
    assert_eq!(
        signature.extra_outputs[0].access,
        MirKernelResourceAccess::Write
    );
    assert_eq!(signature.extra_outputs[0].element_ty, element_ty);
    assert_eq!(signature.extra_outputs[0].element_count, 4);
    assert_eq!(signature.output_resources().count(), 2);

    // Attach an accumulation-buffer resource (S3-A1): read-write device
    // state bound after the outputs.
    signature.push_accumulation_buffer(MirKernelResource {
        group: 0,
        binding: 4,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::ReadWrite,
        element_ty,
        element_count: 4,
        source_local: None,
        source_name: None,
        reduced_projection: None,
    });
    assert_eq!(signature.accumulation_buffers.len(), 1);
    assert_eq!(signature.accumulation_buffers[0].binding, 4);

    // resources() sees both inputs + both outputs + the accumulation
    // buffer (five storage buffers in binding order).
    assert_eq!(signature.resources().count(), 5);

    // The plan pass accepts the multi-output signature (elementwise body
    // → no recipe, never an error).
    let plan =
        crate::device_program_plans::kernel_plan_for_function(&function, &signature, &validation)
            .expect("multi-output signature validates through the plan pass");
    assert!(plan.is_none(), "an elementwise body carries no recipe");

    // The result-tuple contract is coherent (distinct output slots).
    assert!(crate::kernel_plan::multi_output_contract_valid(&signature));
}

#[test]
fn result_tuple_gradient_outputs_bind_distinct_slots() {
    // S3-A1 done_when 2: the two gradient outputs of a companion's
    // (grad_x, grad_w) tuple bind to DISTINCT output slots — never a
    // shared slot, so each gradient lands in its own device buffer.
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor_id = types.tensor_with_shape(f32, shape);
    let tensor_ty = MirType::semantic(tensor_id);
    let tuple_return_ty = MirType::semantic(types.intern(Type::Tuple(vec![tensor_id, tensor_id])));
    let validation = validation_with_device_tensor_layouts(&types);
    let function = result_tuple_kernel_function(tensor_ty, tuple_return_ty);
    let signature =
        MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("signature");

    let outputs: Vec<_> = signature.output_resources().collect();
    assert_eq!(outputs.len(), 2);
    assert_ne!(
        outputs[0].binding, outputs[1].binding,
        "two gradient outputs must bind to distinct output slots"
    );
    assert_eq!(outputs[0].binding, 2);
    assert_eq!(outputs[1].binding, 3);
}

#[test]
fn result_tuple_outputs_are_separate_resources_not_an_aggregate() {
    // S3-A1 done_when 3: tuple outputs become SEPARATE output resources,
    // so no kernel value carries an aggregate type — the Metal emitter's
    // non-ordered aggregate rejection ("kernel tuple construct spread") is
    // unreachable through the multi-output ABI.
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let element_ty = MirType::semantic(f32);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor_id = types.tensor_with_shape(f32, shape);
    let tensor_ty = MirType::semantic(tensor_id);
    let tuple_return_ty = MirType::semantic(types.intern(Type::Tuple(vec![tensor_id, tensor_id])));
    let validation = validation_with_device_tensor_layouts(&types);
    let function = result_tuple_kernel_function(tensor_ty, tuple_return_ty);
    let signature =
        MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("signature");

    for output in signature.output_resources() {
        // Each output's element type resolves to a SCALAR layout (f32),
        // never an aggregate — the tuple return was split into separate
        // per-slot resources at the ABI boundary.
        assert_eq!(output.element_ty, element_ty);
        let layout = validation
            .layouts
            .layout_for_type(output.element_ty)
            .expect("output element type resolves");
        assert!(
            matches!(layout.kind, MirLayoutKind::Scalar(_)),
            "an output's element type must be a scalar, not an aggregate: {layout:?}"
        );
    }
}

/// R6 (P1): heterogeneous result-tuple outputs `[2]` and `[4]` — the
/// dispatch is the UNION (max = 4) of the per-output extents, never the
/// first output's count (2) alone; per-output extents are recorded in
/// binding order for the emitters' per-output guards.
#[test]
fn result_tuple_unequal_extents_dispatch_is_union_not_first_output() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape2 = types.intern_index(IndexExpr::Literal(2));
    let shape4 = types.intern_index(IndexExpr::Literal(4));
    let tensor2 = MirType::semantic(types.tensor_with_shape(f32, shape2));
    let tensor4 = MirType::semantic(types.tensor_with_shape(f32, shape4));
    let tuple_return_ty = MirType::semantic(types.intern(Type::Tuple(vec![
        tensor2.semantic_id(),
        tensor4.semantic_id(),
    ])));
    let validation = validation_with_device_tensor_layouts(&types);
    let function = result_tuple_kernel_function(tensor2, tuple_return_ty);
    let signature =
        MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("signature");

    // The dispatch covers BOTH outputs — 4, not the first output's 2.
    assert_eq!(
        signature.dispatch_size.x, 4,
        "dispatch must cover every output extent, not the first output's count alone"
    );
    // Per-output extent facts, in binding order.
    assert_eq!(
        crate::kernel_plan::multi_output_output_extents(&signature),
        vec![2, 4]
    );
    assert_eq!(
        crate::kernel_plan::multi_output_dispatch_extent(&signature),
        4
    );
}

/// R6 (P1): reversed order `[4]` then `[2]` — dispatch is still the
/// union (4); the per-output extents `[4, 2]` are what the emitters guard
/// with (threads 2..4 must not overrun the smaller buffer).
#[test]
fn result_tuple_unequal_extents_reverse_order_records_per_output_extents() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape2 = types.intern_index(IndexExpr::Literal(2));
    let shape4 = types.intern_index(IndexExpr::Literal(4));
    let tensor2 = MirType::semantic(types.tensor_with_shape(f32, shape2));
    let tensor4 = MirType::semantic(types.tensor_with_shape(f32, shape4));
    let tuple_return_ty = MirType::semantic(types.intern(Type::Tuple(vec![
        tensor4.semantic_id(),
        tensor2.semantic_id(),
    ])));
    let validation = validation_with_device_tensor_layouts(&types);
    let function = result_tuple_kernel_function(tensor4, tuple_return_ty);
    let signature =
        MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("signature");

    assert_eq!(signature.dispatch_size.x, 4);
    assert_eq!(signature.output.element_count, 4);
    assert_eq!(signature.extra_outputs[0].element_count, 2);
    assert_eq!(
        crate::kernel_plan::multi_output_output_extents(&signature),
        vec![4, 2]
    );
    assert_eq!(
        crate::kernel_plan::multi_output_dispatch_extent(&signature),
        4
    );
}

/// S3-A5: the scalar-return reduction signature (the S3-B1 forward shape
/// `simple_loss(x, w) → f32` with `mean(mul(x, w))`) synthesizes the
/// indexed-view reduction contract: inputs x + w, output = the 4 partial
/// slots, workgroup 256, dispatch 1000 — and the plan pass derives the
/// tree-reduction recipe over that signature (the S3-A3 fail-closed
/// surface).
#[test]
fn reduction_scalar_return_kernel_synthesizes_indexed_view_signature() {
    let mut types = TypeTable::new();
    let function = mul_mean_scalar_return_function(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation)
        .expect("scalar-return reduction derives a signature");

    assert_eq!(signature.input.element_count, 1000);
    assert_eq!(signature.extra_inputs.len(), 1);
    assert_eq!(signature.extra_inputs[0].element_count, 1000);
    // Reduction contract: partials output, corrected workgroup/dispatch.
    assert_eq!(signature.workgroup.x, 256);
    assert_eq!(signature.workgroup.y, 1);
    assert_eq!(signature.dispatch_size.x, 1000);
    assert_eq!(signature.output.element_count, 4);

    // The S3-A3 fail-closed plan surface accepts the signature: the
    // mul+mean forward carries the tree-reduction recipe.
    let derived =
        crate::device_program_plans::kernel_plan_for_function(&function, &signature, &validation)
            .expect("plan facts resolve")
            .expect("mul+mean forward carries the reduction recipe");
    assert!(
        matches!(
            derived,
            crate::kernel_plan::CollectionKernelPlan::TreeReduction(_)
        ),
        "the mul+mean forward must carry a tree-reduction plan, got {derived:?}"
    );
}

/// S3-B1 forward shape:
/// `functio simple_loss(tf32[1000] x, tf32[1000] w) → f32 {
///    fixum tensor<f32,[1000]> product ← x.multiplica(w); redde product.media() }`.
fn mul_mean_scalar_return_function(types: &mut TypeTable) -> MirFunction {
    let f32_id = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.intern_index(IndexExpr::Literal(1000));
    let tensor_id = types.tensor_with_shape(f32_id, shape);
    let tensor_ty = MirType::semantic(tensor_id);
    let f32_ty = MirType::semantic(f32_id);
    let span = Span::default();
    MirFunction {
        id: MirFunctionId(0),
        source: Some(DefId(12)),
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor_ty,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: tensor_ty,
                mode: MirParamMode::Owned,
                span,
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor_ty,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: tensor_ty,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: tensor_ty,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(3),
                name: None,
                ty: f32_ty,
                mutable: false,
                span,
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                // product = mul(x, w)
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(2))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMul),
                            args: vec![
                                MirOperand::Place(MirPlace::local(MirLocalId(0))),
                                MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            ],
                            return_ty: tensor_ty,
                        },
                    },
                    span,
                },
                // result = mean(product)
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(3))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMean),
                            args: vec![MirOperand::Place(MirPlace::local(MirLocalId(2)))],
                            return_ty: f32_ty,
                        },
                    },
                    span,
                },
            ],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(3),
                )))),
                span,
            },
            span,
        }],
        return_ty: f32_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn result_tuple_errors_classify_to_specific_issue_tags() {
    // S3-A1 residual N1: the two result-tuple ABI errors classify to
    // their own issue tags instead of falling through to generic
    // `mir_abi_error`.
    let required = MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRED);
    assert_eq!(required.issue, "mir_kernel_result_tuple_required");
    let requires_input = MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRES_INPUT);
    assert_eq!(
        requires_input.issue,
        "mir_kernel_result_tuple_requires_input"
    );
}

/// Rank-2 transpose kernel shape (S5-U1):
/// `functio transpono(tf32[M,N] x) → tf32[N,M] {
///    fixum tensor<f32,[N,M]> t ← transponi(x); redde t }`.
fn transpose_rank2_function(types: &mut TypeTable, dims: [u64; 2]) -> MirFunction {
    let f32_id = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_id, shape)));

    // The transposed `[N,M]` type (the destination / return).
    let out_dims = dims
        .iter()
        .rev()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let out_shape = types.intern_index(IndexExpr::Tuple(out_dims));
    let out_ty = MirType::semantic(types.intern(Type::Tensor(f32_id, out_shape)));

    let span = Span::default();
    MirFunction {
        id: MirFunctionId(0),
        source: Some(DefId(12)),
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: tensor_ty,
            mode: MirParamMode::Owned,
            span,
        }],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor_ty,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: out_ty,
                mutable: false,
                span,
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorTranspose),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                        return_ty: out_ty,
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
        return_ty: out_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn transpose_contract_resolves_rank2_dims() {
    // S5-U1 dispatch contract facts: the resolver reads the literal
    // rank-2 operand dims `[M,N]` off the tensor type.
    let mut types = TypeTable::new();
    let function = transpose_rank2_function(&mut types, [4, 4]);
    let validation = MirValidationContext::new(&types);
    let contract = collection_op_contract(&function, &validation)
        .expect("transpose facts resolve")
        .expect("transpose body carries a dispatch contract");
    assert_eq!(
        contract,
        CollectionOpContract::Transpose { m: 4, n: 4 },
        "the rank-2 transpose must resolve the M×N dispatch contract"
    );
}

#[test]
fn transpose_contract_rejects_rank1_operand() {
    // A rank-1 operand has no `[M,N] → [N,M]` remap — typed diagnostic,
    // never a silent contract.
    let mut types = TypeTable::new();
    let f32_id = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_id, shape)));
    let span = Span::default();
    let function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: tensor_ty,
            mode: MirParamMode::Owned,
            span,
        }],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorTranspose),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                        return_ty: tensor_ty,
                    },
                },
                span,
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span,
            },
            span,
        }],
        return_ty: tensor_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    };
    let validation = MirValidationContext::new(&types);
    let error = collection_op_contract(&function, &validation)
        .expect_err("rank-1 transpose must fail the contract resolver");
    assert!(
        error.message.contains("rank-2"),
        "diagnostic must demand a rank-2 operand, got: {}",
        error.message
    );
}

#[test]
fn transpose_signature_applies_elementwise_style_dispatch() {
    // S5-U1 apply arm: a square MLP 4×4 transpose kernel carries the
    // elementwise-style 1D contract — workgroup (64,1,1), dispatch
    // (M·N,1,1), input/output sized M·N. The generic return-buffer ABI
    // synthesis seeds the 1D shape; the contract correction applies the
    // transpose dispatch on top.
    let mut types = TypeTable::new();
    let element_ty = MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32));
    let function = transpose_rank2_function(&mut types, [4, 4]);
    let validation = validation_with_device_tensor_layouts(&types);

    // The generic base signature the return-buffer ABI seeds (workgroup
    // (1,1,1), 1D dispatch over the output element count).
    let mut signature = MirKernelSignature {
        function: function.id,
        input: MirKernelResource {
            group: 0,
            binding: 0,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty,
            element_count: 16,
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
            element_count: 16,
            source_local: None,
            source_name: None,
            reduced_projection: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 16, y: 1, z: 1 },
    };

    // `apply_collection_op_contract` corrects the generic 1D dispatch to
    // the transpose contract facts.
    apply_collection_op_contract(&function, &mut signature, &validation)
        .expect("transpose contract applies");
    assert_eq!(
        signature.workgroup,
        MirWorkgroupSize {
            x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
            y: 1,
            z: 1,
        }
    );
    assert_eq!(
        signature.dispatch_size,
        MirKernelDispatchSize { x: 16, y: 1, z: 1 }
    );
    assert_eq!(signature.input.element_count, 16);
    assert_eq!(signature.output.element_count, 16);
    assert!(signature.extra_inputs.is_empty());

    // The typed plan pass admits the transpose kernel: the body resolves
    // the rank-2 Transpose recipe over the contract-shaped signature.
    let derived =
        crate::device_program_plans::kernel_plan_for_function(&function, &signature, &validation)
            .expect("transpose facts resolve")
            .expect("transpose body carries the transpose recipe");
    assert!(
        matches!(
            derived,
            crate::kernel_plan::CollectionKernelPlan::Transpose(_)
        ),
        "the transpose body must carry a transpose plan, got {derived:?}"
    );
    derived.validate().expect("transpose plan validates");
}

#[test]
fn return_buffer_kernel_applies_transpose_contract_for_rectangular_dims() {
    // S5-U1c: the single-input return-buffer ABI arm calls
    // `apply_collection_op_contract` itself, so a rectangular rank-2
    // transpose `[M,N] → [N,M]` (M ≠ N) synthesized through the full
    // `storage_buffer_kernel` path carries the transpose dispatch
    // contract (workgroup TRANSPOSE_WORKGROUP_X, dispatch M·N) instead of
    // the generic 1D contract — with input/output element counts equal to
    // the flat M·N.
    let mut types = TypeTable::new();
    let function = transpose_rank2_function(&mut types, [2, 3]);
    let validation = validation_with_device_tensor_layouts(&types);
    let signature =
        MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("signature");

    assert_eq!(
        signature.workgroup,
        MirWorkgroupSize {
            x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
            y: 1,
            z: 1,
        },
        "the ABI must apply the transpose workgroup contract automatically"
    );
    assert_eq!(
        signature.dispatch_size,
        MirKernelDispatchSize { x: 6, y: 1, z: 1 },
        "the rectangular transpose [2,3] → [3,2] dispatches over M·N elements"
    );
    assert_eq!(signature.input.element_count, 6);
    assert_eq!(signature.output.element_count, 6);

    // The typed plan pass admits the contract-shaped transpose kernel.
    let derived =
        crate::device_program_plans::kernel_plan_for_function(&function, &signature, &validation)
            .expect("transpose facts resolve")
            .expect("transpose body carries the transpose recipe");
    assert!(
        matches!(
            derived,
            crate::kernel_plan::CollectionKernelPlan::Transpose(_)
        ),
        "the transpose body must carry a transpose plan, got {derived:?}"
    );
    derived.validate().expect("transpose plan validates");
}

#[test]
fn multi_input_return_buffer_kernel_keeps_plain_elementwise_contract() {
    // S5-U1c: a plain elementwise multi-input kernel (no collection op in
    // its body) is untouched by the contract application — the generic 1D
    // dispatch and (1,1,1) workgroup remain, and the extra-input count
    // still holds one per param.
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor_ty = MirType::semantic(types.tensor_with_shape(f32, shape));
    let validation = validation_with_device_tensor_layouts(&types);
    let function = multi_input_return_kernel_function(tensor_ty, tensor_ty, tensor_ty);
    let signature =
        MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("signature");

    assert_eq!(
        signature.workgroup,
        MirWorkgroupSize { x: 1, y: 1, z: 1 },
        "an elementwise multi-input kernel keeps the default workgroup"
    );
    assert_eq!(
        signature.dispatch_size,
        MirKernelDispatchSize { x: 4, y: 1, z: 1 },
        "an elementwise multi-input kernel keeps the generic 1D dispatch"
    );
    assert_eq!(signature.extra_inputs.len(), 1);
    assert_eq!(signature.extra_inputs[0].element_count, 4);
    assert_eq!(signature.output.element_count, 4);
}

#[test]
fn kernel_abi_keeps_f64_scalar_fail_closed_over_f32_tensor_graph() {
    // S5A-U2 (CTO correction): the whole-function ABI scalar surface keeps an
    // f64 scalar FAIL CLOSED even over an f32 tensor graph — the former
    // graph-wide "all tensors are f32" narrowing (D-6a) is gone, because
    // graph shape proves nothing about an individual f64 scalar's value. The
    // companion is `backward(x, w, nil(), upstream: fractus) →
    // (grad_x, grad_w)` — the S3-A2 tuple-return shape with an f64 seed over
    // f32 tensor params: the exact shape the old rule admitted, now rejected
    // with the typed `mir_kernel_scalar_layout_unsupported` diagnostic.
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let f64 = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let f64_ty = MirType::semantic(f64);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor_id = types.tensor_with_shape(f32, shape);
    let tensor_ty = MirType::semantic(tensor_id);
    let tuple_return_ty = MirType::semantic(types.intern(Type::Tuple(vec![tensor_id, tensor_id])));
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let validation = validation_with_device_tensor_layouts(&types);

    let function = MirFunction {
        id: MirFunctionId(20),
        source: Some(DefId(20)),
        name: None,
        params: vec![
            MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor_ty,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: tensor_ty,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
            MirParam {
                local: MirLocalId(2),
                name: None,
                ty: vacuum_ty,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
            MirParam {
                local: MirLocalId(3),
                name: None,
                ty: f64_ty,
                mode: MirParamMode::Owned,
                span: Span::default(),
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: tensor_ty,
                mutable: false,
                span: Span::default(),
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: tensor_ty,
                mutable: false,
                span: Span::default(),
            },
            MirLocal {
                id: MirLocalId(3),
                name: None,
                ty: f64_ty,
                mutable: false,
                span: Span::default(),
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: tuple_return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };

    let error = MirKernelSignature::storage_buffer_kernel(&function, &validation)
        .expect_err("an f64 scalar over an f32 tensor graph must fail closed");

    assert_eq!(error.issue, "mir_kernel_scalar_layout_unsupported");
    assert_eq!(error.detail.as_deref(), Some("f64"));
}

#[test]
fn kernel_abi_keeps_f64_scalar_fail_closed_for_pure_scalar_kernel() {
    // S5A-U2: the whole-function scalar surface rejects a PURE-scalar f64
    // kernel (no tensor graph at all) with the same typed diagnostic —
    // `add_one(fractus x) → fractus` stays a kernel scalar layout f64
    // rejection, exactly as before the D-6a rule existed.
    let mut types = TypeTable::new();
    let f64 = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let f64_ty = MirType::semantic(f64);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = scalar_kernel_function(f64_ty);
    let error = MirKernelSignature::storage_buffer_kernel(&function, &validation)
        .expect_err("a pure-scalar f64 kernel must keep the fail-closed diagnostic");

    assert_eq!(error.issue, "mir_kernel_scalar_layout_unsupported");
    assert_eq!(error.detail.as_deref(), Some("f64"));
}

/// Build a tensor-tensor `TensorAdd` kernel: `out = add(left, right)` with
/// the given left/right/result tensor types.
fn tensor_add_kernel(
    left_ty: MirType,
    right_ty: MirType,
    result_ty: MirType,
    left_local: MirLocalId,
    right_local: MirLocalId,
    result_local: MirLocalId,
) -> MirFunction {
    let span = Span::default();
    MirFunction {
        id: MirFunctionId(0),
        source: Some(DefId(9)),
        name: None,
        params: vec![
            MirParam {
                local: left_local,
                name: None,
                ty: left_ty,
                mode: MirParamMode::Owned,
                span,
            },
            MirParam {
                local: right_local,
                name: None,
                ty: right_ty,
                mode: MirParamMode::Owned,
                span,
            },
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(result_local)),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
                        args: vec![
                            MirOperand::Place(MirPlace::local(left_local)),
                            MirOperand::Place(MirPlace::local(right_local)),
                        ],
                        return_ty: result_ty,
                    },
                },
                span,
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    result_local,
                )))),
                span,
            },
            span,
        }],
        return_ty: result_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

/// Tensor types for the S6-C2 rank-extension pair: `[2,8]`, `[8]`, `[2]`.
fn rank_extension_types(types: &mut TypeTable) -> (MirType, MirType, MirType, MirType) {
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let f32_ty = MirType::semantic(f32);
    let dim2 = types.intern_index(IndexExpr::Literal(2));
    let dim8 = types.intern_index(IndexExpr::Literal(8));
    let shape_b = types.intern_index(IndexExpr::Tuple(vec![dim8]));
    let shape_x = types.intern_index(IndexExpr::Tuple(vec![dim2, dim8]));
    let shape_tail_wrong = types.intern_index(IndexExpr::Tuple(vec![dim2]));
    (
        MirType::semantic(types.tensor_with_shape(f32, shape_x)),
        MirType::semantic(types.tensor_with_shape(f32, shape_b)),
        MirType::semantic(types.tensor_with_shape(f32, shape_tail_wrong)),
        f32_ty,
    )
}

/// S6-C2: the device ABI records the typed rank-extension broadcast fact for
/// a `[2,8]` + `[8]` tensor-tensor add (the `addita_bias` lowering).
#[test]
fn abi_records_rank_extension_broadcast_fact() {
    let mut types = TypeTable::new();
    let (x_ty, b_ty, _wrong_ty, _) = rank_extension_types(&mut types);
    let validation = MirValidationContext::new(&types);
    let function = tensor_add_kernel(
        x_ty,
        b_ty,
        x_ty,
        MirLocalId(0),
        MirLocalId(1),
        MirLocalId(2),
    );

    let fact = rank_extension_broadcast_fact(&function, &validation)
        .expect("the [2,8]+[8] pair must resolve")
        .expect("the kernel carries a broadcast fact");

    assert_eq!(fact.lhs_shape, vec![2, 8]);
    assert_eq!(fact.rhs_shape, vec![8]);
    assert_eq!(fact.result_shape, vec![2, 8]);
    assert_eq!(fact.declaration, MirBroadcastDeclaration::RankExtension);
}

/// S6-C2: a same-shape tensor-tensor add (the general `addita`) carries no
/// broadcast fact — the fact is only for rank-extension pairs.
#[test]
fn abi_records_no_fact_for_same_shape_add() {
    let mut types = TypeTable::new();
    let (x_ty, _, _, _) = rank_extension_types(&mut types);
    let validation = MirValidationContext::new(&types);
    let function = tensor_add_kernel(
        x_ty,
        x_ty,
        x_ty,
        MirLocalId(0),
        MirLocalId(1),
        MirLocalId(2),
    );

    let fact = rank_extension_broadcast_fact(&function, &validation).expect("no failure");
    assert!(fact.is_none(), "same-shape add carries no broadcast fact");
}

/// S6-C2: a tensor-tensor add whose shape pair is neither same-shape nor a
/// valid rank extension fails closed with a typed diagnostic (A3/A7 — the
/// fact cannot be recorded, so the kernel must not reach emission).
#[test]
fn abi_rejects_invalid_broadcast_pair_fail_closed() {
    let mut types = TypeTable::new();
    let (x_ty, _, wrong_tail_ty, _) = rank_extension_types(&mut types);
    // `[2,8]` + `[2]`: rank difference is one, but `[2]` is not the trailing
    // dims `[8]` — an invalid broadcast pair.
    let validation = MirValidationContext::new(&types);
    let function = tensor_add_kernel(
        x_ty,
        wrong_tail_ty,
        x_ty,
        MirLocalId(0),
        MirLocalId(1),
        MirLocalId(2),
    );

    let error = rank_extension_broadcast_fact(&function, &validation)
        .expect_err("an invalid broadcast pair must fail closed");

    assert!(
        error
            .message
            .contains("neither same-shape nor a rank extension"),
        "typed diagnostic expected, got: {}",
        error.message
    );
    assert_eq!(error.issue, "mir_abi_error");
}

fn rank2_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32_id = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32_id, shape)))
}

fn packed_binary_function(
    types: &mut TypeTable,
    left_dims: [u64; 2],
    right_dims: [u64; 2],
    out_dims: [u64; 2],
    op: MirCollectionOp,
    layout_id: i64,
) -> MirFunction {
    let left = rank2_ty(types, left_dims);
    let right = rank2_ty(types, right_dims);
    let out = rank2_ty(types, out_dims);
    let span = Span::default();
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
                span,
            },
            MirParam {
                local: MirLocalId(1),
                name: None,
                ty: right,
                mode: MirParamMode::Owned,
                span,
            },
        ],
        locals: vec![
            MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: left,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(1),
                name: None,
                ty: right,
                mutable: false,
                span,
            },
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(2))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(op),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            MirOperand::Constant(MirConstant::Int(layout_id)),
                        ],
                        return_ty: out,
                    },
                },
                span,
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(2),
                )))),
                span,
            },
            span,
        }],
        return_ty: out,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn quantized_matmul_contract_resolves_q4k_layout() {
    let mut types = TypeTable::new();
    let function = packed_binary_function(
        &mut types,
        [8, 16],
        [16, 16],
        [8, 16],
        MirCollectionOp::TensorQuantizedMatMul,
        12,
    );
    let validation = MirValidationContext::new(&types);
    let contract = collection_op_contract(&function, &validation)
        .expect("Q4_K facts resolve")
        .expect("Q4_K body carries a packed contract");
    assert_eq!(
        contract,
        CollectionOpContract::QuantizedMatMul {
            dispatch_x: 2,
            dispatch_y: 1,
            m: 8,
            k: 16,
            n: 16,
            layout: PackedStorageLayout::Q4_K,
        }
    );
}

#[test]
fn quantized_matmul_contract_rejects_unknown_layout() {
    let mut types = TypeTable::new();
    let function = packed_binary_function(
        &mut types,
        [8, 16],
        [16, 16],
        [8, 16],
        MirCollectionOp::TensorQuantizedMatMul,
        99,
    );
    let validation = MirValidationContext::new(&types);
    let error = collection_op_contract(&function, &validation)
        .expect_err("unknown GGML type id must fail closed");
    assert!(
        error.message.contains("admitted GGML type id"),
        "typed diagnostic expected, got: {}",
        error.message
    );
}

fn rank3_ty(types: &mut TypeTable, dims: [u64; 3]) -> MirType {
    let f32_id = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32_id, shape)))
}

/// V projection: input `[M, hidden]`, weights `[hidden, kv_heads * head_dim]`,
/// output `[kv_heads, M, head_dim]`. The seq const may differ from M — that
/// is the decode-regime case (context N+1, this step one row).
fn v_projection_function(
    types: &mut TypeTable,
    m: u64,
    seq_const: u64,
    kv_heads: u64,
    q_per_kv: u64,
    head_dim: u64,
    hidden: u64,
) -> MirFunction {
    let input = rank2_ty(types, [m, hidden]);
    let weights = rank2_ty(types, [hidden, kv_heads * head_dim]);
    let output = rank3_ty(types, [kv_heads, m, head_dim]);
    let u32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let span = Span::default();
    let param = |local: u32, ty: MirType| MirParam {
        local: MirLocalId(local),
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span,
    };
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            param(0, input),
            param(1, weights),
            param(2, output),
            param(3, u32_ty),
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(2))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(
                            MirCollectionOp::TensorBatchedAttention,
                        ),
                        args: vec![
                            MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            MirOperand::Place(MirPlace::local(MirLocalId(1))),
                            MirOperand::Constant(MirConstant::Int(6)), // Q5_0
                            MirOperand::Constant(MirConstant::Int(2)), // KIND_V
                            MirOperand::Constant(MirConstant::Int(kv_heads as i64)),
                            MirOperand::Constant(MirConstant::Int(q_per_kv as i64)),
                            MirOperand::Constant(MirConstant::Int(seq_const as i64)),
                            MirOperand::Constant(MirConstant::Int(head_dim as i64)),
                            MirOperand::Constant(MirConstant::Int(hidden as i64)),
                        ],
                        return_ty: output,
                    },
                },
                span,
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span,
            },
            span,
        }],
        return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn projection_gemv_dispatch_uses_step_rows_not_seq_const() {
    // Decode continuation: seq const is N+1 (literal sequence position), but
    // THIS step has one row (M=1). GEMV launch must expand by NR0=4 → ×8.
    let mut types = TypeTable::new();
    let function = v_projection_function(&mut types, 1, 10, 2, 3, 4, 24);
    let validation = MirValidationContext::new(&types);
    let contract = collection_op_contract(&function, &validation)
        .expect("M=1 V projection at seq=N+1 must resolve")
        .expect("batched attention contract");
    let CollectionOpContract::BatchedAttention {
        kind,
        seq,
        output_elements,
        ..
    } = contract
    else {
        panic!("expected BatchedAttention, got {contract:?}");
    };
    assert_eq!(kind, crate::kernel_plan::BatchedAttentionKind::ProjectionV);
    assert_eq!(
        seq, 1,
        "plan seq is THIS step's row count, not the seq const"
    );
    assert_eq!(
        output_elements, 64,
        "logical 2*1*4=8 columns × 8 GEMV lanes"
    );
}

#[test]
fn projection_prefill_does_not_expand_when_step_has_many_rows() {
    let mut types = TypeTable::new();
    let function = v_projection_function(&mut types, 10, 10, 2, 3, 4, 24);
    let validation = MirValidationContext::new(&types);
    let contract = collection_op_contract(&function, &validation)
        .expect("M=10 V projection must resolve")
        .expect("batched attention contract");
    let CollectionOpContract::BatchedAttention {
        seq,
        output_elements,
        ..
    } = contract
    else {
        panic!("expected BatchedAttention, got {contract:?}");
    };
    assert_eq!(seq, 10);
    assert_eq!(output_elements, 80, "logical 2*10*4=80, no GEMV expansion");
}
