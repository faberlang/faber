//! Shared ABI metadata for MIR-backed systems targets.
//!
//! TARGET: low-level emitters that need callable, runtime-helper, or kernel
//! resource facts before choosing target syntax. This module records the shape
//! and purpose of ABI surfaces; target-specific spelling still belongs to the
//! target emitter.

use crate::{
    shader_contract::ShaderStageIoRole, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic,
    MirLayoutKind, MirLocalId, MirParam, MirRuntimeCall, MirScalarLayout, MirStatementKind,
    MirType, MirValidationContext,
};
use radix_hir::DefId;
use radix_lexer::Interner;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const ERR_KERNEL_PARAM_RETURN_MISMATCH: &str = "kernel parameter and return types must match";
const ERR_INDEXED_VIEW_RETURNS_VACUUM: &str = "indexed view kernel must return vacuum";
const ERR_INDEXED_VIEW_REQUIRES_INPUT: &str = "indexed view kernel requires at least one input";
const ERR_KERNEL_BUILTIN_UNKNOWN_NAME: &str =
    "kernel builtin parameter must use a known builtin name";
const ERR_KERNEL_BUILTIN_DUPLICATE_NAME: &str =
    "kernel builtin parameter must not repeat a builtin name";
const ERR_KERNEL_WORKGROUP_NONZERO: &str = "kernel workgroup size must be nonzero";
const ERR_KERNEL_REFLECTION_SCALAR_ELEMENT: &str =
    "kernel reflection resource element must be scalar";
const ERR_KERNEL_BUILTIN_SCALAR_INPUT: &str = "kernel builtin input must be scalar";
// The return-buffer ABI is elementwise-only: every input and the return must
// agree on element type AND element count. The count arm is what a matmul
// author hits (input/return shapes differ), so the message names the equality
// law and points to the indexed-view ABI that carries the matmul contract.
const ERR_KERNEL_IO_ELEMENT_MISMATCH: &str =
    "kernel input and output resource element types and counts must match; the return-buffer ABI is elementwise-only — matmul and reduction kernels must use the indexed-view ABI";
const ERR_KERNEL_INDEX_U32: &str = "kernel index parameter must be u32";
const ERR_KERNEL_DEVICE_VIEW_ELEMENT: &str = "kernel device view element type";
const ERR_KERNEL_VECTOR_PARAM: &str = "kernel vector param";
/// Stable shape for rejected matrix kernel params/returns (Unit B ratchet).
const ERR_KERNEL_MATRIX_PARAM: &str = "kernel matrix param";
const ERR_KERNEL_AGGREGATE_LAYOUT: &str = "kernel aggregate layout";
const ERR_KERNEL_OPAQUE_LAYOUT: &str = "kernel opaque layout";
/// S3-A1: the result-tuple ABI requires a non-empty tuple return of device
/// views (the multi-output contract).
const ERR_KERNEL_RESULT_TUPLE_REQUIRED: &str =
    "result-tuple kernel requires a non-empty tuple return of device-view tensors";
/// S3-A1: the result-tuple ABI requires at least one input param.
const ERR_KERNEL_RESULT_TUPLE_REQUIRES_INPUT: &str =
    "result-tuple kernel requires at least one input parameter";
const ERR_KERNEL_TENSOR_STORAGE_LOCUS: &str = "kernel tensor storage/locus requires device handle";
const ERR_KERNEL_VOID_SCALAR: &str = "void kernel scalar";
const ERR_KERNEL_NEVER_SCALAR: &str = "never kernel scalar";
const ERR_INDEXED_KERNEL_DEVICE_VIEW: &str = "indexed kernel resource must be device view";
const ERR_KERNEL_RESOURCE_BINDING_OVERFLOW: &str = "kernel resource binding index exceeds u32";
const ERR_KERNEL_VECTOR_ELEMENT_COUNT: &str = "kernel vector element count";
const ERR_KERNEL_DEVICE_VIEW_ELEMENT_COUNT: &str = "kernel device view element count";
const ERR_KERNEL_REFLECTION_BYTE_OVERFLOW: &str =
    "kernel reflection resource byte length overflows u64";
const ERR_VERTEX_INPUT_DUPLICATE_LOCATION: &str = "vertex input locations must be unique";
const ERR_VERTEX_INPUT_UNSUPPORTED_FORMAT_CODE: &str = "unsupported Triga vertex input format code";
const ERR_VARYING_DUPLICATE_LOCATION: &str = "varying locations must be unique";
const ERR_FRAGMENT_OUTPUT_DUPLICATE_LOCATION: &str = "fragment output locations must be unique";
const ERR_VARYING_FRAGMENT_WITHOUT_VERTEX: &str = "fragment varying has no matching vertex output";
const ERR_VARYING_STAGE_MISMATCH: &str =
    "vertex and fragment varyings disagree on format or interpolation";
const ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX: &str =
    "graphics pipeline reflection requires vertex-stage reflection";
const ERR_GRAPHICS_PIPELINE_REQUIRES_FRAGMENT: &str =
    "graphics pipeline reflection requires fragment-stage reflection";
const ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX_INPUTS: &str =
    "graphics pipeline reflection requires vertex inputs";
const ERR_GRAPHICS_PIPELINE_VARYING_MISMATCH: &str =
    "graphics pipeline reflection requires compatible vertex/fragment varyings";
const ERR_GRAPHICS_PIPELINE_REQUIRES_COLOR_TARGETS: &str =
    "graphics pipeline reflection requires at least one color target";

fn kernel_atomic_param_error(element: MirScalarLayout) -> String {
    format!(
        "kernel atomic parameters are not supported for {}",
        element.spelling()
    )
}

fn kernel_atomic_element_count_error(element: MirScalarLayout) -> String {
    format!(
        "kernel atomic element counts are not supported for {}",
        element.spelling()
    )
}

fn kernel_atomic_reflection_error(element: MirScalarLayout) -> String {
    format!(
        "kernel atomic reflection resources are not supported for {}",
        element.spelling()
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirAbiError {
    pub message: String,
    pub issue: &'static str,
    pub detail: Option<String>,
}

impl MirAbiError {
    fn new(message: impl Into<String>) -> Self {
        let message = message.into();
        let issue = abi_issue(&message);
        Self {
            message,
            issue,
            detail: None,
        }
    }

    pub(crate) fn with_detail(
        message: impl Into<String>,
        issue: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            message: message.into(),
            issue,
            detail: Some(detail.into()),
        }
    }
}

fn abi_issue(message: &str) -> &'static str {
    if message == ERR_KERNEL_BUILTIN_DUPLICATE_NAME {
        "mir_kernel_builtin_duplicate_name"
    } else if message == ERR_KERNEL_WORKGROUP_NONZERO {
        "mir_kernel_workgroup_nonzero"
    } else if message == ERR_KERNEL_TENSOR_STORAGE_LOCUS {
        "mir_kernel_tensor_storage_locus_unsupported"
    } else if message == ERR_KERNEL_REFLECTION_BYTE_OVERFLOW {
        "mir_kernel_reflection_byte_length_overflow"
    } else if message == ERR_INDEXED_KERNEL_DEVICE_VIEW {
        "mir_indexed_kernel_resource_not_device_view"
    } else if message == ERR_KERNEL_RESOURCE_BINDING_OVERFLOW {
        "mir_kernel_resource_binding_overflow"
    } else if message == ERR_KERNEL_RESULT_TUPLE_REQUIRED {
        "mir_kernel_result_tuple_required"
    } else if message == ERR_KERNEL_RESULT_TUPLE_REQUIRES_INPUT {
        "mir_kernel_result_tuple_requires_input"
    } else if message.starts_with("kernel atomic parameters are not supported for ") {
        "mir_kernel_atomic_parameters_unsupported"
    } else if message.starts_with("kernel atomic reflection resources are not supported for ") {
        "mir_kernel_atomic_reflection_resources_unsupported"
    } else if message == ERR_KERNEL_MATRIX_PARAM {
        "mir_kernel_matrix_param_unsupported"
    } else if message == ERR_VERTEX_INPUT_DUPLICATE_LOCATION {
        "mir_vertex_input_duplicate_location"
    } else if message == ERR_VERTEX_INPUT_UNSUPPORTED_FORMAT_CODE {
        "mir_vertex_input_unsupported_format_code"
    } else if message == ERR_VARYING_DUPLICATE_LOCATION {
        "mir_varying_duplicate_location"
    } else if message == ERR_FRAGMENT_OUTPUT_DUPLICATE_LOCATION {
        "mir_fragment_output_duplicate_location"
    } else if message == ERR_VARYING_FRAGMENT_WITHOUT_VERTEX {
        "mir_varying_fragment_without_vertex"
    } else if message == ERR_VARYING_STAGE_MISMATCH {
        "mir_varying_stage_mismatch"
    } else if message == ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX {
        "mir_graphics_pipeline_requires_vertex"
    } else if message == ERR_GRAPHICS_PIPELINE_REQUIRES_FRAGMENT {
        "mir_graphics_pipeline_requires_fragment"
    } else if message == ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX_INPUTS {
        "mir_graphics_pipeline_requires_vertex_inputs"
    } else if message == ERR_GRAPHICS_PIPELINE_REQUIRES_COLOR_TARGETS {
        "mir_graphics_pipeline_requires_color_targets"
    } else if message == ERR_GRAPHICS_PIPELINE_VARYING_MISMATCH {
        "mir_graphics_pipeline_varying_mismatch"
    } else {
        "mir_abi_error"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirFunctionAbi {
    pub function: MirFunctionId,
    pub params: Vec<MirFunctionAbiParam>,
    pub return_ty: MirType,
    pub error_ty: Option<MirType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirFunctionAbiParam {
    pub local: MirLocalId,
    pub ty: MirType,
}

impl MirFunctionAbi {
    #[must_use]
    pub fn from_function(function: &MirFunction) -> Self {
        Self {
            function: function.id,
            params: function
                .params
                .iter()
                .map(|param| MirFunctionAbiParam {
                    local: param.local,
                    ty: param.ty,
                })
                .collect(),
            return_ty: function.return_ty,
            error_ty: function.error_ty,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirRuntimeAbiCategory {
    CoreSemantics,
    HostIntegration,
    ProbeScaffolding,
    IntentionalOpaqueRuntime,
}

impl MirRuntimeAbiCategory {
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            Self::CoreSemantics => "core-semantics",
            Self::HostIntegration => "host-integration",
            Self::ProbeScaffolding => "probe-scaffolding",
            Self::IntentionalOpaqueRuntime => "intentional-opaque-runtime",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirRuntimeAbiFamily {
    RuntimeDiagnostic,
    RuntimeAssert,
    RuntimeFormat,
    RuntimeConvert,
    RuntimeCollection,
    RuntimeAtomic,
    RuntimePanic,
    RuntimeProvider,
    RuntimeSermo,
    RuntimeReadLine,
    RuntimeCede,
    RuntimeGpuBuiltin,
    RuntimeCursorStream,
    RuntimeGradient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirRuntimeAbi {
    pub family: MirRuntimeAbiFamily,
    pub category: MirRuntimeAbiCategory,
    pub returns_void: bool,
}

impl MirRuntimeAbi {
    #[must_use]
    pub fn from_call(call: &MirRuntimeCall) -> Self {
        Self::from_intrinsic(&call.intrinsic)
    }

    #[must_use]
    pub fn from_intrinsic(intrinsic: &MirIntrinsic) -> Self {
        let (family, category, returns_void) = match intrinsic {
            MirIntrinsic::Diagnostic(_) => (
                MirRuntimeAbiFamily::RuntimeDiagnostic,
                MirRuntimeAbiCategory::HostIntegration,
                true,
            ),
            MirIntrinsic::Assert => (
                MirRuntimeAbiFamily::RuntimeAssert,
                MirRuntimeAbiCategory::CoreSemantics,
                true,
            ),
            MirIntrinsic::FormatString { .. } => (
                MirRuntimeAbiFamily::RuntimeFormat,
                MirRuntimeAbiCategory::CoreSemantics,
                false,
            ),
            MirIntrinsic::Convert(_) => (
                MirRuntimeAbiFamily::RuntimeConvert,
                MirRuntimeAbiCategory::CoreSemantics,
                false,
            ),
            MirIntrinsic::Collection(_) => (
                MirRuntimeAbiFamily::RuntimeCollection,
                MirRuntimeAbiCategory::CoreSemantics,
                false,
            ),
            MirIntrinsic::Atomic(op) => (
                MirRuntimeAbiFamily::RuntimeAtomic,
                MirRuntimeAbiCategory::CoreSemantics,
                matches!(op, crate::MirAtomicOp::Store),
            ),
            MirIntrinsic::Panic => (
                MirRuntimeAbiFamily::RuntimePanic,
                MirRuntimeAbiCategory::CoreSemantics,
                true,
            ),
            MirIntrinsic::Provider(_) => (
                MirRuntimeAbiFamily::RuntimeProvider,
                MirRuntimeAbiCategory::HostIntegration,
                false,
            ),
            MirIntrinsic::SermoOpen | MirIntrinsic::SermoSetOpener | MirIntrinsic::Sermo(_) => (
                MirRuntimeAbiFamily::RuntimeSermo,
                MirRuntimeAbiCategory::HostIntegration,
                false,
            ),
            MirIntrinsic::ReadLine => (
                MirRuntimeAbiFamily::RuntimeReadLine,
                MirRuntimeAbiCategory::HostIntegration,
                false,
            ),
            MirIntrinsic::Cede => (
                MirRuntimeAbiFamily::RuntimeCede,
                MirRuntimeAbiCategory::HostIntegration,
                false,
            ),
            MirIntrinsic::GpuBuiltin(_) => (
                MirRuntimeAbiFamily::RuntimeGpuBuiltin,
                MirRuntimeAbiCategory::CoreSemantics,
                false,
            ),
            MirIntrinsic::CursorStream(_) => (
                MirRuntimeAbiFamily::RuntimeCursorStream,
                MirRuntimeAbiCategory::HostIntegration,
                false,
            ),
            MirIntrinsic::Gradient(_) => (
                MirRuntimeAbiFamily::RuntimeGradient,
                MirRuntimeAbiCategory::HostIntegration,
                false,
            ),
        };
        Self {
            family,
            category,
            returns_void,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirKernelSignature {
    pub function: MirFunctionId,
    pub input: MirKernelResource,
    pub extra_inputs: Vec<MirKernelResource>,
    pub output: MirKernelResource,
    /// S3-A1: additional output resources of a **result-tuple** kernel
    /// contract (the tuple elements after the first), in tuple order.
    /// `output` is the first tuple element; every output binds to a DISTINCT
    /// slot, so a backward companion's N gradient outputs each get their own
    /// output binding. Empty for single-output kernels.
    pub extra_outputs: Vec<MirKernelResource>,
    /// S3-A1: accumulation-buffer resources — read-write device state a
    /// kernel accumulates into across the grid, bound after the outputs.
    /// Populated by the companion-aware materializer (S3-A2) when partial
    /// gradients accumulate; empty for kernels without accumulation.
    pub accumulation_buffers: Vec<MirKernelResource>,
    pub builtin_inputs: Vec<MirKernelBuiltinInput>,
    pub called_builtins: Vec<MirKernelBuiltin>,
    pub workgroup: MirWorkgroupSize,
    pub dispatch_size: MirKernelDispatchSize,
}

/// Classified builtin usage for a kernel signature — which invocation-header
/// parameters each backend must declare. This is shared classification data
/// only (L2); backend-local text mapping stays in each emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MirKernelBuiltinUsage {
    /// Any global-invocation builtin is referenced. An empty builtin list
    /// still declares the grid position (the default header), so this is true
    /// for an empty list.
    pub uses_global_invocation: bool,
    /// A workgroup-id builtin is referenced.
    pub uses_workgroup_id: bool,
    /// A local-invocation-id builtin is referenced.
    pub uses_local_invocation_id: bool,
    /// A workgroup-size builtin is referenced. Metal declares a
    /// `threads_per_threadgroup` param for it; WGSL emits workgroup-size
    /// constants inline and ignores this flag.
    pub uses_workgroup_size: bool,
}

impl MirKernelBuiltinUsage {
    /// Classify an explicit builtin list. The WGSL emitter classifies its
    /// post-`additional_header_builtins()`-augmented list; the signature
    /// method classifies `header_builtins()`.
    #[must_use]
    pub fn from_builtins(builtins: &[MirKernelBuiltin]) -> Self {
        let mut usage = MirKernelBuiltinUsage {
            uses_global_invocation: builtins.is_empty(),
            uses_workgroup_id: false,
            uses_local_invocation_id: false,
            uses_workgroup_size: false,
        };
        for builtin in builtins {
            match builtin {
                MirKernelBuiltin::GlobalInvocationX
                | MirKernelBuiltin::GlobalInvocationY
                | MirKernelBuiltin::GlobalInvocationZ
                | MirKernelBuiltin::GlobalInvocationId => usage.uses_global_invocation = true,
                MirKernelBuiltin::WorkgroupIdX
                | MirKernelBuiltin::WorkgroupIdY
                | MirKernelBuiltin::WorkgroupIdZ
                | MirKernelBuiltin::WorkgroupId => usage.uses_workgroup_id = true,
                MirKernelBuiltin::LocalInvocationIdX
                | MirKernelBuiltin::LocalInvocationIdY
                | MirKernelBuiltin::LocalInvocationIdZ
                | MirKernelBuiltin::LocalInvocationId => usage.uses_local_invocation_id = true,
                MirKernelBuiltin::WorkgroupSizeX
                | MirKernelBuiltin::WorkgroupSizeY
                | MirKernelBuiltin::WorkgroupSizeZ => usage.uses_workgroup_size = true,
            }
        }
        usage
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MirGpuReflection {
    pub kernels: Vec<MirKernelReflection>,
    pub pipeline: Option<MirGraphicsPipelineReflection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MirKernelReflection {
    pub function: MirFunctionId,
    pub provenance: MirKernelProvenance,
    pub entry_name: String,
    pub shader_stage: MirKernelShaderStage,
    pub resources: Vec<MirKernelResourceReflection>,
    pub vertex_inputs: Vec<MirVertexInputReflection>,
    pub varyings: Vec<MirVaryingReflection>,
    pub fragment_outputs: Vec<MirFragmentOutputReflection>,
    pub builtin_inputs: Vec<MirKernelBuiltinInputReflection>,
    pub called_builtins: Vec<MirKernelBuiltin>,
    pub workgroup: MirWorkgroupSize,
    pub dispatch_size: MirKernelDispatchSize,
    pub workgroup_count: MirKernelWorkgroupCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirKernelProvenance {
    MirFunction(MirFunctionId),
    SourceFunction(DefId),
}

/// Vertex input step mode: per-vertex or per-instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirInputStepMode {
    Vertex,
    Instance,
}

impl MirInputStepMode {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Vertex => "vertex",
            Self::Instance => "instance",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MirVertexInputReflection {
    pub source_name: Option<String>,
    pub location: u32,
    pub format: MirVertexInputFormat,
    pub step_mode: MirInputStepMode,
    pub offset_bytes: u64,
    pub stride_bytes: u64,
    /// Explicit stage-I/O role (see [`crate::shader_contract::ShaderStageIoRole`]).
    pub role: ShaderStageIoRole,
}

/// Vertex-output / fragment-input contract. A varying is a value that flows from
/// the vertex stage to the fragment stage; its location, format, and
/// interpolation must agree across both stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MirVaryingReflection {
    pub source_name: String,
    pub location: u32,
    pub format: MirVertexInputFormat,
    pub interpolation: MirVaryingInterpolation,
    /// Explicit stage-I/O role (see [`crate::shader_contract::ShaderStageIoRole`]).
    pub role: ShaderStageIoRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirVaryingInterpolation {
    Perspective,
    Linear,
    Flat,
}

/// Fragment render-target output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MirFragmentOutputReflection {
    pub location: u32,
    pub format: MirVertexInputFormat,
    /// Explicit stage-I/O role (see [`crate::shader_contract::ShaderStageIoRole`]).
    pub role: ShaderStageIoRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirKernelLaunchPlan {
    pub entry_name: String,
    pub shader_stage: MirKernelShaderStage,
    pub pipeline_layout: MirKernelLaunchPipelineLayout,
    pub bind_group_count: usize,
    pub bind_groups: Vec<MirKernelLaunchBindGroup>,
    pub vertex_input_count: usize,
    pub vertex_inputs: Vec<MirVertexInputReflection>,
    pub required_builtins: Vec<MirKernelBuiltin>,
    pub builtin_inputs: Vec<MirKernelBuiltinInputReflection>,
    pub called_builtins: Vec<MirKernelBuiltin>,
    pub workgroup: MirWorkgroupSize,
    pub dispatch_size: MirKernelDispatchSize,
    pub workgroup_count: MirKernelWorkgroupCount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirKernelLaunchPipelineLayout {
    pub bind_group_layout_count: usize,
    pub bind_group_layouts: Vec<MirKernelLaunchPipelineBindGroupLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirKernelLaunchPipelineBindGroupLayout {
    pub bind_group_index: usize,
    pub group: u32,
    pub layout_entry_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct MirKernelLaunchBindGroup {
    pub bind_group_index: usize,
    pub group: u32,
    pub resource_indexes: Vec<usize>,
    pub resource_index_count: usize,
    pub resource_kinds: Vec<MirKernelResourceKind>,
    pub resource_kind_count: usize,
    pub resource_roles: Vec<MirKernelResourceRole>,
    pub resource_role_count: usize,
    pub shader_accesses: Vec<MirKernelShaderAccess>,
    pub shader_access_count: usize,
    pub buffer_binding_types: Vec<MirKernelBufferBindingType>,
    pub buffer_binding_type_count: usize,
    pub buffer_byte_offsets: Vec<u64>,
    pub buffer_byte_offset_count: usize,
    pub buffer_byte_lens: Vec<u64>,
    pub buffer_byte_len_count: usize,
    pub binding_byte_lens: Vec<u64>,
    pub binding_byte_len_count: usize,
    pub min_binding_sizes: Vec<u64>,
    pub min_binding_size_count: usize,
    pub has_dynamic_offsets: Vec<bool>,
    pub dynamic_offset_flag_count: usize,
    pub offset_modes: Vec<MirKernelOffsetMode>,
    pub offset_mode_entry_count: usize,
    pub requires_dynamic_offsets: bool,
    pub has_non_dynamic_offsets: bool,
    pub has_mixed_offset_modes: bool,
    pub offset_mode_count: usize,
    pub has_only_non_dynamic_offsets: bool,
    pub has_only_dynamic_offsets: bool,
    pub non_dynamic_offset_count: usize,
    pub non_dynamic_offset_resource_indexes: Vec<usize>,
    pub non_dynamic_offset_resource_index_count: usize,
    pub non_dynamic_offset_binding_indexes: Vec<u32>,
    pub non_dynamic_offset_binding_index_count: usize,
    pub non_dynamic_offset_layout_entry_indexes: Vec<usize>,
    pub non_dynamic_offset_layout_entry_index_count: usize,
    pub dynamic_offset_count: usize,
    pub dynamic_offset_resource_indexes: Vec<usize>,
    pub dynamic_offset_resource_index_count: usize,
    pub dynamic_offset_binding_indexes: Vec<u32>,
    pub dynamic_offset_binding_index_count: usize,
    pub dynamic_offset_layout_entry_indexes: Vec<usize>,
    pub dynamic_offset_layout_entry_index_count: usize,
    pub binding_indexes: Vec<u32>,
    pub binding_index_count: usize,
    pub layout_entry_indexes: Vec<usize>,
    pub layout_entry_index_count: usize,
    pub source_locals: Vec<MirLocalId>,
    pub source_local_count: usize,
    pub source_names: Vec<String>,
    pub source_name_count: usize,
    pub binding_count: usize,
    pub layout_entry_count: usize,
    pub layout_entries: Vec<MirKernelLaunchBindGroupLayoutEntry>,
    pub bindings: Vec<MirKernelLaunchBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirKernelLaunchBindGroupLayoutEntry {
    pub layout_entry_index: usize,
    pub binding_index: usize,
    pub resource_index: usize,
    pub binding: u32,
    pub buffer_byte_len: u64,
    pub buffer_byte_offset: u64,
    pub binding_byte_len: u64,
    pub shader_visibility: MirKernelShaderStage,
    pub buffer_binding_type: MirKernelBufferBindingType,
    pub min_binding_size: u64,
    pub has_dynamic_offset: bool,
    pub source_local: Option<MirLocalId>,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirKernelOffsetMode {
    Fixed,
    Dynamic,
}

impl MirKernelOffsetMode {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Dynamic => "dynamic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirKernelShaderStage {
    Compute,
    Vertex,
    Fragment,
}

impl MirKernelShaderStage {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Compute => "compute",
            Self::Vertex => "vertex",
            Self::Fragment => "fragment",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirVertexInputFormat {
    Float32x2,
    Float32x3,
    Float32x4,
}

impl MirVertexInputFormat {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Float32x2 => "float32x2",
            Self::Float32x3 => "float32x3",
            Self::Float32x4 => "float32x4",
        }
    }
}

/// Color attachment texture format for a graphics pipeline target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirColorTargetFormat {
    Rgba8Unorm,
    Bgra8Unorm,
    Bgra8UnormSrgb,
}

impl MirColorTargetFormat {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Rgba8Unorm => "rgba8unorm",
            Self::Bgra8Unorm => "bgra8unorm",
            Self::Bgra8UnormSrgb => "bgra8unorm-srgb",
        }
    }
}

/// Primitive topology for draw calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirPrimitiveTopology {
    TriangleList,
    LineList,
    PointList,
}

impl MirPrimitiveTopology {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::TriangleList => "triangle-list",
            Self::LineList => "line-list",
            Self::PointList => "point-list",
        }
    }
}

/// Depth comparison function for depth testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirDepthCompareFunction {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

impl MirDepthCompareFunction {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Less => "less",
            Self::Equal => "equal",
            Self::LessEqual => "less-equal",
            Self::Greater => "greater",
            Self::NotEqual => "not-equal",
            Self::GreaterEqual => "greater-equal",
            Self::Always => "always",
        }
    }
}

/// Stencil operation applied when the stencil test fails or passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirStencilOp {
    Keep,
    Zero,
    Replace,
    IncrementClamp,
    DecrementClamp,
    IncrementWrap,
    DecrementWrap,
    Invert,
}

impl MirStencilOp {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Zero => "zero",
            Self::Replace => "replace",
            Self::IncrementClamp => "increment-clamp",
            Self::DecrementClamp => "decrement-clamp",
            Self::IncrementWrap => "increment-wrap",
            Self::DecrementWrap => "decrement-wrap",
            Self::Invert => "invert",
        }
    }
}

/// Per-face stencil state (front or back).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MirStencilFaceState {
    pub compare: MirDepthCompareFunction,
    pub fail_op: MirStencilOp,
    pub depth_fail_op: MirStencilOp,
    pub pass_op: MirStencilOp,
}

impl MirStencilFaceState {
    /// Disabled stencil state: always passes, no modification.
    pub const DISABLED: Self = Self {
        compare: MirDepthCompareFunction::Always,
        fail_op: MirStencilOp::Keep,
        depth_fail_op: MirStencilOp::Keep,
        pass_op: MirStencilOp::Keep,
    };
}

/// Depth/stencil state for a graphics pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MirDepthStencilState {
    pub depth_write_enabled: bool,
    pub depth_compare: MirDepthCompareFunction,
    pub stencil_read_mask: u32,
    pub stencil_write_mask: u32,
    pub stencil_front: MirStencilFaceState,
    pub stencil_back: MirStencilFaceState,
}

/// Reflection describing what a host needs to build a render pipeline and
/// issue a draw call from a vertex + fragment stage pair. This is the
/// Stage 4 draw-prerequisite seam consumed by Goal 05 (host rendering).
///
/// Resource bindings (uniforms, storage buffers, textures, samplers) are
/// carried in the per-stage resource vectors. The count fields remain as
/// convenience accessors derived from the vector lengths.
///
/// Produced by [`MirKernelReflection::graphics_pipeline_reflection`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MirGraphicsPipelineReflection {
    pub color_target_formats: Vec<MirColorTargetFormat>,
    pub primitive_topology: MirPrimitiveTopology,
    pub vertex_count: u32,
    pub vertex_input_count: usize,
    pub varying_count: usize,
    pub vertex_resources: Vec<MirKernelResourceReflection>,
    pub vertex_resource_count: usize,
    pub fragment_resources: Vec<MirKernelResourceReflection>,
    pub fragment_resource_count: usize,
    pub depth_stencil: Option<MirDepthStencilState>,
}

impl MirGraphicsPipelineReflection {
    /// Set depth/stencil state with stencil disabled (Always compare, Keep
    /// ops, full read/write masks). The host uses this to configure the depth
    /// attachment in the render pipeline descriptor. Use `.with_stencil()` to
    /// enable stencil testing separately.
    #[must_use = "builder: chain with `.with_stencil()` or use the returned `MirGraphicsPipelineReflection`"]
    pub fn with_depth_stencil(
        mut self,
        depth_compare: MirDepthCompareFunction,
        depth_write_enabled: bool,
    ) -> Self {
        self.depth_stencil = Some(MirDepthStencilState {
            depth_write_enabled,
            depth_compare,
            stencil_read_mask: u32::MAX,
            stencil_write_mask: u32::MAX,
            stencil_front: MirStencilFaceState::DISABLED,
            stencil_back: MirStencilFaceState::DISABLED,
        });
        self
    }

    /// Enable stencil testing on an existing depth/stencil state. Sets
    /// per-face compare functions and operations, plus read/write masks.
    ///
    /// # Errors
    ///
    /// Returns `mir_stencil_without_depth_stencil` when
    /// `.with_depth_stencil()` has not been called first.
    pub fn with_stencil(
        mut self,
        read_mask: u32,
        write_mask: u32,
        front: MirStencilFaceState,
        back: MirStencilFaceState,
    ) -> Result<Self, MirAbiError> {
        let ds = self.depth_stencil.as_mut().ok_or_else(|| {
            MirAbiError::with_detail(
                "with_stencil requires with_depth_stencil to be called first",
                "mir_stencil_without_depth_stencil",
                "with_stencil was called on a pipeline reflection with no depth/stencil state",
            )
        })?;
        ds.stencil_read_mask = read_mask;
        ds.stencil_write_mask = write_mask;
        ds.stencil_front = front;
        ds.stencil_back = back;
        Ok(self)
    }

    /// Override the primitive topology. Defaults to `TriangleList`. The host
    /// selects this when assembling the primitive-state member of the render
    /// pipeline descriptor.
    #[must_use = "builder: chain with other pipeline builder methods or use the returned `MirGraphicsPipelineReflection`"]
    pub fn with_topology(mut self, topology: MirPrimitiveTopology) -> Self {
        self.primitive_topology = topology;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirKernelLaunchBinding {
    pub binding_index: usize,
    pub resource_index: usize,
    pub binding: u32,
    pub kind: MirKernelResourceKind,
    pub role: MirKernelResourceRole,
    pub access: MirKernelResourceAccess,
    pub shader_access: MirKernelShaderAccess,
    pub shader_visibility: MirKernelShaderStage,
    pub buffer_binding_type: MirKernelBufferBindingType,
    pub element_ty: MirType,
    pub element_layout: MirScalarLayout,
    pub element_byte_width: u16,
    pub element_count: u64,
    pub buffer_byte_len: u64,
    pub buffer_byte_offset: u64,
    pub binding_byte_len: u64,
    pub min_binding_size: u64,
    pub has_dynamic_offset: bool,
    pub source_local: Option<MirLocalId>,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirWebGpuLaunchAdapter {
    pub pipeline_layout_descriptor: MirWebGpuPipelineLayoutDescriptor,
    pub bind_group_layout_descriptor_count: usize,
    pub bind_group_layout_descriptor_indexes: Vec<usize>,
    pub bind_group_layout_descriptor_index_count: usize,
    pub bind_group_layout_descriptors: Vec<MirWebGpuBindGroupLayoutDescriptor>,
    pub bind_group_descriptor_count: usize,
    pub bind_group_descriptor_indexes: Vec<usize>,
    pub bind_group_descriptor_index_count: usize,
    pub bind_group_descriptors: Vec<MirWebGpuBindGroupDescriptor>,
    pub vertex_buffer_layout_descriptor_count: usize,
    pub vertex_buffer_layout_descriptor_indexes: Vec<usize>,
    pub vertex_buffer_layout_descriptor_index_count: usize,
    pub vertex_buffer_layout_descriptors: Vec<MirWebGpuVertexBufferLayoutDescriptor>,
    pub dispatch_workgroup_dimension_count: usize,
    pub dispatch_workgroups: MirWebGpuDispatchWorkgroups,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirWebGpuPipelineLayoutDescriptor {
    pub bind_group_layout_count: usize,
    pub bind_group_layout_indexes: Vec<usize>,
    pub bind_group_layout_index_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirWebGpuBindGroupLayoutDescriptor {
    pub bind_group_index: usize,
    pub group: u32,
    pub layout_entry_indexes: Vec<usize>,
    pub layout_entry_index_count: usize,
    pub entry_count: usize,
    pub entries: Vec<MirWebGpuBindGroupLayoutEntryDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirWebGpuBindGroupLayoutEntryDescriptor {
    pub binding: u32,
    pub binding_index: usize,
    pub buffer_byte_len: u64,
    pub buffer_byte_offset: u64,
    pub binding_byte_len: u64,
    pub visibility: MirKernelShaderStage,
    pub buffer_type: MirKernelBufferBindingType,
    pub has_dynamic_offset: bool,
    pub min_binding_size: u64,
    pub resource_index: usize,
    pub layout_entry_index: usize,
    pub source_local: Option<MirLocalId>,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirWebGpuBindGroupDescriptor {
    pub bind_group_index: usize,
    pub group: u32,
    pub entry_indexes: Vec<usize>,
    pub entry_index_count: usize,
    pub entry_count: usize,
    pub entries: Vec<MirWebGpuBindGroupEntryDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirWebGpuBindGroupEntryDescriptor {
    pub binding: u32,
    pub kind: MirKernelResourceKind,
    pub role: MirKernelResourceRole,
    pub access: MirKernelResourceAccess,
    pub shader_access: MirKernelShaderAccess,
    pub shader_visibility: MirKernelShaderStage,
    pub element_ty: MirType,
    pub element_layout: MirScalarLayout,
    pub element_byte_width: u16,
    pub element_count: u64,
    pub resource_index: usize,
    pub binding_index: usize,
    pub buffer_type: MirKernelBufferBindingType,
    pub buffer_byte_len: u64,
    pub buffer_byte_offset: u64,
    pub binding_byte_len: u64,
    pub min_binding_size: u64,
    pub has_dynamic_offset: bool,
    pub source_local: Option<MirLocalId>,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirWebGpuVertexBufferLayoutDescriptor {
    pub buffer_index: usize,
    pub array_stride: u64,
    pub step_mode: MirWebGpuVertexStepMode,
    pub attribute_indexes: Vec<usize>,
    pub attribute_index_count: usize,
    pub attribute_count: usize,
    pub attributes: Vec<MirWebGpuVertexAttributeDescriptor>,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirWebGpuVertexAttributeDescriptor {
    pub attribute_index: usize,
    pub shader_location: u32,
    pub format: MirVertexInputFormat,
    pub offset: u64,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirWebGpuVertexStepMode {
    Vertex,
    Instance,
}

impl MirWebGpuVertexStepMode {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Vertex => "vertex",
            Self::Instance => "instance",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirWebGpuDispatchWorkgroups {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MirKernelResourceReflection {
    pub group: u32,
    pub binding: u32,
    pub kind: MirKernelResourceKind,
    pub role: MirKernelResourceRole,
    pub access: MirKernelResourceAccess,
    pub element_ty: MirType,
    pub element_layout: MirScalarLayout,
    pub element_byte_width: u16,
    pub element_count: u64,
    pub buffer_byte_len: u64,
    pub source_local: Option<MirLocalId>,
    pub source_name: Option<String>,
    /// Explicit stage-I/O role (see [`crate::shader_contract::ShaderStageIoRole`]).
    /// Carried so transform detection and other role-driven emission can key
    /// on the declared contract role instead of source-name spelling.
    pub stage_io_role: ShaderStageIoRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirKernelResource {
    pub group: u32,
    pub binding: u32,
    pub kind: MirKernelResourceKind,
    pub role: MirKernelResourceRole,
    pub access: MirKernelResourceAccess,
    pub element_ty: MirType,
    pub element_count: u64,
    pub source_local: Option<MirLocalId>,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirKernelBuiltinInput {
    pub local: MirLocalId,
    pub source_name: Option<String>,
    pub builtin: MirKernelBuiltin,
    pub ty: MirType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MirKernelBuiltinInputReflection {
    pub source_local: MirLocalId,
    pub source_name: Option<String>,
    pub builtin: MirKernelBuiltin,
    pub axis: MirKernelAxis,
    pub ty: MirType,
    pub layout: MirScalarLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirKernelBuiltin {
    GlobalInvocationX,
    GlobalInvocationY,
    GlobalInvocationZ,
    GlobalInvocationId,
    WorkgroupIdX,
    WorkgroupIdY,
    WorkgroupIdZ,
    WorkgroupId,
    LocalInvocationIdX,
    LocalInvocationIdY,
    LocalInvocationIdZ,
    LocalInvocationId,
    WorkgroupSizeX,
    WorkgroupSizeY,
    WorkgroupSizeZ,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirKernelAxis {
    X,
    Y,
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirKernelResourceKind {
    StorageBuffer,
    /// Runtime-extent count channel (U2, L3/L5).
    ///
    /// A host-supplied `u32` element count that device guards read instead of
    /// the static dispatch shape. The extent is an explicit
    /// [`MirKernelResource`] appended to `extra_inputs` — no parallel field
    /// (L3) — so its binding falls out of the positional allocator and
    /// `resources()` / `launch_plan()` / `emit_chain_descriptor` enumerate it
    /// for free (L4: chain-descriptor JSON shape unchanged).
    ///
    /// NVVM forward obligation (L7): future NVVM/PTX device guards must
    /// consume this channel (read the extent binding, with the shared
    /// `oob_guard_limit` static axis as fallback) — they must not reintroduce
    /// a static-shape guard.
    RuntimeExtent,
}

impl MirKernelBuiltin {
    /// Width of vector-valued builtins (`global_invocation_id`, `workgroup_id`, `local_invocation_id`).
    pub const VECTOR_WIDTH: u8 = 3;

    pub const ALL: [Self; 15] = [
        Self::GlobalInvocationX,
        Self::GlobalInvocationY,
        Self::GlobalInvocationZ,
        Self::GlobalInvocationId,
        Self::WorkgroupIdX,
        Self::WorkgroupIdY,
        Self::WorkgroupIdZ,
        Self::WorkgroupId,
        Self::LocalInvocationIdX,
        Self::LocalInvocationIdY,
        Self::LocalInvocationIdZ,
        Self::LocalInvocationId,
        Self::WorkgroupSizeX,
        Self::WorkgroupSizeY,
        Self::WorkgroupSizeZ,
    ];

    #[must_use]
    pub fn from_spelling(spelling: &str) -> Option<Self> {
        match spelling {
            "global_invocation_x" => Some(Self::GlobalInvocationX),
            "global_invocation_y" => Some(Self::GlobalInvocationY),
            "global_invocation_z" => Some(Self::GlobalInvocationZ),
            "global_invocation_id" => Some(Self::GlobalInvocationId),
            "workgroup_id_x" => Some(Self::WorkgroupIdX),
            "workgroup_id_y" => Some(Self::WorkgroupIdY),
            "workgroup_id_z" => Some(Self::WorkgroupIdZ),
            "workgroup_id" => Some(Self::WorkgroupId),
            "local_invocation_id_x" => Some(Self::LocalInvocationIdX),
            "local_invocation_id_y" => Some(Self::LocalInvocationIdY),
            "local_invocation_id_z" => Some(Self::LocalInvocationIdZ),
            "local_invocation_id" => Some(Self::LocalInvocationId),
            "workgroup_size_x" => Some(Self::WorkgroupSizeX),
            "workgroup_size_y" => Some(Self::WorkgroupSizeY),
            "workgroup_size_z" => Some(Self::WorkgroupSizeZ),
            _ => None,
        }
    }

    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::GlobalInvocationX => "global_invocation_x",
            Self::GlobalInvocationY => "global_invocation_y",
            Self::GlobalInvocationZ => "global_invocation_z",
            Self::GlobalInvocationId => "global_invocation_id",
            Self::WorkgroupIdX => "workgroup_id_x",
            Self::WorkgroupIdY => "workgroup_id_y",
            Self::WorkgroupIdZ => "workgroup_id_z",
            Self::WorkgroupId => "workgroup_id",
            Self::LocalInvocationIdX => "local_invocation_id_x",
            Self::LocalInvocationIdY => "local_invocation_id_y",
            Self::LocalInvocationIdZ => "local_invocation_id_z",
            Self::LocalInvocationId => "local_invocation_id",
            Self::WorkgroupSizeX => "workgroup_size_x",
            Self::WorkgroupSizeY => "workgroup_size_y",
            Self::WorkgroupSizeZ => "workgroup_size_z",
        }
    }

    #[must_use]
    pub fn returns_vector(self) -> bool {
        matches!(
            self,
            Self::GlobalInvocationId | Self::LocalInvocationId | Self::WorkgroupId
        )
    }

    #[must_use]
    pub fn axis(self) -> MirKernelAxis {
        match self {
            Self::GlobalInvocationX
            | Self::WorkgroupIdX
            | Self::LocalInvocationIdX
            | Self::WorkgroupSizeX => MirKernelAxis::X,
            Self::GlobalInvocationY
            | Self::WorkgroupIdY
            | Self::LocalInvocationIdY
            | Self::WorkgroupSizeY => MirKernelAxis::Y,
            Self::GlobalInvocationZ
            | Self::WorkgroupIdZ
            | Self::LocalInvocationIdZ
            | Self::WorkgroupSizeZ => MirKernelAxis::Z,
            Self::GlobalInvocationId | Self::WorkgroupId | Self::LocalInvocationId => {
                MirKernelAxis::X
            }
        }
    }
}

impl MirKernelAxis {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
        }
    }
}

impl MirKernelResourceKind {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::StorageBuffer => "storage-buffer",
            Self::RuntimeExtent => "runtime-extent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirKernelResourceRole {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirKernelResourceAccess {
    Read,
    Write,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirKernelShaderAccess {
    Read,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirKernelBufferBindingType {
    ReadOnlyStorage,
    Storage,
}

impl MirKernelShaderAccess {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::ReadWrite => "read_write",
        }
    }
}

impl MirKernelBufferBindingType {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::ReadOnlyStorage => "read-only-storage",
            Self::Storage => "storage",
        }
    }
}

const DEFAULT_KERNEL_RESOURCE_GROUP: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MirWorkgroupSize {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MirKernelDispatchSize {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MirKernelWorkgroupCount {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

struct IndexedViewKernelContext<'a> {
    validation: &'a MirValidationContext<'a>,
    builtin_inputs: Vec<MirKernelBuiltinInput>,
    interner: Option<&'a Interner>,
    allow_host_owned_tensor_entry: bool,
}

/// Well-known resource name for the runtime-extent count channel.
///
/// The host contract populates the extent binding with a `u32` element count
/// before dispatch (CTO #4 option b — well-known binding, `combineMetadata`
/// runtime-param precedent). L8/L9 dtype/stride headroom: the extent is
/// count-typed (`u32`) with no f32-width assumption; payload element types
/// stay per-resource.
const RUNTIME_EXTENT_RESOURCE_NAME: &str = "runtime_extent";

/// Build a runtime-extent count resource.
///
/// CXO F2: extents are `Vec`-capable from day one — one entry per extent
/// channel, appended to `extra_inputs`; Wave 1 populates at most one. The
/// binding is caller-assigned as the next free binding from the positional
/// allocator.
fn runtime_extent_resource(binding: u32, extent_ty: TypeId) -> MirKernelResource {
    MirKernelResource {
        group: DEFAULT_KERNEL_RESOURCE_GROUP,
        binding,
        kind: MirKernelResourceKind::RuntimeExtent,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty: MirType::semantic(extent_ty),
        element_count: 1,
        source_local: None,
        source_name: Some(RUNTIME_EXTENT_RESOURCE_NAME.to_owned()),
    }
}

/// Append a runtime-extent count resource when the kernel is scalar-shaped.
///
/// "Where the runtime extent applies" (U2): a kernel whose static IO element
/// count is 1 has `dispatch_size = (1, 1, 1)`, so a guard over the static
/// count culls every host-dispatched thread past the first. The extent
/// channel fixes that: the host supplies the element count at dispatch and
/// the guard reads the binding. Kernels with statically-known multi-element
/// shapes keep the static limit (correct), and collection-op contracts
/// (reduction/matmul) stay static per the U2 non-goals.
///
/// The count type (`numerus<u32>`, L8/L9) must already exist in the frozen
/// type table; when it cannot be resolved the kernel keeps the static
/// dispatch-axis fallback (pre-U2 behavior) — the guard still fires, but at
/// the static `oob_guard_limit` instead of the host-supplied count.
/// PRODUCTION INVARIANT: the GPU builtin registry interns `numerus<u32>`
/// for every kernel program, so real kernels always carry the extent; only
/// hand-built unit tables may lack it (`abi_test` pins the fallback).
fn maybe_append_runtime_extent(
    function: &MirFunction,
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) {
    if kernel_has_collection_op(function, |_| true) {
        return;
    }
    if signature.input.element_count != 1 || signature.output.element_count != 1 {
        return;
    }
    let Some(extent_ty) = validation
        .types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
    else {
        return;
    };
    let binding = signature
        .resources()
        .map(|resource| resource.binding)
        .max()
        .unwrap_or(0)
        + 1;
    signature
        .extra_inputs
        .push(runtime_extent_resource(binding, extent_ty));
}

impl MirKernelSignature {
    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_names(function, validation, None, false)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_interner(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_names(function, validation, Some(interner), false)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_interner_for_target_entry(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_names(function, validation, Some(interner), true)
    }

    fn storage_buffer_kernel_with_names(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        // Unit B: stable shape before arity matching so matrix kernel IO does not
        // fall through to the generic "one buffer parameter" message.
        reject_kernel_matrix_abi(function, validation)?;

        // S3-A1: a result-tuple return (a non-empty tuple of device views) is
        // the multi-output contract — every param is an input and the tuple
        // elements are DISTINCT output resources. Checked before the
        // single-output arms, which would reject the tuple as an aggregate.
        if !function.params.is_empty() && result_tuple_count(function, validation).is_some() {
            return Self::multi_output_return_kernel(
                function,
                &function.params,
                validation,
                interner,
                allow_host_owned_tensor_entry,
            );
        }

        if let Some(interner) = interner {
            if let Some(signature) = Self::source_named_indexed_view_kernel(
                function,
                validation,
                interner,
                allow_host_owned_tensor_entry,
            )? {
                return Ok(signature);
            }
        }

        // S3-A5: a kernel whose body carries a Reduction contract and whose
        // return is a scalar (the mean/sum result) synthesizes the
        // indexed-view reduction signature WITHOUT explicit output/index
        // parameters: every tensor param becomes a read input and the output
        // is the per-workgroup partials buffer from the resolved contract.
        // The scalar return is the host-combined reduction result; the
        // emitter's reduction arm writes the per-workgroup partials (the
        // S3-B1 mul-mean forward `simple_loss(x, w) → f32` shape).
        if let Some(signature) = Self::reduction_scalar_return_kernel(
            function,
            validation,
            interner,
            allow_host_owned_tensor_entry,
        )? {
            return Ok(signature);
        }

        match function.params.as_slice() {
            [param] => Self::return_buffer_kernel(
                function,
                validation,
                param,
                interner,
                allow_host_owned_tensor_entry,
            ),
            inputs if !inputs.is_empty() && !is_vacuum(function.return_ty, validation) => {
                Self::multi_input_return_buffer_kernel(
                    function,
                    validation,
                    inputs,
                    interner,
                    allow_host_owned_tensor_entry,
                )
            }
            [input, output, index] => {
                let context = IndexedViewKernelContext {
                    validation,
                    builtin_inputs: vec![builtin_input_for_index_param(index, interner)],
                    interner,
                    allow_host_owned_tensor_entry,
                };
                Self::indexed_view_kernel(function, input, output, index, context)
            }
            [inputs @ .., output, index] if !inputs.is_empty() => {
                let context = IndexedViewKernelContext {
                    validation,
                    builtin_inputs: vec![builtin_input_for_index_param(index, interner)],
                    interner,
                    allow_host_owned_tensor_entry,
                };
                Self::multi_input_indexed_view_kernel(function, inputs, output, index, context)
            }
            _ => Err(MirAbiError::new(
                "kernel must have either one buffer parameter or input*/output/index parameters",
            )),
        }
    }

    fn source_named_indexed_view_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Option<Self>, MirAbiError> {
        let params = function.params.as_slice();
        if params.len() < 3 {
            return Ok(None);
        }

        let mut builtin_start = params.len();
        while builtin_start > 0
            && named_builtin_for_param(&params[builtin_start - 1], interner).is_some()
        {
            builtin_start -= 1;
        }
        if builtin_start == params.len() {
            return Ok(None);
        }
        if builtin_start < 2 {
            return Err(MirAbiError::new(
                "indexed view kernel requires input/output parameters before builtin parameters",
            ));
        }

        let inputs = &params[..builtin_start - 1];
        let output = &params[builtin_start - 1];
        let builtin_params = &params[builtin_start..];
        let builtin_inputs = builtin_inputs_for_named_params(builtin_params, validation, interner)?;
        if let [input] = inputs {
            let context = IndexedViewKernelContext {
                validation,
                builtin_inputs,
                interner: Some(interner),
                allow_host_owned_tensor_entry,
            };
            Self::indexed_view_kernel(function, input, output, &builtin_params[0], context)
                .map(Some)
        } else {
            let context = IndexedViewKernelContext {
                validation,
                builtin_inputs,
                interner: Some(interner),
                allow_host_owned_tensor_entry,
            };
            Self::multi_input_indexed_view_kernel(
                function,
                inputs,
                output,
                &builtin_params[0],
                context,
            )
            .map(Some)
        }
    }

    /// Synthesize the storage-buffer ABI for a **result-tuple** kernel
    /// (S3-A1): a function whose return type is a non-empty tuple of device
    /// views. Every param becomes a read input buffer; the tuple's elements
    /// become DISTINCT output resources — one per tuple element, in tuple
    /// order, each bound to its own slot (the first element is `output`, the
    /// rest `extra_outputs`).
    ///
    /// This is the device ABI for a backward companion's `(grad_x, grad_w)`
    /// tuple: N gradient outputs bind to N distinct output slots, and no
    /// kernel value carries an aggregate type — the Metal aggregate
    /// rejection is unreachable through this ABI because tuple outputs are
    /// separate resources, never an aggregate.
    fn multi_output_return_kernel(
        function: &MirFunction,
        inputs: &[MirParam],
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        let Type::Tuple(elements) = validation.types.get(function.return_ty.semantic_id()) else {
            return Err(MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRED));
        };
        if elements.is_empty() {
            return Err(MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRED));
        }

        // Every non-vacuum param becomes a read input; a vacuum "residual"
        // param (the reverse-AD nil placeholder) carries no device storage
        // and is skipped (S3-A2 companion shape: `backward(x, w, nil(),
        // upstream) → (grad_x, grad_w)`). Output bindings start after the
        // inputs, so each output binds a distinct slot.
        let mut input_resources = Vec::with_capacity(inputs.len());
        let mut next_binding = 0u32;
        for input in inputs {
            if is_vacuum(input.ty, validation) {
                continue;
            }
            let element_ty =
                buffer_element_type(input.ty, validation, allow_host_owned_tensor_entry)?;
            let element_count = buffer_element_count(input.ty, validation)?;
            input_resources.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: next_binding,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty,
                element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, interner),
            });
            next_binding += 1;
        }
        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();

        // Tuple elements → distinct output resources (first → `output`).
        let mut outputs = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            let element_ty = MirType::semantic(*element);
            let view_ty =
                device_view_element_type(element_ty, validation, allow_host_owned_tensor_entry)?;
            let view_count = device_view_element_count(element_ty, validation)?;
            outputs.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: next_binding
                    .checked_add(u32::try_from(index).unwrap_or(u32::MAX))
                    .ok_or_else(|| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: view_ty,
                element_count: view_count,
                source_local: None,
                source_name: None,
            });
        }
        let output = outputs.remove(0);
        let extra_outputs = outputs;
        // R6 (P1): the dispatch must cover EVERY output's extent — the union
        // (max) over the per-output extents, never the first output's count
        // alone. A first-output-only dispatch either overruns a smaller
        // buffer or leaves a larger one incomplete; each output's write is
        // guarded by its own extent under the union (kernel_plan).
        let dispatch_count = extra_outputs
            .iter()
            .fold(output.element_count, |union, extra| {
                union.max(extra.element_count)
            });

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output,
            extra_outputs,
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(dispatch_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        Ok(signature)
    }

    /// S3-A5: synthesize the indexed-view reduction signature for a kernel
    /// whose body carries a Reduction contract and whose return is a scalar
    /// (the mean/sum result — the S3-B1 forward shape
    /// `simple_loss(x, w) → f32` with `mean(mul(x, w))`).
    ///
    /// Every non-vacuum tensor param becomes a read input; the output is the
    /// per-workgroup partials buffer from the resolved contract (count =
    /// `partials`), and the workgroup/dispatch come from the contract. The
    /// emitter's reduction arm writes each workgroup's partial there; the
    /// scalar return is the host-combined reduction result. `None` when the
    /// body carries no Reduction contract or the return is not a scalar.
    fn reduction_scalar_return_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Option<Self>, MirAbiError> {
        let Some(contract) = collection_op_contract(function, validation)? else {
            return Ok(None);
        };
        let CollectionOpContract::Reduction {
            length,
            workgroup_x,
            partials,
        } = contract
        else {
            return Ok(None);
        };
        // The return must be a scalar (the reduction result) — tensor and
        // tuple returns have their own signature arms.
        if !matches!(
            validation.layouts.layout_for_type(function.return_ty),
            Ok(layout) if matches!(layout.kind, MirLayoutKind::Scalar(_))
        ) {
            return Ok(None);
        }

        let mut input_resources = Vec::with_capacity(function.params.len());
        let mut next_binding = 0u32;
        for input in &function.params {
            if is_vacuum(input.ty, validation) {
                continue;
            }
            let element_ty =
                device_view_element_type(input.ty, validation, allow_host_owned_tensor_entry)?;
            let element_count = device_view_element_count(input.ty, validation)?;
            input_resources.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: next_binding,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty,
                element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, interner),
            });
            next_binding += 1;
        }
        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();
        let reduction_element_ty = input.element_ty;

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: next_binding,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: reduction_element_ty,
                element_count: partials,
                source_local: None,
                source_name: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            // The reduction recipe's grid-stride loop starts from the GLOBAL
            // thread index (the header's first builtin selects the index
            // expression); the tree + partial write add `local_id.x`/
            // `workgroup_id.x` via the emitter's C5 builtins.
            called_builtins: {
                let mut builtins = vec![MirKernelBuiltin::GlobalInvocationX];
                builtins.extend(called_builtins_for_function(function));
                builtins
            },
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
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        Ok(Some(signature))
    }

    fn return_buffer_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        param: &MirParam,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        // The strict semantic-type match is the plain-elementwise law. A
        // collection-op kernel remaps the shape by contract (Transpose
        // `[M,N] → [N,M]`), so for those the flat element-count equality
        // below carries the binding shape and
        // `apply_collection_op_contract` supplies the dispatch facts.
        if !kernel_has_collection_op(function, |_| true)
            && !validation
                .types
                .equals(param.ty.semantic_id(), function.return_ty.semantic_id())
        {
            return Err(MirAbiError::new(ERR_KERNEL_PARAM_RETURN_MISMATCH));
        }

        let param_element_ty =
            buffer_element_type(param.ty, validation, allow_host_owned_tensor_entry)?;
        let return_element_ty = buffer_element_type(
            function.return_ty,
            validation,
            allow_host_owned_tensor_entry,
        )?;
        require_same_element_type(param_element_ty, return_element_ty, validation)?;
        let param_element_count = buffer_element_count(param.ty, validation)?;
        let return_element_count = buffer_element_count(function.return_ty, validation)?;
        // The return-buffer equality law (shared with the multi-input arm):
        // the input and output buffers are flat views of the same element
        // count, so a host can assume one elementwise shape across bindings.
        // A rectangular transpose's `[M,N]` and `[N,M]` shapes both hold the
        // flat `M·N` count, so it passes here; genuinely size-changing
        // kernels fail closed with the elementwise-only diagnostic.
        if param_element_count != return_element_count {
            return Err(MirAbiError::new(ERR_KERNEL_IO_ELEMENT_MISMATCH));
        }

        let mut signature = Self {
            function: function.id,
            input: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: 0,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: param_element_ty,
                element_count: param_element_count,
                source_local: Some(param.local),
                source_name: param_source_name(param, interner),
            },
            extra_inputs: Vec::new(),
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: 1,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: return_element_ty,
                element_count: return_element_count,
                source_local: None,
                source_name: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(return_element_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): collection-op kernels (Transpose, Reduction,
        // MatMul) carry their dispatch/workgroup/output-buffer contract even
        // through the single-input return-buffer arm (shared ABI law, all
        // compute targets). A rank-2 transpose `[M,N] → [N,M]` keeps the
        // flat `M·N` element counts but must advertise the transpose
        // workgroup/dispatch contract, not the generic 1D one.
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    /// Synthesize the storage-buffer ABI for a multi-input return-buffer
    /// kernel (CUDA C3): every param becomes a read input buffer and the
    /// tensor return becomes the write output buffer.
    ///
    /// Shared ABI law for all compute targets (WGSL/Metal auto-consume, LLVM
    /// device consumes). Equal element counts across every input and the
    /// return are enforced with the same equality law the single-input
    /// return-buffer path uses, so a host can assume one elementwise shape
    /// across bindings.
    fn multi_input_return_buffer_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        inputs: &[MirParam],
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        let return_element_ty = buffer_element_type(
            function.return_ty,
            validation,
            allow_host_owned_tensor_entry,
        )?;
        let return_element_count = buffer_element_count(function.return_ty, validation)?;

        let mut input_resources = Vec::with_capacity(inputs.len());
        for (binding, input) in inputs.iter().enumerate() {
            let input_element_ty =
                buffer_element_type(input.ty, validation, allow_host_owned_tensor_entry)?;
            require_same_element_type(input_element_ty, return_element_ty, validation)?;
            let input_element_count = buffer_element_count(input.ty, validation)?;
            if input_element_count != return_element_count {
                return Err(MirAbiError::new(ERR_KERNEL_IO_ELEMENT_MISMATCH));
            }
            input_resources.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(binding)
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: input_element_ty,
                element_count: input_element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, interner),
            });
        }

        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(inputs.len())
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: return_element_ty,
                element_count: return_element_count,
                source_local: None,
                source_name: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(return_element_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): collection-op kernels carry their contract
        // through this arm too — the dispatch/workgroup/output-buffer shape
        // is a property of the resolved collection op, not of the arity that
        // picked the ABI arm (shared ABI law, all compute targets).
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    fn indexed_view_kernel(
        function: &MirFunction,
        input: &MirParam,
        output: &MirParam,
        index: &MirParam,
        context: IndexedViewKernelContext<'_>,
    ) -> Result<Self, MirAbiError> {
        let validation = context.validation;
        if !is_vacuum(function.return_ty, validation) {
            return Err(MirAbiError::new(ERR_INDEXED_VIEW_RETURNS_VACUUM));
        }
        require_u32_index(index.ty, validation)?;

        let input_element_ty =
            device_view_element_type(input.ty, validation, context.allow_host_owned_tensor_entry)?;
        let output_element_ty =
            device_view_element_type(output.ty, validation, context.allow_host_owned_tensor_entry)?;
        require_same_element_type(input_element_ty, output_element_ty, validation)?;
        let input_element_count = device_view_element_count(input.ty, validation)?;
        let output_element_count = device_view_element_count(output.ty, validation)?;

        let mut signature = Self {
            function: function.id,
            input: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: 0,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: input_element_ty,
                element_count: input_element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, context.interner),
            },
            extra_inputs: Vec::new(),
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: 1,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: output_element_ty,
                element_count: output_element_count,
                source_local: Some(output.local),
                source_name: param_source_name(output, context.interner),
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: context.builtin_inputs,
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(output_element_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): reduction/matmul kernels carry a corrected
        // dispatch/workgroup/output-buffer contract (shared ABI law, all
        // compute targets).
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    fn multi_input_indexed_view_kernel(
        function: &MirFunction,
        inputs: &[MirParam],
        output: &MirParam,
        index: &MirParam,
        context: IndexedViewKernelContext<'_>,
    ) -> Result<Self, MirAbiError> {
        let validation = context.validation;
        if !is_vacuum(function.return_ty, validation) {
            return Err(MirAbiError::new(ERR_INDEXED_VIEW_RETURNS_VACUUM));
        }
        require_u32_index(index.ty, validation)?;

        let output_element_ty =
            device_view_element_type(output.ty, validation, context.allow_host_owned_tensor_entry)?;
        let output_element_count = device_view_element_count(output.ty, validation)?;
        let mut input_resources = Vec::new();
        for (binding, input) in inputs.iter().enumerate() {
            let input_element_ty = device_view_element_type(
                input.ty,
                validation,
                context.allow_host_owned_tensor_entry,
            )?;
            require_same_element_type(input_element_ty, output_element_ty, validation)?;
            let input_element_count = device_view_element_count(input.ty, validation)?;
            input_resources.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(binding)
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: input_element_ty,
                element_count: input_element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, context.interner),
            });
        }

        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(inputs.len())
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: output_element_ty,
                element_count: output_element_count,
                source_local: Some(output.local),
                source_name: param_source_name(output, context.interner),
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: context.builtin_inputs,
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(output_element_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): reduction/matmul kernels carry a corrected
        // dispatch/workgroup/output-buffer contract (shared ABI law, all
        // compute targets).
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    pub fn input_resources(&self) -> impl Iterator<Item = &MirKernelResource> {
        std::iter::once(&self.input).chain(self.extra_inputs.iter())
    }

    /// S3-A1: attach an accumulation-buffer resource — read-write device
    /// state the kernel accumulates into across the grid. Bound after the
    /// outputs. The companion-aware materializer (S3-A2) attaches these when
    /// partial gradients accumulate; the ABI carries them so the plan surface
    /// and emitters bind them after the result-tuple outputs.
    pub fn push_accumulation_buffer(&mut self, resource: MirKernelResource) {
        self.accumulation_buffers.push(resource);
    }

    /// Every output resource (the first plus the extra tuple elements), in
    /// tuple order — each binds to a distinct slot (S3-A1 result-tuple
    /// contract).
    pub fn output_resources(&self) -> impl Iterator<Item = &MirKernelResource> {
        std::iter::once(&self.output).chain(self.extra_outputs.iter())
    }

    /// All storage-buffer resources in binding order: inputs, then the first
    /// output, then the extra tuple outputs, then the accumulation buffers.
    pub fn resources(&self) -> impl Iterator<Item = &MirKernelResource> {
        self.input_resources()
            .chain(std::iter::once(&self.output))
            .chain(self.extra_outputs.iter())
            .chain(self.accumulation_buffers.iter())
    }

    /// The kernel's runtime-extent count resource, when present (U2).
    ///
    /// Both emitters read the guard limit from this binding and fall back to
    /// the static dispatch-axis limit via [`Self::oob_guard_limit`] when
    /// absent. Returns the first extent resource (CXO F2: `extra_inputs` is
    /// `Vec`-capable; Wave 1 populates at most one).
    #[must_use]
    pub fn runtime_extent(&self) -> Option<&MirKernelResource> {
        self.extra_inputs
            .iter()
            .find(|resource| resource.kind == MirKernelResourceKind::RuntimeExtent)
    }

    #[must_use]
    pub fn header_builtins(&self) -> Vec<MirKernelBuiltin> {
        combined_builtin_dependencies(
            self.builtin_inputs.iter().map(|input| input.builtin),
            &self.called_builtins,
        )
    }

    /// Classify which invocation-header params the signature's builtins
    /// require. Backend-local text mapping stays in each emitter (L2).
    #[must_use]
    pub fn builtin_usage(&self) -> MirKernelBuiltinUsage {
        MirKernelBuiltinUsage::from_builtins(&self.header_builtins())
    }

    /// Return the dispatch-size axis that corresponds to the first invocation
    /// builtin.
    ///
    /// F-036/F-049: the OOB guard must compare against the correct dispatch
    /// axis instead of always using `dispatch_size.x`.
    ///
    /// `first_builtin` is an explicit argument (L1) because the WGSL emitter
    /// augments the header builtins with body-required builtins before
    /// selecting the first one; Metal passes `header_builtins().first()`.
    #[must_use]
    pub fn oob_guard_limit(&self, first_builtin: MirKernelBuiltin) -> u32 {
        match first_builtin {
            // WorkgroupSize* are compile-time constants and never drive an OOB
            // guard; fall back to the X axis bound (same as pure-X invocation ids).
            MirKernelBuiltin::GlobalInvocationX
            | MirKernelBuiltin::GlobalInvocationId
            | MirKernelBuiltin::WorkgroupIdX
            | MirKernelBuiltin::WorkgroupId
            | MirKernelBuiltin::LocalInvocationIdX
            | MirKernelBuiltin::LocalInvocationId
            | MirKernelBuiltin::WorkgroupSizeX
            | MirKernelBuiltin::WorkgroupSizeY
            | MirKernelBuiltin::WorkgroupSizeZ => {
                u32::try_from(self.dispatch_size.x).unwrap_or(u32::MAX)
            }
            MirKernelBuiltin::GlobalInvocationY
            | MirKernelBuiltin::WorkgroupIdY
            | MirKernelBuiltin::LocalInvocationIdY => {
                u32::try_from(self.dispatch_size.y).unwrap_or(u32::MAX)
            }
            MirKernelBuiltin::GlobalInvocationZ
            | MirKernelBuiltin::WorkgroupIdZ
            | MirKernelBuiltin::LocalInvocationIdZ => {
                u32::try_from(self.dispatch_size.z).unwrap_or(u32::MAX)
            }
        }
    }
}

fn combined_builtin_dependencies(
    input_builtins: impl IntoIterator<Item = MirKernelBuiltin>,
    called_builtins: &[MirKernelBuiltin],
) -> Vec<MirKernelBuiltin> {
    let mut builtins = Vec::new();
    for builtin in input_builtins {
        push_unique_builtin(&mut builtins, builtin);
    }
    for builtin in called_builtins {
        push_unique_builtin(&mut builtins, *builtin);
    }
    builtins
}

fn called_builtins_for_function(function: &MirFunction) -> Vec<MirKernelBuiltin> {
    let mut builtins = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            if let MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::GpuBuiltin(builtin),
                        ..
                    },
                ..
            } = &statement.kind
            {
                push_unique_builtin(&mut builtins, *builtin);
            }
        }
    }
    builtins
}

fn push_unique_builtin(builtins: &mut Vec<MirKernelBuiltin>, builtin: MirKernelBuiltin) {
    if !builtins.contains(&builtin) {
        builtins.push(builtin);
    }
}

/// Corrected dispatch/workgroup/output-buffer contract for collection-op
/// kernels (PRP-004 U3, decision D1-A default).
///
/// The generic indexed-view contract (`workgroup (1,1,1)`, dispatch = output
/// length) cannot drive real multi-workgroup execution:
/// - reduction output length is 1, so the generic contract launches exactly
///   one thread — the emitted kernel would read `buffer[0]` only;
/// - matmul dispatch sized by M·N with `workgroup (1,1,1)` pins `id.y` to 0,
///   so only output row 0 can ever be written.
///
/// The corrected contract:
/// - **reduction (Sum/Mean)**: `dispatch_size.x = n` (the grid covers the
///   whole tensor), power-of-two workgroup `min(256, next_pow2(n))`, output
///   buffer sized to `workgroup_count = ceil(n/ws)` partial slots (one
///   per-workgroup partial; the host combines);
/// - **matmul (`TensorMatMul`)**: `workgroup (8,8,1)` (the emitted tile),
///   `dispatch (ceil(N/8), ceil(M/8), 1)` — one workgroup per output tile;
/// - **transpose (`TensorTranspose`, S5-U1)**: `workgroup
///   (TRANSPOSE_WORKGROUP_X, 1, 1)` (elementwise-style 1D lanes),
///   `dispatch (M·N, 1, 1)` — one thread per output element of the rank-2
///   `[M,N] → [N,M]` remap.
///
/// Consumed by the kernel plan module
/// (`kernel_plan::CollectionKernelPlan::from_contract`) as the dispatch half
/// of the collection-op recipe (council L5: plan and contract stay separate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionOpContract {
    Reduction {
        /// True flat tensor length `n`.
        length: u64,
        /// Power-of-two workgroup lane count.
        workgroup_x: u32,
        /// Partial slot count `ceil(n / ws)`.
        partials: u64,
    },
    MatMul {
        /// Output tile grid `(ceil(N/8), ceil(M/8))`.
        dispatch_x: u64,
        dispatch_y: u64,
    },
    Transpose {
        /// Source rows `M` of the rank-2 operand (output columns).
        m: u64,
        /// Source columns `N` of the rank-2 operand (output rows).
        n: u64,
    },
}

/// Apply the collection-op dispatch contract to a kernel signature in place.
///
/// Non-collection kernels are untouched (elementwise and scalar kernels keep
/// the generic 1D contract).
fn apply_collection_op_contract(
    function: &MirFunction,
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    let Some(contract) = collection_op_contract(function, validation)? else {
        return Ok(());
    };
    match contract {
        CollectionOpContract::Reduction {
            length,
            workgroup_x,
            partials,
        } => {
            signature.workgroup = MirWorkgroupSize {
                x: workgroup_x,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: length,
                y: 1,
                z: 1,
            };
            signature.output.element_count = partials;
        }
        CollectionOpContract::MatMul {
            dispatch_x,
            dispatch_y,
        } => {
            signature.workgroup = MirWorkgroupSize { x: 8, y: 8, z: 1 };
            signature.dispatch_size = MirKernelDispatchSize {
                x: dispatch_x,
                y: dispatch_y,
                z: 1,
            };
        }
        CollectionOpContract::Transpose { m, n } => {
            // Elementwise-style 1D contract: one thread per output element
            // of the rank-2 remap (S5-U1).
            signature.workgroup = MirWorkgroupSize {
                x: crate::kernel_plan::TRANSPOSE_WORKGROUP_X,
                y: 1,
                z: 1,
            };
            signature.dispatch_size = MirKernelDispatchSize {
                x: m.saturating_mul(n),
                y: 1,
                z: 1,
            };
        }
    }
    Ok(())
}

/// Detect a collection-op kernel and resolve its dispatch contract from the
/// operand MIR types (statement scan analogous to
/// [`called_builtins_for_function`]).
///
/// The plan pass (`kernel_plan::CollectionKernelPlan::from_contract`) consumes
/// the resolved contract as its dispatch half (L5); this resolver stays the
/// single source of truth for the dispatch math.
///
/// # Errors
/// Returns [`MirAbiError`] when an operand type cannot be resolved to a
/// static device-view shape, when the reduction tensor length is zero, when
/// matmul operands are not rank-2 with matching inner dimensions, or when
/// transpose operands are not rank-2 with non-zero dimensions.
pub fn collection_op_contract(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Option<CollectionOpContract>, MirAbiError> {
    for block in &function.blocks {
        for statement in &block.statements {
            let Some(op) = crate::collection_ops::statement_collection_op(statement) else {
                continue;
            };
            let MirStatementKind::RuntimeCall {
                call: MirRuntimeCall { args, .. },
                ..
            } = &statement.kind
            else {
                continue;
            };
            match op {
                MirCollectionOp::Sum | MirCollectionOp::TensorMean => {
                    let Some(operand_ty) = args
                        .first()
                        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
                    else {
                        continue;
                    };
                    let length = device_view_element_count(operand_ty, validation)?;
                    if length == 0 {
                        return Err(MirAbiError::new("reduction tensor length must be non-zero"));
                    }
                    let workgroup_x = reduction_workgroup_x(length);
                    let partials = length.div_ceil(u64::from(workgroup_x));
                    return Ok(Some(CollectionOpContract::Reduction {
                        length,
                        workgroup_x,
                        partials,
                    }));
                }
                MirCollectionOp::TensorMatMul => {
                    let Some(left_ty) = args
                        .first()
                        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
                    else {
                        continue;
                    };
                    let Some(right_ty) = args
                        .get(1)
                        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
                    else {
                        continue;
                    };
                    let left_dims = crate::tensor_type::tensor_literal_dims(left_ty, validation)
                        .map_err(MirAbiError::new)?;
                    let right_dims = crate::tensor_type::tensor_literal_dims(right_ty, validation)
                        .map_err(MirAbiError::new)?;
                    if left_dims.len() != 2 || right_dims.len() != 2 {
                        return Err(MirAbiError::new(
                            "matmul dispatch contract requires rank-2 tensor operands",
                        ));
                    }
                    if left_dims[1] != right_dims[0] {
                        return Err(MirAbiError::new("matmul inner dimensions must match"));
                    }
                    let tile: u64 = 8;
                    return Ok(Some(CollectionOpContract::MatMul {
                        dispatch_x: right_dims[1].div_ceil(tile),
                        dispatch_y: left_dims[0].div_ceil(tile),
                    }));
                }
                MirCollectionOp::TensorTranspose => {
                    let Some(operand_ty) = args
                        .first()
                        .and_then(|arg| crate::local_ty::operand_local_ty(function, arg))
                    else {
                        continue;
                    };
                    let dims = crate::tensor_type::tensor_literal_dims(operand_ty, validation)
                        .map_err(MirAbiError::new)?;
                    if dims.len() != 2 {
                        return Err(MirAbiError::new(
                            "transpose dispatch contract requires a rank-2 tensor operand",
                        ));
                    }
                    if dims[0] == 0 || dims[1] == 0 {
                        return Err(MirAbiError::new(
                            "transpose dispatch contract requires non-zero dimensions",
                        ));
                    }
                    return Ok(Some(CollectionOpContract::Transpose {
                        m: dims[0],
                        n: dims[1],
                    }));
                }
                _ => {}
            }
        }
    }
    Ok(None)
}

/// Power-of-two workgroup lane count for a reduction: `min(256, next_pow2(n))`
/// (D1-A default).
fn reduction_workgroup_x(length: u64) -> u32 {
    let next = if length <= 1 {
        1
    } else {
        length.next_power_of_two()
    };
    u32::try_from(next.min(256)).unwrap_or(256)
}

fn builtin_input_for_index_param(
    index: &MirParam,
    interner: Option<&Interner>,
) -> MirKernelBuiltinInput {
    MirKernelBuiltinInput {
        local: index.local,
        source_name: param_source_name(index, interner),
        builtin: interner
            .and_then(|interner| named_builtin_for_param(index, interner))
            .unwrap_or(MirKernelBuiltin::GlobalInvocationX),
        ty: index.ty,
    }
}

fn builtin_inputs_for_named_params(
    params: &[MirParam],
    validation: &MirValidationContext<'_>,
    interner: &Interner,
) -> Result<Vec<MirKernelBuiltinInput>, MirAbiError> {
    let mut inputs = Vec::new();
    for param in params {
        require_u32_index(param.ty, validation)?;
        let builtin = named_builtin_for_param(param, interner)
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_BUILTIN_UNKNOWN_NAME))?;
        if inputs
            .iter()
            .any(|input: &MirKernelBuiltinInput| input.builtin == builtin)
        {
            return Err(MirAbiError::new(ERR_KERNEL_BUILTIN_DUPLICATE_NAME));
        }
        inputs.push(MirKernelBuiltinInput {
            local: param.local,
            source_name: param_source_name(param, Some(interner)),
            builtin,
            ty: param.ty,
        });
    }
    Ok(inputs)
}

fn shader_access_for_resource(access: MirKernelResourceAccess) -> MirKernelShaderAccess {
    match access {
        MirKernelResourceAccess::Read => MirKernelShaderAccess::Read,
        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite => {
            MirKernelShaderAccess::ReadWrite
        }
    }
}

fn buffer_binding_type_for_shader_access(
    shader_access: MirKernelShaderAccess,
) -> MirKernelBufferBindingType {
    match shader_access {
        MirKernelShaderAccess::Read => MirKernelBufferBindingType::ReadOnlyStorage,
        MirKernelShaderAccess::ReadWrite => MirKernelBufferBindingType::Storage,
    }
}

fn param_source_name(param: &MirParam, interner: Option<&Interner>) -> Option<String> {
    // A synthetic symbol outside the interner's string table (the reverse-AD
    // residual/upstream placeholders of a generated companion, S3-A2) is
    // treated as unnamed rather than indexing out of bounds.
    let name = param.name?;
    let interner = interner?;
    if (name.0 as usize) < interner.strings().len() {
        Some(interner.resolve(name).to_owned())
    } else {
        None
    }
}

fn named_builtin_for_param(param: &MirParam, interner: &Interner) -> Option<MirKernelBuiltin> {
    param
        .name
        .and_then(|name| MirKernelBuiltin::from_spelling(interner.resolve(name)))
}

impl MirKernelReflection {
    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn from_signature(
        entry_name: impl Into<String>,
        signature: &MirKernelSignature,
        validation: &MirValidationContext<'_>,
    ) -> Result<Self, MirAbiError> {
        Ok(Self {
            function: signature.function,
            provenance: MirKernelProvenance::MirFunction(signature.function),
            entry_name: entry_name.into(),
            shader_stage: MirKernelShaderStage::Compute,
            resources: signature
                .resources()
                .map(|resource| reflect_kernel_resource(resource, validation))
                .collect::<Result<_, _>>()?,
            vertex_inputs: Vec::new(),
            varyings: Vec::new(),
            fragment_outputs: Vec::new(),
            builtin_inputs: signature
                .builtin_inputs
                .iter()
                .map(|input| reflect_kernel_builtin_input(input, validation))
                .collect::<Result<_, _>>()?,
            called_builtins: signature.called_builtins.clone(),
            workgroup: signature.workgroup,
            dispatch_size: signature.dispatch_size,
            workgroup_count: workgroup_count_for_dispatch(
                signature.dispatch_size,
                signature.workgroup,
            )?,
        })
    }

    /// Recompute the reflection's workgroup grid and workgroup count against
    /// the effective workgroup with collection-op-aware dispatch semantics.
    ///
    /// [`from_signature`] reflects the signature default: `workgroup_count` is
    /// the naive `ceil(dispatch_size / workgroup)`. Fused-grid collection-op
    /// kernels override `@workgroup_size` via the effective workgroup (the
    /// caller's emitter policy — WGSL's tiled matmul tile, the metal
    /// signature workgroup, …). This is the single shared correction every
    /// reflection-producing consumer applies so the launch plan, chain
    /// descriptor, and reflection JSON report the same dispatch contract:
    /// - **matmul**: `dispatch_size` is the tile grid
    ///   `(ceil(N/8), ceil(M/8), 1)` — one workgroup per output tile — so the
    ///   workgroup count is `dispatch_size` verbatim, identical to the
    ///   chain-descriptor path. Dividing by the tile would under-launch: a
    ///   16×16 matmul would dispatch one (8,8) workgroup instead of four,
    ///   silently leaving 3 of 4 output tiles zero (PRP-004).
    /// - **everything else** (reduction, elementwise, non-collection):
    ///   `dispatch_size` carries thread coverage, so the count is
    ///   `ceil(dispatch_size / effective)`.
    ///
    /// Idempotent when the effective workgroup equals the signature default:
    /// the count then equals the naive `from_signature` value, so
    /// non-collection consumers are unchanged (C-A1-N).
    ///
    /// Semantics lifted from the former WGSL-local `apply_effective_workgroup`
    /// (decision D-A1, delivery D-W6-A1): this method is the single source of
    /// truth for the collection-op `workgroup_count` semantics.
    pub fn apply_effective_workgroup(
        &mut self,
        function: &MirFunction,
        effective: MirWorkgroupSize,
    ) {
        self.workgroup = effective;
        self.workgroup_count =
            collection_op_workgroup_count(function, self.dispatch_size, effective);
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn vertex_entry(
        function: MirFunctionId,
        entry_name: impl Into<String>,
        vertex_inputs: Vec<MirVertexInputReflection>,
        resources: Vec<MirKernelResourceReflection>,
    ) -> Result<Self, MirAbiError> {
        validate_vertex_input_locations(&vertex_inputs)?;
        Ok(Self {
            function,
            provenance: MirKernelProvenance::MirFunction(function),
            entry_name: entry_name.into(),
            shader_stage: MirKernelShaderStage::Vertex,
            resources,
            vertex_inputs,
            varyings: Vec::new(),
            fragment_outputs: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: Vec::new(),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        })
    }

    /// Build a fragment-stage entry reflection. `varyings` are the values the
    /// fragment consumes from the vertex stage; `fragment_outputs` are its
    /// render-target outputs. Locations must be unique within each list.
    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn fragment_entry(
        function: MirFunctionId,
        entry_name: impl Into<String>,
        varyings: Vec<MirVaryingReflection>,
        fragment_outputs: Vec<MirFragmentOutputReflection>,
        resources: Vec<MirKernelResourceReflection>,
    ) -> Result<Self, MirAbiError> {
        validate_varying_locations(&varyings)?;
        validate_fragment_output_locations(&fragment_outputs)?;
        Ok(Self {
            function,
            provenance: MirKernelProvenance::MirFunction(function),
            entry_name: entry_name.into(),
            shader_stage: MirKernelShaderStage::Fragment,
            resources,
            vertex_inputs: Vec::new(),
            varyings,
            fragment_outputs,
            builtin_inputs: Vec::new(),
            called_builtins: Vec::new(),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        })
    }

    /// Source-annotation fragment entry: an `@ fragment` function with an empty
    /// body, paired with reflected varyings and render-target outputs supplied
    /// by the driver/proof path.
    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn source_fragment_entry(
        source_def: DefId,
        entry_name: impl Into<String>,
        varyings: Vec<MirVaryingReflection>,
        fragment_outputs: Vec<MirFragmentOutputReflection>,
        resources: Vec<MirKernelResourceReflection>,
    ) -> Result<Self, MirAbiError> {
        validate_varying_locations(&varyings)?;
        validate_fragment_output_locations(&fragment_outputs)?;
        Ok(Self {
            function: MirFunctionId(0),
            provenance: MirKernelProvenance::SourceFunction(source_def),
            entry_name: entry_name.into(),
            shader_stage: MirKernelShaderStage::Fragment,
            resources,
            vertex_inputs: Vec::new(),
            varyings,
            fragment_outputs,
            builtin_inputs: Vec::new(),
            called_builtins: Vec::new(),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        })
    }

    #[must_use]
    pub fn mir_function_provenance(&self) -> Option<MirFunctionId> {
        match self.provenance {
            MirKernelProvenance::MirFunction(function) => Some(function),
            MirKernelProvenance::SourceFunction(_) => None,
        }
    }

    #[must_use]
    pub fn source_function_provenance(&self) -> Option<DefId> {
        match self.provenance {
            MirKernelProvenance::SourceFunction(def_id) => Some(def_id),
            MirKernelProvenance::MirFunction(_) => None,
        }
    }

    /// Build a graphics pipeline reflection from a vertex + fragment stage pair.
    ///
    /// This is the Stage 4 draw-prerequisite seam: it validates that the stages
    /// are compatible (vertex + fragment, varying agreement) and produces the
    /// target formats, topology, and vertex count a host needs to construct a
    /// render pipeline and issue a draw call.
    ///
    /// # Defaults
    ///
    /// | Field | Default | Builder |
    /// |---|---|---|
    /// | `primitive_topology` | `TriangleList` | [`with_topology`] |
    /// | `depth_stencil` | `None` | [`with_depth_stencil`] + [`with_stencil`] |
    ///
    /// [`color_target_formats`] is set from the caller-supplied `color_targets`
    /// slice directly — pass multiple targets for MRT; there is no single-target
    /// default.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    /// - the vertex entry is not a vertex shader stage
    /// - the fragment entry is not a fragment shader stage
    /// - the vertex has no vertex inputs
    /// - color targets list is empty
    /// - vertex and fragment varyings are not consistent
    pub fn graphics_pipeline_reflection(
        vertex: &MirKernelReflection,
        fragment: &MirKernelReflection,
        vertex_count: u32,
        color_targets: &[MirColorTargetFormat],
    ) -> Result<MirGraphicsPipelineReflection, MirAbiError> {
        if vertex.shader_stage != MirKernelShaderStage::Vertex {
            return Err(MirAbiError::new(ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX));
        }
        if fragment.shader_stage != MirKernelShaderStage::Fragment {
            return Err(MirAbiError::new(ERR_GRAPHICS_PIPELINE_REQUIRES_FRAGMENT));
        }
        if vertex.vertex_inputs.is_empty() {
            return Err(MirAbiError::new(
                ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX_INPUTS,
            ));
        }
        if color_targets.is_empty() {
            return Err(MirAbiError::new(
                ERR_GRAPHICS_PIPELINE_REQUIRES_COLOR_TARGETS,
            ));
        }
        if let Err(inner) = validate_varying_consistency(vertex, fragment) {
            return Err(MirAbiError::with_detail(
                ERR_GRAPHICS_PIPELINE_VARYING_MISMATCH,
                "mir_graphics_pipeline_varying_mismatch",
                inner.message,
            ));
        }
        Ok(MirGraphicsPipelineReflection {
            color_target_formats: color_targets.to_vec(),
            primitive_topology: MirPrimitiveTopology::TriangleList,
            vertex_count,
            vertex_input_count: vertex.vertex_inputs.len(),
            varying_count: vertex.varyings.len(),
            vertex_resources: vertex.resources.clone(),
            vertex_resource_count: vertex.resources.len(),
            fragment_resources: fragment.resources.clone(),
            fragment_resource_count: fragment.resources.len(),
            depth_stencil: None,
        })
    }

    #[must_use]
    pub fn launch_plan(&self) -> MirKernelLaunchPlan {
        let mut bind_groups = BTreeMap::<u32, Vec<MirKernelLaunchBinding>>::new();
        for (resource_index, resource) in self.resources.iter().enumerate() {
            let shader_access = shader_access_for_resource(resource.access);
            bind_groups
                .entry(resource.group)
                .or_default()
                .push(MirKernelLaunchBinding {
                    binding_index: 0,
                    resource_index,
                    binding: resource.binding,
                    kind: resource.kind,
                    role: resource.role,
                    access: resource.access,
                    shader_access,
                    shader_visibility: self.shader_stage,
                    buffer_binding_type: buffer_binding_type_for_shader_access(shader_access),
                    element_ty: resource.element_ty,
                    element_layout: resource.element_layout,
                    element_byte_width: resource.element_byte_width,
                    element_count: resource.element_count,
                    buffer_byte_len: resource.buffer_byte_len,
                    buffer_byte_offset: 0,
                    binding_byte_len: resource.buffer_byte_len,
                    min_binding_size: resource.buffer_byte_len,
                    has_dynamic_offset: false,
                    source_local: resource.source_local,
                    source_name: resource.source_name.clone(),
                });
        }
        let bind_groups = bind_groups
            .into_iter()
            .enumerate()
            .map(|(bind_group_index, (group, mut bindings))| {
                bindings.sort_by_key(|binding| binding.binding);
                for (binding_index, binding) in bindings.iter_mut().enumerate() {
                    binding.binding_index = binding_index;
                }
                let resource_indexes = bindings
                    .iter()
                    .map(|binding| binding.resource_index)
                    .collect::<Vec<_>>();
                let resource_index_count = resource_indexes.len();
                let resource_kinds = bindings
                    .iter()
                    .map(|binding| binding.kind)
                    .collect::<Vec<_>>();
                let resource_kind_count = resource_kinds.len();
                let resource_roles = bindings
                    .iter()
                    .map(|binding| binding.role)
                    .collect::<Vec<_>>();
                let resource_role_count = resource_roles.len();
                let shader_accesses = bindings
                    .iter()
                    .map(|binding| binding.shader_access)
                    .collect::<Vec<_>>();
                let shader_access_count = shader_accesses.len();
                let buffer_binding_types = bindings
                    .iter()
                    .map(|binding| binding.buffer_binding_type)
                    .collect::<Vec<_>>();
                let buffer_binding_type_count = buffer_binding_types.len();
                let buffer_byte_offsets = bindings
                    .iter()
                    .map(|binding| binding.buffer_byte_offset)
                    .collect::<Vec<_>>();
                let buffer_byte_offset_count = buffer_byte_offsets.len();
                let buffer_byte_lens = bindings
                    .iter()
                    .map(|binding| binding.buffer_byte_len)
                    .collect::<Vec<_>>();
                let buffer_byte_len_count = buffer_byte_lens.len();
                let binding_byte_lens = bindings
                    .iter()
                    .map(|binding| binding.binding_byte_len)
                    .collect::<Vec<_>>();
                let binding_byte_len_count = binding_byte_lens.len();
                let min_binding_sizes = bindings
                    .iter()
                    .map(|binding| binding.min_binding_size)
                    .collect::<Vec<_>>();
                let min_binding_size_count = min_binding_sizes.len();
                let has_dynamic_offsets = bindings
                    .iter()
                    .map(|binding| binding.has_dynamic_offset)
                    .collect::<Vec<_>>();
                let dynamic_offset_flag_count = has_dynamic_offsets.len();
                let offset_modes = has_dynamic_offsets
                    .iter()
                    .map(|has_offset| {
                        if *has_offset {
                            MirKernelOffsetMode::Dynamic
                        } else {
                            MirKernelOffsetMode::Fixed
                        }
                    })
                    .collect::<Vec<_>>();
                let offset_mode_entry_count = offset_modes.len();
                let dynamic_offset_count = has_dynamic_offsets
                    .iter()
                    .filter(|has_offset| **has_offset)
                    .count();
                let requires_dynamic_offsets = dynamic_offset_count > 0;
                let non_dynamic_offset_count = has_dynamic_offsets.len() - dynamic_offset_count;
                let has_non_dynamic_offsets = non_dynamic_offset_count > 0;
                let has_mixed_offset_modes = requires_dynamic_offsets && has_non_dynamic_offsets;
                let offset_mode_count =
                    usize::from(requires_dynamic_offsets) + usize::from(has_non_dynamic_offsets);
                let has_only_non_dynamic_offsets =
                    has_non_dynamic_offsets && !requires_dynamic_offsets;
                let has_only_dynamic_offsets = requires_dynamic_offsets && !has_non_dynamic_offsets;
                let non_dynamic_offset_resource_indexes = bindings
                    .iter()
                    .filter(|binding| !binding.has_dynamic_offset)
                    .map(|binding| binding.resource_index)
                    .collect::<Vec<_>>();
                let non_dynamic_offset_resource_index_count =
                    non_dynamic_offset_resource_indexes.len();
                let non_dynamic_offset_binding_indexes = bindings
                    .iter()
                    .filter(|binding| !binding.has_dynamic_offset)
                    .map(|binding| binding.binding)
                    .collect::<Vec<_>>();
                let non_dynamic_offset_binding_index_count =
                    non_dynamic_offset_binding_indexes.len();
                let dynamic_offset_resource_indexes = bindings
                    .iter()
                    .filter(|binding| binding.has_dynamic_offset)
                    .map(|binding| binding.resource_index)
                    .collect::<Vec<_>>();
                let dynamic_offset_resource_index_count = dynamic_offset_resource_indexes.len();
                let dynamic_offset_binding_indexes = bindings
                    .iter()
                    .filter(|binding| binding.has_dynamic_offset)
                    .map(|binding| binding.binding)
                    .collect::<Vec<_>>();
                let dynamic_offset_binding_index_count = dynamic_offset_binding_indexes.len();
                let binding_indexes = bindings
                    .iter()
                    .map(|binding| binding.binding)
                    .collect::<Vec<_>>();
                let binding_index_count = binding_indexes.len();
                let layout_entries = bindings
                    .iter()
                    .enumerate()
                    .map(|(layout_entry_index, binding)| {
                        layout_entry_for_launch_binding(layout_entry_index, binding)
                    })
                    .collect::<Vec<_>>();
                let non_dynamic_offset_layout_entry_indexes = layout_entries
                    .iter()
                    .filter(|entry| !entry.has_dynamic_offset)
                    .map(|entry| entry.layout_entry_index)
                    .collect::<Vec<_>>();
                let non_dynamic_offset_layout_entry_index_count =
                    non_dynamic_offset_layout_entry_indexes.len();
                let dynamic_offset_layout_entry_indexes = layout_entries
                    .iter()
                    .filter(|entry| entry.has_dynamic_offset)
                    .map(|entry| entry.layout_entry_index)
                    .collect::<Vec<_>>();
                let dynamic_offset_layout_entry_index_count =
                    dynamic_offset_layout_entry_indexes.len();
                let layout_entry_indexes = layout_entries
                    .iter()
                    .map(|entry| entry.layout_entry_index)
                    .collect::<Vec<_>>();
                let layout_entry_index_count = layout_entry_indexes.len();
                let source_locals = bindings
                    .iter()
                    .filter_map(|binding| binding.source_local)
                    .collect::<Vec<_>>();
                let source_local_count = source_locals.len();
                let source_names = bindings
                    .iter()
                    .filter_map(|binding| binding.source_name.clone())
                    .collect::<Vec<_>>();
                let source_name_count = source_names.len();
                MirKernelLaunchBindGroup {
                    bind_group_index,
                    group,
                    resource_indexes,
                    resource_index_count,
                    resource_kinds,
                    resource_kind_count,
                    resource_roles,
                    resource_role_count,
                    shader_accesses,
                    shader_access_count,
                    buffer_binding_types,
                    buffer_binding_type_count,
                    buffer_byte_offsets,
                    buffer_byte_offset_count,
                    buffer_byte_lens,
                    buffer_byte_len_count,
                    binding_byte_lens,
                    binding_byte_len_count,
                    min_binding_sizes,
                    min_binding_size_count,
                    has_dynamic_offsets,
                    dynamic_offset_flag_count,
                    offset_modes,
                    offset_mode_entry_count,
                    requires_dynamic_offsets,
                    has_non_dynamic_offsets,
                    has_mixed_offset_modes,
                    offset_mode_count,
                    has_only_non_dynamic_offsets,
                    has_only_dynamic_offsets,
                    non_dynamic_offset_count,
                    non_dynamic_offset_resource_indexes,
                    non_dynamic_offset_resource_index_count,
                    non_dynamic_offset_binding_indexes,
                    non_dynamic_offset_binding_index_count,
                    non_dynamic_offset_layout_entry_indexes,
                    non_dynamic_offset_layout_entry_index_count,
                    dynamic_offset_count,
                    dynamic_offset_resource_indexes,
                    dynamic_offset_resource_index_count,
                    dynamic_offset_binding_indexes,
                    dynamic_offset_binding_index_count,
                    dynamic_offset_layout_entry_indexes,
                    dynamic_offset_layout_entry_index_count,
                    binding_indexes,
                    binding_index_count,
                    layout_entry_indexes,
                    layout_entry_index_count,
                    source_locals,
                    source_local_count,
                    source_names,
                    source_name_count,
                    binding_count: bindings.len(),
                    layout_entry_count: layout_entries.len(),
                    layout_entries,
                    bindings,
                }
            })
            .collect::<Vec<_>>();
        let pipeline_layout = pipeline_layout_for_launch_bind_groups(&bind_groups);
        MirKernelLaunchPlan {
            entry_name: self.entry_name.clone(),
            shader_stage: self.shader_stage,
            pipeline_layout,
            bind_group_count: bind_groups.len(),
            bind_groups,
            vertex_input_count: self.vertex_inputs.len(),
            vertex_inputs: self.vertex_inputs.clone(),
            required_builtins: self.required_builtins(),
            builtin_inputs: self.builtin_inputs.clone(),
            called_builtins: self.called_builtins.clone(),
            workgroup: self.workgroup,
            dispatch_size: self.dispatch_size,
            workgroup_count: self.workgroup_count,
        }
    }

    fn required_builtins(&self) -> Vec<MirKernelBuiltin> {
        combined_builtin_dependencies(
            self.builtin_inputs.iter().map(|input| input.builtin),
            &self.called_builtins,
        )
    }
}

fn validate_vertex_input_locations(inputs: &[MirVertexInputReflection]) -> Result<(), MirAbiError> {
    let mut seen = BTreeMap::<u32, &MirVertexInputReflection>::new();
    for input in inputs {
        if seen.insert(input.location, input).is_some() {
            return Err(MirAbiError::with_detail(
                ERR_VERTEX_INPUT_DUPLICATE_LOCATION,
                "mir_vertex_input_duplicate_location",
                input.location.to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_varying_locations(varyings: &[MirVaryingReflection]) -> Result<(), MirAbiError> {
    let mut seen = BTreeMap::<u32, &MirVaryingReflection>::new();
    for varying in varyings {
        if seen.insert(varying.location, varying).is_some() {
            return Err(MirAbiError::with_detail(
                ERR_VARYING_DUPLICATE_LOCATION,
                "mir_varying_duplicate_location",
                varying.location.to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_fragment_output_locations(
    outputs: &[MirFragmentOutputReflection],
) -> Result<(), MirAbiError> {
    let mut seen = BTreeMap::<u32, &MirFragmentOutputReflection>::new();
    for output in outputs {
        if seen.insert(output.location, output).is_some() {
            return Err(MirAbiError::with_detail(
                ERR_FRAGMENT_OUTPUT_DUPLICATE_LOCATION,
                "mir_fragment_output_duplicate_location",
                output.location.to_string(),
            ));
        }
    }
    Ok(())
}

/// Cross-stage consistency: every fragment varying must match a vertex output
/// varying by location, format, and interpolation. Called when a vertex and a
/// fragment entry are emitted together.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`MirAbiError`].
pub fn validate_varying_consistency(
    vertex: &MirKernelReflection,
    fragment: &MirKernelReflection,
) -> Result<(), MirAbiError> {
    for fragment_varying in &fragment.varyings {
        let Some(vertex_varying) = vertex
            .varyings
            .iter()
            .find(|v| v.location == fragment_varying.location)
        else {
            return Err(MirAbiError::with_detail(
                ERR_VARYING_FRAGMENT_WITHOUT_VERTEX,
                "mir_varying_fragment_without_vertex",
                fragment_varying.location.to_string(),
            ));
        };
        if vertex_varying.format != fragment_varying.format
            || vertex_varying.interpolation != fragment_varying.interpolation
        {
            return Err(MirAbiError::with_detail(
                ERR_VARYING_STAGE_MISMATCH,
                "mir_varying_stage_mismatch",
                fragment_varying.location.to_string(),
            ));
        }
    }
    // Reverse: every vertex varying must have a matching fragment input.
    for vertex_varying in &vertex.varyings {
        if !fragment
            .varyings
            .iter()
            .any(|v| v.location == vertex_varying.location)
        {
            return Err(MirAbiError::with_detail(
                ERR_VARYING_STAGE_MISMATCH,
                "mir_varying_vertex_without_fragment",
                vertex_varying.location.to_string(),
            ));
        }
    }
    Ok(())
}

fn pipeline_layout_for_launch_bind_groups(
    bind_groups: &[MirKernelLaunchBindGroup],
) -> MirKernelLaunchPipelineLayout {
    let bind_group_layouts = bind_groups
        .iter()
        .enumerate()
        .map(
            |(bind_group_index, bind_group)| MirKernelLaunchPipelineBindGroupLayout {
                bind_group_index,
                group: bind_group.group,
                layout_entry_count: bind_group.layout_entries.len(),
            },
        )
        .collect::<Vec<_>>();
    MirKernelLaunchPipelineLayout {
        bind_group_layout_count: bind_group_layouts.len(),
        bind_group_layouts,
    }
}

impl MirKernelLaunchPlan {
    pub fn webgpu_adapter(&self) -> MirWebGpuLaunchAdapter {
        let bind_group_layout_descriptor_indexes = self
            .bind_groups
            .iter()
            .map(|group| group.bind_group_index)
            .collect::<Vec<_>>();
        let bind_group_layout_descriptor_index_count = bind_group_layout_descriptor_indexes.len();
        let bind_group_descriptor_indexes = self
            .bind_groups
            .iter()
            .map(|group| group.bind_group_index)
            .collect::<Vec<_>>();
        let bind_group_descriptor_index_count = bind_group_descriptor_indexes.len();
        let vertex_buffer_layout_descriptor_indexes = self
            .vertex_inputs
            .iter()
            .enumerate()
            .map(|(buffer_index, _)| buffer_index)
            .collect::<Vec<_>>();
        let vertex_buffer_layout_descriptor_index_count =
            vertex_buffer_layout_descriptor_indexes.len();

        MirWebGpuLaunchAdapter {
            pipeline_layout_descriptor: webgpu_pipeline_layout_descriptor(&self.pipeline_layout),
            bind_group_layout_descriptor_count: self.bind_group_count,
            bind_group_layout_descriptor_indexes,
            bind_group_layout_descriptor_index_count,
            bind_group_layout_descriptors: self
                .bind_groups
                .iter()
                .map(webgpu_bind_group_layout_descriptor)
                .collect(),
            bind_group_descriptor_count: self.bind_group_count,
            bind_group_descriptor_indexes,
            bind_group_descriptor_index_count,
            bind_group_descriptors: self
                .bind_groups
                .iter()
                .map(webgpu_bind_group_descriptor)
                .collect(),
            vertex_buffer_layout_descriptor_count: self.vertex_input_count,
            vertex_buffer_layout_descriptor_indexes,
            vertex_buffer_layout_descriptor_index_count,
            vertex_buffer_layout_descriptors: self
                .vertex_inputs
                .iter()
                .enumerate()
                .map(|(buffer_index, input)| {
                    webgpu_vertex_buffer_layout_descriptor(buffer_index, input)
                })
                .collect(),
            dispatch_workgroup_dimension_count: 3,
            dispatch_workgroups: MirWebGpuDispatchWorkgroups {
                x: self.workgroup_count.x,
                y: self.workgroup_count.y,
                z: self.workgroup_count.z,
            },
        }
    }
}

fn webgpu_pipeline_layout_descriptor(
    layout: &MirKernelLaunchPipelineLayout,
) -> MirWebGpuPipelineLayoutDescriptor {
    let bind_group_layout_indexes = layout
        .bind_group_layouts
        .iter()
        .map(|bind_group_layout| bind_group_layout.bind_group_index)
        .collect::<Vec<_>>();
    let bind_group_layout_index_count = bind_group_layout_indexes.len();

    MirWebGpuPipelineLayoutDescriptor {
        bind_group_layout_count: layout.bind_group_layout_count,
        bind_group_layout_indexes,
        bind_group_layout_index_count,
    }
}

fn webgpu_bind_group_layout_descriptor(
    group: &MirKernelLaunchBindGroup,
) -> MirWebGpuBindGroupLayoutDescriptor {
    let layout_entry_indexes = group
        .layout_entries
        .iter()
        .map(|entry| entry.layout_entry_index)
        .collect::<Vec<_>>();
    let layout_entry_index_count = layout_entry_indexes.len();

    MirWebGpuBindGroupLayoutDescriptor {
        bind_group_index: group.bind_group_index,
        group: group.group,
        layout_entry_indexes,
        layout_entry_index_count,
        entry_count: group.layout_entry_count,
        entries: group
            .layout_entries
            .iter()
            .map(webgpu_bind_group_layout_entry_descriptor)
            .collect(),
    }
}

fn webgpu_bind_group_layout_entry_descriptor(
    entry: &MirKernelLaunchBindGroupLayoutEntry,
) -> MirWebGpuBindGroupLayoutEntryDescriptor {
    MirWebGpuBindGroupLayoutEntryDescriptor {
        binding: entry.binding,
        binding_index: entry.binding_index,
        buffer_byte_len: entry.buffer_byte_len,
        buffer_byte_offset: entry.buffer_byte_offset,
        binding_byte_len: entry.binding_byte_len,
        visibility: entry.shader_visibility,
        buffer_type: entry.buffer_binding_type,
        has_dynamic_offset: entry.has_dynamic_offset,
        min_binding_size: entry.min_binding_size,
        resource_index: entry.resource_index,
        layout_entry_index: entry.layout_entry_index,
        source_local: entry.source_local,
        source_name: entry.source_name.clone(),
    }
}

fn webgpu_bind_group_descriptor(group: &MirKernelLaunchBindGroup) -> MirWebGpuBindGroupDescriptor {
    let entry_indexes = group
        .bindings
        .iter()
        .map(|binding| binding.binding_index)
        .collect::<Vec<_>>();
    let entry_index_count = entry_indexes.len();

    MirWebGpuBindGroupDescriptor {
        bind_group_index: group.bind_group_index,
        group: group.group,
        entry_indexes,
        entry_index_count,
        entry_count: group.binding_count,
        entries: group
            .bindings
            .iter()
            .map(webgpu_bind_group_entry_descriptor)
            .collect(),
    }
}

fn webgpu_bind_group_entry_descriptor(
    binding: &MirKernelLaunchBinding,
) -> MirWebGpuBindGroupEntryDescriptor {
    MirWebGpuBindGroupEntryDescriptor {
        binding: binding.binding,
        kind: binding.kind,
        role: binding.role,
        access: binding.access,
        shader_access: binding.shader_access,
        shader_visibility: binding.shader_visibility,
        element_ty: binding.element_ty,
        element_layout: binding.element_layout,
        element_byte_width: binding.element_byte_width,
        element_count: binding.element_count,
        resource_index: binding.resource_index,
        binding_index: binding.binding_index,
        buffer_type: binding.buffer_binding_type,
        buffer_byte_len: binding.buffer_byte_len,
        buffer_byte_offset: binding.buffer_byte_offset,
        binding_byte_len: binding.binding_byte_len,
        min_binding_size: binding.min_binding_size,
        has_dynamic_offset: binding.has_dynamic_offset,
        source_local: binding.source_local,
        source_name: binding.source_name.clone(),
    }
}

fn webgpu_vertex_buffer_layout_descriptor(
    buffer_index: usize,
    input: &MirVertexInputReflection,
) -> MirWebGpuVertexBufferLayoutDescriptor {
    let attributes = vec![MirWebGpuVertexAttributeDescriptor {
        attribute_index: 0,
        shader_location: input.location,
        format: input.format,
        offset: input.offset_bytes,
        source_name: input.source_name.clone(),
    }];
    let attribute_indexes = attributes
        .iter()
        .map(|attribute| attribute.attribute_index)
        .collect::<Vec<_>>();
    let attribute_index_count = attribute_indexes.len();
    let step_mode = match input.step_mode {
        MirInputStepMode::Vertex => MirWebGpuVertexStepMode::Vertex,
        MirInputStepMode::Instance => MirWebGpuVertexStepMode::Instance,
    };
    MirWebGpuVertexBufferLayoutDescriptor {
        buffer_index,
        array_stride: input.stride_bytes,
        step_mode,
        attribute_indexes,
        attribute_index_count,
        attribute_count: attributes.len(),
        attributes,
        source_name: input.source_name.clone(),
    }
}

fn layout_entry_for_launch_binding(
    layout_entry_index: usize,
    binding: &MirKernelLaunchBinding,
) -> MirKernelLaunchBindGroupLayoutEntry {
    MirKernelLaunchBindGroupLayoutEntry {
        layout_entry_index,
        binding_index: binding.binding_index,
        resource_index: binding.resource_index,
        binding: binding.binding,
        buffer_byte_len: binding.buffer_byte_len,
        buffer_byte_offset: binding.buffer_byte_offset,
        binding_byte_len: binding.binding_byte_len,
        shader_visibility: binding.shader_visibility,
        buffer_binding_type: binding.buffer_binding_type,
        min_binding_size: binding.min_binding_size,
        has_dynamic_offset: binding.has_dynamic_offset,
        source_local: binding.source_local,
        source_name: binding.source_name.clone(),
    }
}

fn dispatch_size_for_1d_elements(element_count: u64) -> MirKernelDispatchSize {
    MirKernelDispatchSize {
        x: element_count,
        y: 1,
        z: 1,
    }
}

fn workgroup_count_for_dispatch(
    dispatch_size: MirKernelDispatchSize,
    workgroup: MirWorkgroupSize,
) -> Result<MirKernelWorkgroupCount, MirAbiError> {
    Ok(MirKernelWorkgroupCount {
        x: ceil_div_dimension(dispatch_size.x, workgroup.x)?,
        y: ceil_div_dimension(dispatch_size.y, workgroup.y)?,
        z: ceil_div_dimension(dispatch_size.z, workgroup.z)?,
    })
}

/// Collection-op-aware workgroup count for a dispatch grid under the
/// effective workgroup (D-A1, delivery D-W6-A1).
///
/// The single shared semantics every reflection consumer applies after
/// [`MirKernelReflection::from_signature`] via
/// [`MirKernelReflection::apply_effective_workgroup`]:
/// - **matmul**: `dispatch_size` is the tile grid (one workgroup per output
///   tile) — the count is `dispatch_size` verbatim; dividing by the tile
///   under-launches (PRP-004).
/// - **everything else** (reduction, elementwise, non-collection):
///   `ceil(dispatch_size / effective)` per dimension.
///
/// Semantics lifted from the former WGSL-local `kernel_dispatch_workgroups` /
/// `apply_effective_workgroup` pair.
#[must_use]
pub fn collection_op_workgroup_count(
    function: &MirFunction,
    dispatch_size: MirKernelDispatchSize,
    effective: MirWorkgroupSize,
) -> MirKernelWorkgroupCount {
    if kernel_has_collection_op(function, |op| matches!(op, MirCollectionOp::TensorMatMul)) {
        return MirKernelWorkgroupCount {
            x: dispatch_size.x,
            y: dispatch_size.y,
            z: dispatch_size.z,
        };
    }
    MirKernelWorkgroupCount {
        x: effective_ceil_dimension(dispatch_size.x, effective.x),
        y: effective_ceil_dimension(dispatch_size.y, effective.y),
        z: effective_ceil_dimension(dispatch_size.z, effective.z),
    }
}

/// True when the kernel body contains a collection op matching `predicate`.
///
/// Single statement scan shared by [`collection_op_contract`],
/// [`collection_op_workgroup_count`], and the target emitters, so the
/// collection-op semantic classification stays in one place.
fn kernel_has_collection_op(
    function: &MirFunction,
    mut predicate: impl FnMut(MirCollectionOp) -> bool,
) -> bool {
    for block in &function.blocks {
        for statement in &block.statements {
            let Some(op) = crate::collection_ops::statement_collection_op(statement) else {
                continue;
            };
            if predicate(op) {
                return true;
            }
        }
    }
    false
}

/// `ceil(dispatch / workgroup)` with a defensively nonzero workgroup lane
/// count (mirrors the WGSL chain-descriptor guard; the effective workgroup is
/// nonzero for every real consumer).
fn effective_ceil_dimension(dispatch: u64, workgroup: u32) -> u64 {
    dispatch.div_ceil(u64::from(workgroup.max(1)))
}

fn ceil_div_dimension(dispatch: u64, workgroup: u32) -> Result<u64, MirAbiError> {
    if workgroup == 0 {
        return Err(MirAbiError::new(ERR_KERNEL_WORKGROUP_NONZERO));
    }
    Ok(effective_ceil_dimension(dispatch, workgroup))
}

fn reflect_kernel_resource(
    resource: &MirKernelResource,
    validation: &MirValidationContext<'_>,
) -> Result<MirKernelResourceReflection, MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(resource.element_ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    let element_layout = match layout.kind {
        MirLayoutKind::Scalar(element_layout) => element_layout,
        MirLayoutKind::Atomic(atomic) => {
            return Err(MirAbiError::new(kernel_atomic_reflection_error(
                atomic.element,
            )))
        }
        _ => return Err(MirAbiError::new(ERR_KERNEL_REFLECTION_SCALAR_ELEMENT)),
    };
    let element_byte_width = kernel_element_byte_width(element_layout)?;
    let buffer_byte_len = kernel_buffer_byte_len(element_byte_width, resource.element_count)?;
    Ok(MirKernelResourceReflection {
        group: resource.group,
        binding: resource.binding,
        kind: resource.kind,
        role: resource.role,
        access: resource.access,
        element_ty: resource.element_ty,
        element_layout,
        element_byte_width,
        element_count: resource.element_count,
        buffer_byte_len,
        source_local: resource.source_local,
        source_name: resource.source_name.clone(),
        stage_io_role: ShaderStageIoRole::Other,
    })
}

fn kernel_buffer_byte_len(element_byte_width: u16, element_count: u64) -> Result<u64, MirAbiError> {
    u64::from(element_byte_width)
        .checked_mul(element_count)
        .ok_or_else(|| MirAbiError::new(ERR_KERNEL_REFLECTION_BYTE_OVERFLOW))
}

fn reflect_kernel_builtin_input(
    input: &MirKernelBuiltinInput,
    validation: &MirValidationContext<'_>,
) -> Result<MirKernelBuiltinInputReflection, MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(input.ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    let MirLayoutKind::Scalar(scalar) = layout.kind else {
        return Err(MirAbiError::new(ERR_KERNEL_BUILTIN_SCALAR_INPUT));
    };
    Ok(MirKernelBuiltinInputReflection {
        source_local: input.local,
        source_name: input.source_name.clone(),
        builtin: input.builtin,
        axis: input.builtin.axis(),
        ty: input.ty,
        layout: scalar,
    })
}

fn require_same_element_type(
    input: MirType,
    output: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    if validation
        .types
        .equals(input.semantic_id(), output.semantic_id())
    {
        Ok(())
    } else {
        Err(MirAbiError::new(ERR_KERNEL_IO_ELEMENT_MISMATCH))
    }
}

fn is_vacuum(ty: MirType, validation: &MirValidationContext<'_>) -> bool {
    matches!(
        validation.types.get(ty.semantic_id()),
        Type::Primitive(Primitive::Vacuum)
    )
}

/// The tuple-element count of a kernel **result-tuple** return (S3-A1), or
/// `None` when the return type is not a non-empty tuple. The tuple elements
/// are the multi-output contract's output resources. `pub(crate)` so the
/// decomposition seam's D-2b-1 tuple-return tests can assert the ABI
/// recognizes the synthetic's tuple form.
pub(crate) fn result_tuple_count(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Option<usize> {
    match validation.types.get(function.return_ty.semantic_id()) {
        Type::Tuple(elements) if !elements.is_empty() => Some(elements.len()),
        _ => None,
    }
}

fn require_u32_index(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    if layout.kind == MirLayoutKind::Scalar(MirScalarLayout::U32) {
        Ok(())
    } else {
        Err(MirAbiError::new(ERR_KERNEL_INDEX_U32))
    }
}

fn buffer_element_type(
    ty: MirType,
    validation: &MirValidationContext<'_>,
    allow_host_owned_tensor_entry: bool,
) -> Result<MirType, MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    match layout.kind {
        MirLayoutKind::Scalar(scalar) if crate::tensor_type::is_kernel_element_layout(scalar) => {
            Ok(ty)
        }
        MirLayoutKind::Scalar(scalar) => Err(MirAbiError::with_detail(
            format!("kernel scalar layout {}", scalar.spelling()),
            "mir_kernel_scalar_layout_unsupported",
            scalar.spelling(),
        )),
        MirLayoutKind::DeviceView(view) => {
            require_kernel_device_view(view, allow_host_owned_tensor_entry)?;
            tensor_element_type(ty, validation)
                .ok_or_else(|| MirAbiError::new(ERR_KERNEL_DEVICE_VIEW_ELEMENT))
        }
        MirLayoutKind::DeviceVector(_) => Err(MirAbiError::new(ERR_KERNEL_VECTOR_PARAM)),
        MirLayoutKind::DeviceMatrix(_) => Err(MirAbiError::new(ERR_KERNEL_MATRIX_PARAM)),
        MirLayoutKind::Atomic(atomic) => {
            Err(MirAbiError::new(kernel_atomic_param_error(atomic.element)))
        }
        MirLayoutKind::PackedNumeric(packed) => {
            Err(MirAbiError::new(packed.unsupported_execution_issue()))
        }
        MirLayoutKind::Aggregate(_) => Err(MirAbiError::new(ERR_KERNEL_AGGREGATE_LAYOUT)),
        MirLayoutKind::OpaqueHandle => Err(MirAbiError::new(ERR_KERNEL_OPAQUE_LAYOUT)),
        MirLayoutKind::Void => Err(MirAbiError::new(ERR_KERNEL_VOID_SCALAR)),
        MirLayoutKind::Never => Err(MirAbiError::new(ERR_KERNEL_NEVER_SCALAR)),
    }
}

fn device_view_element_type(
    ty: MirType,
    validation: &MirValidationContext<'_>,
    allow_host_owned_tensor_entry: bool,
) -> Result<MirType, MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    match layout.kind {
        MirLayoutKind::DeviceView(view) => {
            require_kernel_device_view(view, allow_host_owned_tensor_entry)?;
            tensor_element_type(ty, validation)
                .ok_or_else(|| MirAbiError::new(ERR_KERNEL_DEVICE_VIEW_ELEMENT))
        }
        _ => Err(MirAbiError::new(ERR_INDEXED_KERNEL_DEVICE_VIEW)),
    }
}

fn require_kernel_device_view(
    view: crate::MirDeviceViewLayout,
    allow_host_owned_tensor_entry: bool,
) -> Result<(), MirAbiError> {
    if allow_host_owned_tensor_entry
        && view.storage == crate::MirTensorStorageLayout::HostOwned
        && view.locus == crate::MirTensorLocusLayout::Host
    {
    } else if view.storage != crate::MirTensorStorageLayout::DeviceHandle
        || view.locus != crate::MirTensorLocusLayout::Device
    {
        return Err(MirAbiError::new(ERR_KERNEL_TENSOR_STORAGE_LOCUS));
    }
    if !view.row_major || !view.contiguous || view.offset_elements != 0 {
        return Err(MirAbiError::new("kernel device view layout"));
    }
    if view.rank < 1 || view.rank > 2 {
        return Err(MirAbiError::new(format!(
            "kernel device view rank {}",
            view.rank
        )));
    }
    if !crate::tensor_type::is_kernel_element_layout(view.element) {
        return Err(MirAbiError::with_detail(
            format!("kernel device view element {}", view.element.spelling()),
            "mir_kernel_device_view_element_unsupported",
            view.element.spelling(),
        ));
    }
    Ok(())
}

fn buffer_element_count(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<u64, MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    match layout.kind {
        MirLayoutKind::Scalar(scalar) if crate::tensor_type::is_kernel_element_layout(scalar) => {
            Ok(1)
        }
        MirLayoutKind::Scalar(scalar) => Err(MirAbiError::new(format!(
            "kernel scalar layout {}",
            scalar.spelling()
        ))),
        MirLayoutKind::DeviceView(_) => device_view_element_count(ty, validation),
        MirLayoutKind::DeviceVector(_) => Err(MirAbiError::new(ERR_KERNEL_VECTOR_ELEMENT_COUNT)),
        MirLayoutKind::DeviceMatrix(_) => Err(MirAbiError::new(ERR_KERNEL_MATRIX_PARAM)),
        MirLayoutKind::Atomic(atomic) => Err(MirAbiError::new(kernel_atomic_element_count_error(
            atomic.element,
        ))),
        MirLayoutKind::PackedNumeric(packed) => {
            Err(MirAbiError::new(packed.unsupported_execution_issue()))
        }
        MirLayoutKind::Aggregate(_) => Err(MirAbiError::new(ERR_KERNEL_AGGREGATE_LAYOUT)),
        MirLayoutKind::OpaqueHandle => Err(MirAbiError::new(ERR_KERNEL_OPAQUE_LAYOUT)),
        MirLayoutKind::Void => Err(MirAbiError::new(ERR_KERNEL_VOID_SCALAR)),
        MirLayoutKind::Never => Err(MirAbiError::new(ERR_KERNEL_NEVER_SCALAR)),
    }
}

/// Fail closed with a single stable shape when any kernel param or return is a
/// register matrix (storage-buffer ABI does not map `DeviceMatrix` IO).
///
/// Unit B ratchet: shared by WGSL/Metal storage-buffer kernels and llvm-text
/// `@nucleum` emit so matrix kernel IO is never fail-open on a staging target.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`MirAbiError`].
pub fn reject_kernel_matrix_abi(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    for param in &function.params {
        if layout_is_device_matrix(param.ty, validation) {
            return Err(MirAbiError::new(ERR_KERNEL_MATRIX_PARAM));
        }
    }
    if layout_is_device_matrix(function.return_ty, validation) {
        return Err(MirAbiError::new(ERR_KERNEL_MATRIX_PARAM));
    }
    Ok(())
}

fn layout_is_device_matrix(ty: MirType, validation: &MirValidationContext<'_>) -> bool {
    matches!(
        validation
            .layouts
            .layout_for_type(ty)
            .map(|layout| layout.kind),
        Ok(MirLayoutKind::DeviceMatrix(_))
    )
}

fn device_view_element_count(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<u64, MirAbiError> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(_, shape) => device_view_element_count_from_shape(*shape, validation),
        Type::Alias(_, target) => device_view_element_count(MirType::semantic(*target), validation),
        _ => Err(MirAbiError::new(ERR_KERNEL_DEVICE_VIEW_ELEMENT_COUNT)),
    }
}

fn device_view_element_count_from_shape(
    shape: radix_types::IndexId,
    validation: &MirValidationContext<'_>,
) -> Result<u64, MirAbiError> {
    match validation.types.get_index(shape) {
        IndexExpr::Literal(count) => Ok(*count),
        IndexExpr::Tuple(dims) => {
            let mut product: u64 = 1;
            for &dim in dims {
                match validation.types.get_index(dim) {
                    IndexExpr::Literal(count) => product = product.saturating_mul(*count),
                    _ => {
                        return Err(MirAbiError::new(
                            "kernel device view element count must be literal dimensions",
                        ))
                    }
                }
            }
            Ok(product)
        }
        _ => Err(MirAbiError::new(
            "kernel device view element count must be literal dimensions",
        )),
    }
}

fn tensor_element_type(ty: MirType, validation: &MirValidationContext<'_>) -> Option<MirType> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(element, _) => Some(MirType::semantic(*element)),
        Type::Alias(_, target) => tensor_element_type(MirType::semantic(*target), validation),
        _ => None,
    }
}

fn kernel_element_byte_width(layout: MirScalarLayout) -> Result<u16, MirAbiError> {
    if !crate::tensor_type::is_kernel_element_layout(layout) {
        return Err(MirAbiError::new(format!(
            "kernel scalar layout {}",
            layout.spelling()
        )));
    }
    let size_bits = layout.size_bits();
    if !size_bits.is_multiple_of(8) {
        return Err(MirAbiError::new(format!(
            "kernel scalar layout {} is not byte-sized",
            layout.spelling()
        )));
    }
    Ok(size_bits / 8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MirBlock, MirBlockId, MirIntrinsic, MirLocal, MirOperand, MirParam, MirParamMode, MirPlace,
        MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind,
    };
    use radix_lexer::Span;
    use radix_types::TypeTable;

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
        let _u32 = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
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
        let element_ty =
            MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32));
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
        let tuple_return_ty =
            MirType::semantic(types.intern(Type::Tuple(vec![tensor_id, tensor_id])));
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
        });
        assert_eq!(signature.accumulation_buffers.len(), 1);
        assert_eq!(signature.accumulation_buffers[0].binding, 4);

        // resources() sees both inputs + both outputs + the accumulation
        // buffer (five storage buffers in binding order).
        assert_eq!(signature.resources().count(), 5);

        // The plan pass accepts the multi-output signature (elementwise body
        // → no recipe, never an error).
        let plan = crate::device_program_plans::kernel_plan_for_function(
            &function,
            &signature,
            &validation,
        )
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
        let tuple_return_ty =
            MirType::semantic(types.intern(Type::Tuple(vec![tensor_id, tensor_id])));
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
        // so no kernel value carries an aggregate type — the Metal aggregate
        // rejection (radix-mir-metal lib.rs:3066/:3110) is unreachable
        // through the multi-output ABI.
        let mut types = TypeTable::new();
        let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
        let element_ty = MirType::semantic(f32);
        let shape = types.intern_index(IndexExpr::Literal(4));
        let tensor_id = types.tensor_with_shape(f32, shape);
        let tensor_ty = MirType::semantic(tensor_id);
        let tuple_return_ty =
            MirType::semantic(types.intern(Type::Tuple(vec![tensor_id, tensor_id])));
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
        let derived = crate::device_program_plans::kernel_plan_for_function(
            &function,
            &signature,
            &validation,
        )
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
        let element_ty =
            MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32));
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
        let derived = crate::device_program_plans::kernel_plan_for_function(
            &function,
            &signature,
            &validation,
        )
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
        let derived = crate::device_program_plans::kernel_plan_for_function(
            &function,
            &signature,
            &validation,
        )
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
}
