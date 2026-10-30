//! Backend-neutral launch descriptors and executor-plan extensions.

use crate::abi::{MirKernelResourceAccess, PackedStorageLayout};
use crate::device_plan_compiler::LibraryOperation;
use crate::device_program::{
    Binding, BufferRole, DeviceProgram, DeviceProgramError, DeviceResource, KernelUnit,
};
use crate::kernel_plan::{BatchedAttentionKind, CollectionKernelPlan, SharedMemoryLayout};
use crate::layout::MirTensorStorageLayout;
use crate::{MirKernelDispatchSize, MirKernelWorkgroupCount, MirType, MirWorkgroupSize};
use std::fmt;

// ---------------------------------------------------------------------------
// RECIPE FAMILIES
// ---------------------------------------------------------------------------

/// The recipe family of a kernel, as a typed tag.
///
/// Backends and hosts switch on this tag to select launch semantics (grid
/// coverage, shared-memory presence); it is derived from the typed plan,
/// never from emitted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeKind {
    /// No collection-op recipe (elementwise kernel).
    Elementwise,
    /// Tiled-matmul recipe.
    TiledMatMul,
    /// Tree-reduction recipe.
    TreeReduction,
    /// Rank-2 transpose recipe (S5-U1).
    Transpose,
    /// Axis-reduction recipe (S6-P1): row-wise reduction over the declared
    /// axis (the transformer `TensorSumAxis` op).
    AxisReduction,
    /// Row-wise softmax recipe (S6-P1).
    RowSoftmax,
    /// Layer-normalization recipe (S6-P1).
    LayerNormalization,
    /// Row-gather recipe (GI3-1).
    Gather,
    /// Root-mean-square-normalization recipe (GI3-1).
    RmsNormalization,
    /// Rotary-position-embedding recipe (GI3-1).
    Rope,
    /// Causal masked-row softmax recipe (GI3-1).
    CausalMaskedSoftmax,
    /// Packed quantized GEMM recipe (R-PACK-01A).
    QuantizedMatMul,
    /// Grouped packed GEMM recipe (R-PACK-01A).
    GroupedMatMul,
    /// M3 head-batched attention recipe.
    BatchedAttention,
    /// SSM conv1d recipe (R-PACK-01A).
    SsmConv1d,
    /// SSM linear-recurrence scan recipe (R-PACK-01A).
    SsmScan,
    /// Lane-form kernel (gpu-reset C4): one thread per lane of the domain.
    LaneGrid,
    /// Single-lane kernel (gpu-reset A2 rung 3): one thread.
    SingleLane,
    /// Whole-grid reduction (gpu-reset C4-8): two launches through a
    /// dispatcher-owned partials scratch.
    GridReduce,
    /// Ordered three-stage prefix collective.
    Scan,
}

impl RecipeKind {
    pub(super) fn from_plan(plan: &CollectionKernelPlan) -> Self {
        match plan {
            CollectionKernelPlan::Elementwise => Self::Elementwise,
            CollectionKernelPlan::TiledMatMul(_) => Self::TiledMatMul,
            // SFR-5: the declared distributed sum's floor IS the tree recipe —
            // a backend with no native collective arm selects the tree-reduction
            // launch semantics here; native arms (SFR-6/7) match the plan variant
            // before consulting this tag.
            CollectionKernelPlan::TreeReduction(_) | CollectionKernelPlan::DistributedSum(_) => {
                Self::TreeReduction
            }
            CollectionKernelPlan::Transpose(_) => Self::Transpose,
            CollectionKernelPlan::AxisReduction(_) => Self::AxisReduction,
            CollectionKernelPlan::RowSoftmax(_) => Self::RowSoftmax,
            CollectionKernelPlan::LayerNormalization(_) => Self::LayerNormalization,
            CollectionKernelPlan::Gather(_) => Self::Gather,
            CollectionKernelPlan::RmsNormalization(_) => Self::RmsNormalization,
            CollectionKernelPlan::Rope(_) => Self::Rope,
            CollectionKernelPlan::CausalMaskedSoftmax(_) => Self::CausalMaskedSoftmax,
            CollectionKernelPlan::QuantizedMatMul(_) => Self::QuantizedMatMul,
            CollectionKernelPlan::GroupedMatMul(_) => Self::GroupedMatMul,
            CollectionKernelPlan::BatchedAttention(_) => Self::BatchedAttention,
            CollectionKernelPlan::SsmConv1d(_) => Self::SsmConv1d,
            CollectionKernelPlan::SsmScan(_) => Self::SsmScan,
            CollectionKernelPlan::LaneGrid { .. } => Self::LaneGrid,
            CollectionKernelPlan::SingleLane => Self::SingleLane,
            CollectionKernelPlan::GridReduce { .. } => Self::GridReduce,
            CollectionKernelPlan::Scan { .. } => Self::Scan,
        }
    }
}

// ---------------------------------------------------------------------------
// STATIC EXECUTOR-PLAN BINDS
// ---------------------------------------------------------------------------

/// Version of the additive executor-plan bind extension frozen by M0.
///
/// This extension carries library selection facts beside the existing
/// `DeviceProgram` shape. It is intentionally not a WIRE-v8 schema change:
/// callers that do not consume the extension continue to use the generated
/// kernel entry and its existing resource/launch facts.
pub const EXECUTOR_PLAN_BIND_ABI_VERSION: u16 = 1;

/// Uniform name for the packed GGML type id (M5-U2 / M6-U1 seam).
pub const PACKED_LAYOUT_GGML_TYPE_ID_UNIFORM: &str = "packed_layout_ggml_type_id";
/// Uniform name for packed block bytes.
pub const PACKED_BLOCK_BYTES_UNIFORM: &str = "packed_block_bytes";
/// Uniform name for packed block element count.
pub const PACKED_BLOCK_ELEMENTS_UNIFORM: &str = "packed_block_elements";
/// Uniform name for the native packed-region byte length.
pub const PACKED_BYTES_UNIFORM: &str = "packed_bytes";
/// Uniform name for decode GEMV selection (`1` when THIS step's row count M is 1).
pub const DECODE_GEMV_UNIFORM: &str = "decode_gemv";
/// Library entry for a packed GEMM body (M ≠ 1).
pub const QUANTIZED_GEMM_ENTRY: &str = "quantized_gemm";
/// Library entry for a packed GEMV body (M = 1 decode).
pub const QUANTIZED_GEMV_ENTRY: &str = "quantized_gemv";

/// CTO ruling `f083c057`: batched is the numeric baseline for decode
/// projections; M=1 GEMV selection stays off until PB-3 root-causes the
/// numeric divergence and proves identity. The GEMV bodies remain in the
/// tree; re-enabling is this one flag.
pub const DECODE_GEMV_SELECTION_ENABLED: bool = false;

/// Target-neutral layout family for a library body bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindLayout {
    /// A flat one-dimensional body.
    Flat,
    /// Contiguous row-major storage.
    RowMajor,
    /// A non-contiguous view described by the carried strides.
    Strided,
    /// `RoPE` consecutive-pair layout.
    RopeConsecutivePair { rotated_width: u64 },
    /// `RoPE` rotate-half layout.
    RopeRotateHalf { rotated_width: u64 },
}

/// Dimensions, strides, layout, and grid bound to one library body.
///
/// The descriptor is a typed executor-plan fact. Backends may lower it to
/// uniforms or native launch metadata, but they must not infer it from an
/// emitted kernel body or from a physical buffer length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindDescriptor {
    /// Logical dimensions in outer-to-inner order.
    pub dims: Vec<u64>,
    /// Physical element strides in the same order as [`Self::dims`].
    pub strides: Vec<u64>,
    /// Logical interpretation of the dimensions and strides.
    pub layout: BindLayout,
    /// Backend-neutral workgroup grid.
    pub grid: MirKernelWorkgroupCount,
}

impl BindDescriptor {
    fn row_major(dims: Vec<u64>, grid: MirKernelWorkgroupCount, layout: BindLayout) -> Self {
        let mut strides = vec![1u64; dims.len()];
        for axis in (0..dims.len()).rev().skip(1) {
            strides[axis] = strides[axis + 1].saturating_mul(dims[axis + 1]);
        }
        Self {
            dims,
            strides,
            layout,
            grid,
        }
    }

    /// Whether this descriptor has structurally coherent rank and extents.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        !self.dims.is_empty()
            && self.dims.len() == self.strides.len()
            && self.dims.iter().all(|dim| *dim > 0)
            && self.strides.iter().all(|stride| *stride > 0)
            && self.grid.x > 0
            && self.grid.y > 0
            && self.grid.z > 0
    }
}

/// Versioned scalar types accepted by library-call uniforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UniformType {
    /// IEEE-754 single precision value.
    F32,
    /// IEEE-754 half precision value, carried as its bit pattern.
    F16,
    /// Unsigned 32-bit integer.
    U32,
    /// Signed 32-bit integer.
    I32,
}

/// One constant or runtime uniform supplied to a library body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniformBinding {
    /// Stable logical uniform name.
    pub name: String,
    /// Uniform scalar type.
    pub ty: UniformType,
    /// Raw little-endian bits (4 bytes for all current uniform types).
    pub bits: u32,
}

/// One offset/stride fact for a bound view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetBinding {
    /// Logical view name (for example `q`, `k`, `v`, or `output`).
    pub name: String,
    /// Element offset from the base allocation.
    pub offset_elements: u64,
    /// Physical element strides in logical axis order.
    pub strides: Vec<u64>,
}

/// One typed buffer bind supplied to a library body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryBind {
    /// Stable logical view name.
    pub name: String,
    /// Device-program binding slot.
    pub binding: Binding,
    /// Logical dimensions of the view.
    pub dims: Vec<u64>,
    /// Physical element strides of the view.
    pub strides: Vec<u64>,
    /// Base element offset in the allocation.
    pub offset_elements: u64,
}

/// Optional scratch/workspace allocation required by a library body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceBinding {
    /// Required byte size.
    pub bytes: u64,
    /// Required byte alignment.
    pub alignment: u32,
}

/// A backend capability required by a selected library body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendRequirement {
    /// Backend identity, such as `metal`.
    pub backend: String,
    /// Capability or feature name.
    pub feature: String,
}

/// Provenance for a plan compiler selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanProvenance {
    /// Name of the pass that made the selection.
    pub pass: String,
    /// Revision of that pass's typed recognition rules.
    pub revision: u32,
    /// Canonical graph node identities consumed by the selection.
    pub source_nodes: Vec<u32>,
}

/// The logical signature selected independently of emitted kernel syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalSignature {
    /// Stable library operation name.
    pub operation: String,
    /// Logical input view names.
    pub inputs: Vec<String>,
    /// Logical output view names.
    pub outputs: Vec<String>,
}

/// One optional library selection and its launch bind facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySelection {
    /// Index into [`DeviceProgram`]`::kernels` or the compiled schedule.
    pub kernel_index: usize,
    /// The logical body selected by the executor. `None` retains the
    /// generated-kernel fallback for a recipe not yet owned by library v0.
    pub library_entry: Option<String>,
    /// The logical signature of the selected body.
    pub logical_signature: LogicalSignature,
    /// Primary shape/layout/grid facts supplied to the selected body.
    pub bind: BindDescriptor,
    /// All named view binds. `bind` is retained as the primary launch view.
    pub binds: Vec<LibraryBind>,
    /// Constant/runtime scalar uniforms.
    pub uniforms: Vec<UniformBinding>,
    /// Explicit base offsets and physical strides.
    pub offsets: Vec<OffsetBinding>,
    /// Optional scratch allocation.
    pub workspace: Option<WorkspaceBinding>,
    /// Backend requirements that must be admitted before dispatch.
    pub backend_requirements: Vec<BackendRequirement>,
    /// Compiler provenance for audit and cache identity.
    pub provenance: PlanProvenance,
}

/// M0-frozen additive executor-plan section for library calls. This section is
/// versioned independently of WIRE v8; an unknown version fails closed before
/// any selection field is interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorPlanExtension {
    /// Section schema version.
    pub abi_version: u16,
    /// Ordered selection/bind records, one per declared kernel.
    pub selections: Vec<LibrarySelection>,
}

impl ExecutorPlanExtension {
    /// Validate the section against the program kernel count before a consumer
    /// interprets any library selection.
    ///
    /// # Errors
    /// Returns a `String` detail when the section disagrees with the kernel count.
    pub fn validate_for_kernel_count(&self, kernel_count: usize) -> Result<(), String> {
        if self.abi_version != EXECUTOR_PLAN_BIND_ABI_VERSION {
            return Err(format!(
                "executor-plan section version {} is not supported (expected {})",
                self.abi_version, EXECUTOR_PLAN_BIND_ABI_VERSION
            ));
        }
        if self.selections.len() != kernel_count {
            return Err(format!(
                "executor-plan selection count {} does not match kernel count {}",
                self.selections.len(),
                kernel_count
            ));
        }
        for (position, selection) in self.selections.iter().enumerate() {
            if selection.kernel_index != position {
                return Err(format!(
                    "executor-plan kernel indices are not the ordered unique range at position {position}"
                ));
            }
            if !selection.bind.is_well_formed() {
                return Err(format!(
                    "executor-plan selection {position} has an invalid bind descriptor"
                ));
            }
            if selection.logical_signature.operation.is_empty()
                || selection.provenance.pass.is_empty()
            {
                return Err(format!(
                    "executor-plan selection {position} has incomplete signature/provenance"
                ));
            }
        }
        Ok(())
    }
}

/// Version of the executor-plan section used when launch bindings carry
/// runtime state. Static plans deliberately retain [`EXECUTOR_PLAN_BIND_ABI_VERSION`]
/// and the v1 shape above; dynamic plans use this version and the additive
/// allocation/view/binding facts below.
pub const EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION: u16 = 2;

// ---------------------------------------------------------------------------
// SPECULATIVE VERIFICATION
// ---------------------------------------------------------------------------

/// Version of the additive speculative-verification record section.
///
/// The runtime-binding v2 payload remains wire-compatible for existing
/// consumers. Verification records are a new section at the next version so
/// an older consumer cannot default-fill `L`, `k`, generation, or result
/// fields and accidentally admit a different invocation.
pub const EXECUTOR_PLAN_VERIFICATION_ABI_VERSION: u16 = EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION + 1;

/// The layout of the ordered candidate-logits result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerificationLogitsLayout {
    /// Consecutive rows, each containing the complete vocabulary in order.
    RowMajor,
    /// A layout that is not the ordered row-major result contract.
    ColumnMajor,
}

impl VerificationLogitsLayout {
    /// Stable spelling used by diagnostics and identity canonicalization.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::RowMajor => "row_major",
            Self::ColumnMajor => "column_major",
        }
    }
}

/// Static model/output facts for one verification plan.
///
/// `prefix_before`, `query_rows`, generation, candidate ids, and transaction
/// identity deliberately do not appear here. They are invocation facts and
/// must not change the compiled-plan identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationPlanShape {
    /// The model's stable logical dimensions, excluding invocation extents.
    pub model_shape: Vec<u64>,
    /// Vocabulary width of each ordered logits row.
    pub vocab: u64,
    /// Result layout admitted by the plan.
    pub logits_layout: VerificationLogitsLayout,
}

impl VerificationPlanShape {
    /// Validate the static shape facts before they enter a plan identity.
    ///
    /// # Errors
    /// Returns `VerificationAdmissionError` when a shape fact is inadmissible.
    pub fn validate(&self) -> Result<(), VerificationAdmissionError> {
        if self.model_shape.is_empty() || self.model_shape.contains(&0) {
            return Err(VerificationAdmissionError::InvalidPlanShape {
                detail: "model shape must have a positive rank and dimensions".to_owned(),
            });
        }
        if self.vocab == 0 {
            return Err(VerificationAdmissionError::InvalidPlanShape {
                detail: "vocabulary width must be positive".to_owned(),
            });
        }
        Ok(())
    }

    /// Return the stable identity of this static plan shape.
    #[must_use]
    pub fn static_identity(&self) -> VerificationPlanIdentity {
        let mut bytes = Vec::new();
        push_verification_identity_bytes(&mut bytes, b"radix-mir:verification-plan:v1");
        push_verification_identity_u64(
            &mut bytes,
            u64::from(EXECUTOR_PLAN_VERIFICATION_ABI_VERSION),
        );
        push_verification_identity_u64(
            &mut bytes,
            u64::try_from(self.model_shape.len()).unwrap_or(u64::MAX),
        );
        for dimension in &self.model_shape {
            push_verification_identity_u64(&mut bytes, *dimension);
        }
        push_verification_identity_u64(&mut bytes, self.vocab);
        push_verification_identity_bytes(&mut bytes, self.logits_layout.spelling().as_bytes());
        VerificationPlanIdentity(format!("sha256:{}", handroll::sha256_hex(bytes)))
    }
}

/// Stable identity for static verification-plan facts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VerificationPlanIdentity(String);

impl VerificationPlanIdentity {
    /// Canonical `sha256:<64-hex>` identity spelling.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.0
    }
}

/// Target-neutral admission record for one speculative verification block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationBlockAdmission {
    /// Version of this additive verification record section.
    pub abi_version: u16,
    /// Committed prefix length `L` before this invocation.
    pub prefix_before: u64,
    /// Explicit candidate/query row count `k`; never inferred from a sequence.
    pub query_rows: u64,
    /// Candidate token ids, in the order represented by the result rows.
    pub candidate_ids: Vec<i64>,
    /// Generation of the committed cache state being read.
    pub committed_generation: Option<u64>,
    /// Identity of the transaction that owns the uncommitted candidate rows.
    pub transaction_id: Option<u64>,
    /// Static model and result-shape facts.
    pub plan: VerificationPlanShape,
}

/// Target-neutral result record returned for an admitted verification block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationBlockResult {
    /// Transaction that produced these candidate rows and logits.
    pub transaction_id: Option<u64>,
    /// Generation of the committed cache state used for the forward pass.
    pub committed_generation: Option<u64>,
    /// Ordered logits shape, expected to be exactly `[k, vocab]`.
    pub logits_shape: Vec<u64>,
    /// Physical/logical layout of the logits rows.
    pub logits_layout: VerificationLogitsLayout,
    /// Number of candidate rows accepted by the caller, `0 ≤ r ≤ k`.
    pub accepted_length: i64,
}

/// Focused fail-closed errors at the speculative-verification seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationAdmissionError {
    /// A static model/output shape is incomplete or malformed.
    InvalidPlanShape { detail: String },
    /// A prefix of zero would masquerade as fresh prefill.
    FreshPrefillMasquerade,
    /// The explicit candidate/query row count is zero.
    InvalidQueryRows,
    /// Candidate ids do not have the explicitly declared row count.
    CandidateCountMismatch { declared: u64, actual: usize },
    /// A candidate id is negative or outside the declared vocabulary.
    InvalidCandidateId { index: usize, candidate_id: i64 },
    /// The committed cache generation was omitted.
    MissingGeneration,
    /// The transaction identity was omitted.
    MissingTransactionIdentity,
    /// A result belongs to a different committed cache generation.
    StaleGeneration { expected: u64, actual: u64 },
    /// A result belongs to a different verification transaction.
    TransactionIdentityMismatch { expected: u64, actual: u64 },
    /// The result shape is not the ordered `[k, vocab]` shape, including a
    /// candidate-id/logit-row count mismatch.
    LogitsShapeSkew {
        expected: Vec<u64>,
        actual: Vec<u64>,
    },
    /// The result layout does not match the admitted plan layout.
    LogitsLayoutSkew {
        expected: VerificationLogitsLayout,
        actual: VerificationLogitsLayout,
    },
    /// The caller supplied an impossible accepted length.
    InvalidAcceptedLength { accepted: i64, maximum: u64 },
    /// A section consumer received a stale or unknown record version.
    UnsupportedVersion { actual: u16, expected: u16 },
}

impl fmt::Display for VerificationAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPlanShape { detail } => {
                write!(formatter, "invalid verification plan shape: {detail}")
            }
            Self::FreshPrefillMasquerade => {
                write!(formatter, "verification prefix_before must be nonzero")
            }
            Self::InvalidQueryRows => {
                write!(formatter, "verification query_rows k must be nonzero")
            }
            Self::CandidateCountMismatch { declared, actual } => write!(
                formatter,
                "verification candidate count {actual} does not match explicit k={declared}"
            ),
            Self::InvalidCandidateId {
                index,
                candidate_id,
            } => write!(
                formatter,
                "verification candidate id {candidate_id} at index {index} is outside the admitted vocabulary"
            ),
            Self::MissingGeneration => {
                write!(formatter, "verification committed generation is missing")
            }
            Self::MissingTransactionIdentity => {
                write!(formatter, "verification transaction identity is missing")
            }
            Self::StaleGeneration { expected, actual } => write!(
                formatter,
                "verification result generation {actual} is stale; expected {expected}"
            ),
            Self::TransactionIdentityMismatch { expected, actual } => write!(
                formatter,
                "verification result transaction {actual} does not match admitted transaction {expected}"
            ),
            Self::LogitsShapeSkew { expected, actual } => write!(
                formatter,
                "verification logits shape {actual:?} does not match ordered {expected:?}"
            ),
            Self::LogitsLayoutSkew { expected, actual } => write!(
                formatter,
                "verification logits layout {} does not match {}",
                actual.spelling(),
                expected.spelling()
            ),
            Self::InvalidAcceptedLength { accepted, maximum } => write!(
                formatter,
                "verification accepted length r={accepted} is outside 0..={maximum}"
            ),
            Self::UnsupportedVersion { actual, expected } => write!(
                formatter,
                "unsupported verification record version {actual}; expected {expected}"
            ),
        }
    }
}

impl std::error::Error for VerificationAdmissionError {}

impl VerificationBlockAdmission {
    /// Validate and admit the invocation facts without inferring `k` from any
    /// sequence or candidate vector.
    ///
    /// # Errors
    /// Returns `VerificationAdmissionError` when the invocation facts are inconsistent.
    pub fn admit(self) -> Result<Self, VerificationAdmissionError> {
        self.validate()?;
        Ok(self)
    }

    /// Validate this invocation record.
    ///
    /// # Errors
    /// Returns `VerificationAdmissionError` when an invocation fact is inadmissible.
    pub fn validate(&self) -> Result<(), VerificationAdmissionError> {
        admit_executor_plan_verification_version(self.abi_version)?;
        self.plan.validate()?;
        if self.prefix_before == 0 {
            return Err(VerificationAdmissionError::FreshPrefillMasquerade);
        }
        if self.query_rows == 0 {
            return Err(VerificationAdmissionError::InvalidQueryRows);
        }
        if self.candidate_ids.len() != usize::try_from(self.query_rows).unwrap_or(usize::MAX) {
            return Err(VerificationAdmissionError::CandidateCountMismatch {
                declared: self.query_rows,
                actual: self.candidate_ids.len(),
            });
        }
        for (index, candidate_id) in self.candidate_ids.iter().copied().enumerate() {
            let in_vocab = u64::try_from(candidate_id)
                .is_ok_and(|candidate_id| candidate_id < self.plan.vocab);
            if !in_vocab {
                return Err(VerificationAdmissionError::InvalidCandidateId {
                    index,
                    candidate_id,
                });
            }
        }
        if self.committed_generation.is_none() {
            return Err(VerificationAdmissionError::MissingGeneration);
        }
        if self.transaction_id.is_none() {
            return Err(VerificationAdmissionError::MissingTransactionIdentity);
        }
        Ok(())
    }

    /// The static identity excludes `L`, `k`, generation, candidates, and the
    /// transaction so equivalent model shapes share one compiled plan.
    #[must_use]
    pub fn static_plan_identity(&self) -> VerificationPlanIdentity {
        self.plan.static_identity()
    }

    /// Admit a result against this invocation's generation, transaction, row
    /// shape, layout, and accepted-length contract.
    ///
    /// # Errors
    /// Returns `VerificationAdmissionError` when the result violates the admitted contract.
    pub fn admit_result(
        &self,
        result: VerificationBlockResult,
    ) -> Result<VerificationBlockResult, VerificationAdmissionError> {
        self.validate()?;
        result.validate_against(self)?;
        Ok(result)
    }
}

impl VerificationBlockResult {
    /// Validate this result against its admitted verification block.
    ///
    /// # Errors
    /// Returns `VerificationAdmissionError` when the result does not match its admission.
    pub fn validate_against(
        &self,
        admission: &VerificationBlockAdmission,
    ) -> Result<(), VerificationAdmissionError> {
        let expected_generation = admission
            .committed_generation
            .ok_or(VerificationAdmissionError::MissingGeneration)?;
        let actual_generation = self
            .committed_generation
            .ok_or(VerificationAdmissionError::MissingGeneration)?;
        if actual_generation != expected_generation {
            return Err(VerificationAdmissionError::StaleGeneration {
                expected: expected_generation,
                actual: actual_generation,
            });
        }
        let expected_transaction = admission
            .transaction_id
            .ok_or(VerificationAdmissionError::MissingTransactionIdentity)?;
        let actual_transaction = self
            .transaction_id
            .ok_or(VerificationAdmissionError::MissingTransactionIdentity)?;
        if actual_transaction != expected_transaction {
            return Err(VerificationAdmissionError::TransactionIdentityMismatch {
                expected: expected_transaction,
                actual: actual_transaction,
            });
        }
        let expected_shape = vec![admission.query_rows, admission.plan.vocab];
        if self.logits_shape != expected_shape {
            return Err(VerificationAdmissionError::LogitsShapeSkew {
                expected: expected_shape,
                actual: self.logits_shape.clone(),
            });
        }
        if self.logits_layout != admission.plan.logits_layout {
            return Err(VerificationAdmissionError::LogitsLayoutSkew {
                expected: admission.plan.logits_layout,
                actual: self.logits_layout,
            });
        }
        if self.accepted_length < 0
            || u64::try_from(self.accepted_length)
                .map_or(true, |accepted| accepted > admission.query_rows)
        {
            return Err(VerificationAdmissionError::InvalidAcceptedLength {
                accepted: self.accepted_length,
                maximum: admission.query_rows,
            });
        }
        Ok(())
    }
}

/// Admit a verification section version before interpreting its records.
///
/// # Errors
/// Returns `VerificationAdmissionError` when the version is unsupported.
pub fn admit_executor_plan_verification_version(
    version: u16,
) -> Result<(), VerificationAdmissionError> {
    if version != EXECUTOR_PLAN_VERIFICATION_ABI_VERSION {
        return Err(VerificationAdmissionError::UnsupportedVersion {
            actual: version,
            expected: EXECUTOR_PLAN_VERIFICATION_ABI_VERSION,
        });
    }
    Ok(())
}

fn push_verification_identity_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    output.extend_from_slice(bytes);
}

fn push_verification_identity_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

// ---------------------------------------------------------------------------
// RUNTIME ALLOCATION AND VIEW BINDINGS
// ---------------------------------------------------------------------------

/// The scalar data type of an allocation or typed source.
///
/// This is an alias of the existing uniform type vocabulary so the producer
/// and the v1 constant surface cannot disagree about byte widths. The KV
/// campaign's integrated route uses `F32`; `F16` remains an explicit type,
/// not an implicit narrowing.
pub type AllocationDType = UniformType;
/// Short alias used by consumers that call the field simply `dtype`.
pub type DType = AllocationDType;

impl UniformType {
    /// Number of bytes occupied by one scalar of this type.
    #[must_use]
    pub const fn byte_width(self) -> u64 {
        match self {
            Self::F16 => 2,
            Self::F32 | Self::U32 | Self::I32 => 4,
        }
    }
}

/// The typed invocation-state fields admitted as runtime binding sources.
///
/// These are tags, not string sentinels or magic uniform names. Every field is
/// carried as a `u32` by the invocation-state buffer and can therefore be
/// shared by append and attention launches without changing static plan
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeSource {
    /// Absolute write position / current prefix position.
    Position,
    /// Prefix length after the current append.
    ValidLenAfter,
    /// Number of query rows in this invocation.
    QueryRows,
    /// Monotonic sequence/reset epoch.
    SequenceEpoch,
}

impl RuntimeSource {
    /// The scalar type carried by every invocation-state field.
    #[must_use]
    pub const fn ty(self) -> UniformType {
        UniformType::U32
    }

    /// Stable field spelling for diagnostics and wire mirrors.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Position => "position",
            Self::ValidLenAfter => "valid_len_after",
            Self::QueryRows => "query_rows",
            Self::SequenceEpoch => "sequence_epoch",
        }
    }
}

/// A tagged source for a v2 launch binding.
///
/// The enum tag is the source contract: a constant is a typed bit pattern and
/// a runtime value is one of the closed invocation-state fields. A consumer
/// must never reinterpret a v1 constant as a runtime cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutorPlanSource {
    /// A compile-time scalar value.
    Constant { ty: UniformType, bits: u32 },
    /// A value read from the typed invocation-state buffer.
    Runtime(RuntimeSource),
}

impl ExecutorPlanSource {
    /// Construct a typed constant source.
    #[must_use]
    pub const fn constant(ty: UniformType, bits: u32) -> Self {
        Self::Constant { ty, bits }
    }

    /// Construct a typed runtime source.
    #[must_use]
    pub const fn runtime(source: RuntimeSource) -> Self {
        Self::Runtime(source)
    }

    /// The scalar type consumed by this source.
    #[must_use]
    pub const fn ty(self) -> UniformType {
        match self {
            Self::Constant { ty, .. } => ty,
            Self::Runtime(source) => source.ty(),
        }
    }

    /// Whether this source is dynamic.
    #[must_use]
    pub const fn is_runtime(self) -> bool {
        matches!(self, Self::Runtime(_))
    }
}

/// Compatibility spelling for the shared field table.
pub type TypedSource = ExecutorPlanSource;
/// Compatibility spelling for consumers that name this a plan source.
pub type PlanSource = ExecutorPlanSource;

/// The independent initialization fact of an allocation.
pub use crate::device_semantics::InitializationPolicy;

/// Typed fixed-capacity storage facts for one allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationFact {
    /// Stable program-level buffer identity.
    pub buffer: crate::device_program::BufferIdentity,
    /// Element data type of the allocation.
    pub dtype: AllocationDType,
    /// Physical allocation capacity in bytes, distinct from every view span.
    pub capacity_bytes: u64,
    /// Allocation lifetime, repeated explicitly for the v2 ABI contract.
    pub lifetime: crate::device_program::BufferLifetime,
    /// How the allocation reaches its first defined state.
    pub initialization: InitializationPolicy,
}

/// A bounded logical view over an allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewFact {
    /// Allocation identity shared by this view and any sibling views.
    pub allocation: crate::device_program::BufferId,
    /// Logical dimensions in outer-to-inner order.
    pub logical_dims: Vec<u64>,
    /// Element strides in the same order as [`Self::logical_dims`].
    pub strides: Vec<u64>,
    /// Static base element offset in the allocation.
    pub static_base: u64,
    /// Maximum byte span from the static base, not allocation capacity.
    pub maximum_span: u64,
}

/// Compatibility spelling for the shared field table.
pub type AllocationView = ViewFact;
/// Compatibility spelling for consumers that call this a view fact.
pub type View = ViewFact;

/// One v2 launch binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchBinding {
    /// Kernel selection receiving this binding.
    pub kernel_index: usize,
    /// Allocation handle/identity bound by the launch.
    pub handle: crate::device_program::BufferId,
    /// Target-neutral binding index.
    pub binding: crate::device_program::Binding,
    /// Byte offset from the allocation base.
    pub byte_offset: u64,
    /// Byte span exposed to this launch.
    pub view_span: u64,
    /// Typed constant or invocation-state source for the binding.
    pub runtime_source: ExecutorPlanSource,
}

impl LaunchBinding {
    /// Construct a launch binding while keeping the field table explicit.
    #[must_use]
    pub const fn new(
        kernel_index: usize,
        handle: crate::device_program::BufferId,
        binding: crate::device_program::Binding,
        byte_offset: u64,
        view_span: u64,
        runtime_source: ExecutorPlanSource,
    ) -> Self {
        Self {
            kernel_index,
            handle,
            binding,
            byte_offset,
            view_span,
            runtime_source,
        }
    }
}

/// All dynamic facts supplied to the v2 producer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutorPlanV2Facts {
    /// Fixed-capacity physical allocations.
    pub allocations: Vec<AllocationFact>,
    /// Logical bounded views over those allocations.
    pub views: Vec<ViewFact>,
    /// Per-kernel launch bindings.
    pub launch_bindings: Vec<LaunchBinding>,
}

/// Compatibility spelling for the producer input.
pub type RuntimeBindingFacts = ExecutorPlanV2Facts;

/// A typed v2 executor-plan section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorPlanExtensionV2 {
    /// Section schema version.
    pub abi_version: u16,
    /// The same ordered library/fallback selections as the static plan.
    pub selections: Vec<LibrarySelection>,
    /// Allocation facts shared by all views and launches.
    pub allocations: Vec<AllocationFact>,
    /// Bounded logical views over those allocations.
    pub views: Vec<ViewFact>,
    /// Launch bindings carrying byte ranges and tagged sources.
    pub launch_bindings: Vec<LaunchBinding>,
}

/// Compatibility spelling for the v2 payload.
pub type ExecutorPlanV2 = ExecutorPlanExtensionV2;

/// Typed v2 producer failure. Validation happens before the static selection
/// projection so malformed or overflowing spans cannot reach emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutorPlanV2Error {
    /// An allocation identity appeared more than once.
    DuplicateAllocation {
        buffer: crate::device_program::BufferId,
    },
    /// An allocation's repeated identity/capacity facts are malformed.
    InvalidAllocation {
        buffer: crate::device_program::BufferId,
        detail: String,
    },
    /// A view refers to no allocation or has malformed/overflowing span facts.
    InvalidView {
        allocation: crate::device_program::BufferId,
        detail: String,
    },
    /// A binding points outside the ordered kernel/resource facts.
    InvalidLaunchBinding { kernel_index: usize, detail: String },
    /// Two bindings claim the same kernel slot.
    DuplicateLaunchBinding {
        kernel_index: usize,
        group: u32,
        binding: u32,
    },
    /// A v2 section without any dynamic source is an accidental static plan.
    MissingRuntimeSource,
    /// A v2 section has a stale or skewed selection count/order.
    SelectionSkew { detail: String },
}

impl fmt::Display for ExecutorPlanV2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateAllocation { buffer } => {
                write!(f, "executor-plan v2 repeats allocation {}", buffer.0)
            }
            Self::InvalidAllocation { buffer, detail } => {
                write!(
                    f,
                    "executor-plan v2 allocation {} is invalid: {detail}",
                    buffer.0
                )
            }
            Self::InvalidView { allocation, detail } => {
                write!(
                    f,
                    "executor-plan v2 view for allocation {} is invalid: {detail}",
                    allocation.0
                )
            }
            Self::InvalidLaunchBinding {
                kernel_index,
                detail,
            } => write!(
                f,
                "executor-plan v2 binding for kernel {kernel_index} is invalid: {detail}"
            ),
            Self::DuplicateLaunchBinding {
                kernel_index,
                group,
                binding,
            } => write!(
                f,
                "executor-plan v2 repeats kernel {kernel_index} binding ({group}, {binding})"
            ),
            Self::MissingRuntimeSource => {
                write!(f, "executor-plan v2 carries no tagged runtime source")
            }
            Self::SelectionSkew { detail } => {
                write!(f, "executor-plan v2 selection skew: {detail}")
            }
        }
    }
}

impl std::error::Error for ExecutorPlanV2Error {}

impl ExecutorPlanV2Facts {
    /// Validate dynamic facts against the typed device program before any
    /// executor-plan selection is projected.
    ///
    /// # Errors
    /// Returns `ExecutorPlanV2Error` when a dynamic fact contradicts the program.
    pub fn validate_for_program(&self, program: &DeviceProgram) -> Result<(), ExecutorPlanV2Error> {
        let allocations = collect_allocation_facts(&self.allocations)?;

        validate_view_facts(&self.views, &allocations)?;

        let mut seen_bindings = std::collections::BTreeSet::new();
        let mut has_runtime_source = false;
        for launch_binding in &self.launch_bindings {
            let key = (
                launch_binding.kernel_index,
                launch_binding.binding.group,
                launch_binding.binding.binding,
            );
            if !seen_bindings.insert(key) {
                return Err(ExecutorPlanV2Error::DuplicateLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    group: launch_binding.binding.group,
                    binding: launch_binding.binding.binding,
                });
            }
            let Some(kernel) = program.kernels.get(launch_binding.kernel_index) else {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: "kernel index is outside the program".to_owned(),
                });
            };
            let Some(resource) = kernel
                .resources
                .iter()
                .find(|resource| resource.binding == launch_binding.binding)
            else {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: format!(
                        "binding ({}, {}) is not declared by the kernel",
                        launch_binding.binding.group, launch_binding.binding.binding
                    ),
                });
            };
            if resource.buffer.id != launch_binding.handle {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: format!(
                        "handle {} disagrees with declared resource handle {}",
                        launch_binding.handle.0, resource.buffer.id.0
                    ),
                });
            }
            let Some(allocation) = allocations.get(&launch_binding.handle) else {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: "binding refers to an unknown allocation".to_owned(),
                });
            };
            if launch_binding.view_span == 0 {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: "view span must be nonzero".to_owned(),
                });
            }
            let Some(end) = launch_binding
                .byte_offset
                .checked_add(launch_binding.view_span)
            else {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: "byte offset plus view span overflows".to_owned(),
                });
            };
            if end > allocation.capacity_bytes {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: format!(
                        "binding end {end} exceeds allocation capacity {}",
                        allocation.capacity_bytes
                    ),
                });
            }
            if view_base_bytes_for_binding(
                &self.views,
                allocation,
                launch_binding.byte_offset,
                launch_binding.view_span,
            )
            .is_none()
            {
                return Err(ExecutorPlanV2Error::InvalidLaunchBinding {
                    kernel_index: launch_binding.kernel_index,
                    detail: "byte range does not match a bounded allocation view".to_owned(),
                });
            }
            has_runtime_source |= launch_binding.runtime_source.is_runtime();
        }
        if !has_runtime_source {
            return Err(ExecutorPlanV2Error::MissingRuntimeSource);
        }
        Ok(())
    }
}

fn collect_allocation_facts(
    allocations: &[AllocationFact],
) -> Result<
    std::collections::BTreeMap<crate::device_program::BufferId, &AllocationFact>,
    ExecutorPlanV2Error,
> {
    let mut by_id = std::collections::BTreeMap::new();
    for allocation in allocations {
        let id = allocation.buffer.id;
        if by_id.insert(id, allocation).is_some() {
            return Err(ExecutorPlanV2Error::DuplicateAllocation { buffer: id });
        }
        if allocation.capacity_bytes == 0 {
            return Err(ExecutorPlanV2Error::InvalidAllocation {
                buffer: id,
                detail: "capacity bytes must be nonzero".to_owned(),
            });
        }
        if allocation.lifetime != allocation.buffer.lifetime {
            return Err(ExecutorPlanV2Error::InvalidAllocation {
                buffer: id,
                detail: "lifetime disagrees with buffer identity".to_owned(),
            });
        }
    }
    Ok(by_id)
}

fn validate_view_facts(
    views: &[ViewFact],
    allocations: &std::collections::BTreeMap<crate::device_program::BufferId, &AllocationFact>,
) -> Result<(), ExecutorPlanV2Error> {
    for view in views {
        let Some(allocation) = allocations.get(&view.allocation) else {
            return Err(ExecutorPlanV2Error::InvalidView {
                allocation: view.allocation,
                detail: "view refers to an unknown allocation".to_owned(),
            });
        };
        validate_view_span(view, allocation)?;
    }
    Ok(())
}

fn validate_view_span(
    view: &ViewFact,
    allocation: &AllocationFact,
) -> Result<(), ExecutorPlanV2Error> {
    if view.logical_dims.is_empty() || view.logical_dims.len() != view.strides.len() {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "logical dimensions and strides must have the same nonzero rank".to_owned(),
        });
    }
    if view
        .logical_dims
        .iter()
        .chain(view.strides.iter())
        .any(|value| *value == 0)
    {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "logical dimensions and strides must be positive".to_owned(),
        });
    }
    if view.maximum_span == 0 {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "maximum span must be nonzero".to_owned(),
        });
    }
    let Some(required_elements) = view_required_elements(view) else {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "logical span arithmetic overflows".to_owned(),
        });
    };
    let required_span = required_elements
        .checked_mul(allocation.dtype.byte_width())
        .ok_or_else(|| ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "byte span arithmetic overflows".to_owned(),
        })?;
    if view.maximum_span < required_span {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: format!(
                "maximum span {} is smaller than required span {required_span}",
                view.maximum_span
            ),
        });
    }
    let base_bytes = view
        .static_base
        .checked_mul(allocation.dtype.byte_width())
        .ok_or_else(|| ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "static base byte arithmetic overflows".to_owned(),
        })?;
    let end = base_bytes.checked_add(view.maximum_span).ok_or_else(|| {
        ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: "view end arithmetic overflows".to_owned(),
        }
    })?;
    if end > allocation.capacity_bytes {
        return Err(ExecutorPlanV2Error::InvalidView {
            allocation: view.allocation,
            detail: format!(
                "view end {end} exceeds allocation capacity {}",
                allocation.capacity_bytes
            ),
        });
    }
    Ok(())
}

fn view_base_bytes_for_binding(
    views: &[ViewFact],
    allocation: &AllocationFact,
    byte_offset: u64,
    view_span: u64,
) -> Option<u64> {
    let dtype_width = allocation.dtype.byte_width();
    views.iter().find_map(|view| {
        if view.allocation != allocation.buffer.id || view.maximum_span < view_span {
            return None;
        }
        let required_span = view_required_elements(view)?.checked_mul(dtype_width)?;
        if view_span < required_span {
            return None;
        }
        let base_bytes = view.static_base.checked_mul(dtype_width)?;
        (base_bytes == byte_offset).then_some(base_bytes)
    })
}

/// The bounded element count a view's logical dims/strides address; `None`
/// when the logical span arithmetic overflows.
fn view_required_elements(view: &ViewFact) -> Option<u64> {
    let max_element =
        view.logical_dims
            .iter()
            .zip(&view.strides)
            .try_fold(0u64, |acc, (dim, stride)| {
                dim.checked_sub(1)
                    .and_then(|extent| extent.checked_mul(*stride))
                    .and_then(|extent| acc.checked_add(extent))
            })?;
    max_element.checked_add(1)
}

impl ExecutorPlanExtensionV2 {
    /// Validate version, ordered selections, and binding ownership before a
    /// consumer interprets any v2 field.
    ///
    /// # Errors
    /// Returns a `String` detail when a v2 field disagrees with the kernel count.
    pub fn validate_for_kernel_count(&self, kernel_count: usize) -> Result<(), String> {
        if self.abi_version != EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION {
            return Err(format!(
                "executor-plan section version {} is not supported (expected {})",
                self.abi_version, EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION
            ));
        }
        if self.selections.len() != kernel_count {
            return Err(format!(
                "executor-plan selection count {} does not match kernel count {}",
                self.selections.len(),
                kernel_count
            ));
        }
        for (position, selection) in self.selections.iter().enumerate() {
            if selection.kernel_index != position || !selection.bind.is_well_formed() {
                return Err(format!(
                    "executor-plan v2 selection {position} is not an ordered well-formed selection"
                ));
            }
        }
        for binding in &self.launch_bindings {
            if binding.kernel_index >= kernel_count {
                return Err(format!(
                    "executor-plan v2 binding references kernel {} but there are {} kernels",
                    binding.kernel_index, kernel_count
                ));
            }
        }
        if !self
            .launch_bindings
            .iter()
            .any(|binding| binding.runtime_source.is_runtime())
        {
            return Err("executor-plan v2 carries no tagged runtime source".to_owned());
        }
        Ok(())
    }
}

/// Produce a v2 executor plan after validating every allocation, view, and
/// launch binding against the typed program. The v1 [`executor_plan_extension`]
/// producer remains the static compatibility path and is not changed.
///
/// # Errors
/// Returns `ExecutorPlanV2Error` when any allocation, view, or launch binding
/// fails validation against the typed program.
pub fn executor_plan_extension_v2(
    program: &DeviceProgram,
    facts: ExecutorPlanV2Facts,
) -> Result<ExecutorPlanExtensionV2, ExecutorPlanV2Error> {
    facts.validate_for_program(program)?;
    let static_plan = executor_plan_extension(program);
    let extension = ExecutorPlanExtensionV2 {
        abi_version: EXECUTOR_PLAN_RUNTIME_BIND_ABI_VERSION,
        selections: static_plan.selections,
        allocations: facts.allocations,
        views: facts.views,
        launch_bindings: facts.launch_bindings,
    };
    extension
        .validate_for_kernel_count(program.kernels.len())
        .map_err(|detail| ExecutorPlanV2Error::SelectionSkew { detail })?;
    Ok(extension)
}

// ---------------------------------------------------------------------------
// LAUNCH DESCRIPTORS AND PROJECTIONS
// ---------------------------------------------------------------------------

/// One typed buffer slot in a launch descriptor.
///
/// A program fact snapshot of a [`DeviceResource`]: the binding indices, the
/// versioned shape, and the slot access. Hosts bind buffers from these typed
/// facts — never by parsing emitted MSL/LLVM/PTX (A3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchBufferSlot {
    /// Target-neutral binding indices.
    pub binding: Binding,
    /// Element type of the bound shape.
    pub element_ty: MirType,
    /// Element count of the bound shape.
    pub element_count: u64,
    /// Program-level buffer role (input / output / in-out).
    pub role: BufferRole,
    /// Where the storage lives (`DeviceHandle` / `HostOwned`).
    pub storage: MirTensorStorageLayout,
}

/// Backend-neutral typed launch descriptor for one device-program kernel.
///
/// The host-facing launch facts (S1-4 consumes them): the logical entry name,
/// the recipe family, the workgroup/dispatch/workgroup-count plan, the typed
/// buffer slots, and the shared-memory footprint. Every field is a program
/// fact; nothing is inferred from emitted text (A3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelLaunchDescriptor {
    /// Index into [`DeviceProgram`]`::kernels`.
    pub kernel_index: usize,
    /// Target-neutral logical entry name (backends map it to their syntax).
    pub entry: String,
    /// M2 library-v0 bind descriptor. The descriptor is carried separately
    /// from the entry so one body can serve multiple concrete shapes.
    pub bind: BindDescriptor,
    /// Recipe family tag.
    pub recipe: RecipeKind,
    /// Threads per workgroup.
    pub workgroup: MirWorkgroupSize,
    /// Dispatch extent in workgroups.
    pub dispatch: MirKernelDispatchSize,
    /// Derived workgroup count the host launches.
    pub workgroup_count: MirKernelWorkgroupCount,
    /// Typed buffer slots, in the kernel's typed resource order.
    pub slots: Vec<LaunchBufferSlot>,
    /// Shared-memory footprint in bytes (0 for elementwise kernels).
    pub shared_memory_bytes: u64,
    /// Per-output bounds/index domains (DDCP1-U3, contract §Per-output
    /// bounds): each Write/ReadWrite slot's own extent. Heterogeneous
    /// outputs each carry their own derived domain — hosts dispatch per
    /// output, never under one guessed shared extent.
    pub output_domains: Vec<PerOutputDomain>,
}

impl KernelLaunchDescriptor {
    /// Build the launch descriptor for one kernel unit.
    #[must_use]
    pub fn from_kernel_unit(kernel: &KernelUnit, kernel_index: usize) -> Self {
        Self {
            kernel_index,
            entry: kernel.entry.clone(),
            bind: bind_descriptor_for_kernel(kernel),
            recipe: RecipeKind::from_plan(&kernel.plan),
            workgroup: kernel.launch.workgroup,
            dispatch: kernel.launch.dispatch_size,
            workgroup_count: kernel.launch.workgroup_count,
            slots: kernel.resources.iter().map(launch_buffer_slot).collect(),
            shared_memory_bytes: plan_shared_memory_bytes(&kernel.plan),
            output_domains: per_output_domains(kernel),
        }
    }
}

/// Derive the typed launch descriptors for every kernel of a device program,
/// in program order.
#[must_use]
pub fn launch_descriptors(program: &DeviceProgram) -> Vec<KernelLaunchDescriptor> {
    program
        .kernels
        .iter()
        .enumerate()
        .map(|(index, kernel)| KernelLaunchDescriptor::from_kernel_unit(kernel, index))
        .collect()
}

/// Derive the M2 library-v0 bind facts for one carried kernel.
///
/// This is the only planner-side projection needed by the additive executor
/// extension. It preserves the existing `KernelUnit` and `DeviceProgram`
/// schema: generated kernels continue to carry their original entry, resources,
/// and launch plan, while library consumers receive the same typed dimensions,
/// strides, layout, and grid as a sidecar fact.
#[must_use]
pub fn bind_descriptor_for_kernel(kernel: &KernelUnit) -> BindDescriptor {
    let output_extent = kernel
        .resources
        .iter()
        .find(|resource| {
            matches!(
                resource.access,
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
            )
        })
        .map_or_else(
            || {
                kernel
                    .resources
                    .first()
                    .map_or(1, |resource| resource.version.element_count)
            },
            |resource| resource.version.element_count,
        );
    let (dims, layout) = bind_dimensions(&kernel.plan, output_extent);
    BindDescriptor::row_major(dims, kernel.launch.workgroup_count, layout)
}

/// Build the additive M0 executor-plan section without changing the existing
/// device-program or FMIR wire shape.
#[must_use]
pub fn executor_plan_extension(program: &DeviceProgram) -> ExecutorPlanExtension {
    ExecutorPlanExtension {
        abi_version: EXECUTOR_PLAN_BIND_ABI_VERSION,
        selections: program
            .kernels
            .iter()
            .enumerate()
            .map(|(kernel_index, kernel)| {
                let recipe = RecipeKind::from_plan(&kernel.plan);
                let bind = bind_descriptor_for_kernel(kernel);
                let binds = library_binds_for_kernel(kernel, &bind);
                let offsets = binds
                    .iter()
                    .map(|bind| OffsetBinding {
                        name: bind.name.clone(),
                        offset_elements: bind.offset_elements,
                        strides: bind.strides.clone(),
                    })
                    .collect();
                LibrarySelection {
                    kernel_index,
                    library_entry: library_entry_for_kernel(kernel, recipe),
                    logical_signature: logical_signature_for_kernel(kernel, recipe),
                    binds,
                    uniforms: packed_decode_uniforms_for_kernel(kernel),
                    offsets,
                    workspace: None,
                    backend_requirements: packed_backend_requirements_for_kernel(kernel),
                    provenance: PlanProvenance {
                        pass: "device-program-plan".to_owned(),
                        revision: u32::from(EXECUTOR_PLAN_BIND_ABI_VERSION),
                        source_nodes: vec![kernel.function.0],
                    },
                    bind,
                }
            })
            .collect(),
    }
}

fn bind_dimensions(plan: &CollectionKernelPlan, output_extent: u64) -> (Vec<u64>, BindLayout) {
    match plan {
        CollectionKernelPlan::RmsNormalization(plan) => {
            if plan.width > 0 && output_extent.is_multiple_of(plan.width) {
                (
                    vec![output_extent / plan.width, plan.width],
                    BindLayout::RowMajor,
                )
            } else {
                (vec![output_extent], BindLayout::Flat)
            }
        }
        CollectionKernelPlan::Rope(plan) => {
            let layout = if plan.rotate_half {
                BindLayout::RopeRotateHalf {
                    rotated_width: plan.dim,
                }
            } else {
                BindLayout::RopeConsecutivePair {
                    rotated_width: plan.dim,
                }
            };
            let rows = if plan.width > 0 && output_extent.is_multiple_of(plan.width) {
                output_extent / plan.width
            } else {
                plan.rows.max(1)
            };
            (vec![rows, plan.width.max(1)], layout)
        }
        CollectionKernelPlan::CausalMaskedSoftmax(plan) => {
            (vec![plan.rows, plan.cols], BindLayout::RowMajor)
        }
        CollectionKernelPlan::TiledMatMul(plan) => (vec![plan.m, plan.n], BindLayout::RowMajor),
        CollectionKernelPlan::QuantizedMatMul(plan) => (vec![plan.m, plan.n], BindLayout::RowMajor),
        CollectionKernelPlan::GroupedMatMul(plan) => {
            (vec![plan.groups, plan.m, plan.n], BindLayout::RowMajor)
        }
        CollectionKernelPlan::BatchedAttention(plan) => {
            let dims = match plan.kind {
                BatchedAttentionKind::ProjectionQ => {
                    vec![plan.kv_heads, plan.q_per_kv, plan.seq, plan.head_dim]
                }
                BatchedAttentionKind::ProjectionK | BatchedAttentionKind::ProjectionV => {
                    vec![plan.kv_heads, plan.seq, plan.head_dim]
                }
                BatchedAttentionKind::Scores | BatchedAttentionKind::Softmax => {
                    vec![plan.kv_heads, plan.q_per_kv, plan.seq, plan.seq]
                }
                BatchedAttentionKind::ContextSequenceMajor => vec![plan.seq, plan.hidden],
            };
            (dims, BindLayout::Strided)
        }
        CollectionKernelPlan::Transpose(plan) => (vec![plan.m, plan.n], BindLayout::RowMajor),
        CollectionKernelPlan::TreeReduction(plan) => (vec![plan.length], BindLayout::Flat),
        CollectionKernelPlan::Elementwise
        | CollectionKernelPlan::AxisReduction(_)
        | CollectionKernelPlan::RowSoftmax(_)
        | CollectionKernelPlan::LayerNormalization(_)
        | CollectionKernelPlan::Gather(_)
        | CollectionKernelPlan::SsmConv1d(_)
        | CollectionKernelPlan::SsmScan(_)
        // SFR-5: the declared distributed sum's broadcast result is a flat
        // partial-slot write — same bind shape as the tree floor.
        | CollectionKernelPlan::LaneGrid { .. }
        | CollectionKernelPlan::SingleLane
        | CollectionKernelPlan::GridReduce { .. }
        | CollectionKernelPlan::Scan { .. }
        | CollectionKernelPlan::DistributedSum(_) => (vec![output_extent.max(1)], BindLayout::Flat),
    }
}

fn logical_signature_for_kernel(kernel: &KernelUnit, recipe: RecipeKind) -> LogicalSignature {
    let inputs = kernel
        .resources
        .iter()
        .filter(|resource| {
            matches!(
                resource.access,
                MirKernelResourceAccess::Read | MirKernelResourceAccess::ReadWrite
            )
        })
        .enumerate()
        .map(|(index, _)| format!("input{index}"))
        .collect();
    let outputs = kernel
        .resources
        .iter()
        .filter(|resource| {
            matches!(
                resource.access,
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
            )
        })
        .enumerate()
        .map(|(index, _)| format!("output{index}"))
        .collect();
    LogicalSignature {
        operation: format!("{recipe:?}"),
        inputs,
        outputs,
    }
}

fn library_binds_for_kernel(kernel: &KernelUnit, bind: &BindDescriptor) -> Vec<LibraryBind> {
    let packed = packed_region_for_plan(&kernel.plan);
    kernel
        .resources
        .iter()
        .enumerate()
        .map(|(index, resource)| {
            let (dims, strides) = match packed.as_ref() {
                Some(packed) if resource.binding.binding == 1 => {
                    (vec![packed.packed_bytes], vec![1])
                }
                _ => (bind.dims.clone(), bind.strides.clone()),
            };
            LibraryBind {
                name: format!("buffer{index}"),
                binding: resource.binding,
                dims,
                strides,
                offset_elements: 0,
            }
        })
        .collect()
}

pub(super) fn library_entry_for_kernel(kernel: &KernelUnit, recipe: RecipeKind) -> Option<String> {
    if let Some(packed) = packed_region_for_plan(&kernel.plan) {
        return Some(quantized_compute_entry(packed.decode_extent).to_owned());
    }
    let entry = match recipe {
        RecipeKind::RmsNormalization => Some("rms"),
        RecipeKind::Rope => Some("rope"),
        RecipeKind::RowSoftmax | RecipeKind::CausalMaskedSoftmax => Some("softmax"),
        RecipeKind::Elementwise => match kernel.library_operation {
            LibraryOperation::SwiGlu => Some("swiglu"),
            LibraryOperation::ResidualRmsNorm => Some("residual"),
            _ => None,
        },
        _ => None,
    }?;
    Some(entry.to_owned())
}

fn quantized_compute_entry(decode_extent: u64) -> &'static str {
    if DECODE_GEMV_SELECTION_ENABLED && decode_extent == 1 {
        QUANTIZED_GEMV_ENTRY
    } else {
        QUANTIZED_GEMM_ENTRY
    }
}

struct PackedRegionFacts {
    layout: PackedStorageLayout,
    packed_bytes: u64,
    decode_extent: u64,
}

fn packed_region_for_plan(plan: &CollectionKernelPlan) -> Option<PackedRegionFacts> {
    match plan {
        CollectionKernelPlan::QuantizedMatMul(plan) => Some(PackedRegionFacts {
            layout: plan.layout,
            packed_bytes: plan
                .layout
                .packed_bytes_for_elements(plan.k.saturating_mul(plan.n))?,
            decode_extent: plan.m,
        }),
        CollectionKernelPlan::BatchedAttention(plan) => {
            let layout = plan.layout?;
            let n = match plan.kind {
                BatchedAttentionKind::ProjectionQ => plan.hidden,
                BatchedAttentionKind::ProjectionK | BatchedAttentionKind::ProjectionV => {
                    plan.kv_heads.saturating_mul(plan.head_dim)
                }
                _ => return None,
            };
            Some(PackedRegionFacts {
                layout,
                packed_bytes: layout.packed_bytes_for_elements(plan.hidden.saturating_mul(n))?,
                decode_extent: plan.seq,
            })
        }
        _ => None,
    }
}

fn packed_decode_uniforms_for_kernel(kernel: &KernelUnit) -> Vec<UniformBinding> {
    let Some(packed) = packed_region_for_plan(&kernel.plan) else {
        return Vec::new();
    };
    let packed_bytes = u32::try_from(packed.packed_bytes).unwrap_or(u32::MAX);
    vec![
        UniformBinding {
            name: PACKED_LAYOUT_GGML_TYPE_ID_UNIFORM.to_owned(),
            ty: UniformType::U32,
            bits: packed.layout.ggml_type_id(),
        },
        UniformBinding {
            name: PACKED_BLOCK_BYTES_UNIFORM.to_owned(),
            ty: UniformType::U32,
            bits: packed.layout.block_bytes(),
        },
        UniformBinding {
            name: PACKED_BLOCK_ELEMENTS_UNIFORM.to_owned(),
            ty: UniformType::U32,
            bits: packed.layout.block_elements(),
        },
        UniformBinding {
            name: PACKED_BYTES_UNIFORM.to_owned(),
            ty: UniformType::U32,
            bits: packed_bytes,
        },
        UniformBinding {
            name: DECODE_GEMV_UNIFORM.to_owned(),
            ty: UniformType::U32,
            bits: u32::from(DECODE_GEMV_SELECTION_ENABLED && packed.decode_extent == 1),
        },
    ]
}

fn packed_backend_requirements_for_kernel(kernel: &KernelUnit) -> Vec<BackendRequirement> {
    let Some(packed) = packed_region_for_plan(&kernel.plan) else {
        return Vec::new();
    };
    vec![BackendRequirement {
        backend: "metal".to_owned(),
        feature: quantized_compute_entry(packed.decode_extent).to_owned(),
    }]
}

fn launch_buffer_slot(resource: &DeviceResource) -> LaunchBufferSlot {
    LaunchBufferSlot {
        binding: resource.binding,
        element_ty: resource.version.element_ty,
        element_count: resource.version.element_count,
        role: resource.buffer.role,
        storage: resource.buffer.storage,
    }
}

/// The per-output bounds/index domain of one kernel output slot
/// (DDCP1-U3, contract §Per-output bounds rule 1): every output has its
/// OWN extent — never inferred from the shared dispatch and never taken
/// from another output's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerOutputDomain {
    /// The output slot's target-neutral binding indices.
    pub binding: Binding,
    /// The output's own element extent.
    pub element_count: u64,
}

/// Derive each kernel output's own bounds/index domain (DDCP1-U3).
///
/// Output slots are the kernel's `Write` and `ReadWrite` resources — each one
/// gets its own extent from its bound version's element count. The derivation
/// is a pure function over carried program facts (A3: never text).
#[must_use]
pub fn per_output_domains(kernel: &KernelUnit) -> Vec<PerOutputDomain> {
    kernel
        .resources
        .iter()
        .filter(|resource| {
            matches!(
                resource.access,
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
            )
        })
        // A runtime-shaped output has no static extent; the dispatcher checks
        // the actual argument against the lane domain at launch (E_INVALID_ARGS).
        .filter(|resource| {
            resource.version.element_count != crate::abi::RUNTIME_SHAPED_ELEMENT_COUNT
        })
        .map(|resource| PerOutputDomain {
            binding: resource.binding,
            element_count: resource.version.element_count,
        })
        .collect()
}

/// Validate per-output bounds/index domains fail-closed (DDCP1-U3, contract
/// §Per-output bounds rules 1–3).
///
/// Every kernel output must be covered by the kernel's dispatched lanes —
/// no shared dispatch extent may under-fill a heterogeneous or single
/// output. A single dispatch extent is legal only where the member outputs
/// provably share one domain (identical extents); heterogeneous outputs are
/// legal only because each output carries its own derived domain (the
/// launch descriptor exposes [`PerOutputDomain`] per output).
///
/// # Errors
/// Returns [`DeviceProgramError::OutputExtentUnderfilled`] when an output's
/// own domain exceeds the dispatched lanes.
pub fn validate_per_output_domains(program: &DeviceProgram) -> Result<(), DeviceProgramError> {
    for (kernel_index, kernel) in program.kernels.iter().enumerate() {
        // A single-lane kernel's one thread runs the whole body and addresses
        // every output element itself: its coverage is the body, not lanes.
        if kernel.plan == CollectionKernelPlan::SingleLane {
            continue;
        }
        let dispatched = dispatched_lanes(kernel);
        for domain in per_output_domains(kernel) {
            if domain.element_count > dispatched {
                return Err(DeviceProgramError::OutputExtentUnderfilled {
                    kernel: kernel_index,
                    group: domain.binding.group,
                    binding: domain.binding.binding,
                    element_count: domain.element_count,
                    dispatched,
                });
            }
        }
    }
    Ok(())
}

/// The output coverage of the shared dispatch: workgroup count ×
/// workgroup size across all three axes — one lane per element for the
/// per-element launches. The PGC-R5 row-reduction family is the one
/// collective 1D exception: one `ROW_REDUCTION_WORKGROUP_X`-lane workgroup
/// owns one FULL row (the `RowReduction` contract's law), so the write
/// coverage is workgroups × row width, not lanes — the per-thread
/// under-coverage oracle does not apply (a workgroup writes a whole row,
/// not one element; the tiled-matmul over-cover class is the precedent).
/// Keyed on the contract-minted launch shape for the three row-reduction
/// plans; an under-rowed launch still under-fills and fails closed.
fn dispatched_lanes(kernel: &KernelUnit) -> u64 {
    let launch = &kernel.launch;
    let lanes = launch.workgroup_count.x
        * u64::from(launch.workgroup.x)
        * launch.workgroup_count.y
        * u64::from(launch.workgroup.y)
        * launch.workgroup_count.z
        * u64::from(launch.workgroup.z);
    if launch.workgroup.y == 1
        && launch.workgroup.z == 1
        && launch.workgroup.x == crate::kernel_plan::ROW_REDUCTION_WORKGROUP_X
    {
        let workgroups = launch.workgroup_count.x;
        match &kernel.plan {
            CollectionKernelPlan::RmsNormalization(plan) => {
                return workgroups.saturating_mul(plan.width);
            }
            CollectionKernelPlan::CausalMaskedSoftmax(plan) => {
                return workgroups.saturating_mul(plan.cols);
            }
            // The additive-mask softmax idiom (a `RowSoftmax` plan under
            // the row launch): the row width is the output extent
            // partitioned by the row workgroups — coverage is the row
            // partition itself (an indivisible extent keeps the lane law
            // and fails closed).
            CollectionKernelPlan::RowSoftmax(_) => {
                if let Some(output) = kernel.resources.iter().rev().find(|resource| {
                    matches!(
                        resource.access,
                        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                    )
                }) {
                    let extent = output.version.element_count;
                    if workgroups > 0 && extent % workgroups == 0 {
                        return extent;
                    }
                }
            }
            _ => {}
        }
    }
    lanes
}

/// The recipe's shared-memory footprint in bytes (the host sizes the
/// workgroup-memory budget from this; 0 for elementwise kernels).
pub(super) fn plan_shared_memory_bytes(plan: &CollectionKernelPlan) -> u64 {
    match plan {
        CollectionKernelPlan::Elementwise
        | CollectionKernelPlan::Transpose(_)
        // S6-P1: the transformer recipes declare no shared-memory layout —
        // each row is one thread's serial pass (max → exp → row sum →
        // divide), so the launch descriptor reports a zero workgroup-memory
        // footprint.
        | CollectionKernelPlan::AxisReduction(_)
        | CollectionKernelPlan::RowSoftmax(_)
        | CollectionKernelPlan::LayerNormalization(_)
        // GI3-1: the inference recipes are likewise row-wise serial passes
        // with no shared-memory layout.
        | CollectionKernelPlan::Gather(_)
        | CollectionKernelPlan::RmsNormalization(_)
        | CollectionKernelPlan::Rope(_)
        | CollectionKernelPlan::CausalMaskedSoftmax(_)
        | CollectionKernelPlan::QuantizedMatMul(_)
        | CollectionKernelPlan::GroupedMatMul(_)
        | CollectionKernelPlan::BatchedAttention(_)
        | CollectionKernelPlan::SsmConv1d(_)
        | CollectionKernelPlan::SsmScan(_)
        | CollectionKernelPlan::LaneGrid { .. }
        | CollectionKernelPlan::SingleLane => 0,
        // The workgroup tree: one `value_bytes + ...` partial per lane.
        CollectionKernelPlan::GridReduce {
            workgroup,
            partial_layout,
            ..
        } => u64::from(*workgroup) * u64::from(partial_layout.element_bytes()),
        CollectionKernelPlan::Scan { workgroup, partial_layout, .. } =>
            2 * u64::from(*workgroup) * u64::from(partial_layout.element_bytes()),
        CollectionKernelPlan::TreeReduction(plan) => {
            shared_memory_layout_bytes(&plan.shared_memory)
        }
        // SFR-5: the declared distributed sum carries the same tree floor
        // shared-memory layout — one slot per workgroup lane.
        CollectionKernelPlan::DistributedSum(plan) => {
            shared_memory_layout_bytes(&plan.shared_memory)
        }
        CollectionKernelPlan::TiledMatMul(plan) => {
            shared_memory_layout_bytes(&plan.shared_memory.shared_a)
                + shared_memory_layout_bytes(&plan.shared_memory.shared_b)
        }
    }
}

fn shared_memory_layout_bytes(layout: &SharedMemoryLayout) -> u64 {
    u64::from(layout.slot_count)
        .saturating_mul(u64::try_from(layout.element_byte_width).unwrap_or(u64::MAX))
}
