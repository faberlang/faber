//! Index fault sites of a lane-form kernel (gpu-reset U9-2): the proven-index
//! analysis, the `TensorElementCount` and `FaultRecord` resources, and the
//! reflection's `fault_sites`. Only a kernel with an UNPROVEN index carries
//! them.

use crate::codegen::Target;
use crate::driver::{AnalyzedModule, Config, Session};
use crate::mir::{MirLoweringOptions, lower_analyzed_unit};
use crate::program::analyze_prepared_program_name;
use radix_mir::{
    INDEX_FAULT_TEXT, MirFaultKind, MirKernelReflection, MirKernelResourceKind as Kind, MirKernelSignature,
};

fn analyze(source: &str) -> AnalyzedModule {
    analyze_prepared_program_name(
        &Session::new(Config::default().with_target(Target::HirTypeScript)),
        "fault-sites.fab",
        &format!("+++\nlocale = \"en\"\n+++\n{source}"),
    )
    .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"))
}

/// The device-form signature and reflection of the program's first function.
fn kernel(source: &str) -> (MirKernelSignature, MirKernelReflection) {
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("device lowering validates");
    let signature = MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
        lowered
            .validated
            .program()
            .functions
            .iter()
            .find(|function| function.shader_stage == Some(radix_mir::MirKernelShaderStage::Compute))
            .expect("the program has a compute kernel"),
        lowered.validated.validation(),
        &lowered.interner,
    )
    .expect("the lane kernel has an ABI signature");
    let reflection = MirKernelReflection::from_signature("k", &signature, lowered.validated.validation())
        .expect("the signature reflects");
    (signature, reflection)
}

/// `(binding, kind)` of every resource in binding order.
fn kinds(signature: &MirKernelSignature) -> Vec<(u32, Kind)> {
    signature
        .resources_by_binding()
        .map(|resource| (resource.binding, resource.kind))
        .collect()
}

fn has_fault_channel(signature: &MirKernelSignature) -> bool {
    kinds(signature)
        .iter()
        .any(|(_, kind)| matches!(kind, Kind::TensorElementCount { .. } | Kind::FaultRecord { .. }))
}

/// A generic kernel `k` over `[H, W]` tensors (shared dims are the
/// same-shape-type evidence) plus a caller, so the kernel is not orphaned.
fn program(params: &str, body: &str, call: &str) -> String {
    format!(
        r#"
@ kernel
@ public
fn k<size H, size W>({params}) → void {{
    for from src at [i, j] thread lane const x {{
        {body}
    }}
}}

@ cli "fault-sites"
@ operand int rows
@ operand int cols
main args args {{
    const size R ← args.rows
    const size C ← args.cols
    mut tensor<f32, [R, C]> src ← tf32[R, C] {{}}
    mut tensor<f32, [R, C]> other ← tf32[R, C] {{}}
    mut tensor<f32, [R, C]> dst ← tf32[R, C] {{}}
    {call}
}}
"#
    )
}

const TWO: &str = "tensor<f32, [H, W]> src, mut tensor<f32, [H, W]> dst";
const THREE: &str = "tensor<f32, [H, W]> src, tensor<f32, [H, W]> other, mut tensor<f32, [H, W]> dst";

#[test]
fn a_plain_lane_copy_carries_no_fault_channel() {
    let (signature, reflection) = kernel(&program(TWO, "dst[[i, j]] ← x", "k(src, mut dst)"));
    assert!(!has_fault_channel(&signature), "{:?}", kinds(&signature));
    assert!(reflection.fault_sites.is_empty());
    let json = serde_json::to_string(&reflection).expect("reflection serializes");
    assert!(!json.contains("fault_sites"), "an empty site list is not serialized");
}

#[test]
fn binder_indexed_loads_and_stores_carry_no_fault_channel() {
    let (signature, reflection) = kernel(&program(THREE, "dst[[i, j]] ← x + other[[i, j]]", "k(src, other, mut dst)"));
    assert!(!has_fault_channel(&signature), "{:?}", kinds(&signature));
    assert!(reflection.fault_sites.is_empty());
}

#[test]
fn a_depth_plane_style_vector_kernel_carries_no_fault_channel() {
    let source = r#"
@ kernel
@ public
fn k<size H, size W>(tensor<vector<f32, 4>, [H, W]> state, mut tensor<f32, [H, W]> depth) → void {
    for from state at [i, j] thread lane const c {
        depth[[i, j]] ← c[0]
    }
}

@ cli "fault-sites"
@ operand int rows
@ operand int cols
main args args {
    const size R ← args.rows
    const size C ← args.cols
    mut tensor<vector<f32, 4>, [R, C]> state ← tensor<vector<f32, 4>, [R, C]> {}
    mut tensor<f32, [R, C]> depth ← tf32[R, C] {}
    k(state, mut depth)
}
"#;
    let (signature, reflection) = kernel(source);
    assert!(!has_fault_channel(&signature), "{:?}", kinds(&signature));
    assert!(reflection.fault_sites.is_empty());
}

#[test]
fn a_shifted_load_counts_the_loaded_tensor_only_and_the_record_is_last() {
    let (signature, reflection) =
        kernel(&program(THREE, "dst[[i, j]] ← x + other[[i, j + 1]]", "k(src, other, mut dst)"));
    let kinds = kinds(&signature);
    let counts: Vec<Kind> = kinds
        .iter()
        .map(|(_, kind)| *kind)
        .filter(|kind| matches!(kind, Kind::TensorElementCount { .. }))
        .collect();
    // `other` is the second tensor parameter, binding 1.
    assert_eq!(counts, [Kind::TensorElementCount { tensor_binding: 1 }], "{kinds:?}");
    assert_eq!(
        kinds.last().map(|(_, kind)| *kind),
        Some(Kind::FaultRecord { sites: 1 }),
        "{kinds:?}"
    );
    let first_new = kinds
        .iter()
        .position(|(_, kind)| matches!(kind, Kind::TensorElementCount { .. }))
        .expect("count resource");
    assert!(
        kinds[..first_new]
            .iter()
            .all(|(_, kind)| !matches!(kind, Kind::FaultRecord { .. })),
        "the count precedes the record"
    );
    assert_eq!(reflection.fault_sites.len(), 1);
    let site = &reflection.fault_sites[0];
    assert_eq!(
        (site.id, site.kind, site.text.as_str()),
        (0, MirFaultKind::Fatal, INDEX_FAULT_TEXT)
    );
    let json = serde_json::to_string(&reflection).expect("reflection serializes");
    assert!(json.contains("fault_sites"));
    let back: MirKernelReflection = serde_json::from_str(&json).expect("reflection deserializes");
    assert_eq!(back, reflection, "fault sites round trip");
}

#[test]
fn two_unproven_tensors_get_two_counts_and_two_sites() {
    let (signature, reflection) = kernel(&program(
        THREE,
        "dst[[i, j]] ← src[[i + 1, j]] + other[[i, j + 1]]",
        "k(src, other, mut dst)",
    ));
    let kinds = kinds(&signature);
    let counts: Vec<Kind> = kinds
        .iter()
        .map(|(_, kind)| *kind)
        .filter(|kind| matches!(kind, Kind::TensorElementCount { .. }))
        .collect();
    assert_eq!(
        counts,
        [
            Kind::TensorElementCount { tensor_binding: 0 },
            Kind::TensorElementCount { tensor_binding: 1 }
        ],
        "{kinds:?}"
    );
    assert_eq!(kinds.last().map(|(_, kind)| *kind), Some(Kind::FaultRecord { sites: 2 }));
    let ids: Vec<u32> = reflection.fault_sites.iter().map(|site| site.id).collect();
    assert_eq!(ids, [0, 1], "ids are dense from 0");
}
