//! Canonical typed model/op graph plan compiler for the device-executor spine.
//!
//! This pass is deliberately above one-to-one MIR kernel decomposition.  It
//! recognizes only typed, shape-checked patterns and lowers them to ordinary
//! library selections.  It never adds a `Fused*` variant to
//! [`crate::CollectionKernelPlan`].  Unrecognized nodes remain explicit
//! fallback schedule entries and an incomplete pattern is a typed miss rather
//! than a guessed fusion.

use crate::device_program::{Binding, DeviceProgram};
use crate::device_program_plans::{
    BackendRequirement, BindDescriptor, BindLayout, ExecutorPlanExtension, LibraryBind,
    LibrarySelection, LogicalSignature, OffsetBinding, PlanProvenance, UniformBinding,
    WorkspaceBinding, EXECUTOR_PLAN_BIND_ABI_VERSION,
};
use crate::MirKernelWorkgroupCount;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Revision of the canonical graph recognition rules.
pub const PLAN_COMPILER_REVISION: u32 = 1;

/// Stable id of a typed graph node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphNodeId(pub u32);

/// Target-neutral tensor shape carried by the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorShape {
    /// Logical extents, outermost first.
    pub dims: Vec<u64>,
}

impl TensorShape {
    /// Construct a shape, rejecting empty or zero axes.
    pub fn new(dims: impl Into<Vec<u64>>) -> Result<Self, GraphError> {
        let dims = dims.into();
        if dims.is_empty() || dims.contains(&0) {
            return Err(GraphError::InvalidShape { dims });
        }
        Ok(Self { dims })
    }
}

/// GQA facts required for a causal-attention library selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttentionFacts {
    /// Number of key/value groups.
    pub kv_heads: u64,
    /// Query heads sharing each key/value group.
    pub q_per_kv: u64,
    /// Sequence block length.
    pub seq: u64,
    /// Per-head feature width.
    pub head_dim: u64,
    /// Sequence-major hidden width.
    pub hidden: u64,
    /// Frozen score scale, carried as IEEE-754 bits.
    pub scale_bits: u32,
}

impl AttentionFacts {
    fn valid(self) -> bool {
        let Some(hidden) = self
            .kv_heads
            .checked_mul(self.q_per_kv)
            .and_then(|heads| heads.checked_mul(self.head_dim))
        else {
            return false;
        };
        let Some(kv_stride) = self.seq.checked_mul(self.head_dim) else {
            return false;
        };
        if self.q_per_kv.checked_mul(kv_stride).is_none() {
            return false;
        }
        self.kv_heads > 0
            && self.q_per_kv > 0
            && self.seq > 0
            && self.head_dim > 0
            && self.hidden == hidden
    }

    fn q_dims(self) -> Vec<u64> {
        vec![self.kv_heads, self.q_per_kv, self.seq, self.head_dim]
    }

    fn kv_dims(self) -> Vec<u64> {
        vec![self.kv_heads, self.seq, self.head_dim]
    }

    fn q_strides(self) -> Vec<u64> {
        vec![
            self.q_per_kv * self.seq * self.head_dim,
            self.seq * self.head_dim,
            self.head_dim,
            1,
        ]
    }

    fn kv_strides(self) -> Vec<u64> {
        vec![self.seq * self.head_dim, self.head_dim, 1]
    }

    fn sequence_major_output_strides(self) -> Vec<u64> {
        // The fused body addresses logical [group, q-head, row, d] while the
        // next O projection consumes sequence-major [row, hidden].
        vec![self.head_dim, self.head_dim, self.hidden, 1]
    }
}

/// Typed operation vocabulary.  These are semantic graph operations, not
/// plan variants.  In particular, the four attention operations below remain
/// ordinary graph nodes and compile to a `CausalAttention` library call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedOp {
    /// Shared input activation.
    Input,
    /// One Q/K/V projection.
    ProjectionQ,
    /// One key projection.
    ProjectionK,
    /// One value projection.
    ProjectionV,
    /// Grouped Q·Kᵀ score operation.
    GroupedScores,
    /// Explicit score scaling operation.
    Scale,
    /// Causal row softmax operation.
    CausalSoftmax,
    /// Grouped probability·V context operation.
    GroupedContext,
    /// RMS normalization.
    RmsNorm,
    /// Residual addition.
    ResidualAdd,
    /// SwiGLU gate projection or activation stage.
    SwiGluGate,
    /// SwiGLU up projection.
    SwiGluUp,
    /// SwiGLU activation.
    SwiGluActivation,
    /// SwiGLU gate/up product.
    SwiGluMul,
    /// An ordinary operation with no admitted fusion pattern.
    Other(String),
}

/// One node in the canonical typed graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedOpNode {
    /// Stable graph identity.
    pub id: GraphNodeId,
    /// Transformer-layer identity.
    pub layer: u32,
    /// Semantic operation.
    pub op: TypedOp,
    /// Typed result shape.
    pub shape: TensorShape,
    /// Direct data-flow predecessors.
    pub inputs: Vec<GraphNodeId>,
    /// Attention facts when this node belongs to an attention subgraph.
    pub attention: Option<AttentionFacts>,
}

/// A canonical typed model/op graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedModelGraph {
    /// Nodes in stable topological order.
    pub nodes: Vec<TypedOpNode>,
}

impl TypedModelGraph {
    /// Validate and construct a graph.  Node order is part of the canonical
    /// identity; callers must supply topological order, not declaration order
    /// recovered from emitted names.
    pub fn new(nodes: Vec<TypedOpNode>) -> Result<Self, GraphError> {
        let graph = Self { nodes };
        graph.validate()?;
        Ok(graph)
    }

    /// Validate ids, shapes, edge order, and attention facts.
    pub fn validate(&self) -> Result<(), GraphError> {
        let mut positions = BTreeMap::new();
        for (position, node) in self.nodes.iter().enumerate() {
            if positions.insert(node.id, position).is_some() {
                return Err(GraphError::DuplicateNode(node.id));
            }
            if node.attention.is_some_and(|facts| !facts.valid()) {
                return Err(GraphError::InvalidAttentionFacts(node.id));
            }
            for input in &node.inputs {
                let Some(input_position) = positions.get(input) else {
                    return Err(GraphError::UnknownInput {
                        node: node.id,
                        input: *input,
                    });
                };
                if *input_position >= position {
                    return Err(GraphError::NonTopologicalEdge {
                        node: node.id,
                        input: *input,
                    });
                }
            }
        }
        Ok(())
    }

    fn node(&self, id: GraphNodeId) -> Option<&TypedOpNode> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

/// Library operation selected by the compiler.  This is an executor
/// vocabulary, not a `CollectionKernelPlan` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryOperation {
    /// Fused scale, causal scores, softmax, and context.
    CausalAttention,
    /// Q/K/V projection group.
    QkvProjection,
    /// Residual add followed by RMS normalization.
    ResidualRmsNorm,
    /// SwiGLU gate/up/activation/product chain.
    SwiGlu,
    /// No library body admitted; retain the ordinary generated kernel.
    Generated,
}

impl LibraryOperation {
    /// Stable library entry spelling.
    #[must_use]
    pub const fn entry(self) -> Option<&'static str> {
        match self {
            Self::CausalAttention => Some("CausalAttention"),
            Self::QkvProjection => Some("QkvProjection"),
            Self::ResidualRmsNorm => Some("ResidualRmsNorm"),
            Self::SwiGlu => Some("SwiGlu"),
            Self::Generated => None,
        }
    }

    fn signature(self) -> LogicalSignature {
        let (operation, inputs, outputs) = match self {
            Self::CausalAttention => ("CausalAttention", vec!["q", "k", "v"], vec!["output"]),
            Self::QkvProjection => (
                "QkvProjection",
                vec!["input", "q_weight", "k_weight", "v_weight"],
                vec!["q", "k", "v"],
            ),
            Self::ResidualRmsNorm => (
                "ResidualRmsNorm",
                vec!["residual", "skip", "gamma"],
                vec!["output"],
            ),
            Self::SwiGlu => ("SwiGlu", vec!["gate", "up"], vec!["output"]),
            Self::Generated => ("Generated", vec!["input"], vec!["output"]),
        };
        LogicalSignature {
            operation: operation.to_owned(),
            inputs: inputs.into_iter().map(str::to_owned).collect(),
            outputs: outputs.into_iter().map(str::to_owned).collect(),
        }
    }
}

/// One scheduled call and the source graph nodes it replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledCall {
    /// Schedule position.
    pub index: usize,
    /// Transformer layer.
    pub layer: u32,
    /// Selected executor operation.
    pub operation: LibraryOperation,
    /// Canonical graph nodes represented by this call.
    pub source_nodes: Vec<GraphNodeId>,
    /// Earlier schedule calls whose outputs this call reads.
    pub depends_on: Vec<usize>,
}

/// Attributable per-layer decomposition evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerDecomposition {
    /// Transformer layer.
    pub layer: u32,
    /// Number of graph nodes before recognition.
    pub before_kernel_count: usize,
    /// Number of scheduled calls after recognition.
    pub after_kernel_count: usize,
    /// Recognized operations and their source node ids.
    pub recognized: Vec<DecompositionRow>,
}

/// One row of the required per-layer decomposition table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecompositionRow {
    /// Selected operation.
    pub operation: LibraryOperation,
    /// Source nodes consumed by the selection.
    pub source_nodes: Vec<GraphNodeId>,
}

/// The compiled schedule and its versioned executor-plan section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledDevicePlan {
    /// Ordered schedule calls.
    pub schedule: Vec<ScheduledCall>,
    /// Per-layer before/after decomposition evidence.
    pub layers: Vec<LayerDecomposition>,
    /// Versioned executor selections for the schedule.
    pub executor_plan: ExecutorPlanExtension,
}

impl CompiledDevicePlan {
    /// Number of calls scheduled for a layer.
    #[must_use]
    pub fn kernel_count(&self, layer: u32) -> usize {
        self.schedule
            .iter()
            .filter(|call| call.layer == layer)
            .count()
    }

    /// Render the mandatory attributable decomposition table.
    #[must_use]
    pub fn decomposition_table(&self) -> String {
        let mut text = String::from(
            "| layer | before | after | recognized | source nodes |\n|---:|---:|---:|---|---|\n",
        );
        for layer in &self.layers {
            let recognized = layer
                .recognized
                .iter()
                .map(|row| row.operation.entry().unwrap_or("generated"))
                .collect::<Vec<_>>()
                .join(", ");
            let source_nodes = layer
                .recognized
                .iter()
                .map(|row| {
                    row.source_nodes
                        .iter()
                        .map(|node| node.0.to_string())
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .collect::<Vec<_>>()
                .join(", ");
            text.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                layer.layer,
                layer.before_kernel_count,
                layer.after_kernel_count,
                if recognized.is_empty() {
                    "—"
                } else {
                    &recognized
                },
                if source_nodes.is_empty() {
                    "—"
                } else {
                    &source_nodes
                },
            ));
        }
        text
    }
}

/// Typed compiler failure.  All recognition failures remain explicit rather
/// than silently falling back to a semantically different fused body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    /// A graph node id appeared more than once.
    DuplicateNode(GraphNodeId),
    /// A node references an absent predecessor.
    UnknownInput {
        node: GraphNodeId,
        input: GraphNodeId,
    },
    /// An edge points forward in the canonical topological order.
    NonTopologicalEdge {
        node: GraphNodeId,
        input: GraphNodeId,
    },
    /// A shape has no positive axes.
    InvalidShape { dims: Vec<u64> },
    /// Attention facts are incomplete or inconsistent.
    InvalidAttentionFacts(GraphNodeId),
    /// A recognized pattern has a conflicting shape or fact set.
    PatternMismatch { layer: u32, detail: String },
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNode(id) => write!(f, "typed graph repeats node {}", id.0),
            Self::UnknownInput { node, input } => {
                write!(
                    f,
                    "typed graph node {} references unknown input {}",
                    node.0, input.0
                )
            }
            Self::NonTopologicalEdge { node, input } => write!(
                f,
                "typed graph edge {} -> {} is not in canonical topological order",
                input.0, node.0
            ),
            Self::InvalidShape { dims } => write!(f, "typed graph shape is invalid: {dims:?}"),
            Self::InvalidAttentionFacts(id) => {
                write!(
                    f,
                    "typed graph attention facts for node {} are invalid",
                    id.0
                )
            }
            Self::PatternMismatch { layer, detail } => {
                write!(f, "typed graph fusion miss in layer {layer}: {detail}")
            }
        }
    }
}

impl std::error::Error for GraphError {}

/// Plan compiler for the recognized inference spine.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanCompiler;

impl PlanCompiler {
    /// Compile a canonical graph to a deterministic schedule.
    pub fn compile(&self, graph: &TypedModelGraph) -> Result<CompiledDevicePlan, GraphError> {
        graph.validate()?;
        let candidates = self.find_candidates(graph)?;
        let mut claimed = BTreeSet::new();
        for candidate in &candidates {
            for node in &candidate.source_nodes {
                if !claimed.insert(*node) {
                    return Err(GraphError::PatternMismatch {
                        layer: candidate.layer,
                        detail: format!("node {} belongs to multiple fusions", node.0),
                    });
                }
            }
        }

        let mut all_candidates = candidates;
        for node in &graph.nodes {
            if !claimed.contains(&node.id) {
                all_candidates.push(Candidate {
                    layer: node.layer,
                    operation: LibraryOperation::Generated,
                    source_nodes: vec![node.id],
                });
            }
        }
        all_candidates.sort_by_key(|candidate| {
            candidate
                .source_nodes
                .iter()
                .filter_map(|id| graph.nodes.iter().position(|node| node.id == *id))
                .min()
                .unwrap_or(usize::MAX)
        });

        let mut node_to_schedule = BTreeMap::new();
        let mut schedule = Vec::with_capacity(all_candidates.len());
        let mut selections = Vec::with_capacity(all_candidates.len());
        for (index, candidate) in all_candidates.iter().enumerate() {
            let mut depends_on = BTreeSet::new();
            for source in &candidate.source_nodes {
                let node = graph.node(*source).ok_or(GraphError::UnknownInput {
                    node: *source,
                    input: *source,
                })?;
                for input in &node.inputs {
                    if !candidate.source_nodes.contains(input) {
                        if let Some(dependency) = node_to_schedule.get(input) {
                            depends_on.insert(*dependency);
                        }
                    }
                }
            }
            if depends_on.iter().any(|dependency| *dependency >= index) {
                return Err(GraphError::PatternMismatch {
                    layer: candidate.layer,
                    detail: "fusion dependency is not earlier in the schedule".to_owned(),
                });
            }
            for source in &candidate.source_nodes {
                node_to_schedule.insert(*source, index);
            }
            let bind = self.bind_for_candidate(graph, candidate)?;
            let attention_facts = candidate
                .source_nodes
                .iter()
                .find_map(|id| graph.node(*id).and_then(|node| node.attention));
            selections.push(self.selection_for_candidate(candidate, index, bind, attention_facts));
            schedule.push(ScheduledCall {
                index,
                layer: candidate.layer,
                operation: candidate.operation,
                source_nodes: candidate.source_nodes.clone(),
                depends_on: depends_on.into_iter().collect(),
            });
        }

        let mut layers = Vec::new();
        let mut layer_ids = graph
            .nodes
            .iter()
            .map(|node| node.layer)
            .collect::<Vec<_>>();
        layer_ids.sort_unstable();
        layer_ids.dedup();
        for layer in layer_ids {
            let recognized = schedule
                .iter()
                .filter(|call| call.layer == layer && call.operation != LibraryOperation::Generated)
                .map(|call| DecompositionRow {
                    operation: call.operation,
                    source_nodes: call.source_nodes.clone(),
                })
                .collect();
            layers.push(LayerDecomposition {
                layer,
                before_kernel_count: graph
                    .nodes
                    .iter()
                    .filter(|node| node.layer == layer)
                    .count(),
                after_kernel_count: schedule.iter().filter(|call| call.layer == layer).count(),
                recognized,
            });
        }

        let executor_plan = ExecutorPlanExtension {
            abi_version: EXECUTOR_PLAN_BIND_ABI_VERSION,
            selections,
        };
        executor_plan
            .validate_for_kernel_count(schedule.len())
            .map_err(|detail| GraphError::PatternMismatch { layer: 0, detail })?;
        Ok(CompiledDevicePlan {
            schedule,
            layers,
            executor_plan,
        })
    }

    fn find_candidates(&self, graph: &TypedModelGraph) -> Result<Vec<Candidate>, GraphError> {
        let mut candidates = Vec::new();
        candidates.extend(self.find_qkv(graph));
        candidates.extend(self.find_attention(graph)?);
        candidates.extend(self.find_residual_rms(graph));
        candidates.extend(self.find_swiglu(graph));
        candidates.sort_by_key(|candidate| candidate.source_nodes[0]);
        Ok(candidates)
    }

    fn find_qkv(&self, graph: &TypedModelGraph) -> Vec<Candidate> {
        let mut candidates = Vec::new();
        let mut seen_layers = BTreeSet::new();
        for q in graph
            .nodes
            .iter()
            .filter(|node| node.op == TypedOp::ProjectionQ)
        {
            if seen_layers.contains(&q.layer) {
                continue;
            }
            let Some(facts) = q.attention else {
                continue;
            };
            if q.shape.dims != facts.q_dims() {
                continue;
            }
            let Some(k) = graph.nodes.iter().find(|node| {
                node.layer == q.layer
                    && node.op == TypedOp::ProjectionK
                    && node.inputs == q.inputs
                    && node.attention == Some(facts)
                    && node.shape.dims == facts.kv_dims()
            }) else {
                continue;
            };
            let Some(v) = graph.nodes.iter().find(|node| {
                node.layer == q.layer
                    && node.op == TypedOp::ProjectionV
                    && node.inputs == q.inputs
                    && node.attention == Some(facts)
                    && node.shape.dims == facts.kv_dims()
            }) else {
                continue;
            };
            seen_layers.insert(q.layer);
            candidates.push(Candidate {
                layer: q.layer,
                operation: LibraryOperation::QkvProjection,
                source_nodes: vec![q.id, k.id, v.id],
            });
        }
        candidates
    }

    fn find_attention(&self, graph: &TypedModelGraph) -> Result<Vec<Candidate>, GraphError> {
        let mut candidates = Vec::new();
        for context in graph
            .nodes
            .iter()
            .filter(|node| node.op == TypedOp::GroupedContext)
        {
            if context.inputs.len() != 2 {
                continue;
            }
            let Some(softmax_id) = context.inputs.iter().copied().find(|id| {
                graph
                    .node(*id)
                    .is_some_and(|node| node.op == TypedOp::CausalSoftmax)
            }) else {
                continue;
            };
            let Some(value) = context.inputs.iter().find_map(|id| {
                graph
                    .node(*id)
                    .filter(|node| node.op == TypedOp::ProjectionV)
            }) else {
                return Err(GraphError::PatternMismatch {
                    layer: context.layer,
                    detail: "attention context is missing its typed V projection".to_owned(),
                });
            };
            let softmax = graph.node(softmax_id).expect("checked above");
            if softmax.inputs.len() != 1 {
                return Err(GraphError::PatternMismatch {
                    layer: context.layer,
                    detail: "causal softmax must have exactly one scale input".to_owned(),
                });
            }
            let Some(scale) = graph
                .node(softmax.inputs[0])
                .filter(|node| node.op == TypedOp::Scale && node.inputs.len() == 1)
            else {
                return Err(GraphError::PatternMismatch {
                    layer: context.layer,
                    detail: "causal softmax is not directly fed by a scale node".to_owned(),
                });
            };
            let Some(scores) = graph
                .node(scale.inputs[0])
                .filter(|node| node.op == TypedOp::GroupedScores && node.inputs.len() == 2)
            else {
                return Err(GraphError::PatternMismatch {
                    layer: context.layer,
                    detail: "attention scale is not directly fed by grouped scores".to_owned(),
                });
            };
            let Some(query) = scores.inputs.iter().find_map(|id| {
                graph
                    .node(*id)
                    .filter(|node| node.op == TypedOp::ProjectionQ)
            }) else {
                return Err(GraphError::PatternMismatch {
                    layer: context.layer,
                    detail: "attention scores are missing their typed Q projection".to_owned(),
                });
            };
            let Some(key) = scores.inputs.iter().find_map(|id| {
                graph
                    .node(*id)
                    .filter(|node| node.op == TypedOp::ProjectionK)
            }) else {
                return Err(GraphError::PatternMismatch {
                    layer: context.layer,
                    detail: "attention scores are missing their typed K projection".to_owned(),
                });
            };
            let facts = [context, softmax, scale, scores, query, key, value]
                .into_iter()
                .map(|node| node.attention)
                .collect::<Option<Vec<_>>>();
            let Some(facts) = facts else {
                return Err(GraphError::PatternMismatch {
                    layer: context.layer,
                    detail: "attention chain is missing typed GQA facts".to_owned(),
                });
            };
            if facts.iter().any(|fact| *fact != facts[0]) {
                return Err(GraphError::PatternMismatch {
                    layer: context.layer,
                    detail: "attention chain has inconsistent GQA facts".to_owned(),
                });
            }
            if [softmax, scale, scores, query, key, value]
                .into_iter()
                .any(|node| node.layer != context.layer)
            {
                return Err(GraphError::PatternMismatch {
                    layer: context.layer,
                    detail: "attention chain crosses transformer layers".to_owned(),
                });
            }
            let facts = facts[0];
            if query.shape.dims != facts.q_dims()
                || key.shape.dims != facts.kv_dims()
                || value.shape.dims != facts.kv_dims()
                || scores.shape.dims != vec![facts.kv_heads, facts.q_per_kv, facts.seq, facts.seq]
                || context.shape.dims != vec![facts.seq, facts.hidden]
            {
                return Err(GraphError::PatternMismatch {
                    layer: context.layer,
                    detail: "attention chain shapes do not match its GQA facts".to_owned(),
                });
            }
            candidates.push(Candidate {
                layer: context.layer,
                operation: LibraryOperation::CausalAttention,
                source_nodes: vec![scores.id, scale.id, softmax.id, context.id],
            });
        }
        Ok(candidates)
    }

    fn find_residual_rms(&self, graph: &TypedModelGraph) -> Vec<Candidate> {
        let mut candidates = Vec::new();
        for rms in graph
            .nodes
            .iter()
            .filter(|node| node.op == TypedOp::RmsNorm)
        {
            if rms.inputs.len() != 1 {
                continue;
            }
            let Some(residual_id) = rms.inputs.iter().copied().find(|id| {
                graph.node(*id).is_some_and(|node| {
                    node.layer == rms.layer
                        && node.op == TypedOp::ResidualAdd
                        && node.inputs.len() == 2
                })
            }) else {
                continue;
            };
            candidates.push(Candidate {
                layer: rms.layer,
                operation: LibraryOperation::ResidualRmsNorm,
                source_nodes: vec![residual_id, rms.id],
            });
        }
        candidates
    }

    fn find_swiglu(&self, graph: &TypedModelGraph) -> Vec<Candidate> {
        let mut candidates = Vec::new();
        for mul in graph
            .nodes
            .iter()
            .filter(|node| node.op == TypedOp::SwiGluMul)
        {
            if mul.inputs.len() != 2 {
                continue;
            }
            let Some(activation) = mul.inputs.iter().find_map(|id| {
                graph
                    .node(*id)
                    .filter(|node| node.op == TypedOp::SwiGluActivation)
            }) else {
                continue;
            };
            let Some(up) = mul
                .inputs
                .iter()
                .find_map(|id| graph.node(*id).filter(|node| node.op == TypedOp::SwiGluUp))
            else {
                continue;
            };
            if activation.inputs.len() != 1 {
                continue;
            }
            let Some(gate) = graph
                .node(activation.inputs[0])
                .filter(|node| node.op == TypedOp::SwiGluGate && node.layer == mul.layer)
            else {
                continue;
            };
            if up.layer != mul.layer || activation.layer != mul.layer || gate.inputs != up.inputs {
                continue;
            }
            candidates.push(Candidate {
                layer: mul.layer,
                operation: LibraryOperation::SwiGlu,
                source_nodes: vec![gate.id, up.id, activation.id, mul.id],
            });
        }
        candidates
    }

    fn bind_for_candidate(
        &self,
        graph: &TypedModelGraph,
        candidate: &Candidate,
    ) -> Result<BindDescriptor, GraphError> {
        if candidate.operation == LibraryOperation::CausalAttention {
            let facts = candidate
                .source_nodes
                .iter()
                .find_map(|id| graph.node(*id).and_then(|node| node.attention))
                .ok_or_else(|| GraphError::PatternMismatch {
                    layer: candidate.layer,
                    detail: "causal attention selection has no bind facts".to_owned(),
                })?;
            let dims = facts.q_dims();
            return Ok(BindDescriptor {
                dims,
                strides: facts.sequence_major_output_strides(),
                layout: BindLayout::Strided,
                grid: MirKernelWorkgroupCount {
                    x: facts.kv_heads,
                    y: facts.q_per_kv,
                    z: 1,
                },
            });
        }
        let node =
            graph
                .node(candidate.source_nodes[0])
                .ok_or_else(|| GraphError::PatternMismatch {
                    layer: candidate.layer,
                    detail: "selection source node disappeared".to_owned(),
                })?;
        let dims = node.shape.dims.clone();
        let mut strides = vec![1u64; dims.len()];
        for axis in (0..dims.len()).rev().skip(1) {
            strides[axis] = strides[axis + 1].saturating_mul(dims[axis + 1]);
        }
        Ok(BindDescriptor {
            dims,
            strides,
            layout: BindLayout::RowMajor,
            grid: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        })
    }

    fn selection_for_candidate(
        &self,
        candidate: &Candidate,
        index: usize,
        bind: BindDescriptor,
        attention_facts: Option<AttentionFacts>,
    ) -> LibrarySelection {
        let operation = candidate.operation;
        let logical_signature = operation.signature();
        let library_entry = operation.entry().map(str::to_owned);
        let mut binds = Vec::new();
        let mut offsets = Vec::new();
        if operation == LibraryOperation::CausalAttention {
            let facts = attention_facts.expect("attention candidates carry typed facts");
            let views = [
                ("q", facts.q_dims(), facts.q_strides()),
                ("k", facts.kv_dims(), facts.kv_strides()),
                ("v", facts.kv_dims(), facts.kv_strides()),
                (
                    "output",
                    facts.q_dims(),
                    facts.sequence_major_output_strides(),
                ),
            ];
            for (binding, (name, dims, strides)) in views.into_iter().enumerate() {
                binds.push(LibraryBind {
                    name: name.to_owned(),
                    binding: Binding {
                        group: 0,
                        binding: binding as u32,
                    },
                    dims,
                    strides: strides.clone(),
                    offset_elements: 0,
                });
                offsets.push(OffsetBinding {
                    name: name.to_owned(),
                    offset_elements: 0,
                    strides,
                });
            }
        } else {
            for (binding, name) in logical_signature.inputs.iter().enumerate() {
                let binding = Binding {
                    group: 0,
                    binding: binding as u32,
                };
                binds.push(LibraryBind {
                    name: name.clone(),
                    binding,
                    dims: bind.dims.clone(),
                    strides: bind.strides.clone(),
                    offset_elements: 0,
                });
                offsets.push(OffsetBinding {
                    name: name.clone(),
                    offset_elements: 0,
                    strides: bind.strides.clone(),
                });
            }
            for (offset, name) in logical_signature.outputs.iter().enumerate() {
                let binding = Binding {
                    group: 0,
                    binding: (logical_signature.inputs.len() + offset) as u32,
                };
                binds.push(LibraryBind {
                    name: name.clone(),
                    binding,
                    dims: bind.dims.clone(),
                    strides: bind.strides.clone(),
                    offset_elements: 0,
                });
                offsets.push(OffsetBinding {
                    name: name.clone(),
                    offset_elements: 0,
                    strides: bind.strides.clone(),
                });
            }
        }
        let (uniforms, workspace, requirements) = if operation == LibraryOperation::CausalAttention
        {
            (
                vec![UniformBinding {
                    name: "scale_bits".to_owned(),
                    ty: crate::device_program_plans::UniformType::U32,
                    bits: attention_facts.expect("attention facts").scale_bits,
                }],
                Some(WorkspaceBinding {
                    bytes: 0,
                    alignment: 16,
                }),
                vec![BackendRequirement {
                    backend: "metal".to_owned(),
                    feature: "CausalAttention".to_owned(),
                }],
            )
        } else {
            (Vec::new(), None, Vec::new())
        };
        LibrarySelection {
            kernel_index: index,
            library_entry,
            logical_signature,
            bind,
            binds,
            uniforms,
            offsets,
            workspace,
            backend_requirements: requirements,
            provenance: PlanProvenance {
                pass: "device-plan-compiler".to_owned(),
                revision: PLAN_COMPILER_REVISION,
                source_nodes: candidate.source_nodes.iter().map(|id| id.0).collect(),
            },
        }
    }
}

/// Compile an existing DeviceProgram's ordinary kernels into the additive
/// executor section.  This is intentionally conservative: it never invents a
/// fusion from an entry name, and leaves that program's existing selections to
/// [`crate::executor_plan_extension`].
#[must_use]
pub fn executor_plan_for_program(program: &DeviceProgram) -> ExecutorPlanExtension {
    crate::executor_plan_extension(program)
}

#[derive(Debug, Clone)]
struct Candidate {
    layer: u32,
    operation: LibraryOperation,
    source_nodes: Vec<GraphNodeId>,
}

#[cfg(test)]
mod tests {
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
        let mut nodes = vec![node(0, 0, TypedOp::Input, &[8, facts.hidden], &[], None)];
        nodes.extend([
            node(1, 0, TypedOp::RmsNorm, &[8, facts.hidden], &[0], None),
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
                &[8, facts.hidden],
                &[8],
                None,
            ),
            node(
                10,
                0,
                TypedOp::ResidualAdd,
                &[8, facts.hidden],
                &[1, 9],
                None,
            ),
            node(11, 0, TypedOp::RmsNorm, &[8, facts.hidden], &[10], None),
            node(12, 0, TypedOp::SwiGluGate, &[8, 16], &[11], None),
            node(13, 0, TypedOp::SwiGluUp, &[8, 16], &[11], None),
            node(14, 0, TypedOp::SwiGluActivation, &[8, 16], &[12], None),
            node(15, 0, TypedOp::SwiGluMul, &[8, 16], &[14, 13], None),
            node(
                16,
                0,
                TypedOp::Other("down-projection".to_owned()),
                &[8, facts.hidden],
                &[15],
                None,
            ),
            node(
                17,
                0,
                TypedOp::ResidualAdd,
                &[8, facts.hidden],
                &[10, 16],
                None,
            ),
            node(
                18,
                0,
                TypedOp::Other("output".to_owned()),
                &[8, facts.hidden],
                &[17],
                None,
            ),
            node(
                19,
                0,
                TypedOp::Other("observation".to_owned()),
                &[8, facts.hidden],
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
}
