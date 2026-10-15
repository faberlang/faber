use crate::{
    MirFunction, MirFunctionId, MirIntrinsic, MirLocalId, MirRuntimeCall, MirType,
    shader_contract::ShaderStageIoRole,
};
use radix_hir::DefId;
use serde::{Deserialize, Serialize};

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
pub enum MirRuntimeAbiKind {
    CoreSemantics,
    HostIntegration,
    ProbeScaffolding,
    IntentionalOpaqueRuntime,
}

impl MirRuntimeAbiKind {
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
    RuntimeDeviceBlock,
    RuntimeGpuBuiltin,
    RuntimeCursorStream,
    RuntimeGradient,
    RuntimeTypeCheck,
    RuntimeTensorShapeCheck,
    RuntimeTensorShapeSize,
    RuntimeTensorLimes,
    RuntimeFilumReduce,
    RuntimeDuplicate,
    RuntimeTensorLift,
    RuntimeVectorOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirRuntimeAbi {
    pub family: MirRuntimeAbiFamily,
    pub category: MirRuntimeAbiKind,
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
                MirRuntimeAbiKind::HostIntegration,
                true,
            ),
            MirIntrinsic::Assert => (
                MirRuntimeAbiFamily::RuntimeAssert,
                MirRuntimeAbiKind::CoreSemantics,
                true,
            ),
            MirIntrinsic::FormatString { .. } | MirIntrinsic::FormatValue { .. } => (
                MirRuntimeAbiFamily::RuntimeFormat,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            MirIntrinsic::Convert(_) => (
                MirRuntimeAbiFamily::RuntimeConvert,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            MirIntrinsic::Collection(_) => (
                MirRuntimeAbiFamily::RuntimeCollection,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            MirIntrinsic::Atomic(op) => (
                MirRuntimeAbiFamily::RuntimeAtomic,
                MirRuntimeAbiKind::CoreSemantics,
                matches!(op, crate::MirAtomicOp::Store),
            ),
            MirIntrinsic::Panic => (
                MirRuntimeAbiFamily::RuntimePanic,
                MirRuntimeAbiKind::CoreSemantics,
                true,
            ),
            MirIntrinsic::Provider(_) => (
                MirRuntimeAbiFamily::RuntimeProvider,
                MirRuntimeAbiKind::HostIntegration,
                false,
            ),
            MirIntrinsic::SermoOpen | MirIntrinsic::SermoSetOpener | MirIntrinsic::Sermo(_) => (
                MirRuntimeAbiFamily::RuntimeSermo,
                MirRuntimeAbiKind::HostIntegration,
                false,
            ),
            MirIntrinsic::ReadLine | MirIntrinsic::ReadAll => (
                MirRuntimeAbiFamily::RuntimeReadLine,
                MirRuntimeAbiKind::HostIntegration,
                false,
            ),
            MirIntrinsic::Cede => (
                MirRuntimeAbiFamily::RuntimeCede,
                MirRuntimeAbiKind::HostIntegration,
                false,
            ),
            MirIntrinsic::DeviceBlock(_) => (
                MirRuntimeAbiFamily::RuntimeDeviceBlock,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            MirIntrinsic::GpuBuiltin(_) => (
                MirRuntimeAbiFamily::RuntimeGpuBuiltin,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            MirIntrinsic::CursorStream(_) => (
                MirRuntimeAbiFamily::RuntimeCursorStream,
                MirRuntimeAbiKind::HostIntegration,
                false,
            ),
            MirIntrinsic::Gradient(_) => (
                MirRuntimeAbiFamily::RuntimeGradient,
                MirRuntimeAbiKind::HostIntegration,
                false,
            ),
            MirIntrinsic::TypeCheck(_) => (
                MirRuntimeAbiFamily::RuntimeTypeCheck,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            MirIntrinsic::TensorShapeCheck(_) => (
                MirRuntimeAbiFamily::RuntimeTensorShapeCheck,
                MirRuntimeAbiKind::CoreSemantics,
                true,
            ),
            MirIntrinsic::TensorShapeSize(_) => (
                MirRuntimeAbiFamily::RuntimeTensorShapeSize,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            MirIntrinsic::TensorLimes(_) => (
                MirRuntimeAbiFamily::RuntimeTensorLimes,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            // SFR-5: the filum sum declaration is core semantics (the plan
            // pass resolves it); host lanes reject it by target policy.
            MirIntrinsic::FilumReduce(_) => (
                MirRuntimeAbiFamily::RuntimeFilumReduce,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            // `copy` marker duplication is core value semantics.
            MirIntrinsic::Duplicate => (
                MirRuntimeAbiFamily::RuntimeDuplicate,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            // K4: elementwise operator lift over tensors; the runner is the
            // reference, other lanes reject it.
            MirIntrinsic::TensorLift(_) => (
                MirRuntimeAbiFamily::RuntimeTensorLift,
                MirRuntimeAbiKind::CoreSemantics,
                false,
            ),
            // A deferred vector op is expanded before any executor runs;
            // validation rejects a survivor, so no lane dispatches this
            // family.
            MirIntrinsic::VectorOp(_) => (
                MirRuntimeAbiFamily::RuntimeVectorOp,
                MirRuntimeAbiKind::CoreSemantics,
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

/// Backend-neutral storage-buffer binding capacity for one kernel.
///
/// The shared ABI consumes this record rather than teaching every lane one
/// limit. The default is the conservative minimum across admitted lanes;
/// lane-owned records may carry a higher capacity when that lane admits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageBufferLimitRecord {
    /// Human-readable lane identity for typed diagnostics.
    pub lane: &'static str,
    /// Maximum number of storage-buffer bindings in one kernel.
    pub max_storage_buffers: usize,
}

impl StorageBufferLimitRecord {
    /// Construct a lane-owned storage-buffer limit record.
    #[must_use]
    pub const fn new(lane: &'static str, max_storage_buffers: usize) -> Self {
        Self {
            lane,
            max_storage_buffers,
        }
    }

    /// Whether a signature with `buffer_count` storage buffers fits this lane.
    #[must_use]
    pub const fn admits(self, buffer_count: usize) -> bool {
        buffer_count <= self.max_storage_buffers
    }

    /// Phrase used by diagnostics while preserving the established default
    /// Metal/conservative-minimum wording byte-for-byte.
    #[must_use]
    pub fn binding_limit_phrase(self) -> String {
        if self == DEFAULT_STORAGE_BUFFER_LIMIT || self == METAL_STORAGE_BUFFER_LIMIT {
            format!("{}-buffer binding maximum", self.max_storage_buffers)
        } else {
            format!(
                "{}-buffer binding maximum for {} lane",
                self.max_storage_buffers, self.lane
            )
        }
    }
}

/// Metal's `[[buffer(N)]]` attribute maximum (bindings `0..=30`, inclusive).
/// This record is lane-owned; it is not the shared default by itself.
pub const METAL_STORAGE_BUFFER_LIMIT: StorageBufferLimitRecord =
    StorageBufferLimitRecord::new("Metal", 31);

/// Conservative shared default: split at the minimum capacity across admitted
/// lanes until a caller selects a lane-owned record.
pub const DEFAULT_STORAGE_BUFFER_LIMIT: StorageBufferLimitRecord =
    StorageBufferLimitRecord::new("conservative minimum across admitted lanes", 31);

/// Compatibility name for the former ceiling. Shared consumers use the
/// record's `max_storage_buffers` field or pass a lane-owned record instead.
pub const MAX_STORAGE_BUFFERS_PER_KERNEL: StorageBufferLimitRecord = DEFAULT_STORAGE_BUFFER_LIMIT;

/// Build the storage-buffer limit diagnostic. The default Metal behavior keeps
/// its established message; a lane-owned record with another capacity names
/// the active lane and limit in the message and detail.
pub(crate) fn storage_buffer_limit_error(
    buffer_count: usize,
    limit: StorageBufferLimitRecord,
) -> crate::abi::MirAbiError {
    if limit == DEFAULT_STORAGE_BUFFER_LIMIT || limit == METAL_STORAGE_BUFFER_LIMIT {
        return crate::abi::MirAbiError::new(format!(
            "kernel storage-buffer count {buffer_count} exceeds the {}-buffer binding maximum; \
             the decomposition must split the body",
            limit.max_storage_buffers
        ));
    }
    crate::abi::MirAbiError::with_detail(
        format!(
            "kernel storage-buffer count {buffer_count} exceeds the {}; \
             the decomposition must split the body",
            limit.binding_limit_phrase()
        ),
        "mir_kernel_storage_buffer_limit",
        format!(
            "lane={} max_storage_buffers={}",
            limit.lane, limit.max_storage_buffers
        ),
    )
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
    /// A workgroup-size builtin is referenced. Each lane maps this fact to
    /// its own invocation-header syntax; for example, one lane declares a
    /// threadgroup-size parameter while another emits a workgroup constant.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirGpuReflection {
    pub kernels: Vec<MirKernelReflection>,
    pub pipeline: Option<MirGraphicsPipelineReflection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirKernelReflection {
    pub function: MirFunctionId,
    pub provenance: MirKernelProvenance,
    pub entry_name: String,
    pub shader_stage: MirKernelShaderStage,
    pub resources: Vec<MirKernelResourceReflection>,
    /// Opaque library-typed resources bound to a stage parameter. Omitted
    /// from serialized reflection when empty so existing reflection JSON is
    /// byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bound_resources: Vec<MirBoundResourceReflection>,
    pub vertex_inputs: Vec<MirVertexInputReflection>,
    pub varyings: Vec<MirVaryingReflection>,
    pub fragment_outputs: Vec<MirFragmentOutputReflection>,
    pub builtin_inputs: Vec<MirKernelBuiltinInputReflection>,
    pub called_builtins: Vec<MirKernelBuiltin>,
    pub workgroup: MirWorkgroupSize,
    pub dispatch_size: MirKernelDispatchSize,
    pub workgroup_count: MirKernelWorkgroupCount,
    /// The kernel's device fault sites (gpu-reset U9-2), ids dense from 0: one
    /// slot of the per-launch fault record each. Non-empty exactly when the
    /// resource list carries a `FaultRecord` binding. Omitted from serialized
    /// reflection when empty so existing reflection JSON is byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fault_sites: Vec<MirFaultSite>,
}

/// What a device fault site does when it fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirFaultKind {
    /// Kills the program with the device kernel fault error.
    Fatal,
    /// Delivers `text` as a value a handler can route.
    Recoverable,
}

/// One fault site of a kernel: `id` is the slot in the launch's fault record
/// (the number the device passes to its atomic min); `text` is the constant
/// message the host reports for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirFaultSite {
    pub id: u32,
    pub kind: MirFaultKind,
    pub text: String,
}

/// The text of the index bounds fault site.
pub const INDEX_FAULT_TEXT: &str = "index out of range";

/// A bound resource whose type is an opaque library type: the group/binding
/// slot, the stage-parameter name, and the bound type's name as the library
/// declared it. The compiler attaches no meaning to `bound_type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirBoundResourceReflection {
    pub group: u32,
    pub binding: u32,
    pub name: String,
    pub bound_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirKernelProvenance {
    MirFunction(MirFunctionId),
    SourceFunction(DefId),
}

/// Vertex input step mode: per-vertex or per-instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirVaryingReflection {
    pub source_name: String,
    pub location: u32,
    pub format: MirVertexInputFormat,
    pub interpolation: MirVaryingInterpolation,
    /// Explicit stage-I/O role (see [`crate::shader_contract::ShaderStageIoRole`]).
    pub role: ShaderStageIoRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirVaryingInterpolation {
    Perspective,
    Linear,
    Flat,
}

/// Fragment render-target output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirFragmentOutputReflection {
    pub location: u32,
    pub format: MirVertexInputFormat,
    /// Explicit stage-I/O role (see [`crate::shader_contract::ShaderStageIoRole`]).
    pub role: ShaderStageIoRole,
}
