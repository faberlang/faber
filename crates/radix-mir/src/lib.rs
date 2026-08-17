//! Middle IR model for execution-shaped compiler analysis (leaf crate).
//!
//! Package `radix` re-exports this crate and owns HIR→MIR lowering plus emit backends.

#![allow(clippy::struct_excessive_bools)] // structural: ABI structs model hardware flags

pub mod abi;
pub mod ai_workbench_stage5;
pub mod artifact_completeness;
pub mod boundary_abi;
pub mod capability;
mod collection_ops;
pub mod control;
pub mod device;
pub mod device_program;
pub mod device_program_plans;
pub mod device_semantics;
pub mod distributed;
pub mod dump;
pub mod generic;
pub mod gradient;
pub mod host_partition;
pub mod inference_blocker_matrix;
pub mod kernel_decomposition;
pub mod kernel_module;
pub mod kernel_plan;
pub mod layout;
pub mod literal_indexed_access;
mod local_ty;
pub mod names;
pub mod nodes;
pub mod placement;
pub mod rank2_reduction_readiness;
pub mod shader_contract;
pub mod static_shape_fold;
pub mod tensor_operation_floor;
pub mod tensor_operation_parity;
pub mod tensor_placement;
pub mod tensor_runtime_abi;
pub mod tensor_systems_target;
mod tensor_type;
pub mod ty;
pub mod validate;
pub mod visit;

mod figura;
/// Shared statement/value read analysis for device-program lowering.
mod read_analysis;
#[allow(dead_code)]
mod tensor_bracket;

/// Shared lista→tensor shape helpers used by validation and the MIR runner.
pub mod lista_tensor_conversio;
/// Literal matrix shape resolution for MIR validation and the runner.
pub mod matrix_shape;
/// Literal vector width resolution for MIR layout and the runner.
pub mod vector_width;

pub use abi::{
    collection_op_workgroup_count, rank_extension_broadcast_fact, reject_kernel_matrix_abi,
    validate_varying_consistency, MirAbiError, MirBroadcastDeclaration, MirColorTargetFormat,
    MirDepthCompareFunction, MirDepthStencilState, MirFragmentOutputReflection, MirFunctionAbi,
    MirFunctionAbiParam, MirGpuReflection, MirGraphicsPipelineReflection, MirInputStepMode,
    MirKernelBufferBindingType, MirKernelBuiltinInput, MirKernelBuiltinInputReflection,
    MirKernelBuiltinUsage, MirKernelDispatchSize, MirKernelLaunchBindGroup,
    MirKernelLaunchBindGroupLayoutEntry, MirKernelLaunchBinding,
    MirKernelLaunchPipelineBindGroupLayout, MirKernelLaunchPipelineLayout, MirKernelLaunchPlan,
    MirKernelOffsetMode, MirKernelProvenance, MirKernelReflection, MirKernelResource,
    MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceReflection,
    MirKernelResourceRole, MirKernelShaderAccess, MirKernelShaderStage, MirKernelSignature,
    MirKernelWorkgroupCount, MirPrimitiveTopology, MirRankExtensionBroadcast, MirRuntimeAbi,
    MirRuntimeAbiCategory, MirRuntimeAbiFamily, MirStencilFaceState, MirStencilOp,
    MirVaryingInterpolation, MirVaryingReflection, MirVertexInputFormat, MirVertexInputReflection,
    MirWebGpuBindGroupDescriptor, MirWebGpuBindGroupEntryDescriptor,
    MirWebGpuBindGroupLayoutDescriptor, MirWebGpuBindGroupLayoutEntryDescriptor,
    MirWebGpuDispatchWorkgroups, MirWebGpuLaunchAdapter, MirWebGpuPipelineLayoutDescriptor,
    MirWebGpuVertexAttributeDescriptor, MirWebGpuVertexBufferLayoutDescriptor,
    MirWebGpuVertexStepMode, MirWorkgroupSize,
};
pub use ai_workbench_stage5::{
    ai_workbench_stage5_native_status, ai_workbench_stage5_rows, AiWorkbenchStage5Facet,
    AiWorkbenchStage5Row, AiWorkbenchStage5Status,
};
pub use boundary_abi::{
    validate_boundary_abi, BoundaryAbiClass, BoundaryAbiError, BoundaryCall, PreparedRegion,
    HOST_CALL_ABI_VERSION,
};
// Vocabulary moved down to radix-types (feature-granularity §2); re-exported
pub use device::{
    classify_device_safe, classify_device_safe_for_target_entry, device_roles_from_hir,
    MirDeviceContext, MirDeviceRole,
};
pub use device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRegistry, BufferRole, BufferVersion,
    DataFlowPair, DeviceProgram, DeviceProgramError, DeviceProgramLifetime, DeviceResource,
    KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit, ObservationCadence, RegistryBuffer,
    RegistryVersion, ResultBuffer,
};
pub use device_program_plans::{
    carried_plan_agrees, kernel_plan_for_function, launch_descriptors, plan_for_collection_op,
    subchain_signature_for_emission, KernelLaunchDescriptor, LaunchBufferSlot, PlanMappingError,
    RecipeKind,
};
pub use dump::dump_program;
pub use generic::{
    bind_generic_type_witness, fold_shape_size_constant, function_contains_shape_size,
    instantiate_existing_generic_type, rewrite_function_constants, substitute_shape_size_constants,
    type_contains_param, MirGenericBindings,
};
pub use host_partition::{derive_host_partition, HostPartition, HostPartitionError};
pub use inference_blocker_matrix::{
    inference_blocker_rows, inference_non_claims, InferenceBlockerCategory, InferenceBlockerRow,
    InferenceBlockerStatus, InferenceNonClaim, InferenceNonClaimKind,
};
pub use kernel_decomposition::{decompose_kernel_function, KernelDecomposition, KernelSubchain};
pub use kernel_plan::{
    BarrierPhase, BarrierPoint, CollectionKernelPlan, GroupedMatMulPlan, MatMulPlan,
    OobPaddingPolicy, PlanValidationError, QuantizedMatMulPlan, ReduceOp, ReductionPlan,
    SharedMemoryLayout, SsmConv1dPlan, SsmScanPlan, TransposePlan, TRANSPOSE_WORKGROUP_X,
};
pub use layout::{
    MirAggregateCarrier, MirAggregateFamily, MirAggregateLayout, MirAtomicLayout,
    MirDeviceMatrixLayout, MirDeviceVectorLayout, MirDeviceViewLayout, MirLayout, MirLayoutError,
    MirLayoutKind, MirLayoutTable, MirPackedBitOrder, MirPackedNumericFormat,
    MirPackedNumericLayout, MirScalarLayout, MirTensorLocusLayout, MirTensorStorageLayout,
    MirTensorStrideLayout, MirTensorViewLayout,
};
pub use names::{
    ascii_literal_identity, external_function_import_name, octeti_literal_identity,
    regex_flags_literal_identity, regex_literal_flags_suffix, regex_pattern_literal_identity,
    text_literal_identity, MirNames,
};
pub use nodes::*;
pub use radix_types::{
    CapabilityGap, KernelModule, Lowerability, MirAtomicOp, MirCollectionOp, MirKernelAxis,
    MirKernelBuiltin,
};
pub use rank2_reduction_readiness::{
    rank2_reduction_readiness_rows, Rank2ReductionReadinessPath, Rank2ReductionReadinessRow,
    Rank2ReductionReadinessStatus,
};
pub use shader_contract::{
    color_target_format_from_code, depth_compare_function_from_code,
    fragment_entry_from_shader_contract, fragment_output_reflection_from_shader_contract,
    graphics_pipeline_from_shader_contract, input_step_mode_from_code,
    primitive_topology_from_code, resource_access_from_code, resource_kind_from_code,
    resource_reflection_from_shader_contract, resource_role_from_code,
    source_fragment_entry_from_shader_contract, source_vertex_entry_from_shader_contract,
    stage_io_role_from_code, varying_interpolation_from_code,
    varying_reflection_from_shader_contract, vertex_entry_from_shader_contract,
    vertex_format_from_code, vertex_input_reflection_from_shader_contract, ContractProvenance,
    ShaderFragmentOutput, ShaderPipelineState, ShaderResourceBinding, ShaderStageIoRole,
    ShaderVarying, ShaderVertexAttribute,
};
pub use static_shape_fold::{
    admit_device_shape_materialization, fold_static_shapes, FoldedShapeFact, ShapeFactSource,
    ShapeFoldError, ShapeFoldErrorKind, ShapeFoldOutcome, ShapeFoldReport, ShapeFoldedAdmission,
};
pub use tensor_operation_floor::{
    required_tensor_operation_floor_families, tensor_operation_floor_rows,
    TensorOperationFloorFamily, TensorOperationFloorRow, TensorOperationFloorStatus,
    TensorOperationFloorTarget,
};
pub use tensor_operation_parity::{
    indexed_access_parity, TensorOperationFamily, TensorOperationParityRow, TensorParityStatus,
    TensorParityTarget,
};
pub use tensor_placement::{
    tensor_placement_operations, tensor_placement_policy_rows, TensorPlacementFacet,
    TensorPlacementOperation, TensorPlacementPolicyRow, TensorPlacementStatus,
};
pub use tensor_runtime_abi::{
    tensor_runtime_abi_rows, TensorRuntimeAbiFacet, TensorRuntimeAbiRow, TensorRuntimeAbiStatus,
    TensorRuntimeAbiTarget,
};
pub use tensor_systems_target::{
    selected_tensor_systems_family, tensor_systems_target_rows, TensorSystemsTarget,
    TensorSystemsTargetLayout, TensorSystemsTargetRank, TensorSystemsTargetRow,
    TensorSystemsTargetStatus,
};
pub use ty::{
    constant_ty, index_projection_result_ty, option_chain_base_ty, option_payload_ty,
    place_base_ty, MirTypeError, MirTypeLookup,
};
pub use validate::{
    validate_program, MirEnumRepresentation, MirFunctionSignature, MirParamSignature,
    MirScalarValueDomain, MirValidationContext, MirValidationError, ValidatedMir,
    INTEGER_PAYLOAD_I64_SENTINEL, INTEGER_PAYLOAD_U64_SENTINEL, INTEGER_PAYLOAD_U8_SENTINEL,
};
pub use visit::{
    terminator_successors, walk_operand, walk_place, walk_stmt, walk_terminator, walk_value,
};

/// Field name for string-backed fac/cape handler error message projection.
pub const HANDLER_MESSAGE_FIELD: &str = "nuntius";
