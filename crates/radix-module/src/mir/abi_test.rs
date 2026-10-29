//! MIR kernel ABI: reflection, launch plans, bind groups, vertex input, rejection contracts.
//!
//! Tests: MirKernelReflection, MirKernelLaunchPlan, vertex reflection, GPU builtin inputs,
//! tensor-view ABI mapping, and explicit rejection cases (unstable layouts, atomics, overflow).

use super::abi::collection_op_workgroup_count;
use super::test_support::*;
use super::{
    MirColorTargetFormat, MirFragmentOutputReflection, MirFunctionAbi, MirInputStepMode, MirKernelAxis,
    MirKernelBufferBindingKind, MirKernelBuiltin, MirKernelBuiltinInput, MirKernelDispatchSize, MirKernelOffsetMode,
    MirKernelReflection, MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceRole,
    MirKernelShaderAccess, MirKernelShaderStage, MirKernelSignature, MirRuntimeAbi, MirRuntimeAbiFamily,
    MirRuntimeAbiKind, MirVaryingInterpolation, MirVaryingReflection, MirVertexInputFormat, MirVertexInputReflection,
    MirWebGpuVertexStepMode, MirWorkgroupSize,
};
use crate::lexer::Interner;
use crate::mir::{
    MirBlock, MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirLocalId, MirOperand, MirParam,
    MirParamMode, MirRuntimeCall, MirScalarLayout, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind,
    MirType, MirValidationContext,
};
use crate::semantic::{NumericWidth, Primitive, TypeTable};
use crate::test_support::test_span;

#[test]
fn kernel_abi_scalar_signature_function_and_input() {
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(ty);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");

    assert_eq!(signature.function, MirFunctionId(7));
    assert_eq!(signature.input.group, 0);
    assert_eq!(signature.input.binding, 0);
    assert_eq!(signature.input.role, MirKernelResourceRole::Input);
    assert_eq!(signature.input.access, MirKernelResourceAccess::Read);
    assert_eq!(signature.input.element_ty, ty);
    assert_eq!(signature.input.element_count, 1);
    assert_eq!(signature.input.source_local, Some(MirLocalId(3)));
}

#[test]
fn kernel_abi_scalar_signature_output_properties() {
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(ty);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");

    assert_eq!(signature.output.group, 0);
    assert_eq!(signature.output.binding, 1);
    assert_eq!(signature.output.role, MirKernelResourceRole::Output);
    assert_eq!(signature.output.access, MirKernelResourceAccess::Write);
    assert_eq!(signature.output.element_ty, ty);
    assert_eq!(signature.output.element_count, 1);
    assert_eq!(signature.output.source_local, None);
}

#[test]
fn kernel_abi_scalar_signature_unit_dimensions() {
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(ty);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");

    assert_eq!((signature.workgroup.x, signature.workgroup.y, signature.workgroup.z), (1, 1, 1));
    assert_eq!(
        (signature.dispatch_size.x, signature.dispatch_size.y, signature.dispatch_size.z),
        (1, 1, 1)
    );
}

// ============================================================================
// Split tests: kernel_reflection_records_signature_layout_facts (was 1 test, now 7)
// ============================================================================

#[test]
fn kernel_reflection_records_function_id() {
    let (_types, _ty, reflection, _launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(reflection.function, MirFunctionId(7));
}

#[test]
fn kernel_reflection_records_entry_name() {
    let (_types, _ty, reflection, _launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(reflection.entry_name, "add_one");
}

#[test]
fn kernel_reflection_has_no_builtin_inputs() {
    let (_types, _ty, reflection, _launch) = setup_simple_kernel_reflection_launch_plan();
    assert!(reflection.builtin_inputs.is_empty());
}

#[test]
fn kernel_reflection_records_unit_dimensions() {
    let (_types, _ty, reflection, _launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(
        (reflection.workgroup.x, reflection.workgroup.y, reflection.workgroup.z),
        (1, 1, 1)
    );
    assert_eq!(
        (
            reflection.dispatch_size.x,
            reflection.dispatch_size.y,
            reflection.dispatch_size.z
        ),
        (1, 1, 1)
    );
    assert_eq!(
        (
            reflection.workgroup_count.x,
            reflection.workgroup_count.y,
            reflection.workgroup_count.z
        ),
        (1, 1, 1)
    );
}

#[test]
fn kernel_reflection_records_two_resources() {
    let (_types, _ty, reflection, _launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(reflection.resources.len(), 2);
}

#[test]
fn kernel_reflection_records_input_resource_0_properties() {
    let (_types, ty, reflection, _launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(reflection.resources[0].group, 0);
    assert_eq!(reflection.resources[0].binding, 0);
    assert_eq!(reflection.resources[0].role, MirKernelResourceRole::Input);
    assert_eq!(reflection.resources[0].access, MirKernelResourceAccess::Read);
    assert_eq!(reflection.resources[0].element_ty, ty);
    assert_eq!(reflection.resources[0].element_layout, MirScalarLayout::F32);
    assert_eq!(reflection.resources[0].element_byte_width, 4);
    assert_eq!(reflection.resources[0].element_count, 1);
    assert_eq!(reflection.resources[0].buffer_byte_len, 4);
    assert_eq!(reflection.resources[0].source_local, Some(MirLocalId(3)));
}

#[test]
fn kernel_reflection_records_output_resource_1_properties() {
    let (_types, ty, reflection, _launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(reflection.resources[1].group, 0);
    assert_eq!(reflection.resources[1].binding, 1);
    assert_eq!(reflection.resources[1].role, MirKernelResourceRole::Output);
    assert_eq!(reflection.resources[1].access, MirKernelResourceAccess::Write);
    assert_eq!(reflection.resources[1].element_ty, ty);
    assert_eq!(reflection.resources[1].element_layout, MirScalarLayout::F32);
    assert_eq!(reflection.resources[1].element_byte_width, 4);
    assert_eq!(reflection.resources[1].element_count, 1);
    assert_eq!(reflection.resources[1].buffer_byte_len, 4);
    assert_eq!(reflection.resources[1].source_local, None);
}

// ============================================================================
// Split tests: kernel_launch_plan_groups_reflected_bindings (was 1 test, now 17)
// ============================================================================

#[test]
fn simple_launch_plan_reflection_records_storage_buffer_resource_kinds() {
    let (_types, _ty, reflection, _launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(reflection.resources[0].kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(reflection.resources[1].kind, MirKernelResourceKind::StorageBuffer);
}

#[test]
fn simple_launch_plan_records_entry_name_and_shader_stage() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(launch.entry_name, "add_one");
    assert_eq!(launch.shader_stage, MirKernelShaderStage::Compute);
}

#[test]
fn simple_launch_plan_records_workgroup_and_dispatch_dimensions() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!((launch.workgroup.x, launch.workgroup.y, launch.workgroup.z), (1, 1, 1));
    assert_eq!(
        (launch.dispatch_size.x, launch.dispatch_size.y, launch.dispatch_size.z),
        (1, 1, 1)
    );
    assert_eq!(
        (launch.workgroup_count.x, launch.workgroup_count.y, launch.workgroup_count.z),
        (1, 1, 1)
    );
}

#[test]
fn simple_launch_plan_has_one_bind_group() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(launch.bind_groups.len(), 1);
    assert_eq!(launch.bind_group_count, 1);
    assert_eq!(launch.pipeline_layout.bind_group_layout_count, 1);
    assert_eq!(launch.pipeline_layout.bind_group_layouts.len(), 1);
}

#[test]
fn simple_launch_plan_bind_group_0_index_and_pipeline_layout_match() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(launch.pipeline_layout.bind_group_layouts[0].bind_group_index, 0);
    assert_eq!(launch.pipeline_layout.bind_group_layouts[0].group, 0);
    assert_eq!(launch.pipeline_layout.bind_group_layouts[0].layout_entry_count, 2);
    assert_eq!(launch.bind_groups[0].bind_group_index, 0);
    assert_eq!(launch.bind_groups[0].group, 0);
}

#[test]
fn simple_launch_plan_bind_group_0_resource_indexes_kinds_and_roles() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(launch.bind_groups[0].resource_indexes, vec![0, 1]);
    assert_eq!(launch.bind_groups[0].resource_index_count, 2);
    assert_eq!(
        launch.bind_groups[0].resource_kinds,
        vec![
            MirKernelResourceKind::StorageBuffer,
            MirKernelResourceKind::StorageBuffer
        ]
    );
    assert_eq!(launch.bind_groups[0].resource_kind_count, 2);
    assert_eq!(
        launch.bind_groups[0].resource_roles,
        vec![MirKernelResourceRole::Input, MirKernelResourceRole::Output]
    );
    assert_eq!(launch.bind_groups[0].resource_role_count, 2);
}

#[test]
fn simple_launch_plan_bind_group_0_shader_accesses_and_buffer_types() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(
        launch.bind_groups[0].shader_accesses,
        vec![MirKernelShaderAccess::Read, MirKernelShaderAccess::ReadWrite]
    );
    assert_eq!(launch.bind_groups[0].shader_access_count, 2);
    assert_eq!(
        launch.bind_groups[0].buffer_binding_types,
        vec![
            MirKernelBufferBindingKind::ReadOnlyStorage,
            MirKernelBufferBindingKind::Storage
        ]
    );
    assert_eq!(launch.bind_groups[0].buffer_binding_type_count, 2);
}

#[test]
fn simple_launch_plan_bind_group_0_buffer_byte_sizes_match_element_counts() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(launch.bind_groups[0].buffer_byte_offsets, vec![0, 0]);
    assert_eq!(launch.bind_groups[0].buffer_byte_offset_count, 2);
    assert_eq!(launch.bind_groups[0].buffer_byte_lens, vec![4, 4]);
    assert_eq!(launch.bind_groups[0].buffer_byte_len_count, 2);
    assert_eq!(launch.bind_groups[0].binding_byte_lens, vec![4, 4]);
    assert_eq!(launch.bind_groups[0].binding_byte_len_count, 2);
    assert_eq!(launch.bind_groups[0].min_binding_sizes, vec![4, 4]);
    assert_eq!(launch.bind_groups[0].min_binding_size_count, 2);
}

#[test]
fn simple_launch_plan_bind_group_0_offsets_are_fixed_and_non_dynamic() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(launch.bind_groups[0].has_dynamic_offsets, vec![false, false]);
    assert_eq!(launch.bind_groups[0].dynamic_offset_flag_count, 2);
    assert_eq!(
        launch.bind_groups[0].offset_modes,
        vec![MirKernelOffsetMode::Fixed, MirKernelOffsetMode::Fixed]
    );
    assert_eq!(launch.bind_groups[0].offset_mode_entry_count, 2);
    assert!(!launch.bind_groups[0].requires_dynamic_offsets);
    assert!(launch.bind_groups[0].has_non_dynamic_offsets);
    assert!(!launch.bind_groups[0].has_mixed_offset_modes);
    assert_eq!(launch.bind_groups[0].offset_mode_count, 1);
    assert!(launch.bind_groups[0].has_only_non_dynamic_offsets);
    assert!(!launch.bind_groups[0].has_only_dynamic_offsets);
}

#[test]
fn simple_launch_plan_bind_group_0_non_dynamic_offset_indexes_are_sequential() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_count, 2);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_resource_indexes, vec![0, 1]);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_resource_index_count, 2);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_binding_indexes, vec![0, 1]);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_binding_index_count, 2);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_layout_entry_indexes, vec![0, 1]);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_layout_entry_index_count, 2);
}

#[test]
fn simple_launch_plan_bind_group_0_dynamic_offset_sets_are_empty() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(launch.bind_groups[0].dynamic_offset_count, 0);
    assert!(
        launch.bind_groups[0]
            .dynamic_offset_resource_indexes
            .is_empty()
    );
    assert_eq!(launch.bind_groups[0].dynamic_offset_resource_index_count, 0);
    assert!(
        launch.bind_groups[0]
            .dynamic_offset_binding_indexes
            .is_empty()
    );
    assert_eq!(launch.bind_groups[0].dynamic_offset_binding_index_count, 0);
    assert!(
        launch.bind_groups[0]
            .dynamic_offset_layout_entry_indexes
            .is_empty()
    );
    assert_eq!(launch.bind_groups[0].dynamic_offset_layout_entry_index_count, 0);
}

#[test]
fn simple_launch_plan_bind_group_0_binding_and_layout_index_and_counts() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(launch.bind_groups[0].binding_indexes, vec![0, 1]);
    assert_eq!(launch.bind_groups[0].binding_index_count, 2);
    assert_eq!(launch.bind_groups[0].layout_entry_indexes, vec![0, 1]);
    assert_eq!(launch.bind_groups[0].layout_entry_index_count, 2);
    assert_eq!(launch.bind_groups[0].binding_count, 2);
    assert_eq!(launch.bind_groups[0].layout_entry_count, 2);
    assert_eq!(launch.bind_groups[0].layout_entries.len(), 2);
}

#[test]
fn simple_launch_plan_bind_group_0_source_locals_and_names() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert_eq!(launch.bind_groups[0].source_locals, vec![MirLocalId(3)]);
    assert_eq!(launch.bind_groups[0].source_local_count, 1);
    assert!(launch.bind_groups[0].source_names.is_empty());
    assert_eq!(launch.bind_groups[0].source_name_count, 0);
}

#[test]
fn simple_launch_plan_bind_group_0_layout_entry_0_is_readonly_storage_input() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    let e = &launch.bind_groups[0].layout_entries[0];
    assert_eq!(e.layout_entry_index, 0);
    assert_eq!(e.binding_index, 0);
    assert_eq!(e.resource_index, 0);
    assert_eq!(e.binding, 0);
    assert_eq!(e.buffer_byte_len, 4);
    assert_eq!(e.buffer_byte_offset, 0);
    assert_eq!(e.binding_byte_len, 4);
    assert_eq!(e.shader_visibility, MirKernelShaderStage::Compute);
    assert_eq!(e.buffer_binding_type, MirKernelBufferBindingKind::ReadOnlyStorage);
    assert_eq!(e.min_binding_size, 4);
    assert!(!e.has_dynamic_offset);
    assert_eq!(e.source_local, Some(MirLocalId(3)));
    assert_eq!(e.source_name, None);
}

#[test]
fn simple_launch_plan_bind_group_0_layout_entry_1_is_storage_output() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    let e = &launch.bind_groups[0].layout_entries[1];
    assert_eq!(e.layout_entry_index, 1);
    assert_eq!(e.binding_index, 1);
    assert_eq!(e.resource_index, 1);
    assert_eq!(e.binding, 1);
    assert_eq!(e.buffer_byte_len, 4);
    assert_eq!(e.buffer_byte_offset, 0);
    assert_eq!(e.binding_byte_len, 4);
    assert_eq!(e.shader_visibility, MirKernelShaderStage::Compute);
    assert_eq!(e.buffer_binding_type, MirKernelBufferBindingKind::Storage);
    assert_eq!(e.min_binding_size, 4);
    assert!(!e.has_dynamic_offset);
    assert_eq!(e.source_local, None);
    assert_eq!(e.source_name, None);
}

#[test]
fn simple_launch_plan_bind_group_0_binding_0_is_readonly_input_resource() {
    let (_types, ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    let b = &launch.bind_groups[0].bindings[0];
    assert_eq!(b.binding_index, 0);
    assert_eq!(b.resource_index, 0);
    assert_eq!(b.binding, 0);
    assert_eq!(b.kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(b.role, MirKernelResourceRole::Input);
    assert_eq!(b.access, MirKernelResourceAccess::Read);
    assert_eq!(b.shader_access, MirKernelShaderAccess::Read);
    assert_eq!(b.shader_visibility, MirKernelShaderStage::Compute);
    assert_eq!(b.buffer_binding_type, MirKernelBufferBindingKind::ReadOnlyStorage);
    assert_eq!(b.element_ty, ty);
    assert_eq!(b.element_layout, MirScalarLayout::F32);
    assert_eq!(b.element_byte_width, 4);
    assert_eq!(b.element_count, 1);
    assert_eq!(b.buffer_byte_len, 4);
    assert_eq!(b.buffer_byte_offset, 0);
    assert_eq!(b.binding_byte_len, 4);
    assert_eq!(b.min_binding_size, 4);
    assert!(!b.has_dynamic_offset);
    assert_eq!(b.source_local, Some(MirLocalId(3)));
}

#[test]
fn simple_launch_plan_bind_group_0_binding_1_is_readwrite_output_resource() {
    let (_types, ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    let b = &launch.bind_groups[0].bindings[1];
    assert_eq!(b.binding_index, 1);
    assert_eq!(b.resource_index, 1);
    assert_eq!(b.binding, 1);
    assert_eq!(b.kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(b.role, MirKernelResourceRole::Output);
    assert_eq!(b.access, MirKernelResourceAccess::Write);
    assert_eq!(b.shader_access, MirKernelShaderAccess::ReadWrite);
    assert_eq!(b.shader_visibility, MirKernelShaderStage::Compute);
    assert_eq!(b.buffer_binding_type, MirKernelBufferBindingKind::Storage);
    assert_eq!(b.element_ty, ty);
    assert_eq!(b.element_layout, MirScalarLayout::F32);
    assert_eq!(b.buffer_byte_len, 4);
    assert_eq!(b.buffer_byte_offset, 0);
    assert_eq!(b.binding_byte_len, 4);
    assert_eq!(b.min_binding_size, 4);
    assert!(!b.has_dynamic_offset);
}

#[test]
fn simple_launch_plan_has_no_builtin_references() {
    let (_types, _ty, _reflection, launch) = setup_simple_kernel_reflection_launch_plan();
    assert!(launch.required_builtins.is_empty());
    assert!(launch.builtin_inputs.is_empty());
    assert!(launch.called_builtins.is_empty());
}

// ============================================================================
// Spine g-dispatch-guard-runtime-extent — U3: runtime extent in the launch plan
// ============================================================================
//
// Post-U2, a scalar-shaped kernel (static IO element count 1) carries a
// RuntimeExtent resource so the guard reads the host-supplied count at
// dispatch instead of the static `(1,1,1)` limit. These tests pin the facade
// `abi_test.rs` view of the guard end-to-end: signature → reflection →
// launch plan. The extent is appended only when the frozen type table
// already contains the `numerus<u32>` count type — the production invariant
// is that the GPU builtin registry interns it for every kernel program. The
// second test asserts the fallback the guard falls back to when it does not:
// the static `oob_guard_limit` still fires, so the OOB guard never silently
// disappears (it degrades to the pre-U2 static limit, which is asserted).
#[test]
fn scalar_kernel_launch_plan_carries_runtime_extent() {
    let mut types = TypeTable::new();
    let _extent_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let ty = f32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(ty);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    let reflection =
        MirKernelReflection::from_signature("add_one", &signature, &validation).expect("kernel reflection");
    let launch = reflection.launch_plan();

    // (1) The signature carries the runtime extent at the next-free binding.
    let extent = signature
        .runtime_extent()
        .expect("scalar kernel carries a runtime extent");
    assert_eq!(extent.kind, MirKernelResourceKind::RuntimeExtent);
    assert_eq!(extent.binding, 2);
    assert_eq!(extent.group, 0);
    assert_eq!(extent.role, MirKernelResourceRole::Input);
    assert_eq!(extent.access, MirKernelResourceAccess::Read);
    assert_eq!(extent.element_count, 1);
    assert_eq!(extent.source_name.as_deref(), Some("runtime_extent"));

    // (2) Reflection enumerates the extent: input → extra_inputs (extent) →
    // output, so the extent lands at index 1.
    assert_eq!(reflection.resources.len(), 3);
    let resource = &reflection.resources[1];
    assert_eq!(resource.kind, MirKernelResourceKind::RuntimeExtent);
    assert_eq!(resource.binding, 2);
    assert_eq!(resource.element_layout, MirScalarLayout::U32);
    assert_eq!(resource.element_byte_width, 4);
    assert_eq!(resource.buffer_byte_len, 4);

    // (3) The launch plan enumerates the extent with its binding.
    assert_eq!(launch.bind_groups.len(), 1);
    let bindings = &launch.bind_groups[0].bindings;
    assert_eq!(bindings.len(), 3);
    let binding = &bindings[2];
    assert_eq!(binding.kind, MirKernelResourceKind::RuntimeExtent);
    assert_eq!(binding.binding, 2);
    assert_eq!(binding.element_count, 1);
    assert_eq!(binding.buffer_byte_len, 4);
    assert_eq!(binding.source_name.as_deref(), Some("runtime_extent"));

    // (4) dispatch_size stays the static (1,1,1) shape hint — the host
    // supplies the real extent at dispatch via the extent binding.
    assert_eq!(
        (signature.dispatch_size.x, signature.dispatch_size.y, signature.dispatch_size.z),
        (1, 1, 1)
    );
    // (5) The guard limit the emitters fall back to when the extent binding
    // is absent stays the static dispatch-axis shape hint.
    assert_eq!(signature.oob_guard_limit(MirKernelBuiltin::GlobalInvocationX), 1);
}

// The guard must not silently disappear when the count type is absent from
// the frozen type table: the kernel keeps the static dispatch-axis limit, and
// the emitted `if (i >= limit) return;` still fires at that limit.
#[test]
fn scalar_kernel_without_u32_count_type_keeps_static_oob_guard() {
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(ty);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");

    assert_eq!(signature.input.element_count, 1);
    assert_eq!(signature.output.element_count, 1);
    assert!(
        signature.runtime_extent().is_none(),
        "no extent without an interned u32 count type"
    );
    assert_eq!(signature.dispatch_size.x, 1);
    // The OOB guard still fires, at the static scalar limit.
    assert_eq!(signature.oob_guard_limit(MirKernelBuiltin::GlobalInvocationX), 1);
}

// ============================================================================
// Split tests: kernel_launch_plan_pipeline_layout_indexes_sorted_bind_groups (was 1 test, now 16)
// ============================================================================

#[test]
fn sparse_launch_plan_has_two_bind_groups() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.bind_group_count, 2);
    assert_eq!(launch.pipeline_layout.bind_group_layout_count, 2);
}

#[test]
fn sparse_launch_plan_bind_groups_are_sorted_by_group_number() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    // Group 0 is the output resource group (original group 0)
    assert_eq!(launch.bind_groups[0].bind_group_index, 0);
    assert_eq!(launch.bind_groups[0].group, 0);
    assert_eq!(launch.bind_groups[0].resource_indexes, vec![1]);
    // Group 1 is the input resource group (original group 2, now at index 1)
    assert_eq!(launch.bind_groups[1].bind_group_index, 1);
    assert_eq!(launch.bind_groups[1].group, 2);
    assert_eq!(launch.bind_groups[1].resource_indexes, vec![0]);
}

#[test]
fn sparse_launch_plan_bind_groups_have_expected_resource_kinds() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.bind_groups[0].resource_kinds, vec![MirKernelResourceKind::StorageBuffer]);
    assert_eq!(launch.bind_groups[1].resource_kinds, vec![MirKernelResourceKind::StorageBuffer]);
}

#[test]
fn sparse_launch_plan_bind_groups_have_expected_resource_roles_and_accesses() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.bind_groups[0].resource_roles, vec![MirKernelResourceRole::Output]);
    assert_eq!(launch.bind_groups[0].resource_role_count, 1);
    assert_eq!(launch.bind_groups[0].shader_accesses, vec![MirKernelShaderAccess::ReadWrite]);
    assert_eq!(launch.bind_groups[0].shader_access_count, 1);
    assert_eq!(launch.bind_groups[1].resource_roles, vec![MirKernelResourceRole::Input]);
    assert_eq!(launch.bind_groups[1].resource_role_count, 1);
    assert_eq!(launch.bind_groups[1].shader_accesses, vec![MirKernelShaderAccess::Read]);
    assert_eq!(launch.bind_groups[1].shader_access_count, 1);
}

#[test]
fn sparse_launch_plan_bind_groups_have_expected_buffer_binding_types() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(
        launch.bind_groups[0].buffer_binding_types,
        vec![MirKernelBufferBindingKind::Storage]
    );
    assert_eq!(launch.bind_groups[0].buffer_binding_type_count, 1);
    assert_eq!(
        launch.bind_groups[1].buffer_binding_types,
        vec![MirKernelBufferBindingKind::ReadOnlyStorage]
    );
    assert_eq!(launch.bind_groups[1].buffer_binding_type_count, 1);
}

#[test]
fn sparse_launch_plan_bind_groups_buffer_byte_details_match_single_element() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    // Group 0 (output, group 0)
    assert_eq!(launch.bind_groups[0].buffer_byte_offsets, vec![0]);
    assert_eq!(launch.bind_groups[0].buffer_byte_offset_count, 1);
    assert_eq!(launch.bind_groups[0].buffer_byte_lens, vec![4]);
    assert_eq!(launch.bind_groups[0].buffer_byte_len_count, 1);
    assert_eq!(launch.bind_groups[0].binding_byte_lens, vec![4]);
    assert_eq!(launch.bind_groups[0].binding_byte_len_count, 1);
    assert_eq!(launch.bind_groups[0].min_binding_sizes, vec![4]);
    assert_eq!(launch.bind_groups[0].min_binding_size_count, 1);
    // Group 1 (input, group 2)
    assert_eq!(launch.bind_groups[1].buffer_byte_offsets, vec![0]);
    assert_eq!(launch.bind_groups[1].buffer_byte_offset_count, 1);
    assert_eq!(launch.bind_groups[1].buffer_byte_lens, vec![4]);
    assert_eq!(launch.bind_groups[1].buffer_byte_len_count, 1);
    assert_eq!(launch.bind_groups[1].binding_byte_lens, vec![4]);
    assert_eq!(launch.bind_groups[1].binding_byte_len_count, 1);
    assert_eq!(launch.bind_groups[1].min_binding_sizes, vec![4]);
    assert_eq!(launch.bind_groups[1].min_binding_size_count, 1);
}

#[test]
fn sparse_launch_plan_bind_groups_have_fixed_offsets_and_no_dynamic_flags() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.bind_groups[0].has_dynamic_offsets, vec![false]);
    assert_eq!(launch.bind_groups[0].dynamic_offset_flag_count, 1);
    assert_eq!(launch.bind_groups[0].offset_modes, vec![MirKernelOffsetMode::Fixed]);
    assert_eq!(launch.bind_groups[0].offset_mode_entry_count, 1);
    assert!(!launch.bind_groups[0].requires_dynamic_offsets);
    assert!(launch.bind_groups[0].has_non_dynamic_offsets);
    assert!(!launch.bind_groups[0].has_mixed_offset_modes);
    assert_eq!(launch.bind_groups[0].offset_mode_count, 1);
    assert!(launch.bind_groups[0].has_only_non_dynamic_offsets);
    assert!(!launch.bind_groups[0].has_only_dynamic_offsets);

    assert_eq!(launch.bind_groups[1].has_dynamic_offsets, vec![false]);
    assert_eq!(launch.bind_groups[1].dynamic_offset_flag_count, 1);
    assert_eq!(launch.bind_groups[1].offset_modes, vec![MirKernelOffsetMode::Fixed]);
    assert_eq!(launch.bind_groups[1].offset_mode_entry_count, 1);
    assert!(!launch.bind_groups[1].requires_dynamic_offsets);
    assert!(launch.bind_groups[1].has_non_dynamic_offsets);
    assert!(!launch.bind_groups[1].has_mixed_offset_modes);
    assert_eq!(launch.bind_groups[1].offset_mode_count, 1);
    assert!(launch.bind_groups[1].has_only_non_dynamic_offsets);
    assert!(!launch.bind_groups[1].has_only_dynamic_offsets);
}

#[test]
fn sparse_launch_plan_bind_groups_non_dynamic_offset_indexes_are_single_entry() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_count, 1);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_resource_indexes, vec![1]);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_resource_index_count, 1);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_binding_indexes, vec![0]);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_binding_index_count, 1);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_layout_entry_indexes, vec![0]);
    assert_eq!(launch.bind_groups[0].non_dynamic_offset_layout_entry_index_count, 1);

    assert_eq!(launch.bind_groups[1].non_dynamic_offset_count, 1);
    assert_eq!(launch.bind_groups[1].non_dynamic_offset_resource_indexes, vec![0]);
    assert_eq!(launch.bind_groups[1].non_dynamic_offset_resource_index_count, 1);
    assert_eq!(launch.bind_groups[1].non_dynamic_offset_binding_indexes, vec![0]);
    assert_eq!(launch.bind_groups[1].non_dynamic_offset_binding_index_count, 1);
    assert_eq!(launch.bind_groups[1].non_dynamic_offset_layout_entry_indexes, vec![0]);
    assert_eq!(launch.bind_groups[1].non_dynamic_offset_layout_entry_index_count, 1);
}

#[test]
fn sparse_launch_plan_bind_groups_dynamic_offset_sets_are_empty() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.bind_groups[0].dynamic_offset_count, 0);
    assert!(
        launch.bind_groups[0]
            .dynamic_offset_resource_indexes
            .is_empty()
    );
    assert_eq!(launch.bind_groups[0].dynamic_offset_resource_index_count, 0);
    assert!(
        launch.bind_groups[0]
            .dynamic_offset_binding_indexes
            .is_empty()
    );
    assert_eq!(launch.bind_groups[0].dynamic_offset_binding_index_count, 0);
    assert!(
        launch.bind_groups[0]
            .dynamic_offset_layout_entry_indexes
            .is_empty()
    );
    assert_eq!(launch.bind_groups[0].dynamic_offset_layout_entry_index_count, 0);

    assert_eq!(launch.bind_groups[1].dynamic_offset_count, 0);
    assert!(
        launch.bind_groups[1]
            .dynamic_offset_resource_indexes
            .is_empty()
    );
    assert_eq!(launch.bind_groups[1].dynamic_offset_resource_index_count, 0);
    assert!(
        launch.bind_groups[1]
            .dynamic_offset_binding_indexes
            .is_empty()
    );
    assert_eq!(launch.bind_groups[1].dynamic_offset_binding_index_count, 0);
    assert!(
        launch.bind_groups[1]
            .dynamic_offset_layout_entry_indexes
            .is_empty()
    );
    assert_eq!(launch.bind_groups[1].dynamic_offset_layout_entry_index_count, 0);
}

#[test]
fn sparse_launch_plan_bind_groups_binding_and_layout_indexes_and_counts() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.bind_groups[0].binding_indexes, vec![0]);
    assert_eq!(launch.bind_groups[0].binding_index_count, 1);
    assert_eq!(launch.bind_groups[0].resource_index_count, 1);
    assert_eq!(launch.bind_groups[0].resource_kind_count, 1);
    assert_eq!(launch.bind_groups[0].layout_entry_indexes, vec![0]);
    assert_eq!(launch.bind_groups[0].layout_entry_index_count, 1);
    assert_eq!(launch.bind_groups[0].binding_count, 1);
    assert_eq!(launch.bind_groups[0].layout_entry_count, 1);

    assert_eq!(launch.bind_groups[1].binding_indexes, vec![0]);
    assert_eq!(launch.bind_groups[1].binding_index_count, 1);
    assert_eq!(launch.bind_groups[1].resource_index_count, 1);
    assert_eq!(launch.bind_groups[1].resource_kind_count, 1);
    assert_eq!(launch.bind_groups[1].layout_entry_indexes, vec![0]);
    assert_eq!(launch.bind_groups[1].layout_entry_index_count, 1);
    assert_eq!(launch.bind_groups[1].binding_count, 1);
    assert_eq!(launch.bind_groups[1].layout_entry_count, 1);
}

#[test]
fn sparse_launch_plan_bind_groups_source_details_are_empty() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert!(launch.bind_groups[0].source_locals.is_empty());
    assert_eq!(launch.bind_groups[0].source_local_count, 0);
    assert!(launch.bind_groups[0].source_names.is_empty());
    assert_eq!(launch.bind_groups[0].source_name_count, 0);

    assert!(launch.bind_groups[1].source_locals.is_empty());
    assert_eq!(launch.bind_groups[1].source_local_count, 0);
    assert!(launch.bind_groups[1].source_names.is_empty());
    assert_eq!(launch.bind_groups[1].source_name_count, 0);
}

#[test]
fn sparse_launch_plan_bind_groups_layout_entry_basics_are_sequential() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.bind_groups[0].layout_entries[0].layout_entry_index, 0);
    assert_eq!(launch.bind_groups[0].layout_entries[0].binding_index, 0);
    assert_eq!(launch.bind_groups[1].layout_entries[0].layout_entry_index, 0);
    assert_eq!(launch.bind_groups[1].layout_entries[0].binding_index, 0);
}

#[test]
fn sparse_launch_plan_bind_groups_layout_entry_buffer_details_match() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.bind_groups[0].layout_entries[0].buffer_byte_len, 4);
    assert_eq!(launch.bind_groups[0].layout_entries[0].buffer_byte_offset, 0);
    assert_eq!(launch.bind_groups[0].layout_entries[0].binding_byte_len, 4);
    assert_eq!(launch.bind_groups[1].layout_entries[0].buffer_byte_len, 4);
    assert_eq!(launch.bind_groups[1].layout_entries[0].buffer_byte_offset, 0);
    assert_eq!(launch.bind_groups[1].layout_entries[0].binding_byte_len, 4);
}

#[test]
fn sparse_launch_plan_bind_groups_binding_details_are_indexed() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.bind_groups[0].bindings[0].binding_index, 0);
    assert_eq!(launch.bind_groups[1].bindings[0].binding_index, 0);
}

#[test]
fn sparse_launch_plan_pipeline_layout_maps_bind_groups_correctly() {
    let (_types, launch) = setup_sparse_group_launch_plan();
    assert_eq!(launch.pipeline_layout.bind_group_layouts[0].bind_group_index, 0);
    assert_eq!(launch.pipeline_layout.bind_group_layouts[0].group, 0);
    assert_eq!(launch.pipeline_layout.bind_group_layouts[1].bind_group_index, 1);
    assert_eq!(launch.pipeline_layout.bind_group_layouts[1].group, 2);
}

#[test]
fn vertex_reflection_accepts_stage4_position_layout_fact() {
    let reflection = MirKernelReflection::vertex_entry(
        MirFunctionId(4),
        "plane_vertex",
        vec![MirVertexInputReflection {
            source_name: Some("position".to_owned()),
            location: 0,
            format: MirVertexInputFormat::Float32x3,
            step_mode: MirInputStepMode::Vertex,
            offset_bytes: 0,
            stride_bytes: 12,
            role: radix_mir::ShaderStageIoRole::Other,
        }],
        vec![],
    )
    .expect("vertex reflection");

    assert_eq!(reflection.shader_stage, MirKernelShaderStage::Vertex);
    assert_eq!(reflection.vertex_inputs.len(), 1);
    assert_eq!(reflection.vertex_inputs[0].source_name.as_deref(), Some("position"));
    assert_eq!(reflection.vertex_inputs[0].location, 0);
    assert_eq!(reflection.vertex_inputs[0].format, MirVertexInputFormat::Float32x3);
    assert_eq!(reflection.vertex_inputs[0].step_mode, MirInputStepMode::Vertex);
    assert_eq!(reflection.vertex_inputs[0].offset_bytes, 0);
    assert_eq!(reflection.vertex_inputs[0].stride_bytes, 12);
}

#[test]
fn vertex_reflection_rejects_duplicate_shader_locations() {
    let error = MirKernelReflection::vertex_entry(
        MirFunctionId(4),
        "plane_vertex",
        vec![
            MirVertexInputReflection {
                source_name: Some("position".to_owned()),
                location: 0,
                format: MirVertexInputFormat::Float32x3,
                step_mode: MirInputStepMode::Vertex,
                offset_bytes: 0,
                stride_bytes: 12,
                role: radix_mir::ShaderStageIoRole::Other,
            },
            MirVertexInputReflection {
                source_name: Some("normal".to_owned()),
                location: 0,
                format: MirVertexInputFormat::Float32x3,
                step_mode: MirInputStepMode::Vertex,
                offset_bytes: 0,
                stride_bytes: 12,
                role: radix_mir::ShaderStageIoRole::Other,
            },
        ],
        vec![],
    )
    .expect_err("duplicate vertex location should fail closed");

    assert_eq!(error.issue, "mir_vertex_input_duplicate_location");
    assert_eq!(error.detail.as_deref(), Some("0"));
}

/// A canonical shader-contract vertex attribute for generic-builder tests.
///
/// The stage-I/O role is declared explicitly (role param on the contract
/// record), matching the U6 surface where roles come from the declared
/// contract, never from source-name spelling.
fn shader_vertex_attribute(
    semantic: &str,
    location: u32,
    format: MirVertexInputFormat,
    offset_bytes: u64,
    stride_bytes: u64,
    role: radix_mir::ShaderStageIoRole,
) -> radix_mir::ShaderVertexAttribute {
    radix_mir::ShaderVertexAttribute {
        semantic: Some(semantic.to_owned()),
        location,
        format,
        offset_bytes,
        stride_bytes,
        step_mode: MirInputStepMode::Vertex,
        role,
        provenance: radix_mir::ContractProvenance::default(),
    }
}

#[test]
fn vertex_reflection_lowers_shader_contract_attributes() {
    let reflection = radix_mir::vertex_entry_from_shader_contract(
        MirFunctionId(4),
        "shader_plane_vertex",
        &[
            shader_vertex_attribute(
                "position",
                0,
                MirVertexInputFormat::Float32x3,
                0,
                12,
                radix_mir::ShaderStageIoRole::Position,
            ),
            shader_vertex_attribute(
                "uv",
                2,
                MirVertexInputFormat::Float32x2,
                0,
                8,
                radix_mir::ShaderStageIoRole::Other,
            ),
        ],
        vec![],
    )
    .expect("shader contract vertex attributes should lower");

    assert_eq!(reflection.vertex_inputs.len(), 2);
    assert_eq!(reflection.vertex_inputs[0].format, MirVertexInputFormat::Float32x3);
    assert_eq!(reflection.vertex_inputs[1].format, MirVertexInputFormat::Float32x2);
    assert_eq!(reflection.vertex_inputs[1].source_name.as_deref(), Some("uv"));
    assert_eq!(reflection.vertex_inputs[1].location, 2);
    assert_eq!(reflection.vertex_inputs[1].offset_bytes, 0);
    assert_eq!(reflection.vertex_inputs[1].stride_bytes, 8);
}

#[test]
fn vertex_launch_plan_projects_webgpu_vertex_buffer_layouts() {
    let reflection = radix_mir::vertex_entry_from_shader_contract(
        MirFunctionId(4),
        "shader_plane_vertex",
        &[
            shader_vertex_attribute(
                "position",
                0,
                MirVertexInputFormat::Float32x3,
                0,
                12,
                radix_mir::ShaderStageIoRole::Position,
            ),
            shader_vertex_attribute(
                "normal",
                1,
                MirVertexInputFormat::Float32x3,
                0,
                12,
                radix_mir::ShaderStageIoRole::Other,
            ),
            shader_vertex_attribute(
                "uv",
                2,
                MirVertexInputFormat::Float32x2,
                0,
                8,
                radix_mir::ShaderStageIoRole::Other,
            ),
        ],
        vec![],
    )
    .expect("shader contract vertex attributes should lower");

    let launch = reflection.launch_plan();
    assert_eq!(launch.vertex_input_count, 3);
    assert_eq!(launch.vertex_inputs[0].source_name.as_deref(), Some("position"));
    assert_eq!(launch.vertex_inputs[2].format, MirVertexInputFormat::Float32x2);

    let adapter = launch.webgpu_adapter();
    assert_eq!(adapter.vertex_buffer_layout_descriptor_count, 3);
    assert_eq!(adapter.vertex_buffer_layout_descriptor_indexes, vec![0, 1, 2]);
    assert_eq!(adapter.vertex_buffer_layout_descriptor_index_count, 3);

    let position = &adapter.vertex_buffer_layout_descriptors[0];
    assert_eq!(position.buffer_index, 0);
    assert_eq!(position.array_stride, 12);
    assert_eq!(position.step_mode, MirWebGpuVertexStepMode::Vertex);
    assert_eq!(position.attribute_indexes, vec![0]);
    assert_eq!(position.attribute_index_count, 1);
    assert_eq!(position.attribute_count, 1);
    assert_eq!(position.source_name.as_deref(), Some("position"));
    assert_eq!(position.attributes[0].attribute_index, 0);
    assert_eq!(position.attributes[0].shader_location, 0);
    assert_eq!(position.attributes[0].format, MirVertexInputFormat::Float32x3);
    assert_eq!(position.attributes[0].offset, 0);
    assert_eq!(position.attributes[0].source_name.as_deref(), Some("position"));

    let uv = &adapter.vertex_buffer_layout_descriptors[2];
    assert_eq!(uv.buffer_index, 2);
    assert_eq!(uv.array_stride, 8);
    assert_eq!(uv.attributes[0].shader_location, 2);
    assert_eq!(uv.attributes[0].format, MirVertexInputFormat::Float32x2);
    assert_eq!(uv.attributes[0].source_name.as_deref(), Some("uv"));
}

#[test]
fn shader_contract_format_code_rejects_unknown_fail_closed() {
    let error = radix_mir::vertex_format_from_code(5).expect_err("unsupported format code should fail closed");

    assert_eq!(error.issue, "mir_shader_format_code_unsupported");
    assert_eq!(error.detail.as_deref(), Some("5"));
}

#[test]
fn input_step_mode_spelling_is_stable() {
    assert_eq!(MirInputStepMode::Vertex.spelling(), "vertex");
    assert_eq!(MirInputStepMode::Instance.spelling(), "instance");
}

#[test]
fn vertex_input_reflection_supports_instance_step_mode() {
    let reflection = MirKernelReflection::vertex_entry(
        MirFunctionId(4),
        "instance_vertex",
        vec![MirVertexInputReflection {
            source_name: Some("instance_data".to_owned()),
            location: 0,
            format: MirVertexInputFormat::Float32x4,
            step_mode: MirInputStepMode::Instance,
            offset_bytes: 0,
            stride_bytes: 16,
            role: radix_mir::ShaderStageIoRole::Other,
        }],
        vec![],
    )
    .expect("vertex reflection with instance step mode");

    assert_eq!(reflection.vertex_inputs[0].step_mode, MirInputStepMode::Instance);
}

#[test]
fn color_target_format_bgra8_unorm_spelling_is_stable() {
    assert_eq!(MirColorTargetFormat::Bgra8Unorm.spelling(), "bgra8unorm");
}

#[test]
fn graphics_pipeline_accepts_bgra8_unorm_color_target() {
    let mut vertex = MirKernelReflection::vertex_entry(
        MirFunctionId(9),
        "vertex",
        vec![MirVertexInputReflection {
            source_name: Some("position".to_owned()),
            location: 0,
            format: MirVertexInputFormat::Float32x3,
            step_mode: MirInputStepMode::Vertex,
            offset_bytes: 0,
            stride_bytes: 12,
            role: radix_mir::ShaderStageIoRole::Other,
        }],
        vec![],
    )
    .expect("vertex");
    vertex.varyings = vec![MirVaryingReflection {
        source_name: "position".to_owned(),
        location: 0,
        format: MirVertexInputFormat::Float32x4,
        interpolation: MirVaryingInterpolation::Perspective,
        role: radix_mir::ShaderStageIoRole::Other,
    }];
    let fragment = MirKernelReflection::fragment_entry(
        MirFunctionId(10),
        "fragment",
        vec![MirVaryingReflection {
            source_name: "position".to_owned(),
            location: 0,
            format: MirVertexInputFormat::Float32x4,
            interpolation: MirVaryingInterpolation::Perspective,
            role: radix_mir::ShaderStageIoRole::Other,
        }],
        vec![MirFragmentOutputReflection {
            location: 0,
            format: MirVertexInputFormat::Float32x4,
            role: radix_mir::ShaderStageIoRole::Other,
        }],
        vec![],
    )
    .expect("fragment");

    let pipeline =
        MirKernelReflection::graphics_pipeline_reflection(&vertex, &fragment, 3, &[MirColorTargetFormat::Bgra8Unorm])
            .expect("pipeline reflection with bgra8unorm");

    assert_eq!(pipeline.color_target_formats, vec![MirColorTargetFormat::Bgra8Unorm]);
}

#[test]
fn webgpu_adapter_pipeline_layout_bind_group_counts() {
    let (_types, _ty, reflection) = setup_webgpu_probe_reflection();
    let adapter = reflection.launch_plan().webgpu_adapter();

    assert_eq!(adapter.pipeline_layout_descriptor.bind_group_layout_count, 2);
    assert_eq!(adapter.pipeline_layout_descriptor.bind_group_layout_indexes, vec![0, 1]);
    assert_eq!(
        adapter
            .pipeline_layout_descriptor
            .bind_group_layout_index_count,
        2
    );
    assert_eq!(adapter.bind_group_layout_descriptor_count, 2);
    assert_eq!(adapter.bind_group_layout_descriptor_indexes, vec![0, 1]);
    assert_eq!(adapter.bind_group_layout_descriptor_index_count, 2);
    assert_eq!(adapter.bind_group_descriptor_count, 2);
    assert_eq!(adapter.bind_group_descriptor_indexes, vec![0, 1]);
    assert_eq!(adapter.bind_group_descriptor_index_count, 2);
}

#[test]
fn webgpu_adapter_dispatch_dimensions_are_non_unit() {
    let (_types, _ty, reflection) = setup_webgpu_probe_reflection();
    let adapter = reflection.launch_plan().webgpu_adapter();

    assert_eq!(adapter.dispatch_workgroup_dimension_count, 3);
    assert_eq!(
        (
            adapter.dispatch_workgroups.x,
            adapter.dispatch_workgroups.y,
            adapter.dispatch_workgroups.z
        ),
        (3, 2, 1)
    );
}

#[test]
fn webgpu_adapter_bind_group_0_storage_output_layout() {
    let (_types, ty, reflection) = setup_webgpu_probe_reflection();
    let adapter = reflection.launch_plan().webgpu_adapter();

    let layout = &adapter.bind_group_layout_descriptors[0];
    assert_eq!(layout.bind_group_index, 0);
    assert_eq!(layout.group, 0);
    assert_eq!(layout.layout_entry_indexes, vec![0]);
    assert_eq!(layout.layout_entry_index_count, 1);
    assert_eq!(layout.entry_count, 1);
    assert_eq!(layout.entries[0].binding, 0);
    assert_eq!(layout.entries[0].binding_index, 0);
    assert_eq!(layout.entries[0].resource_index, 1);
    assert_eq!(layout.entries[0].visibility, MirKernelShaderStage::Compute);
    assert_eq!(layout.entries[0].buffer_type, MirKernelBufferBindingKind::Storage);
    assert_eq!(layout.entries[0].buffer_byte_len, 4);
    assert_eq!(layout.entries[0].buffer_byte_offset, 0);
    assert_eq!(layout.entries[0].binding_byte_len, 4);
    assert_eq!(layout.entries[0].min_binding_size, 4);
    assert!(!layout.entries[0].has_dynamic_offset);

    let bind_group = &adapter.bind_group_descriptors[0];
    assert_eq!(bind_group.bind_group_index, 0);
    assert_eq!(bind_group.group, 0);
    assert_eq!(bind_group.entry_indexes, vec![0]);
    assert_eq!(bind_group.entry_index_count, 1);
    assert_eq!(bind_group.entry_count, 1);
    assert_eq!(bind_group.entries[0].binding, 0);
    assert_eq!(bind_group.entries[0].kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(bind_group.entries[0].role, MirKernelResourceRole::Output);
    assert_eq!(bind_group.entries[0].access, MirKernelResourceAccess::Write);
    assert_eq!(bind_group.entries[0].shader_access, MirKernelShaderAccess::ReadWrite);
    assert_eq!(bind_group.entries[0].shader_visibility, MirKernelShaderStage::Compute);
    assert_eq!(bind_group.entries[0].element_ty, ty);
    assert_eq!(bind_group.entries[0].element_layout, MirScalarLayout::F32);
    assert_eq!(bind_group.entries[0].element_byte_width, 4);
    assert_eq!(bind_group.entries[0].element_count, 1);
    assert_eq!(bind_group.entries[0].resource_index, 1);
    assert_eq!(bind_group.entries[0].binding_index, 0);
    assert_eq!(bind_group.entries[0].buffer_type, MirKernelBufferBindingKind::Storage);
    assert_eq!(bind_group.entries[0].buffer_byte_len, 4);
    assert_eq!(bind_group.entries[0].buffer_byte_offset, 0);
    assert_eq!(bind_group.entries[0].binding_byte_len, 4);
    assert_eq!(bind_group.entries[0].min_binding_size, 4);
    assert!(!bind_group.entries[0].has_dynamic_offset);
}

#[test]
fn webgpu_adapter_bind_group_1_readonly_storage_input_layout() {
    let (_types, ty, reflection) = setup_webgpu_probe_reflection();
    let adapter = reflection.launch_plan().webgpu_adapter();

    let layout = &adapter.bind_group_layout_descriptors[1];
    assert_eq!(layout.bind_group_index, 1);
    assert_eq!(layout.group, 2);
    assert_eq!(layout.layout_entry_indexes, vec![0]);
    assert_eq!(layout.layout_entry_index_count, 1);
    assert_eq!(layout.entry_count, 1);
    assert_eq!(layout.entries[0].binding, 0);
    assert_eq!(layout.entries[0].binding_index, 0);
    assert_eq!(layout.entries[0].resource_index, 0);
    assert_eq!(layout.entries[0].visibility, MirKernelShaderStage::Compute);
    assert_eq!(layout.entries[0].buffer_type, MirKernelBufferBindingKind::ReadOnlyStorage);
    assert_eq!(layout.entries[0].buffer_byte_len, 4);
    assert_eq!(layout.entries[0].buffer_byte_offset, 0);
    assert_eq!(layout.entries[0].binding_byte_len, 4);
    assert_eq!(layout.entries[0].min_binding_size, 4);
    assert!(!layout.entries[0].has_dynamic_offset);

    let bind_group = &adapter.bind_group_descriptors[1];
    assert_eq!(bind_group.bind_group_index, 1);
    assert_eq!(bind_group.group, 2);
    assert_eq!(bind_group.entry_indexes, vec![0]);
    assert_eq!(bind_group.entry_index_count, 1);
    assert_eq!(bind_group.entry_count, 1);
    assert_eq!(bind_group.entries[0].binding, 0);
    assert_eq!(bind_group.entries[0].kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(bind_group.entries[0].role, MirKernelResourceRole::Input);
    assert_eq!(bind_group.entries[0].access, MirKernelResourceAccess::Read);
    assert_eq!(bind_group.entries[0].shader_access, MirKernelShaderAccess::Read);
    assert_eq!(bind_group.entries[0].shader_visibility, MirKernelShaderStage::Compute);
    assert_eq!(bind_group.entries[0].element_ty, ty);
    assert_eq!(bind_group.entries[0].element_layout, MirScalarLayout::F32);
    assert_eq!(bind_group.entries[0].element_byte_width, 4);
    assert_eq!(bind_group.entries[0].element_count, 1);
    assert_eq!(bind_group.entries[0].resource_index, 0);
    assert_eq!(bind_group.entries[0].binding_index, 0);
    assert_eq!(bind_group.entries[0].buffer_type, MirKernelBufferBindingKind::ReadOnlyStorage);
    assert_eq!(bind_group.entries[0].buffer_byte_len, 4);
    assert_eq!(bind_group.entries[0].buffer_byte_offset, 0);
    assert_eq!(bind_group.entries[0].binding_byte_len, 4);
    assert_eq!(bind_group.entries[0].min_binding_size, 4);
    assert!(!bind_group.entries[0].has_dynamic_offset);
}

#[test]
fn tensor_view_kernel_abi_maps_rank1_tf32_to_f32_buffers() {
    let mut types = TypeTable::new();
    let (tensor_ty, element_ty) = rank1_tf32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(tensor_ty);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");

    assert_eq!(signature.input.binding, 0);
    assert_eq!(signature.input.kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(signature.input.role, MirKernelResourceRole::Input);
    assert_eq!(signature.input.access, MirKernelResourceAccess::Read);
    assert_eq!(signature.input.element_ty, element_ty);
    assert_eq!(signature.input.element_count, 4);
    assert_eq!(signature.input.source_local, Some(MirLocalId(3)));
    assert_eq!(signature.output.binding, 1);
    assert_eq!(signature.output.kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(signature.output.role, MirKernelResourceRole::Output);
    assert_eq!(signature.output.access, MirKernelResourceAccess::Write);
    assert_eq!(signature.output.element_ty, element_ty);
    assert_eq!(signature.output.element_count, 4);
    assert_eq!(signature.output.source_local, None);
    assert_eq!(
        (signature.dispatch_size.x, signature.dispatch_size.y, signature.dispatch_size.z),
        (4, 1, 1)
    );
}

#[test]
fn tensor_placement_rejects_implicit_host_to_device_movement() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let function = indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let validation = MirValidationContext::new(&types);

    let error = MirKernelSignature::storage_buffer_kernel(&function, &validation)
        .expect_err("implicit host-to-device tensor movement must reject");

    assert_eq!(error.issue, "mir_kernel_tensor_storage_locus_unsupported");
}

#[test]
fn gpu_builtin_indexed_tensor_view_kernel_abi_records_global_invocation_input() {
    let mut types = TypeTable::new();
    let (tensor_ty, element_ty) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let function = indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");

    assert_eq!(signature.function, MirFunctionId(8));
    assert_eq!(signature.input.binding, 0);
    assert_eq!(signature.input.role, MirKernelResourceRole::Input);
    assert_eq!(signature.input.access, MirKernelResourceAccess::Read);
    assert_eq!(signature.input.element_ty, element_ty);
    assert_eq!(signature.input.element_count, 4);
    assert_eq!(signature.input.source_local, Some(MirLocalId(0)));
    assert_eq!(signature.output.binding, 1);
    assert_eq!(signature.output.role, MirKernelResourceRole::Output);
    assert_eq!(signature.output.access, MirKernelResourceAccess::Write);
    assert_eq!(signature.output.element_ty, element_ty);
    assert_eq!(signature.output.element_count, 4);
    assert_eq!(signature.output.source_local, Some(MirLocalId(1)));
    assert_eq!(signature.builtin_inputs.len(), 1);
    assert_eq!(signature.builtin_inputs[0].local, MirLocalId(2));
    assert_eq!(signature.builtin_inputs[0].builtin, MirKernelBuiltin::GlobalInvocationX);
    assert_eq!(signature.builtin_inputs[0].ty, index_ty);
    assert_eq!(
        (signature.dispatch_size.x, signature.dispatch_size.y, signature.dispatch_size.z),
        (4, 1, 1)
    );
}

#[test]
fn gpu_builtin_kernel_reflection_records_global_invocation_input() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let function = indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    let reflection = MirKernelReflection::from_signature("copy", &signature, &validation).expect("kernel reflection");

    assert_eq!(reflection.shader_stage, MirKernelShaderStage::Compute);
    assert_eq!(reflection.builtin_inputs.len(), 1);
    assert_eq!(reflection.builtin_inputs[0].source_local, MirLocalId(2));
    assert_eq!(reflection.builtin_inputs[0].builtin, MirKernelBuiltin::GlobalInvocationX);
    assert_eq!(reflection.builtin_inputs[0].axis, MirKernelAxis::X);
    assert_eq!(reflection.builtin_inputs[0].ty, index_ty);
    assert_eq!(reflection.builtin_inputs[0].layout, MirScalarLayout::U32);
    assert_eq!(
        (
            reflection.dispatch_size.x,
            reflection.dispatch_size.y,
            reflection.dispatch_size.z
        ),
        (4, 1, 1)
    );
    assert_eq!(
        (
            reflection.workgroup_count.x,
            reflection.workgroup_count.y,
            reflection.workgroup_count.z
        ),
        (4, 1, 1)
    );
}

#[test]
fn gpu_builtin_global_invocation_axis_facts_are_stable() {
    assert_eq!(MirKernelBuiltin::GlobalInvocationX.spelling(), "global_invocation_x");
    assert_eq!(MirKernelBuiltin::GlobalInvocationY.spelling(), "global_invocation_y");
    assert_eq!(MirKernelBuiltin::GlobalInvocationZ.spelling(), "global_invocation_z");
    assert_eq!(
        MirKernelBuiltin::from_spelling("global_invocation_x"),
        Some(MirKernelBuiltin::GlobalInvocationX)
    );
    assert_eq!(
        MirKernelBuiltin::from_spelling("global_invocation_y"),
        Some(MirKernelBuiltin::GlobalInvocationY)
    );
    assert_eq!(
        MirKernelBuiltin::from_spelling("global_invocation_z"),
        Some(MirKernelBuiltin::GlobalInvocationZ)
    );
    assert_eq!(MirKernelBuiltin::GlobalInvocationX.axis(), MirKernelAxis::X);
    assert_eq!(MirKernelBuiltin::GlobalInvocationY.axis(), MirKernelAxis::Y);
    assert_eq!(MirKernelBuiltin::GlobalInvocationZ.axis(), MirKernelAxis::Z);
    assert_eq!(MirKernelAxis::X.spelling(), "x");
    assert_eq!(MirKernelAxis::Y.spelling(), "y");
    assert_eq!(MirKernelAxis::Z.spelling(), "z");
}

#[test]
fn gpu_builtin_workgroup_and_local_axis_facts_are_stable() {
    let cases = [
        (MirKernelBuiltin::WorkgroupIdX, "workgroup_id_x", MirKernelAxis::X),
        (MirKernelBuiltin::WorkgroupIdY, "workgroup_id_y", MirKernelAxis::Y),
        (MirKernelBuiltin::WorkgroupIdZ, "workgroup_id_z", MirKernelAxis::Z),
        (MirKernelBuiltin::LocalInvocationIdX, "local_invocation_id_x", MirKernelAxis::X),
        (MirKernelBuiltin::LocalInvocationIdY, "local_invocation_id_y", MirKernelAxis::Y),
        (MirKernelBuiltin::LocalInvocationIdZ, "local_invocation_id_z", MirKernelAxis::Z),
        (MirKernelBuiltin::WorkgroupSizeX, "workgroup_size_x", MirKernelAxis::X),
        (MirKernelBuiltin::WorkgroupSizeY, "workgroup_size_y", MirKernelAxis::Y),
        (MirKernelBuiltin::WorkgroupSizeZ, "workgroup_size_z", MirKernelAxis::Z),
    ];

    for (builtin, spelling, axis) in cases {
        assert_eq!(builtin.spelling(), spelling);
        assert_eq!(MirKernelBuiltin::from_spelling(spelling), Some(builtin));
        assert_eq!(builtin.axis(), axis);
    }
    assert_eq!(MirKernelBuiltin::from_spelling("id"), None);
    assert_eq!(MirKernelBuiltin::from_spelling("thread_position_in_grid"), None);
}

#[test]
fn gpu_builtin_indexed_kernel_uses_reserved_index_parameter_name_as_source() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut function = indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let mut interner = Interner::new();
    function.params[2].name = Some(interner.intern("workgroup_id_y"));
    let validation = validation_with_device_tensor_layouts(&types);

    let signature = MirKernelSignature::storage_buffer_kernel_with_interner(&function, &validation, &interner)
        .expect("kernel signature");
    let reflection = MirKernelReflection::from_signature("named", &signature, &validation).expect("reflection");

    assert_eq!(signature.builtin_inputs[0].builtin, MirKernelBuiltin::WorkgroupIdY);
    assert_eq!(signature.builtin_inputs[0].source_name.as_deref(), Some("workgroup_id_y"));
    assert_eq!(reflection.builtin_inputs[0].source_local, MirLocalId(2));
    assert_eq!(reflection.builtin_inputs[0].source_name.as_deref(), Some("workgroup_id_y"));
    assert_eq!(reflection.builtin_inputs[0].builtin, MirKernelBuiltin::WorkgroupIdY);
    assert_eq!(reflection.builtin_inputs[0].axis, MirKernelAxis::Y);
    assert_eq!(reflection.builtin_inputs[0].layout, MirScalarLayout::U32);
}

#[test]
fn gpu_builtin_signature_records_called_and_header_builtins() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut function = indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    function.blocks[0].statements = vec![
        gpu_builtin_statement(MirKernelBuiltin::WorkgroupIdY, index_ty),
        gpu_builtin_statement(MirKernelBuiltin::GlobalInvocationX, index_ty),
        gpu_builtin_statement(MirKernelBuiltin::WorkgroupIdY, index_ty),
    ];
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");

    assert_eq!(signature.builtin_inputs.len(), 1);
    assert_eq!(signature.builtin_inputs[0].builtin, MirKernelBuiltin::GlobalInvocationX);
    assert_eq!(
        signature.called_builtins,
        vec![MirKernelBuiltin::WorkgroupIdY, MirKernelBuiltin::GlobalInvocationX]
    );
    assert_eq!(
        signature.header_builtins(),
        vec![MirKernelBuiltin::GlobalInvocationX, MirKernelBuiltin::WorkgroupIdY]
    );
}

#[test]
fn gpu_builtin_launch_plan_projects_called_builtins_from_signature() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut function = indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    function.blocks[0].statements = vec![
        gpu_builtin_statement(MirKernelBuiltin::WorkgroupIdY, index_ty),
        gpu_builtin_statement(MirKernelBuiltin::GlobalInvocationX, index_ty),
        gpu_builtin_statement(MirKernelBuiltin::WorkgroupIdY, index_ty),
    ];
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    let reflection = MirKernelReflection::from_signature("called", &signature, &validation).expect("reflection");
    let launch = reflection.launch_plan();

    assert_eq!(reflection.builtin_inputs.len(), 1);
    assert_eq!(reflection.builtin_inputs[0].builtin, MirKernelBuiltin::GlobalInvocationX);
    assert_eq!(
        reflection.called_builtins,
        vec![MirKernelBuiltin::WorkgroupIdY, MirKernelBuiltin::GlobalInvocationX]
    );
    assert_eq!(
        launch.required_builtins,
        vec![MirKernelBuiltin::GlobalInvocationX, MirKernelBuiltin::WorkgroupIdY]
    );
    assert_eq!(launch.builtin_inputs.len(), 1);
    assert_eq!(launch.builtin_inputs[0].builtin, MirKernelBuiltin::GlobalInvocationX);
    assert_eq!(
        launch.called_builtins,
        vec![MirKernelBuiltin::WorkgroupIdY, MirKernelBuiltin::GlobalInvocationX]
    );
}

#[test]
fn gpu_builtin_signature_preserves_source_names() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut function = indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let mut interner = Interner::new();
    function.params[0].name = Some(interner.intern("input"));
    function.params[1].name = Some(interner.intern("output"));
    function.params[2].name = Some(interner.intern("global_invocation_x"));
    function.params.push(MirParam {
        local: MirLocalId(3),
        name: Some(interner.intern("workgroup_id_y")),
        ty: index_ty,
        mode: MirParamMode::Owned,
        span: test_span(),
    });
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel_with_interner(&function, &validation, &interner)
        .expect("kernel signature");

    assert_eq!(signature.output.source_local, Some(MirLocalId(1)));
    assert_eq!(signature.input.source_name.as_deref(), Some("input"));
    assert_eq!(signature.output.source_name.as_deref(), Some("output"));
    assert_eq!(signature.builtin_inputs.len(), 2);
    assert_eq!(signature.builtin_inputs[0].local, MirLocalId(2));
    assert_eq!(signature.builtin_inputs[0].source_name.as_deref(), Some("global_invocation_x"));
    assert_eq!(signature.builtin_inputs[0].builtin, MirKernelBuiltin::GlobalInvocationX);
    assert_eq!(signature.builtin_inputs[1].local, MirLocalId(3));
    assert_eq!(signature.builtin_inputs[1].source_name.as_deref(), Some("workgroup_id_y"));
    assert_eq!(signature.builtin_inputs[1].builtin, MirKernelBuiltin::WorkgroupIdY);
}

#[test]
fn gpu_builtin_reflection_and_launch_preserves_source_names() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut function = indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let mut interner = Interner::new();
    function.params[0].name = Some(interner.intern("input"));
    function.params[1].name = Some(interner.intern("output"));
    function.params[2].name = Some(interner.intern("global_invocation_x"));
    function.params.push(MirParam {
        local: MirLocalId(3),
        name: Some(interner.intern("workgroup_id_y")),
        ty: index_ty,
        mode: MirParamMode::Owned,
        span: test_span(),
    });
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel_with_interner(&function, &validation, &interner)
        .expect("kernel signature");
    let reflection = MirKernelReflection::from_signature("multi", &signature, &validation).expect("reflection");

    assert_eq!(reflection.resources[0].source_name.as_deref(), Some("input"));
    assert_eq!(reflection.resources[1].source_name.as_deref(), Some("output"));
    let launch = reflection.launch_plan();
    assert_eq!(
        launch.bind_groups[0].source_names,
        vec!["input".to_owned(), "output".to_owned()]
    );
    assert_eq!(launch.bind_groups[0].source_name_count, 2);
    assert_eq!(reflection.builtin_inputs.len(), 2);
    assert_eq!(reflection.builtin_inputs[0].source_local, MirLocalId(2));
    assert_eq!(reflection.builtin_inputs[0].source_name.as_deref(), Some("global_invocation_x"));
    assert_eq!(reflection.builtin_inputs[0].builtin, MirKernelBuiltin::GlobalInvocationX);
    assert_eq!(reflection.builtin_inputs[0].axis, MirKernelAxis::X);
    assert_eq!(reflection.builtin_inputs[1].source_local, MirLocalId(3));
    assert_eq!(reflection.builtin_inputs[1].source_name.as_deref(), Some("workgroup_id_y"));
    assert_eq!(reflection.builtin_inputs[1].builtin, MirKernelBuiltin::WorkgroupIdY);
    assert_eq!(reflection.builtin_inputs[1].axis, MirKernelAxis::Y);
}

#[test]
fn gpu_builtin_indexed_kernel_rejects_duplicate_trailing_builtin_sources() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut function = indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let mut interner = Interner::new();
    let builtin_name = interner.intern("global_invocation_x");
    function.params[2].name = Some(builtin_name);
    function.params.push(MirParam {
        local: MirLocalId(3),
        name: Some(builtin_name),
        ty: index_ty,
        mode: MirParamMode::Owned,
        span: test_span(),
    });
    let validation = validation_with_device_tensor_layouts(&types);

    let error = MirKernelSignature::storage_buffer_kernel_with_interner(&function, &validation, &interner)
        .expect_err("duplicate builtin sources should fail");

    assert_eq!(error.issue, "mir_kernel_builtin_duplicate_name");
}

#[test]
fn gpu_builtin_indexed_kernel_keeps_global_invocation_x_for_ordinary_index_names() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut function = indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let mut interner = Interner::new();
    function.params[2].name = Some(interner.intern("id"));
    let validation = validation_with_device_tensor_layouts(&types);

    let signature = MirKernelSignature::storage_buffer_kernel_with_interner(&function, &validation, &interner)
        .expect("kernel signature");

    assert_eq!(signature.builtin_inputs[0].builtin, MirKernelBuiltin::GlobalInvocationX);
}

#[test]
fn gpu_builtin_kernel_reflection_can_record_yz_global_invocation_inputs() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(tensor_ty);
    let mut signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    signature.builtin_inputs = vec![
        MirKernelBuiltinInput {
            local: MirLocalId(10),
            source_name: None,
            builtin: MirKernelBuiltin::GlobalInvocationY,
            ty: index_ty,
        },
        MirKernelBuiltinInput {
            local: MirLocalId(11),
            source_name: None,
            builtin: MirKernelBuiltin::GlobalInvocationZ,
            ty: index_ty,
        },
    ];

    let reflection = MirKernelReflection::from_signature("axis", &signature, &validation).expect("kernel reflection");

    assert_eq!(reflection.builtin_inputs.len(), 2);
    assert_eq!(reflection.builtin_inputs[0].source_local, MirLocalId(10));
    assert_eq!(reflection.builtin_inputs[0].builtin, MirKernelBuiltin::GlobalInvocationY);
    assert_eq!(reflection.builtin_inputs[0].axis, MirKernelAxis::Y);
    assert_eq!(reflection.builtin_inputs[0].ty, index_ty);
    assert_eq!(reflection.builtin_inputs[0].layout, MirScalarLayout::U32);
    assert_eq!(reflection.builtin_inputs[1].source_local, MirLocalId(11));
    assert_eq!(reflection.builtin_inputs[1].builtin, MirKernelBuiltin::GlobalInvocationZ);
    assert_eq!(reflection.builtin_inputs[1].axis, MirKernelAxis::Z);
    assert_eq!(reflection.builtin_inputs[1].ty, index_ty);
    assert_eq!(reflection.builtin_inputs[1].layout, MirScalarLayout::U32);
}

#[test]
fn gpu_builtin_kernel_reflection_can_record_workgroup_and_local_inputs() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(tensor_ty);
    let mut signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    signature.builtin_inputs = vec![
        MirKernelBuiltinInput {
            local: MirLocalId(20),
            source_name: None,
            builtin: MirKernelBuiltin::WorkgroupIdX,
            ty: index_ty,
        },
        MirKernelBuiltinInput {
            local: MirLocalId(21),
            source_name: None,
            builtin: MirKernelBuiltin::LocalInvocationIdY,
            ty: index_ty,
        },
        MirKernelBuiltinInput {
            local: MirLocalId(22),
            source_name: None,
            builtin: MirKernelBuiltin::WorkgroupSizeZ,
            ty: index_ty,
        },
    ];

    let reflection =
        MirKernelReflection::from_signature("workgroup_local", &signature, &validation).expect("kernel reflection");

    assert_eq!(reflection.builtin_inputs.len(), 3);
    assert_eq!(reflection.builtin_inputs[0].source_local, MirLocalId(20));
    assert_eq!(reflection.builtin_inputs[0].builtin, MirKernelBuiltin::WorkgroupIdX);
    assert_eq!(reflection.builtin_inputs[0].axis, MirKernelAxis::X);
    assert_eq!(reflection.builtin_inputs[0].ty, index_ty);
    assert_eq!(reflection.builtin_inputs[0].layout, MirScalarLayout::U32);
    assert_eq!(reflection.builtin_inputs[1].source_local, MirLocalId(21));
    assert_eq!(reflection.builtin_inputs[1].builtin, MirKernelBuiltin::LocalInvocationIdY);
    assert_eq!(reflection.builtin_inputs[1].axis, MirKernelAxis::Y);
    assert_eq!(reflection.builtin_inputs[1].ty, index_ty);
    assert_eq!(reflection.builtin_inputs[1].layout, MirScalarLayout::U32);
    assert_eq!(reflection.builtin_inputs[2].source_local, MirLocalId(22));
    assert_eq!(reflection.builtin_inputs[2].builtin, MirKernelBuiltin::WorkgroupSizeZ);
    assert_eq!(reflection.builtin_inputs[2].axis, MirKernelAxis::Z);
    assert_eq!(reflection.builtin_inputs[2].ty, index_ty);
    assert_eq!(reflection.builtin_inputs[2].layout, MirScalarLayout::U32);
}

#[test]
fn kernel_reflection_rounds_workgroup_counts_up() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(tensor_ty);
    let mut signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    signature.dispatch_size = MirKernelDispatchSize { x: 10, y: 7, z: 1 };
    signature.workgroup = MirWorkgroupSize { x: 4, y: 3, z: 2 };

    let reflection =
        MirKernelReflection::from_signature("rounded", &signature, &validation).expect("kernel reflection");

    assert_eq!(
        (
            reflection.workgroup_count.x,
            reflection.workgroup_count.y,
            reflection.workgroup_count.z
        ),
        (3, 3, 1)
    );
}

#[test]
fn kernel_reflection_rejects_zero_workgroup_dimension() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(tensor_ty);
    let mut signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    signature.workgroup = MirWorkgroupSize { x: 0, y: 1, z: 1 };

    let error = MirKernelReflection::from_signature("invalid", &signature, &validation)
        .expect_err("zero workgroup dimension should reject");

    assert_eq!(error.issue, "mir_kernel_workgroup_nonzero");
}

#[test]
fn multi_input_signature_records_extra_input_facts() {
    let mut types = TypeTable::new();
    let (tensor_ty, element_ty) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let function = multi_input_indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");

    assert_eq!(signature.input.binding, 0);
    assert_eq!(signature.input.group, 0);
    assert_eq!(signature.input.role, MirKernelResourceRole::Input);
    assert_eq!(signature.input.access, MirKernelResourceAccess::Read);
    assert_eq!(signature.input.element_ty, element_ty);
    assert_eq!(signature.input.element_count, 4);
    assert_eq!(signature.input.source_local, Some(MirLocalId(0)));
    assert_eq!(signature.extra_inputs.len(), 1);
    assert_eq!(signature.extra_inputs[0].group, 0);
    assert_eq!(signature.extra_inputs[0].binding, 1);
    assert_eq!(signature.extra_inputs[0].role, MirKernelResourceRole::Input);
    assert_eq!(signature.extra_inputs[0].access, MirKernelResourceAccess::Read);
    assert_eq!(signature.extra_inputs[0].element_ty, element_ty);
    assert_eq!(signature.extra_inputs[0].element_count, 4);
    assert_eq!(signature.extra_inputs[0].source_local, Some(MirLocalId(1)));
    assert_eq!(signature.output.group, 0);
    assert_eq!(signature.output.binding, 2);
    assert_eq!(signature.output.role, MirKernelResourceRole::Output);
    assert_eq!(signature.output.access, MirKernelResourceAccess::Write);
    assert_eq!(signature.output.element_ty, element_ty);
    assert_eq!(signature.output.element_count, 4);
    assert_eq!(signature.output.source_local, Some(MirLocalId(2)));
    assert_eq!(signature.builtin_inputs[0].local, MirLocalId(3));
    assert_eq!(
        (signature.dispatch_size.x, signature.dispatch_size.y, signature.dispatch_size.z),
        (4, 1, 1)
    );
}

#[test]
fn multi_input_reflection_records_three_resources() {
    let mut types = TypeTable::new();
    let (tensor_ty, _element_ty) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let function = multi_input_indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    let reflection = MirKernelReflection::from_signature("sum", &signature, &validation).expect("kernel reflection");

    assert_eq!(reflection.resources.len(), 3);
    assert_eq!(
        (
            reflection.dispatch_size.x,
            reflection.dispatch_size.y,
            reflection.dispatch_size.z
        ),
        (4, 1, 1)
    );
    assert_eq!(reflection.resources[0].group, 0);
    assert_eq!(reflection.resources[0].binding, 0);
    assert_eq!(reflection.resources[0].source_local, Some(MirLocalId(0)));
    assert_eq!(reflection.resources[0].element_layout, MirScalarLayout::F32);
    assert_eq!(reflection.resources[0].element_byte_width, 4);
    assert_eq!(reflection.resources[0].element_count, 4);
    assert_eq!(reflection.resources[0].buffer_byte_len, 16);
    assert_eq!(reflection.resources[1].group, 0);
    assert_eq!(reflection.resources[1].binding, 1);
    assert_eq!(reflection.resources[1].source_local, Some(MirLocalId(1)));
    assert_eq!(reflection.resources[1].element_layout, MirScalarLayout::F32);
    assert_eq!(reflection.resources[1].element_byte_width, 4);
    assert_eq!(reflection.resources[1].element_count, 4);
    assert_eq!(reflection.resources[1].buffer_byte_len, 16);
    assert_eq!(reflection.resources[2].group, 0);
    assert_eq!(reflection.resources[2].binding, 2);
    assert_eq!(reflection.resources[2].source_local, Some(MirLocalId(2)));
    assert_eq!(reflection.resources[2].element_layout, MirScalarLayout::F32);
    assert_eq!(reflection.resources[2].element_byte_width, 4);
    assert_eq!(reflection.resources[2].element_count, 4);
    assert_eq!(reflection.resources[2].buffer_byte_len, 16);
}

#[test]
fn kernel_reflection_rejects_resource_byte_length_overflow() {
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(ty);
    let mut signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    signature.input.element_count = u64::MAX;

    let error =
        MirKernelReflection::from_signature("overflow", &signature, &validation).expect_err("byte length overflow");

    assert_eq!(error.issue, "mir_kernel_reflection_byte_length_overflow");
}

#[test]
fn indexed_tensor_view_kernel_abi_binds_and_reflects_scalar_resources() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let scalar_ty = f32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut function = multi_input_indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    let mut interner = Interner::new();
    // Runtime-varying epsilon remains a parameter, rather than a literal or
    // specialization fact. Its ABI resource must therefore survive binding,
    // reflection, and launch-plan projection.
    function.params[1].ty = scalar_ty;
    function.params[1].name = Some(interner.intern("epsilon"));
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel_with_interner(&function, &validation, &interner)
        .expect("indexed kernel accepts a count-one scalar resource");

    let epsilon = &signature.extra_inputs[0];
    assert_eq!(epsilon.binding, 1);
    assert_eq!(epsilon.kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(epsilon.role, MirKernelResourceRole::Input);
    assert_eq!(epsilon.access, MirKernelResourceAccess::Read);
    assert_eq!(epsilon.element_ty, scalar_ty);
    assert_eq!(epsilon.element_count, 1);
    assert_eq!(epsilon.source_local, Some(MirLocalId(1)));
    assert_eq!(epsilon.source_name.as_deref(), Some("epsilon"));

    let reflection =
        MirKernelReflection::from_signature("rmsnorm", &signature, &validation).expect("scalar binding reflects");
    let epsilon = reflection
        .resources
        .iter()
        .find(|resource| resource.source_name.as_deref() == Some("epsilon"))
        .expect("runtime epsilon resource reflection");
    assert_eq!(epsilon.kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(epsilon.element_layout, MirScalarLayout::F32);
    assert_eq!(epsilon.element_count, 1);
    assert_eq!(epsilon.buffer_byte_len, 4);

    let launch = reflection.launch_plan();
    assert_eq!(launch.bind_groups[0].resource_kinds[1], MirKernelResourceKind::StorageBuffer);
    assert_eq!(launch.bind_groups[0].resource_roles[1], MirKernelResourceRole::Input);

    // The former indexed-view rejection still applies to scalar layouts that
    // are not device-safe kernel elements; accepting f32 must not make f64
    // inputs fail open as count-one resources.
    function.params[1].ty = fractus_type(&types);
    let error = MirKernelSignature::storage_buffer_kernel_with_interner(&function, &validation, &interner)
        .expect_err("unsupported scalar input remains outside the indexed ABI");
    assert_eq!(error.issue, "mir_indexed_kernel_resource_not_device_view");
}

#[test]
fn multi_input_return_buffer_kernel_binds_scalar_resources_without_tensor_count_matching() {
    let mut types = TypeTable::new();
    let (tensor_ty, _) = rank1_tf32_type(&mut types);
    let scalar_ty = f32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut function = multi_input_indexed_kernel_function(tensor_ty, index_ty, vacuum_ty);
    function.params.pop();
    function.params[1].ty = scalar_ty;
    function.return_ty = tensor_ty;
    let validation = validation_with_device_tensor_layouts(&types);

    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation)
        .expect("multi-input return ABI accepts a count-one scalar resource");
    let epsilon = &signature.extra_inputs[0];
    assert_eq!(epsilon.binding, 1);
    assert_eq!(epsilon.kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(epsilon.element_ty, scalar_ty);
    assert_eq!(epsilon.element_count, 1);
    assert_eq!(signature.output.binding, 3);
}

#[test]
fn kernel_abi_rejects_default_fractus_layout() {
    let types = TypeTable::new();
    let ty = fractus_type(&types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(ty);
    let error = MirKernelSignature::storage_buffer_kernel(&function, &validation)
        .expect_err("bare fractus should not become a storage-buffer kernel ABI");

    assert_eq!(error.issue, "mir_kernel_scalar_layout_unsupported");
    assert_eq!(error.detail.as_deref(), Some("f64"));
}

#[test]
fn kernel_abi_rejects_atomic_buffer_parameters_explicitly() {
    let mut types = TypeTable::new();
    let ty = atomic_i32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(ty);
    let error = MirKernelSignature::storage_buffer_kernel(&function, &validation)
        .expect_err("atomic buffer parameters need a deliberate storage ABI");

    assert_eq!(error.issue, "mir_kernel_atomic_parameters_unsupported");
}

#[test]
fn kernel_reflection_rejects_atomic_resources_explicitly() {
    let mut types = TypeTable::new();
    let ty = atomic_i32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature {
        function: MirFunctionId(12),
        input: MirKernelResource {
            group: 0,
            binding: 0,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty: ty,
            element_count: 1,
            source_local: Some(MirLocalId(0)),
            source_name: Some("cell".to_owned()),
            reduced_projection: None,
        },
        extra_inputs: Vec::new(),
        output: MirKernelResource {
            group: 0,
            binding: 1,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Output,
            access: MirKernelResourceAccess::Write,
            element_ty: ty,
            element_count: 1,
            source_local: None,
            source_name: None,
            reduced_projection: None,
        },
        extra_outputs: Vec::new(),
        accumulation_buffers: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
    };
    let error = MirKernelReflection::from_signature("atomic_cell", &signature, &validation)
        .expect_err("atomic reflection resources need a deliberate storage contract");

    assert_eq!(error.issue, "mir_kernel_atomic_reflection_resources_unsupported");
}

#[test]
fn kernel_abi_admits_64_bit_integer_device_view_elements() {
    // Kernel-wide types ruling 2026-10-28: i64 and u64 are kernel values.
    for width in [NumericWidth::I64, NumericWidth::U64] {
        let mut types = TypeTable::new();
        let tensor_ty = rank1_tensor_type(&mut types, Primitive::Numerus, width);
        let index_ty = u32_type(&mut types);
        let return_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
        let validation = validation_with_device_tensor_layouts(&types);
        let function = indexed_kernel_function(tensor_ty, index_ty, return_ty);
        MirKernelSignature::storage_buffer_kernel(&function, &validation)
            .expect("64-bit integer device view elements are admitted");
    }
}

#[test]
fn kernel_abi_rejects_unsupported_scalar_device_view_elements() {
    for (primitive, width, expected) in [
        (Primitive::Numerus, NumericWidth::I16, "i16"),
        (Primitive::Fractus, NumericWidth::F64, "f64"),
    ] {
        let mut types = TypeTable::new();
        let tensor_ty = rank1_tensor_type(&mut types, primitive, width);
        let index_ty = u32_type(&mut types);
        let return_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
        let validation = validation_with_device_tensor_layouts(&types);
        let function = indexed_kernel_function(tensor_ty, index_ty, return_ty);
        let error = MirKernelSignature::storage_buffer_kernel(&function, &validation)
            .expect_err("unsupported scalar device view should remain outside kernel ABI");

        assert_eq!(error.issue, "mir_kernel_device_view_element_unsupported");
        assert_eq!(error.detail.as_deref(), Some(expected));
    }
}

#[test]
fn kernel_abi_runtime_classifies_selected_helper_families() {
    let types = TypeTable::new();
    let text_ty = MirType::semantic(types.primitive(Primitive::Textus));
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let diagnostic = MirRuntimeCall {
        intrinsic: MirIntrinsic::Diagnostic(crate::mir::MirDiagnosticKind::Nota),
        args: vec![MirOperand::Constant(crate::mir::MirConstant::String(
            crate::lexer::Symbol(1),
        ))],
        return_ty: vacuum_ty,
    };
    let format = MirRuntimeCall {
        intrinsic: MirIntrinsic::FormatString { template: crate::lexer::Symbol(2) },
        args: Vec::new(),
        return_ty: text_ty,
    };
    let assert_call = MirRuntimeCall { intrinsic: MirIntrinsic::Assert, args: Vec::new(), return_ty: vacuum_ty };

    let diagnostic_abi = MirRuntimeAbi::from_call(&diagnostic);
    assert_eq!(diagnostic_abi.family, MirRuntimeAbiFamily::RuntimeDiagnostic);
    assert_eq!(diagnostic_abi.category, MirRuntimeAbiKind::HostIntegration);
    assert!(diagnostic_abi.returns_void);

    let format_abi = MirRuntimeAbi::from_call(&format);
    assert_eq!(format_abi.family, MirRuntimeAbiFamily::RuntimeFormat);
    assert_eq!(format_abi.category, MirRuntimeAbiKind::CoreSemantics);
    assert!(!format_abi.returns_void);

    let assert_abi = MirRuntimeAbi::from_call(&assert_call);
    assert_eq!(assert_abi.family, MirRuntimeAbiFamily::RuntimeAssert);
    assert_eq!(assert_abi.category.slug(), "core-semantics");
    assert!(assert_abi.returns_void);
}

#[test]
fn kernel_abi_function_preserves_direct_signature_shape() {
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let function = kernel_function(ty);
    let abi = MirFunctionAbi::from_function(&function);

    assert_eq!(abi.function, function.id);
    assert_eq!(abi.params.len(), 1);
    assert_eq!(abi.params[0].local, MirLocalId(3));
    assert_eq!(abi.params[0].ty, ty);
    assert_eq!(abi.return_ty, ty);
    assert_eq!(abi.error_ty, None);
}

// ============================================================================
// D-W6-A1 — shared collection-op workgroup-count path (U2, ABI-layer suite)
// ============================================================================
//
// The collection-op workgroup_count semantics lifted into the shared ABI
// (decision D-A1): `collection_op_workgroup_count` + the
// `MirKernelReflection::apply_effective_workgroup` correction. These tests
// pin the shared path directly — the values every consumer (WGSL probe, chain
// descriptor, launch plan, reflection JSON, metal probe) must report.

/// Minimal kernel function body: one collection-op runtime call — the only
/// shape `collection_op_workgroup_count` / `apply_effective_workgroup`
/// inspect. No validation is needed; the shared path scans statements only.
fn collection_op_kernel_function(op: MirCollectionOp, return_ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(40),
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
                    call: MirRuntimeCall { intrinsic: MirIntrinsic::Collection(op), args: Vec::new(), return_ty },
                },
                span: test_span(),
            }],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
            span: test_span(),
        }],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

#[test]
fn abi_collection_op_workgroup_count_matmul_reports_dispatch_verbatim() {
    // Blind-spot suite (D-W6-A1 U2): the shared path must report the matmul
    // dispatch grid verbatim. PRE-lift, `workgroup_count_for_dispatch`
    // computed the naive ceil-divide — for every shape below
    // ceil(dispatch/(8,8,1)) collapses to (1,1,1) once any tile dimension
    // exceeds 8, which is exactly the M=N=2 blind spot (16×16, rectangular
    // 16×8/4×16, and the M=1 edge all under-launch to one workgroup).
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let function = collection_op_kernel_function(MirCollectionOp::TensorMatMul, ty);

    // 16×16: tile grid (2,2,1) — PRP-004's canonical case.
    let count = collection_op_workgroup_count(
        &function,
        MirKernelDispatchSize { x: 2, y: 2, z: 1 },
        MirWorkgroupSize { x: 8, y: 8, z: 1 },
    );
    assert_eq!((count.x, count.y, count.z), (2, 2, 1));

    // Rectangular 16×8: tile grid (1,2,1) — the y-axis tile grid must not be
    // divided by the tile (naive would say (1,1,1)).
    let count = collection_op_workgroup_count(
        &function,
        MirKernelDispatchSize { x: 1, y: 2, z: 1 },
        MirWorkgroupSize { x: 8, y: 8, z: 1 },
    );
    assert_eq!((count.x, count.y, count.z), (1, 2, 1));

    // Rectangular 4×16: tile grid (2,1,1) — the x-axis.
    let count = collection_op_workgroup_count(
        &function,
        MirKernelDispatchSize { x: 2, y: 1, z: 1 },
        MirWorkgroupSize { x: 8, y: 8, z: 1 },
    );
    assert_eq!((count.x, count.y, count.z), (2, 1, 1));

    // M=1 edge (1×16): tile grid (2,1,1) — naive ceil((2,1,1)/(8,8,1)) =
    // (1,1,1) would drop the right half of the single output row.
    let count = collection_op_workgroup_count(
        &function,
        MirKernelDispatchSize { x: 2, y: 1, z: 1 },
        MirWorkgroupSize { x: 8, y: 8, z: 1 },
    );
    assert_eq!((count.x, count.y, count.z), (2, 1, 1));
}

#[test]
fn abi_collection_op_workgroup_count_reduction_reports_ceil_partials() {
    // Blind-spot suite (D-W6-A1 U2): reduction (Sum/Mean) partial slots are
    // ceil(n/ws) — the shared path must report the same value as the
    // consumers. n=100/ws=128 → 1 slot; n=500/ws=256 → 2 slots.
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let function = collection_op_kernel_function(MirCollectionOp::Sum, ty);

    // n < ws.
    let count = collection_op_workgroup_count(
        &function,
        MirKernelDispatchSize { x: 100, y: 1, z: 1 },
        MirWorkgroupSize { x: 128, y: 1, z: 1 },
    );
    assert_eq!((count.x, count.y, count.z), (1, 1, 1));

    // n > ws.
    let count = collection_op_workgroup_count(
        &function,
        MirKernelDispatchSize { x: 500, y: 1, z: 1 },
        MirWorkgroupSize { x: 256, y: 1, z: 1 },
    );
    assert_eq!((count.x, count.y, count.z), (2, 1, 1));
}

#[test]
fn abi_collection_op_workgroup_count_elementwise_keeps_ceil() {
    // Parity guard (D-A1, C-A1-N): the non-matmul branch must stay unchanged
    // for elementwise/non-collection kernels — ceil(dispatch/effective).
    // A standalone TensorAdd dispatches (8,1,1) thread coverage under the
    // effective (8,1,1) grid → one workgroup; a zero-workgroup effective
    // dimension is defended by `.max(1)` (no panic, matches the chain
    // descriptor guard).
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let function = collection_op_kernel_function(MirCollectionOp::TensorAdd, ty);

    let count = collection_op_workgroup_count(
        &function,
        MirKernelDispatchSize { x: 8, y: 1, z: 1 },
        MirWorkgroupSize { x: 8, y: 1, z: 1 },
    );
    assert_eq!((count.x, count.y, count.z), (1, 1, 1));

    let count = collection_op_workgroup_count(
        &function,
        MirKernelDispatchSize { x: 10, y: 1, z: 1 },
        MirWorkgroupSize { x: 8, y: 1, z: 1 },
    );
    assert_eq!((count.x, count.y, count.z), (2, 1, 1));
}

#[test]
fn abi_apply_effective_workgroup_idempotent_for_non_collection_kernel() {
    // C-A1-N guard at the shared layer: applying the correction with the
    // signature default effective workgroup must be byte-identical to the
    // naive `from_signature` value — non-collection consumers are unchanged
    // by the lift.
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let function = kernel_function(ty);
    let validation = validation_with_device_tensor_layouts(&types);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    let before = MirKernelReflection::from_signature("add_one", &signature, &validation).expect("reflection");
    let mut after = before.clone();
    after.apply_effective_workgroup(&function, signature.workgroup);

    assert_eq!(before.workgroup, after.workgroup);
    assert_eq!(before.workgroup_count, after.workgroup_count);
    assert_eq!(before.dispatch_size, after.dispatch_size);
    assert_eq!(
        (after.workgroup_count.x, after.workgroup_count.y, after.workgroup_count.z),
        (1, 1, 1)
    );
}
