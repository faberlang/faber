//! S05 family-1 reachability probe: `rope_half` from source to Metal text.
//!
//! Need (GPU research S05) landed the registry row (`rope_half` →
//! `MirCollectionOp::TensorRopeHalf`) and the source → MIR → CPU-runner proof.
//! This probe closes the missing device-path question: where does
//! `x.rope_half<size 4>(pos, table)` stop on the Metal lane?
//!
//! The answer proven here: the spelling reaches the device emitter and the
//! first failing seam is a typed fact, not the spelling.
//! - Green half: with a literal `pos` the plan resolves and the emitted MSL
//!   carries the Qwen/NeoX `rotate_half` half-split body
//!   (`(x[k], x[k + dim/2])`), never the llama consecutive-pair body. The
//!   source needs the indexed-view ABI (a trailing `u32` index parameter and
//!   a `mut` output) because the return-buffer ABI is elementwise-only and
//!   the rank-3 angle table has a different element count than the rotated
//!   row.
//! - Refusal half: a run-time `pos` cannot reach the device plan. The
//!   intrinsic's position parameter is the native `numerus` width (`i64`).
//!   Until the operator ruling of 2026-10-28 that was a check-stage refusal
//!   (`nucleum_wide_integer`); 64-bit integers are now kernel values, so the
//!   body checks and the Metal emitter refuses by name at plan resolution.
//!
//! The research need predicted the refusal at the device plan
//! (`rope_plan_facts` / `const_non_negative`). This file pins what the
//! compiler actually does. The return-buffer spelling from that need is
//! pinned as its own refusal so the next attempt does not rediscover the ABI
//! seam.

use super::compile;
use crate::Output;
use crate::codegen::Target;
use crate::diagnostics::{Diagnostic, DiagnosticPhase};
use crate::driver::{Config, ModuleCompileRequest, Session};

/// The reachable source shape: `rope_half<size 4>` with a literal position over the
/// pinned `[1, 2, 3]` angle table, written through the indexed-view ABI.
const ROPE_HALF_LITERAL_POS: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn ropam(tensor<f32, [4]> x, tensor<f32, [1, 2, 3]> table, mut tensor<f32, [4]> out, u32 i) → void {
    out ⇇ x.rope_half<size 4>(0, table)
}
"#;

/// The research need's return-buffer spelling: two inputs, tensor return.
const ROPE_HALF_RETURN_BUFFER: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn ropam(tensor<f32, [4]> x, tensor<f32, [1, 2, 3]> table) → tensor<f32, [4]> {
    return x.rope_half<size 4>(0, table)
}
"#;

/// The indexed-view fn with the rotation position supplied at run time.
const ROPE_HALF_RUNTIME_POS: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn ropam(tensor<f32, [4]> x, u32 pos, tensor<f32, [1, 2, 3]> table, mut tensor<f32, [4]> out, u32 i) → void {
    out ⇇ x.rope_half<size 4>(pos, table)
}
"#;

fn metal_session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::MirMetal)
            .with_dev_locale_root(),
    )
}

fn first_error(result: &crate::CompileResult) -> &Diagnostic {
    result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error())
        .expect("refusal must carry an error diagnostic")
}

fn arg_value<'a>(diagnostic: &'a Diagnostic, name: &str) -> Option<&'a str> {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

/// S05 family-1, green half: `rope_half<size 4>` with a literal position reaches
/// Metal text and the body is the half-split rotate, distinct from the
/// consecutive-pair `rope_norm` body.
#[test]
fn rope_half_metal_literal_pos_emits_rotate_half() {
    let session = metal_session();
    let result = compile(
        &session,
        "rope-half-literal.fab",
        ROPE_HALF_LITERAL_POS,
        ModuleCompileRequest::default(),
    );
    assert!(
        result.success(),
        "rope_half with a literal position must reach the Metal emitter: {:?}",
        result.diagnostics
    );
    let Some(Output::MetalText(output)) = result.output else {
        panic!("expected Metal text output");
    };
    let msl = output.code;
    // The half-split pair stride and the two `rotate_half` formulas. The
    // consecutive-pair body would pair `col / 2` with `col ± 1` instead.
    assert!(
        msl.contains("uint half_dim = 2u;") && msl.contains("uint pair = col % half_dim;"),
        "rotate_half must pair col with col±half_dim:\n{msl}"
    );
    assert!(
        msl.contains("output[o] = (b0_in[row * 4u + col] * cos_t) - (b0_in[row * 4u + (col + half_dim)] * sin_t);"),
        "the first-half rotate_half formula is missing:\n{msl}"
    );
    assert!(
        msl.contains("output[o] = (b0_in[row * 4u + (col - half_dim)] * sin_t) + (b0_in[row * 4u + col] * cos_t);"),
        "the second-half rotate_half formula is missing:\n{msl}"
    );
    assert!(
        !msl.contains("uint pair = col / 2u;"),
        "rotate_half must not emit llama consecutive-pair indexing:\n{msl}"
    );
}

/// S05 family-1 seam: the research need's return-buffer spelling is refused
/// one layer before the device plan — the return-buffer ABI is elementwise
/// only, and the rank-3 table does not share the row's element count.
#[test]
fn rope_half_metal_return_buffer_spelling_fails_at_the_abi() {
    let session = metal_session();
    let result = compile(
        &session,
        "rope-half-return.fab",
        ROPE_HALF_RETURN_BUFFER,
        ModuleCompileRequest::default(),
    );
    assert!(
        !result.success(),
        "the elementwise return-buffer ABI must refuse a table-carrying rope kernel"
    );
    let error = first_error(&result);
    assert_eq!(
        arg_value(error, "issue"),
        Some("mir_metal_text_unsupported"),
        "the refusal must be the Metal emit seam: {error:?}"
    );
    assert_eq!(error.phase, DiagnosticPhase::Codegen, "the refusal is at codegen");
}

/// S05 family-1 seam: a run-time position is refused at the Metal emit seam
/// by name. The intrinsic's position parameter is the native `numerus` width,
/// a kernel value since the 2026-10-28 ruling, so check stage no longer
/// refuses it.
#[test]
fn rope_half_metal_runtime_pos_fails_at_the_emit_seam() {
    let session = metal_session();
    let result = compile(
        &session,
        "rope-half-runtime.fab",
        ROPE_HALF_RUNTIME_POS,
        ModuleCompileRequest::default(),
    );
    assert!(!result.success(), "rope_half with a run-time position must fail closed");
    let error = first_error(&result);
    assert_eq!(
        arg_value(error, "issue"),
        Some("mir_metal_text_unsupported"),
        "the refusal must be the Metal emit seam: {error:?}"
    );
    assert_eq!(error.phase, DiagnosticPhase::Codegen, "the refusal is at codegen");
}
