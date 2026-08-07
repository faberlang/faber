//! GI3-5 — the Q1-default prefill device-run driver tests.
//!
//! Families:
//! 1. **Declared f32 repack**: the per-head weight splits and the
//!    quantized-tensor f16 rounding (synthetic tensors — no model file).
//! 2. **MIR program assembly**: the constructed functions resolve the frozen
//!    recipe plans (Gather / RmsNormalization / Rope / CausalMaskedSoftmax /
//!    MatMul / elementwise) through the shared plan pass — the same typed-fact
//!    derivation the emitters cross-check.
//! 3. **Device program structure**: the assembled kernel graph has the
//!    expected kernel/launch counts, the logits observation is declared, and
//!    the wire section assembles for the Metal backend.
//! 4. **The gated burgus Metal device run** (env `FABER_PREFILL_DEVICE_RUN`):
//!    the full repack → build → execute → Q2 comparison path on real
//!    hardware. The runner is the gated consumer step (metal available on
//!    burgus).

use super::*;
use faber::device::{DeviceBackend, DeviceSelection};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// 1. Declared f32 repack
// ---------------------------------------------------------------------------

#[test]
fn split_columns_cuts_each_head_columns() {
    // A synthetic K-major [2, 3*4] weight (storage `data[i + 2*j]` — input
    // row `i`, output column `j`, the oracle's `dense` layout) → 3 heads of
    // [2, 4]: head `h` gets `slice[k*4 + n] = data[k + 2*(h*4 + n)]`.
    let source = (0..2 * 3 * 4)
        .map(|value| value as f32)
        .collect::<Vec<f32>>();
    let heads = split_columns(&source, 2, 3 * 4, 4);
    assert_eq!(heads.len(), 3);
    for h in 0..3 {
        assert_eq!(heads[h].len(), 2 * 4);
        for k in 0..2 {
            for n in 0..4 {
                assert_eq!(
                    heads[h][k * 4 + n],
                    source[k + 2 * (h * 4 + n)],
                    "head {h} input {k} head-dim {n}"
                );
            }
        }
    }
}

#[test]
fn split_rows_cuts_each_output_head_rows() {
    // A synthetic K-major [3*4, 6] weight → 3 heads of [4, 6]: head `h`
    // gets `slice[k*6 + d] = data[d*(3*4) + h*4 + k]`.
    let source = (0..3 * 4 * 6)
        .map(|value| value as f32)
        .collect::<Vec<f32>>();
    let heads = split_rows(&source, 3 * 4, 6, 4);
    assert_eq!(heads.len(), 3);
    for h in 0..3 {
        assert_eq!(heads[h].len(), 4 * 6);
        for k in 0..4 {
            for d in 0..6 {
                assert_eq!(
                    heads[h][k * 6 + d],
                    source[d * (3 * 4) + h * 4 + k],
                    "head {h} ctx-dim {k} output {d}"
                );
            }
        }
    }
}

#[test]
fn f16_round_trips_through_half_precision() {
    // Exact f16-representable values round-trip bit-exactly; the
    // non-representable value rounds to nearest.
    assert_eq!(f16_round(0.0), 0.0);
    assert_eq!(f16_round(1.0), 1.0);
    assert_eq!(f16_round(-0.5), -0.5);
    assert_eq!(f16_round(65504.0), 65504.0);
    let rounded = f16_round(0.1);
    assert!(
        (rounded - 0.0999755859375).abs() < 1e-9,
        "0.1 f16-rounds to {rounded}"
    );
}

// ---------------------------------------------------------------------------
// 2. MIR program assembly (the frozen recipe plans resolve)
// ---------------------------------------------------------------------------

/// A PrefillWeights with the pinned shapes but zero values — the MIR program
/// construction only consumes shapes; the declared-input values ride the
/// section.
fn zero_weights() -> PrefillWeights {
    PrefillWeights {
        token_embd: vec![0.0f32; (VOCAB_SIZE * HIDDEN_SIZE) as usize],
        layers: (0..LAYER_COUNT as usize)
            .map(|_| LayerRepack {
                attn_norm: vec![0.0; HIDDEN_SIZE as usize],
                attn_q_heads: (0..HEAD_COUNT as usize)
                    .map(|_| vec![0.0; (HIDDEN_SIZE * HEAD_DIM) as usize])
                    .collect(),
                attn_k_heads: (0..KV_HEAD_COUNT as usize)
                    .map(|_| vec![0.0; (HIDDEN_SIZE * HEAD_DIM) as usize])
                    .collect(),
                attn_v_heads: (0..KV_HEAD_COUNT as usize)
                    .map(|_| vec![0.0; (HIDDEN_SIZE * HEAD_DIM) as usize])
                    .collect(),
                attn_out_heads: (0..HEAD_COUNT as usize)
                    .map(|_| vec![0.0; (HEAD_DIM * HIDDEN_SIZE) as usize])
                    .collect(),
                ffn_norm: vec![0.0; HIDDEN_SIZE as usize],
                ffn_gate: vec![0.0; (HIDDEN_SIZE * FFN_SIZE) as usize],
                ffn_up: vec![0.0; (HIDDEN_SIZE * FFN_SIZE) as usize],
                ffn_down: vec![0.0; (FFN_SIZE * HIDDEN_SIZE) as usize],
            })
            .collect(),
        output_norm: vec![0.0; HIDDEN_SIZE as usize],
    }
}

#[test]
fn the_constructed_mir_resolves_the_frozen_recipe_plans() {
    let artifact = build_prefill_program().expect("prefill program assembles");
    let validation = MirValidationContext::new(&artifact.mir.types);
    let _validated = ValidatedMir::new(artifact.mir.functions.clone(), validation.clone())
        .expect("the constructed prefill MIR validates");

    // The carried plans are proven against the emitter by the section test
    // (`the_prefill_section_assembles_and_admits_for_metal` runs
    // `emit_metal_device_artifact`, which fails closed on any carried-plan
    // disagreement). Here we assert the structural contract: kernel/launch
    // counts (1 gather + 32×162 + 3 = 5188), the logits observation, and the
    // per-head attention fan-out entries (each query head's
    // scores/scale/softmax/context/output, folding the 15 head outputs).
    assert_eq!(
        artifact.program.kernels.len(),
        5188,
        "1 gather + 32×162 + 3 (output_norm, transpose token_embd, logits)"
    );
    assert_eq!(artifact.program.launches.len(), 5188);
    assert_eq!(artifact.program.results.len(), 1, "the logits observation");
    assert_eq!(artifact.program.results[0].buffer.name, "prefill.logits");
    assert_eq!(
        artifact.program.results[0].version.element_count,
        PROMPT_TOKEN_COUNT * VOCAB_SIZE,
        "the observed logits are the full [9, 49152]"
    );
    // The first layer's kernel entries follow the forward path order: the
    // per-head Q fan-out, the per-KV-head K/V + f16 rounding + transpose,
    // the per-query-head attention (scores → scale → softmax → context →
    // output) folded by `sum_o_h{1..14}`, then the FFN.
    let layer = |base: &str| format!("prefill_blk_0_{base}");
    let mut expected: Vec<String> = vec!["prefill_gather".to_owned()];
    expected.push(layer("attn_norm"));
    // Per query head: Q projection then per-position RoPE (Q stays f32).
    for hq in 0..HEAD_COUNT as usize {
        expected.push(layer(&format!("attn_q_h{hq}")));
        expected.push(layer(&format!("rope_q_h{hq}")));
    }
    // Per KV head: K projection → RoPE → f16 rounding → transpose, then V
    // projection → f16 rounding.
    for g in 0..KV_HEAD_COUNT as usize {
        expected.push(layer(&format!("attn_k_g{g}")));
        expected.push(layer(&format!("rope_k_g{g}")));
        expected.push(layer(&format!("kv_f16_round_k_g{g}")));
        expected.push(layer(&format!("transpose_k_g{g}")));
        expected.push(layer(&format!("attn_v_g{g}")));
        expected.push(layer(&format!("kv_f16_round_v_g{g}")));
    }
    // Per query head: scores → scale → softmax → context → output, folding
    // the head outputs from the second head on.
    for hq in 0..HEAD_COUNT as usize {
        expected.push(layer(&format!("scores_h{hq}")));
        expected.push(layer(&format!("scale_scores_h{hq}")));
        expected.push(layer(&format!("causal_softmax_h{hq}")));
        expected.push(layer(&format!("context_h{hq}")));
        expected.push(layer(&format!("attn_output_h{hq}")));
        if hq > 0 {
            expected.push(layer(&format!("sum_o_h{hq}")));
        }
    }
    expected.push(layer("residual_attn"));
    expected.push(layer("ffn_norm"));
    expected.push(layer("ffn_gate"));
    expected.push(layer("ffn_up"));
    expected.push(layer("swiglu_neg"));
    expected.push(layer("swiglu_exp"));
    expected.push(layer("swiglu_add1"));
    expected.push(layer("swiglu_div"));
    expected.push(layer("swiglu_silu"));
    expected.push(layer("swiglu_up"));
    expected.push(layer("ffn_down"));
    expected.push(layer("residual_ffn"));
    assert_eq!(expected.len(), 163, "gather + one full layer");
    let entries: Vec<&str> = artifact
        .program
        .kernels
        .iter()
        .take(expected.len())
        .map(|kernel| kernel.entry.as_str())
        .collect();
    let expected_refs: Vec<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(entries, expected_refs);
}

// ---------------------------------------------------------------------------
// 3. Wire + descriptor assembly (no hardware)
// ---------------------------------------------------------------------------

#[test]
fn the_prefill_section_assembles_and_admits_for_metal() {
    let weights = zero_weights();
    let artifact = build_prefill_program().expect("prefill program assembles");
    let validation = MirValidationContext::new(&artifact.mir.types);
    let validated = ValidatedMir::new(artifact.mir.functions.clone(), validation.clone())
        .expect("the constructed prefill MIR validates");
    let mut inputs = weights.declared_inputs();
    inputs.insert("prompt_tokens".to_owned(), prompt_ids_values());
    let (cos, sin) = rope_tables(&[0, 1, 2, 3, 4, 5, 6, 7, 8], HEAD_DIM, 100_000.0);
    inputs.insert("prefill.rope.cos".to_owned(), cos);
    inputs.insert("prefill.rope.sin".to_owned(), sin);
    let section = device_section_for_program(
        &artifact.program,
        &artifact.semantics,
        &validated,
        &Interner::new(),
        DeviceSectionBuild {
            selection: DeviceSelection::Metal,
            inputs: &inputs,
            ptx_target: "sm_90",
            repeating_steps: 0,
        },
    )
    .expect("the prefill section assembles");

    // Wire 7 unchanged; admission passes; the wire carries the logits result.
    admit_device_program_section(&section.device_program).expect("wire admission passes");
    assert_eq!(section.device_program.v, super::super::DEVICE_RUN_PLAN_VERSION);
    assert_eq!(section.device_program.program.launches.len(), 5188);
    assert_eq!(
        section.declared_inputs.len(),
        1445,
        "2 fixed + 32×45 per-layer weights + prompt + 2 rope tables"
    );
    assert!(section
        .declared_inputs
        .iter()
        .any(|input| input.name == "token_embd.weight"));
    assert!(section
        .declared_inputs
        .iter()
        .any(|input| input.name == "blk.0.attn_q.h14.weight"));
}

// ---------------------------------------------------------------------------
// 4. The gated burgus Metal device run
// ---------------------------------------------------------------------------

/// The full Q1-default device run on real Metal. Gated by
/// `FABER_PREFILL_DEVICE_RUN` (the burgus Metal consumer step); the model
/// path defaults to the pinned admitted file.
#[test]
#[ignore = "env-gated: FABER_PREFILL_DEVICE_RUN=1 on the burgus Metal machine"]
fn burgus_metal_prefill_device_run() {
    if std::env::var("FABER_PREFILL_DEVICE_RUN").as_deref() != Ok("1") {
        return;
    }
    use faber_host_macos_arm64::composite_host::admitted_backends;
    if !admitted_backends().contains(&DeviceBackend::Metal) {
        panic!("FABER_PREFILL_DEVICE_RUN requires a Metal device (burgus)");
    }
    let model = std::env::var("FABER_PREFILL_MODEL").unwrap_or_else(|_| {
        "/Users/ianzepp/ai/models/SmolLM2-360M-Instruct-Q4_K_M.gguf".to_owned()
    });
    let golden = std::env::var("FABER_PREFILL_GOLDEN_DIR").unwrap_or_else(|_| {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let runtime = if manifest.ends_with("/faber") {
            format!("{manifest}-runtime")
        } else {
            format!("{manifest}/../faber-runtime")
        };
        format!("{runtime}/testdata/gi2-3-logits-golden")
    });
    let evidence = std::env::var("FABER_PREFILL_EVIDENCE_DIR").unwrap_or_else(|_| {
        format!(
            "{}/../radix/docs/factory/gpu-inference-gguf/evidence",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let outcome = run_prefill_device_route(
        &PathBuf::from(model),
        &PathBuf::from(golden),
        &PathBuf::from(evidence),
        DeviceBackend::Metal,
    )
    .unwrap_or_else(|errors| panic!("prefill device run failed: {errors:#?}"));
    println!("{outcome}");
}
