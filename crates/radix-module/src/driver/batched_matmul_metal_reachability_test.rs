//! S05 family-3 Metal reachability probe: the batched attention contraction
//! (`q · kt`) from source to Metal text.
//!
//! `TensorBatchedAttention` has no source spelling by design — it is internal
//! device vocabulary. The source-reachable form of the same contraction is
//! the rank-3/4 `·` product that the Metal planner extends into the
//! batch-extended `TiledMatMul` (`crates/radix-mir-metal/src/emit/matmul.rs`,
//! `emit_batched_tiled_matmul_body`). These probes close the source →
//! device-emitter question through the driver's own `compile` route
//! (`Target::MirMetal`, `crate::Output::MetalText`).
//!
//! - Static half: the GQA decode shape (`[5, 3, 1, 64] · [5, 64, 76]`) is
//!   admitted and the emitted MSL carries the batch/share decomposition
//!   (`id.y` folds the batch into one padded `ceil(M/tile)` row-block per
//!   batch; the shorter right batch prefix maps through one division by the
//!   share width).
//! - Generic half: the same contraction over run-time extents. The observed
//!   first failing seam is not the Metal emitter's run-time-extent refusal the
//!   code read predicted: a size-generic `@ kernel` body with no same-unit
//!   caller fails the DFV2-2 orphan admission at check stage (`ADM001`, issue
//!   `nucleum_orphaned_annotation`) before any plan is formed, because the
//!   single-file driver route threads no package export admission (`@ public`
//!   alone is not the publica-tier package fact). The test pins that observed
//!   identity and never message text.
//!
//! ABI note: a body carrying a recipe op (matmul, reduction, softmax, …) is
//! not a single-lane kernel, so `mut` output plus `⇇` is admitted only with an
//! explicit trailing `u32` index parameter (the same ABI the family-1
//! `rope_half` probe uses). Without it the positional fallback reads the
//! output tensor as the index parameter and fails the ABI with
//! `kernel index parameter must be u32`. The return-buffer ABI is
//! elementwise-only (`rope_half_metal_reachability_test.rs` pins that
//! refusal for family 1).

use super::compile;
use crate::Output;
use crate::codegen::Target;
use crate::diagnostics::{Diagnostic, DiagnosticPhase};
use crate::driver::{Config, ModuleCompileRequest, Session};

/// The static source: GQA decode `[5, 3, 1, 64] · [5, 64, 76] → [5, 3, 1, 76]`
/// (left count 960 → batch 15, right count 24320 → right_batch 5, share 3).
const BATCHED_MATMUL_STATIC: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn bmm(tensor<f32, [5, 3, 1, 64]> q, tensor<f32, [5, 64, 76]> kt, mut tensor<f32, [5, 3, 1, 76]> out, u32 i) → void {
    out ⇇ q · kt
}
"#;

/// The same contraction with generic extents.
const BATCHED_MATMUL_GENERIC: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn bmm<size H, size M, size K, size N>(tensor<f32, [H, M, K]> q, tensor<f32, [H, K, N]> kt, mut tensor<f32, [H, M, N]> out, u32 i) → void {
    out ⇇ q · kt
}
"#;

fn metal_session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::MirMetal)
            .with_dev_locale_root(),
    )
}

fn arg_value<'a>(diagnostic: &'a Diagnostic, name: &str) -> Option<&'a str> {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

/// S05 family-3 static half: the GQA decode contraction reaches the Metal
/// emitter and the body is the batched tile recipe — `id.y` decomposes into
/// `(batch, row)` over one padded row-block per batch, and the right
/// operand's shorter batch prefix comes from one division by the GQA share.
#[test]
fn batched_matmul_metal_static_emits_batched_tile_body() {
    let session = metal_session();
    let result = compile(
        &session,
        "batched-matmul-static.fab",
        BATCHED_MATMUL_STATIC,
        ModuleCompileRequest::default(),
    );
    assert!(
        result.success(),
        "the rank-4 · rank-3 decode contraction must reach the Metal emitter: {:?}",
        result.diagnostics
    );
    let Some(Output::MetalText(output)) = result.output else {
        panic!("expected Metal text output");
    };
    let msl = output.code;
    // M = 1 pads to one tile row, so the per-batch row-block is 8 rows and
    // `id.y` is `batch · 8 + row`.
    assert!(
        msl.contains("uint b = id.y / 8u;"),
        "the batch index must decompose id.y over the padded row-block:\n{msl}"
    );
    // batch 15 over right_batch 5 is the GQA share 3.
    assert!(
        msl.contains("uint b_r = b / 3u;"),
        "the right-operand batch index must map through the GQA share:\n{msl}"
    );
    // The left operand's per-batch dense base: b · M · K with M = 1, K = 64.
    assert!(
        msl.contains("uint a_idx = (b * 1u * 64u)"),
        "the left operand must carry the per-batch base offset:\n{msl}"
    );
}

/// S05 family-3 generic half: run-time extents stop at the DFV2-2 orphan
/// admission (a size-generic kernel body with no same-unit caller), before
/// the Metal emitter's run-time-extent rule is ever consulted — divergence
/// from the predicted `mir_metal_text_unsupported` recorded in the module doc.
/// Pinned by diagnostic identity only.
#[test]
fn batched_matmul_metal_generic_extents_seam() {
    let session = metal_session();
    let result = compile(
        &session,
        "batched-matmul-generic.fab",
        BATCHED_MATMUL_GENERIC,
        ModuleCompileRequest::default(),
    );
    let Some(error) = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error())
    else {
        panic!(
            "run-time extents unexpectedly compiled; if the batched body emitted, pin its runtime-extent markers instead. output={}",
            result.output.is_some()
        );
    };
    assert_eq!(
        error.code,
        Some("ADM001"),
        "the orphan admission is the first failing seam: {error:?}"
    );
    assert_eq!(
        arg_value(error, "issue"),
        Some("nucleum_orphaned_annotation"),
        "the refusal must be the orphan admission issue: {error:?}"
    );
    assert_eq!(
        error.phase,
        DiagnosticPhase::Analysis,
        "the orphan admission runs at check stage"
    );
}
