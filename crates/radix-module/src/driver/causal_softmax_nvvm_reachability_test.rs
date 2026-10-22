//! S05 family-2 CUDA-column probe: `x.softmax_causal()` from source to NVVM
//! text.
//!
//! The source spelling and its arithmetic are pinned in
//! `causal_softmax_intrinsic_test.rs`; the Metal column is pinned in
//! `causal_softmax_metal_reachability_test.rs`. This file closes the CUDA
//! column through the driver's own `compile` route (`Target::MirLlvm`,
//! `with_nvvm_device_module()`, `crate::Output::LlvmText`):
//!
//! - Probe A `[1, 64]` (T=1 decode): rows <= 1 is refused by the row-reduction
//!   admission, so the emitted body is the generic per-element causal body
//!   (`radix-mir-llvm` `nvvm/causal_softmax.rs`): flat `outer = o / cols`
//!   addressing, the `%faber.cs.masked` extent, and the `%faber.cs.stable`
//!   row scan.
//! - Probe B `[8, 64]` (prefill): rows > 1, width 64 >= 32 and 64 % 4 == 0
//!   admits `causal_softmax_row_reduction_admission`, so the emitted body is
//!   the threadgroup row recipe (`radix-mir-llvm` `nvvm/recipe.rs`,
//!   `emit_causal_softmax_row_reduction_body`): the `%faber.cs2.*` symbols,
//!   and none of the generic `%faber.cs.*` body.
//! - Probe C `fn csm<size R, size C>`: the run-time-dimensions column. The
//!   observed first failing seam is not the plan gate's literal-dimension
//!   refusal the code read predicted: a size-generic `@ nucleum` body with no
//!   same-unit caller fails the DFV2-2 orphan admission at Analysis
//!   (`ADM001`, issue `nucleum_orphaned_annotation`) before any plan is
//!   formed, because the single-file driver route threads no package export
//!   admission (`@ public` alone is not the publica-tier package fact). The
//!   test pins that observed identity (code, `issue` arg, phase) and never
//!   message text.
//!
//! Locale note: the source is an `en` file, so the canonical
//! `softmax_causal` method is spelled `softmax_causal` on this surface (the
//! intrinsic test pins the same spelling).

use super::compile;
use crate::codegen::Target;
use crate::diagnostics::Diagnostic;
use crate::driver::{Config, ModuleCompileRequest, Session};

fn nvvm_session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::MirLlvm)
            .with_dev_locale_root()
            .with_nvvm_device_module(),
    )
}

/// The concrete-shape source of probes A and B: one `@ kernel` entry whose
/// whole body is the causal masked softmax of its rank-2 operand.
fn csm_source(rows: u64, cols: u64) -> String {
    format!(
        "+++
locale = \"en\"
+++

@ kernel
@ public
fn csm(tensor<f32, [{rows}, {cols}]> x) → tensor<f32, [{rows}, {cols}]> {{
    return x.softmax_causal()
}}
"
    )
}

/// Probe C's source: the same body over run-time extents.
fn csm_generic_source() -> String {
    "+++
locale = \"en\"
+++

@ kernel
@ public
fn csm<size R, size C>(tensor<f32, [R, C]> x) → tensor<f32, [R, C]> {
    return x.softmax_causal()
}
"
    .to_owned()
}

/// The `issue` args of a failed compile, so a divergence is reported by
/// diagnostic identity (never by message text).
fn issue_facts(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .flat_map(|diagnostic| diagnostic.args.iter())
        .filter(|arg| arg.name == "issue")
        .map(|arg| arg.value.clone())
        .collect()
}

/// Compile one concrete-shape probe to NVVM text, failing with diagnostic
/// identity facts.
fn compile_llvm(rows: u64, cols: u64) -> String {
    let source = csm_source(rows, cols);
    let result = compile(&nvvm_session(), "csm.fab", &source, ModuleCompileRequest::default());
    if !result.success() {
        panic!(
            "softmax_causal [{rows}, {cols}] should emit NVVM text; issues={:?}",
            issue_facts(&result.diagnostics)
        );
    }
    match result.output {
        Some(crate::Output::LlvmText(output)) => output.code,
        other => panic!("expected LlvmText output, got {}", other.is_some()),
    }
}

/// Probe A: the T=1 decode shape keeps the generic per-element body — the
/// kernel entry and the causal masked-row scan, with the column extent fixed
/// by the declared 64 columns.
#[test]
fn causal_softmax_nvvm_t1_emits_generic_causal_body() {
    let code = compile_llvm(1, 64);
    assert!(
        code.contains("@nucleum device-entry"),
        "device module has no kernel entry:\n{code}"
    );
    assert!(
        code.contains("!nvvm.annotations"),
        "device module has no NVVM annotations:\n{code}"
    );
    assert!(
        code.contains("%faber.cs.masked"),
        "generic causal body's masked extent did not emit:\n{code}"
    );
    assert!(
        code.contains("%faber.cs.stable"),
        "generic causal body's stable row scan did not emit:\n{code}"
    );
    // The mask is the recipe's own semantics, with the declared 64 columns
    // as the extent cap: masked = min(outer + 1, 64).
    assert!(
        code.contains("%faber.cs.masked = select i1 %faber.cs.mask.cmp, i32 %faber.cs.outer.one, i32 64"),
        "causal extent must be capped at the declared 64 columns:\n{code}"
    );
    // The row-reduction recipe is not selected for rows <= 1.
    assert!(
        !code.contains("%faber.cs2."),
        "T=1 must not emit the row-reduction body:\n{code}"
    );
}

/// Probe B: the admitted prefill shape emits the threadgroup row-reduction
/// recipe and none of the generic per-element body.
#[test]
fn causal_softmax_nvvm_prefill_emits_row_reduction_body() {
    let code = compile_llvm(8, 64);
    assert!(
        code.contains("%faber.cs2.masked"),
        "row-reduction body's masked extent did not emit:\n{code}"
    );
    assert!(
        code.contains("%faber.cs2.max"),
        "row-reduction body's row max did not emit:\n{code}"
    );
    assert!(
        !code.contains("%faber.cs.masked"),
        "the generic per-element body must not be selected for the admitted prefill shape:\n{code}"
    );
}

/// Probe C: run-time extents. The observed first failing seam is the DFV2-2
/// orphan admission (a size-generic kernel body with no same-unit caller),
/// not the plan gate's literal-dimension refusal — divergence recorded in the
/// module doc comment. Pinned by diagnostic identity only.
#[test]
fn causal_softmax_nvvm_generic_extents_seam() {
    let result = compile(
        &nvvm_session(),
        "csm_generic.fab",
        &csm_generic_source(),
        ModuleCompileRequest::default(),
    );
    let error = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error())
        .unwrap_or_else(|| {
            panic!(
                "run-time extents unexpectedly compiled; if the causal body emitted, pin its markers instead. output={}",
                result.output.is_some()
            )
        });
    assert_eq!(
        error.code,
        Some("ADM001"),
        "observed diagnostics {:?} issues={:?}",
        result.diagnostics,
        issue_facts(&result.diagnostics)
    );
    assert_eq!(error.phase, radix_diagnostics::DiagnosticPhase::Analysis);
    let facts = issue_facts(&result.diagnostics);
    assert!(
        facts
            .iter()
            .any(|fact| fact == "nucleum_orphaned_annotation"),
        "expected the orphan admission issue, got {facts:?}"
    );
}
