//! Analyzed load-time ingress for the device-executor primary path (M4-U3).
//!
//! The allowed route implemented here is a purpose-built model-graph → plan
//! materializer.  It consumes GGUF header facts (architecture, dimensions,
//! GQA, sequence, regime) and emits a canonical [`TypedModelGraph`] that
//! preserves the analysis facts the plan compiler requires.  Raw `HirModule`
//! construction is not a path.  Generated Faber is debug-only.

use crate::device_plan_compiler::{
    AttentionFacts, CompiledDevicePlan, GraphError, GraphNodeId, LibraryOperation, PlanCompiler,
    TensorShape, TypedModelGraph, TypedOp, TypedOpNode, PLAN_COMPILER_REVISION,
};
use std::fmt::Write as _;

/// Revision of the analyzed-ingress materializer.
pub const INGRESS_REVISION: u32 = 1;

/// Stable cache-key class for the compiled semantic plan.
pub const SEMANTIC_PLAN_KEY_CLASS: &str = "semantic-plan";

/// Stable cache-key class for the backend artifact identity.
pub const BACKEND_ARTIFACT_KEY_CLASS: &str = "backend-artifact";

/// Prefill vs first-continuation decode.  Sequence extent is part of the
/// semantic-plan identity; the two regimes do not share a compiled plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRegime {
    /// Prompt-end prefill.
    Prefill,
    /// First-continuation decode (`seq = prompt_len + 1`).
    Decode,
}

impl PlanRegime {
    /// Stable spelling used in cache keys and debug Faber.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Prefill => "prefill",
            Self::Decode => "decode",
        }
    }
}

/// Analyzed facts recovered from a GGUF header (never the weight payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelGraphFacts {
    /// `general.architecture`.
    pub architecture: String,
    /// Transformer layer count.
    pub layers: u32,
    /// Hidden width.
    pub hidden: u64,
    /// Query head count.
    pub heads: u64,
    /// Key/value group count.
    pub kv_heads: u64,
    /// Per-head feature width.
    pub head_dim: u64,
    /// Feed-forward inner width.
    pub ffn: u64,
    /// Vocabulary width.
    pub vocab: u64,
    /// Sequence / context bound for this compiled plan.
    pub seq: u64,
    /// Batch extent. Dense rungs are batch-1.
    pub batch: u64,
    /// Prefill or decode regime.
    pub regime: PlanRegime,
    /// IEEE-754 bits of `1/sqrt(head_dim)`.
    pub scale_bits: u32,
}

impl ModelGraphFacts {
    /// Construct facts, rejecting incomplete or inconsistent GQA/dims.
    ///
    /// # Errors
    /// Returns [`GraphError::PatternMismatch`] when architecture, dimensions,
    /// or GQA facts are empty, zero, or internally inconsistent.
    #[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
    pub fn new(
        architecture: impl Into<String>,
        layers: u32,
        hidden: u64,
        heads: u64,
        kv_heads: u64,
        head_dim: u64,
        ffn: u64,
        vocab: u64,
        seq: u64,
        regime: PlanRegime,
    ) -> Result<Self, GraphError> {
        let architecture = architecture.into();
        if architecture.is_empty()
            || layers == 0
            || hidden == 0
            || heads == 0
            || kv_heads == 0
            || head_dim == 0
            || ffn == 0
            || vocab == 0
            || seq == 0
            || !heads.is_multiple_of(kv_heads)
        {
            return Err(GraphError::PatternMismatch {
                layer: 0,
                detail: "analyzed GGUF header facts are incomplete or inconsistent".to_owned(),
            });
        }
        let q_per_kv = heads / kv_heads;
        let expected_hidden = kv_heads
            .checked_mul(q_per_kv)
            .and_then(|heads| heads.checked_mul(head_dim));
        if expected_hidden != Some(hidden) {
            return Err(GraphError::PatternMismatch {
                layer: 0,
                detail: format!(
                    "hidden {hidden} does not equal kv_heads {kv_heads} * q_per_kv {q_per_kv} * head_dim {head_dim}"
                ),
            });
        }
        let scale = 1.0f32 / (head_dim as f32).sqrt();
        if !scale.is_finite() || scale == 0.0 {
            return Err(GraphError::PatternMismatch {
                layer: 0,
                detail: "attention scale derived from head_dim is invalid".to_owned(),
            });
        }
        Ok(Self {
            architecture,
            layers,
            hidden,
            heads,
            kv_heads,
            head_dim,
            ffn,
            vocab,
            seq,
            batch: 1,
            regime,
            scale_bits: scale.to_bits(),
        })
    }

    /// GQA ratio.  Facts construction already rejected a remainder.
    #[must_use]
    pub fn q_per_kv(&self) -> u64 {
        self.heads / self.kv_heads
    }

    /// Typed attention facts shared by every attention node of every layer.
    #[must_use]
    pub fn attention_facts(&self) -> AttentionFacts {
        AttentionFacts {
            kv_heads: self.kv_heads,
            q_per_kv: self.q_per_kv(),
            seq: self.seq,
            head_dim: self.head_dim,
            hidden: self.hidden,
            scale_bits: self.scale_bits,
        }
    }

    /// Semantic-plan identity string.  Header hash is evidence, not the key.
    #[must_use]
    pub fn semantic_plan_identity(&self) -> String {
        format!(
            "{class}/arch={arch}/layers={layers}/hidden={hidden}/heads={heads}/kv={kv}/hd={hd}/ffn={ffn}/vocab={vocab}/batch={batch}/seq={seq}/regime={regime}/compiler={compiler}/ingress={ingress}",
            class = SEMANTIC_PLAN_KEY_CLASS,
            arch = self.architecture,
            layers = self.layers,
            hidden = self.hidden,
            heads = self.heads,
            kv = self.kv_heads,
            hd = self.head_dim,
            ffn = self.ffn,
            vocab = self.vocab,
            batch = self.batch,
            seq = self.seq,
            regime = self.regime.spelling(),
            compiler = PLAN_COMPILER_REVISION,
            ingress = INGRESS_REVISION,
        )
    }
}

/// Backend-artifact identity.  Packed-weight and prepared-session keys are M5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendArtifactKey {
    /// Semantic-plan identity this artifact was compiled from.
    pub semantic_identity: String,
    /// Backend spelling (`metal`).
    pub backend: String,
    /// Device capability class.
    pub device_class: String,
    /// Artifact ABI version (WIRE device-program version).
    pub artifact_abi: u32,
    /// Kernel-library revision.
    pub kernel_library_revision: u32,
}

impl BackendArtifactKey {
    /// Construct the M4-U3 metal backend-artifact key.
    #[must_use]
    pub fn metal(semantic_identity: impl Into<String>) -> Self {
        Self {
            semantic_identity: semantic_identity.into(),
            backend: "metal".to_owned(),
            device_class: "macos-arm64".to_owned(),
            artifact_abi: 8,
            kernel_library_revision: 1,
        }
    }

    /// Backend-artifact identity string.
    #[must_use]
    pub fn identity(&self) -> String {
        format!(
            "{class}/semantic={semantic}/backend={backend}/device={device}/abi={abi}/lib={lib}",
            class = BACKEND_ARTIFACT_KEY_CLASS,
            semantic = self.semantic_identity,
            backend = self.backend,
            device = self.device_class,
            abi = self.artifact_abi,
            lib = self.kernel_library_revision,
        )
    }
}

/// Deterministic schedule fingerprint used by cache hit equivalence.
#[must_use]
pub fn schedule_identity(plan: &CompiledDevicePlan) -> String {
    plan.schedule
        .iter()
        .map(|call| {
            format!(
                "{}:{}:{}",
                call.layer,
                operation_spelling(call.operation),
                call.source_nodes
                    .iter()
                    .map(|node| node.0.to_string())
                    .collect::<Vec<_>>()
                    .join("/")
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn operation_spelling(operation: LibraryOperation) -> &'static str {
    operation.entry().unwrap_or("Generated")
}

/// Materialize the canonical typed graph from analyzed header facts.
///
/// # Errors
/// Returns [`GraphError`] when a layer shape is invalid or the assembled
/// graph is not a canonical topological order.
pub fn materialize_typed_graph(facts: &ModelGraphFacts) -> Result<TypedModelGraph, GraphError> {
    let attention = facts.attention_facts();
    let mut nodes = Vec::new();
    let mut next_id = 0_u32;
    for layer in 0..facts.layers {
        nodes.extend(canonical_layer_nodes(
            layer,
            &mut next_id,
            facts,
            attention,
        )?);
    }
    TypedModelGraph::new(nodes)
}

/// Compile analyzed header facts to a fused schedule.
///
/// # Errors
/// Returns [`GraphError`] when graph materialization or plan compilation
/// fails closed.
pub fn compile_analyzed_ingress(facts: &ModelGraphFacts) -> Result<CompiledDevicePlan, GraphError> {
    PlanCompiler.compile(&materialize_typed_graph(facts)?)
}

/// Debug Faber retained as `--emit-faber` output.  It is not the live ingress.
#[must_use]
pub fn emit_debug_faber(facts: &ModelGraphFacts) -> String {
    let mut source = String::new();
    let _ = writeln!(
        source,
        "# generated by device-plan-ingress revision {INGRESS_REVISION}"
    );
    let _ = writeln!(source, "# live path: purpose-built model-graph → plan");
    let _ = writeln!(source, "# architecture {}", facts.architecture);
    let _ = writeln!(source, "# layers {}", facts.layers);
    let _ = writeln!(source, "# hidden {}", facts.hidden);
    let _ = writeln!(source, "# heads {}", facts.heads);
    let _ = writeln!(source, "# kv_heads {}", facts.kv_heads);
    let _ = writeln!(source, "# head_dim {}", facts.head_dim);
    let _ = writeln!(source, "# ffn {}", facts.ffn);
    let _ = writeln!(source, "# vocab {}", facts.vocab);
    let _ = writeln!(source, "# batch {}", facts.batch);
    let _ = writeln!(source, "# seq {}", facts.seq);
    let _ = writeln!(source, "# regime {}", facts.regime.spelling());
    let _ = writeln!(source, "# compiler_revision {PLAN_COMPILER_REVISION}");
    let _ = writeln!(
        source,
        "# semantic_plan_key {}",
        facts.semantic_plan_identity()
    );
    source.push_str("modulus dense_plan {\n}\n");
    source
}

fn canonical_layer_nodes(
    layer: u32,
    next_id: &mut u32,
    facts: &ModelGraphFacts,
    attention: AttentionFacts,
) -> Result<Vec<TypedOpNode>, GraphError> {
    let seq = facts.seq;
    let hidden = facts.hidden;
    let ffn = facts.ffn;
    let q = attention.q_dims();
    let kv = attention.kv_dims();
    let scores = vec![attention.kv_heads, attention.q_per_kv, seq, seq];
    let mut alloc = || {
        let id = *next_id;
        *next_id += 1;
        GraphNodeId(id)
    };
    let input = alloc();
    let rms_attn = alloc();
    let proj_q = alloc();
    let proj_k = alloc();
    let proj_v = alloc();
    let grouped_scores = alloc();
    let scale = alloc();
    let softmax = alloc();
    let context = alloc();
    let output_proj = alloc();
    let residual_attn = alloc();
    let rms_ffn = alloc();
    let gate = alloc();
    let up = alloc();
    let activation = alloc();
    let mul = alloc();
    let down = alloc();
    let residual_ffn = alloc();
    let output = alloc();
    let observation = alloc();
    Ok(vec![
        node(input, layer, TypedOp::Input, &[seq, hidden], &[], None)?,
        node(
            rms_attn,
            layer,
            TypedOp::RmsNorm,
            &[seq, hidden],
            &[input],
            None,
        )?,
        node(
            proj_q,
            layer,
            TypedOp::ProjectionQ,
            &q,
            &[rms_attn],
            Some(attention),
        )?,
        node(
            proj_k,
            layer,
            TypedOp::ProjectionK,
            &kv,
            &[rms_attn],
            Some(attention),
        )?,
        node(
            proj_v,
            layer,
            TypedOp::ProjectionV,
            &kv,
            &[rms_attn],
            Some(attention),
        )?,
        node(
            grouped_scores,
            layer,
            TypedOp::GroupedScores,
            &scores,
            &[proj_q, proj_k],
            Some(attention),
        )?,
        node(
            scale,
            layer,
            TypedOp::Scale,
            &scores,
            &[grouped_scores],
            Some(attention),
        )?,
        node(
            softmax,
            layer,
            TypedOp::CausalSoftmax,
            &scores,
            &[scale],
            Some(attention),
        )?,
        node(
            context,
            layer,
            TypedOp::GroupedContext,
            &[seq, hidden],
            &[softmax, proj_v],
            Some(attention),
        )?,
        node(
            output_proj,
            layer,
            TypedOp::Other("output-projection".to_owned()),
            &[seq, hidden],
            &[context],
            None,
        )?,
        node(
            residual_attn,
            layer,
            TypedOp::ResidualAdd,
            &[seq, hidden],
            &[rms_attn, output_proj],
            None,
        )?,
        node(
            rms_ffn,
            layer,
            TypedOp::RmsNorm,
            &[seq, hidden],
            &[residual_attn],
            None,
        )?,
        node(
            gate,
            layer,
            TypedOp::SwiGluGate,
            &[seq, ffn],
            &[rms_ffn],
            None,
        )?,
        node(up, layer, TypedOp::SwiGluUp, &[seq, ffn], &[rms_ffn], None)?,
        node(
            activation,
            layer,
            TypedOp::SwiGluActivation,
            &[seq, ffn],
            &[gate],
            None,
        )?,
        node(
            mul,
            layer,
            TypedOp::SwiGluMul,
            &[seq, ffn],
            &[activation, up],
            None,
        )?,
        node(
            down,
            layer,
            TypedOp::Other("down-projection".to_owned()),
            &[seq, hidden],
            &[mul],
            None,
        )?,
        node(
            residual_ffn,
            layer,
            TypedOp::ResidualAdd,
            &[seq, hidden],
            &[residual_attn, down],
            None,
        )?,
        node(
            output,
            layer,
            TypedOp::Other("output".to_owned()),
            &[seq, hidden],
            &[residual_ffn],
            None,
        )?,
        node(
            observation,
            layer,
            TypedOp::Other("observation".to_owned()),
            &[seq, hidden],
            &[output],
            None,
        )?,
    ])
}

fn node(
    id: GraphNodeId,
    layer: u32,
    op: TypedOp,
    dims: &[u64],
    inputs: &[GraphNodeId],
    attention: Option<AttentionFacts>,
) -> Result<TypedOpNode, GraphError> {
    Ok(TypedOpNode {
        id,
        layer,
        op,
        shape: TensorShape::new(dims.to_vec())?,
        inputs: inputs.to_vec(),
        attention,
        moe: None,
        ssm: None,
    })
}

#[cfg(test)]
#[path = "device_plan_ingress_test.rs"]
mod tests;
