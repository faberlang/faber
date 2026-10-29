use crate::{MirFunction, MirFunctionId, MirValidationContext};
use radix_hir::DefId;
use std::collections::BTreeMap;

use super::{
    ERR_GRAPHICS_PIPELINE_REQUIRES_COLOR_TARGETS, ERR_GRAPHICS_PIPELINE_REQUIRES_FRAGMENT,
    ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX, ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX_INPUTS,
    ERR_GRAPHICS_PIPELINE_VARYING_MISMATCH, INDEX_FAULT_TEXT, MirAbiError, MirColorTargetFormat,
    MirFaultKind, MirFaultSite, MirFragmentOutputReflection, MirGraphicsPipelineReflection,
    MirKernelBuiltin, MirKernelDispatchSize, MirKernelLaunchBindGroup, MirKernelLaunchBinding,
    MirKernelLaunchPlan, MirKernelOffsetMode, MirKernelProvenance, MirKernelReflection,
    MirKernelResourceKind, MirKernelResourceReflection, MirKernelShaderStage, MirKernelSignature,
    MirKernelWorkgroupCount, MirPrimitiveTopology, MirVaryingReflection, MirVertexInputReflection,
    MirWorkgroupSize, buffer_binding_type_for_shader_access, collection_op_workgroup_count,
    combined_builtin_dependencies, layout_entry_for_launch_binding,
    pipeline_layout_for_launch_bind_groups, reflect_kernel_builtin_input, reflect_kernel_resource,
    shader_access_for_resource, validate_fragment_output_locations, validate_varying_consistency,
    validate_varying_locations, validate_vertex_input_locations, workgroup_count_for_dispatch,
};

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
            bound_resources: Vec::new(),
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
            fault_sites: fault_sites_for_signature(signature),
            lane_range_binding: None,
            slab_origin_bindings: Vec::new(),
        })
    }

    /// Replace the signature-derived fault sites with the kernel's full list
    /// (gpu-reset U9-6): tensor and vector index sites first (`Fatal`, the
    /// index text), grouped-key axis sites next (`Fatal`), then one
    /// `Recoverable` site per constant throw in block order, carrying the
    /// thrown text. The signature alone knows only how many
    /// sites the fault record holds, so a consumer that has the function applies
    /// this after [`Self::from_signature`]. A kernel with no fault record is
    /// untouched.
    pub fn apply_fault_sites(
        &mut self,
        function: &MirFunction,
        validation: &crate::MirValidationContext<'_>,
    ) {
        if self.fault_sites.is_empty() {
            return;
        }
        let indexes = super::unproven_indexes(function, validation).len()
            + super::unproven_vector_indexes(function, validation).len()
            + super::grid_reduce_helper_tensors(function, validation).len()
            + super::grid_reduce_node(function)
                .and_then(|(filum, _)| filum.group.as_ref())
                .map_or(0, |group| group.keys.len());
        let throws = super::throw_sites(function, &validation.types).unwrap_or_default();
        let d64 = usize::from(super::d64_fault_site(function, validation));
        if indexes + throws.len() + d64 != self.fault_sites.len() {
            return;
        }
        for (site, throw) in self.fault_sites[indexes..].iter_mut().zip(&throws) {
            site.kind = MirFaultKind::Recoverable;
            site.text = validation
                .interner
                .as_interner()
                .map_or_else(String::new, |interner| {
                    interner.resolve(throw.text).to_owned()
                });
        }
        if d64 == 1
            && let Some(site) = self.fault_sites.last_mut()
        {
            super::D64_FAULT_TEXT.clone_into(&mut site.text);
        }
    }

    /// Recompute the reflection's workgroup grid and workgroup count against
    /// the effective workgroup with collection-op-aware dispatch semantics.
    ///
    /// [`from_signature`] reflects the signature default: `workgroup_count` is
    /// the naive `ceil(dispatch_size / workgroup)`. Fused-grid collection-op
    /// kernels override `@workgroup_size` via the effective workgroup (the
    /// caller's emitter policy — a target's tiled matmul tile or its
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
    /// Semantics lifted from the former target-local
    /// `apply_effective_workgroup` (decision D-A1, delivery D-W6-A1): this
    /// method is the single source of truth for collection-op
    /// `workgroup_count` semantics.
    pub fn apply_effective_workgroup(
        &mut self,
        function: &MirFunction,
        effective: MirWorkgroupSize,
    ) {
        self.workgroup = effective;
        self.workgroup_count =
            collection_op_workgroup_count(function, self.dispatch_size, effective);
    }

    /// The full per-kernel reflection of one device-artifact kernel unit,
    /// independent of the emitting backend: [`Self::from_signature`] under the
    /// validation context the emission signature was synthesized with, then
    /// [`Self::apply_fault_sites`] (the full fault-site list from the
    /// function), then [`Self::apply_effective_workgroup`] against
    /// `effective_workgroup` (the device program's launch-plan workgroup, the
    /// authoritative per-kernel fact — for a generic one-dimensional kernel it
    /// differs from the signature's `1×1×1`). `entry_name` is the logical
    /// entry the host launches by.
    ///
    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn for_device_unit(
        entry_name: impl Into<String>,
        function: &MirFunction,
        signature: &MirKernelSignature,
        validation: &MirValidationContext<'_>,
        effective_workgroup: MirWorkgroupSize,
    ) -> Result<Self, MirAbiError> {
        let mut reflection = Self::from_signature(entry_name, signature, validation)?;
        reflection.apply_fault_sites(function, validation);
        reflection.apply_effective_workgroup(function, effective_workgroup);
        Ok(reflection)
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
            fault_sites: Vec::new(),
            lane_range_binding: None,
            slab_origin_bindings: Vec::new(),
            bound_resources: Vec::new(),
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
            fault_sites: Vec::new(),
            lane_range_binding: None,
            slab_origin_bindings: Vec::new(),
            bound_resources: Vec::new(),
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
            fault_sites: Vec::new(),
            lane_range_binding: None,
            slab_origin_bindings: Vec::new(),
            bound_resources: Vec::new(),
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

/// The fault sites a signature declares: its `FaultRecord` resource says how
/// many, and every site is the fatal index bounds fault.
fn fault_sites_for_signature(signature: &MirKernelSignature) -> Vec<MirFaultSite> {
    signature
        .resources()
        .find_map(|resource| match resource.kind {
            MirKernelResourceKind::FaultRecord { sites } => Some(sites),
            _ => None,
        })
        .map_or_else(Vec::new, |sites| {
            (0..sites)
                .map(|id| MirFaultSite {
                    id,
                    kind: MirFaultKind::Fatal,
                    text: INDEX_FAULT_TEXT.to_owned(),
                })
                .collect()
        })
}
