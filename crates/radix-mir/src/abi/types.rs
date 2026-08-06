use crate::{
    shader_contract::ShaderStageIoRole, MirFunction, MirFunctionId, MirIntrinsic, MirLocalId,
    MirRuntimeCall, MirType,
};
use radix_hir::DefId;
use serde::Serialize;

use super::{
    MirGraphicsPipelineReflection, MirKernelBuiltin, MirKernelBuiltinInput,
    MirKernelBuiltinInputReflection, MirKernelDispatchSize, MirKernelResource,
    MirKernelResourceReflection, MirKernelShaderStage, MirKernelWorkgroupCount,
    MirVertexInputFormat, MirWorkgroupSize,
};

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

/// The maximum number of storage-buffer bindings ONE kernel may declare —
/// the Metal `[[buffer(N)]]` attribute maximum (bindings `0..=30`,
/// inclusive). A device-resident body whose whole-function storage-buffer
/// signature would exceed this cannot be emitted as one kernel: the ABI
/// signature synthesis refuses it (fail-closed, so the callers fall through
/// to the decomposition split) and the plan pass reports the same typed
/// diagnostic for pre-built signatures.
pub const MAX_STORAGE_BUFFERS_PER_KERNEL: usize = 31;

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
