use crate::{MirLocalId, MirScalarLayout, MirType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::{
    ceil_div_dimension, MirAbiError, MirFragmentOutputReflection, MirInputStepMode,
    MirKernelBufferBindingType, MirKernelBuiltin, MirKernelBuiltinInputReflection,
    MirKernelDispatchSize, MirKernelReflection, MirKernelResourceAccess, MirKernelResourceKind,
    MirKernelResourceReflection, MirKernelResourceRole, MirKernelShaderAccess,
    MirKernelWorkgroupCount, MirVaryingReflection, MirVertexInputReflection, MirWorkgroupSize,
    ERR_FRAGMENT_OUTPUT_DUPLICATE_LOCATION, ERR_VARYING_DUPLICATE_LOCATION,
    ERR_VARYING_FRAGMENT_WITHOUT_VERTEX, ERR_VARYING_STAGE_MISMATCH,
    ERR_VERTEX_INPUT_DUPLICATE_LOCATION,
};

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
pub(crate) fn validate_vertex_input_locations(
    inputs: &[MirVertexInputReflection],
) -> Result<(), MirAbiError> {
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

pub(crate) fn validate_varying_locations(
    varyings: &[MirVaryingReflection],
) -> Result<(), MirAbiError> {
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

pub(crate) fn validate_fragment_output_locations(
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

pub(crate) fn pipeline_layout_for_launch_bind_groups(
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

pub(crate) fn layout_entry_for_launch_binding(
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

pub(crate) fn dispatch_size_for_1d_elements(element_count: u64) -> MirKernelDispatchSize {
    MirKernelDispatchSize {
        x: element_count,
        y: 1,
        z: 1,
    }
}

pub(crate) fn workgroup_count_for_dispatch(
    dispatch_size: MirKernelDispatchSize,
    workgroup: MirWorkgroupSize,
) -> Result<MirKernelWorkgroupCount, MirAbiError> {
    Ok(MirKernelWorkgroupCount {
        x: ceil_div_dimension(dispatch_size.x, workgroup.x)?,
        y: ceil_div_dimension(dispatch_size.y, workgroup.y)?,
        z: ceil_div_dimension(dispatch_size.z, workgroup.z)?,
    })
}
