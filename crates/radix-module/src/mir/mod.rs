//! Middle IR model for execution-shaped compiler analysis.
//!
//! Core MIR lives in foundation leaf `radix-mir`. This package module re-exports
//! that leaf and owns residual MIR emit backends until those peel into their own
//! crates. HIR→MIR lowering moved to `radix-mir-lower` (goal `radix-crate-dag`,
//! C-M1); the `lower`/fragment modules below are shims over it.

// Shims over `radix-mir-lower` (production moved; the fragments keep their test
// hosts here, so they are test-only modules).
#[cfg(test)]
pub(crate) mod fragment_composition;
#[cfg(test)]
pub(crate) mod fragment_instantiation;
mod lower;
#[cfg(feature = "device")]
pub(crate) use lower::expand_deferred_vector_ops;
// Emit + interpret + coverage leaves (façades re-export radix-mir-*).
#[cfg(feature = "mir-basic")]
mod basic_text;
#[cfg(feature = "mir-coverage")]
mod coverage;
#[cfg(feature = "mir-llvm")]
mod llvm_text;
#[cfg(feature = "mir-metal")]
mod metal_text;
#[cfg(feature = "runner")]
mod runner;
#[cfg(feature = "mir-sexp")]
mod sexp_text;
#[cfg(feature = "mir-wasm")]
mod wasm_text;
#[cfg(feature = "mir-wgsl")]
mod wgsl_text;

#[cfg(any(test, feature = "test-support"))]
#[path = "test_support.rs"]
pub mod test_support;

#[cfg(test)]
#[path = "float_literal_receiver_test.rs"]
mod float_literal_receiver_test;
#[cfg(test)]
#[path = "itera_apud_indexed_test.rs"]
mod itera_apud_indexed_tests;
#[cfg(test)]
#[path = "itera_destructure_test.rs"]
mod itera_destructure_tests;

#[cfg(test)]
#[path = "bounded_itera_test.rs"]
mod bounded_itera_test;

#[cfg(test)]
#[path = "bounded_cord_methods_test.rs"]
mod bounded_cord_methods_test;

#[cfg(test)]
#[path = "bounded_text_ops_test.rs"]
mod bounded_text_ops_test;

#[cfg(test)]
#[path = "affine_destination_test.rs"]
mod affine_destination_test;

#[cfg(test)]
#[path = "runtime_extent_itera_test.rs"]
mod runtime_extent_itera_test;

#[cfg(all(test, feature = "runner"))]
#[path = "reducta_filum_test.rs"]
mod reducta_filum_test;

#[cfg(all(test, feature = "runner"))]
#[path = "scan_reference_test.rs"]
mod scan_reference_tests;

#[cfg(all(test, feature = "runner"))]
#[path = "kernel_host_lowering_test.rs"]
mod kernel_host_lowering_test;

#[cfg(test)]
#[path = "fault_sites_test.rs"]
mod fault_sites_test;

#[cfg(test)]
#[path = "index_proof_test.rs"]
mod index_proof_test;
#[cfg(all(test, feature = "mir-metal"))]
#[path = "shift_view_test.rs"]
mod shift_view_test;

#[cfg(test)]
#[path = "kernel_get_set_test.rs"]
mod kernel_get_set_test;

#[cfg(test)]
#[path = "kernel_throw_test.rs"]
mod kernel_throw_test;

#[cfg(all(
    test,
    feature = "mir-sexp",
    feature = "mir-wasm",
    feature = "mir-metal",
    feature = "mir-llvm",
    feature = "mir-wgsl"
))]
#[path = "device_block_erase_test.rs"]
mod device_block_erase_test;

#[cfg(test)]
#[path = "grid_reduce_test.rs"]
mod grid_reduce_test;

#[cfg(test)]
#[path = "control_coverage_test.rs"]
mod control_coverage_test;

#[cfg(test)]
#[path = "reduction_access_marker_test.rs"]
mod reduction_access_marker_test;

#[cfg(test)]
#[path = "device_value_join_test.rs"]
mod device_value_join_test;
#[cfg(test)]
#[path = "kernel_tier_test.rs"]
mod kernel_tier_test;
#[cfg(test)]
#[path = "lane_form_test.rs"]
mod lane_form_test;

#[cfg(test)]
#[path = "interface_dispatch_test.rs"]
mod interface_dispatch_test;
#[cfg(test)]
#[path = "ledger_lower_test.rs"]
mod ledger_lower_test;
#[cfg(test)]
#[path = "lista_lift_test.rs"]
mod lista_lift_test;
#[cfg(test)]
#[path = "optional_vector_literal_test.rs"]
mod optional_vector_literal_tests;
#[cfg(test)]
#[path = "queue_stack_test.rs"]
mod queue_stack_test;
#[cfg(test)]
#[path = "sink_into_run_test.rs"]
mod sink_into_run_test;
#[cfg(test)]
#[path = "union_overlap_dispatch_test.rs"]
mod union_overlap_dispatch_test;

// --- Core from radix-mir (module paths for residual code) ---
pub use radix_mir::{
    DisplayTokensId, KernelModule, MirAtomicOp, MirCollectionOp, MirColorTargetFormat, MirDepthCompareFunction,
    MirDepthStencilState, MirDisplayTokens, MirGraphicsPipelineReflection, MirInputStepMode, MirPrimitiveTopology,
    MirStencilFaceState, MirStencilOp, abi, ai_workbench_stage5, artifact_completeness, boundary_abi, capability,
    control, device, device_semantics, dump, generic, host_partition, inference_blocker_matrix, layout, names, nodes,
    rank2_reduction_readiness, tensor_operation_floor, tensor_operation_parity, tensor_placement, tensor_runtime_abi,
    tensor_systems_target, ty, validate, visit,
};

#[cfg(feature = "mir-coverage")]
pub use coverage::{MIR_COVERAGE_TARGETS, MirCoverageTarget, classify_mir_coverage};
#[cfg(feature = "mir-llvm")]
// DDCP3-U2 (b) mixed-path disambiguation: `LlvmEmitRequest` selects the
// host-partition CPU-module path for a package that carries device roles
// (direct declarations/calls, no device bodies); the ordinary request with
// roles remains the NVVM device-artifact probe path. The driver routes
// `llvm-text`/`llvm-host` through the lane when roles are present — no route
// silently emits device bodies into a CPU module.
pub use llvm_text::{
    CliAdapterField, CliAdapterPlan, CliRecordFunction, LlvmEmitRequest, MirLlvmTextProbeError, classify_lowerability,
    emit_llvm_text_probe, fmir_artifact_packet_facts,
};
pub use lower::prepare_mir_lowering_interner;
pub use lower::{
    ImportedDeviceIndex, ImportedDeviceRegistration, ImportedDeviceRoute, KernelLowering, LoweredMirUnit, MirError,
    MirGenericParam, MirLoweringOptions, MirRuntimeRecordField, MirRuntimeRecordValue, MirTensorEdgeAppliedArg,
    MirTensorEdgePolicyMetadata, MirTensorEdgePolicyProvider, MirTensorEdgePolicySelector, MirTensorEdgeShapeIdentity,
    MirTensorEdgeTypeIdentity, MirTensorLimesTemplate, PackageStructMetadata, dump_analyzed_unit, holds_print_site,
    instantiate_merged_generic_calls, instantiate_merged_generic_calls_with_devices,
    instantiate_merged_generic_calls_with_devices_and_metadata,
    instantiate_merged_generic_calls_with_tensor_edge_policy, lower_analyzed_unit,
};
#[cfg(feature = "mir-metal")]
pub use metal_text::{
    MirMetalTextProbeError, MirMetalTextProbeOutput, emit_metal_text_probe_with_device_roles,
    emit_metal_text_probe_with_reflection_and_device_roles,
};
pub use radix_mir::boundary_abi::{
    BoundaryAbiError, BoundaryAbiKind, BoundaryCall, HOST_CALL_ABI_VERSION, PreparedRegion, validate_boundary_abi,
};
// DDCP3-U4/U5: the typed `§ArtifactPacket` `HostDeviceCall` surface (typed
// bindings, bounded numeric argument layout, the execution-descriptor
// identity domain) + the typed completeness gate (gate bullets 5/6) + the
// gate-6 hostile-shape surface. Re-export hunks serialized after U2/U3's
// (the U4 residual; U5 adds the gate classes + the caller-route adapter).
pub use radix_mir::artifact_completeness::{
    ArtifactPacketFacts, CompletenessError, EntrypointSymbolFact, LaunchDimensionFact, PreparedResourceFact,
    ReflectedKernelFact, ReflectedResourceFact, RegionPreparationFacts, check_preparation_completeness,
    derive_region_preparation_facts,
};
pub use radix_mir::boundary_abi::{
    CallBinding, CallBindingRole, CallFailureBehavior, ExecutionDescriptor, HostDeviceCall, MemberKernel,
    RegionArgumentLayout, derive_host_device_calls, execution_descriptor_hash,
};
pub use radix_mir::dump_program;
// DDCP3-U3: the typed, serializable partition/callsite record (membership +
// callsite mapping + explicit effect boundaries) — the compiler artifact
// proving DDCP3 gate bullets 1 + 2. Re-export hunks serialized after U2's.
pub use radix_mir::host_partition::{
    CallsiteId, CallsiteRecord, EffectBoundaryRecord, HostPartition, HostPartitionError, PartitionMember,
    PartitionRecord, PartitionRole, RegionRecord, derive_host_partition, derive_partition_record,
    derive_partition_record_with_metadata,
};
pub use radix_mir::nodes::*;
pub use radix_mir::{
    AiWorkbenchStage5Facet, AiWorkbenchStage5Row, AiWorkbenchStage5Status, ai_workbench_stage5_native_status,
    ai_workbench_stage5_rows,
};
pub use radix_mir::{
    MirDeviceContext, MirDeviceRole, classify_device_safe, classify_device_safe_for_target_entry,
    device_entry_roles_from_hir, device_roles_from_hir,
};
// KPC-EMIT: the imported-entry merge surface re-exported from the device
// module (the crate-root list above predates it; `device` is public).
// KCM-05: the synthetic-kernel metadata carrier rides the same device module.
#[cfg(feature = "mir-basic")]
pub use basic_text::{MirBasicError, emit_basic_text};
pub(crate) use radix_mir::MirGenericBindings;
pub use radix_mir::device::{
    MirDeviceFunctionMetadata, MirDeviceFunctionMetadataMap, MirImportedEntryError, MirImportedEntryFact,
    merge_imported_entry_roles,
};
#[cfg(test)]
pub(crate) use radix_mir::type_contains_param;
pub use radix_mir::{CapabilityGap, Lowerability};
pub use radix_mir::{
    INTEGER_PAYLOAD_I64_SENTINEL, INTEGER_PAYLOAD_U8_SENTINEL, INTEGER_PAYLOAD_U64_SENTINEL, MirEnumRepresentation,
    MirFunctionSignature, MirInterner, MirParamSignature, MirScalarValueDomain, MirTypeTable, MirValidationContext,
    MirValidationError, ValidatedMir, validate_program,
};
pub use radix_mir::{
    InferenceBlockerKind, InferenceBlockerRow, InferenceBlockerStatus, InferenceNonClaim, InferenceNonClaimKind,
    inference_blocker_rows, inference_non_claims,
};
pub use radix_mir::{
    MirAbiError, MirFragmentOutputReflection, MirFunctionAbi, MirFunctionAbiParam, MirGpuReflection, MirKernelAxis,
    MirKernelBufferBindingKind, MirKernelBuiltin, MirKernelBuiltinInput, MirKernelBuiltinInputReflection,
    MirKernelDispatchSize, MirKernelLaunchBindGroup, MirKernelLaunchBindGroupLayoutEntry, MirKernelLaunchBinding,
    MirKernelLaunchPipelineBindGroupLayout, MirKernelLaunchPipelineLayout, MirKernelLaunchPlan, MirKernelOffsetMode,
    MirKernelProvenance, MirKernelReflection, MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind,
    MirKernelResourceReflection, MirKernelResourceRole, MirKernelShaderAccess, MirKernelShaderStage,
    MirKernelSignature, MirKernelWorkgroupCount, MirRuntimeAbi, MirRuntimeAbiFamily, MirRuntimeAbiKind,
    MirVaryingInterpolation, MirVaryingReflection, MirVertexInputFormat, MirVertexInputReflection,
    MirWebGpuBindGroupDescriptor, MirWebGpuBindGroupEntryDescriptor, MirWebGpuBindGroupLayoutDescriptor,
    MirWebGpuBindGroupLayoutEntryDescriptor, MirWebGpuDispatchWorkgroups, MirWebGpuLaunchAdapter,
    MirWebGpuPipelineLayoutDescriptor, MirWebGpuVertexAttributeDescriptor, MirWebGpuVertexBufferLayoutDescriptor,
    MirWebGpuVertexStepMode, MirWorkgroupSize, ShaderStageIoRole, reject_kernel_matrix_abi,
    validate_varying_consistency,
};
pub use radix_mir::{
    MirAggregateCarrier, MirAggregateFamily, MirAggregateLayout, MirAtomicLayout, MirDeviceMatrixLayout,
    MirDeviceVectorLayout, MirDeviceViewElement, MirDeviceViewLayout, MirLayout, MirLayoutError, MirLayoutKind,
    MirLayoutTable, MirPackedBitOrder, MirPackedNumericFormat, MirPackedNumericLayout, MirScalarLayout,
    MirTensorLocusLayout, MirTensorStorageLayout, MirTensorStrideLayout, MirTensorViewLayout,
};
pub use radix_mir::{
    MirTypeError, MirTypeLookup, constant_ty, index_projection_result_ty, option_chain_base_ty, option_payload_ty,
    place_base_ty,
};
pub use radix_mir::{
    Rank2ReductionReadinessPath, Rank2ReductionReadinessRow, Rank2ReductionReadinessStatus,
    rank2_reduction_readiness_rows,
};
pub use radix_mir::{
    TensorOperationFamily, TensorOperationParityRow, TensorParityStatus, TensorParityTarget, indexed_access_parity,
};
pub use radix_mir::{
    TensorOperationFloorFamily, TensorOperationFloorRow, TensorOperationFloorStatus, TensorOperationFloorTarget,
    required_tensor_operation_floor_families, tensor_operation_floor_rows,
};
pub use radix_mir::{
    TensorPlacementFacet, TensorPlacementOperation, TensorPlacementPolicyRow, TensorPlacementStatus,
    tensor_placement_operations, tensor_placement_policy_rows,
};
pub use radix_mir::{
    TensorRuntimeAbiFacet, TensorRuntimeAbiRow, TensorRuntimeAbiStatus, TensorRuntimeAbiTarget, tensor_runtime_abi_rows,
};
pub use radix_mir::{
    TensorSystemsTarget, TensorSystemsTargetLayout, TensorSystemsTargetRank, TensorSystemsTargetRow,
    TensorSystemsTargetStatus, selected_tensor_systems_family, tensor_systems_target_rows,
};
#[cfg(feature = "runner")]
pub use runner::{
    BufferHost, Host, RUNNER_KERNEL_REFUSED_ISSUE, RunSourceError, RunnerDispatchStatus, RunnerError,
    RunnerKernelPolicy, RunnerRunOutcome, RunnerStats, StdioHost, Value, classify_run_tier,
    classify_runner_lowerability, configure_shuffle_seed, reset_execution_state, run_analyzed,
    run_analyzed_with_kernel_lowering, run_entry, run_entry_with_kernel_policy, run_entry_with_stats,
    run_entry_with_stats_and_kernel_policy, run_function, run_function_with_kernel_policy, run_function_with_limits,
    run_function_with_limits_and_kernel_policy, run_function_with_stats_and_limits,
    run_function_with_stats_and_limits_and_kernel_policy, run_function_with_step_budget, run_source,
    runner_dispatch_status, serve_route,
};
#[cfg(feature = "mir-sexp")]
pub use sexp_text::{MirSexpTextProbeError, classify_sexp_lowerability, emit_sexp_text};
#[cfg(feature = "mir-wasm")]
pub use wasm_text::{MirWasmTextProbeError, emit_wasm_text_probe, emit_wasm_text_probe_package_aware};
#[cfg(feature = "mir-wgsl")]
pub use wgsl_text::{
    BindGroupEntry, InterKernelBufferIdentity, KernelChainDescriptor, KernelDescriptor, MirWgslTextProbeError,
    MirWgslTextProbeOutput, StorageBufferDecl, WGSL_UNBOUND_CALL_ISSUE, WgslAnnotatedStage, WgslBoundCallTable,
    WgslCalleeKey, WgslEntryContext, WgslFieldBuiltins, emit_chain_descriptor, emit_wgsl_annotated_stages,
    emit_wgsl_fragment_entry_contract, emit_wgsl_fragment_entry_contract_with_targets,
    emit_wgsl_text_probe_with_device_roles, emit_wgsl_text_probe_with_reflection_and_device_roles,
    emit_wgsl_vertex_entry_contract,
};

pub use radix_mir::HANDLER_MESSAGE_FIELD;
pub use radix_mir::{
    MirNames, ascii_literal_identity, external_function_import_name, octeti_literal_identity,
    regex_flags_literal_identity, regex_literal_flags_suffix, regex_pattern_literal_identity, text_literal_identity,
};
pub use radix_mir::{terminator_successors, walk_operand, walk_place, walk_stmt, walk_terminator, walk_value};

// --- Package residual unit tests for radix-mir core (façade host) ---
// Tests stayed in package when core peels moved to leaves; super:: is this module.
#[cfg(test)]
#[path = "abi_test.rs"]
mod abi_tests;
#[cfg(test)]
#[path = "ai_workbench_stage5_test.rs"]
mod ai_workbench_stage5_tests;
#[cfg(test)]
#[path = "axis_residual_run_test.rs"]
mod axis_residual_run_tests;
#[cfg(test)]
#[path = "bits_conversio_run_test.rs"]
mod bits_conversio_run_tests;
#[cfg(test)]
#[path = "bits_ops_test.rs"]
mod bits_ops_tests;
#[cfg(test)]
#[path = "capability_test.rs"]
mod capability_tests;
#[cfg(test)]
#[path = "capped_store_test.rs"]
mod capped_store_tests;

#[cfg(test)]
#[path = "commune_stepper_test.rs"]
mod commune_stepper_tests;
#[cfg(test)]
#[path = "constant_fold_agreement_test.rs"]
mod constant_fold_agreement_tests;
#[cfg(test)]
#[path = "conversio_function_test.rs"]
mod conversio_function_tests;
#[cfg(test)]
#[path = "copy_conversio_test.rs"]
mod copy_conversio_tests;
#[cfg(test)]
#[path = "copy_duplicate_test.rs"]
mod copy_duplicate_tests;
#[cfg(all(test, target_os = "macos", feature = "mir-metal"))]
#[path = "copy_into_device_test.rs"]
mod copy_into_device_tests;
#[cfg(test)]
#[path = "copy_into_run_test.rs"]
mod copy_into_run_tests;
#[cfg(test)]
#[path = "decimal_constant_join_test.rs"]
mod decimal_constant_join_tests;
#[cfg(test)]
#[path = "decimal_projection_compare_test.rs"]
mod decimal_projection_compare_tests;
#[cfg(test)]
#[path = "device_test.rs"]
mod device_tests;
#[cfg(test)]
#[path = "exact_integer_test.rs"]
mod exact_integer_tests;
#[cfg(test)]
#[path = "generic_call_run_test.rs"]
mod generic_call_run_tests;
#[cfg(test)]
#[path = "generic_receiver_call_test.rs"]
mod generic_receiver_call_tests;
#[cfg(test)]
#[path = "implicit_store_test.rs"]
mod implicit_store_tests;
#[cfg(test)]
#[path = "inc_dec_store_test.rs"]
mod inc_dec_store_tests;
#[cfg(test)]
#[path = "inf_approx_test.rs"]
mod inf_approx_tests;
#[cfg(test)]
#[path = "inf_test.rs"]
mod inf_tests;
#[cfg(test)]
#[path = "inference_blocker_matrix_test.rs"]
mod inference_blocker_matrix_tests;
#[cfg(test)]
#[path = "infinity_literal_test.rs"]
mod infinity_literal_tests;
#[cfg(test)]
#[path = "layout_test.rs"]
mod layout_tests;
#[cfg(test)]
#[path = "list_slice_literal_run_test.rs"]
mod list_slice_literal_run_tests;
#[cfg(test)]
#[path = "list_window_test.rs"]
mod list_window_tests;
#[cfg(test)]
#[path = "littera_slot_test.rs"]
mod littera_slot_tests;
#[cfg(test)]
#[path = "matrix_bracket_access_test.rs"]
mod matrix_bracket_access_tests;
#[cfg(test)]
#[path = "matrix_store_test.rs"]
mod matrix_store_tests;
#[cfg(test)]
#[path = "membership_test.rs"]
mod membership_tests;
#[cfg(test)]
#[path = "narrowing_conversio_test.rs"]
mod narrowing_conversio_tests;
#[cfg(test)]
#[path = "nodes_test.rs"]
mod nodes_tests;
#[cfg(test)]
#[path = "numeric_unary_test.rs"]
mod numeric_unary_tests;
#[cfg(test)]
#[path = "ordering_test.rs"]
mod ordering_tests;
#[cfg(test)]
#[path = "ordina_key_test.rs"]
mod ordina_key_tests;
#[cfg(test)]
#[path = "ordo_conversio_test.rs"]
mod ordo_conversio_tests;
#[cfg(test)]
#[path = "potentia_test.rs"]
mod potentia_tests;
#[cfg(test)]
#[path = "ranged_list_test.rs"]
mod ranged_list_tests;
#[cfg(test)]
#[path = "rank2_reduction_readiness_test.rs"]
mod rank2_reduction_readiness_tests;
#[cfg(test)]
#[path = "record_literal_test.rs"]
mod record_literal_tests;
#[cfg(test)]
#[path = "reducta_group_identity_test.rs"]
mod reducta_group_identity_tests;
#[cfg(test)]
#[path = "saturating_word_test.rs"]
mod saturating_word_tests;
#[cfg(test)]
#[path = "sort_expr_test.rs"]
mod sort_expr_tests;

#[cfg(test)]
#[path = "ascii_unicode_escape_test.rs"]
mod ascii_unicode_escape_tests;
#[cfg(test)]
#[path = "tensor_bracket_range_test.rs"]
mod tensor_bracket_range_tests;
#[cfg(test)]
#[path = "tensor_family_conversio_test.rs"]
mod tensor_family_conversio_tests;
#[cfg(test)]
#[path = "tensor_lift_test.rs"]
mod tensor_lift_tests;
#[cfg(test)]
#[path = "tensor_operation_floor_test.rs"]
mod tensor_operation_floor_tests;
#[cfg(test)]
#[path = "tensor_operation_parity_test.rs"]
mod tensor_operation_parity_tests;
#[cfg(test)]
#[path = "tensor_placement_test.rs"]
mod tensor_placement_tests;
#[cfg(test)]
#[path = "tensor_runtime_abi_test.rs"]
mod tensor_runtime_abi_tests;
#[cfg(test)]
#[path = "tensor_systems_target_test.rs"]
mod tensor_systems_target_tests;
#[cfg(test)]
#[path = "textus_unicode_escape_test.rs"]
mod textus_unicode_escape_tests;
#[cfg(test)]
#[path = "true_division_zero_trap_test.rs"]
mod true_division_zero_trap_tests;
#[cfg(test)]
#[path = "ty_test.rs"]
mod ty_tests;
#[cfg(test)]
#[path = "unsigned_integer_test.rs"]
mod unsigned_integer_tests;
#[cfg(test)]
#[path = "vector_crea_test.rs"]
mod vector_crea_tests;
#[cfg(test)]
#[path = "vector_deferral_test.rs"]
mod vector_deferral_tests;

#[cfg(test)]
#[allow(unused_imports)]
#[path = "names_test.rs"]
mod names_unit;

#[cfg(test)]
#[allow(unused_imports)]
#[path = "validate_test.rs"]
mod validate_unit;

#[cfg(test)]
#[allow(unused_imports)]
#[path = "validate_enum_representation_test.rs"]
mod validate_enum_unit;

#[cfg(test)]
#[allow(unused_imports)]
#[path = "visit_test.rs"]
mod visit_unit;

#[cfg(test)]
#[path = "fmir_target_delegation_test.rs"]
mod fmir_target_delegation_tests;
