use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// The typed complete device program (S3-A4 / N3.3)
// ---------------------------------------------------------------------------
//
// Mirrors the radix-mir `device_program::DeviceProgram` schema field-for-field
// (the S1-1 shared-schema authority) so the wire carries the COMPLETE program:
// kernels (function id, entry, plan, typed resources, launch), launches
// (ordered; may be >1 per kernel), the program lifetime, and the explicit
// result buffers. Per-resource access and the content version are distinct
// fields (the 4-axis separation, N3.4); data-flow edges derive
// deterministically from the complete access + launches (the source facts are
// on the wire), so the host never re-derives topology from launch order or a
// slot-role string.
//
// Stage 3R (U1, S3–S7) extends the typed program with the F1–F7 carried
// facts: a semantic-value table + per-buffer identity (F1), explicit value
// generations per slot (F2), carried roots + dependency edges (F3), the
// lossless companion relation (F4), and the initialization / observation axes
// (F5/F6). `deny_unknown_fields` admission means an old consumer never
// silently ignores these sections.

/// The complete device program (wire form).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDeviceProgram {
    /// Ordered kernel declarations.
    pub kernels: Vec<WireKernelUnit>,
    /// Ordered launch sequence (a kernel may be launched more than once).
    pub launches: Vec<WireLaunchUnit>,
    /// Program execution-lifetime regime.
    pub lifetime: WireProgramLifetime,
    /// Explicit multi-output result buffers.
    pub results: Vec<WireResultBuffer>,
    /// Carried semantic-value table (F1): one row per stable value identity.
    pub semantic_values: Vec<WireSemanticValue>,
    /// Declared legal execution roots — launch ids the host may start from
    /// (F3). Never inferred from kernel declaration order.
    pub roots: Vec<u32>,
    /// Carried producer/consumer dependency edges (F3).
    pub dependencies: Vec<WireDependencyEdge>,
    /// Lossless primal/companion relation rows (F4/S7).
    pub relations: Vec<WireCompanionRelation>,
}

/// One carried semantic value (F1): the stable identity plus the origin fact
/// it is derived from. Two unrelated same-name/same-shape values are distinct
/// rows; two rows minted from one origin would alias and fail admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSemanticValue {
    /// Program-unique stable identity.
    pub id: u32,
    /// Diagnostic name (never an identity fact).
    pub name: String,
    /// The carried fact this identity is derived from.
    pub origin: WireSemanticValueOrigin,
}

/// The carried MIR/value fact a [`WireSemanticValue`] identity derives from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireSemanticValueOrigin {
    /// The value flows from a specific MIR local of a specific function.
    MirLocal { function: u32, local: u32 },
    /// A host-provided input value.
    HostInput,
    /// A synthetic value minted by the materializer (e.g. a gradient slot);
    /// the label discriminates distinct synthetic values.
    Synthetic { label: String },
}

/// One carried producer/consumer dependency edge (F3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDependencyEdge {
    /// The launch that produces the value consumed downstream.
    pub producer: u32,
    /// The launch that consumes it.
    pub consumer: u32,
    /// The buffer whose content flows across the edge.
    pub buffer: u32,
    /// The content version that flows.
    pub version: u32,
}

/// One lossless primal/companion relation row (F4/S7): selected inputs with
/// the gradient-to-primal identity, selected outputs, and device-residency.
/// Duplicate or contradictory rows fail admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCompanionRelation {
    /// Primal function id.
    pub primal: u32,
    /// Companion function id.
    pub companion: u32,
    /// Derivative kind.
    pub derivative: WireCompanionDerivativeKind,
    /// The primal is an explicitly device-resident forward kernel.
    pub device_resident: bool,
    /// The selected differentiable inputs.
    pub selected_inputs: Vec<WireCompanionSelectedInput>,
    /// The selected primal outputs.
    pub selected_outputs: Vec<WireCompanionSelectedOutput>,
}

/// The derivative kind a companion computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireCompanionDerivativeKind {
    /// Reverse-mode vector-Jacobian product.
    ReverseModeVjp,
}

/// One selected differentiable input (F4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCompanionSelectedInput {
    /// The primal parameter this input is.
    pub param: u32,
    /// Parameter position in the primal signature.
    pub position: u32,
    /// The primal parameter's type id.
    pub ty: u32,
    /// The companion result-tuple slot carrying this input's gradient.
    pub gradient_slot: u32,
}

/// One selected primal output (F4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCompanionSelectedOutput {
    /// Output position in the primal result tuple.
    pub position: u32,
    /// The output's type id.
    pub ty: u32,
    /// The upstream gradient's type id.
    pub upstream_gradient_ty: u32,
}

/// One ordered kernel declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireKernelUnit {
    /// MIR function id this kernel is lowered from.
    pub function: u32,
    /// Target-neutral logical entry name.
    pub entry: String,
    /// Shared algorithmic recipe (elementwise, matmul, reduction). A
    /// rank-extension tensor-tensor add kernel carries its typed broadcast
    /// fact inside the plan (the S6-C2 `RankExtensionAdd` variant) — the
    /// plan is a program fact, never dropped on the wire.
    pub plan: WireCollectionKernelPlan,
    /// Typed buffer slots bound by this kernel.
    pub resources: Vec<WireDeviceResource>,
    /// Workgroup / dispatch / launch plan.
    pub launch: WireKernelLaunchPlan,
}

/// The typed broadcast fact carried on the wire (S6-C2), mirroring
/// `radix_mir::abi::MirRankExtensionBroadcast`: the operand/result shapes
/// and the broadcast declaration of a rank-extension elementwise op.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBroadcastFact {
    /// Higher-rank operand shape (e.g. `[B, D]`).
    pub lhs_shape: Vec<u64>,
    /// Lower-rank operand shape (e.g. `[D]`).
    pub rhs_shape: Vec<u64>,
    /// Result shape (equals the higher-rank operand shape for an add).
    pub result_shape: Vec<u64>,
    /// The broadcast declaration.
    pub declaration: WireBroadcastDeclaration,
}

/// How a tensor operand is broadcast against its elementwise peer (A3: the
/// declaration is a carried fact, never derived from source spelling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireBroadcastDeclaration {
    /// Rank extension: LHS rank = RHS rank + 1 and the lower-rank dims equal
    /// the higher-rank trailing dims (`[B,D]` + `[D]` → `[B,D]`).
    RankExtension,
}

/// A typed buffer slot bound to a kernel: identity + content version +
/// binding + slot access (the 4-axis separation keeps access a distinct
/// field from role/lifetime, N3.4), plus the F2 generation this slot
/// consumes/produces and the F5 initialization policy that governs the
/// buffer's storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDeviceResource {
    /// The buffer's program-level identity facts.
    pub buffer: WireBufferIdentity,
    /// The content version (and shape) this slot binds.
    pub version: WireBufferVersion,
    /// Target-neutral binding indices.
    pub binding: WireBinding,
    /// Slot read/write access.
    pub access: WireResourceAccess,
    /// Explicit value generation (F2): a write slot produces this generation;
    /// a read slot consumes it; a read-write slot produces it and consumes
    /// the previous one. Generation 0 (unset) is rejected.
    pub generation: u32,
    /// Independent initialization axis (F5): how the buffer's storage is
    /// brought to its first defined state — never derived from role/lifetime.
    pub initialization: WireInitializationPolicy,
}

/// Stable identity facts of one device buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBufferIdentity {
    /// Program-unique identity key.
    pub id: u32,
    /// Logical name (diagnostics; target-neutral — never a backend binding).
    pub name: String,
    /// Program-level role (input / output / in-out).
    pub role: WireBufferRole,
    /// Where the storage lives (`DeviceHandle` / `HostOwned`).
    pub storage: WireStorageLayout,
    /// How long the storage lives.
    pub lifetime: WireBufferLifetime,
    /// The stable semantic value identity this buffer holds (F1).
    pub semantic_value: u32,
}

/// One content version of a buffer (a shape snapshot; version ≥ 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBufferVersion {
    /// Monotonic version number within the buffer (≥ 1).
    pub version: u32,
    /// Element-type spelling of this version's shape (the campaign dtype
    /// surface; "f32" for this delivery).
    pub element_ty: String,
    /// Element count of this version's shape.
    pub element_count: u64,
    /// The declaring kernel parameter's concrete shape vector
    /// (declaration-order literal dims) the producer stamped on this
    /// version; `None` when the buffer binds no declared tensor parameter.
    /// Carried beside `element_count` because the flattened count cannot
    /// distinguish `[4,4]` from `[16]` (the wire-shape widening); admitted
    /// fail-closed against the count.
    pub shape: Option<Vec<u64>>,
    /// The producer-defined reduced-buffer projection (council-8 CB-3):
    /// present when this version is the reduced output of an axis
    /// reduction — the mapping a keep-dims consumer uses to address the
    /// reduced buffer. Serialized explicitly (no silent default): an
    /// `AxisReduction` producer stamps it on the reduced buffer version and
    /// every consumer binding that version reads the carried fact.
    pub reduced_projection: Option<WireReducedProjection>,
}

/// Target-neutral binding indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBinding {
    /// First-level binding index.
    pub group: u32,
    /// Second-level binding index within the group.
    pub binding: u32,
}

/// The workgroup / dispatch / launch plan of one kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireKernelLaunchPlan {
    /// Threads per workgroup.
    pub workgroup: WireWorkgroupSize,
    /// Dispatch extent in workgroups for each axis.
    pub dispatch_size: WireDispatchSize,
    /// Derived workgroup count the host launches.
    pub workgroup_count: WireWorkgroupCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireWorkgroupSize {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireDispatchSize {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireWorkgroupCount {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

/// One entry in the ordered launch sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireLaunchUnit {
    /// Program-unique launch id.
    pub id: u32,
    /// Index into `kernels`.
    pub kernel_index: u32,
}

/// An explicit result buffer (multi-output readback).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireResultBuffer {
    /// Identity of the observed buffer.
    pub buffer: WireBufferIdentity,
    /// Observed content version and shape.
    pub version: WireBufferVersion,
    /// Role of the observation (output or in-out).
    pub role: WireBufferRole,
    /// The launch that produced the observed version.
    pub produced_by: u32,
    /// The explicit observation fact (F5/F6): this row is a DECLARED
    /// observation point. A writable intermediate or persistent state is a
    /// result only through this fact — never because it is writable.
    pub observation: WireObservationFact,
}

/// The explicit observation fact of a result row (F5/F6): the value is read
/// back at a declared completion boundary, decoupled from role and lifetime.
/// The fact also carries the **observation cadence** (S5A-U1): whether the
/// declared observation is read back every step
/// ([`PerStep`](WireObservationCadence::PerStep) — e.g. the loss) or once after
/// the step loop ([`EndOfRun`](WireObservationCadence::EndOfRun) — e.g. the
/// final forward,
/// gradients, params).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireObservationFact {
    /// The launch whose completion boundary makes the observation valid.
    pub at_launch: u32,
    /// The declared observation cadence (S5A-U1): `PerStep` or `EndOfRun`.
    pub cadence: WireObservationCadence,
}

/// The declared observation cadence of a result row (S5A-U1): WHEN a
/// declared observation is read back — an explicit wire fact, never derived
/// from the buffer's shape, role, or lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireObservationCadence {
    /// Read back at the producing launch's completion boundary within EVERY
    /// step of a `RepeatingStep` program (e.g. the loss trace).
    PerStep,
    /// Read back ONCE after the step loop at the final step's completion
    /// boundary (e.g. the final forward, final gradients, final params).
    EndOfRun,
}

impl WireObservationCadence {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::PerStep => "per-step",
            Self::EndOfRun => "end-of-run",
        }
    }
}

/// Independent initialization axis (F5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireInitializationPolicy {
    /// Storage is zero-filled at allocation.
    ZeroFill,
    /// Storage is uploaded from host-provided values.
    HostProvided,
    /// Storage is fully written by a device kernel before any read.
    KernelInitialized,
}

impl WireInitializationPolicy {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::ZeroFill => "zero-fill",
            Self::HostProvided => "host-provided",
            Self::KernelInitialized => "kernel-initialized",
        }
    }
}

/// Program-level buffer role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireBufferRole {
    /// Host-provided input; every slot accesses it read-only.
    Input,
    /// Device-produced result; read back at an observation point.
    Output,
    /// Device-resident intermediate shared across kernels.
    InOut,
}

impl WireBufferRole {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::InOut => "in-out",
        }
    }
}

/// Storage-lifetime class (the frozen three-class enum, N3.4 — no fourth
/// class by convenience).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireBufferLifetime {
    /// Allocated once for the whole program; persists across steps.
    PerProgram,
    /// Live within one training step; recycled between iterations.
    PerStep,
    /// Read back at a declared observation point.
    ObservationPoint,
}

impl WireBufferLifetime {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::PerProgram => "per-program",
            Self::PerStep => "per-step",
            Self::ObservationPoint => "observation-point",
        }
    }
}

/// Slot read/write access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireResourceAccess {
    Read,
    Write,
    ReadWrite,
}

/// Where the storage lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireStorageLayout {
    HostOwned,
    DeviceHandle,
    /// MLC-4: sparse backing store (`sparsa` values) — layout fact carried on
    /// the wire; kernel admission surfaces reject it by name, fail-closed.
    Sparse,
}

/// Program execution-lifetime regime. A `RepeatingStep` program carries its
/// **declared step count** (the S5-U5b wire carriage): the number of training
/// steps the route drives on one session, admitted fail-closed at decode (a
/// zero count is rejected by [`crate::schema::validate_wire_program`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireProgramLifetime {
    /// One-shot program run.
    #[default]
    SingleRun,
    /// Repeating training step with the declared step count; per-step buffers
    /// recycle between iterations.
    RepeatingStep(u32),
}

/// The shared algorithmic recipe set (mirror of `CollectionKernelPlan`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum WireCollectionKernelPlan {
    /// No collection-op recipe (elementwise kernel).
    Elementwise,
    /// S6-C2: an elementwise kernel whose tensor-tensor add carries the
    /// typed rank-extension broadcast fact (the `addita_bias` lowering
    /// `[B,D]` + `[D]`). The fact rides the plan so it is never dropped and
    /// never inferred (A3); admission validates it fail-closed (A7).
    RankExtensionAdd(WireBroadcastFact),
    /// Tiled-matmul recipe.
    TiledMatMul(WireMatMulPlan),
    /// Batched tiled-matmul recipe (U1 head-axis): leading head axes with
    /// the argument's batch dims a prefix of the receiver's. A distinct
    /// variant — the rank-2 `TiledMatMul` image is unchanged and a
    /// batched fact can never decode as rank-2 by omission.
    BatchedTiledMatMul(WireBatchedMatMulPlan),
    /// Sequential tree-reduction recipe.
    TreeReduction(WireReductionPlan),
    /// Rank-2 transpose recipe (S5-U1).
    Transpose(WireTransposePlan),
    /// Batched transpose recipe (U3 head-axis): leading head axes over the
    /// swapped trailing `[M, N]`. A distinct variant — the rank-2
    /// `Transpose` image is unchanged and a batched fact can never decode
    /// as rank-2 by omission.
    BatchedTranspose(WireBatchedTransposePlan),
    /// Axis-reduction recipe (S6-N1 name freeze; wire mirror lands in S6-C2):
    /// reduce a tensor over a single axis by `op`. Mirrors
    /// `kernel_plan::AxisReductionPlan`; admission/resolution is S6-P1.
    AxisReduction(WireAxisReductionPlan),
    /// Row-wise softmax recipe (S6-N1 name freeze; wire mirror lands in
    /// S6-C2): numerically stable softmax over the last (row) axis. Mirrors
    /// `kernel_plan::RowSoftmaxPlan`; admission/resolution is S6-P1.
    RowSoftmax(WireRowSoftmaxPlan),
    /// Batched row-softmax recipe (U2 head-axis): leading head/query axes
    /// with the additive mask operand kept as a broadcast fact. A distinct
    /// variant — the rank-1/2 `RowSoftmax` image is unchanged and a
    /// batched fact can never decode as rank-2 by omission.
    BatchedRowSoftmax(WireBatchedRowSoftmaxPlan),
    /// Layer-normalization recipe (S6-N1 name freeze; wire mirror lands in
    /// S6-C2): row mean/var with optional affine. Mirrors
    /// `kernel_plan::LayerNormalizationPlan`; admission/resolution is S6-P1.
    LayerNormalization(WireLayerNormalizationPlan),
    /// Row-gather recipe (GI3-1 name freeze; wire mirror in GI3-1):
    /// `out[t] = table[ids[t]]` over a rank-2 `[V, D]` table. Mirrors
    /// `kernel_plan::GatherPlan`; admission/resolution is GI3-1.
    Gather(WireGatherPlan),
    /// Root-mean-square-normalization recipe (GI3-1 name freeze; wire mirror
    /// in GI3-1): `x/sqrt(mean(x²)+eps)·γ`, no mean subtraction. Mirrors
    /// `kernel_plan::RmsNormalizationPlan`; admission/resolution is GI3-1.
    RmsNormalization(WireRmsNormalizationPlan),
    /// Rotary-position-embedding recipe (GI3-1 name freeze; wire mirror in
    /// GI3-1): llama-arch NORM consecutive-pair rotation. Mirrors
    /// `kernel_plan::RopePlan`; admission/resolution is GI3-1.
    Rope(WireRopePlan),
    /// Causal masked-row softmax recipe (GI3-1 name freeze; wire mirror in
    /// GI3-1): masked row softmax over the causal scores, zero probability
    /// above the diagonal. Mirrors `kernel_plan::CausalMaskedSoftmaxPlan`;
    /// admission/resolution is GI3-1.
    CausalMaskedSoftmax(WireCausalMaskedSoftmaxPlan),
    /// Packed quantized GEMM recipe (R-PACK-01C mechanical mirror of
    /// `kernel_plan::QuantizedMatMulPlan`). Plan fields only; accepted wire
    /// version unchanged.
    QuantizedMatMul(WireQuantizedMatMulPlan),
    /// Grouped packed GEMM recipe (R-PACK-01C mechanical mirror of
    /// `kernel_plan::GroupedMatMulPlan`). Plan fields only; accepted wire
    /// version unchanged.
    GroupedMatMul(WireGroupedMatMulPlan),
    /// M3 head-batched attention recipe. The explicit GQA axes, projection
    /// layout, strides, and rope mode are carried field-for-field.
    BatchedAttention(WireBatchedAttentionPlan),
    /// SSM conv1d recipe (R-PACK-01C mechanical mirror of
    /// `kernel_plan::SsmConv1dPlan`). Plan fields only; accepted wire
    /// version unchanged.
    SsmConv1d(WireSsmConv1dPlan),
    /// SSM linear-recurrence scan recipe (R-PACK-01C mechanical mirror of
    /// `kernel_plan::SsmScanPlan`). Plan fields only; accepted wire version
    /// unchanged.
    SsmScan(WireSsmScanPlan),
    /// Lane-form kernel (gpu-reset C4-1): one thread per lane of the domain
    /// named by `source_resource`'s call-site shape. Mirrors
    /// `kernel_plan::CollectionKernelPlan::LaneGrid`.
    LaneGrid(WireLaneGridPlan),
    /// Single-lane kernel (gpu-reset A2 rung 3): a body with no lane loop,
    /// launched as one thread. Mirrors
    /// `kernel_plan::CollectionKernelPlan::SingleLane`.
    SingleLane,
    /// Whole-grid reduction (gpu-reset C4-8). Mirrors
    /// `kernel_plan::CollectionKernelPlan::GridReduce`.
    GridReduce(WireGridReducePlan),
}

/// Axis-reduction recipe plan (wire mirror of `kernel_plan::AxisReductionPlan`,
/// S6-N1 name freeze, S6-C2 wire mirror). The axis comes from the trailing
/// const operand of the lowering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireAxisReductionPlan {
    /// Reduction operator (`Sum` in v1).
    pub op: WireReduceOp,
    /// Axis reduced over (from the trailing const operand).
    pub axis: u64,
    /// The producer-defined reduced-resource projection (council-8 CB-3):
    /// `axis_extent` (`dims[axis]`) and `inner_stride` (the dim product
    /// after the axis). Mirrored from
    /// `radix_mir::abi::ReducedProjection`; carried on the wire so a
    /// keep-dims consumer of the reduced buffer never reconstructs the
    /// projection from element count.
    pub projection: WireReducedProjection,
}

/// The wire mirror of the producer-defined reduced-buffer projection
/// (council-8 CB-3): `axis_extent` is the reduced-axis dimension
/// (`dims[axis]`) and `inner_stride` is the product of the dims after the
/// axis (`1` when the axis is last). Distinguishes equal-dim reductions
/// (a `[4,4]` axis-0 vs axis-1 reduction both produce 4 elements but map
/// differently).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireReducedProjection {
    /// The reduced-axis extent (`dims[axis]`).
    pub axis_extent: u64,
    /// The product of the dims after the axis (`1` when the axis is last).
    pub inner_stride: u64,
}

/// The named ABI successor of GEA3-WIRE-BUFFER-V1 (GEA3 delivery Amendment
/// 3, unit GEA3-A1). V1 carries the law that one (buffer id, content
/// version) names exactly one allocation identity and one element count;
/// V2 keeps that allocation law and adds sub-window projection facts so a
/// read or write slot may address a bounded window of the one allocation
/// instead of minting a second identity. The fact's carrier today is the
/// GEA3 program-plan envelope (`gea3-program-plan-v1` + this ABI tag); the
/// production `WireBufferVersion` slot lands with its own unit when the
/// FMIR image needs to carry it.
pub const GEA3_WIRE_BUFFER_ABI_V2: &str = "gea3-wire-buffer-v2";

/// The sub-window projection fact of GEA3-WIRE-BUFFER-V2 (Amendment 3): a
/// bounded window of one producer buffer — `row_count` rows of
/// `row_width` contiguous elements at `row_stride` producer-element pitch,
/// starting at `element_offset`. The window is contiguous when
/// `row_stride == row_width`; a strided window is a read-side fact only.
/// `derived_element_count` is carried beside the geometry so the
/// consumer's bound count and the projection can be cross-checked
/// fail-closed (the two-truths guard,
/// [`validate_carried_sub_window_projection`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSubWindowProjection {
    /// The producer buffer the window addresses. Must equal the carrying
    /// resource's own buffer id — the buffer keeps one identity.
    pub buffer: u32,
    /// Element offset of the window's first row inside the producer buffer.
    pub element_offset: u64,
    /// Contiguous elements per window row (the consumer's addressing width).
    pub row_width: u64,
    /// Window rows.
    pub row_count: u64,
    /// Producer elements per row (the producer's row pitch; `>= row_width`).
    pub row_stride: u64,
    /// The carried derived element count (`row_width · row_count`).
    pub derived_element_count: u64,
}

impl WireSubWindowProjection {
    /// The window's covering span in producer elements:
    /// `(row_count - 1) · row_stride + row_width`. `None` on overflow or when
    /// `row_count` is zero.
    #[must_use]
    pub fn covering_span(&self) -> Option<u64> {
        self.row_count
            .checked_sub(1)
            .and_then(|rows| rows.checked_mul(self.row_stride))
            .and_then(|span| span.checked_add(self.row_width))
    }

    /// The window addresses its rows contiguously (one flat range).
    #[must_use]
    pub fn is_contiguous(&self) -> bool {
        self.row_stride == self.row_width
    }

    /// The geometry's own element count (`row_width · row_count`); `None`
    /// on overflow.
    #[must_use]
    pub fn window_element_count(&self) -> Option<u64> {
        self.row_width.checked_mul(self.row_count)
    }
}

/// Admit one carried sub-window projection fail-closed (the GEA3-A1
/// two-truths guard, patterned on `validate_carried_reduced_projection`):
/// the geometry is positive, `derived_element_count` equals
/// `row_width · row_count`, the consumer's carried `element_count` equals
/// that derived count, the window nests inside the producer buffer's own
/// element count, and a strided window is rejected for write access (a
/// scattered write cannot be one binding). Errors name the broken law.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn validate_carried_sub_window_projection(
    carrier: &str,
    element_count: u64,
    producer_element_count: u64,
    write_access: bool,
    projection: &WireSubWindowProjection,
) -> Result<(), String> {
    if projection.buffer == 0 {
        return Err(format!(
            "{carrier} carries a sub-window projection with buffer id 0; a window addresses a real producer buffer (GEA3-WIRE-BUFFER-V2)"
        ));
    }
    if projection.row_width < 1 || projection.row_count < 1 {
        return Err(format!(
            "{carrier} carries a sub-window projection with row width {} and row count {}; a zero-extent window addresses nothing (GEA3-WIRE-BUFFER-V2)",
            projection.row_width, projection.row_count
        ));
    }
    if projection.row_stride < projection.row_width {
        return Err(format!(
            "{carrier} carries a sub-window projection with row stride {} below row width {}; rows cannot overlap their pitch (GEA3-WIRE-BUFFER-V2)",
            projection.row_stride, projection.row_width
        ));
    }
    if write_access && !projection.is_contiguous() {
        return Err(format!(
            "{carrier} carries a strided sub-window projection ({projection:?}) on a write slot; a scattered write is not one binding (GEA3-WIRE-BUFFER-V2)"
        ));
    }
    let Some(derived) = projection.window_element_count() else {
        return Err(format!(
            "{carrier} carries a sub-window projection whose row_width · row_count saturates u64 ({projection:?}); a saturated window cannot address a real buffer (GEA3-WIRE-BUFFER-V2)"
        ));
    };
    if projection.derived_element_count != derived {
        return Err(format!(
            "{carrier} carries a sub-window projection with derived element count {} but geometry {} · {} = {derived}; the carried and derived truths must agree (GEA3-WIRE-BUFFER-V2)",
            projection.derived_element_count, projection.row_width, projection.row_count
        ));
    }
    if element_count != derived {
        return Err(format!(
            "{carrier} binds {element_count} elements but its sub-window projection derives {derived}; the carried slot count and the projection's derived count must agree (GEA3-WIRE-BUFFER-V2)",
        ));
    }
    let Some(span) = projection.covering_span() else {
        return Err(format!(
            "{carrier} carries a sub-window projection whose covering span saturates u64 ({projection:?}); a saturated span cannot address a real buffer (GEA3-WIRE-BUFFER-V2)"
        ));
    };
    match projection
        .element_offset
        .checked_add(span)
        .filter(|end| *end <= producer_element_count)
    {
        Some(_) => Ok(()),
        None => Err(format!(
            "{carrier} carries a sub-window projection spanning {} of {} elements from offset {}; the window must nest inside its producer buffer (GEA3-WIRE-BUFFER-V2)",
            span, producer_element_count, projection.element_offset
        )),
    }
}

/// Row-wise softmax recipe plan (wire mirror of `kernel_plan::RowSoftmaxPlan`,
/// S6-N1 name freeze, S6-C2 wire mirror). Operates over the last (row) axis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRowSoftmaxPlan {
    /// Row axis (the last axis in v1).
    pub axis: u64,
}

/// Batched row-softmax recipe plan (U2 head-axis; wire mirror of
/// `kernel_plan::RowSoftmaxBatch`). The leading head/query axes fold into
/// the row count; the additive mask operand is kept as a broadcast count
/// fact. The decode-side admission validates the batch law fail-closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBatchedRowSoftmaxPlan {
    /// Row axis (the last axis).
    pub axis: u64,
    /// Product of the leading head/query axes — the row count.
    pub batch: u64,
    /// Last-axis (row) width — the key extent.
    pub width: u64,
    /// The additive mask buffer's element count; `None` for a plain
    /// batched softmax. When carried it must divide `batch · width`.
    pub mask_count: Option<u64>,
}

/// Layer-normalization recipe plan (wire mirror of
/// `kernel_plan::LayerNormalizationPlan`, S6-N1 name freeze, S6-C2 wire
/// mirror). The full field set (epsilon, affine) is resolved by S6-P1 from
/// the const operands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireLayerNormalizationPlan {
    /// Normalization axis (from the trailing const operand).
    pub axis: u64,
}

/// Row-gather recipe plan (wire mirror of `kernel_plan::GatherPlan`, GI3-1
/// name freeze). The typed facts resolve from the `TensorGather` statement:
/// the rank-2 `[V, D]` table dims and the id-vector length `T`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireGatherPlan {
    /// Table rows `V`.
    pub table_rows: u64,
    /// Table columns `D` (also the output row width).
    pub table_cols: u64,
    /// Id-vector length `T` (the output row count).
    pub id_count: u64,
}

/// Root-mean-square-normalization recipe plan (wire mirror of
/// `kernel_plan::RmsNormalizationPlan`, GI3-1 name freeze). The epsilon
/// rides as f32 bits (the CCI-1 `epsilon_bits` precedent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRmsNormalizationPlan {
    /// Normalization axis (from the trailing const operand).
    pub axis: u64,
    /// Epsilon as f32 bits.
    pub epsilon_bits: u32,
    /// Normalization dimension width.
    pub width: u64,
}

/// Rotary-position-embedding recipe plan (wire mirror of
/// `kernel_plan::RopePlan`, GI3-1 name freeze). llama-arch NORM
/// consecutive-pair rotation; elements beyond `dim` preserved unrotated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRopePlan {
    /// Position (single-position mode).
    pub pos: u64,
    /// Rotated width (positive and even).
    pub dim: u64,
    /// Row width (the last-axis extent).
    pub width: u64,
    /// Per-position rotation (GI3-1 amendment): each row rotates at its own
    /// position via the rank-2 `[rows, dim/2]` tables.
    pub per_row: bool,
    /// The row count (per-row mode; the table row stride).
    pub rows: u64,
    /// Qwen2 / GPT-NeoX `rotate_half` pairing. Default false keeps llama
    /// pair-rotate wire payloads unchanged.
    #[serde(default)]
    pub rotate_half: bool,
}

/// Causal masked-row softmax recipe plan (wire mirror of
/// `kernel_plan::CausalMaskedSoftmaxPlan`, GI3-1 name freeze). Row `i` masks
/// columns `j > i` (zero probability above the diagonal) before the row
/// softmax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCausalMaskedSoftmaxPlan {
    /// Row (query) extent.
    pub rows: u64,
    /// Column (key) extent.
    pub cols: u64,
}

/// Admitted packed storage layouts (R-PACK-01C mechanical mirror of
/// `radix_mir::abi::PackedStorageLayout`). GGUF spellings are the layout
/// authority; the mirror carries the closed enum, never a filename.
#[allow(non_camel_case_types)] // GGUF type names are the layout authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WirePackedStorageLayout {
    /// Plain f32 passthrough.
    F32,
    /// Native 2-byte bfloat16.
    Bf16,
    /// f16 scale + int8.
    Q8_0,
    /// 6-bit superblock `Q4_K`.
    Q4_K,
    /// 6-bit superblock `Q5_K`.
    Q5_K,
    /// 6-bit superblock `Q6_K`.
    Q6_K,
    /// f16 scale + 5-bit (dense-rung).
    Q5_0,
}

/// Packed quantized GEMM plan (R-PACK-01C mechanical mirror of
/// `kernel_plan::QuantizedMatMulPlan`). Field-for-field; no extra facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireQuantizedMatMulPlan {
    /// Left matrix rows `M`.
    pub m: u64,
    /// Contracted inner dimension `K`.
    pub k: u64,
    /// Right matrix columns `N`.
    pub n: u64,
    /// Stored weight layout.
    pub layout: WirePackedStorageLayout,
    /// Square tile edge.
    pub tile: u32,
    /// Workgroup lane count on `x`.
    pub workgroup_x: u32,
    /// Workgroup lane count on `y`.
    pub workgroup_y: u32,
}

/// Grouped packed GEMM plan (R-PACK-01C mechanical mirror of
/// `kernel_plan::GroupedMatMulPlan`). Field-for-field; no extra facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireGroupedMatMulPlan {
    /// Left matrix rows `M`.
    pub m: u64,
    /// Contracted inner dimension `K`.
    pub k: u64,
    /// Right matrix columns `N`.
    pub n: u64,
    /// Expert / group count.
    pub groups: u64,
    /// Stored expert-weight layout.
    pub layout: WirePackedStorageLayout,
    /// Square tile edge.
    pub tile: u32,
    /// Workgroup lane count on `x`.
    pub workgroup_x: u32,
    /// Workgroup lane count on `y`.
    pub workgroup_y: u32,
}

/// M3 head-batched attention plan mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBatchedAttentionPlan {
    /// Materialized operation kind.
    pub kind: WireBatchedAttentionKind,
    /// Number of KV groups.
    pub kv_heads: u64,
    /// Query heads per KV group.
    pub q_per_kv: u64,
    /// Sequence extent.
    pub seq: u64,
    /// Per-head feature width.
    pub head_dim: u64,
    /// Hidden-state width.
    pub hidden: u64,
    /// Packed projection layout, when present.
    pub layout: Option<WirePackedStorageLayout>,
    /// Explicit `NeoX` rotate-half projection mode.
    pub rotate_half: bool,
    /// Row-major output strides.
    pub output_strides: [u64; 4],
    /// IEEE-754 score scale bits.
    pub scale_bits: u32,
    /// Typed decode (M=1) launch fact: outputs one workgroup computes;
    /// `None` keeps the generic one-thread-per-output launch law.
    pub decode_outputs_per_workgroup: Option<u64>,
}

/// Wire spelling of the M3 batched-attention operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireBatchedAttentionKind {
    ProjectionQ,
    ProjectionK,
    ProjectionV,
    Scores,
    Softmax,
    ContextSequenceMajor,
}

/// SSM conv1d plan (R-PACK-01C mechanical mirror of
/// `kernel_plan::SsmConv1dPlan`). Field-for-field; no extra facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSsmConv1dPlan {
    /// Sequence length.
    pub length: u64,
    /// Channel extent.
    pub channels: u64,
    /// Convolution kernel width.
    pub kernel_width: u64,
    /// Stored kernel/state layout.
    pub layout: WirePackedStorageLayout,
}

/// SSM linear-recurrence scan plan (R-PACK-01C mechanical mirror of
/// `kernel_plan::SsmScanPlan`). Field-for-field; no extra facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSsmScanPlan {
    /// Sequence length.
    pub length: u64,
    /// Recurrence state dimension.
    pub state_dim: u64,
    /// Stored state/weight layout.
    pub layout: WirePackedStorageLayout,
}

/// Lane-grid plan (gpu-reset C4-1): mirror of
/// `CollectionKernelPlan::LaneGrid`. Field-for-field; no extra facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireLaneGridPlan {
    /// Index of the resource whose call-site shape is the lane domain.
    pub source_resource: u64,
    /// Number of lane-domain axes.
    pub rank: u8,
}

/// Grid-reduce plan (gpu-reset C4-8): mirror of
/// `CollectionKernelPlan::GridReduce`, with the partial layout flattened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireGridReducePlan {
    /// Index of the resource holding the reduced tensor.
    pub source_resource: u64,
    /// Number of source axes.
    pub rank: u8,
    /// Workgroup lane count of both stages.
    pub workgroup: u32,
    /// Maximum partial slots in the dispatcher-owned scratch.
    pub partials_cap: u32,
    /// Byte width of one partial's value.
    pub partial_value_bytes: u32,
    /// `u32` coordinates carried after the value.
    pub partial_coordinates: u8,
    /// Whether a `u32` `seen` flag follows.
    pub partial_seen: bool,
}

/// Rank-2 transpose recipe (`[M,N] → [N,M]`), the wire mirror of
/// `kernel_plan::TransposePlan` (S5-U1): one thread per output element with
/// a column-major index remap, no shared memory, no barriers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireTransposePlan {
    /// Source rows `M` (output columns).
    pub m: u64,
    /// Source columns `N` (output rows).
    pub n: u64,
    /// Workgroup lane count on `x` (the elementwise-style 1D contract).
    pub workgroup_x: u32,
    /// Dispatch extent on `x` (`M·N` output elements).
    pub dispatch_x: u64,
}

/// Batched transpose recipe (U3 head-axis): `[batch, M, N] → [batch, N, M]`
/// under the same elementwise-style 1D contract (the batch folds into the
/// dispatch extent). The wire admits both codecs' lossless round-trip and
/// the decode-side admission validates the batch law fail-closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBatchedTransposePlan {
    /// Product of the leading batch axes (the head count).
    pub batch: u64,
    /// Source rows `M` per batch (output columns).
    pub m: u64,
    /// Source columns `N` per batch (output rows).
    pub n: u64,
    /// Workgroup lane count on `x` (the elementwise-style 1D contract).
    pub workgroup_x: u32,
    /// Dispatch extent on `x` (`batch·M·N` output elements).
    pub dispatch_x: u64,
}

/// Dense scalar layouts carried by the tiled-matmul wire plan. The dense
/// recipe admits only F32 and BF16 operands; tiles widen into F32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WireMatMulLayout {
    #[default]
    F32,
    Bf16,
}

/// Tiled-matmul recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireMatMulPlan {
    /// Left matrix rows `M`.
    pub m: u64,
    /// Contracted inner dimension `K`.
    pub k: u64,
    /// Right matrix columns `N`.
    pub n: u64,
    /// Left operand scalar layout; defaults to F32 for legacy images.
    #[serde(default)]
    pub left_layout: WireMatMulLayout,
    /// Right operand scalar layout; defaults to F32 for legacy images.
    #[serde(default)]
    pub right_layout: WireMatMulLayout,
    /// Square tile edge.
    pub tile: u32,
    /// Workgroup lane count on `x`.
    pub workgroup_x: u32,
    /// Workgroup lane count on `y`.
    pub workgroup_y: u32,
    /// The two shared-memory operand tiles.
    pub shared_memory: WireMatMulSharedMemory,
    /// Per-K-tile barrier schedule.
    pub barriers: Vec<WireBarrierPoint>,
    /// Zero-fill policy for the operand tail tiles.
    pub oob_padding: WireOobPaddingPolicy,
}

/// Batched tiled-matmul recipe (U1 head-axis). Field-for-field with
/// [`WireMatMulPlan`] plus the batch facts; the wire admits both codecs'
/// lossless round-trip and the decode-side admission validates the batch
/// law fail-closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBatchedMatMulPlan {
    /// Left matrix rows `M` (per batch).
    pub m: u64,
    /// Contracted inner dimension `K`.
    pub k: u64,
    /// Right matrix columns `N`.
    pub n: u64,
    /// Left operand scalar layout.
    pub left_layout: WireMatMulLayout,
    /// Right operand scalar layout.
    pub right_layout: WireMatMulLayout,
    /// Product of the receiver's leading batch axes (the head axes).
    pub batch: u64,
    /// Product of the argument's leading batch axes — a prefix share of
    /// `batch`; must divide `batch`.
    pub right_batch: u64,
    /// Square tile edge.
    pub tile: u32,
    /// Workgroup lane count on `x`.
    pub workgroup_x: u32,
    /// Workgroup lane count on `y`.
    pub workgroup_y: u32,
    /// The two shared-memory operand tiles.
    pub shared_memory: WireMatMulSharedMemory,
    /// Per-K-tile barrier schedule.
    pub barriers: Vec<WireBarrierPoint>,
    /// Zero-fill policy for the operand tail tiles.
    pub oob_padding: WireOobPaddingPolicy,
}

/// The two shared-memory operand tiles of the tiled-matmul recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireMatMulSharedMemory {
    /// Left-matrix tile.
    pub shared_a: WireSharedMemoryLayout,
    /// Right-matrix tile.
    pub shared_b: WireSharedMemoryLayout,
}

/// Tree-reduction recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireReductionPlan {
    /// Sum or mean (mean divides by the true tensor length).
    pub op: WireReduceOp,
    /// True flat tensor length `n`.
    pub length: u64,
    /// Power-of-two workgroup lane count.
    pub workgroup_x: u32,
    /// Per-workgroup partial slot count.
    pub partials: u64,
    /// Shared-memory buffer the tree reduces through.
    pub shared_memory: WireSharedMemoryLayout,
    /// Ordered barrier schedule.
    pub barriers: Vec<WireBarrierPoint>,
    /// Zero-fill policy for tail elements beyond `length`.
    pub oob_padding: WireOobPaddingPolicy,
}

/// One shared-memory buffer layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSharedMemoryLayout {
    /// Byte width of one element.
    pub element_byte_width: u32,
    /// Number of slots.
    pub slot_count: u32,
    /// Logical buffer name (backends map to target syntax).
    pub buffer_name: String,
}

/// A barrier program point in a recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBarrierPoint {
    /// The phase whose writes this barrier publishes.
    pub after: WireBarrierPhase,
}

/// What shared-memory writes a barrier publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireBarrierPhase {
    SharedMemoryLoad,
    ReductionStep,
    InnerProductStep,
}

/// Out-of-bounds padding policy for collection-op recipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireOobPaddingPolicy {
    ZeroFill,
}

/// Reduction combination operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireReduceOp {
    Sum,
    Mean,
    /// Maximum fold over the tensor elements (the `TensorMax` twin; the
    /// Metal tree-reduction emitter carries it — axis-reduction stays
    /// Sum-only under the CB-W8 allow-list).
    Max,
    /// Minimum fold over the tensor elements (the `TensorMin` twin).
    Min,
}

// ---------------------------------------------------------------------------
// Session/cadence wire surface (GI4-2)
// ---------------------------------------------------------------------------
//
// The versioned cadence/session wire revision (GI4-2; CTO S12; lane queue
// §2/§5): the GI4-1-frozen session facts ride the accepted wire version as a
// versioned session surface with its own ratchet + admission — the MD2-W1
// sibling-field precedent. `WIRE_DEVICE_PROGRAM_VERSION` and the outer
// `PACKAGE_MIR_ARTIFACT_VERSION = 6` stay untouched; the MD2 `distributed`
// section files are never co-edited. The wire types here mirror the GI4-1
// frozen public faber crate facts field-for-field (the `session.rs` / `kv_cache.rs`
// types): the per-invocation input cadence, the workload modes, the session
// observation cadence, and the typed `KvCacheLayout` carriage.

/// The per-invocation input cadence wire mirror (GI4-2; the GI4-1
/// `faber_runtime::session::InputUpdateCadence` freeze — which inputs
/// update per token versus stay resident across the session).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireInputUpdateCadence {
    /// Uploaded once at session creation; never re-copied per invocation
    /// (weights, KV K/V).
    Resident,
    /// Updated on every invocation (decode token id, the per-position `RoPE`
    /// table — a per-invocation updated input, never a plan change).
    PerInvocation,
}

/// The workload-mode wire mirror (GI4-2; the GI4-1
/// `faber_runtime::session::InvocationMode` freeze): one prefill step or
/// one scalar-decode step — the regime label for engine receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireInvocationMode {
    /// A prefill step (consumes the next prompt token; writes one KV
    /// generation per layer at the current position).
    Prefill,
    /// A one-token scalar decode step (consumes the fed-back token; writes
    /// the single new score row into KV).
    ScalarDecode,
}

impl WireInvocationMode {
    /// Stable diagnostic spelling (`"prefill"` / `"scalar_decode"`).
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Prefill => "prefill",
            Self::ScalarDecode => "scalar_decode",
        }
    }
}

/// The observation cadence of the session's declared observation (GI4-2;
/// lane queue §2) — WHEN the session surface reads its observation back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireSessionObservationCadence {
    /// Read back on every invocation (the per-token full-vocab logits
    /// readback of a decode loop, sampled host-side).
    PerInvocation,
    /// Read back once at session teardown.
    PerSession,
}

impl WireSessionObservationCadence {
    /// Stable diagnostic spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::PerInvocation => "per-invocation",
            Self::PerSession => "per-session",
        }
    }
}

/// The KV cache element dtype wire mirror (GI4-1 §3): the dtype is **closed**
/// — a dtype change is a contract revision, never a silent widening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireKvCacheDtype {
    /// IEEE-754 binary32 (4 bytes per element) — the declared-conversion
    /// representation for the GI4 row.
    F32,
}

impl WireKvCacheDtype {
    /// Stable diagnostic spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::F32 => "f32",
        }
    }
}

/// Version of the static KV-cache semantic contract carried by
/// [`WireKvCacheSemanticV2`]. This is the semantic payload version, distinct
/// from the enclosing session-section version.
pub const WIRE_KV_CACHE_SEMANTIC_CONTRACT_VERSION: u32 = 2;

/// The layout of cached V rows. K layout is determined by the post-RoPE rule
/// carried on [`WireKvCacheSemanticV2`]; this enum describes only V storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireKvCacheVLayout {
    /// V rows are stored in the transposed layout used by classic attention.
    Transposed,
    /// V rows are stored in the straight layout used by flash attention.
    Straight,
}

impl WireKvCacheVLayout {
    /// Stable diagnostic spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Transposed => "transposed",
            Self::Straight => "straight",
        }
    }
}

/// Attention algorithm family associated with the static cache semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireKvCacheAttentionFamily {
    /// The classic causal attention family.
    Classic,
    /// The flash attention family.
    Flash,
}

impl WireKvCacheAttentionFamily {
    /// Stable diagnostic spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Flash => "flash",
        }
    }
}

/// Capacity policy of the logical cache. Runtime cursor and allocation/view
/// state do not belong here; KV-B carries those facts separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireKvCacheCapacityPolicy {
    /// The cache has one immutable context capacity for its lifetime.
    Fixed,
}

impl WireKvCacheCapacityPolicy {
    /// Stable diagnostic spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
        }
    }
}

/// `RoPE` pairing convention carried by the semantic contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireKvCacheRopePairing {
    /// Consecutive pairs: `(x[0], x[1]), (x[2], x[3]), ...`.
    ConsecutivePair,
    /// Rotate-half pairs: the first half is paired with the second half.
    RotateHalf,
}

impl WireKvCacheRopePairing {
    /// Stable wire/diagnostic spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::ConsecutivePair => "consecutive_pair",
            Self::RotateHalf => "rotate_half",
        }
    }
}

/// Version-2 static KV-cache semantic record.
///
/// This is deliberately only the model/cache meaning shared by Gradus and
/// Radix. It carries distinct K/V dtype facts, dimensions, layout and
/// attention choices, fixed-capacity semantics, and typed `RoPE` facts. It does
/// **not** carry a runtime cursor, buffer identity, allocation or view, or
/// backend binding; those facts belong to KV-B's executor-plan surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireKvCacheSemanticV2 {
    /// Semantic contract version, independent of the enclosing section version.
    pub semantic_contract_version: u32,
    /// Stored K element dtype. Kept separate from [`Self::kv_dtype_v`].
    pub kv_dtype_k: WireKvCacheDtype,
    /// Stored V element dtype. Kept separate from [`Self::kv_dtype_k`].
    pub kv_dtype_v: WireKvCacheDtype,
    /// Static layout of cached V rows.
    pub v_layout: WireKvCacheVLayout,
    /// Static attention family consuming the cache.
    pub attention_family: WireKvCacheAttentionFamily,
    /// Number of query heads.
    pub query_head_count: u32,
    /// Number of stored KV heads.
    pub kv_head_count: u32,
    /// Width of one attention head.
    pub head_dim: u32,
    /// Number of transformer layers represented by the cache.
    pub layer_count: u32,
    /// Immutable logical context capacity.
    pub context_capacity: u32,
    /// Capacity policy for the logical cache.
    pub capacity_policy: WireKvCacheCapacityPolicy,
    /// Number of dimensions rotated by `RoPE`.
    pub rope_dimension: u32,
    /// `RoPE` frequency base (theta).
    pub rope_frequency_base: f32,
    /// `RoPE` position/frequency scale multiplier.
    pub rope_frequency_scale: f32,
    /// Typed `RoPE` pairing convention.
    pub rope_pairing: WireKvCacheRopePairing,
    /// The semantic rule that cached K is stored after `RoPE`.
    pub k_stored_post_rope: bool,
}

/// The KV reserve policy wire mirror (GI4-1 §3): the declared headroom on
/// top of the deterministic storage figure — a separate fact, never folded
/// silently into storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireKvReservePolicy {
    /// The reserved bytes, on top of the deterministic storage figure.
    pub bytes: u64,
}

/// The typed KV-cache layout carriage (GI4-2; GI4-1 §3 / lane queue §2): the
/// five layout facts (`slots`, `context_length`, `layer_count`,
/// `kv_head_count`, `head_dim`) plus `dtype` and `reserve_policy`. The
/// storage byte figure is the layout's own authority (consumed, never
/// re-derived from dims — `hosts/crates/host-coordinator/src/partition.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireKvCacheLayout {
    /// The sequence-slot count (GI4 begins with 1).
    pub slots: u32,
    /// The maximum context length per slot (the pinned row: ctx 8192).
    pub context_length: u32,
    /// The transformer layer count (the pinned row: 32).
    pub layer_count: u32,
    /// The KV head count per layer (the pinned row: 5, GQA 15/5).
    pub kv_head_count: u32,
    /// The head dimension (the pinned row: 64).
    pub head_dim: u32,
    /// The element dtype (closed to `f32` for GI4).
    pub dtype: WireKvCacheDtype,
    /// The declared reserve policy.
    pub reserve_policy: WireKvReservePolicy,
}

/// One declared session/host-input slot (GI4-2; lane queue §2 `HostInput{slot}`):
/// the program buffer slot the session inputs at, plus its per-invocation
/// input cadence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSessionInput {
    /// The program buffer slot this input targets (the wire buffer id).
    pub slot: u32,
    /// The per-invocation input cadence of this slot.
    pub cadence: WireInputUpdateCadence,
}

#[cfg(test)]
#[path = "wire_gea3_test.rs"]
mod gea3_wire_tests;
