//! Wire mirrors for the separately versioned executor-plan section.

use super::wire::{
    WireBinding, WireBufferLifetime, WireBufferRole, WireInitializationPolicy, WireStorageLayout,
};
use serde::{Deserialize, Serialize};

/// One executor-plan selection row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireExecutorPlan {
    /// Ordered kernel/schedule index.
    pub kernel_index: u32,
    /// Optional library entry; `None` means generated fallback.
    pub library_entry: Option<String>,
    /// Logical signature independent of emitted syntax.
    pub logical_signature: WireLogicalSignature,
    /// Primary launch bind, including layout and grid.
    pub primary_bind: WireBindDescriptor,
    /// Named buffer binds.
    pub binds: Vec<WireLibraryBind>,
    /// Uniform values/facts.
    pub uniforms: Vec<WireUniformBinding>,
    /// Explicit offsets and strides.
    pub offsets: Vec<WireOffsetBinding>,
    /// Optional scratch workspace.
    pub workspace: Option<WireWorkspaceBinding>,
    /// Backend capability requirements.
    pub backend_requirements: Vec<WireBackendRequirement>,
    /// Recognition provenance.
    pub provenance: WirePlanProvenance,
}

/// Logical library signature mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireLogicalSignature {
    /// Stable operation name.
    pub operation: String,
    /// Logical inputs.
    pub inputs: Vec<String>,
    /// Logical outputs.
    pub outputs: Vec<String>,
}

/// Primary dimensions/strides/layout/grid bind mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBindDescriptor {
    /// Logical dimensions.
    pub dims: Vec<u64>,
    /// Physical strides.
    pub strides: Vec<u64>,
    /// Closed layout-family spelling.
    pub layout: String,
    /// Workgroup grid.
    pub grid: [u64; 3],
}

/// Named buffer bind mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireLibraryBind {
    /// Logical view name.
    pub name: String,
    /// Binding group.
    pub group: u32,
    /// Binding slot.
    pub binding: u32,
    /// Logical dimensions.
    pub dims: Vec<u64>,
    /// Physical strides.
    pub strides: Vec<u64>,
    /// Base element offset.
    pub offset_elements: u64,
}

/// Uniform mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireUniformBinding {
    /// Uniform name.
    pub name: String,
    /// Closed scalar type spelling.
    pub ty: String,
    /// Raw scalar bits.
    pub bits: u32,
}

/// Offset/stride mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireOffsetBinding {
    /// Logical view name.
    pub name: String,
    /// Base element offset.
    pub offset_elements: u64,
    /// Physical strides.
    pub strides: Vec<u64>,
}

/// Workspace mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireWorkspaceBinding {
    /// Required byte size.
    pub bytes: u64,
    /// Required alignment.
    pub alignment: u32,
}

/// Backend requirement mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBackendRequirement {
    /// Backend identity.
    pub backend: String,
    /// Capability/feature identity.
    pub feature: String,
}

/// Provenance mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WirePlanProvenance {
    /// Compiler pass name.
    pub pass: String,
    /// Recognition-rule revision.
    pub revision: u32,
    /// Canonical graph node ids.
    pub source_nodes: Vec<u32>,
}

impl WireExecutorPlan {
    /// Project one target-neutral selection to the wire mirror.
    #[must_use]
    pub fn from_mir(selection: &radix_mir::LibrarySelection) -> Self {
        Self {
            kernel_index: u32::try_from(selection.kernel_index).unwrap_or(u32::MAX),
            library_entry: selection.library_entry.clone(),
            logical_signature: WireLogicalSignature {
                operation: selection.logical_signature.operation.clone(),
                inputs: selection.logical_signature.inputs.clone(),
                outputs: selection.logical_signature.outputs.clone(),
            },
            primary_bind: WireBindDescriptor {
                dims: selection.bind.dims.clone(),
                strides: selection.bind.strides.clone(),
                layout: format!("{:?}", selection.bind.layout),
                grid: [
                    selection.bind.grid.x,
                    selection.bind.grid.y,
                    selection.bind.grid.z,
                ],
            },
            binds: selection
                .binds
                .iter()
                .map(|bind| WireLibraryBind {
                    name: bind.name.clone(),
                    group: bind.binding.group,
                    binding: bind.binding.binding,
                    dims: bind.dims.clone(),
                    strides: bind.strides.clone(),
                    offset_elements: bind.offset_elements,
                })
                .collect(),
            uniforms: selection
                .uniforms
                .iter()
                .map(|uniform| WireUniformBinding {
                    name: uniform.name.clone(),
                    ty: format!("{:?}", uniform.ty),
                    bits: uniform.bits,
                })
                .collect(),
            offsets: selection
                .offsets
                .iter()
                .map(|offset| WireOffsetBinding {
                    name: offset.name.clone(),
                    offset_elements: offset.offset_elements,
                    strides: offset.strides.clone(),
                })
                .collect(),
            workspace: selection.workspace.map(|workspace| WireWorkspaceBinding {
                bytes: workspace.bytes,
                alignment: workspace.alignment,
            }),
            backend_requirements: selection
                .backend_requirements
                .iter()
                .map(|requirement| WireBackendRequirement {
                    backend: requirement.backend.clone(),
                    feature: requirement.feature.clone(),
                })
                .collect(),
            provenance: WirePlanProvenance {
                pass: selection.provenance.pass.clone(),
                revision: selection.provenance.revision,
                source_nodes: selection.provenance.source_nodes.clone(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Executor-plan v2 (KV-B)
// ---------------------------------------------------------------------------

/// The executor-plan ABI version that carries runtime binding facts.
///
/// This is the producer payload version from
/// `radix_mir::EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION`. The outer FMIR
/// section carrier has its own ratchet, owned by A4V. A v1
/// [`WireExecutorPlan`] remains a legacy static plan; its constant bits are
/// never decoded as a runtime cursor.
pub const EXECUTOR_PLAN_V2_VERSION: u16 = 2;
/// Outer FMIR executor-plan section version paired with the v2 payload.
pub const EXECUTOR_PLAN_SECTION_V2_VERSION: u32 = 2;

/// One typed invocation-state field that may drive a v2 binding.
///
/// `Unknown` is representable so a decoder can preserve the delivered payload
/// long enough for the focused admission helper to reject it. It is not a
/// compatibility arm and must never be lowered to a guessed cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireRuntimeSource {
    /// Absolute position of the current invocation.
    Position,
    /// Valid K/V length after the append for the current invocation.
    ValidLenAfter,
    /// Number of query rows in the current invocation.
    QueryRows,
    /// Monotonic sequence/reset epoch.
    SequenceEpoch,
    /// A source name not known by this reader.
    Unknown(String),
}

/// A source tag for a v2 launch binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireExecutorPlanSource {
    /// A compile-time scalar value.
    Constant {
        /// The source's scalar type spelling.
        ty: String,
        /// Raw scalar bits.
        bits: u32,
    },
    /// A value read from the typed invocation-state buffer.
    Runtime {
        /// Invocation-state field selected by this source.
        source: WireRuntimeSource,
    },
}

/// One fixed-capacity device allocation used by a v2 executor plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireAllocationIdentity {
    /// Program-local allocation identity.
    pub id: u32,
    /// Logical allocation name.
    pub name: String,
    /// Program-level role.
    pub role: WireBufferRole,
    /// Storage residency.
    pub storage: WireStorageLayout,
    /// Allocation lifetime.
    pub lifetime: WireBufferLifetime,
}

/// Typed fixed-capacity storage facts for one allocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireAllocationFact {
    /// Stable program-level buffer identity.
    pub buffer: WireAllocationIdentity,
    /// Element dtype spelling.
    pub dtype: String,
    /// Physical allocation capacity in bytes, distinct from every view span.
    pub capacity_bytes: u64,
    /// Repeated allocation lifetime fact.
    pub lifetime: WireBufferLifetime,
    /// How the allocation reaches its first defined state.
    pub initialization: WireInitializationPolicy,
}

/// A bounded logical view over an allocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireViewFact {
    /// Allocation identity shared by this view and sibling views.
    pub allocation: u32,
    /// Logical dimensions in outer-to-inner order.
    pub logical_dims: Vec<u64>,
    /// Element strides in the same order as [`Self::logical_dims`].
    pub strides: Vec<u64>,
    /// Static base element offset in the allocation.
    pub static_base: u64,
    /// Maximum byte span from the static base, not allocation capacity.
    pub maximum_span: u64,
}

/// Compatibility aliases for consumers that name these records by their
/// allocation/view nouns rather than their producer `Fact` suffix.
pub type WireAllocation = WireAllocationFact;
pub type WireAllocationView = WireViewFact;

/// One v2 launch binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireLaunchBinding {
    /// Kernel selection receiving this binding.
    pub kernel_index: u32,
    /// Allocation handle/identity bound by the launch.
    pub handle: u32,
    /// Target-neutral binding index.
    pub binding: WireBinding,
    /// Byte offset from the allocation base.
    pub byte_offset: u64,
    /// Byte span exposed to this launch.
    pub view_span: u64,
    /// Typed constant or invocation-state source for the binding.
    pub runtime_source: WireExecutorPlanSource,
}

/// The FMIR mirror of `radix_mir::ExecutorPlanExtensionV2`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireExecutorPlanV2 {
    /// V2 producer payload version.
    pub abi_version: u16,
    /// The same ordered library/fallback selections as the static plan.
    pub selections: Vec<WireExecutorPlan>,
    /// Allocation facts shared by all views and launches.
    pub allocations: Vec<WireAllocationFact>,
    /// Bounded logical views over those allocations.
    pub views: Vec<WireViewFact>,
    /// Launch bindings carrying byte ranges and tagged sources.
    pub launch_bindings: Vec<WireLaunchBinding>,
}

impl WireExecutorPlanV2 {
    /// Project the typed v2 producer payload onto the FMIR mirror.
    #[must_use]
    pub fn from_mir(plan: &radix_mir::device_program_plans::ExecutorPlanExtensionV2) -> Self {
        Self {
            abi_version: plan.abi_version,
            selections: plan
                .selections
                .iter()
                .map(WireExecutorPlan::from_mir)
                .collect(),
            allocations: plan.allocations.iter().map(wire_allocation).collect(),
            views: plan.views.iter().map(wire_view).collect(),
            launch_bindings: plan
                .launch_bindings
                .iter()
                .map(wire_launch_binding)
                .collect(),
        }
    }
}

fn wire_allocation(
    allocation: &radix_mir::device_program_plans::AllocationFact,
) -> WireAllocationFact {
    WireAllocationFact {
        buffer: WireAllocationIdentity {
            id: allocation.buffer.id.0,
            name: allocation.buffer.name.clone(),
            role: wire_buffer_role(allocation.buffer.role),
            storage: wire_storage_layout(allocation.buffer.storage),
            lifetime: wire_buffer_lifetime(allocation.buffer.lifetime),
        },
        dtype: format!("{:?}", allocation.dtype),
        capacity_bytes: allocation.capacity_bytes,
        lifetime: wire_buffer_lifetime(allocation.lifetime),
        initialization: wire_initialization(allocation.initialization),
    }
}

fn wire_view(view: &radix_mir::device_program_plans::ViewFact) -> WireViewFact {
    WireViewFact {
        allocation: view.allocation.0,
        logical_dims: view.logical_dims.clone(),
        strides: view.strides.clone(),
        static_base: view.static_base,
        maximum_span: view.maximum_span,
    }
}

fn wire_launch_binding(
    binding: &radix_mir::device_program_plans::LaunchBinding,
) -> WireLaunchBinding {
    WireLaunchBinding {
        kernel_index: u32::try_from(binding.kernel_index).unwrap_or(u32::MAX),
        handle: binding.handle.0,
        binding: WireBinding {
            group: binding.binding.group,
            binding: binding.binding.binding,
        },
        byte_offset: binding.byte_offset,
        view_span: binding.view_span,
        runtime_source: wire_source(binding.runtime_source),
    }
}

fn wire_source(
    source: radix_mir::device_program_plans::ExecutorPlanSource,
) -> WireExecutorPlanSource {
    match source {
        radix_mir::device_program_plans::ExecutorPlanSource::Constant { ty, bits } => {
            WireExecutorPlanSource::Constant {
                ty: format!("{:?}", ty),
                bits,
            }
        }
        radix_mir::device_program_plans::ExecutorPlanSource::Runtime(source) => {
            WireExecutorPlanSource::Runtime {
                source: wire_runtime_source(source),
            }
        }
    }
}

fn wire_runtime_source(
    source: radix_mir::device_program_plans::RuntimeSource,
) -> WireRuntimeSource {
    match source {
        radix_mir::device_program_plans::RuntimeSource::Position => WireRuntimeSource::Position,
        radix_mir::device_program_plans::RuntimeSource::ValidLenAfter => {
            WireRuntimeSource::ValidLenAfter
        }
        radix_mir::device_program_plans::RuntimeSource::QueryRows => WireRuntimeSource::QueryRows,
        radix_mir::device_program_plans::RuntimeSource::SequenceEpoch => {
            WireRuntimeSource::SequenceEpoch
        }
    }
}

fn wire_buffer_role(role: radix_mir::BufferRole) -> WireBufferRole {
    match role {
        radix_mir::BufferRole::Input => WireBufferRole::Input,
        radix_mir::BufferRole::Output => WireBufferRole::Output,
        radix_mir::BufferRole::InOut => WireBufferRole::InOut,
    }
}

fn wire_storage_layout(storage: radix_mir::MirTensorStorageLayout) -> WireStorageLayout {
    match storage {
        radix_mir::MirTensorStorageLayout::HostOwned => WireStorageLayout::HostOwned,
        radix_mir::MirTensorStorageLayout::DeviceHandle => WireStorageLayout::DeviceHandle,
    }
}

fn wire_buffer_lifetime(lifetime: radix_mir::BufferLifetime) -> WireBufferLifetime {
    match lifetime {
        radix_mir::BufferLifetime::PerProgram => WireBufferLifetime::PerProgram,
        radix_mir::BufferLifetime::PerStep => WireBufferLifetime::PerStep,
        radix_mir::BufferLifetime::ObservationPoint => WireBufferLifetime::ObservationPoint,
    }
}

fn wire_initialization(
    initialization: radix_mir::device_semantics::InitializationPolicy,
) -> WireInitializationPolicy {
    match initialization {
        radix_mir::device_semantics::InitializationPolicy::ZeroFill => {
            WireInitializationPolicy::ZeroFill
        }
        radix_mir::device_semantics::InitializationPolicy::HostProvided => {
            WireInitializationPolicy::HostProvided
        }
        radix_mir::device_semantics::InitializationPolicy::KernelInitialized => {
            WireInitializationPolicy::KernelInitialized
        }
    }
}
