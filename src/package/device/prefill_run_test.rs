//! GI3-5 — the Q1-default prefill device-run driver tests.
//!
//! Families:
//! 1. **Declared f32 repack**: the KV-head group expansion math and the
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
fn kv_expansion_replicates_each_kv_head_across_its_query_group() {
    // A synthetic [2, 5*64] KV weight (5 KV heads × 64 dims) expands to
    // [2, 15*64] with each KV head's columns replicated for the 3 query heads
    // of its group.
    let source = (0..2 * 5 * 64)
        .map(|value| value as f32)
        .collect::<Vec<f32>>();
    let expanded = expand_kv_weight(&source, 2);
    assert_eq!(expanded.len(), 2 * 15 * 64);
    for k in 0..2 {
        for h in 0..15 {
            let kv_head = h / 3;
            for d in 0..64 {
                assert_eq!(
                    expanded[k * (15 * 64) + h * 64 + d],
                    source[k * (5 * 64) + kv_head * 64 + d],
                    "expanded[{k}, {h}, {d}] must copy kv head {kv_head}"
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
                attn_q: vec![0.0; (HIDDEN_SIZE * HIDDEN_SIZE) as usize],
                attn_k_exp: vec![0.0; (HIDDEN_SIZE * HIDDEN_SIZE) as usize],
                attn_v_exp: vec![0.0; (HIDDEN_SIZE * HIDDEN_SIZE) as usize],
                attn_output: vec![0.0; (HIDDEN_SIZE * HIDDEN_SIZE) as usize],
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
    // counts (1 gather + 32×24 + 3 = 772), the logits observation, and the
    // per-layer GQA grouping (k_exp/v_exp share the 960-wide matmul shape).
    assert_eq!(artifact.program.kernels.len(), 772, "1 gather + 32×24 + 3");
    assert_eq!(artifact.program.launches.len(), 772);
    assert_eq!(artifact.program.results.len(), 1, "the logits observation");
    assert_eq!(artifact.program.results[0].buffer.name, "prefill.logits");
    assert_eq!(
        artifact.program.results[0].version.element_count,
        PROMPT_TOKEN_COUNT * VOCAB_SIZE,
        "the observed logits are the full [9, 49152]"
    );
    // The first layer's kernel entries follow the forward path order.
    let entries: Vec<&str> = artifact
        .program
        .kernels
        .iter()
        .take(24)
        .map(|kernel| kernel.entry.as_str())
        .collect();
    assert_eq!(
        entries,
        [
            "prefill_gather",
            "prefill_blk_0_attn_norm",
            "prefill_blk_0_attn_q",
            "prefill_blk_0_attn_k",
            "prefill_blk_0_attn_v",
            "prefill_blk_0_rope_q",
            "prefill_blk_0_rope_k",
            "prefill_blk_0_transpose_k",
            "prefill_blk_0_scores",
            "prefill_blk_0_scale_scores",
            "prefill_blk_0_causal_softmax",
            "prefill_blk_0_context",
            "prefill_blk_0_attn_output",
            "prefill_blk_0_residual_attn",
            "prefill_blk_0_ffn_norm",
            "prefill_blk_0_ffn_gate",
            "prefill_blk_0_ffn_up",
            "prefill_blk_0_swiglu_neg",
            "prefill_blk_0_swiglu_exp",
            "prefill_blk_0_swiglu_add1",
            "prefill_blk_0_swiglu_div",
            "prefill_blk_0_swiglu_silu",
            "prefill_blk_0_swiglu_up",
            "prefill_blk_0_ffn_down",
        ]
    );
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
    let (cos, sin) = rope_tables(8, 100_000.0);
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
    assert_eq!(section.device_program.program.launches.len(), 772);
    assert_eq!(
        section.declared_inputs.len(),
        293,
        "290 weights + prompt + 2 rope tables"
    );
    assert!(section
        .declared_inputs
        .iter()
        .any(|input| input.name == "token_embd.weight"));
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
