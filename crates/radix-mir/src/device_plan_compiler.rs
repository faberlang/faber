//! Canonical typed model/op graph plan compiler for the device-executor spine.
//!
//! This pass is deliberately above one-to-one MIR kernel decomposition.  It
//! recognizes only typed, shape-checked patterns and lowers them to ordinary
//! library selections.  It never adds a `Fused*` variant to
//! [`crate::CollectionKernelPlan`].  Unrecognized nodes remain explicit
//! fallback schedule entries and an incomplete pattern is a typed miss rather
//! than a guessed fusion.

use crate::MirKernelWorkgroupCount;
use crate::abi::PackedStorageLayout;
use crate::device_program::{Binding, DeviceProgram};
use crate::device_program_plans::{
    BackendRequirement, BindDescriptor, BindLayout, EXECUTOR_PLAN_BIND_ABI_VERSION,
    ExecutorPlanExtension, LibraryBind, LibrarySelection, LogicalSignature, OffsetBinding,
    PlanProvenance, UniformBinding, WorkspaceBinding,
};
use crate::text::push_line;
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
    ///
    /// # Errors
    /// Returns a `GraphError` when `dims` is empty or contains a zero axis.
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

    pub(crate) fn q_dims(self) -> Vec<u64> {
        vec![self.kv_heads, self.q_per_kv, self.seq, self.head_dim]
    }

    pub(crate) fn kv_dims(self) -> Vec<u64> {
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

/// `MoE` expert-routing facts required for router and grouped-expert
/// selections (PM3 `RouterPolicy` semantics carried as graph facts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoEFacts {
    /// Declared expert count; also the router logit width.
    pub experts: u64,
    /// Declared active experts per token (top-k).
    pub active: u64,
    /// Token rows in this step.
    pub rows: u64,
    /// Model hidden width.
    pub hidden: u64,
    /// Per-expert FFN intermediate width.
    pub intermediate: u64,
}

impl MoEFacts {
    fn valid(self) -> bool {
        self.experts > 0
            && self.active > 0
            && self.active <= self.experts
            && self.rows > 0
            && self.hidden > 0
            && self.intermediate > 0
    }
}

/// SSM state-operation facts carried by a hybrid model graph.
///
/// These are the same typed extents admitted by [`crate::SsmConv1dPlan`] and
/// [`crate::SsmScanPlan`].  The state bodies currently prove only the plain F32
/// storage row; other closed packed layouts remain explicit typed misses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsmFacts {
    /// Causal convolution over `[length, channels]` state rows.
    Conv1d {
        /// Number of time rows.
        length: u64,
        /// Number of state channels per row.
        channels: u64,
        /// Number of causal kernel taps.
        kernel_width: u64,
        /// Stored kernel/state layout.
        layout: PackedStorageLayout,
    },
    /// Linear recurrence over `[length, state_dim]` state rows.
    Scan {
        /// Number of time rows.
        length: u64,
        /// Number of state values per row.
        state_dim: u64,
        /// Stored state/update layout.
        layout: PackedStorageLayout,
    },
}

impl SsmFacts {
    fn valid(self) -> bool {
        match self {
            Self::Conv1d {
                length,
                channels,
                kernel_width,
                ..
            } => length > 0 && channels > 0 && kernel_width > 0,
            Self::Scan {
                length, state_dim, ..
            } => length > 0 && state_dim > 0,
        }
    }

    fn servable(self) -> bool {
        match self {
            Self::Conv1d { layout, .. } | Self::Scan { layout, .. } => {
                layout == PackedStorageLayout::F32
            }
        }
    }

    fn state_dims(self) -> Vec<u64> {
        match self {
            Self::Conv1d {
                length, channels, ..
            } => vec![length, channels],
            Self::Scan {
                length, state_dim, ..
            } => vec![length, state_dim],
        }
    }

    fn extent(self) -> Option<u64> {
        match self {
            Self::Conv1d {
                length, channels, ..
            } => length.checked_mul(channels),
            Self::Scan {
                length, state_dim, ..
            } => length.checked_mul(state_dim),
        }
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
    /// `SwiGLU` gate projection or activation stage.
    SwiGluGate,
    /// `SwiGLU` up projection.
    SwiGluUp,
    /// `SwiGLU` activation.
    SwiGluActivation,
    /// `SwiGLU` gate/up product.
    SwiGluMul,
    /// Router logits projection (one logit per declared expert).
    RouterProjection,
    /// Deterministic top-k + softmax router selection (device router body).
    RouterTopK,
    /// Grouped-expert `SwiGLU` gate projection.
    ExpertGate,
    /// Grouped-expert `SwiGLU` up projection.
    ExpertUp,
    /// Grouped-expert `SwiGLU` activation.
    ExpertActivation,
    /// Grouped-expert gate/up product.
    ExpertMul,
    /// Causal SSM convolution state operation.
    SsmConv1d,
    /// SSM linear-recurrence state operation.
    SsmScan,
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
    /// `MoE` facts when this node belongs to a routed expert subgraph.
    pub moe: Option<MoEFacts>,
    /// SSM state facts when this node is a conv/scan state operation.
    pub ssm: Option<SsmFacts>,
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
    ///
    /// # Errors
    /// Returns a `GraphError` when validation rejects the node list.
    pub fn new(nodes: Vec<TypedOpNode>) -> Result<Self, GraphError> {
        let graph = Self { nodes };
        graph.validate()?;
        Ok(graph)
    }

    /// Validate ids, shapes, edge order, and attention facts.
    ///
    /// # Errors
    /// Returns a `GraphError` when a node id, shape, edge order, or attention fact is invalid.
    pub fn validate(&self) -> Result<(), GraphError> {
        let mut positions = BTreeMap::new();
        for (position, node) in self.nodes.iter().enumerate() {
            if positions.insert(node.id, position).is_some() {
                return Err(GraphError::DuplicateNode(node.id));
            }
            if node.attention.is_some_and(|facts| !facts.valid()) {
                return Err(GraphError::InvalidAttentionFacts(node.id));
            }
            if node.moe.is_some_and(|facts| !facts.valid()) {
                return Err(GraphError::InvalidMoeFacts(node.id));
            }
            if node.ssm.is_some_and(|facts| !facts.valid())
                || !matches!(
                    (&node.op, node.ssm),
                    (TypedOp::SsmConv1d, Some(SsmFacts::Conv1d { .. }))
                        | (TypedOp::SsmScan, Some(SsmFacts::Scan { .. }))
                        | (_, None)
                )
            {
                return Err(GraphError::InvalidSsmFacts(node.id));
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
    /// `SwiGLU` gate/up/activation/product chain.
    SwiGlu,
    /// Router GEMV + deterministic top-k/softmax selection feeding the
    /// grouped-dispatch ids/weights buffers (device-side router body).
    RouterSelection,
    /// Grouped expert gate/up/activation/product packed execution.
    GroupedExpertGemm,
    /// Causal SSM convolution body over state channels.
    SsmConv1d,
    /// SSM linear-recurrence scan body over state rows.
    SsmScan,
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
            Self::RouterSelection => Some("RouterSelection"),
            Self::GroupedExpertGemm => Some("GroupedExpertGemm"),
            Self::SsmConv1d => Some("SsmConv1d"),
            Self::SsmScan => Some("SsmScan"),
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
            Self::RouterSelection => (
                "RouterSelection",
                vec!["input", "router_weight"],
                vec!["expert_ids", "expert_weights"],
            ),
            Self::GroupedExpertGemm => (
                "GroupedExpertGemm",
                vec!["expert_ids", "expert_weights", "gate_weight", "up_weight"],
                vec!["output"],
            ),
            Self::SsmConv1d => ("SsmConv1d", vec!["input", "kernel"], vec!["output"]),
            Self::SsmScan => ("SsmScan", vec!["input"], vec!["output"]),
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
            push_line!(
                text,
                "| {} | {} | {} | {} | {} |",
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
            );
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
    /// `MoE` expert-routing facts are incomplete or inconsistent.
    InvalidMoeFacts(GraphNodeId),
    /// SSM state-operation facts are incomplete or inconsistent.
    InvalidSsmFacts(GraphNodeId),
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
            Self::InvalidMoeFacts(id) => {
                write!(f, "typed graph MoE facts for node {} are invalid", id.0)
            }
            Self::InvalidSsmFacts(id) => {
                write!(f, "typed graph SSM facts for node {} are invalid", id.0)
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
    ///
    /// # Errors
    /// Returns a `GraphError` when graph validation or candidate admission fails.
    pub fn compile(&self, graph: &TypedModelGraph) -> Result<CompiledDevicePlan, GraphError> {
        graph.validate()?;
        let candidates = Self::find_candidates(graph)?;
        let claimed = claim_fusion_nodes(&candidates)?;

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
                    if !candidate.source_nodes.contains(input)
                        && let Some(dependency) = node_to_schedule.get(input)
                    {
                        depends_on.insert(*dependency);
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
            let bind = Self::bind_for_candidate(graph, candidate)?;
            let attention_facts = candidate
                .source_nodes
                .iter()
                .find_map(|id| graph.node(*id).and_then(|node| node.attention));
            let moe_facts = candidate
                .source_nodes
                .iter()
                .find_map(|id| graph.node(*id).and_then(|node| node.moe));
            let ssm_facts = candidate
                .source_nodes
                .iter()
                .find_map(|id| graph.node(*id).and_then(|node| node.ssm));
            let seq_extent = Self::projection_step_rows(graph, candidate, attention_facts);
            selections.push(Self::selection_for_candidate(
                candidate,
                index,
                bind,
                attention_facts,
                moe_facts,
                ssm_facts,
                seq_extent,
            )?);
            schedule.push(ScheduledCall {
                index,
                layer: candidate.layer,
                operation: candidate.operation,
                source_nodes: candidate.source_nodes.clone(),
                depends_on: depends_on.into_iter().collect(),
            });
        }

        let layers = layer_decompositions(graph, &schedule);

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

    fn find_candidates(graph: &TypedModelGraph) -> Result<Vec<Candidate>, GraphError> {
        let mut candidates = Vec::new();
        candidates.extend(Self::find_qkv(graph));
        candidates.extend(Self::find_attention(graph)?);
        candidates.extend(Self::find_residual_rms(graph));
        candidates.extend(Self::find_swiglu(graph));
        candidates.extend(Self::find_moe_router(graph)?);
        candidates.extend(Self::find_grouped_experts(graph)?);
        candidates.extend(Self::find_ssm(graph)?);
        candidates.sort_by_key(|candidate| candidate.source_nodes[0]);
        Ok(candidates)
    }

    fn find_qkv(graph: &TypedModelGraph) -> Vec<Candidate> {
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

    fn find_attention(graph: &TypedModelGraph) -> Result<Vec<Candidate>, GraphError> {
        let mut candidates = Vec::new();
        for context in graph
            .nodes
            .iter()
            .filter(|node| node.op == TypedOp::GroupedContext)
        {
            if context.inputs.len() != 2 {
                continue;
            }
            let Some(softmax) = context.inputs.iter().find_map(|id| {
                graph
                    .node(*id)
                    .filter(|node| node.op == TypedOp::CausalSoftmax)
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
            Self::attention_chain_facts(context, softmax, scale, scores, query, key, value)?;
            candidates.push(Candidate {
                layer: context.layer,
                operation: LibraryOperation::CausalAttention,
                source_nodes: vec![scores.id, scale.id, softmax.id, context.id],
            });
        }
        Ok(candidates)
    }

    fn attention_chain_facts(
        context: &TypedOpNode,
        softmax: &TypedOpNode,
        scale: &TypedOpNode,
        scores: &TypedOpNode,
        query: &TypedOpNode,
        key: &TypedOpNode,
        value: &TypedOpNode,
    ) -> Result<(), GraphError> {
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
        Ok(())
    }

    fn find_residual_rms(graph: &TypedModelGraph) -> Vec<Candidate> {
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

    fn find_swiglu(graph: &TypedModelGraph) -> Vec<Candidate> {
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

    /// Recognize the `MoE` router chain (logits projection + deterministic
    /// top-k/softmax) and lower it to one device router body row.  Fail
    /// closed on an undeclared or inconsistent expert policy (PM3
    /// `RouterPolicy` semantics): a routed layer without a declared policy
    /// is a typed rejection, never a guessed selection.
    fn find_moe_router(graph: &TypedModelGraph) -> Result<Vec<Candidate>, GraphError> {
        let mut candidates = Vec::new();
        for topk in graph
            .nodes
            .iter()
            .filter(|node| node.op == TypedOp::RouterTopK)
        {
            let Some(facts) = topk.moe else {
                return Err(GraphError::PatternMismatch {
                    layer: topk.layer,
                    detail: "router selection has no declared expert policy".to_owned(),
                });
            };
            if topk.inputs.len() != 1 {
                return Err(GraphError::PatternMismatch {
                    layer: topk.layer,
                    detail: "router top-k must have exactly one logits input".to_owned(),
                });
            }
            let Some(logits) = graph
                .node(topk.inputs[0])
                .filter(|node| node.op == TypedOp::RouterProjection)
            else {
                return Err(GraphError::PatternMismatch {
                    layer: topk.layer,
                    detail: "router top-k is not directly fed by router logits".to_owned(),
                });
            };
            if logits.layer != topk.layer || logits.moe != Some(facts) {
                return Err(GraphError::PatternMismatch {
                    layer: topk.layer,
                    detail: "router chain has inconsistent expert policy facts".to_owned(),
                });
            }
            if logits.shape.dims != vec![facts.rows, facts.experts]
                || topk.shape.dims != vec![facts.rows, facts.active]
            {
                return Err(GraphError::PatternMismatch {
                    layer: topk.layer,
                    detail: "router chain shapes do not match its expert policy".to_owned(),
                });
            }
            candidates.push(Candidate {
                layer: topk.layer,
                operation: LibraryOperation::RouterSelection,
                source_nodes: vec![logits.id, topk.id],
            });
        }
        Ok(candidates)
    }

    /// Recognize the grouped-expert `SwiGLU` chain and lower it to one
    /// packed grouped-expert row reading the router's device ids/weights.
    fn find_grouped_experts(graph: &TypedModelGraph) -> Result<Vec<Candidate>, GraphError> {
        let mut candidates = Vec::new();
        for mul in graph
            .nodes
            .iter()
            .filter(|node| node.op == TypedOp::ExpertMul)
        {
            if mul.inputs.len() != 2 {
                continue;
            }
            let Some(activation) = mul.inputs.iter().find_map(|id| {
                graph
                    .node(*id)
                    .filter(|node| node.op == TypedOp::ExpertActivation)
            }) else {
                continue;
            };
            let Some(up) = mul
                .inputs
                .iter()
                .find_map(|id| graph.node(*id).filter(|node| node.op == TypedOp::ExpertUp))
            else {
                continue;
            };
            if activation.inputs.len() != 1 {
                continue;
            }
            let Some(gate) = graph
                .node(activation.inputs[0])
                .filter(|node| node.op == TypedOp::ExpertGate && node.layer == mul.layer)
            else {
                continue;
            };
            if up.layer != mul.layer || activation.layer != mul.layer || gate.inputs != up.inputs {
                continue;
            }
            let facts = [gate, up, activation, mul]
                .into_iter()
                .map(|node| node.moe)
                .collect::<Option<Vec<_>>>()
                .ok_or(GraphError::PatternMismatch {
                    layer: mul.layer,
                    detail: "grouped expert chain is missing expert policy facts".to_owned(),
                })?;
            if facts.iter().any(|fact| *fact != facts[0]) {
                return Err(GraphError::PatternMismatch {
                    layer: mul.layer,
                    detail: "grouped expert chain has inconsistent expert policy facts".to_owned(),
                });
            }
            let facts = facts[0];
            if [gate, up, activation, mul]
                .into_iter()
                .any(|node| node.shape.dims != vec![facts.rows, facts.intermediate])
            {
                return Err(GraphError::PatternMismatch {
                    layer: mul.layer,
                    detail: "grouped expert chain shapes do not match its expert policy".to_owned(),
                });
            }
            candidates.push(Candidate {
                layer: mul.layer,
                operation: LibraryOperation::GroupedExpertGemm,
                source_nodes: vec![gate.id, up.id, activation.id, mul.id],
            });
        }
        Ok(candidates)
    }

    /// Recognize the typed SSM state operations used by the PS5 hybrid
    /// shape.  The bodies currently serve only F32 state storage, so every
    /// other closed layout is a deliberate typed miss rather than a guessed
    /// conversion or a fallback to a dense kernel.
    fn find_ssm(graph: &TypedModelGraph) -> Result<Vec<Candidate>, GraphError> {
        let mut candidates = Vec::new();
        for node in graph
            .nodes
            .iter()
            .filter(|node| matches!(node.op, TypedOp::SsmConv1d | TypedOp::SsmScan))
        {
            let Some(facts) = node.ssm else {
                return Err(GraphError::PatternMismatch {
                    layer: node.layer,
                    detail: "SSM state operation has no declared state facts".to_owned(),
                });
            };
            if !facts.servable() {
                return Err(GraphError::PatternMismatch {
                    layer: node.layer,
                    detail: format!("SSM state layout {facts:?} is not servable"),
                });
            }
            let Some(extent) = facts.extent() else {
                return Err(GraphError::PatternMismatch {
                    layer: node.layer,
                    detail: "SSM state extent overflows the plan grid".to_owned(),
                });
            };
            if extent == 0 || node.shape.dims != facts.state_dims() {
                return Err(GraphError::PatternMismatch {
                    layer: node.layer,
                    detail: "SSM state shape does not match its typed facts".to_owned(),
                });
            }
            match facts {
                SsmFacts::Conv1d { kernel_width, .. } => {
                    if node.inputs.len() != 2 {
                        return Err(GraphError::PatternMismatch {
                            layer: node.layer,
                            detail: "SSM conv1d requires state and kernel inputs".to_owned(),
                        });
                    }
                    let Some(input) = graph.node(node.inputs[0]) else {
                        return Err(GraphError::UnknownInput {
                            node: node.id,
                            input: node.inputs[0],
                        });
                    };
                    let Some(kernel) = graph.node(node.inputs[1]) else {
                        return Err(GraphError::UnknownInput {
                            node: node.id,
                            input: node.inputs[1],
                        });
                    };
                    if input.layer != node.layer || input.shape.dims != node.shape.dims {
                        return Err(GraphError::PatternMismatch {
                            layer: node.layer,
                            detail: "SSM conv1d input shape is not the declared state shape"
                                .to_owned(),
                        });
                    }
                    if kernel.layer != node.layer || kernel.shape.dims != vec![kernel_width] {
                        return Err(GraphError::PatternMismatch {
                            layer: node.layer,
                            detail: "SSM conv1d kernel shape is not the declared width".to_owned(),
                        });
                    }
                    candidates.push(Candidate {
                        layer: node.layer,
                        operation: LibraryOperation::SsmConv1d,
                        source_nodes: vec![node.id],
                    });
                }
                SsmFacts::Scan { .. } => {
                    if node.inputs.len() != 1 {
                        return Err(GraphError::PatternMismatch {
                            layer: node.layer,
                            detail: "SSM scan requires exactly one state input".to_owned(),
                        });
                    }
                    let Some(input) = graph.node(node.inputs[0]) else {
                        return Err(GraphError::UnknownInput {
                            node: node.id,
                            input: node.inputs[0],
                        });
                    };
                    if input.layer != node.layer || input.shape.dims != node.shape.dims {
                        return Err(GraphError::PatternMismatch {
                            layer: node.layer,
                            detail: "SSM scan input shape is not the declared state shape"
                                .to_owned(),
                        });
                    }
                    candidates.push(Candidate {
                        layer: node.layer,
                        operation: LibraryOperation::SsmScan,
                        source_nodes: vec![node.id],
                    });
                }
            }
        }
        Ok(candidates)
    }

    fn bind_for_candidate(
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
        if matches!(
            candidate.operation,
            LibraryOperation::SsmConv1d | LibraryOperation::SsmScan
        ) {
            let facts = candidate
                .source_nodes
                .iter()
                .find_map(|id| graph.node(*id).and_then(|node| node.ssm))
                .ok_or_else(|| GraphError::PatternMismatch {
                    layer: candidate.layer,
                    detail: "SSM selection has no state facts".to_owned(),
                })?;
            let extent = facts.extent().ok_or_else(|| GraphError::PatternMismatch {
                layer: candidate.layer,
                detail: "SSM state extent overflows the plan grid".to_owned(),
            })?;
            let dims = facts.state_dims();
            let mut strides = vec![1u64; dims.len()];
            for axis in (0..dims.len()).rev().skip(1) {
                strides[axis] = strides[axis + 1].saturating_mul(dims[axis + 1]);
            }
            return Ok(BindDescriptor {
                dims,
                strides,
                layout: BindLayout::RowMajor,
                grid: MirKernelWorkgroupCount {
                    x: extent,
                    y: 1,
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

    /// Row count of THIS QKV/SwiGLU step (M), not a longer context seq const.
    fn projection_step_rows(
        graph: &TypedModelGraph,
        candidate: &Candidate,
        attention_facts: Option<AttentionFacts>,
    ) -> u64 {
        let from_shape = candidate.source_nodes.first().and_then(|id| {
            graph.node(*id).and_then(|node| match node.op {
                TypedOp::ProjectionQ => node.shape.dims.get(2).copied(),
                TypedOp::ProjectionK | TypedOp::ProjectionV => node.shape.dims.get(1).copied(),
                _ => node.shape.dims.first().copied(),
            })
        });
        if matches!(
            candidate.operation,
            LibraryOperation::QkvProjection
                | LibraryOperation::SwiGlu
                | LibraryOperation::RouterSelection
                | LibraryOperation::GroupedExpertGemm
        ) {
            return from_shape
                .or_else(|| attention_facts.map(|facts| facts.seq))
                .unwrap_or(0);
        }
        attention_facts
            .map(|facts| facts.seq)
            .or(from_shape)
            .unwrap_or(0)
    }

    #[allow(clippy::too_many_arguments)]
    fn selection_for_candidate(
        candidate: &Candidate,
        index: usize,
        bind: BindDescriptor,
        attention_facts: Option<AttentionFacts>,
        moe_facts: Option<MoEFacts>,
        ssm_facts: Option<SsmFacts>,
        seq_extent: u64,
    ) -> Result<LibrarySelection, GraphError> {
        let operation = candidate.operation;
        let logical_signature = operation.signature();
        let library_entry = operation.entry().map(str::to_owned);
        let mut binds = Vec::new();
        let mut offsets = Vec::new();
        let causal_facts = if operation == LibraryOperation::CausalAttention {
            Some(attention_facts.ok_or_else(|| missing_facts(candidate, "attention"))?)
        } else {
            None
        };
        if let Some(facts) = causal_facts {
            push_causal_attention_binds(facts, &mut binds, &mut offsets);
        } else {
            push_signature_binds(
                operation,
                &logical_signature,
                &bind,
                ssm_facts,
                candidate,
                &mut binds,
                &mut offsets,
            )?;
        }
        let (mut uniforms, workspace, requirements) = if let Some(facts) = causal_facts {
            (
                vec![UniformBinding {
                    name: "scale_bits".to_owned(),
                    ty: crate::device_program_plans::UniformType::U32,
                    bits: facts.scale_bits,
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
        // MoE selections carry their declared expert policy and a
        // metal-scoped body admission requirement; dispatch lands in M8-U1b.
        let mut requirements = requirements;
        push_operation_uniforms(
            operation,
            moe_facts,
            ssm_facts,
            candidate,
            &mut uniforms,
            &mut requirements,
        )?;
        if matches!(
            operation,
            LibraryOperation::QkvProjection | LibraryOperation::SwiGlu
        ) {
            uniforms.push(UniformBinding {
                name: crate::DECODE_GEMV_UNIFORM.to_owned(),
                ty: crate::device_program_plans::UniformType::U32,
                bits: u32::from(seq_extent == 1),
            });
        }
        Ok(LibrarySelection {
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
        })
    }
}

fn claim_fusion_nodes(candidates: &[Candidate]) -> Result<BTreeSet<GraphNodeId>, GraphError> {
    let mut claimed = BTreeSet::new();
    for candidate in candidates {
        for node in &candidate.source_nodes {
            if !claimed.insert(*node) {
                return Err(GraphError::PatternMismatch {
                    layer: candidate.layer,
                    detail: format!("node {} belongs to multiple fusions", node.0),
                });
            }
        }
    }
    Ok(claimed)
}

fn layer_decompositions(
    graph: &TypedModelGraph,
    schedule: &[ScheduledCall],
) -> Vec<LayerDecomposition> {
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
    layers
}

fn push_causal_attention_binds(
    facts: AttentionFacts,
    binds: &mut Vec<LibraryBind>,
    offsets: &mut Vec<OffsetBinding>,
) {
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
                binding: u32::try_from(binding).unwrap_or(u32::MAX),
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
}

fn push_signature_binds(
    operation: LibraryOperation,
    logical_signature: &LogicalSignature,
    bind: &BindDescriptor,
    ssm_facts: Option<SsmFacts>,
    candidate: &Candidate,
    binds: &mut Vec<LibraryBind>,
    offsets: &mut Vec<OffsetBinding>,
) -> Result<(), GraphError> {
    for (binding, name) in logical_signature.inputs.iter().enumerate() {
        let binding = Binding {
            group: 0,
            binding: u32::try_from(binding).unwrap_or(u32::MAX),
        };
        let (dims, strides) = if operation == LibraryOperation::SsmConv1d && name == "kernel" {
            let Some(SsmFacts::Conv1d { kernel_width, .. }) = ssm_facts else {
                return Err(missing_facts(candidate, "SSM conv"));
            };
            (vec![kernel_width], vec![1])
        } else {
            (bind.dims.clone(), bind.strides.clone())
        };
        binds.push(LibraryBind {
            name: name.clone(),
            binding,
            dims,
            strides: strides.clone(),
            offset_elements: 0,
        });
        offsets.push(OffsetBinding {
            name: name.clone(),
            offset_elements: 0,
            strides,
        });
    }
    for (offset, name) in logical_signature.outputs.iter().enumerate() {
        let binding = Binding {
            group: 0,
            binding: u32::try_from(logical_signature.inputs.len() + offset).unwrap_or(u32::MAX),
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
    Ok(())
}

fn push_operation_uniforms(
    operation: LibraryOperation,
    moe_facts: Option<MoEFacts>,
    ssm_facts: Option<SsmFacts>,
    candidate: &Candidate,
    uniforms: &mut Vec<UniformBinding>,
    requirements: &mut Vec<BackendRequirement>,
) -> Result<(), GraphError> {
    match operation {
        LibraryOperation::RouterSelection => {
            let facts = moe_facts.ok_or_else(|| missing_facts(candidate, "router"))?;
            uniforms.push(UniformBinding {
                name: "expert_count".to_owned(),
                ty: crate::device_program_plans::UniformType::U32,
                bits: u32::try_from(facts.experts).unwrap_or(u32::MAX),
            });
            uniforms.push(UniformBinding {
                name: "active_experts".to_owned(),
                ty: crate::device_program_plans::UniformType::U32,
                bits: u32::try_from(facts.active).unwrap_or(u32::MAX),
            });
            requirements.push(BackendRequirement {
                backend: "metal".to_owned(),
                feature: "RouterSelection".to_owned(),
            });
        }
        LibraryOperation::GroupedExpertGemm => {
            let facts = moe_facts.ok_or_else(|| missing_facts(candidate, "expert"))?;
            uniforms.push(UniformBinding {
                name: "expert_count".to_owned(),
                ty: crate::device_program_plans::UniformType::U32,
                bits: u32::try_from(facts.experts).unwrap_or(u32::MAX),
            });
            requirements.push(BackendRequirement {
                backend: "metal".to_owned(),
                feature: "GroupedExpertGemm".to_owned(),
            });
        }
        LibraryOperation::SsmConv1d => {
            let Some(SsmFacts::Conv1d { kernel_width, .. }) = ssm_facts else {
                return Err(missing_facts(candidate, "SSM conv"));
            };
            uniforms.push(UniformBinding {
                name: "kernel_width".to_owned(),
                ty: crate::device_program_plans::UniformType::U32,
                bits: u32::try_from(kernel_width).unwrap_or(u32::MAX),
            });
            requirements.push(BackendRequirement {
                backend: "metal".to_owned(),
                feature: "SsmConv1d".to_owned(),
            });
        }
        LibraryOperation::SsmScan => {
            let Some(SsmFacts::Scan { state_dim, .. }) = ssm_facts else {
                return Err(missing_facts(candidate, "SSM scan"));
            };
            uniforms.push(UniformBinding {
                name: "state_dim".to_owned(),
                ty: crate::device_program_plans::UniformType::U32,
                bits: u32::try_from(state_dim).unwrap_or(u32::MAX),
            });
            requirements.push(BackendRequirement {
                backend: "metal".to_owned(),
                feature: "SsmScan".to_owned(),
            });
        }
        _ => {}
    }
    Ok(())
}

/// A candidate reached selection without the typed facts its operation needs.
fn missing_facts(candidate: &Candidate, kind: &str) -> GraphError {
    GraphError::PatternMismatch {
        layer: candidate.layer,
        detail: format!("{kind} candidate carries no matching typed facts"),
    }
}

/// Compile an existing `DeviceProgram`'s ordinary kernels into the additive
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
#[path = "device_plan_compiler_test.rs"]
mod tests;
