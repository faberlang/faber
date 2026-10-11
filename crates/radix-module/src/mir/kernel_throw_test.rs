//! Kernel throw sites (gpu-reset U9-6).

use crate::codegen::Target;
use crate::driver::{AnalyzedModule, Config, Session};
use crate::mir::{MirLoweringOptions, lower_analyzed_unit};
use crate::program::analyze_prepared_program_name;
use radix_mir::{
    MirFaultKind, MirKernelReflection, MirKernelResourceKind as Kind, MirKernelSignature, MirTerminatorKind,
};

fn analyze(source: &str) -> AnalyzedModule {
    analyze_prepared_program_name(
        &Session::new(Config::default().with_target(Target::HirTypeScript)),
        "kernel-throw.fab",
        &format!("+++\nlocale = \"en\"\n+++\n{source}"),
    )
    .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"))
}

const KERNEL: &str = r#"
@ kernel
@ public
fn k<size H, size W>(tensor<f32, [H, W]> src, mut tensor<f32, [H, W]> dst) → void ⇥ string {
    for from src at [i, j] thread lane const x {
        if x ≺ 0.0 {
            throw "negative lane"
        }
        dst[[i, j]] ← x * 2.0
    }
}

@ cli "kernel-throw"
@ operand int rows
@ operand int cols
main args args {
    const size R ← args.rows
    const size C ← args.cols
    var tensor<f32, [R, C]> src ← tf32[R, C] {}
    var tensor<f32, [R, C]> dst ← tf32[R, C] {}
    do all {
        k(src, mut dst)
    }
    catch err {
        print err
    }
}
"#;

/// The device-form kernel function, its signature and reflection.
fn lowered_kernel(source: &str) -> (radix_mir::MirFunction, MirKernelSignature, MirKernelReflection) {
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("device lowering validates");
    let function = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| function.shader_stage == Some(radix_mir::MirKernelShaderStage::Compute))
        .expect("the program has a compute kernel")
        .clone();
    let validation = lowered.validated.validation();
    let signature = MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
        &function,
        validation,
        &lowered.interner,
    )
    .expect("the throwing lane kernel has an ABI signature");
    let mut reflection = MirKernelReflection::from_signature("k", &signature, validation).expect("reflects");
    reflection.apply_fault_sites(&function, validation);
    (function, signature, reflection)
}

#[test]
fn a_constant_throw_lowers_without_a_try_call_and_reflects_a_recoverable_site() {
    let (function, signature, reflection) = lowered_kernel(KERNEL);
    assert!(function.error_ty.is_some(), "the kernel declares its error channel");
    let terminators: Vec<&MirTerminatorKind> = function
        .blocks
        .iter()
        .map(|block| &block.terminator.kind)
        .collect();
    assert!(
        !terminators
            .iter()
            .any(|kind| matches!(kind, MirTerminatorKind::TryCall { .. })),
        "a throw in a kernel is never a try-call"
    );
    assert_eq!(
        terminators
            .iter()
            .filter(|kind| matches!(kind, MirTerminatorKind::ReturnError(_)))
            .count(),
        1
    );
    let sites: Vec<_> = reflection
        .fault_sites
        .iter()
        .map(|site| (site.id, site.kind, site.text.as_str()))
        .collect();
    assert_eq!(sites, [(0, MirFaultKind::Recoverable, "negative lane")]);
    let kinds: Vec<Kind> = signature.resources_by_binding().map(|r| r.kind).collect();
    assert_eq!(kinds.last(), Some(&Kind::FaultRecord { sites: 1 }), "{kinds:?}");
    assert!(
        !kinds
            .iter()
            .any(|kind| matches!(kind, Kind::TensorElementCount { .. })),
        "a throw needs no element count: {kinds:?}"
    );
}

#[test]
fn throw_sites_follow_the_index_sites_in_block_order() {
    let source = KERNEL
        .replace(
            "tensor<f32, [H, W]> src, mut",
            "tensor<f32, [H, W]> src, tensor<f32, [H, W]> other, mut",
        )
        .replace(
            "dst[[i, j]] ← x * 2.0",
            "if x ≻ 100.0 {\n            throw \"too large\"\n        }\n        dst[[i, j]] ← x * other[[i, j + 1]]",
        )
        .replace("k(src, mut dst)", "k(src, other, mut dst)")
        .replace(
            "var tensor<f32, [R, C]> dst ←",
            "var tensor<f32, [R, C]> other ← tf32[R, C] {}\n    var tensor<f32, [R, C]> dst ←",
        );
    let (_, signature, reflection) = lowered_kernel(&source);
    let sites: Vec<_> = reflection
        .fault_sites
        .iter()
        .map(|site| (site.id, site.kind, site.text.as_str()))
        .collect();
    assert_eq!(
        sites,
        [
            (0, MirFaultKind::Fatal, radix_mir::INDEX_FAULT_TEXT),
            (1, MirFaultKind::Recoverable, "negative lane"),
            (2, MirFaultKind::Recoverable, "too large"),
        ]
    );
    let record = signature.resources_by_binding().last().map(|r| r.kind);
    assert_eq!(record, Some(Kind::FaultRecord { sites: 3 }));
}

#[test]
fn a_text_channel_kernel_that_never_throws_carries_no_fault_record() {
    let source = KERNEL.replace("if x ≺ 0.0 {\n            throw \"negative lane\"\n        }\n", "");
    let (_, signature, reflection) = lowered_kernel(&source);
    assert!(reflection.fault_sites.is_empty());
    assert!(
        signature
            .resources_by_binding()
            .all(|resource| !matches!(resource.kind, Kind::FaultRecord { .. }))
    );
}

/// A one-lane kernel (`scale`) that calls a throwing kernel (`guard`).
const INLINED: &str = r#"
@ kernel
fn guard(f32 v) → f32 ⇥ string {
    if v + 2.5 ≺ 0.0 {
        throw "very negative lane"
    }
    if v ≺ 0.0 {
        throw "negative lane"
    }
    return v * 2.0
}

@ kernel
@ public
fn scale(tensor<f32, [8]> src, mut tensor<f32, [8]> dst) → void ⇥ string {
    for range 0‥8 const k {
        dst[k] ← guard(src[k])
    }
}

@ cli "kernel-throw-inline"
main args args {
    var tensor<f32, [8]> src ← tf32[8] {}
    var tensor<f32, [8]> dst ← tf32[8] {}
    do all {
        scale(src, mut dst)
    }
    catch err {
        print err
    }
}
"#;

#[test]
fn an_inlined_throwing_kernel_adds_its_throws_as_caller_sites() {
    let (function, signature, reflection) = lowered_kernel(INLINED);
    assert!(
        !function
            .blocks
            .iter()
            .any(|block| matches!(block.terminator.kind, MirTerminatorKind::TryCall { .. })),
        "the failable call is composed away"
    );
    let sites: Vec<_> = reflection
        .fault_sites
        .iter()
        .map(|site| (site.id, site.kind, site.text.as_str()))
        .collect();
    assert_eq!(
        sites,
        [
            (0, MirFaultKind::Recoverable, "very negative lane"),
            (1, MirFaultKind::Recoverable, "negative lane"),
        ]
    );
    let record = signature.resources_by_binding().last().map(|r| r.kind);
    assert_eq!(record, Some(Kind::FaultRecord { sites: 2 }));
}

#[test]
fn the_callers_own_throw_precedes_the_appended_callee_blocks_in_block_order() {
    let source = INLINED.replace(
        "        dst[k] ← guard(src[k])\n",
        "        dst[k] ← guard(src[k])\n        if dst[k] ≻ 100.0 {\n            throw \"too large\"\n        }\n",
    );
    let (_, _, reflection) = lowered_kernel(&source);
    let texts: Vec<_> = reflection
        .fault_sites
        .iter()
        .map(|site| site.text.as_str())
        .collect();
    // The composed callee blocks are appended after the caller's own blocks.
    assert_eq!(texts, ["too large", "very negative lane", "negative lane"]);
}
