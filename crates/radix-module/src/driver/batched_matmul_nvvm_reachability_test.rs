//! S05 family-3 CUDA-column GAP PIN for the source-reachable batched
//! attention contraction (`q · kt`) from source to NVVM text.
//!
//! `TensorBatchedAttention` has no source spelling by design; these probes
//! exercise the rank-3/4 `·` source form through the driver's compile route
//! (`Target::MirLlvm`, `with_nvvm_device_module()`, `crate::Output::LlvmText`).
//! The static test is intentionally a GAP PIN, not desired behavior: the
//! predicted rank-2 batch-unaware body is not reached on this checkout. The
//! observed divergence is an NVVM codegen refusal of the rank-4 left operand
//! (`CODEGEN001`, `unsupported-mir-shape`, Codegen phase). The follow-up
//! packet owns a batch-aware NVVM body; this probe must not repair it.
//!
//! The generic probe pins the first failing seam by diagnostic identity. If
//! the generic compile begins succeeding, it instead checks the rank-2 body
//! markers and thereby records that the gap remains in emitted code.

use super::compile;
use crate::Output;
use crate::codegen::Target;
use crate::diagnostics::{Diagnostic, DiagnosticPhase};
use crate::driver::{Config, ModuleCompileRequest, Session};

/// GQA decode shape: `[5, 3, 1, 64] · [5, 64, 76] → [5, 3, 1, 76]`.
/// The source tensors have 15 and 5 batches respectively, with share 3.
const BATCHED_MATMUL_STATIC: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn bmm(tensor<f32, [5, 3, 1, 64]> q, tensor<f32, [5, 64, 76]> kt, mut tensor<f32, [5, 3, 1, 76]> out, u32 i) → void {
    out ⇇ q · kt
}
"#;

/// The same contraction over generic extents.
const BATCHED_MATMUL_GENERIC: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn bmm<size H, size M, size K, size N>(tensor<f32, [H, M, K]> q, tensor<f32, [H, K, N]> kt, mut tensor<f32, [H, M, N]> out, u32 i) → void {
    out ⇇ q · kt
}
"#;

fn nvvm_session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::MirLlvm)
            .with_dev_locale_root()
            .with_nvvm_device_module(),
    )
}

fn diagnostic_arg<'a>(diagnostic: &'a Diagnostic, name: &str) -> Option<&'a str> {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn assert_rank2_batch_unaware_body(code: &str) {
    assert!(
        code.contains("%faber.mm.a.rowok = icmp ult i32 %faber.index.y, 1"),
        "the rank-2 recipe must guard using M=1, not batch·M:\n{code}"
    );
    assert!(
        code.contains("%faber.mm.b.idx = add i32 %faber.mm.b.ridx, %faber.index"),
        "the rank-2 recipe must have no per-batch B base:\n{code}"
    );
    assert!(
        !code.contains("%faber.mm.batch") && !code.contains("b_r"),
        "the current NVVM body must not contain batch addressing:\n{code}"
    );
}

/// GAP PIN (not desired behavior): the code-read prediction was that the
/// static batched source contraction would emit the rank-2 recipe body. On
/// this checkout the NVVM path refuses the rank-4 left operand before body
/// emission; the follow-up packet is responsible for a batch-aware body.
#[test]
fn batched_matmul_nvvm_static_body_is_batch_unaware() {
    let result = compile(
        &nvvm_session(),
        "batched-matmul-static.fab",
        BATCHED_MATMUL_STATIC,
        ModuleCompileRequest::default(),
    );
    let Some(error) = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error())
    else {
        assert!(
            result.success(),
            "static compile had no error but was not successful: {:?}",
            result.diagnostics
        );
        let Some(Output::LlvmText(output)) = result.output else {
            panic!("static compile unexpectedly succeeded without LlvmText output");
        };
        assert_rank2_batch_unaware_body(&output.code);
        return;
    };
    assert_eq!(
        error.code,
        Some("CODEGEN001"),
        "the observed static refusal must stay pinned by code: {error:?}"
    );
    assert_eq!(
        diagnostic_arg(error, "category"),
        Some("unsupported-mir-shape"),
        "the refusal's structured category must stay pinned: {error:?}"
    );
    assert_eq!(
        diagnostic_arg(error, "shape"),
        Some("matmul requires a rank-2 left tensor and rank-1 or rank-2 right tensor"),
        "the rank refusal is the observed seam: {error:?}"
    );
    assert_eq!(error.phase, DiagnosticPhase::Codegen);
}

/// Pin the first generic-extents seam by structured diagnostic facts; the
/// static-body branch handles and records an unexpected successful compile.
#[test]
fn batched_matmul_nvvm_generic_extents_seam() {
    let result = compile(
        &nvvm_session(),
        "batched-matmul-generic.fab",
        BATCHED_MATMUL_GENERIC,
        ModuleCompileRequest::default(),
    );
    let Some(error) = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error())
    else {
        assert!(
            result.success(),
            "generic compile had no error but was not successful: {:?}",
            result.diagnostics
        );
        let Some(Output::LlvmText(output)) = result.output else {
            panic!("generic compile unexpectedly succeeded without LlvmText output");
        };
        assert_rank2_batch_unaware_body(&output.code);
        return;
    };
    assert_eq!(error.code, Some("ADM001"), "pin the observed first failing seam: {error:?}");
    assert_eq!(
        diagnostic_arg(error, "issue"),
        Some("nucleum_orphaned_annotation"),
        "generic kernel currently stops at orphan admission: {error:?}"
    );
    assert_eq!(
        error.phase,
        DiagnosticPhase::Analysis,
        "the observed orphan admission runs during analysis"
    );
}
