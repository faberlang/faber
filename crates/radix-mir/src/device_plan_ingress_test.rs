use super::*;

fn smol_facts(regime: PlanRegime, seq: u64) -> ModelGraphFacts {
    ModelGraphFacts::new("llama", 32, 960, 15, 5, 64, 2560, 49_152, seq, regime)
        .expect("smollm2 facts")
}

fn qwen_facts(regime: PlanRegime, seq: u64) -> ModelGraphFacts {
    ModelGraphFacts::new("qwen2", 24, 896, 14, 2, 64, 4864, 151_936, seq, regime)
        .expect("qwen facts")
}

#[test]
fn both_rungs_compile_fused_schedule_from_header_facts() {
    for (facts, layers) in [
        (smol_facts(PlanRegime::Prefill, 9), 32_u32),
        (qwen_facts(PlanRegime::Prefill, 17), 24),
    ] {
        let plan = compile_analyzed_ingress(&facts).expect("compile analyzed ingress");
        assert_eq!(plan.layers.len(), layers as usize);
        for layer in &plan.layers {
            assert_eq!(layer.before_kernel_count, 20);
            assert_eq!(layer.after_kernel_count, 11);
            assert!(layer.after_kernel_count <= 16);
        }
        assert_eq!(plan.kernel_count(0), 11);
        assert!(
            plan.schedule
                .iter()
                .any(|call| call.operation == LibraryOperation::CausalAttention)
        );
        assert!(
            plan.schedule
                .iter()
                .any(|call| call.operation == LibraryOperation::QkvProjection)
        );
        assert!(
            plan.schedule
                .iter()
                .any(|call| call.operation == LibraryOperation::SwiGlu)
        );
        assert!(plan.decomposition_table().contains("CausalAttention"));
    }
}

#[test]
fn semantic_plan_key_separates_regime_and_sequence() {
    let prefill = smol_facts(PlanRegime::Prefill, 9);
    let decode = smol_facts(PlanRegime::Decode, 10);
    assert_ne!(
        prefill.semantic_plan_identity(),
        decode.semantic_plan_identity()
    );
    assert!(
        prefill
            .semantic_plan_identity()
            .starts_with(SEMANTIC_PLAN_KEY_CLASS)
    );
    let backend = BackendArtifactKey::metal(prefill.semantic_plan_identity());
    assert!(backend.identity().starts_with(BACKEND_ARTIFACT_KEY_CLASS));
    assert!(
        backend
            .identity()
            .contains(&prefill.semantic_plan_identity())
    );
}

#[test]
fn inconsistent_gqa_facts_fail_closed() {
    let error = ModelGraphFacts::new(
        "llama",
        32,
        960,
        15,
        4,
        64,
        2560,
        49_152,
        9,
        PlanRegime::Prefill,
    )
    .expect_err("inconsistent GQA must reject");
    assert!(error.to_string().contains("incomplete or inconsistent"));
}

#[test]
fn debug_faber_is_not_the_live_ingress() {
    let source = emit_debug_faber(&smol_facts(PlanRegime::Prefill, 9));
    assert!(source.contains("purpose-built model-graph"));
    assert!(source.contains("architecture llama"));
    assert!(source.contains("semantic_plan_key"));
}

#[test]
fn schedule_identity_is_stable_across_compiles() {
    let facts = qwen_facts(PlanRegime::Decode, 18);
    let first = compile_analyzed_ingress(&facts).expect("first compile");
    let second = compile_analyzed_ingress(&facts).expect("second compile");
    assert_eq!(schedule_identity(&first), schedule_identity(&second));
    assert_eq!(first.kernel_count(0), 11);
}
