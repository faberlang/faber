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
pub mod cost_analysis;
pub mod device;
pub mod device_copy_into;
pub mod device_plan_compiler;
pub mod device_plan_ingress;
pub mod device_program;
pub mod device_program_plans;
pub mod device_semantics;
pub mod display_tokens;
pub mod distributed;
pub mod dump;
pub mod elementwise_plan;
pub mod generic;
pub mod gradient;
pub mod host_partition;
pub mod inference_blocker_matrix;
pub mod kernel_decomposition;
pub mod kernel_module;
pub mod kernel_plan;
mod kernel_shift_view;
pub mod lane_op;
pub mod layout;
pub mod literal_indexed_access;
mod local_ty;
pub mod names;
pub mod nodes;
pub mod placement;
pub mod rank2_reduction_readiness;
pub mod reduce_fusion;
pub mod result_use;
pub mod shader_contract;
pub mod static_shape_fold;
pub mod store_trap;
pub mod struct_layout;
pub mod structurize;
pub mod tensor_expansion;
pub mod tensor_operation_floor;
pub mod tensor_operation_parity;
pub mod tensor_placement;
pub mod tensor_runtime_abi;
pub mod tensor_systems_target;
mod tensor_type;
mod text;
pub mod ty;
pub mod validate;
pub mod vector_expand;
pub mod visit;
pub mod wrapping_store;

#[cfg(test)]
mod control_test;
#[cfg(test)]
mod device_copy_into_test;
mod figura;
#[cfg(test)]
mod function_index_test;
#[cfg(test)]
mod inf_test;
#[cfg(test)]
mod lane_op_test;
#[cfg(test)]
mod layout_test;
#[cfg(test)]
mod packed_negative_matrix_test;
#[cfg(test)]
mod param_mode_test;
/// Shared statement/value read analysis for device-program lowering.
mod read_analysis;
#[cfg(test)]
mod struct_layout_test;
#[cfg(test)]
mod structurize_test;
#[cfg(test)]
mod vector_expand_test;
#[cfg(test)]
mod vector_op_test;
#[cfg(test)]
mod wrapping_store_test;

/// Shared lista→tensor shape helpers used by validation and the MIR runner.
pub mod lista_tensor_conversio;
/// Literal matrix shape resolution for MIR validation and the runner.
pub mod matrix_shape;
/// Literal vector width resolution for MIR layout and the runner.
pub mod vector_width;

pub use abi::{
    INDEX_FAULT_TEXT, IndexGuard, LaneIndexProof, MirAbiError, MirBoundResourceReflection,
    MirBroadcastDeclaration, MirColorTargetFormat, MirDepthCompareFunction, MirDepthStencilState,
    MirFaultKind, MirFaultSite, MirFragmentOutputReflection, MirFunctionAbi, MirFunctionAbiParam,
    MirGpuReflection, MirGraphicsPipelineReflection, MirInputStepMode, MirKernelBufferBindingKind,
    MirKernelBuiltinInput, MirKernelBuiltinInputReflection, MirKernelBuiltinUsage,
    MirKernelDispatchSize, MirKernelLaunchBindGroup, MirKernelLaunchBindGroupLayoutEntry,
    MirKernelLaunchBinding, MirKernelLaunchPipelineBindGroupLayout, MirKernelLaunchPipelineLayout,
    MirKernelLaunchPlan, MirKernelOffsetMode, MirKernelProvenance, MirKernelReflection,
    MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceReflection,
    MirKernelResourceRole, MirKernelShaderAccess, MirKernelShaderStage, MirKernelSignature,
    MirKernelWorkgroupCount, MirPrimitiveTopology, MirRankExtensionBroadcast, MirRuntimeAbi,
    MirRuntimeAbiFamily, MirRuntimeAbiKind, MirSlabOriginBinding, MirStencilFaceState,
    MirStencilOp, MirVaryingInterpolation, MirVaryingReflection, MirVertexInputFormat,
    MirVertexInputReflection, MirWebGpuBindGroupDescriptor, MirWebGpuBindGroupEntryDescriptor,
    MirWebGpuBindGroupLayoutDescriptor, MirWebGpuBindGroupLayoutEntryDescriptor,
    MirWebGpuDispatchWorkgroups, MirWebGpuLaunchAdapter, MirWebGpuPipelineLayoutDescriptor,
    MirWebGpuVertexAttributeDescriptor, MirWebGpuVertexBufferLayoutDescriptor,
    MirWebGpuVertexStepMode, MirWorkgroupSize, ParameterAliases, ThrowSite, UnprovenIndex,
    UnprovenVectorIndex, collection_op_workgroup_count, d64_fault_site, grid_reduce_helper_tensors,
    is_d64_type, rank_extension_broadcast_fact, reject_kernel_matrix_abi, throw_sites,
    unproven_indexes, unproven_vector_indexes, validate_varying_consistency,
};
pub use ai_workbench_stage5::{
    AiWorkbenchStage5Facet, AiWorkbenchStage5Row, AiWorkbenchStage5Status,
    ai_workbench_stage5_native_status, ai_workbench_stage5_rows,
};
pub use boundary_abi::{
    BoundaryAbiError, BoundaryAbiKind, BoundaryCall, HOST_CALL_ABI_VERSION, PreparedRegion,
    validate_boundary_abi,
};
pub use cost_analysis::{
    COST_BASIS, CostBasisRow, CostClass, CostFamily, CostFunctionRollup, CostSite, CostSiteKind,
    CostTag, amplify_class, basis_tag, classify_conversion, conversion_family, cost_rollups,
    cost_sites, foldable_tag, uncosted_tag,
};
pub use display_tokens::{DisplayTokensId, MirDisplayTokens};
// Vocabulary moved down to radix-types (feature-granularity §2); re-exported
pub use device::{
    FragmentSurvivor, FragmentSurvivorKind, MirDeviceContext, MirDeviceRole,
    NUCLEUM_FRAGMENT_SURVIVED, classify_device_safe, classify_device_safe_for_target_entry,
    device_entry_roles_from_hir, device_roles_from_hir, first_surviving_fragment_call,
    reject_nucleum_fragment_survivors, surviving_nucleum_fragments,
};
pub use device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRegistry, BufferRole, BufferVersion,
    DataFlowPair, DeviceProgram, DeviceProgramError, DeviceProgramLifetime, DeviceResource,
    KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit, ObservationCadence, RegistryBuffer,
    RegistryVersion, ResultBuffer,
};
pub use device_program_plans::{
    BackendRequirement, BindDescriptor, BindLayout, DECODE_GEMV_SELECTION_ENABLED,
    DECODE_GEMV_UNIFORM, ExecutorPlanExtension, KernelLaunchDescriptor, LaunchBufferSlot,
    LibraryBind, LibrarySelection, LogicalSignature, OffsetBinding, PACKED_BLOCK_BYTES_UNIFORM,
    PACKED_BLOCK_ELEMENTS_UNIFORM, PACKED_BYTES_UNIFORM, PACKED_LAYOUT_GGML_TYPE_ID_UNIFORM,
    PlanMappingError, PlanProvenance, QUANTIZED_GEMM_ENTRY, QUANTIZED_GEMV_ENTRY, RecipeKind,
    UniformBinding, UniformType, WorkspaceBinding, carried_plan_agrees, executor_plan_extension,
    kernel_plan_for_function, launch_descriptors, plan_for_collection_op,
    subchain_signature_for_emission,
};
pub use dump::dump_program;
pub use elementwise_plan::{
    ConcreteTensorShape, ElementwiseBarrierError, ElementwiseExpr, ElementwiseInput, ElementwiseOp,
    ElementwiseOutput, ElementwisePlan, IndexMapping, NumericalPolicy, ReassociationPolicy,
    SourceSpanFact, elementwise_plan_summary, plan_elementwise,
};
pub use generic::{
    MirGenericBindings, bind_generic_type_witness, fold_shape_size_constant,
    function_contains_shape_size, instantiate_existing_generic_type, rewrite_function_constants,
    substitute_shape_size_constants, type_contains_param,
};
pub use host_partition::{
    HostPartition, HostPartitionError, derive_host_partition,
    derive_partition_record_with_metadata, kernel_entry_name, unique_kernel_entry_name,
};
pub use inference_blocker_matrix::{
    InferenceBlockerKind, InferenceBlockerRow, InferenceBlockerStatus, InferenceNonClaim,
    InferenceNonClaimKind, inference_blocker_rows, inference_non_claims,
};
pub use kernel_decomposition::{
    KernelDecomposition, KernelSubchain, decompose_kernel_function,
    decompose_kernel_function_with_limit,
};
pub use kernel_plan::{
    BarrierPhase, BarrierPoint, CollectionKernelPlan, GroupedMatMulPlan, MatMulClass, MatMulDims,
    MatMulPlan, MatMulShapes, OobPaddingPolicy, PackedAdmissionError, PackedDequantError,
    PackedDequantKind, PackedDequantResolution, PlanValidationError, QuantizedGatherPlan,
    QuantizedMatMulPlan, ROW_REDUCTION_WORKGROUP_X, ReduceOp, ReductionPlan, RowReductionFacts,
    SIMDGROUP_MATMUL_MAX_N, SIMDGROUP_MATMUL_PARTIAL_SLOTS, SIMDGROUP_MATMUL_STAGING_SLOTS,
    SharedMemoryLayout, SimdgroupMatMulFacts, SsmConv1dPlan, SsmScanPlan, TRANSPOSE_WORKGROUP_X,
    TransposePlan, admit_quantized_gather, admit_quantized_matmul,
    causal_softmax_row_reduction_admission, matmul_class_for_extents,
    rmsnorm_row_reduction_admission, simdgroup_matmul_admission,
};
pub use layout::{
    MirAggregateCarrier, MirAggregateFamily, MirAggregateLayout, MirAtomicLayout,
    MirDeviceMatrixLayout, MirDeviceVectorLayout, MirDeviceViewElement, MirDeviceViewLayout,
    MirLayout, MirLayoutError, MirLayoutKind, MirLayoutTable, MirPackedBitOrder,
    MirPackedNumericFormat, MirPackedNumericLayout, MirScalarLayout, MirTensorLocusLayout,
    MirTensorStorageLayout, MirTensorStrideLayout, MirTensorViewLayout,
};
pub use names::{
    MirNames, ascii_literal_identity, external_function_import_name, octeti_literal_identity,
    regex_flags_literal_identity, regex_literal_flags_suffix, regex_pattern_literal_identity,
    text_literal_identity,
};
pub use nodes::*;
pub use radix_types::{
    CapabilityGap, KernelModule, Lowerability, MirAtomicOp, MirCollectionOp, MirKernelAxis,
    MirKernelBuiltin,
};
pub use rank2_reduction_readiness::{
    Rank2ReductionReadinessPath, Rank2ReductionReadinessRow, Rank2ReductionReadinessStatus,
    rank2_reduction_readiness_rows,
};
pub use shader_contract::{
    ContractProvenance, ShaderBoundResourceBinding, ShaderFragmentOutput, ShaderPipelineState,
    ShaderResourceBinding, ShaderStageIoRole, ShaderVarying, ShaderVertexAttribute,
    bound_resource_reflection_from_shader_contract, color_target_format_from_code,
    depth_compare_function_from_code, fragment_entry_from_shader_contract,
    fragment_output_reflection_from_shader_contract, graphics_pipeline_from_shader_contract,
    input_step_mode_from_code, primitive_topology_from_code, resource_access_from_code,
    resource_kind_from_code, resource_reflection_from_shader_contract, resource_role_from_code,
    source_fragment_entry_from_shader_contract, source_vertex_entry_from_shader_contract,
    stage_io_role_from_code, varying_interpolation_from_code,
    varying_reflection_from_shader_contract, vertex_entry_from_shader_contract,
    vertex_format_from_code, vertex_input_reflection_from_shader_contract,
};
pub use static_shape_fold::{
    FoldedShapeFact, ShapeFactSource, ShapeFoldError, ShapeFoldKind, ShapeFoldOutcome,
    ShapeFoldReport, ShapeFoldedAdmission, admit_device_shape_materialization, fold_static_shapes,
    fold_validated_device_program,
};
pub use struct_layout::{
    MAX_STRUCT_LAYOUT_BYTES, MirStructField, MirStructLayout, StructLayoutRefusal,
    resolve_struct_layout,
};
pub use tensor_operation_floor::{
    TensorOperationFloorFamily, TensorOperationFloorRow, TensorOperationFloorStatus,
    TensorOperationFloorTarget, required_tensor_operation_floor_families,
    tensor_operation_floor_rows,
};
pub use tensor_operation_parity::{
    TensorOperationFamily, TensorOperationParityRow, TensorParityStatus, TensorParityTarget,
    indexed_access_parity,
};
pub use tensor_placement::{
    TensorPlacementFacet, TensorPlacementOperation, TensorPlacementPolicyRow,
    TensorPlacementStatus, tensor_placement_operations, tensor_placement_policy_rows,
};
pub use tensor_runtime_abi::{
    TensorRuntimeAbiFacet, TensorRuntimeAbiRow, TensorRuntimeAbiStatus, TensorRuntimeAbiTarget,
    tensor_runtime_abi_rows,
};
pub use tensor_systems_target::{
    TensorSystemsTarget, TensorSystemsTargetLayout, TensorSystemsTargetRank,
    TensorSystemsTargetRow, TensorSystemsTargetStatus, selected_tensor_systems_family,
    tensor_systems_target_rows,
};
pub use ty::{
    MirTypeError, MirTypeLookup, constant_ty, index_projection_result_ty, option_chain_base_ty,
    option_payload_ty, place_base_ty,
};
pub use validate::{
    DEFERRED_VECTOR_OP_SURVIVED, DetachedMir, INTEGER_PAYLOAD_I64_SENTINEL,
    INTEGER_PAYLOAD_U8_SENTINEL, INTEGER_PAYLOAD_U64_SENTINEL, MirEnumRepresentation,
    MirFunctionSignature, MirInterner, MirParamSignature, MirScalarValueDomain, MirTypeTable,
    MirValidationContext, MirValidationError, ValidatedMir, validate_program,
    validate_program_with_device_roles,
};
pub use visit::{
    terminator_successors, walk_operand, walk_place, walk_stmt, walk_terminator, walk_value,
};
pub use wrapping_store::{
    WrappingStoreRegion, WrappingStoreSite, wrapping_reduce_width, wrapping_store_regions,
};

/// Field name for string-backed fac/cape handler error message projection.
pub const HANDLER_MESSAGE_FIELD: &str = "nuntius";
