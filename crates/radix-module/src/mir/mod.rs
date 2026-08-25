//! Middle IR model for execution-shaped compiler analysis.
//!
//! Core MIR lives in foundation leaf `radix-mir`. This package module re-exports
//! that leaf and owns residual HIR→MIR lowering plus emit backends until those
//! peel into their own crates.

// Residual: HIR→MIR lowering (needs AnalyzedModule / driver).
pub(crate) mod fragment_composition;
mod fragment_instantiation;
mod lower;
// Emit + interpret + coverage leaves (façades re-export radix-mir-*).
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

#[cfg(test)]
#[path = "test_support.rs"]
pub mod test_support;

#[cfg(test)]
#[path = "itera_destructure_test.rs"]
mod itera_destructure_tests;

#[cfg(test)]
#[path = "bounded_itera_test.rs"]
mod bounded_itera_test;

#[cfg(test)]
#[path = "queue_stack_test.rs"]
mod queue_stack_test;

// --- Core from radix-mir (module paths for residual code) ---
pub use radix_mir::{
    abi, ai_workbench_stage5, artifact_completeness, boundary_abi, capability, control, device, device_semantics, dump,
    generic, host_partition, inference_blocker_matrix, layout, names, nodes, rank2_reduction_readiness,
    tensor_operation_floor, tensor_operation_parity, tensor_placement, tensor_runtime_abi, tensor_systems_target, ty,
    validate, visit, KernelModule, MirAtomicOp, MirCollectionOp, MirColorTargetFormat, MirDepthCompareFunction,
    MirDepthStencilState, MirGraphicsPipelineReflection, MirInputStepMode, MirPrimitiveTopology, MirStencilFaceState,
    MirStencilOp,
};

#[cfg(feature = "mir-coverage")]
pub use coverage::{classify_mir_coverage, MirCoverageTarget, MIR_COVERAGE_TARGETS};
#[cfg(feature = "mir-llvm")]
// DDCP3-U2 (b) mixed-path disambiguation: `LlvmEmitRequest` selects the
// host-partition CPU-module path for a package that carries device roles
// (direct declarations/calls, no device bodies); the ordinary request with
// roles remains the NVVM device-artifact probe path. The driver routes
// `llvm-text`/`llvm-host` through the lane when roles are present — no route
// silently emits device bodies into a CPU module.
pub use llvm_text::{
    classify_lowerability, emit_llvm_text_probe, fmir_artifact_packet_facts, CliAdapterField, CliAdapterPlan,
    CliRecordFunction, LlvmEmitRequest, MirLlvmTextProbeError,
};
pub use lower::prepare_mir_lowering_interner;
pub use lower::{
    dump_analyzed_unit, instantiate_merged_generic_calls, lower_analyzed_unit, LoweredMirUnit, MirError,
    MirGenericParam, MirLoweringOptions, MirRuntimeRecordField, MirRuntimeRecordValue,
};
#[cfg(feature = "mir-metal")]
pub use metal_text::{
    emit_metal_text_probe_with_device_roles, emit_metal_text_probe_with_reflection_and_device_roles,
    MirMetalTextProbeError, MirMetalTextProbeOutput,
};
pub use radix_mir::boundary_abi::{
    validate_boundary_abi, BoundaryAbiError, BoundaryAbiKind, BoundaryCall, PreparedRegion, HOST_CALL_ABI_VERSION,
};
// DDCP3-U4/U5: the typed `§ArtifactPacket` `HostDeviceCall` surface (typed
// bindings, bounded numeric argument layout, the execution-descriptor
// identity domain) + the typed completeness gate (gate bullets 5/6) + the
// gate-6 hostile-shape surface. Re-export hunks serialized after U2/U3's
// (the U4 residual; U5 adds the gate classes + the caller-route adapter).
pub use radix_mir::artifact_completeness::{
    check_preparation_completeness, derive_region_preparation_facts, ArtifactPacketFacts, CompletenessError,
    EntrypointSymbolFact, LaunchDimensionFact, PreparedResourceFact, ReflectedKernelFact, ReflectedResourceFact,
    RegionPreparationFacts,
};
pub use radix_mir::boundary_abi::{
    derive_host_device_calls, execution_descriptor_hash, CallBinding, CallBindingRole, CallFailureBehavior,
    ExecutionDescriptor, HostDeviceCall, MemberKernel, RegionArgumentLayout,
};
pub use radix_mir::dump_program;
// DDCP3-U3: the typed, serializable partition/callsite record (membership +
// callsite mapping + explicit effect boundaries) — the compiler artifact
// proving DDCP3 gate bullets 1 + 2. Re-export hunks serialized after U2's.
pub use radix_mir::host_partition::{
    derive_host_partition, derive_partition_record, CallsiteId, CallsiteRecord, EffectBoundaryRecord, HostPartition,
    HostPartitionError, PartitionMember, PartitionRecord, PartitionRole, RegionRecord,
};
pub use radix_mir::nodes::*;
pub use radix_mir::{
    ai_workbench_stage5_native_status, ai_workbench_stage5_rows, AiWorkbenchStage5Facet, AiWorkbenchStage5Row,
    AiWorkbenchStage5Status,
};
pub use radix_mir::{
    classify_device_safe, classify_device_safe_for_target_entry, device_roles_from_hir, MirDeviceContext, MirDeviceRole,
};
pub use radix_mir::{
    constant_ty, index_projection_result_ty, option_chain_base_ty, option_payload_ty, place_base_ty, MirTypeError,
    MirTypeLookup,
};
pub use radix_mir::{
    indexed_access_parity, TensorOperationFamily, TensorOperationParityRow, TensorParityStatus, TensorParityTarget,
};
pub use radix_mir::{
    inference_blocker_rows, inference_non_claims, InferenceBlockerKind, InferenceBlockerRow, InferenceBlockerStatus,
    InferenceNonClaim, InferenceNonClaimKind,
};
pub(crate) use radix_mir::{instantiate_existing_generic_type, type_contains_param, MirGenericBindings};
pub use radix_mir::{
    rank2_reduction_readiness_rows, Rank2ReductionReadinessPath, Rank2ReductionReadinessRow,
    Rank2ReductionReadinessStatus,
};
pub use radix_mir::{
    reject_kernel_matrix_abi, validate_varying_consistency, MirAbiError, MirFragmentOutputReflection, MirFunctionAbi,
    MirFunctionAbiParam, MirGpuReflection, MirKernelAxis, MirKernelBufferBindingKind, MirKernelBuiltin,
    MirKernelBuiltinInput, MirKernelBuiltinInputReflection, MirKernelDispatchSize, MirKernelLaunchBindGroup,
    MirKernelLaunchBindGroupLayoutEntry, MirKernelLaunchBinding, MirKernelLaunchPipelineBindGroupLayout,
    MirKernelLaunchPipelineLayout, MirKernelLaunchPlan, MirKernelOffsetMode, MirKernelProvenance, MirKernelReflection,
    MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceReflection,
    MirKernelResourceRole, MirKernelShaderAccess, MirKernelShaderStage, MirKernelSignature, MirKernelWorkgroupCount,
    MirRuntimeAbi, MirRuntimeAbiFamily, MirRuntimeAbiKind, MirVaryingInterpolation, MirVaryingReflection,
    MirVertexInputFormat, MirVertexInputReflection, MirWebGpuBindGroupDescriptor, MirWebGpuBindGroupEntryDescriptor,
    MirWebGpuBindGroupLayoutDescriptor, MirWebGpuBindGroupLayoutEntryDescriptor, MirWebGpuDispatchWorkgroups,
    MirWebGpuLaunchAdapter, MirWebGpuPipelineLayoutDescriptor, MirWebGpuVertexAttributeDescriptor,
    MirWebGpuVertexBufferLayoutDescriptor, MirWebGpuVertexStepMode, MirWorkgroupSize, ShaderStageIoRole,
};
pub use radix_mir::{
    required_tensor_operation_floor_families, tensor_operation_floor_rows, TensorOperationFloorFamily,
    TensorOperationFloorRow, TensorOperationFloorStatus, TensorOperationFloorTarget,
};
pub use radix_mir::{
    selected_tensor_systems_family, tensor_systems_target_rows, TensorSystemsTarget, TensorSystemsTargetLayout,
    TensorSystemsTargetRank, TensorSystemsTargetRow, TensorSystemsTargetStatus,
};
pub use radix_mir::{
    tensor_placement_operations, tensor_placement_policy_rows, TensorPlacementFacet, TensorPlacementOperation,
    TensorPlacementPolicyRow, TensorPlacementStatus,
};
pub use radix_mir::{
    tensor_runtime_abi_rows, TensorRuntimeAbiFacet, TensorRuntimeAbiRow, TensorRuntimeAbiStatus, TensorRuntimeAbiTarget,
};
pub use radix_mir::{
    validate_program, MirEnumRepresentation, MirFunctionSignature, MirParamSignature, MirScalarValueDomain,
    MirValidationContext, MirValidationError, ValidatedMir, INTEGER_PAYLOAD_I64_SENTINEL, INTEGER_PAYLOAD_U64_SENTINEL,
    INTEGER_PAYLOAD_U8_SENTINEL,
};
pub use radix_mir::{CapabilityGap, Lowerability};
pub use radix_mir::{
    MirAggregateCarrier, MirAggregateFamily, MirAggregateLayout, MirAtomicLayout, MirDeviceMatrixLayout,
    MirDeviceVectorLayout, MirDeviceViewLayout, MirLayout, MirLayoutError, MirLayoutKind, MirLayoutTable,
    MirPackedBitOrder, MirPackedNumericFormat, MirPackedNumericLayout, MirScalarLayout, MirTensorLocusLayout,
    MirTensorStorageLayout, MirTensorStrideLayout, MirTensorViewLayout,
};
#[cfg(feature = "runner")]
pub use runner::{
    classify_runner_lowerability, run_analyzed, run_entry, run_function, run_function_with_step_budget, run_source,
    runner_dispatch_status, BufferHost, Host, RunSourceError, RunnerDispatchStatus, RunnerError, StdioHost, Value,
};
#[cfg(feature = "mir-sexp")]
pub use sexp_text::{classify_sexp_lowerability, emit_sexp_text, MirSexpTextProbeError};
#[cfg(feature = "mir-wasm")]
pub use wasm_text::{emit_wasm_text_probe, emit_wasm_text_probe_package_aware, MirWasmTextProbeError};
#[cfg(feature = "mir-wgsl")]
pub use wgsl_text::{
    emit_chain_descriptor, emit_wgsl_fragment_entry_contract, emit_wgsl_fragment_entry_contract_with_targets,
    emit_wgsl_text_probe_with_device_roles, emit_wgsl_text_probe_with_reflection_and_device_roles,
    emit_wgsl_vertex_entry_contract, BindGroupEntry, InterKernelBufferIdentity, KernelChainDescriptor,
    KernelDescriptor, MirWgslTextProbeError, MirWgslTextProbeOutput, StorageBufferDecl,
};

pub use radix_mir::HANDLER_MESSAGE_FIELD;
pub use radix_mir::{
    ascii_literal_identity, external_function_import_name, octeti_literal_identity, regex_flags_literal_identity,
    regex_literal_flags_suffix, regex_pattern_literal_identity, text_literal_identity, MirNames,
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
#[path = "capability_test.rs"]
mod capability_tests;
#[cfg(test)]
#[path = "commune_stepper_test.rs"]
mod commune_stepper_tests;
#[cfg(test)]
#[path = "device_test.rs"]
mod device_tests;
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
#[path = "nodes_test.rs"]
mod nodes_tests;
#[cfg(test)]
#[path = "potentia_test.rs"]
mod potentia_tests;
#[cfg(test)]
#[path = "numeric_unary_test.rs"]
mod numeric_unary_tests;
#[cfg(test)]
#[path = "rank2_reduction_readiness_test.rs"]
mod rank2_reduction_readiness_tests;
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
#[path = "ty_test.rs"]
mod ty_tests;
#[cfg(test)]
#[path = "unsigned_integer_test.rs"]
mod unsigned_integer_tests;

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
