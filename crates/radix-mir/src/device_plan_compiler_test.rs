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
        moe: None,
        ssm: None,
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

#[test]
fn library_operation_spellings_have_one_owner_per_match_site() {
    let compiler = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/device_plan_compiler.rs"
    ));
    let section = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../faber/src/package/device/section.rs"
    ));

    let operations = [
        (
            LibraryOperation::CausalAttention,
            "CausalAttention",
            "Self::CausalAttention => Some(\"CausalAttention\")",
            // The section producer handles both spellings through its shared
            // fallback arm since a match_same_arms merge (c3b807c324) folded
            // them together; the arm is still the single owner of the decision.
            "_ => continue,",
        ),
        (
            LibraryOperation::QkvProjection,
            "QkvProjection",
            "Self::QkvProjection => Some(\"QkvProjection\")",
            "\"QkvProjection\" => (",
        ),
        (
            LibraryOperation::ResidualRmsNorm,
            "ResidualRmsNorm",
            "Self::ResidualRmsNorm => Some(\"ResidualRmsNorm\")",
            "\"ResidualRmsNorm\" => (vec![\"residual\", \"skip\", \"gamma\"], vec![\"output\"]),",
        ),
        (
            LibraryOperation::SwiGlu,
            "SwiGlu",
            "Self::SwiGlu => Some(\"SwiGlu\")",
            "_ => continue,",
        ),
    ];

    for (operation, spelling, compiler_owner, section_owner) in operations {
        assert_eq!(operation.entry(), Some(spelling));
        for (site, source, owner) in [
            ("compiler matches", compiler, compiler_owner),
            ("section producer", section, section_owner),
        ] {
            let count = source.matches(owner).count();
            assert_eq!(
                count, 1,
                "{spelling} must have exactly one owner at {site}; found {count} occurrences of {owner:?}"
            );
        }
    }
}

fn moe_node(
    id: u32,
    layer: u32,
    op: TypedOp,
    dims: &[u64],
    inputs: &[u32],
    moe: Option<MoEFacts>,
) -> TypedOpNode {
    TypedOpNode {
        id: GraphNodeId(id),
        layer,
        op,
        shape: shape(dims),
        inputs: inputs.iter().copied().map(GraphNodeId).collect(),
        attention: None,
        moe,
        ssm: None,
    }
}

fn moe_facts() -> MoEFacts {
    MoEFacts {
        experts: 128,
        active: 8,
        rows: 8,
        hidden: 1024,
        intermediate: 512,
    }
}

/// moe-probe exemplar class: a routed FFN block (router logits + top-k
/// selection feeding grouped expert `SwiGLU` execution).
fn moe_nodes(facts: MoEFacts) -> Vec<TypedOpNode> {
    let mut nodes = vec![node(
        0,
        0,
        TypedOp::Input,
        &[facts.rows, facts.hidden],
        &[],
        None,
    )];
    nodes.push(node(
        1,
        0,
        TypedOp::RmsNorm,
        &[facts.rows, facts.hidden],
        &[0],
        None,
    ));
    nodes.push(moe_node(
        2,
        0,
        TypedOp::RouterProjection,
        &[facts.rows, facts.experts],
        &[1],
        Some(facts),
    ));
    nodes.push(moe_node(
        3,
        0,
        TypedOp::RouterTopK,
        &[facts.rows, facts.active],
        &[2],
        Some(facts),
    ));
    nodes.push(moe_node(
        4,
        0,
        TypedOp::ExpertGate,
        &[facts.rows, facts.intermediate],
        &[1],
        Some(facts),
    ));
    nodes.push(moe_node(
        5,
        0,
        TypedOp::ExpertUp,
        &[facts.rows, facts.intermediate],
        &[1],
        Some(facts),
    ));
    nodes.push(moe_node(
        6,
        0,
        TypedOp::ExpertActivation,
        &[facts.rows, facts.intermediate],
        &[4],
        Some(facts),
    ));
    nodes.push(moe_node(
        7,
        0,
        TypedOp::ExpertMul,
        &[facts.rows, facts.intermediate],
        &[6, 5],
        Some(facts),
    ));
    nodes.push(node(
        8,
        0,
        TypedOp::Other("down-projection".to_owned()),
        &[facts.rows, facts.hidden],
        &[7],
        None,
    ));
    nodes.push(node(
        9,
        0,
        TypedOp::ResidualAdd,
        &[facts.rows, facts.hidden],
        &[1, 8],
        None,
    ));
    nodes.push(node(
        10,
        0,
        TypedOp::Other("output".to_owned()),
        &[facts.rows, facts.hidden],
        &[9],
        None,
    ));
    nodes
}

fn moe_graph(facts: MoEFacts) -> TypedModelGraph {
    TypedModelGraph::new(moe_nodes(facts)).expect("canonical moe graph")
}

#[test]
fn moe_program_lowers_to_router_and_grouped_expert_selection_rows() {
    let facts = moe_facts();
    let plan = PlanCompiler
        .compile(&moe_graph(facts))
        .expect("compile moe graph");
    assert!(
        plan.schedule
            .iter()
            .any(|call| call.operation == LibraryOperation::RouterSelection)
    );
    assert!(
        plan.schedule
            .iter()
            .any(|call| call.operation == LibraryOperation::GroupedExpertGemm)
    );
    assert_eq!(plan.executor_plan.selections.len(), plan.schedule.len());

    let router = plan
        .executor_plan
        .selections
        .iter()
        .find(|selection| selection.library_entry.as_deref() == Some("RouterSelection"))
        .expect("router selection row");
    assert_eq!(
        router
            .uniforms
            .iter()
            .find(|uniform| uniform.name == "expert_count")
            .expect("expert_count uniform")
            .bits,
        128
    );
    assert_eq!(
        router
            .uniforms
            .iter()
            .find(|uniform| uniform.name == "active_experts")
            .expect("active_experts uniform")
            .bits,
        8
    );
    assert!(
        router
            .backend_requirements
            .iter()
            .any(|requirement| requirement.backend == "metal"
                && requirement.feature == "RouterSelection")
    );
    assert!(
        router
            .logical_signature
            .outputs
            .iter()
            .all(|name| name == "expert_ids" || name == "expert_weights")
    );

    let grouped = plan
        .executor_plan
        .selections
        .iter()
        .find(|selection| selection.library_entry.as_deref() == Some("GroupedExpertGemm"))
        .expect("grouped expert selection row");
    assert!(
        grouped
            .backend_requirements
            .iter()
            .any(|requirement| requirement.backend == "metal"
                && requirement.feature == "GroupedExpertGemm")
    );
    // The router row precedes the grouped row so dispatch reads resident ids.
    assert!(router.kernel_index < grouped.kernel_index);
    assert!(plan.decomposition_table().contains("GroupedExpertGemm"));
}

#[test]
fn moe_undeclared_expert_policy_fails_closed() {
    let facts = moe_facts();
    let mut graph = moe_graph(facts);
    graph.nodes[3].moe = None;
    let error = PlanCompiler
        .compile(&graph)
        .expect_err("undeclared policy must reject");
    assert!(error.to_string().contains("no declared expert policy"));
}

#[test]
fn moe_invalid_expert_policy_fails_closed() {
    let mut facts = moe_facts();
    facts.active = facts.experts + 1;
    let graph = TypedModelGraph {
        nodes: moe_nodes(facts),
    };
    let error = PlanCompiler
        .compile(&graph)
        .expect_err("invalid policy must reject");
    assert!(error.to_string().contains("MoE facts"), "{error}");
}

#[test]
fn dense_plans_stay_byte_identical_on_both_rungs() {
    for (kv_heads, q_per_kv, golden) in [
        (
            5u64,
            3u64,
            include_str!("../testdata/m8u1a_dense_plan_rung_5_3.txt"),
        ),
        (
            2,
            7,
            include_str!("../testdata/m8u1a_dense_plan_rung_2_7.txt"),
        ),
    ] {
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
        assert_eq!(format!("{plan:?}"), golden.trim_end());
    }
}

fn ssm_node(
    id: u32,
    layer: u32,
    op: TypedOp,
    dims: &[u64],
    inputs: &[u32],
    ssm: SsmFacts,
) -> TypedOpNode {
    TypedOpNode {
        id: GraphNodeId(id),
        layer,
        op,
        shape: shape(dims),
        inputs: inputs.iter().copied().map(GraphNodeId).collect(),
        attention: None,
        moe: None,
        ssm: Some(ssm),
    }
}

fn hybrid_ssm_graph() -> TypedModelGraph {
    let length = 3;
    let state_dim = 128;
    TypedModelGraph::new(vec![
        node(0, 0, TypedOp::Input, &[length, state_dim], &[], None),
        node(
            1,
            0,
            TypedOp::Other("conv-kernel".to_owned()),
            &[4],
            &[],
            None,
        ),
        ssm_node(
            2,
            0,
            TypedOp::SsmConv1d,
            &[length, state_dim],
            &[0, 1],
            SsmFacts::Conv1d {
                length,
                channels: state_dim,
                kernel_width: 4,
                layout: PackedStorageLayout::F32,
            },
        ),
        ssm_node(
            3,
            0,
            TypedOp::SsmScan,
            &[length, state_dim],
            &[2],
            SsmFacts::Scan {
                length,
                state_dim,
                layout: PackedStorageLayout::F32,
            },
        ),
        node(
            4,
            0,
            TypedOp::Other("hybrid-output".to_owned()),
            &[length, state_dim],
            &[3],
            None,
        ),
    ])
    .expect("canonical PS5-shaped hybrid graph")
}

#[test]
fn hybrid_ssm_program_lowers_to_conv_and_scan_selection_rows() {
    let plan = PlanCompiler
        .compile(&hybrid_ssm_graph())
        .expect("compile PS5-shaped hybrid graph");

    let conv = plan
        .executor_plan
        .selections
        .iter()
        .find(|selection| selection.library_entry.as_deref() == Some("SsmConv1d"))
        .expect("SSM conv selection row");
    assert_eq!(conv.bind.dims, vec![3, 128]);
    assert_eq!(conv.bind.grid.x, 3 * 128);
    assert_eq!(
        conv.binds
            .iter()
            .find(|bind| bind.name == "kernel")
            .expect("SSM kernel bind")
            .dims,
        vec![4]
    );
    assert_eq!(
        conv.uniforms
            .iter()
            .find(|uniform| uniform.name == "kernel_width")
            .expect("kernel width uniform")
            .bits,
        4
    );
    assert!(conv
        .backend_requirements
        .iter()
        .any(|requirement| requirement.backend == "metal" && requirement.feature == "SsmConv1d"));

    let scan = plan
        .executor_plan
        .selections
        .iter()
        .find(|selection| selection.library_entry.as_deref() == Some("SsmScan"))
        .expect("SSM scan selection row");
    assert_eq!(scan.bind.dims, vec![3, 128]);
    assert_eq!(scan.bind.grid.x, 3 * 128);
    assert_eq!(
        scan.uniforms
            .iter()
            .find(|uniform| uniform.name == "state_dim")
            .expect("state dimension uniform")
            .bits,
        128
    );
    assert!(
        scan.backend_requirements
            .iter()
            .any(|requirement| requirement.backend == "metal" && requirement.feature == "SsmScan")
    );
    assert!(conv.kernel_index < scan.kernel_index);
    assert!(plan.decomposition_table().contains("SsmConv1d"));
    assert!(plan.decomposition_table().contains("SsmScan"));
}

#[test]
fn hybrid_ssm_unservable_state_layout_fails_closed() {
    let mut graph = hybrid_ssm_graph();
    graph.nodes[2].ssm = Some(SsmFacts::Conv1d {
        length: 3,
        channels: 128,
        kernel_width: 4,
        layout: PackedStorageLayout::Q4_K,
    });
    let error = PlanCompiler
        .compile(&graph)
        .expect_err("unservable SSM layout must reject");
    assert!(error.to_string().contains("not servable"), "{error}");
}

#[test]
fn row_major_strides_place_the_innermost_axis_first() {
    assert_eq!(row_major_strides(&[5, 3, 8, 4]), vec![96, 32, 4, 1]);
    assert_eq!(row_major_strides(&[7]), vec![1]);
    assert!(row_major_strides(&[]).is_empty());
}

#[test]
fn u32_uniform_clamps_an_extent_wider_than_its_field() {
    assert_eq!(u32_uniform("extent", u64::from(u32::MAX)).bits, u32::MAX);
    assert_eq!(u32_uniform("extent", u64::MAX).bits, u32::MAX);
}
