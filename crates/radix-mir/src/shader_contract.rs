//! Domain-neutral shader contract records.
//!
//! This module owns the compiler-defined shader-contract surface: vertex-layout
//! attributes, stage varyings, fragment outputs, resource bindings, and
//! pipeline state. Each fact carries canonical fields, an explicit
//! [`ShaderStageIoRole`], and provenance (resolved source identity + span) for
//! diagnostics.
//!
//! Libraries map their APIs onto these records. Identity-resolved ingestion
//! lives in the driver. Stage semantics come from the declared role, not from
//! source-name spelling. Unknown numeric codes fail closed with
//! [`MirAbiError`] — including the depth-compare table, which must error
//! rather than degrade to `Never`.
//!
//! Builders project the records onto [`MirKernelReflection`] and
//! [`MirGraphicsPipelineReflection`].

use crate::{
    MirAbiError, MirBoundResourceReflection, MirColorTargetFormat, MirDepthCompareFunction,
    MirDepthStencilState, MirFragmentOutputReflection, MirFunctionId,
    MirGraphicsPipelineReflection, MirInputStepMode, MirKernelProvenance, MirKernelReflection,
    MirKernelResourceAccess, MirKernelResourceKind, MirKernelResourceReflection,
    MirKernelResourceRole, MirPrimitiveTopology, MirScalarLayout, MirType, MirVaryingInterpolation,
    MirVaryingReflection, MirVertexInputFormat, MirVertexInputReflection,
};
use radix_hir::DefId;
use radix_lexer::Span;
use serde::{Deserialize, Serialize};

const ERR_SHADER_VERTEX_FORMAT_CODE: &str = "unsupported shader vertex format code";
const ERR_SHADER_COLOR_TARGET_CODE: &str = "unsupported shader color target format code";
const ERR_SHADER_TOPOLOGY_CODE: &str = "unsupported shader primitive topology code";
const ERR_SHADER_DEPTH_COMPARE_CODE: &str = "unsupported shader depth compare code";
const ERR_SHADER_STEP_MODE_CODE: &str = "unsupported shader input step mode code";
const ERR_SHADER_INTERPOLATION_CODE: &str = "unsupported shader varying interpolation code";
const ERR_SHADER_RESOURCE_KIND_CODE: &str = "unsupported shader resource kind code";
const ERR_SHADER_RESOURCE_ROLE_CODE: &str = "unsupported shader resource role code";
const ERR_SHADER_RESOURCE_ACCESS_CODE: &str = "unsupported shader resource access code";
const ERR_SHADER_STAGE_IO_ROLE_CODE: &str = "unsupported shader stage-I/O role code";

fn unsupported_code_error(message: &'static str, issue: &'static str, code: u32) -> MirAbiError {
    MirAbiError::with_detail(message, issue, code.to_string())
}

/// Explicit stage-I/O role on contract records and on the reflection records
/// the builders produce.
///
/// Stage semantics come from this role, not from source-name spelling.
/// [`Self::Position`] is the vertex-position attribute, [`Self::Color`] is
/// color-valued stage I/O, and [`Self::Transform`] is the transform storage
/// resource. [`Self::Other`] is the default for attributes with no special
/// stage semantics (normals, texture coordinates, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShaderStageIoRole {
    /// Vertex position attribute.
    Position,
    /// Color-valued stage input/output.
    Color,
    /// Transform resource (model/world/view-projection state).
    Transform,
    /// No special stage semantics; the neutral default.
    #[default]
    Other,
}

/// # Errors
/// Returns an error when the numeric code is not a canonical stage-I/O role.
///
/// Unknown codes fail closed. `Other` (the default) is code `4`.
pub fn stage_io_role_from_code(code: u32) -> Result<ShaderStageIoRole, MirAbiError> {
    match code {
        1 => Ok(ShaderStageIoRole::Position),
        2 => Ok(ShaderStageIoRole::Color),
        3 => Ok(ShaderStageIoRole::Transform),
        4 => Ok(ShaderStageIoRole::Other),
        _ => Err(unsupported_code_error(
            ERR_SHADER_STAGE_IO_ROLE_CODE,
            "shader_stage_io_role_code",
            code,
        )),
    }
}

/// Provenance of a shader-contract fact: the resolved source identity it traces
/// to (when known) plus the source span for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContractProvenance {
    /// Resolved source definition the fact traces to, when available.
    pub source: Option<DefId>,
    /// Source span for diagnostics, when available.
    pub span: Span,
}

/// Vertex-layout contract: one vertex buffer attribute (shader input).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderVertexAttribute {
    /// Semantic/source identity of the attribute (source member name).
    pub semantic: Option<String>,
    /// Vertex input location (shader location).
    pub location: u32,
    /// Canonical vertex input format.
    pub format: MirVertexInputFormat,
    /// Byte offset of the attribute within its vertex buffer.
    pub offset_bytes: u64,
    /// Byte stride of the vertex buffer.
    pub stride_bytes: u64,
    /// Per-vertex or per-instance step mode.
    pub step_mode: MirInputStepMode,
    /// Explicit stage-I/O role.
    pub role: ShaderStageIoRole,
    /// Provenance + source span for diagnostics.
    pub provenance: ContractProvenance,
}

/// Stage-varying contract: a value the vertex stage writes and the fragment
/// stage reads. Location, format, and interpolation must agree across stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderVarying {
    /// Semantic/source identity of the varying.
    pub semantic: String,
    /// Varying location.
    pub location: u32,
    /// Canonical varying format.
    pub format: MirVertexInputFormat,
    /// Interpolation mode (perspective, linear, flat).
    pub interpolation: MirVaryingInterpolation,
    /// Explicit stage-I/O role.
    pub role: ShaderStageIoRole,
    /// Provenance + source span for diagnostics.
    pub provenance: ContractProvenance,
}

/// Fragment render-target output contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderFragmentOutput {
    /// Semantic/source identity of the output, when known.
    pub semantic: Option<String>,
    /// Render-target output location.
    pub location: u32,
    /// Canonical output format.
    pub format: MirVertexInputFormat,
    /// Explicit stage-I/O role.
    pub role: ShaderStageIoRole,
    /// Provenance + source span for diagnostics.
    pub provenance: ContractProvenance,
}

/// Resource-binding contract: a GPU resource a stage consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderResourceBinding {
    /// Bind group index.
    pub group: u32,
    /// Binding index within the bind group.
    pub binding: u32,
    /// Canonical resource kind (storage buffer, runtime extent).
    pub kind: MirKernelResourceKind,
    /// Canonical resource role (input/output).
    pub role: MirKernelResourceRole,
    /// Canonical resource access (read/write/read-write).
    pub access: MirKernelResourceAccess,
    /// Element type as reflected in MIR.
    pub element_ty: MirType,
    /// Canonical scalar element layout.
    pub element_layout: MirScalarLayout,
    /// Element byte width.
    pub element_byte_width: u16,
    /// Element count.
    pub element_count: u64,
    /// Source name of the binding, when known.
    pub source_name: Option<String>,
    /// Explicit stage-I/O role (declared on the contract call).
    pub stage_io_role: ShaderStageIoRole,
    /// Provenance + source span for diagnostics.
    pub provenance: ContractProvenance,
}

/// Bound-resource contract: a stage parameter of an opaque library type that
/// occupies a `(group, binding)` slot. The bound type's name is supplied by the
/// caller when the fact is projected onto reflection; the compiler attaches no
/// meaning to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderBoundResourceBinding {
    /// Bind group index.
    pub group: u32,
    /// Binding index within the bind group.
    pub binding: u32,
    /// Source name of the bound stage parameter.
    pub source_name: String,
    /// Provenance + source span for diagnostics.
    pub provenance: ContractProvenance,
}

/// Pipeline-state contract: draw-prerequisite configuration for a graphics
/// pipeline (topology, color targets, depth/stencil, vertex count).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderPipelineState {
    /// Primitive topology for draw calls.
    pub topology: MirPrimitiveTopology,
    /// Color attachment formats, in target order.
    pub color_target_formats: Vec<MirColorTargetFormat>,
    /// Depth/stencil state, when the pipeline has a depth attachment.
    pub depth_stencil: Option<MirDepthStencilState>,
    /// Vertex count for the draw.
    pub vertex_count: u32,
    /// Provenance + source span for diagnostics.
    pub provenance: ContractProvenance,
}

/// # Errors
/// Returns an error when the numeric code is not a canonical vertex format.
pub fn vertex_format_from_code(code: u32) -> Result<MirVertexInputFormat, MirAbiError> {
    match code {
        2 => Ok(MirVertexInputFormat::Float32x2),
        3 => Ok(MirVertexInputFormat::Float32x3),
        4 => Ok(MirVertexInputFormat::Float32x4),
        _ => Err(unsupported_code_error(
            ERR_SHADER_VERTEX_FORMAT_CODE,
            "mir_shader_format_code_unsupported",
            code,
        )),
    }
}

/// # Errors
/// Returns an error when the numeric code is not a canonical color target.
pub fn color_target_format_from_code(code: u32) -> Result<MirColorTargetFormat, MirAbiError> {
    match code {
        1 => Ok(MirColorTargetFormat::Bgra8Unorm),
        2 => Ok(MirColorTargetFormat::Bgra8UnormSrgb),
        _ => Err(unsupported_code_error(
            ERR_SHADER_COLOR_TARGET_CODE,
            "mir_shader_color_target_code_unsupported",
            code,
        )),
    }
}

/// # Errors
/// Returns an error when the numeric code is not a canonical topology.
pub fn primitive_topology_from_code(code: u32) -> Result<MirPrimitiveTopology, MirAbiError> {
    match code {
        1 => Ok(MirPrimitiveTopology::TriangleList),
        2 => Ok(MirPrimitiveTopology::LineList),
        3 => Ok(MirPrimitiveTopology::PointList),
        _ => Err(unsupported_code_error(
            ERR_SHADER_TOPOLOGY_CODE,
            "mir_shader_topology_code_unsupported",
            code,
        )),
    }
}

/// # Errors
/// Returns an error when the numeric code is not a canonical depth compare.
///
/// Fail-closed by design (council lock): unknown depth codes error instead of
/// degrading to `Never`.
pub fn depth_compare_function_from_code(code: u32) -> Result<MirDepthCompareFunction, MirAbiError> {
    match code {
        1 => Ok(MirDepthCompareFunction::Less),
        2 => Ok(MirDepthCompareFunction::Equal),
        3 => Ok(MirDepthCompareFunction::LessEqual),
        4 => Ok(MirDepthCompareFunction::Greater),
        5 => Ok(MirDepthCompareFunction::NotEqual),
        6 => Ok(MirDepthCompareFunction::GreaterEqual),
        7 => Ok(MirDepthCompareFunction::Always),
        _ => Err(unsupported_code_error(
            ERR_SHADER_DEPTH_COMPARE_CODE,
            "mir_shader_depth_compare_code_unsupported",
            code,
        )),
    }
}

/// # Errors
/// Returns an error when the numeric code is not a canonical input step mode.
pub fn input_step_mode_from_code(code: u32) -> Result<MirInputStepMode, MirAbiError> {
    match code {
        1 => Ok(MirInputStepMode::Vertex),
        2 => Ok(MirInputStepMode::Instance),
        _ => Err(unsupported_code_error(
            ERR_SHADER_STEP_MODE_CODE,
            "mir_shader_step_mode_code_unsupported",
            code,
        )),
    }
}

/// # Errors
/// Returns an error when the numeric code is not a canonical interpolation.
pub fn varying_interpolation_from_code(code: u32) -> Result<MirVaryingInterpolation, MirAbiError> {
    match code {
        1 => Ok(MirVaryingInterpolation::Perspective),
        2 => Ok(MirVaryingInterpolation::Linear),
        3 => Ok(MirVaryingInterpolation::Flat),
        _ => Err(unsupported_code_error(
            ERR_SHADER_INTERPOLATION_CODE,
            "mir_shader_interpolation_code_unsupported",
            code,
        )),
    }
}

/// # Errors
/// Returns an error when the numeric code is not a canonical resource kind.
pub fn resource_kind_from_code(code: u32) -> Result<MirKernelResourceKind, MirAbiError> {
    match code {
        1 => Ok(MirKernelResourceKind::StorageBuffer),
        2 => Ok(MirKernelResourceKind::RuntimeExtent),
        _ => Err(unsupported_code_error(
            ERR_SHADER_RESOURCE_KIND_CODE,
            "mir_shader_resource_kind_code_unsupported",
            code,
        )),
    }
}

/// # Errors
/// Returns an error when the numeric code is not a canonical resource role.
pub fn resource_role_from_code(code: u32) -> Result<MirKernelResourceRole, MirAbiError> {
    match code {
        1 => Ok(MirKernelResourceRole::Input),
        2 => Ok(MirKernelResourceRole::Output),
        _ => Err(unsupported_code_error(
            ERR_SHADER_RESOURCE_ROLE_CODE,
            "mir_shader_resource_role_code_unsupported",
            code,
        )),
    }
}

/// # Errors
/// Returns an error when the numeric code is not a canonical resource access.
pub fn resource_access_from_code(code: u32) -> Result<MirKernelResourceAccess, MirAbiError> {
    match code {
        1 => Ok(MirKernelResourceAccess::Read),
        2 => Ok(MirKernelResourceAccess::Write),
        3 => Ok(MirKernelResourceAccess::ReadWrite),
        _ => Err(unsupported_code_error(
            ERR_SHADER_RESOURCE_ACCESS_CODE,
            "mir_shader_resource_access_code_unsupported",
            code,
        )),
    }
}

/// Project a vertex-layout contract attribute onto [`MirVertexInputReflection`].
#[must_use]
pub fn vertex_input_reflection_from_shader_contract(
    attribute: &ShaderVertexAttribute,
) -> MirVertexInputReflection {
    MirVertexInputReflection {
        source_name: attribute.semantic.clone(),
        location: attribute.location,
        format: attribute.format,
        step_mode: attribute.step_mode,
        offset_bytes: attribute.offset_bytes,
        stride_bytes: attribute.stride_bytes,
        role: attribute.role,
    }
}

/// Project a varying contract onto [`MirVaryingReflection`].
#[must_use]
pub fn varying_reflection_from_shader_contract(varying: &ShaderVarying) -> MirVaryingReflection {
    MirVaryingReflection {
        source_name: varying.semantic.clone(),
        location: varying.location,
        format: varying.format,
        interpolation: varying.interpolation,
        role: varying.role,
    }
}

/// Project a fragment-output contract onto [`MirFragmentOutputReflection`].
#[must_use]
pub fn fragment_output_reflection_from_shader_contract(
    output: &ShaderFragmentOutput,
) -> MirFragmentOutputReflection {
    MirFragmentOutputReflection {
        location: output.location,
        format: output.format,
        role: output.role,
    }
}

/// Project a resource-binding contract onto [`MirKernelResourceReflection`].
///
/// Buffer byte length is `element_count * element_byte_width`.
#[must_use]
pub fn resource_reflection_from_shader_contract(
    binding: &ShaderResourceBinding,
) -> MirKernelResourceReflection {
    MirKernelResourceReflection {
        group: binding.group,
        binding: binding.binding,
        kind: binding.kind,
        role: binding.role,
        access: binding.access,
        element_ty: binding.element_ty,
        element_layout: binding.element_layout,
        element_byte_width: binding.element_byte_width,
        element_count: binding.element_count,
        shape: None,
        buffer_byte_len: binding
            .element_count
            .saturating_mul(u64::from(binding.element_byte_width)),
        source_local: None,
        source_name: binding.source_name.clone(),
        stage_io_role: binding.stage_io_role,
    }
}

/// Project a bound-resource contract onto [`MirBoundResourceReflection`].
#[must_use]
pub fn bound_resource_reflection_from_shader_contract(
    binding: &ShaderBoundResourceBinding,
    bound_type: impl Into<String>,
) -> MirBoundResourceReflection {
    MirBoundResourceReflection {
        group: binding.group,
        binding: binding.binding,
        name: binding.source_name.clone(),
        bound_type: bound_type.into(),
    }
}

/// Build a vertex-stage entry [`MirKernelReflection`] from a vertex-layout
/// contract.
///
/// # Errors
/// Returns an error when the contract has duplicate vertex-input locations.
pub fn vertex_entry_from_shader_contract(
    function: MirFunctionId,
    entry_name: impl Into<String>,
    attributes: &[ShaderVertexAttribute],
    resources: Vec<MirKernelResourceReflection>,
) -> Result<MirKernelReflection, MirAbiError> {
    let vertex_inputs = attributes
        .iter()
        .map(vertex_input_reflection_from_shader_contract)
        .collect::<Vec<_>>();
    MirKernelReflection::vertex_entry(function, entry_name, vertex_inputs, resources)
}

/// Build a source-annotation vertex-stage entry [`MirKernelReflection`] from a
/// vertex-layout contract: provenance is the resolved source definition.
///
/// # Errors
/// Returns an error when the contract has duplicate vertex-input locations.
pub fn source_vertex_entry_from_shader_contract(
    source_def: DefId,
    entry_name: impl Into<String>,
    attributes: &[ShaderVertexAttribute],
    resources: Vec<MirKernelResourceReflection>,
) -> Result<MirKernelReflection, MirAbiError> {
    let mut entry =
        vertex_entry_from_shader_contract(MirFunctionId(0), entry_name, attributes, resources)?;
    entry.provenance = MirKernelProvenance::SourceFunction(source_def);
    Ok(entry)
}

/// Build a fragment-stage entry [`MirKernelReflection`] from a varying +
/// fragment-output contract.
///
/// # Errors
/// Returns an error when the contract has duplicate varying or output
/// locations.
pub fn fragment_entry_from_shader_contract(
    function: MirFunctionId,
    entry_name: impl Into<String>,
    varyings: &[ShaderVarying],
    fragment_outputs: &[ShaderFragmentOutput],
    resources: Vec<MirKernelResourceReflection>,
) -> Result<MirKernelReflection, MirAbiError> {
    let varying_reflections = varyings
        .iter()
        .map(varying_reflection_from_shader_contract)
        .collect::<Vec<_>>();
    let output_reflections = fragment_outputs
        .iter()
        .map(fragment_output_reflection_from_shader_contract)
        .collect::<Vec<_>>();
    MirKernelReflection::fragment_entry(
        function,
        entry_name,
        varying_reflections,
        output_reflections,
        resources,
    )
}

/// Build a source-annotation fragment-stage entry [`MirKernelReflection`] from
/// a varying + fragment-output contract.
///
/// # Errors
/// Returns an error when the contract has duplicate varying or output
/// locations.
pub fn source_fragment_entry_from_shader_contract(
    source_def: DefId,
    entry_name: impl Into<String>,
    varyings: &[ShaderVarying],
    fragment_outputs: &[ShaderFragmentOutput],
    resources: Vec<MirKernelResourceReflection>,
) -> Result<MirKernelReflection, MirAbiError> {
    let mut entry = fragment_entry_from_shader_contract(
        MirFunctionId(0),
        entry_name,
        varyings,
        fragment_outputs,
        resources,
    )?;
    entry.provenance = MirKernelProvenance::SourceFunction(source_def);
    Ok(entry)
}

/// Build a [`MirGraphicsPipelineReflection`] from a vertex + fragment stage
/// pair and a pipeline-state contract.
///
/// Cross-stage validation is the existing kernel reflection check. This
/// builder then applies the contract's topology and depth/stencil state.
///
/// # Errors
/// Returns an error when the stage pair or the pipeline contract is invalid
/// (see [`crate::MirKernelReflection::graphics_pipeline_reflection`]).
pub fn graphics_pipeline_from_shader_contract(
    vertex: &MirKernelReflection,
    fragment: &MirKernelReflection,
    pipeline: &ShaderPipelineState,
) -> Result<MirGraphicsPipelineReflection, MirAbiError> {
    let mut reflection = MirKernelReflection::graphics_pipeline_reflection(
        vertex,
        fragment,
        pipeline.vertex_count,
        &pipeline.color_target_formats,
    )?;
    reflection.primitive_topology = pipeline.topology;
    reflection.depth_stencil = pipeline.depth_stencil;
    Ok(reflection)
}

#[cfg(test)]
#[path = "shader_contract_test.rs"]
mod tests;
