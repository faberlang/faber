use super::*;

fn shape(dims: &[u64]) -> TensorShape {
    TensorShape::new(dims.to_vec()).expect("valid shape")
}

fn node(
    id: u32,
    layer: u32,
    op: TypedOp,
    dims: &[u64],
    inputs: &[u32],
    attention: Option<AttentionFacts>,
) -> TypedOpNode {
    TypedOpNode {
        id: GraphNodeId(id),
        layer,
        op,
        shape: shape(dims),
        inputs: inputs.iter().copied().map(GraphNodeId).collect(),
        attention,
    }
}

fn layer_graph(facts: AttentionFacts) -> TypedModelGraph {
    let seq = facts.seq;
    let mut nodes = vec![node(0, 0, TypedOp::Input, &[seq, facts.hidden], &[], None)];
    nodes.extend([
        node(1, 0, TypedOp::RmsNorm, &[seq, facts.hidden], &[0], None),
        node(
            2,
            0,
            TypedOp::ProjectionQ,
            &facts.q_dims(),
            &[1],
            Some(facts),
        ),
        node(
            3,
            0,
            TypedOp::ProjectionK,
            &facts.kv_dims(),
            &[1],
            Some(facts),
        ),
        node(
            4,
            0,
            TypedOp::ProjectionV,
            &facts.kv_dims(),
            &[1],
            Some(facts),
        ),
        node(
            5,
            0,
            TypedOp::GroupedScores,
            &[facts.kv_heads, facts.q_per_kv, facts.seq, facts.seq],
            &[2, 3],
            Some(facts),
        ),
        node(
            6,
            0,
            TypedOp::Scale,
            &[facts.kv_heads, facts.q_per_kv, facts.seq, facts.seq],
            &[5],
            Some(facts),
        ),
        node(
            7,
            0,
            TypedOp::CausalSoftmax,
            &[facts.kv_heads, facts.q_per_kv, facts.seq, facts.seq],
            &[6],
            Some(facts),
        ),
        node(
            8,
            0,
            TypedOp::GroupedContext,
            &[facts.seq, facts.hidden],
            &[7, 4],
            Some(facts),
        ),
        node(
            9,
            0,
            TypedOp::Other("output-projection".to_owned()),
            &[seq, facts.hidden],
            &[8],
            None,
        ),
        node(
            10,
            0,
            TypedOp::ResidualAdd,
            &[seq, facts.hidden],
            &[1, 9],
            None,
        ),
        node(11, 0, TypedOp::RmsNorm, &[seq, facts.hidden], &[10], None),
        node(12, 0, TypedOp::SwiGluGate, &[seq, 16], &[11], None),
        node(13, 0, TypedOp::SwiGluUp, &[seq, 16], &[11], None),
        node(14, 0, TypedOp::SwiGluActivation, &[seq, 16], &[12], None),
        node(15, 0, TypedOp::SwiGluMul, &[seq, 16], &[14, 13], None),
        node(
            16,
            0,
            TypedOp::Other("down-projection".to_owned()),
            &[seq, facts.hidden],
            &[15],
            None,
        ),
        node(
            17,
            0,
            TypedOp::ResidualAdd,
            &[seq, facts.hidden],
            &[10, 16],
            None,
        ),
        node(
            18,
            0,
            TypedOp::Other("output".to_owned()),
            &[seq, facts.hidden],
            &[17],
            None,
        ),
        node(
            19,
            0,
            TypedOp::Other("observation".to_owned()),
            &[seq, facts.hidden],
            &[18],
            None,
        ),
    ]);
    TypedModelGraph::new(nodes).expect("canonical layer graph")
}

#[test]
fn both_reference_rungs_recognize_attention_and_named_fusions() {
    for (kv_heads, q_per_kv) in [(5, 3), (2, 7)] {
        let facts = AttentionFacts {
            kv_heads,
            q_per_kv,
            seq: 8,
            head_dim: 4,
            hidden: kv_heads * q_per_kv * 4,
            scale_bits: (0.5f32).to_bits(),
        };
        let plan = PlanCompiler
            .compile(&layer_graph(facts))
            .expect("compile rung graph");
        assert_eq!(plan.layers[0].before_kernel_count, 20);
        assert_eq!(plan.kernel_count(0), 11);
        assert!(plan.kernel_count(0) <= 16);
        assert!(plan
            .schedule
            .iter()
            .any(|call| call.operation == LibraryOperation::CausalAttention));
        assert!(plan
            .schedule
            .iter()
            .any(|call| call.operation == LibraryOperation::QkvProjection));
        assert!(plan
            .schedule
            .iter()
            .any(|call| call.operation == LibraryOperation::SwiGlu));
        assert_eq!(plan.executor_plan.selections.len(), plan.schedule.len());
        assert!(plan.decomposition_table().contains("CausalAttention"));
    }
}

#[test]
fn incomplete_attention_fails_closed() {
    let facts = AttentionFacts {
        kv_heads: 2,
        q_per_kv: 7,
        seq: 8,
        head_dim: 4,
        hidden: 56,
        scale_bits: (0.5f32).to_bits(),
    };
    let mut graph = layer_graph(facts);
    graph.nodes[7].attention = None;
    let error = PlanCompiler
        .compile(&graph)
        .expect_err("missing facts must reject");
    assert!(error.to_string().contains("missing typed GQA facts"));
}

#[test]
fn decode_seq_one_exposes_gemv_uniform_on_qkv_and_swiglu() {
    let facts = AttentionFacts {
        kv_heads: 5,
        q_per_kv: 3,
        seq: 1,
        head_dim: 4,
        hidden: 60,
        scale_bits: (0.5f32).to_bits(),
    };
    let plan = PlanCompiler
        .compile(&layer_graph(facts))
        .expect("compile decode graph");
    let qkv = plan
        .executor_plan
        .selections
        .iter()
        .find(|selection| selection.library_entry.as_deref() == Some("QkvProjection"))
        .expect("qkv selection");
    let decode = qkv
        .uniforms
        .iter()
        .find(|uniform| uniform.name == crate::DECODE_GEMV_UNIFORM)
        .expect("decode_gemv uniform");
    assert_eq!(decode.bits, 1);
    let swiglu = plan
        .executor_plan
        .selections
        .iter()
        .find(|selection| selection.library_entry.as_deref() == Some("SwiGlu"))
        .expect("swiglu selection");
    let decode = swiglu
        .uniforms
        .iter()
        .find(|uniform| uniform.name == crate::DECODE_GEMV_UNIFORM)
        .expect("decode_gemv uniform");
    assert_eq!(decode.bits, 1);
}

#[test]
fn output_bind_preserves_sequence_major_context_stride() {
    let facts = AttentionFacts {
        kv_heads: 5,
        q_per_kv: 3,
        seq: 8,
        head_dim: 4,
        hidden: 60,
        scale_bits: (0.5f32).to_bits(),
    };
    let plan = PlanCompiler
        .compile(&layer_graph(facts))
        .expect("compile graph");
    let selection = plan
        .executor_plan
        .selections
        .iter()
        .find(|selection| selection.library_entry.as_deref() == Some("CausalAttention"))
        .expect("attention selection");
    assert_eq!(selection.bind.strides, vec![4, 4, 60, 1]);
    assert_eq!(selection.bind.layout, BindLayout::Strided);
}
