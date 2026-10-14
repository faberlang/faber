use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, ModuleCompileRequest, analyze_source, compile};
use crate::mir::MirKernelShaderStage;
use crate::test_support::test_session_la;

fn compile_source(source: &str) -> crate::CompileResult {
    compile(
        &test_session_la(Target::MirWgsl),
        "shader_contract.fab",
        source,
        ModuleCompileRequest::default(),
    )
}

fn wgsl_output(result: crate::CompileResult) -> crate::mir::MirGpuReflection {
    let output = result.output.expect("wgsl output");
    let crate::Output::WgslText(wgsl) = &output else {
        panic!("expected WGSL output");
    };
    wgsl.reflection.clone().expect("WGSL reflection")
}

const VERTEX_LAYOUT_FN: &str = r#"
@ shader_contract "vertex_layout"
functio vertex_layout_matches(
numerus geo, numerus index, textus source_name,
numerus location, numerus format_code, numerus offset_bytes,
numerus stride_bytes, numerus step_mode_code, numerus role_code
) → bivalens {
redde verum
}
"#;

#[test]
fn contract_vertex_layout_facts_ingest_and_emit() {
    let source = format!(
        r#"
{VERTEX_LAYOUT_FN}
@ vertex
functio shader_vertex() → vacuum {{
}}

incipit {{
adfirma vertex_layout_matches(0, 0, "position", 0, 3, 0, 12, 1, 1) mori "position layout"
adfirma vertex_layout_matches(0, 2, "uv", 2, 2, 0, 8, 1, 4) mori "uv layout"
}}
"#
    );
    let result = compile_source(&source);
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);
    let reflection = wgsl_output(result);
    assert_eq!(reflection.kernels.len(), 1);
    let vertex = &reflection.kernels[0];
    assert_eq!(vertex.shader_stage, MirKernelShaderStage::Vertex);
    assert_eq!(vertex.entry_name, "shader_vertex");
    assert_eq!(vertex.vertex_inputs.len(), 2);
    assert_eq!(vertex.vertex_inputs[0].source_name.as_deref(), Some("position"));
    assert_eq!(vertex.vertex_inputs[0].location, 0);
    assert_eq!(vertex.vertex_inputs[0].format, crate::mir::MirVertexInputFormat::Float32x3);
    assert_eq!(vertex.vertex_inputs[0].stride_bytes, 12);
    assert_eq!(vertex.vertex_inputs[1].source_name.as_deref(), Some("uv"));
    assert_eq!(vertex.vertex_inputs[1].location, 2);
    assert_eq!(vertex.vertex_inputs[1].format, crate::mir::MirVertexInputFormat::Float32x2);
    assert!(matches!(vertex.provenance, crate::mir::MirKernelProvenance::SourceFunction(_)));
}

#[test]
fn contract_graphics_pipeline_assembly_produces_generic_reflection() {
    let source = format!(
        r#"
{VERTEX_LAYOUT_FN}
@ shader_contract "varying"
functio varying_matches(
textus source_name, numerus location, numerus format_code,
numerus interpolation_code, numerus role_code
) → bivalens {{
redde verum
}}

@ shader_contract "fragment_output"
functio fragment_output_matches(
numerus location, numerus format_code, numerus role_code
) → bivalens {{
redde verum
}}

@ shader_contract "pipeline"
functio pipeline_matches(
numerus color_target_format_code, numerus depth_compare_code,
numerus depth_write_enabled_code, numerus primitive_topology_code,
numerus vertex_count
) → bivalens {{
redde verum
}}

@ shader_contract "resource_binding"
functio resource_binding_matches(
numerus group, numerus binding, numerus kind_code, numerus role_code,
numerus access_code, numerus element_count, numerus layout_code,
textus source_name, numerus stage_io_role_code
) → bivalens {{
redde verum
}}

@ vertex
functio shader_vertex() → vacuum {{
}}

@ fragment
functio shader_fragment() → vacuum {{
}}

incipit {{
adfirma vertex_layout_matches(0, 0, "position", 0, 3, 0, 12, 1, 1) mori "position"
adfirma vertex_layout_matches(0, 1, "color", 1, 4, 12, 12, 1, 2) mori "color"
adfirma varying_matches("color", 0, 4, 1, 2) mori "color varying"
adfirma fragment_output_matches(0, 4, 2) mori "color output"
adfirma pipeline_matches(1, 1, 1, 1, 36) mori "pipeline"
adfirma resource_binding_matches(0, 0, 1, 1, 1, 64, 1, "transform", 3) mori "transform buffer"
}}
"#
    );
    let result = compile_source(&source);
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);
    let reflection = wgsl_output(result);

    assert_eq!(reflection.kernels.len(), 2);
    let vertex = reflection
        .kernels
        .iter()
        .find(|k| k.shader_stage == MirKernelShaderStage::Vertex)
        .expect("vertex kernel");
    assert_eq!(vertex.vertex_inputs.len(), 2);
    assert_eq!(vertex.varyings.len(), 1);
    assert_eq!(vertex.varyings[0].source_name, "color");
    assert_eq!(vertex.varyings[0].location, 0);
    assert_eq!(vertex.resources.len(), 1);
    assert_eq!(vertex.resources[0].binding, 0);
    assert_eq!(vertex.resources[0].element_count, 64);
    assert_eq!(vertex.resources[0].source_name.as_deref(), Some("transform"));

    let fragment = reflection
        .kernels
        .iter()
        .find(|k| k.shader_stage == MirKernelShaderStage::Fragment)
        .expect("fragment kernel");
    assert_eq!(fragment.fragment_outputs.len(), 1);
    assert_eq!(fragment.fragment_outputs[0].location, 0);
    assert_eq!(fragment.fragment_outputs[0].format, crate::mir::MirVertexInputFormat::Float32x4);

    let pipeline = reflection
        .pipeline
        .as_ref()
        .expect("graphics pipeline reflection");
    assert_eq!(pipeline.vertex_count, 36);
    assert_eq!(pipeline.vertex_input_count, 2);
    assert_eq!(pipeline.varying_count, 1);
    assert_eq!(pipeline.vertex_resource_count, 1);
    assert_eq!(pipeline.fragment_resource_count, 1);
    assert_eq!(
        pipeline.color_target_formats,
        vec![crate::mir::MirColorTargetFormat::Bgra8Unorm]
    );
    assert_eq!(pipeline.primitive_topology, crate::mir::MirPrimitiveTopology::TriangleList);
    let depth = pipeline.depth_stencil.as_ref().expect("depth stencil");
    assert_eq!(depth.depth_compare, crate::mir::MirDepthCompareFunction::Less);
    assert!(depth.depth_write_enabled);
}

#[test]
fn contract_unknown_role_is_structured_diagnostic() {
    let source = r#"
@ shader_contract "banana"
functio contract_fn() → bivalens {
redde verum
}
"#;
    let result = compile_source(source);
    assert!(!result.success(), "unknown contract role must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("shader_contract_role_unknown"))
    );
}

#[test]
fn contract_non_constant_fact_argument_is_structured_diagnostic() {
    let source = format!(
        r#"
{VERTEX_LAYOUT_FN}
incipit {{
fixum numerus location ← 0
adfirma vertex_layout_matches(0, 0, "position", location, 3, 0, 12, 1, 1) mori "position layout"
}}
"#
    );
    let result = compile_source(&source);
    assert!(!result.success(), "non-constant contract argument must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("shader_contract_arg_non_constant"))
    );
}

#[test]
fn contract_duplicate_fact_is_rejected_not_merged() {
    let source = format!(
        r#"
{VERTEX_LAYOUT_FN}
incipit {{
adfirma vertex_layout_matches(0, 0, "position", 0, 3, 0, 12, 1, 1) mori "position layout"
adfirma vertex_layout_matches(0, 0, "position", 0, 4, 0, 16, 1, 1) mori "conflicting position layout"
}}
"#
    );
    let result = compile_source(&source);
    assert!(!result.success(), "duplicate/conflicting facts must be rejected");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("shader_contract_fact_conflict"))
    );
}

#[test]
fn contract_unknown_code_fails_closed() {
    let source = format!(
        r#"
{VERTEX_LAYOUT_FN}
incipit {{
adfirma vertex_layout_matches(0, 0, "position", 0, 99, 0, 12, 1, 1) mori "bogus format code"
}}
"#
    );
    let result = compile_source(&source);
    assert!(!result.success(), "unknown format code must fail closed");
    let unsupported = result
        .diagnostics
        .iter()
        .find(|diag| diag.issue() == Some("shader_contract_code_unsupported"))
        .expect("rejected code must report the unsupported-code identity");
    assert_eq!(unsupported.code, Some("SHADERCONTRACT002"));
}

#[test]
fn contract_malformed_arity_is_distinct_diagnostic() {
    // The arity guard is reachable for a contract call whose argument
    // count still type-checks (optional `sponte` parameter omitted), so
    // the driver must report the malformed-call identity — never the
    // duplicate/conflict identity.
    let source = r#"
@ shader_contract "vertex_layout"
functio vertex_layout_matches(
numerus geo, numerus index, textus source_name,
numerus location, numerus format_code, numerus offset_bytes,
numerus stride_bytes, numerus step_mode_code sponte
) → bivalens {
redde verum
}

incipit {
adfirma vertex_layout_matches(0, 0, "position", 0, 3, 0, 12) mori "missing step mode code"
}
"#;
    let result = compile_source(source);
    assert!(!result.success(), "wrong-argument-count contract call must fail closed");
    let arity = result
        .diagnostics
        .iter()
        .find(|diag| diag.issue() == Some("shader_contract_arg_count"))
        .expect("arity mismatch must report the arity identity, not the conflict identity");
    assert_eq!(arity.code, Some("SHADERCONTRACT005"));
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diag| diag.issue() != Some("shader_contract_fact_conflict")),
        "arity mismatch must not be reported as a duplicate/conflict"
    );
}

const BOUND_RESOURCE_BINDING_FN: &str = r#"
@ shader_contract "bound_resource_binding"
functio bound_resource_binding_matches(
numerus group, numerus binding, textus source_name
) → bivalens {
redde verum
}
"#;

const RESOURCE_BINDING_FN: &str = r#"
@ shader_contract "resource_binding"
functio resource_binding_matches(
numerus group, numerus binding, numerus kind_code, numerus role_code,
numerus access_code, numerus element_count, numerus layout_code,
textus source_name, numerus stage_io_role_code
) → bivalens {
redde verum
}
"#;

#[test]
fn contract_bound_resource_binding_yields_one_fact() {
    let source = format!(
        r#"
{BOUND_RESOURCE_BINDING_FN}
incipit {{
adfirma bound_resource_binding_matches(1, 2, "tex") mori "tex binding"
}}
"#
    );
    let analysis = analyze_source(
        &test_session_la(Target::MirWgsl),
        "shader_contract.fab",
        &source,
        AnalyzeRequest::default(),
    )
    .unwrap_or_else(|diagnostics| panic!("analysis failed: {diagnostics:?}"));
    let mut diagnostics = Vec::new();
    let facts = super::collect_shader_contract_facts(
        &analysis.hir,
        &analysis.types,
        &analysis.interner,
        "shader_contract.fab",
        &mut diagnostics,
    );
    assert!(diagnostics.is_empty(), "unexpected diagnostics: {diagnostics:?}");
    assert_eq!(facts.bound_resource_bindings.len(), 1);
    let fact = &facts.bound_resource_bindings[0];
    assert_eq!(fact.group, 1);
    assert_eq!(fact.binding, 2);
    assert_eq!(fact.source_name, "tex");
    assert!(fact.provenance.source.is_some());
    assert!(facts.resource_bindings.is_empty());
}

#[test]
fn contract_bound_and_plain_resource_binding_share_conflict_key() {
    let source = format!(
        r#"
{RESOURCE_BINDING_FN}
{BOUND_RESOURCE_BINDING_FN}
incipit {{
adfirma resource_binding_matches(0, 1, 1, 1, 1, 64, 1, "transform", 3) mori "transform buffer"
adfirma bound_resource_binding_matches(0, 1, "tex") mori "tex binding on the same slot"
}}
"#
    );
    let result = compile_source(&source);
    assert!(!result.success(), "same (group, binding) across roles must be rejected");
    let conflict = result
        .diagnostics
        .iter()
        .find(|diag| diag.issue() == Some("shader_contract_fact_conflict"))
        .expect("shared (group, binding) must report the conflict identity");
    assert_eq!(conflict.code, Some("SHADERCONTRACT004"));
}

#[test]
fn contract_bound_resource_binding_non_constant_argument_is_diagnostic() {
    let source = format!(
        r#"
{BOUND_RESOURCE_BINDING_FN}
incipit {{
fixum numerus binding ← 2
adfirma bound_resource_binding_matches(1, binding, "tex") mori "tex binding"
}}
"#
    );
    let result = compile_source(&source);
    assert!(!result.success(), "non-constant bound binding argument must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("shader_contract_arg_non_constant"))
    );
}

#[test]
fn contract_bound_resource_binding_wrong_arity_is_diagnostic() {
    let source = r#"
@ shader_contract "bound_resource_binding"
functio bound_resource_binding_matches(
numerus group, numerus binding, textus source_name sponte
) → bivalens {
redde verum
}

incipit {
adfirma bound_resource_binding_matches(1, 2) mori "missing source name"
}
"#;
    let result = compile_source(source);
    assert!(!result.success(), "wrong-argument-count bound binding call must fail closed");
    let arity = result
        .diagnostics
        .iter()
        .find(|diag| diag.issue() == Some("shader_contract_arg_count"))
        .expect("arity mismatch must report the arity identity");
    assert_eq!(arity.code, Some("SHADERCONTRACT005"));
}
