//! Middle IR model for execution-shaped compiler analysis (leaf crate).
//!
//! Package `radix` re-exports this crate and owns HIR→MIR lowering plus emit backends.

#![allow(clippy::struct_excessive_bools)] // structural: ABI structs model hardware flags

pub mod abi;
pub mod ai_workbench_stage5;
pub mod capability;
pub mod control;
pub mod device;
pub mod dump;
pub mod generic;
pub mod gradient;
pub mod inference_blocker_matrix;
pub mod kernel_module;
pub mod layout;
pub mod names;
pub mod nodes;
pub mod placement;
pub mod rank2_reduction_readiness;
pub mod tensor_operation_floor;
pub mod tensor_operation_parity;
pub mod tensor_placement;
pub mod tensor_runtime_abi;
pub mod tensor_systems_target;
pub mod ty;
pub mod validate;
pub mod visit;

mod figura;
#[allow(dead_code)]
mod tensor_bracket;

/// Shared lista→tensor shape helpers used by validation and the MIR stepper.
pub mod lista_tensor_conversio;
/// Literal matrix shape resolution for MIR validation and the stepper.
pub mod matrix_shape;
/// Literal vector width resolution for MIR layout and the stepper.
pub mod vector_width;

pub use abi::{
    collection_op_workgroup_count, reject_kernel_matrix_abi, validate_varying_consistency,
    MirAbiError, MirColorTargetFormat, MirDepthCompareFunction, MirDepthStencilState,
    MirFragmentOutputReflection, MirFunctionAbi, MirFunctionAbiParam, MirGpuReflection,
    MirGraphicsPipelineReflection, MirInputStepMode, MirKernelAxis, MirKernelBufferBindingType,
    MirKernelBuiltin, MirKernelBuiltinInput, MirKernelBuiltinInputReflection,
    MirKernelBuiltinUsage, MirKernelDispatchSize, MirKernelLaunchBindGroup, MirKernelLaunchBindGroupLayoutEntry,
    MirKernelLaunchBinding, MirKernelLaunchPipelineBindGroupLayout, MirKernelLaunchPipelineLayout,
    MirKernelLaunchPlan, MirKernelOffsetMode, MirKernelProvenance, MirKernelReflection,
    MirKernelResource, MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceReflection,
    MirKernelResourceRole, MirKernelShaderAccess, MirKernelShaderStage, MirKernelSignature,
    MirKernelWorkgroupCount, MirPrimitiveTopology, MirRuntimeAbi, MirRuntimeAbiCategory,
    MirRuntimeAbiFamily, MirStencilFaceState, MirStencilOp, MirTrigaFragmentOutputFact,
    MirTrigaPipelineFact, MirTrigaResourceBindingFact, MirTrigaVaryingFact,
    MirTrigaVertexLayoutFact, MirVaryingInterpolation, MirVaryingReflection, MirVertexInputFormat,
    MirVertexInputReflection, MirWebGpuBindGroupDescriptor, MirWebGpuBindGroupEntryDescriptor,
    MirWebGpuBindGroupLayoutDescriptor, MirWebGpuBindGroupLayoutEntryDescriptor,
    MirWebGpuDispatchWorkgroups, MirWebGpuLaunchAdapter, MirWebGpuPipelineLayoutDescriptor,
    MirWebGpuVertexAttributeDescriptor, MirWebGpuVertexBufferLayoutDescriptor,
    MirWebGpuVertexStepMode, MirWorkgroupSize,
};
pub use ai_workbench_stage5::{
    ai_workbench_stage5_native_status, ai_workbench_stage5_rows, AiWorkbenchStage5Facet,
    AiWorkbenchStage5Row, AiWorkbenchStage5Status,
};
pub use capability::{CapabilityGap, Lowerability};
pub use device::{
    classify_device_safe, classify_device_safe_for_target_entry, device_roles_from_hir,
    MirDeviceContext, MirDeviceRole,
};
pub use dump::dump_program;
pub use generic::{
    bind_generic_type_witness, instantiate_existing_generic_type, type_contains_param,
    MirGenericBindings,
};
pub use inference_blocker_matrix::{
    inference_blocker_rows, inference_non_claims, InferenceBlockerCategory, InferenceBlockerRow,
    InferenceBlockerStatus, InferenceNonClaim, InferenceNonClaimKind,
};
pub use kernel_module::KernelModule;
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
pub use rank2_reduction_readiness::{
    rank2_reduction_readiness_rows, Rank2ReductionReadinessPath, Rank2ReductionReadinessRow,
    Rank2ReductionReadinessStatus,
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
