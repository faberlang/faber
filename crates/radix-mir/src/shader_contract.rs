//! Domain-neutral shader contract records (WGSL shader-contract boundary, U1).
//!
//! This module is the compiler-owned, generic shader-contract surface that
//! replaces the Triga-shaped fact types (`MirTriga*` in [`crate::abi`]). A
//! shader contract is the set of facts a library declares — vertex layout
//! attributes, stage varyings, fragment outputs, resource bindings, and
//! pipeline state — expressed with canonical fields, explicit stage-I/O
//! [`ShaderStageIoRole`]s, and provenance (resolved source identity + span) for
//! diagnostics.
//!
//! The records are domain-neutral: any library (Triga, …) maps its concrete API
//! onto them, and the driver ingests them (identity-resolved). The roles are
//! carried for the migration; the WGSL emitter consumes them in place of name
//! spellings after U3 lands, with name-spelling detection remaining in the
//! emitter until then. Unknown numeric codes fail closed with [`MirAbiError`] —
//! including the depth-compare table, which must error rather than degrade to
//! `Never`.
//!
//! U1 is additive: [`crate::abi`]'s `MirTriga*` types, `from_triga_*`
//! conversions, and the old builders stay until the cleanup unit (U6). The
//! builders here produce the existing [`MirKernelReflection`] /
//! [`MirGraphicsPipelineReflection`] / [`MirGpuReflection`] shapes unchanged,
//! so the reflection JSON sidecar serializes identically to today.

use crate::{
    MirAbiError, MirColorTargetFormat, MirDepthCompareFunction, MirDepthStencilState,
    MirFragmentOutputReflection, MirFunctionId, MirGraphicsPipelineReflection, MirInputStepMode,
    MirKernelProvenance, MirKernelReflection, MirKernelResourceAccess, MirKernelResourceKind,
    MirKernelResourceReflection, MirKernelResourceRole, MirPrimitiveTopology, MirScalarLayout,
    MirType, MirVaryingInterpolation, MirVaryingReflection, MirVertexInputFormat,
    MirVertexInputReflection,
};
use radix_hir::DefId;
use radix_lexer::Span;
use serde::Serialize;

const ERR_SHADER_VERTEX_FORMAT_CODE: &str = "unsupported shader vertex format code";
const ERR_SHADER_COLOR_TARGET_CODE: &str = "unsupported shader color target format code";
const ERR_SHADER_TOPOLOGY_CODE: &str = "unsupported shader primitive topology code";
const ERR_SHADER_DEPTH_COMPARE_CODE: &str = "unsupported shader depth compare code";
const ERR_SHADER_STEP_MODE_CODE: &str = "unsupported shader input step mode code";
const ERR_SHADER_INTERPOLATION_CODE: &str = "unsupported shader varying interpolation code";
const ERR_SHADER_RESOURCE_KIND_CODE: &str = "unsupported shader resource kind code";
const ERR_SHADER_RESOURCE_ROLE_CODE: &str = "unsupported shader resource role code";
const ERR_SHADER_RESOURCE_ACCESS_CODE: &str = "unsupported shader resource access code";

fn unsupported_code_error(message: &'static str, issue: &'static str, code: u32) -> MirAbiError {
    MirAbiError::with_detail(message, issue, code.to_string())
}

/// Explicit stage-I/O role carried on shader-contract records and on the MIR
/// reflection records the builders produce.
///
/// U1 carries the role on the records for the migration; the WGSL emitter will
/// consume it in place of name spellings after U3 (the role-wiring unit) lands
/// — [`Self::Position`] replacing `source_name == "position"`, [`Self::Color`]
/// replacing `"color"` matching, and [`Self::Transform`] replacing
/// transform-resource detection by name. Until then the emitter keeps
/// name-spelling detection. [`Self::Other`] is the neutral default for
/// attributes with no special stage semantics (normals, texture coordinates,
/// …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum ShaderStageIoRole {
    /// Vertex position output.
    Position,
    /// Color-valued stage input/output.
    Color,
    /// Transform resource (model/world/view-projection state).
    Transform,
    /// No special stage semantics; the neutral default.
    #[default]
    Other,
}

impl ShaderStageIoRole {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Position => "position",
            Self::Color => "color",
            Self::Transform => "transform",
            Self::Other => "other",
        }
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
///
/// Canonical fields replace the `MirTrigaVertexLayoutFact` numeric-code shape:
/// the format is the canonical [`MirVertexInputFormat`], the step mode is the
/// canonical [`MirInputStepMode`], and provenance carries the resolved source
/// identity + span.
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
/// degrading to `Never` as the legacy `MirDepthCompareFunction::from_triga_code`
/// table does.
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

/// Convert a vertex-layout contract attribute into the MIR vertex-input
/// reflection record, carrying the role.
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

/// Convert a varying contract into the MIR varying reflection record, carrying
/// the role.
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

/// Convert a fragment-output contract into the MIR fragment-output reflection
/// record, carrying the role.
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

/// Convert a resource-binding contract into the existing
/// [`MirKernelResourceReflection`] record. The buffer byte length is derived
/// from the canonical element layout + count.
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
        buffer_byte_len: binding
            .element_count
            .saturating_mul(u64::from(binding.element_byte_width)),
        source_local: None,
        source_name: binding.source_name.clone(),
    }
}

/// Build a vertex-stage entry [`MirKernelReflection`] from a vertex-layout
/// contract. Produces the existing reflection shape; the role fields ride on
/// the vertex-input records.
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
    let vertex_inputs = attributes
        .iter()
        .map(vertex_input_reflection_from_shader_contract)
        .collect::<Vec<_>>();
    let mut entry =
        MirKernelReflection::vertex_entry(MirFunctionId(0), entry_name, vertex_inputs, resources)?;
    entry.provenance = MirKernelProvenance::SourceFunction(source_def);
    Ok(entry)
}

/// Build a fragment-stage entry [`MirKernelReflection`] from a varying +
/// fragment-output contract. Produces the existing reflection shape; the role
/// fields ride on the varying and output records.
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
    let varying_reflections = varyings
        .iter()
        .map(varying_reflection_from_shader_contract)
        .collect::<Vec<_>>();
    let output_reflections = fragment_outputs
        .iter()
        .map(fragment_output_reflection_from_shader_contract)
        .collect::<Vec<_>>();
    MirKernelReflection::source_fragment_entry(
        source_def,
        entry_name,
        varying_reflections,
        output_reflections,
        resources,
    )
}

/// Build a [`MirGraphicsPipelineReflection`] from a vertex + fragment stage
/// pair and a pipeline-state contract.
///
/// Reuses the existing cross-stage validation (stage kinds, vertex inputs
/// present, color targets present, varying consistency) and then applies the
/// contract's topology and depth/stencil state.
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
mod tests {
    use super::*;
    use crate::{MirStencilFaceState, MirTrigaPipelineFact, MirTrigaVertexLayoutFact};

    fn provenance() -> ContractProvenance {
        ContractProvenance {
            source: Some(DefId(7)),
            span: Span::new(0, 4),
        }
    }

    #[test]
    fn role_spellings_are_stable() {
        assert_eq!(ShaderStageIoRole::Position.spelling(), "position");
        assert_eq!(ShaderStageIoRole::Color.spelling(), "color");
        assert_eq!(ShaderStageIoRole::Transform.spelling(), "transform");
        assert_eq!(ShaderStageIoRole::Other.spelling(), "other");
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
        // `Never` like the legacy fail-open `from_triga_code` table.
        let error = depth_compare_function_from_code(0).expect_err("unknown depth code must error");
        assert_eq!(error.issue, "mir_shader_depth_compare_code_unsupported");
        assert_eq!(error.detail.as_deref(), Some("0"));
        let error =
            depth_compare_function_from_code(99).expect_err("unknown depth code must error");
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
        let error =
            varying_interpolation_from_code(4).expect_err("unknown interpolation must error");
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

    /// Equivalent input through the legacy Triga-facts path and the generic
    /// contract must produce the identical vertex reflection (the JSON sidecar
    /// serializes exactly these fields), proving the round-trip shape.
    #[test]
    fn generic_vertex_entry_matches_triga_facts_path() {
        let generic = vertex_entry_from_shader_contract(
            MirFunctionId(4),
            "triga_plane_vertex",
            &[
                ShaderVertexAttribute {
                    semantic: Some("position".to_owned()),
                    location: 0,
                    format: MirVertexInputFormat::Float32x3,
                    offset_bytes: 0,
                    stride_bytes: 12,
                    step_mode: MirInputStepMode::Vertex,
                    role: ShaderStageIoRole::Other,
                    provenance: provenance(),
                },
                ShaderVertexAttribute {
                    semantic: Some("uv".to_owned()),
                    location: 2,
                    format: MirVertexInputFormat::Float32x2,
                    offset_bytes: 0,
                    stride_bytes: 8,
                    step_mode: MirInputStepMode::Vertex,
                    role: ShaderStageIoRole::Other,
                    provenance: provenance(),
                },
            ],
            vec![],
        )
        .expect("generic vertex entry");

        let triga = MirKernelReflection::vertex_entry_from_triga_layout_facts(
            MirFunctionId(4),
            "triga_plane_vertex",
            &[
                MirTrigaVertexLayoutFact {
                    source_name: Some("position".to_owned()),
                    location: 0,
                    format_code: 3,
                    offset_bytes: 0,
                    stride_bytes: 12,
                    step_mode_code: 1,
                },
                MirTrigaVertexLayoutFact {
                    source_name: Some("uv".to_owned()),
                    location: 2,
                    format_code: 2,
                    offset_bytes: 0,
                    stride_bytes: 8,
                    step_mode_code: 1,
                },
            ],
            vec![],
        )
        .expect("Triga layout facts");

        assert_eq!(generic, triga);
    }

    #[test]
    fn vertex_entry_carries_roles_on_vertex_input_reflections() {
        let entry = vertex_entry_from_shader_contract(
            MirFunctionId(4),
            "triga_plane_vertex",
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
            "triga_plane_fragment",
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
        assert_eq!(reflection.buffer_byte_len, 144);
        assert_eq!(reflection.source_name.as_deref(), Some("transform"));
    }

    fn vertex_entry() -> MirKernelReflection {
        let mut entry = vertex_entry_from_shader_contract(
            MirFunctionId(4),
            "triga_plane_vertex",
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
            "triga_plane_fragment",
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

    /// The legacy Triga pipeline builder shares the fail-closed depth table:
    /// unknown depth codes error there too, never degrading to `Never`.
    #[test]
    fn triga_pipeline_path_errors_on_unknown_depth_code() {
        let fact = MirTrigaPipelineFact {
            color_target_format_code: 1,
            depth_compare_code: Some(99),
            depth_write_enabled: true,
            primitive_topology_code: 1,
            vertex_count: 36,
        };
        let error = fact
            .to_graphics_pipeline_reflection()
            .expect_err("unknown depth code must error, not degrade to Never");
        assert_eq!(error.issue, "mir_shader_depth_compare_code_unsupported");
        assert_eq!(error.detail.as_deref(), Some("99"));
    }
}
