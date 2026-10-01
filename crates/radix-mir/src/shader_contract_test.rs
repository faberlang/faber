use super::*;
use crate::MirStencilFaceState;

fn provenance() -> ContractProvenance {
    ContractProvenance {
        source: Some(DefId(7)),
        span: Span::new(0, 4),
    }
}

#[test]
fn stage_io_role_from_code_accepts_known_codes() {
    assert_eq!(stage_io_role_from_code(1), Ok(ShaderStageIoRole::Position));
    assert_eq!(stage_io_role_from_code(2), Ok(ShaderStageIoRole::Color));
    assert_eq!(stage_io_role_from_code(3), Ok(ShaderStageIoRole::Transform));
    assert_eq!(stage_io_role_from_code(4), Ok(ShaderStageIoRole::Other));
}

#[test]
fn stage_io_role_from_code_rejects_unknown() {
    let error = stage_io_role_from_code(0).expect_err("unknown role code must error");
    assert_eq!(error.issue, "shader_stage_io_role_code");
    assert_eq!(error.detail.as_deref(), Some("0"));
}

#[test]
fn role_defaults_to_other() {
    assert_eq!(ShaderStageIoRole::default(), ShaderStageIoRole::Other);
}

#[test]
fn vertex_format_from_code_accepts_known_codes() {
    assert_eq!(
        vertex_format_from_code(2),
        Ok(MirVertexInputFormat::Float32x2)
    );
    assert_eq!(
        vertex_format_from_code(3),
        Ok(MirVertexInputFormat::Float32x3)
    );
    assert_eq!(
        vertex_format_from_code(4),
        Ok(MirVertexInputFormat::Float32x4)
    );
}

#[test]
fn vertex_format_from_code_rejects_unknown() {
    let error = vertex_format_from_code(5).expect_err("unknown format code must error");
    assert_eq!(error.issue, "mir_shader_format_code_unsupported");
    assert_eq!(error.detail.as_deref(), Some("5"));
}

#[test]
fn color_target_from_code_accepts_known_codes() {
    assert_eq!(
        color_target_format_from_code(1),
        Ok(MirColorTargetFormat::Bgra8Unorm)
    );
    assert_eq!(
        color_target_format_from_code(2),
        Ok(MirColorTargetFormat::Bgra8UnormSrgb)
    );
}

#[test]
fn color_target_from_code_rejects_unknown() {
    let error = color_target_format_from_code(0).expect_err("unknown color target must error");
    assert_eq!(error.issue, "mir_shader_color_target_code_unsupported");
    assert_eq!(error.detail.as_deref(), Some("0"));
}

#[test]
fn topology_from_code_accepts_known_codes() {
    assert_eq!(
        primitive_topology_from_code(1),
        Ok(MirPrimitiveTopology::TriangleList)
    );
    assert_eq!(
        primitive_topology_from_code(2),
        Ok(MirPrimitiveTopology::LineList)
    );
    assert_eq!(
        primitive_topology_from_code(3),
        Ok(MirPrimitiveTopology::PointList)
    );
}

#[test]
fn topology_from_code_rejects_unknown() {
    let error = primitive_topology_from_code(9).expect_err("unknown topology must error");
    assert_eq!(error.issue, "mir_shader_topology_code_unsupported");
    assert_eq!(error.detail.as_deref(), Some("9"));
}

#[test]
fn depth_compare_from_code_accepts_known_codes() {
    assert_eq!(
        depth_compare_function_from_code(1),
        Ok(MirDepthCompareFunction::Less)
    );
    assert_eq!(
        depth_compare_function_from_code(2),
        Ok(MirDepthCompareFunction::Equal)
    );
    assert_eq!(
        depth_compare_function_from_code(3),
        Ok(MirDepthCompareFunction::LessEqual)
    );
    assert_eq!(
        depth_compare_function_from_code(4),
        Ok(MirDepthCompareFunction::Greater)
    );
    assert_eq!(
        depth_compare_function_from_code(5),
        Ok(MirDepthCompareFunction::NotEqual)
    );
    assert_eq!(
        depth_compare_function_from_code(6),
        Ok(MirDepthCompareFunction::GreaterEqual)
    );
    assert_eq!(
        depth_compare_function_from_code(7),
        Ok(MirDepthCompareFunction::Always)
    );
}

#[test]
fn depth_compare_from_code_rejects_unknown_rather_than_never() {
    // Council lock: the depth path must ERROR on unknown codes, not return
    // `Never` like the legacy fail-open depth table.
    let error = depth_compare_function_from_code(0).expect_err("unknown depth code must error");
    assert_eq!(error.issue, "mir_shader_depth_compare_code_unsupported");
    assert_eq!(error.detail.as_deref(), Some("0"));
    let error = depth_compare_function_from_code(99).expect_err("unknown depth code must error");
    assert_eq!(error.detail.as_deref(), Some("99"));
}

#[test]
fn input_step_mode_from_code_accepts_known_codes() {
    assert_eq!(input_step_mode_from_code(1), Ok(MirInputStepMode::Vertex));
    assert_eq!(input_step_mode_from_code(2), Ok(MirInputStepMode::Instance));
}

#[test]
fn input_step_mode_from_code_rejects_unknown() {
    let error = input_step_mode_from_code(3).expect_err("unknown step mode must error");
    assert_eq!(error.issue, "mir_shader_step_mode_code_unsupported");
    assert_eq!(error.detail.as_deref(), Some("3"));
}

#[test]
fn varying_interpolation_from_code_accepts_known_codes() {
    assert_eq!(
        varying_interpolation_from_code(1),
        Ok(MirVaryingInterpolation::Perspective)
    );
    assert_eq!(
        varying_interpolation_from_code(2),
        Ok(MirVaryingInterpolation::Linear)
    );
    assert_eq!(
        varying_interpolation_from_code(3),
        Ok(MirVaryingInterpolation::Flat)
    );
}

#[test]
fn varying_interpolation_from_code_rejects_unknown() {
    let error = varying_interpolation_from_code(4).expect_err("unknown interpolation must error");
    assert_eq!(error.issue, "mir_shader_interpolation_code_unsupported");
    assert_eq!(error.detail.as_deref(), Some("4"));
}

#[test]
fn resource_kind_from_code_accepts_known_codes() {
    assert_eq!(
        resource_kind_from_code(1),
        Ok(MirKernelResourceKind::StorageBuffer)
    );
    assert_eq!(
        resource_kind_from_code(2),
        Ok(MirKernelResourceKind::RuntimeExtent)
    );
}

#[test]
fn resource_kind_from_code_rejects_unknown() {
    let error = resource_kind_from_code(0).expect_err("unknown resource kind must error");
    assert_eq!(error.issue, "mir_shader_resource_kind_code_unsupported");
    assert_eq!(error.detail.as_deref(), Some("0"));
}

#[test]
fn resource_role_from_code_accepts_known_codes() {
    assert_eq!(resource_role_from_code(1), Ok(MirKernelResourceRole::Input));
    assert_eq!(
        resource_role_from_code(2),
        Ok(MirKernelResourceRole::Output)
    );
}

#[test]
fn resource_role_from_code_rejects_unknown() {
    let error = resource_role_from_code(3).expect_err("unknown resource role must error");
    assert_eq!(error.issue, "mir_shader_resource_role_code_unsupported");
    assert_eq!(error.detail.as_deref(), Some("3"));
}

#[test]
fn resource_access_from_code_accepts_known_codes() {
    assert_eq!(
        resource_access_from_code(1),
        Ok(MirKernelResourceAccess::Read)
    );
    assert_eq!(
        resource_access_from_code(2),
        Ok(MirKernelResourceAccess::Write)
    );
    assert_eq!(
        resource_access_from_code(3),
        Ok(MirKernelResourceAccess::ReadWrite)
    );
}

#[test]
fn resource_access_from_code_rejects_unknown() {
    let error = resource_access_from_code(4).expect_err("unknown resource access must error");
    assert_eq!(error.issue, "mir_shader_resource_access_code_unsupported");
    assert_eq!(error.detail.as_deref(), Some("4"));
}

#[test]
fn vertex_entry_carries_roles_on_vertex_input_reflections() {
    let entry = vertex_entry_from_shader_contract(
        MirFunctionId(4),
        "plane_vertex",
        &[
            ShaderVertexAttribute {
                semantic: Some("position".to_owned()),
                location: 0,
                format: MirVertexInputFormat::Float32x3,
                offset_bytes: 0,
                stride_bytes: 12,
                step_mode: MirInputStepMode::Vertex,
                role: ShaderStageIoRole::Position,
                provenance: provenance(),
            },
            ShaderVertexAttribute {
                semantic: Some("color".to_owned()),
                location: 1,
                format: MirVertexInputFormat::Float32x4,
                offset_bytes: 0,
                stride_bytes: 16,
                step_mode: MirInputStepMode::Vertex,
                role: ShaderStageIoRole::Color,
                provenance: provenance(),
            },
        ],
        vec![],
    )
    .expect("generic vertex entry");

    let inputs = &entry.vertex_inputs;
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].source_name.as_deref(), Some("position"));
    assert_eq!(inputs[0].role, ShaderStageIoRole::Position);
    assert_eq!(inputs[1].source_name.as_deref(), Some("color"));
    assert_eq!(inputs[1].role, ShaderStageIoRole::Color);
}

#[test]
fn vertex_entry_rejects_duplicate_shader_locations() {
    let error = vertex_entry_from_shader_contract(
        MirFunctionId(4),
        "duplicate_vertex",
        &[
            ShaderVertexAttribute {
                semantic: Some("position".to_owned()),
                location: 0,
                format: MirVertexInputFormat::Float32x3,
                offset_bytes: 0,
                stride_bytes: 12,
                step_mode: MirInputStepMode::Vertex,
                role: ShaderStageIoRole::Position,
                provenance: provenance(),
            },
            ShaderVertexAttribute {
                semantic: Some("normal".to_owned()),
                location: 0,
                format: MirVertexInputFormat::Float32x3,
                offset_bytes: 0,
                stride_bytes: 12,
                step_mode: MirInputStepMode::Vertex,
                role: ShaderStageIoRole::Other,
                provenance: provenance(),
            },
        ],
        vec![],
    )
    .expect_err("duplicate vertex location must fail closed");
    assert_eq!(error.issue, "mir_vertex_input_duplicate_location");
    assert_eq!(error.detail.as_deref(), Some("0"));
}

#[test]
fn source_vertex_entry_sets_source_provenance() {
    let entry = source_vertex_entry_from_shader_contract(
        DefId(42),
        "source_vertex",
        &[ShaderVertexAttribute {
            semantic: Some("position".to_owned()),
            location: 0,
            format: MirVertexInputFormat::Float32x3,
            offset_bytes: 0,
            stride_bytes: 12,
            step_mode: MirInputStepMode::Vertex,
            role: ShaderStageIoRole::Position,
            provenance: provenance(),
        }],
        vec![],
    )
    .expect("source vertex entry");

    assert_eq!(
        entry.provenance,
        MirKernelProvenance::SourceFunction(DefId(42))
    );
    assert_eq!(entry.vertex_inputs[0].role, ShaderStageIoRole::Position);
}

#[test]
fn fragment_entry_carries_varying_and_output_roles() {
    let entry = fragment_entry_from_shader_contract(
        MirFunctionId(5),
        "plane_fragment",
        &[ShaderVarying {
            semantic: "color".to_owned(),
            location: 0,
            format: MirVertexInputFormat::Float32x4,
            interpolation: MirVaryingInterpolation::Perspective,
            role: ShaderStageIoRole::Color,
            provenance: provenance(),
        }],
        &[ShaderFragmentOutput {
            semantic: Some("color".to_owned()),
            location: 0,
            format: MirVertexInputFormat::Float32x4,
            role: ShaderStageIoRole::Color,
            provenance: provenance(),
        }],
        vec![],
    )
    .expect("fragment entry");

    assert_eq!(entry.shader_stage, crate::MirKernelShaderStage::Fragment);
    assert_eq!(entry.varyings[0].source_name, "color");
    assert_eq!(entry.varyings[0].role, ShaderStageIoRole::Color);
    assert_eq!(entry.fragment_outputs[0].location, 0);
    assert_eq!(entry.fragment_outputs[0].role, ShaderStageIoRole::Color);
}

#[test]
fn source_fragment_entry_sets_source_provenance() {
    let entry = source_fragment_entry_from_shader_contract(
        DefId(43),
        "source_fragment",
        &[],
        &[ShaderFragmentOutput {
            semantic: None,
            location: 0,
            format: MirVertexInputFormat::Float32x4,
            role: ShaderStageIoRole::Color,
            provenance: provenance(),
        }],
        vec![],
    )
    .expect("source fragment entry");

    assert_eq!(
        entry.provenance,
        MirKernelProvenance::SourceFunction(DefId(43))
    );
    assert_eq!(entry.fragment_outputs[0].role, ShaderStageIoRole::Color);
}

#[test]
fn fragment_entry_rejects_duplicate_varying_locations() {
    let error = fragment_entry_from_shader_contract(
        MirFunctionId(5),
        "duplicate_varying",
        &[
            ShaderVarying {
                semantic: "color".to_owned(),
                location: 0,
                format: MirVertexInputFormat::Float32x4,
                interpolation: MirVaryingInterpolation::Perspective,
                role: ShaderStageIoRole::Color,
                provenance: provenance(),
            },
            ShaderVarying {
                semantic: "uv".to_owned(),
                location: 0,
                format: MirVertexInputFormat::Float32x2,
                interpolation: MirVaryingInterpolation::Linear,
                role: ShaderStageIoRole::Other,
                provenance: provenance(),
            },
        ],
        &[],
        vec![],
    )
    .expect_err("duplicate varying location must fail closed");
    assert_eq!(error.issue, "mir_varying_duplicate_location");
    assert_eq!(error.detail.as_deref(), Some("0"));
}

#[test]
fn resource_reflection_derives_buffer_byte_len() {
    let binding = ShaderResourceBinding {
        group: 0,
        binding: 1,
        kind: MirKernelResourceKind::StorageBuffer,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty: MirType::semantic(radix_types::TypeId(0)),
        element_layout: MirScalarLayout::F32,
        element_byte_width: 4,
        element_count: 36,
        source_name: Some("transform".to_owned()),
        stage_io_role: ShaderStageIoRole::Transform,
        provenance: provenance(),
    };
    let reflection = resource_reflection_from_shader_contract(&binding);
    assert_eq!(reflection.group, 0);
    assert_eq!(reflection.binding, 1);
    assert_eq!(reflection.kind, MirKernelResourceKind::StorageBuffer);
    assert_eq!(reflection.role, MirKernelResourceRole::Input);
    assert_eq!(reflection.access, MirKernelResourceAccess::Read);
    assert_eq!(reflection.element_layout, MirScalarLayout::F32);
    assert_eq!(reflection.element_byte_width, 4);
    assert_eq!(reflection.element_count, 36);
    assert_eq!(reflection.stage_io_role, ShaderStageIoRole::Transform);
    assert_eq!(reflection.buffer_byte_len, 144);
    assert_eq!(reflection.source_name.as_deref(), Some("transform"));
}

fn vertex_entry() -> MirKernelReflection {
    let mut entry = vertex_entry_from_shader_contract(
        MirFunctionId(4),
        "plane_vertex",
        &[ShaderVertexAttribute {
            semantic: Some("position".to_owned()),
            location: 0,
            format: MirVertexInputFormat::Float32x3,
            offset_bytes: 0,
            stride_bytes: 12,
            step_mode: MirInputStepMode::Vertex,
            role: ShaderStageIoRole::Position,
            provenance: provenance(),
        }],
        vec![],
    )
    .expect("vertex entry");
    // The vertex stage emits the color varying the fragment stage consumes.
    entry.varyings = vec![MirVaryingReflection {
        source_name: "color".to_owned(),
        location: 0,
        format: MirVertexInputFormat::Float32x4,
        interpolation: MirVaryingInterpolation::Perspective,
        role: ShaderStageIoRole::Color,
    }];
    entry
}

fn fragment_entry() -> MirKernelReflection {
    fragment_entry_from_shader_contract(
        MirFunctionId(5),
        "plane_fragment",
        &[ShaderVarying {
            semantic: "color".to_owned(),
            location: 0,
            format: MirVertexInputFormat::Float32x4,
            interpolation: MirVaryingInterpolation::Perspective,
            role: ShaderStageIoRole::Color,
            provenance: provenance(),
        }],
        &[ShaderFragmentOutput {
            semantic: Some("color".to_owned()),
            location: 0,
            format: MirVertexInputFormat::Float32x4,
            role: ShaderStageIoRole::Color,
            provenance: provenance(),
        }],
        vec![],
    )
    .expect("fragment entry")
}

#[test]
fn graphics_pipeline_applies_contract_topology_and_depth() {
    let pipeline = ShaderPipelineState {
        topology: MirPrimitiveTopology::TriangleList,
        color_target_formats: vec![MirColorTargetFormat::Bgra8Unorm],
        depth_stencil: Some(MirDepthStencilState {
            depth_write_enabled: true,
            depth_compare: MirDepthCompareFunction::Less,
            stencil_read_mask: u32::MAX,
            stencil_write_mask: u32::MAX,
            stencil_front: MirStencilFaceState::DISABLED,
            stencil_back: MirStencilFaceState::DISABLED,
        }),
        vertex_count: 36,
        provenance: provenance(),
    };
    let reflection =
        graphics_pipeline_from_shader_contract(&vertex_entry(), &fragment_entry(), &pipeline)
            .expect("graphics pipeline");
    assert_eq!(
        reflection.color_target_formats,
        vec![MirColorTargetFormat::Bgra8Unorm]
    );
    assert_eq!(
        reflection.primitive_topology,
        MirPrimitiveTopology::TriangleList
    );
    assert_eq!(reflection.vertex_count, 36);
    let depth = reflection.depth_stencil.expect("depth state");
    assert_eq!(depth.depth_compare, MirDepthCompareFunction::Less);
    assert!(depth.depth_write_enabled);
    assert_eq!(reflection.vertex_input_count, 1);
    assert_eq!(reflection.varying_count, 1);
}

#[test]
fn graphics_pipeline_rejects_stage_mismatch() {
    let error = graphics_pipeline_from_shader_contract(
        &vertex_entry(),
        &vertex_entry(),
        &ShaderPipelineState {
            topology: MirPrimitiveTopology::TriangleList,
            color_target_formats: vec![MirColorTargetFormat::Bgra8Unorm],
            depth_stencil: None,
            vertex_count: 36,
            provenance: provenance(),
        },
    )
    .expect_err("vertex stage pair without fragment must fail closed");
    assert_eq!(error.issue, "mir_graphics_pipeline_requires_fragment");
}

#[test]
fn bound_resource_reflection_round_trips_and_is_omitted_when_empty() {
    let binding = ShaderBoundResourceBinding {
        group: 1,
        binding: 3,
        source_name: "tex".to_owned(),
        provenance: provenance(),
    };
    let bound = bound_resource_reflection_from_shader_contract(&binding, "zq_handle<f32>");
    assert_eq!(bound.group, 1);
    assert_eq!(bound.binding, 3);
    assert_eq!(bound.name, "tex");
    assert_eq!(bound.bound_type, "zq_handle<f32>");

    let mut reflection = MirKernelReflection::fragment_entry(
        MirFunctionId(0),
        "frag",
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect("fragment entry");
    assert!(reflection.bound_resources.is_empty());

    // Empty: the key is absent, so the JSON is byte-identical to the shape
    // that predates the field.
    let empty_json = serde_json::to_string(&reflection).expect("serializes");
    assert!(!empty_json.contains("bound_resources"));
    assert_eq!(
        empty_json,
        concat!(
            r#"{"function":0,"provenance":{"MirFunction":0},"entry_name":"frag","#,
            r#""shader_stage":"Fragment","resources":[],"vertex_inputs":[],"varyings":[],"#,
            r#""fragment_outputs":[],"builtin_inputs":[],"called_builtins":[],"#,
            r#""workgroup":{"x":1,"y":1,"z":1},"dispatch_size":{"x":1,"y":1,"z":1},"#,
            r#""workgroup_count":{"x":1,"y":1,"z":1}}"#
        )
    );
    assert_eq!(
        serde_json::from_str::<MirKernelReflection>(&empty_json).expect("decodes"),
        reflection
    );

    reflection.bound_resources.push(bound.clone());
    let json = serde_json::to_string(&reflection).expect("serializes");
    assert!(json.contains("\"bound_type\":\"zq_handle<f32>\""));
    assert_eq!(
        serde_json::from_str::<MirKernelReflection>(&json).expect("decodes"),
        reflection
    );
}
