//! Index fault sites of a lane-form or single-lane kernel (gpu-reset U9-2): the proven-index
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
    for from src at [i, j] thread lane let x {{
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
fn k<size H, size W>(tensor<vector<f32, size 4>, [H, W]> state, mut tensor<f32, [H, W]> depth) → void {
    for from state at [i, j] thread lane let c {
        depth[[i, j]] ← c[0]
    }
}

@ cli "fault-sites"
@ operand int rows
@ operand int cols
main args args {
    const size R ← args.rows
    const size C ← args.cols
    mut tensor<vector<f32, size 4>, [R, C]> state ← tensor<vector<f32, size 4>, [R, C]> {}
    mut tensor<f32, [R, C]> depth ← tf32[R, C] {}
    k(state, mut depth)
}
"#;
    let (signature, reflection) = kernel(source);
    assert!(!has_fault_channel(&signature), "{:?}", kinds(&signature));
    assert!(reflection.fault_sites.is_empty());
}

#[test]
fn a_run_time_register_vector_index_gets_a_fault_site_without_a_tensor_count() {
    let source = r#"
@ kernel
@ public
fn k<size H, size W>(tensor<vector<f32, size 4>, [H, W]> state, i32 pick, mut tensor<f32, [H, W]> depth) → void {
    for from state at [i, j] thread lane let c {
        depth[[i, j]] ← c[pick]
    }
}

main {
    mut tensor<vector<f32, size 4>, [2, 2]> state ← tensor<vector<f32, size 4>, [2, 2]> {}
    mut tensor<f32, [2, 2]> depth ← tf32[2, 2] {}
    k(state, 1, mut depth)
}
"#;
    let (signature, reflection) = kernel(source);
    let resource_kinds = kinds(&signature);
    assert!(
        resource_kinds
            .iter()
            .all(|(_, kind)| !matches!(kind, Kind::TensorElementCount { .. })),
        "a vector-width guard does not need tensor counts: {resource_kinds:?}"
    );
    assert_eq!(
        resource_kinds.last().map(|(_, kind)| *kind),
        Some(Kind::FaultRecord { sites: 1 }),
        "the run-time vector index has one fatal site: {resource_kinds:?}"
    );
    assert_eq!(
        reflection
            .fault_sites
            .iter()
            .map(|site| (site.id, site.kind, site.text.as_str()))
            .collect::<Vec<_>>(),
        [(0, MirFaultKind::Fatal, INDEX_FAULT_TEXT)]
    );
}

#[test]
fn a_single_lane_run_time_vector_index_uses_the_same_fault_channel() {
    let source = r#"
@ kernel
@ public
fn poke(i32 pick, mut tensor<f32, [1]> dst) → void {
    let vector<f32, size 4> value ← [0.0, 1.0, 2.0, 3.0] ↦ vector<f32, size 4>
    dst[0] ← value[pick]
}

main {
    mut tensor<f32, [1]> dst ← tf32[1] {}
    poke(-1, mut dst)
}
"#;
    let (signature, reflection) = kernel(source);
    let resource_kinds = kinds(&signature);
    assert_eq!(
        resource_kinds.last().map(|(_, kind)| *kind),
        Some(Kind::FaultRecord { sites: 1 }),
        "the single-lane vector index has one fatal site: {resource_kinds:?}"
    );
    assert!(
        resource_kinds
            .iter()
            .all(|(_, kind)| !matches!(kind, Kind::TensorElementCount { .. })),
        "only the vector width is checked: {resource_kinds:?}"
    );
    assert_eq!(reflection.fault_sites.len(), 1);
    assert_eq!(reflection.fault_sites[0].text, INDEX_FAULT_TEXT);
}

#[test]
fn a_single_lane_run_time_vector_store_gets_a_fault_site() {
    let source = r#"
@ kernel
@ public
fn poke(i32 pick, mut tensor<f32, [1]> dst) → void {
    mut vector<f32, size 4> value ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, size 4>
    value[pick] ← 9.0
    dst[0] ← value[0] + value[1] + value[2] + value[3]
}

main {
    mut tensor<f32, [1]> dst ← tf32[1] {}
    poke(3, mut dst)
}
"#;
    let (signature, reflection) = kernel(source);
    let resource_kinds = kinds(&signature);
    assert_eq!(
        resource_kinds.last().map(|(_, kind)| *kind),
        Some(Kind::FaultRecord { sites: 1 }),
        "the vector store has one fatal site: {resource_kinds:?}"
    );
    assert!(
        resource_kinds
            .iter()
            .all(|(_, kind)| !matches!(kind, Kind::TensorElementCount { .. })),
        "a register-vector store checks its width directly: {resource_kinds:?}"
    );
    assert_eq!(reflection.fault_sites.len(), 1);
    assert_eq!(reflection.fault_sites[0].text, INDEX_FAULT_TEXT);
}

fn nested_vector_store_source(width: u8, tensor_index: &str, vector_index: &str) -> String {
    format!(
        r#"
@ kernel
@ public
fn poke(i32 row, i32 pick, mut tensor<vector<f32, size {width}>, [3]> src) → void {{
    src[{tensor_index}][{vector_index}] ← 9.0
}}

main {{
    mut tensor<vector<f32, size {width}>, [3]> src ← tensor<vector<f32, size {width}>, [3]> {{}}
    poke(0, 1, mut src)
}}
"#
    )
}

#[test]
fn nested_tensor_vector_store_fault_sites_cover_index_combinations() {
    for (width, tensor_index, vector_index, expected_faults) in [
        (2, "0", "1", 0_usize),
        (2, "row", "1", 1),
        (4, "0", "pick", 1),
        (4, "row", "pick", 2),
    ] {
        let source = nested_vector_store_source(width, tensor_index, vector_index);
        let (signature, reflection) = kernel(&source);
        let resources = kinds(&signature);
        let actual_faults = resources.iter().find_map(|(_, kind)| match kind {
            Kind::FaultRecord { sites } => Some(*sites as usize),
            _ => None,
        });
        assert_eq!(
            actual_faults,
            (expected_faults > 0).then_some(expected_faults),
            "width {width}, src[{tensor_index}][{vector_index}]: {resources:?}"
        );
        assert_eq!(
            resources
                .iter()
                .filter(|(_, kind)| matches!(kind, Kind::TensorElementCount { .. }))
                .count(),
            if tensor_index == "row" { 1 } else { 0 },
            "only a dynamic tensor index needs an element count"
        );
        assert_eq!(reflection.fault_sites.len(), expected_faults);
    }
}

#[cfg(feature = "mir-metal")]
#[test]
fn nested_tensor_vector_store_guards_each_unproven_index_in_metal_text() {
    for (width, tensor_index, vector_index, expected_faults) in [
        (2, "0", "1", 0_usize),
        (2, "row", "1", 1),
        (4, "0", "pick", 1),
        (4, "row", "pick", 2),
    ] {
        let source = nested_vector_store_source(width, tensor_index, vector_index);
        let source = format!("+++\nlocale = \"en\"\n+++\n{source}");
        let text =
            super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
        if expected_faults == 0 {
            assert!(!text.contains("u9_fault["), "{text}");
        } else {
            for site in 0..expected_faults {
                assert!(
                    deferred_slots(&text, expected_faults).contains(&site),
                    "missing fault site {site}: {text}"
                );
            }
        }
        if vector_index == "pick" {
            assert!(
                text.contains(&format!("(ulong)({width}u)")),
                "the vector guard uses its declared width: {text}"
            );
        }
    }
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

/// A single-lane kernel `poke` (no `thread lane` loop, the SingleLane plan):
/// `src[k + read]` and `dst[k + write]` are unproven, `dst[0]` is a literal
/// and `body` is the loop body.
fn single_lane(body: &str, tail: &str) -> String {
    format!(
        r#"
@ kernel
@ public
fn poke(tensor<f32, [4]> src, i32 read, i32 write, mut tensor<f32, [8]> dst) → void {{
    let i32 k_lo ← 0
    let i32 k_hi ← 4
    for range k_lo‥k_hi let k {{
        {body}
    }}
    {tail}
}}

main {{
    mut tensor<f32, [4]> src ← tf32[4] {{}}
    mut tensor<f32, [8]> dst ← tf32[8] {{}}
    poke(src, 0, 0, mut dst)
}}
"#
    )
}

// ── the checker's proofs (need, slice 4) ──────────────────────────

#[test]
fn a_range_loop_coordinate_the_checker_proved_needs_no_fault_site() {
    // `k` runs over `0‥4` and both tensors have at least that many elements on
    // the axis it indexes (`[4]`, `[8]`): the checker proves `0 <= k < extent`
    // for each access, and the lane analysis reads that proof.
    let (signature, reflection) = kernel(&single_lane("dst[k] ← src[k] * 2.0", "dst[0] ← dst[0] + 0.5"));
    assert!(!has_fault_channel(&signature), "{:?}", kinds(&signature));
    assert!(reflection.fault_sites.is_empty());
}

#[test]
fn a_shifted_range_loop_coordinate_keeps_its_fault_site() {
    // `k + read` has no fact (the c4-index-fault shape): only the load stays
    // checked; the proven store adds no site.
    let (signature, reflection) = kernel(&single_lane("dst[k] ← src[k + read]", "dst[0] ← dst[0] + 0.5"));
    let kinds = kinds(&signature);
    assert_eq!(
        kinds.last().map(|(_, kind)| *kind),
        Some(Kind::FaultRecord { sites: 1 }),
        "{kinds:?}"
    );
    assert_eq!(reflection.fault_sites.len(), 1);
}

#[test]
fn a_range_loop_that_overruns_the_extent_keeps_its_fault_site() {
    // `k` reaches 4 and `src` has 4 elements: the proof must not hold.
    let source =
        single_lane("dst[k] ← src[k] * 2.0", "dst[0] ← dst[0] + 0.5").replace("let i32 k_hi ← 4", "let i32 k_hi ← 5");
    let (signature, reflection) = kernel(&source);
    let kinds = kinds(&signature);
    assert_eq!(
        kinds.last().map(|(_, kind)| *kind),
        Some(Kind::FaultRecord { sites: 1 }),
        "{kinds:?}"
    );
    assert_eq!(reflection.fault_sites.len(), 1);
}

#[test]
fn a_single_lane_kernel_with_unproven_indexes_carries_a_fault_channel() {
    let (signature, reflection) = kernel(&single_lane("dst[k + write] ← src[k + read] * 2.0", "dst[0] ← dst[0] + 0.5"));
    let kinds = kinds(&signature);
    let counts: Vec<Kind> = kinds
        .iter()
        .map(|(_, kind)| *kind)
        .filter(|kind| matches!(kind, Kind::TensorElementCount { .. }))
        .collect();
    // `src` is binding 0 and `dst` binding 3: one count each, then the record
    // with a load site (src) and a store site (dst). The literal `dst[0]` adds
    // no site.
    assert_eq!(
        counts,
        [
            Kind::TensorElementCount { tensor_binding: 0 },
            Kind::TensorElementCount { tensor_binding: 3 }
        ],
        "{kinds:?}"
    );
    assert_eq!(
        kinds.last().map(|(_, kind)| *kind),
        Some(Kind::FaultRecord { sites: 2 }),
        "{kinds:?}"
    );
    let sites: Vec<_> = reflection
        .fault_sites
        .iter()
        .map(|site| (site.id, site.kind, site.text.as_str()))
        .collect();
    assert_eq!(
        sites,
        [
            (0, MirFaultKind::Fatal, INDEX_FAULT_TEXT),
            (1, MirFaultKind::Fatal, INDEX_FAULT_TEXT)
        ]
    );
}

#[test]
fn a_single_lane_kernel_with_only_literal_indexes_carries_no_fault_channel() {
    let (signature, reflection) = kernel(&single_lane("dst[1] ← src[3] * 2.0", "dst[0] ← dst[7] + 0.5"));
    assert!(!has_fault_channel(&signature), "{:?}", kinds(&signature));
    assert!(reflection.fault_sites.is_empty());
}

#[test]
fn a_sequential_tensor_iteration_proves_its_coordinate_binder() {
    // `for from src at [k]` with no `thread lane` is a counter loop over
    // `src`'s extent: `src[k]` and the same-shaped `dst[k]` are proven, as the
    // checker proves them; `src[k + 1]` is not.
    let source = |body: &str| {
        format!(
            r#"
@ kernel
@ public
fn scale(tensor<f32, [8]> src, mut tensor<f32, [8]> dst) → void {{
    for from src at [k] let _ {{
        {body}
    }}
}}

main {{
    mut tensor<f32, [8]> src ← tf32[8] {{}}
    mut tensor<f32, [8]> dst ← tf32[8] {{}}
    scale(src, mut dst)
}}
"#
        )
    };
    let (signature, _) = kernel(&source("dst[k] ← src[k] * 2.0"));
    assert!(!has_fault_channel(&signature), "{:?}", kinds(&signature));
    let (signature, reflection) = kernel(&source("dst[k] ← src[k + 1] * 2.0"));
    assert_eq!(
        kinds(&signature).last().map(|(_, kind)| *kind),
        Some(Kind::FaultRecord { sites: 1 }),
        "{:?}",
        kinds(&signature)
    );
    assert_eq!(reflection.fault_sites.len(), 1);
}

#[cfg(feature = "mir-metal")]
#[test]
fn a_single_lane_kernel_guards_its_unproven_indexes_in_metal_text() {
    let source = format!(
        "+++\nlocale = \"en\"\n+++\n{}",
        single_lane("dst[k + write] ← src[k + read] * 2.0", "dst[0] ← dst[0] + 0.5")
    );
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    assert!(text.contains("device atomic_uint* u9_fault"), "{text}");
    // The load guard (site 0, against `src`'s count) precedes the `src`
    // read, the store guard (site 1, against `dst`'s count) the `dst` write.
    let line = |needle: &str| {
        text.lines()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("no line with {needle}: {text}"))
    };
    // Each check is a deferred compare line whose order key carries its slot.
    assert_eq!(deferred_slots(&text, 2), [0, 1, 0, 1], "{text}");
    assert!(line("b0_count[0]);") < line("b0[min((uint)"), "{text}");
    assert!(line("b3_count[0]);") < line("b3[t"), "{text}");
    // The lane leaves, writing the first failed slot, before its first store.
    let store = line("b3[t");
    assert!(
        text.lines()
            .nth(store - 1)
            .is_some_and(|line| line.contains("&u9_fault[u9_first & 3u]")),
        "{text}"
    );
    // The literal `dst[0]` store is proven: its line carries no compare.
    let literal = text
        .lines()
        .find(|line| line.trim_start().starts_with("b3[0] ="))
        .expect("the literal store");
    assert!(!literal.contains("u9_fault"), "{literal}");
}

#[cfg(feature = "mir-metal")]
#[test]
fn a_runtime_vector_index_guards_its_width_and_records_the_fault_in_metal_text() {
    let source = r#"
@ kernel
@ public
fn k<size H, size W>(tensor<vector<f32, size 4>, [H, W]> state, i32 pick, mut tensor<f32, [H, W]> depth) → void {
    for from state at [i, j] thread lane let c {
        depth[[i, j]] ← c[pick]
    }
}

main {
    mut tensor<vector<f32, size 4>, [2, 2]> state ← tensor<vector<f32, size 4>, [2, 2]> {}
    mut tensor<f32, [2, 2]> depth ← tf32[2, 2] {}
    k(state, 1, mut depth)
}
"#;
    let source = format!("+++\nlocale = \"en\"\n+++\n{source}");
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    assert!(text.contains("device atomic_uint* u9_fault"), "{text}");
    assert_eq!(deferred_slots(&text, 1), [0], "the vector fault uses site 0: {text}");
    assert!(text.contains("(ulong)(4u)"), "the guard uses the vector width: {text}");
}

#[cfg(feature = "mir-metal")]
#[test]
fn scalar_metal_extrema_propagate_nan_and_order_signed_zero() {
    let source = r#"
@ kernel
@ public
fn extrema(f32 left, f32 right, mut tensor<f32, [2]> output) → void {
    output[0] ← left.minimum(right)
    output[1] ← left.maximum(right)
}

main {
    mut tensor<f32, [2]> output ← tf32[2] {}
    extrema(0.0, 1.0, mut output)
}
"#;
    let source = format!("+++\nlocale = \"en\"\n+++\n{source}");
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    assert_eq!(text.matches("isnan(").count(), 4, "{text}");
    assert!(text.contains("as_type<uint>"), "signed zero is ordered explicitly: {text}");
    assert!(!text.contains("fmin("), "MSL fmin does not define the language NaN law: {text}");
    assert!(!text.contains("fmax("), "MSL fmax does not define the language NaN law: {text}");
}

#[cfg(feature = "mir-metal")]
#[test]
fn a_run_time_vector_store_guards_its_width_in_metal_text() {
    let source = r#"
@ kernel
@ public
fn poke(i32 pick, mut tensor<f32, [1]> dst) → void {
    mut vector<f32, size 4> value ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, size 4>
    value[pick] ← 9.0
    dst[0] ← value[0] + value[1] + value[2] + value[3]
}

main {
    mut tensor<f32, [1]> dst ← tf32[1] {}
    poke(3, mut dst)
}
"#;
    let source = format!("+++\nlocale = \"en\"\n+++\n{source}");
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    assert!(text.contains("device atomic_uint* u9_fault"), "{text}");
    assert_eq!(deferred_slots(&text, 1), [0], "the vector store uses site 0: {text}");
    assert!(text.contains("(ulong)(4u)"), "the store guard uses the vector width: {text}");
}

/// A lane kernel reading one element of a rank-1 parameter at a literal, or at
/// a module constant, or at a constant expression: each folds to its value
/// and is proven like the literal it is, so the kernel carries no fault
/// channel.
fn constant_index(index: &str) -> String {
    format!(
        r#"
const i32 K = 2

@ kernel
@ public
fn k<size H, size W>(tensor<f32, [H, W]> src, tensor<f32, [24]> clock, mut tensor<f32, [H, W]> dst) → void {{
    for from src at [i, j] thread lane let x {{
        dst[[i, j]] ← x + clock[{index}]
    }}
}}

main {{
    mut tensor<f32, [2, 2]> src ← tf32[2, 2] {{}}
    mut tensor<f32, [24]> clock ← tf32[24] {{}}
    mut tensor<f32, [2, 2]> dst ← tf32[2, 2] {{}}
    k(src, clock, mut dst)
}}
"#
    )
}

#[test]
fn a_literal_or_constant_index_carries_no_fault_channel() {
    for index in ["2", "K", "K + 1", "[K]"] {
        let (signature, reflection) = kernel(&constant_index(index));
        assert!(!has_fault_channel(&signature), "clock[{index}]: {:?}", kinds(&signature));
        assert!(reflection.fault_sites.is_empty(), "clock[{index}]");
    }
    // The control: a lane coordinate is not proven against `clock`'s extent.
    let (signature, _) = kernel(&constant_index("j"));
    assert!(has_fault_channel(&signature), "{:?}", kinds(&signature));
}

/// A two-dimensional stencil body in a lane kernel over `[H, W]` tensors.
#[cfg(feature = "mir-metal")]
fn stencil(body: &str) -> String {
    format!(
        r#"+++
locale = "en"
+++
@ kernel
@ public
fn k<size H, size W>(tensor<f32, [H, W]> src, mut tensor<f32, [H, W]> dst) → void {{
    for from src at [i, j] thread lane let x {{
{body}
    }}
}}

main {{
    mut tensor<f32, [2, 2]> src ← tf32[2, 2] {{}}
    mut tensor<f32, [2, 2]> dst ← tf32[2, 2] {{}}
    k(src, mut dst)
}}
"#
    )
}

/// The deferred bounds checks of the emitted text: one
/// `const bool u9_gN = <compares>;` line per access.
#[cfg(feature = "mir-metal")]
fn guard_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("const bool u9_g") && line.contains("(ulong)("))
        .collect()
}

/// The record slot of each deferred check of the text, in order. A check folds
/// its order key (`ordinal << bits | slot`) into `u9_first`, where `bits` is
/// the field width for `sites` record slots.
#[cfg(feature = "mir-metal")]
fn deferred_slots(text: &str, sites: usize) -> Vec<usize> {
    let bits = (usize::BITS - sites.leading_zeros()).max(1);
    text.lines()
        .filter_map(|line| {
            let line = line
                .trim_start()
                .strip_prefix("u9_first = min(u9_first, u9_g")?;
            let key: usize = line.split("? ").nth(1)?.split('u').next()?.parse().ok()?;
            Some(key & ((1 << bits) - 1))
        })
        .collect()
}

/// Every bounds compare of the text (`(ulong)(a) >= (ulong)(b)`).
#[cfg(feature = "mir-metal")]
fn compares(text: &str) -> usize {
    guard_lines(text)
        .iter()
        .map(|line| line.matches(">= (ulong)(").count())
        .sum()
}

#[cfg(feature = "mir-metal")]
#[test]
fn a_repeated_neighbour_read_is_checked_once_with_one_check_per_access() {
    // Three distinct unproven accesses (east: the column axis, south: the
    // flat compare for the row axis, diagonal: both); each read twice more.
    let source = stencil(
        "        let f32 east ← src[[i, j + 1]]\n\
         \x20       let f32 south ← src[[i + 1, j]]\n\
         \x20       let f32 diag ← src[[i + 1, j + 1]]\n\
         \x20       let f32 east2 ← src[[i, j + 1]]\n\
         \x20       let f32 south2 ← src[[i + 1, j]]\n\
         \x20       let f32 diag2 ← src[[i + 1, j + 1]]\n\
         \x20       dst[[i, j]] ← x + east + south + diag + east2 + south2 + diag2",
    );
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    let guards = guard_lines(&text);
    assert_eq!(guards.len(), 3, "one check per distinct access: {text}");
    // east: column axis; south: flat; diagonal: column axis (seen) + new flat.
    assert_eq!(compares(&text), 3, "{text}");
    // The extent is read from its buffer once per lane, not once per access.
    assert_eq!(text.matches("b0_extent1[0]").count(), 1, "{text}");
    assert!(text.contains("const auto b0_extent1_v = b0_extent1[0];"), "{text}");
}

#[cfg(feature = "mir-metal")]
#[test]
fn one_access_needing_two_compares_writes_one_check() {
    let source = stencil(
        "        let f32 diag ← src[[i + 1, j + 1]]\n\
         \x20       dst[[i, j]] ← x + diag",
    );
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    let guards = guard_lines(&text);
    assert_eq!(guards.len(), 1, "{text}");
    assert_eq!(compares(&text), 2, "a column compare and the flat compare: {text}");
    assert!(guards[0].contains(" || "), "{text}");
}

#[cfg(feature = "mir-metal")]
#[test]
fn a_check_inside_a_branch_does_not_cover_the_code_after_it() {
    // The read in the arm is guarded there; the read after the `if` runs on
    // lanes that skipped the arm, so it is guarded again.
    let source = stencil(
        "        mut f32 acc ← x\n\
         \x20       if i ≡ 0 {\n\
         \x20           acc ← acc + src[[i, j + 1]]\n\
         \x20       }\n\
         \x20       dst[[i, j]] ← acc + src[[i, j + 1]]",
    );
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    assert_eq!(guard_lines(&text).len(), 2, "{text}");
}

#[cfg(feature = "mir-metal")]
#[test]
fn a_check_before_a_branch_covers_the_read_inside_it() {
    let source = stencil(
        "        mut f32 acc ← x + src[[i, j + 1]]\n\
         \x20       if i ≡ 0 {\n\
         \x20           acc ← acc + src[[i, j + 1]]\n\
         \x20       }\n\
         \x20       dst[[i, j]] ← acc",
    );
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    assert_eq!(guard_lines(&text).len(), 1, "{text}");
}

#[cfg(feature = "mir-metal")]
#[test]
fn a_reassigned_coordinate_is_checked_again() {
    // `k` names a different column at the second read: the value changed.
    let source = stencil(
        "        mut i32 k ← j + 1\n\
         \x20       let f32 first ← src[[i, k]]\n\
         \x20       k ← j + 2\n\
         \x20       dst[[i, j]] ← x + first + src[[i, k]]",
    );
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    assert_eq!(guard_lines(&text).len(), 2, "{text}");
}

#[cfg(feature = "mir-metal")]
#[test]
fn a_coordinate_assigned_in_one_arm_is_checked_again_after_the_join() {
    let source = stencil(
        "        mut i32 k ← j + 1\n\
         \x20       let f32 first ← src[[i, k]]\n\
         \x20       if i ≡ 0 {\n\
         \x20           k ← j + 2\n\
         \x20       }\n\
         \x20       dst[[i, j]] ← x + first + src[[i, k]]",
    );
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    assert_eq!(guard_lines(&text).len(), 2, "{text}");
}

#[cfg(feature = "mir-metal")]
#[test]
fn a_coordinate_a_loop_advances_is_checked_inside_the_loop() {
    let source = stencil(
        "        mut i32 k ← j\n\
         \x20       mut f32 acc ← src[[i, k + 1]]\n\
         \x20       while k ≺ 3 {\n\
         \x20           acc ← acc + src[[i, k + 1]]\n\
         \x20           k ← k + 1\n\
         \x20       }\n\
         \x20       dst[[i, j]] ← acc",
    );
    let text = super::test_support::compile_metal_text(&source).unwrap_or_else(|error| panic!("metal text: {error}"));
    assert_eq!(guard_lines(&text).len(), 2, "{text}");
}
