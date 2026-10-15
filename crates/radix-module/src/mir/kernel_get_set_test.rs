//! Unhandled checked tensor `get` / `set` in a `⇥ text` kernel (gpu-reset U9-7).

use crate::codegen::Target;
use crate::driver::{AnalyzedModule, Config, Session};
use crate::mir::{MirLoweringOptions, lower_analyzed_unit};
use crate::program::analyze_prepared_program_name;
use radix_mir::{MirFaultKind, MirKernelReflection, MirKernelSignature, MirTerminatorKind};

fn analyze(source: &str) -> AnalyzedModule {
    analyze_prepared_program_name(
        &Session::new(Config::default().with_target(Target::HirTypeScript)),
        "kernel-get-set.fab",
        &format!("+++\nlocale = \"en\"\n+++\n{source}"),
    )
    .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"))
}

const KERNEL: &str = r#"
@ kernel
@ public
fn k<size H, size W>(tensor<f32, [H, W]> src, mut tensor<f32, [H, W]> dst) → void ⇥ string {
    for from src at [i, j] thread lane const v {
        dst.set([i, j], src.get([i, j + 1]))
    }
}

@ cli "kernel-get-set"
@ operand int rows
@ operand int cols
main args args {
    const size R ← args.rows
    const size C ← args.cols
    mut tensor<f32, [R, C]> src ← tf32[R, C] {}
    mut tensor<f32, [R, C]> dst ← tf32[R, C] {}
    do all {
        k(src, mut dst)
    }
    catch err {
        print err
    }
}
"#;

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
    .unwrap_or_else(|error| panic!("the kernel has an ABI signature: {error:?}"));
    let mut reflection = MirKernelReflection::from_signature("k", &signature, validation).expect("reflects");
    reflection.apply_fault_sites(&function, validation);
    (function, signature, reflection)
}

#[test]
fn unhandled_get_and_set_lower_to_compare_and_recoverable_sites_without_a_try_call() {
    let (function, _, reflection) = lowered_kernel(KERNEL);
    assert!(
        !function
            .blocks
            .iter()
            .any(|block| matches!(block.terminator.kind, MirTerminatorKind::TryCall { .. })),
        "a checked access in a kernel is never a try-call"
    );
    let sites: Vec<_> = reflection
        .fault_sites
        .iter()
        .map(|site| (site.kind, site.text.as_str()))
        .collect();
    let recoverable: Vec<_> = sites
        .iter()
        .filter(|(kind, _)| *kind == MirFaultKind::Recoverable)
        .map(|(_, text)| *text)
        .collect();
    // Block order: the `get` (evaluated first) then the `set`.
    assert_eq!(recoverable, ["tensor accipe invalid index", "tensor ponde invalid index"]);
    // Every Fatal index site precedes the Recoverable ones.
    let first_recoverable = sites
        .iter()
        .position(|(kind, _)| *kind == MirFaultKind::Recoverable);
    assert!(
        sites[first_recoverable.expect("a recoverable site")..]
            .iter()
            .all(|(kind, _)| *kind == MirFaultKind::Recoverable),
        "{sites:?}"
    );
}
