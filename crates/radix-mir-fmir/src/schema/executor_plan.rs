//! Wire mirrors for the separately versioned executor-plan section.

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
