//! GPU reflection JSON sidecar serialization for `radix emit --reflection`.
//!
//! Owns [`output_reflection_json`] which builds a schema-versioned JSON payload
//! from `MIR` GPU reflection data for Metal and WGSL backends. Delegates
//! reflection data to `crate::mir::MirGpuReflection` types and serialization to
//! `serde_json`. Returns `Ok(None)` for targets without reflection metadata.

use serde::Serialize;

const GPU_REFLECTION_SCHEMA_VERSION: u32 = 1;

/// Serialize the GPU reflection sidecar from compiler output, if the target exposes one.
///
/// Returns `Ok(None)` for targets that do not carry GPU reflection data.
///
/// # Errors
///
/// Returns a `serde_json::Error` if the reflection payload cannot be serialized.
pub fn output_reflection_json(output: &crate::Output) -> Result<Option<String>, serde_json::Error> {
    #[cfg(not(any(feature = "mir-metal", feature = "mir-wgsl")))]
    {
        drop(output);
        Ok(None)
    }
    #[cfg(any(feature = "mir-metal", feature = "mir-wgsl"))]
    {
        let (target, reflection) = match output {
            #[cfg(feature = "mir-metal")]
            crate::Output::MetalText(out) => match out.reflection.as_ref() {
                Some(reflection) => ("metal-text", reflection),
                None => return Ok(None),
            },
            #[cfg(feature = "mir-wgsl")]
            crate::Output::WgslText(out) => match out.reflection.as_ref() {
                Some(reflection) => ("wgsl-text", reflection),
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        serde_json::to_string_pretty(&GpuReflectionJson::from_reflection(target, reflection)).map(Some)
    }
}

#[derive(Serialize)]
struct GpuReflectionJson {
    schema_version: u32,
    target: &'static str,
    kernels: Vec<KernelReflectionJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pipeline: Option<GraphicsPipelineReflectionJson>,
}

#[derive(Serialize)]
struct KernelReflectionJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    function: Option<u32>,
    provenance: KernelProvenanceJson,
    entry_name: String,
    shader_stage: &'static str,
    resources: Vec<KernelResourceReflectionJson>,
    vertex_input_count: usize,
    vertex_inputs: Vec<VertexInputReflectionJson>,
    builtin_inputs: Vec<KernelBuiltinInputReflectionJson>,
    called_builtins: Vec<&'static str>,
    launch: KernelLaunchPlanJson,
    workgroup: WorkgroupJson,
    dispatch_size: DispatchSizeJson,
    workgroup_count: WorkgroupCountJson,
}

#[derive(Serialize)]
struct KernelProvenanceJson {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    function: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_def: Option<u32>,
}

#[derive(Serialize)]
struct KernelLaunchPlanJson {
    entry_name: String,
    shader_stage: &'static str,
    pipeline_layout: KernelLaunchPipelineLayoutJson,
    bind_group_count: usize,
    bind_groups: Vec<KernelLaunchBindGroupJson>,
    webgpu_adapter: WebGpuLaunchAdapterJson,
    required_builtins: Vec<&'static str>,
    builtin_inputs: Vec<KernelBuiltinInputReflectionJson>,
    called_builtins: Vec<&'static str>,
    workgroup: WorkgroupJson,
    dispatch_size: DispatchSizeJson,
    workgroup_count: WorkgroupCountJson,
}

#[derive(Serialize)]
struct KernelLaunchPipelineLayoutJson {
    bind_group_layout_count: usize,
    bind_group_layouts: Vec<KernelLaunchPipelineBindGroupLayoutJson>,
}

#[derive(Serialize)]
struct KernelLaunchPipelineBindGroupLayoutJson {
    bind_group_index: usize,
    group: u32,
    layout_entry_count: usize,
}

#[derive(Serialize)]
struct KernelLaunchOffsetModeFlagsJson {
    requires_dynamic_offsets: bool,
    has_non_dynamic_offsets: bool,
    has_mixed_offset_modes: bool,
}

#[derive(Serialize)]
struct KernelLaunchBindGroupJson {
    bind_group_index: usize,
    group: u32,
    resource_indexes: Vec<usize>,
    resource_index_count: usize,
    resource_kinds: Vec<&'static str>,
    resource_kind_count: usize,
    resource_roles: Vec<&'static str>,
    resource_role_count: usize,
    shader_accesses: Vec<&'static str>,
    shader_access_count: usize,
    buffer_binding_types: Vec<&'static str>,
    buffer_binding_type_count: usize,
    buffer_byte_offsets: Vec<u64>,
    buffer_byte_offset_count: usize,
    buffer_byte_lens: Vec<u64>,
    buffer_byte_len_count: usize,
    binding_byte_lens: Vec<u64>,
    binding_byte_len_count: usize,
    min_binding_sizes: Vec<u64>,
    min_binding_size_count: usize,
    has_dynamic_offsets: Vec<bool>,
    dynamic_offset_flag_count: usize,
    offset_modes: Vec<&'static str>,
    offset_mode_entry_count: usize,
    #[serde(flatten)]
    offset_mode_flags: KernelLaunchOffsetModeFlagsJson,
    offset_mode_count: usize,
    has_only_non_dynamic_offsets: bool,
    has_only_dynamic_offsets: bool,
    non_dynamic_offset_count: usize,
    non_dynamic_offset_resource_indexes: Vec<usize>,
    non_dynamic_offset_resource_index_count: usize,
    non_dynamic_offset_binding_indexes: Vec<u32>,
    non_dynamic_offset_binding_index_count: usize,
    non_dynamic_offset_layout_entry_indexes: Vec<usize>,
    non_dynamic_offset_layout_entry_index_count: usize,
    dynamic_offset_count: usize,
    dynamic_offset_resource_indexes: Vec<usize>,
    dynamic_offset_resource_index_count: usize,
    dynamic_offset_binding_indexes: Vec<u32>,
    dynamic_offset_binding_index_count: usize,
    dynamic_offset_layout_entry_indexes: Vec<usize>,
    dynamic_offset_layout_entry_index_count: usize,
    binding_indexes: Vec<u32>,
    binding_index_count: usize,
    layout_entry_indexes: Vec<usize>,
    layout_entry_index_count: usize,
    source_locals: Vec<u32>,
    source_local_count: usize,
    source_names: Vec<String>,
    source_name_count: usize,
    binding_count: usize,
    layout_entry_count: usize,
    layout_entries: Vec<KernelLaunchBindGroupLayoutEntryJson>,
    bindings: Vec<KernelLaunchBindingJson>,
}

#[derive(Serialize)]
struct KernelLaunchBindGroupLayoutEntryJson {
    layout_entry_index: usize,
    resource_index: usize,
    binding: u32,
    shader_visibility: &'static str,
    buffer_binding_type: &'static str,
    min_binding_size: u64,
    has_dynamic_offset: bool,
    source_local: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
}

#[derive(Serialize)]
struct KernelLaunchBindingJson {
    binding_index: usize,
    resource_index: usize,
    binding: u32,
    kind: &'static str,
    role: &'static str,
    access: &'static str,
    shader_access: &'static str,
    shader_visibility: &'static str,
    buffer_binding_type: &'static str,
    element_type: TypeRefJson,
    element_layout: &'static str,
    element_byte_width: u16,
    element_count: u64,
    buffer_byte_len: u64,
    buffer_byte_offset: u64,
    binding_byte_len: u64,
    min_binding_size: u64,
    has_dynamic_offset: bool,
    source_local: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
}

#[derive(Serialize)]
struct WebGpuLaunchAdapterJson {
    pipeline_layout_descriptor: WebGpuPipelineLayoutDescriptorJson,
    bind_group_layout_descriptor_count: usize,
    bind_group_layout_descriptor_indexes: Vec<usize>,
    bind_group_layout_descriptor_index_count: usize,
    bind_group_layout_descriptors: Vec<WebGpuBindGroupLayoutDescriptorJson>,
    bind_group_descriptor_count: usize,
    bind_group_descriptor_indexes: Vec<usize>,
    bind_group_descriptor_index_count: usize,
    bind_group_descriptors: Vec<WebGpuBindGroupDescriptorJson>,
    vertex_buffer_layout_descriptor_count: usize,
    vertex_buffer_layout_descriptor_indexes: Vec<usize>,
    vertex_buffer_layout_descriptor_index_count: usize,
    vertex_buffer_layout_descriptors: Vec<WebGpuVertexBufferLayoutDescriptorJson>,
    dispatch_workgroup_dimension_count: usize,
    dispatch_workgroups: WebGpuDispatchWorkgroupsJson,
}

#[derive(Serialize)]
struct WebGpuPipelineLayoutDescriptorJson {
    #[serde(rename = "bind_group_layout_count")]
    layout_count: usize,
    #[serde(rename = "bind_group_layout_indexes")]
    layout_indexes: Vec<usize>,
    #[serde(rename = "bind_group_layout_index_count")]
    layout_index_count: usize,
}

#[derive(Serialize)]
struct WebGpuBindGroupLayoutDescriptorJson {
    bind_group_index: usize,
    group: u32,
    layout_entry_indexes: Vec<usize>,
    layout_entry_index_count: usize,
    entry_count: usize,
    entries: Vec<WebGpuBindGroupLayoutEntryDescriptorJson>,
}

#[derive(Serialize)]
struct WebGpuBindGroupLayoutEntryDescriptorJson {
    binding: u32,
    binding_index: usize,
    buffer_byte_len: u64,
    buffer_byte_offset: u64,
    binding_byte_len: u64,
    visibility: &'static str,
    buffer_type: &'static str,
    has_dynamic_offset: bool,
    min_binding_size: u64,
    resource_index: usize,
    layout_entry_index: usize,
    source_local: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
}

#[derive(Serialize)]
struct WebGpuBindGroupDescriptorJson {
    bind_group_index: usize,
    group: u32,
    entry_indexes: Vec<usize>,
    entry_index_count: usize,
    entry_count: usize,
    entries: Vec<WebGpuBindGroupEntryDescriptorJson>,
}

#[derive(Serialize)]
struct WebGpuBindGroupEntryDescriptorJson {
    binding: u32,
    kind: &'static str,
    role: &'static str,
    access: &'static str,
    shader_access: &'static str,
    shader_visibility: &'static str,
    element_type: TypeRefJson,
    element_layout: &'static str,
    element_byte_width: u16,
    element_count: u64,
    resource_index: usize,
    binding_index: usize,
    buffer_type: &'static str,
    buffer_byte_len: u64,
    buffer_byte_offset: u64,
    binding_byte_len: u64,
    min_binding_size: u64,
    has_dynamic_offset: bool,
    source_local: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
}

#[derive(Serialize)]
struct WebGpuVertexBufferLayoutDescriptorJson {
    buffer_index: usize,
    array_stride: u64,
    step_mode: &'static str,
    attribute_indexes: Vec<usize>,
    attribute_index_count: usize,
    attribute_count: usize,
    attributes: Vec<WebGpuVertexAttributeDescriptorJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
}

#[derive(Serialize)]
struct WebGpuVertexAttributeDescriptorJson {
    attribute_index: usize,
    shader_location: u32,
    format: &'static str,
    offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
}

#[derive(Serialize)]
struct WebGpuDispatchWorkgroupsJson {
    x: u64,
    y: u64,
    z: u64,
}

#[derive(Serialize)]
struct KernelBuiltinInputReflectionJson {
    source_local: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
    builtin: &'static str,
    axis: &'static str,
    ty: TypeRefJson,
    layout: &'static str,
}

#[derive(Serialize)]
struct KernelResourceReflectionJson {
    group: u32,
    binding: u32,
    kind: &'static str,
    role: &'static str,
    access: &'static str,
    element_type: TypeRefJson,
    element_layout: &'static str,
    element_byte_width: u16,
    element_count: u64,
    buffer_byte_len: u64,
    source_local: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
}

#[derive(Serialize)]
struct VertexInputReflectionJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
    location: u32,
    format: &'static str,
    step_mode: &'static str,
    offset_bytes: u64,
    stride_bytes: u64,
}

#[derive(Serialize)]
struct TypeRefJson {
    semantic: u32,
    layout: Option<u32>,
}

#[derive(Serialize)]
struct WorkgroupJson {
    x: u32,
    y: u32,
    z: u32,
}

#[derive(Serialize)]
struct DispatchSizeJson {
    x: u64,
    y: u64,
    z: u64,
}

#[derive(Serialize)]
struct WorkgroupCountJson {
    x: u64,
    y: u64,
    z: u64,
}

#[derive(Serialize)]
struct GraphicsPipelineReflectionJson {
    color_target_formats: Vec<&'static str>,
    primitive_topology: &'static str,
    vertex_count: u32,
    vertex_input_count: usize,
    varying_count: usize,
    vertex_resources: Vec<KernelResourceReflectionJson>,
    vertex_resource_count: usize,
    fragment_resources: Vec<KernelResourceReflectionJson>,
    fragment_resource_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    depth_stencil: Option<DepthStencilStateJson>,
}

#[derive(Serialize)]
struct DepthStencilStateJson {
    depth_write_enabled: bool,
    depth_compare: &'static str,
    stencil_read_mask: u32,
    stencil_write_mask: u32,
    stencil_front: StencilFaceStateJson,
    stencil_back: StencilFaceStateJson,
}

#[derive(Serialize)]
struct StencilFaceStateJson {
    compare: &'static str,
    fail_op: &'static str,
    depth_fail_op: &'static str,
    pass_op: &'static str,
}

#[cfg(feature = "mir-core")]
impl GpuReflectionJson {
    fn from_reflection(target: &'static str, reflection: &crate::mir::MirGpuReflection) -> Self {
        Self {
            schema_version: GPU_REFLECTION_SCHEMA_VERSION,
            target,
            kernels: reflection
                .kernels
                .iter()
                .map(kernel_reflection_json)
                .collect(),
            pipeline: reflection.pipeline.as_ref().map(pipeline_reflection_json),
        }
    }
}

#[cfg(feature = "mir-core")]
fn kernel_reflection_json(kernel: &crate::mir::MirKernelReflection) -> KernelReflectionJson {
    KernelReflectionJson {
        function: kernel.mir_function_provenance().map(|function| function.0),
        provenance: kernel_provenance_json(kernel.provenance),
        entry_name: kernel.entry_name.clone(),
        shader_stage: kernel.shader_stage.spelling(),
        resources: kernel
            .resources
            .iter()
            .map(resource_reflection_json)
            .collect(),
        vertex_input_count: kernel.vertex_inputs.len(),
        vertex_inputs: kernel
            .vertex_inputs
            .iter()
            .map(vertex_input_reflection_json)
            .collect(),
        builtin_inputs: kernel
            .builtin_inputs
            .iter()
            .map(builtin_input_reflection_json)
            .collect(),
        called_builtins: kernel
            .called_builtins
            .iter()
            .map(|builtin| builtin.spelling())
            .collect(),
        launch: launch_plan_json(&kernel.launch_plan()),
        workgroup: WorkgroupJson { x: kernel.workgroup.x, y: kernel.workgroup.y, z: kernel.workgroup.z },
        dispatch_size: DispatchSizeJson {
            x: kernel.dispatch_size.x,
            y: kernel.dispatch_size.y,
            z: kernel.dispatch_size.z,
        },
        workgroup_count: WorkgroupCountJson {
            x: kernel.workgroup_count.x,
            y: kernel.workgroup_count.y,
            z: kernel.workgroup_count.z,
        },
    }
}

#[cfg(feature = "mir-core")]
fn kernel_provenance_json(provenance: crate::mir::MirKernelProvenance) -> KernelProvenanceJson {
    match provenance {
        crate::mir::MirKernelProvenance::MirFunction(function) => {
            KernelProvenanceJson { kind: "mir-function", function: Some(function.0), source_def: None }
        }
        crate::mir::MirKernelProvenance::SourceFunction(def_id) => {
            KernelProvenanceJson { kind: "source-function", function: None, source_def: Some(def_id.0) }
        }
    }
}

fn launch_plan_json(plan: &crate::mir::MirKernelLaunchPlan) -> KernelLaunchPlanJson {
    let webgpu_adapter = plan.webgpu_adapter();
    KernelLaunchPlanJson {
        entry_name: plan.entry_name.clone(),
        shader_stage: plan.shader_stage.spelling(),
        pipeline_layout: launch_pipeline_layout_json(&plan.pipeline_layout),
        bind_group_count: plan.bind_group_count,
        bind_groups: plan
            .bind_groups
            .iter()
            .map(launch_bind_group_json)
            .collect(),
        webgpu_adapter: webgpu_launch_adapter_json(&webgpu_adapter),
        required_builtins: plan
            .required_builtins
            .iter()
            .map(|builtin| builtin.spelling())
            .collect(),
        builtin_inputs: plan
            .builtin_inputs
            .iter()
            .map(builtin_input_reflection_json)
            .collect(),
        called_builtins: plan
            .called_builtins
            .iter()
            .map(|builtin| builtin.spelling())
            .collect(),
        workgroup: WorkgroupJson { x: plan.workgroup.x, y: plan.workgroup.y, z: plan.workgroup.z },
        dispatch_size: DispatchSizeJson { x: plan.dispatch_size.x, y: plan.dispatch_size.y, z: plan.dispatch_size.z },
        workgroup_count: WorkgroupCountJson {
            x: plan.workgroup_count.x,
            y: plan.workgroup_count.y,
            z: plan.workgroup_count.z,
        },
    }
}

fn launch_pipeline_layout_json(
    pipeline_layout: &crate::mir::MirKernelLaunchPipelineLayout,
) -> KernelLaunchPipelineLayoutJson {
    KernelLaunchPipelineLayoutJson {
        bind_group_layout_count: pipeline_layout.bind_group_layout_count,
        bind_group_layouts: pipeline_layout
            .bind_group_layouts
            .iter()
            .map(launch_pipeline_bind_group_layout_json)
            .collect(),
    }
}

fn launch_pipeline_bind_group_layout_json(
    bind_group_layout: &crate::mir::MirKernelLaunchPipelineBindGroupLayout,
) -> KernelLaunchPipelineBindGroupLayoutJson {
    KernelLaunchPipelineBindGroupLayoutJson {
        bind_group_index: bind_group_layout.bind_group_index,
        group: bind_group_layout.group,
        layout_entry_count: bind_group_layout.layout_entry_count,
    }
}

fn launch_bind_group_json(group: &crate::mir::MirKernelLaunchBindGroup) -> KernelLaunchBindGroupJson {
    KernelLaunchBindGroupJson {
        bind_group_index: group.bind_group_index,
        group: group.group,
        resource_indexes: group.resource_indexes.clone(),
        resource_index_count: group.resource_index_count,
        resource_kinds: group
            .resource_kinds
            .iter()
            .map(|kind| kind.spelling())
            .collect(),
        resource_kind_count: group.resource_kind_count,
        resource_roles: resource_role_names(&group.resource_roles),
        resource_role_count: group.resource_role_count,
        shader_accesses: shader_access_names(&group.shader_accesses),
        shader_access_count: group.shader_access_count,
        buffer_binding_types: group
            .buffer_binding_types
            .iter()
            .map(|binding_type| binding_type.spelling())
            .collect(),
        buffer_binding_type_count: group.buffer_binding_type_count,
        buffer_byte_offsets: group.buffer_byte_offsets.clone(),
        buffer_byte_offset_count: group.buffer_byte_offset_count,
        buffer_byte_lens: group.buffer_byte_lens.clone(),
        buffer_byte_len_count: group.buffer_byte_len_count,
        binding_byte_lens: group.binding_byte_lens.clone(),
        binding_byte_len_count: group.binding_byte_len_count,
        min_binding_sizes: group.min_binding_sizes.clone(),
        min_binding_size_count: group.min_binding_size_count,
        has_dynamic_offsets: group.has_dynamic_offsets.clone(),
        dynamic_offset_flag_count: group.dynamic_offset_flag_count,
        offset_modes: group
            .offset_modes
            .iter()
            .map(|mode| mode.spelling())
            .collect(),
        offset_mode_entry_count: group.offset_mode_entry_count,
        offset_mode_flags: KernelLaunchOffsetModeFlagsJson {
            requires_dynamic_offsets: group.requires_dynamic_offsets,
            has_non_dynamic_offsets: group.has_non_dynamic_offsets,
            has_mixed_offset_modes: group.has_mixed_offset_modes,
        },
        offset_mode_count: group.offset_mode_count,
        has_only_non_dynamic_offsets: group.has_only_non_dynamic_offsets,
        has_only_dynamic_offsets: group.has_only_dynamic_offsets,
        non_dynamic_offset_count: group.non_dynamic_offset_count,
        non_dynamic_offset_resource_indexes: group.non_dynamic_offset_resource_indexes.clone(),
        non_dynamic_offset_resource_index_count: group.non_dynamic_offset_resource_index_count,
        non_dynamic_offset_binding_indexes: group.non_dynamic_offset_binding_indexes.clone(),
        non_dynamic_offset_binding_index_count: group.non_dynamic_offset_binding_index_count,
        non_dynamic_offset_layout_entry_indexes: group.non_dynamic_offset_layout_entry_indexes.clone(),
        non_dynamic_offset_layout_entry_index_count: group.non_dynamic_offset_layout_entry_index_count,
        dynamic_offset_count: group.dynamic_offset_count,
        dynamic_offset_resource_indexes: group.dynamic_offset_resource_indexes.clone(),
        dynamic_offset_resource_index_count: group.dynamic_offset_resource_index_count,
        dynamic_offset_binding_indexes: group.dynamic_offset_binding_indexes.clone(),
        dynamic_offset_binding_index_count: group.dynamic_offset_binding_index_count,
        dynamic_offset_layout_entry_indexes: group.dynamic_offset_layout_entry_indexes.clone(),
        dynamic_offset_layout_entry_index_count: group.dynamic_offset_layout_entry_index_count,
        binding_indexes: group.binding_indexes.clone(),
        binding_index_count: group.binding_index_count,
        layout_entry_indexes: group.layout_entry_indexes.clone(),
        layout_entry_index_count: group.layout_entry_index_count,
        source_locals: local_ids(&group.source_locals),
        source_local_count: group.source_local_count,
        source_names: group.source_names.clone(),
        source_name_count: group.source_name_count,
        binding_count: group.binding_count,
        layout_entry_count: group.layout_entry_count,
        layout_entries: launch_bind_group_layout_entries_json(&group.layout_entries),
        bindings: launch_bindings_json(&group.bindings),
    }
}

fn resource_role_names(roles: &[crate::mir::MirKernelResourceRole]) -> Vec<&'static str> {
    roles.iter().map(|role| resource_role_name(*role)).collect()
}

fn shader_access_names(accesses: &[crate::mir::MirKernelShaderAccess]) -> Vec<&'static str> {
    accesses.iter().map(|access| access.spelling()).collect()
}

fn local_ids(locals: &[crate::mir::MirLocalId]) -> Vec<u32> {
    locals.iter().map(|local| local.0).collect()
}

fn launch_bind_group_layout_entries_json(
    entries: &[crate::mir::MirKernelLaunchBindGroupLayoutEntry],
) -> Vec<KernelLaunchBindGroupLayoutEntryJson> {
    entries
        .iter()
        .map(launch_bind_group_layout_entry_json)
        .collect()
}

fn launch_bindings_json(bindings: &[crate::mir::MirKernelLaunchBinding]) -> Vec<KernelLaunchBindingJson> {
    bindings.iter().map(launch_binding_json).collect()
}

fn launch_bind_group_layout_entry_json(
    entry: &crate::mir::MirKernelLaunchBindGroupLayoutEntry,
) -> KernelLaunchBindGroupLayoutEntryJson {
    KernelLaunchBindGroupLayoutEntryJson {
        layout_entry_index: entry.layout_entry_index,
        resource_index: entry.resource_index,
        binding: entry.binding,
        shader_visibility: entry.shader_visibility.spelling(),
        buffer_binding_type: entry.buffer_binding_type.spelling(),
        min_binding_size: entry.min_binding_size,
        has_dynamic_offset: entry.has_dynamic_offset,
        source_local: entry.source_local.map(|local| local.0),
        source_name: entry.source_name.clone(),
    }
}

fn launch_binding_json(binding: &crate::mir::MirKernelLaunchBinding) -> KernelLaunchBindingJson {
    KernelLaunchBindingJson {
        binding_index: binding.binding_index,
        resource_index: binding.resource_index,
        binding: binding.binding,
        kind: binding.kind.spelling(),
        role: resource_role_name(binding.role),
        access: resource_access_name(binding.access),
        shader_access: binding.shader_access.spelling(),
        shader_visibility: binding.shader_visibility.spelling(),
        buffer_binding_type: binding.buffer_binding_type.spelling(),
        element_type: type_ref_json(binding.element_ty),
        element_layout: binding.element_layout.spelling(),
        element_byte_width: binding.element_byte_width,
        element_count: binding.element_count,
        buffer_byte_len: binding.buffer_byte_len,
        buffer_byte_offset: binding.buffer_byte_offset,
        binding_byte_len: binding.binding_byte_len,
        min_binding_size: binding.min_binding_size,
        has_dynamic_offset: binding.has_dynamic_offset,
        source_local: binding.source_local.map(|local| local.0),
        source_name: binding.source_name.clone(),
    }
}

fn webgpu_launch_adapter_json(adapter: &crate::mir::MirWebGpuLaunchAdapter) -> WebGpuLaunchAdapterJson {
    WebGpuLaunchAdapterJson {
        pipeline_layout_descriptor: webgpu_pipeline_layout_descriptor_json(&adapter.pipeline_layout_descriptor),
        bind_group_layout_descriptor_count: adapter.bind_group_layout_descriptor_count,
        bind_group_layout_descriptor_indexes: adapter.bind_group_layout_descriptor_indexes.clone(),
        bind_group_layout_descriptor_index_count: adapter.bind_group_layout_descriptor_index_count,
        bind_group_layout_descriptors: webgpu_bind_group_layout_descriptors_json(
            &adapter.bind_group_layout_descriptors,
        ),
        bind_group_descriptor_count: adapter.bind_group_descriptor_count,
        bind_group_descriptor_indexes: adapter.bind_group_descriptor_indexes.clone(),
        bind_group_descriptor_index_count: adapter.bind_group_descriptor_index_count,
        bind_group_descriptors: webgpu_bind_group_descriptors_json(&adapter.bind_group_descriptors),
        vertex_buffer_layout_descriptor_count: adapter.vertex_buffer_layout_descriptor_count,
        vertex_buffer_layout_descriptor_indexes: adapter.vertex_buffer_layout_descriptor_indexes.clone(),
        vertex_buffer_layout_descriptor_index_count: adapter.vertex_buffer_layout_descriptor_index_count,
        vertex_buffer_layout_descriptors: webgpu_vertex_buffer_layout_descriptors_json(
            &adapter.vertex_buffer_layout_descriptors,
        ),
        dispatch_workgroup_dimension_count: adapter.dispatch_workgroup_dimension_count,
        dispatch_workgroups: webgpu_dispatch_workgroups_json(&adapter.dispatch_workgroups),
    }
}

fn webgpu_pipeline_layout_descriptor_json(
    layout: &crate::mir::MirWebGpuPipelineLayoutDescriptor,
) -> WebGpuPipelineLayoutDescriptorJson {
    WebGpuPipelineLayoutDescriptorJson {
        layout_count: layout.bind_group_layout_count,
        layout_indexes: layout.bind_group_layout_indexes.clone(),
        layout_index_count: layout.bind_group_layout_index_count,
    }
}

fn webgpu_bind_group_layout_descriptors_json(
    groups: &[crate::mir::MirWebGpuBindGroupLayoutDescriptor],
) -> Vec<WebGpuBindGroupLayoutDescriptorJson> {
    groups
        .iter()
        .map(webgpu_bind_group_layout_descriptor_json)
        .collect()
}

fn webgpu_bind_group_layout_descriptor_json(
    group: &crate::mir::MirWebGpuBindGroupLayoutDescriptor,
) -> WebGpuBindGroupLayoutDescriptorJson {
    WebGpuBindGroupLayoutDescriptorJson {
        bind_group_index: group.bind_group_index,
        group: group.group,
        layout_entry_indexes: group.layout_entry_indexes.clone(),
        layout_entry_index_count: group.layout_entry_index_count,
        entry_count: group.entry_count,
        entries: webgpu_bind_group_layout_entry_descriptors_json(&group.entries),
    }
}

fn webgpu_bind_group_layout_entry_descriptors_json(
    entries: &[crate::mir::MirWebGpuBindGroupLayoutEntryDescriptor],
) -> Vec<WebGpuBindGroupLayoutEntryDescriptorJson> {
    entries
        .iter()
        .map(webgpu_bind_group_layout_entry_descriptor_json)
        .collect()
}

fn webgpu_bind_group_layout_entry_descriptor_json(
    entry: &crate::mir::MirWebGpuBindGroupLayoutEntryDescriptor,
) -> WebGpuBindGroupLayoutEntryDescriptorJson {
    WebGpuBindGroupLayoutEntryDescriptorJson {
        binding: entry.binding,
        binding_index: entry.binding_index,
        buffer_byte_len: entry.buffer_byte_len,
        buffer_byte_offset: entry.buffer_byte_offset,
        binding_byte_len: entry.binding_byte_len,
        visibility: entry.visibility.spelling(),
        buffer_type: entry.buffer_type.spelling(),
        has_dynamic_offset: entry.has_dynamic_offset,
        min_binding_size: entry.min_binding_size,
        resource_index: entry.resource_index,
        layout_entry_index: entry.layout_entry_index,
        source_local: entry.source_local.map(|local| local.0),
        source_name: entry.source_name.clone(),
    }
}

fn webgpu_bind_group_descriptor_json(
    group: &crate::mir::MirWebGpuBindGroupDescriptor,
) -> WebGpuBindGroupDescriptorJson {
    WebGpuBindGroupDescriptorJson {
        bind_group_index: group.bind_group_index,
        group: group.group,
        entry_indexes: group.entry_indexes.clone(),
        entry_index_count: group.entry_index_count,
        entry_count: group.entry_count,
        entries: webgpu_bind_group_entry_descriptors_json(&group.entries),
    }
}

fn webgpu_bind_group_descriptors_json(
    groups: &[crate::mir::MirWebGpuBindGroupDescriptor],
) -> Vec<WebGpuBindGroupDescriptorJson> {
    groups
        .iter()
        .map(webgpu_bind_group_descriptor_json)
        .collect()
}

fn webgpu_bind_group_entry_descriptors_json(
    entries: &[crate::mir::MirWebGpuBindGroupEntryDescriptor],
) -> Vec<WebGpuBindGroupEntryDescriptorJson> {
    entries
        .iter()
        .map(webgpu_bind_group_entry_descriptor_json)
        .collect()
}

fn webgpu_bind_group_entry_descriptor_json(
    binding: &crate::mir::MirWebGpuBindGroupEntryDescriptor,
) -> WebGpuBindGroupEntryDescriptorJson {
    WebGpuBindGroupEntryDescriptorJson {
        binding: binding.binding,
        kind: binding.kind.spelling(),
        role: resource_role_name(binding.role),
        access: resource_access_name(binding.access),
        shader_access: binding.shader_access.spelling(),
        shader_visibility: binding.shader_visibility.spelling(),
        element_type: type_ref_json(binding.element_ty),
        element_layout: binding.element_layout.spelling(),
        element_byte_width: binding.element_byte_width,
        element_count: binding.element_count,
        resource_index: binding.resource_index,
        binding_index: binding.binding_index,
        buffer_type: binding.buffer_type.spelling(),
        buffer_byte_len: binding.buffer_byte_len,
        buffer_byte_offset: binding.buffer_byte_offset,
        binding_byte_len: binding.binding_byte_len,
        min_binding_size: binding.min_binding_size,
        has_dynamic_offset: binding.has_dynamic_offset,
        source_local: binding.source_local.map(|local| local.0),
        source_name: binding.source_name.clone(),
    }
}

fn webgpu_vertex_buffer_layout_descriptors_json(
    descriptors: &[crate::mir::MirWebGpuVertexBufferLayoutDescriptor],
) -> Vec<WebGpuVertexBufferLayoutDescriptorJson> {
    descriptors
        .iter()
        .map(webgpu_vertex_buffer_layout_descriptor_json)
        .collect()
}

fn webgpu_vertex_buffer_layout_descriptor_json(
    descriptor: &crate::mir::MirWebGpuVertexBufferLayoutDescriptor,
) -> WebGpuVertexBufferLayoutDescriptorJson {
    WebGpuVertexBufferLayoutDescriptorJson {
        buffer_index: descriptor.buffer_index,
        array_stride: descriptor.array_stride,
        step_mode: descriptor.step_mode.spelling(),
        attribute_indexes: descriptor.attribute_indexes.clone(),
        attribute_index_count: descriptor.attribute_index_count,
        attribute_count: descriptor.attribute_count,
        attributes: webgpu_vertex_attribute_descriptors_json(&descriptor.attributes),
        source_name: descriptor.source_name.clone(),
    }
}

fn webgpu_vertex_attribute_descriptors_json(
    attributes: &[crate::mir::MirWebGpuVertexAttributeDescriptor],
) -> Vec<WebGpuVertexAttributeDescriptorJson> {
    attributes
        .iter()
        .map(webgpu_vertex_attribute_descriptor_json)
        .collect()
}

fn webgpu_vertex_attribute_descriptor_json(
    attribute: &crate::mir::MirWebGpuVertexAttributeDescriptor,
) -> WebGpuVertexAttributeDescriptorJson {
    WebGpuVertexAttributeDescriptorJson {
        attribute_index: attribute.attribute_index,
        shader_location: attribute.shader_location,
        format: attribute.format.spelling(),
        offset: attribute.offset,
        source_name: attribute.source_name.clone(),
    }
}

fn webgpu_dispatch_workgroups_json(dispatch: &crate::mir::MirWebGpuDispatchWorkgroups) -> WebGpuDispatchWorkgroupsJson {
    WebGpuDispatchWorkgroupsJson { x: dispatch.x, y: dispatch.y, z: dispatch.z }
}

fn resource_reflection_json(resource: &crate::mir::MirKernelResourceReflection) -> KernelResourceReflectionJson {
    KernelResourceReflectionJson {
        group: resource.group,
        binding: resource.binding,
        kind: resource.kind.spelling(),
        role: resource_role_name(resource.role),
        access: resource_access_name(resource.access),
        element_type: type_ref_json(resource.element_ty),
        element_layout: resource.element_layout.spelling(),
        element_byte_width: resource.element_byte_width,
        element_count: resource.element_count,
        buffer_byte_len: resource.buffer_byte_len,
        source_local: resource.source_local.map(|local| local.0),
        source_name: resource.source_name.clone(),
    }
}

fn vertex_input_reflection_json(input: &crate::mir::MirVertexInputReflection) -> VertexInputReflectionJson {
    VertexInputReflectionJson {
        source_name: input.source_name.clone(),
        location: input.location,
        format: input.format.spelling(),
        step_mode: input.step_mode.spelling(),
        offset_bytes: input.offset_bytes,
        stride_bytes: input.stride_bytes,
    }
}

fn builtin_input_reflection_json(
    input: &crate::mir::MirKernelBuiltinInputReflection,
) -> KernelBuiltinInputReflectionJson {
    KernelBuiltinInputReflectionJson {
        source_local: input.source_local.0,
        source_name: input.source_name.clone(),
        builtin: input.builtin.spelling(),
        axis: input.axis.spelling(),
        ty: type_ref_json(input.ty),
        layout: input.layout.spelling(),
    }
}

fn type_ref_json(ty: crate::mir::MirType) -> TypeRefJson {
    TypeRefJson { semantic: ty.semantic_id().0, layout: ty.layout_id().map(|layout| layout.0) }
}

fn pipeline_reflection_json(pipeline: &crate::mir::MirGraphicsPipelineReflection) -> GraphicsPipelineReflectionJson {
    GraphicsPipelineReflectionJson {
        color_target_formats: pipeline
            .color_target_formats
            .iter()
            .map(|format| format.spelling())
            .collect(),
        primitive_topology: pipeline.primitive_topology.spelling(),
        vertex_count: pipeline.vertex_count,
        vertex_input_count: pipeline.vertex_input_count,
        varying_count: pipeline.varying_count,
        vertex_resources: pipeline
            .vertex_resources
            .iter()
            .map(resource_reflection_json)
            .collect(),
        vertex_resource_count: pipeline.vertex_resource_count,
        fragment_resources: pipeline
            .fragment_resources
            .iter()
            .map(resource_reflection_json)
            .collect(),
        fragment_resource_count: pipeline.fragment_resource_count,
        depth_stencil: pipeline
            .depth_stencil
            .as_ref()
            .map(|ds| DepthStencilStateJson {
                depth_write_enabled: ds.depth_write_enabled,
                depth_compare: ds.depth_compare.spelling(),
                stencil_read_mask: ds.stencil_read_mask,
                stencil_write_mask: ds.stencil_write_mask,
                stencil_front: StencilFaceStateJson {
                    compare: ds.stencil_front.compare.spelling(),
                    fail_op: ds.stencil_front.fail_op.spelling(),
                    depth_fail_op: ds.stencil_front.depth_fail_op.spelling(),
                    pass_op: ds.stencil_front.pass_op.spelling(),
                },
                stencil_back: StencilFaceStateJson {
                    compare: ds.stencil_back.compare.spelling(),
                    fail_op: ds.stencil_back.fail_op.spelling(),
                    depth_fail_op: ds.stencil_back.depth_fail_op.spelling(),
                    pass_op: ds.stencil_back.pass_op.spelling(),
                },
            }),
    }
}

fn resource_role_name(role: crate::mir::MirKernelResourceRole) -> &'static str {
    match role {
        crate::mir::MirKernelResourceRole::Input => "input",
        crate::mir::MirKernelResourceRole::Output => "output",
    }
}

fn resource_access_name(access: crate::mir::MirKernelResourceAccess) -> &'static str {
    match access {
        crate::mir::MirKernelResourceAccess::Read => "read",
        crate::mir::MirKernelResourceAccess::Write => "write",
        crate::mir::MirKernelResourceAccess::ReadWrite => "read-write",
    }
}
