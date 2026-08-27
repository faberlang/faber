//! Language-surface vocabulary shared by the analysis and MIR layers.
//!
//! These types are the compiler's *vocabulary* — kernel builtin names,
//! atomic/collection op categories, lowerability verdicts — not MIR
//! machinery. They live here (below both layers) so semantic, HIR, and
//! intrinsics consume them without importing the MIR crate, keeping
//! `radix-mir` a downward-consumed leaf (feature-granularity doc §2).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirKernelBuiltin {
    GlobalInvocationX,
    GlobalInvocationY,
    GlobalInvocationZ,
    GlobalInvocationId,
    WorkgroupIdX,
    WorkgroupIdY,
    WorkgroupIdZ,
    WorkgroupId,
    LocalInvocationIdX,
    LocalInvocationIdY,
    LocalInvocationIdZ,
    LocalInvocationId,
    WorkgroupSizeX,
    WorkgroupSizeY,
    WorkgroupSizeZ,
}

impl MirKernelBuiltin {
    /// Width of vector-valued builtins (`global_invocation_id`, `workgroup_id`, `local_invocation_id`).
    pub const VECTOR_WIDTH: u8 = 3;

    pub const ALL: [Self; 15] = [
        Self::GlobalInvocationX,
        Self::GlobalInvocationY,
        Self::GlobalInvocationZ,
        Self::GlobalInvocationId,
        Self::WorkgroupIdX,
        Self::WorkgroupIdY,
        Self::WorkgroupIdZ,
        Self::WorkgroupId,
        Self::LocalInvocationIdX,
        Self::LocalInvocationIdY,
        Self::LocalInvocationIdZ,
        Self::LocalInvocationId,
        Self::WorkgroupSizeX,
        Self::WorkgroupSizeY,
        Self::WorkgroupSizeZ,
    ];

    #[must_use]
    pub fn from_spelling(spelling: &str) -> Option<Self> {
        match spelling {
            "global_invocation_x" => Some(Self::GlobalInvocationX),
            "global_invocation_y" => Some(Self::GlobalInvocationY),
            "global_invocation_z" => Some(Self::GlobalInvocationZ),
            "global_invocation_id" => Some(Self::GlobalInvocationId),
            "workgroup_id_x" => Some(Self::WorkgroupIdX),
            "workgroup_id_y" => Some(Self::WorkgroupIdY),
            "workgroup_id_z" => Some(Self::WorkgroupIdZ),
            "workgroup_id" => Some(Self::WorkgroupId),
            "local_invocation_id_x" => Some(Self::LocalInvocationIdX),
            "local_invocation_id_y" => Some(Self::LocalInvocationIdY),
            "local_invocation_id_z" => Some(Self::LocalInvocationIdZ),
            "local_invocation_id" => Some(Self::LocalInvocationId),
            "workgroup_size_x" => Some(Self::WorkgroupSizeX),
            "workgroup_size_y" => Some(Self::WorkgroupSizeY),
            "workgroup_size_z" => Some(Self::WorkgroupSizeZ),
            _ => None,
        }
    }

    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::GlobalInvocationX => "global_invocation_x",
            Self::GlobalInvocationY => "global_invocation_y",
            Self::GlobalInvocationZ => "global_invocation_z",
            Self::GlobalInvocationId => "global_invocation_id",
            Self::WorkgroupIdX => "workgroup_id_x",
            Self::WorkgroupIdY => "workgroup_id_y",
            Self::WorkgroupIdZ => "workgroup_id_z",
            Self::WorkgroupId => "workgroup_id",
            Self::LocalInvocationIdX => "local_invocation_id_x",
            Self::LocalInvocationIdY => "local_invocation_id_y",
            Self::LocalInvocationIdZ => "local_invocation_id_z",
            Self::LocalInvocationId => "local_invocation_id",
            Self::WorkgroupSizeX => "workgroup_size_x",
            Self::WorkgroupSizeY => "workgroup_size_y",
            Self::WorkgroupSizeZ => "workgroup_size_z",
        }
    }

    #[must_use]
    pub fn returns_vector(self) -> bool {
        matches!(
            self,
            Self::GlobalInvocationId | Self::LocalInvocationId | Self::WorkgroupId
        )
    }

    #[must_use]
    pub fn axis(self) -> MirKernelAxis {
        match self {
            Self::GlobalInvocationX
            | Self::WorkgroupIdX
            | Self::LocalInvocationIdX
            | Self::WorkgroupSizeX => MirKernelAxis::X,
            Self::GlobalInvocationY
            | Self::WorkgroupIdY
            | Self::LocalInvocationIdY
            | Self::WorkgroupSizeY => MirKernelAxis::Y,
            Self::GlobalInvocationZ
            | Self::WorkgroupIdZ
            | Self::LocalInvocationIdZ
            | Self::WorkgroupSizeZ => MirKernelAxis::Z,
            Self::GlobalInvocationId | Self::WorkgroupId | Self::LocalInvocationId => {
                MirKernelAxis::X
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MirKernelAxis {
    X,
    Y,
    Z,
}

impl MirKernelAxis {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
        }
    }
}

impl MirAtomicOp {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Store => "store",
            Self::Exchange => "exchange",
            Self::CompareExchange => "compare_exchange",
        }
    }
}

/// Compiler-owned atomic cell operation category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirAtomicOp {
    Load,
    Store,
    Exchange,
    CompareExchange,
}

/// Built-in collection operation category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirCollectionOp {
    /// Elementwise numeric arithmetic for array-backed collections. Dynamic
    /// live-window pairs are length-checked by the runner; scalar peers
    /// broadcast on either side. Queue/stack result kind follows the MIR type.
    ArrayAdd,
    ArraySub,
    ArrayMul,
    ArrayDiv,
    Append,
    AppendImmutable,
    Put,
    Delete,
    Index,
    IsEmpty,
    Length,
    Contains,
    First,
    Last,
    RemoveFirst,
    RemoveLast,
    Reverse,
    ReverseInPlace,
    Sort,
    Union,
    Intersection,
    Difference,
    SymmetricDifference,
    IsSubset,
    IsSuperset,
    Slice,
    Take,
    TakeLast,
    DropFirst,
    StartsWith,
    EndsWith,
    Uppercase,
    Lowercase,
    Trim,
    Split,
    Replace,
    Absolute,
    Signum,
    Minimum,
    Maximum,
    Approximate,
    Sum,
    /// First-index argmax over an array-backed collection (LSR-W0-SEMANTIC):
    /// the index of the maximal element, `nihil` when the source is empty —
    /// never a −1 sentinel. One tie law serves host scan and device
    /// tree-combine: among equal values the least index wins, so partials
    /// combine as `(value desc, index asc)` pairs. NaN law is pinned to
    /// `TensorMax`'s implemented fold (`f64::max`, maxNum semantics): a NaN
    /// element never wins while any number participates; an all-NaN source
    /// yields index 0 carrying NaN, matching `max from`'s NaN result.
    ArgMax,
    /// First-index argmin — the exact mirror of [`Self::ArgMax`]: least
    /// value, least index, `nihil` on empty, NaN law pinned to `TensorMin`'s
    /// `f64::min` fold.
    ArgMin,
    /// Sum across a specific axis: `sum(x, axis)`. The axis is a trailing
    /// const operand (`MirConstant::Int`) in the `MirRuntimeCall::args` list.
    /// Result rank is input rank - 1.
    TensorSumAxis,
    Round,
    Floor,
    Ceiling,
    Truncate,
    TensorShape,
    TensorReshape,
    TensorGet,
    TensorSet,
    TensorFill,
    TensorCreate,
    TensorFromFlat,
    TensorFlatten,
    TensorMaterialize,
    TensorSlice,
    TensorAdd,
    TensorSub,
    TensorMul,
    TensorMatMul,
    TensorMean,
    /// Tensor maximum reduction over all elements of the operand.
    TensorMax,
    /// Tensor minimum reduction over all elements of the operand.
    TensorMin,
    /// Layer normalization: `(x - mean) / sqrt(var + eps)` with optional
    /// affine transform (gamma, beta). Axis and epsilon are trailing const
    /// operands in the `MirRuntimeCall::args` list, NOT a payload struct —
    /// `MirIntrinsic::Collection` is a bare discriminant.
    TensorLayerNorm,
    /// Root-mean-square normalization (the CCI-1 `rms_norm` source contract;
    /// `decoder_ops::rms_norm` semantics): `y = x·(1/sqrt(mean(x²)+eps))·gamma`,
    /// no mean subtraction, f64 sum-of-squares → f32 mean. Axis and epsilon
    /// are trailing const operands (`[x, axis, eps, gamma]`), gamma is the
    /// mandatory affine peer operand — mirroring `TensorLayerNorm`'s const
    /// layout without beta. Source-side axis-valid + gamma-width contract is
    /// enforced at the HIR→AIR construction site (fail-closed).
    TensorRmsNorm,
    /// llama-arch NORM consecutive-pair rotary position embedding (the CCI-1
    /// `rope_norm` source contract): pairs `(x[2k], x[2k+1])` with
    /// `theta[k] = pos·freq_base^(−2k/dim)`, `freq_base` frozen at 100000
    /// (v1 source constant). `pos` + `dim` are trailing const operands
    /// (`[x, pos, dim]`); elements beyond `dim` on the last axis are
    /// preserved. Odd/overlarge `dim` fail closed at the source surface.
    TensorRope,
    /// Qwen2 / GPT-NeoX `rotate_half` rotary embedding: pairs
    /// `(x[k], x[k + dim/2])` with the same `theta[k]` tables as
    /// [`Self::TensorRope`]. Same `[x, pos, dim]` const layout; llama
    /// pair-rotate callers stay on [`Self::TensorRope`].
    TensorRopeHalf,
    /// Row gather over the first axis (the CCI-1 `gather` source contract):
    /// `table[V,D]` + `ids[T]` → `[T,D]` with `out[t] = table[ids[t]]`,
    /// repeated/out-of-order ids preserved. The ids vector is a plain
    /// operand (`[x, ids]`); out-of-range/negative ids fail closed at the
    /// source surface (no silent clamping). Shape-transforming — not
    /// `TensorGet`/`TensorSlice`.
    TensorGather,
    /// Causal masked-row softmax (GI3-1): numerically stable row softmax
    /// over the causal attention scores with zero probability above the
    /// diagonal (`[x]` — the rank-2 causal score matrix; row `i` masks
    /// columns `j > i`). No mask tensor input (the causal structure is the
    /// recipe's semantics); the diagonal is included.
    TensorCausalMaskedSoftmax,
    /// Packed quantized GEMM (R-PACK-01A: name frozen). Operands are a
    /// rank-2 activation and a packed weight plus a trailing GGML type-id
    /// const (`[left, right, layout_id]`). In-kernel block dequant; f32
    /// accumulate. No whole-model F32 expansion.
    TensorQuantizedMatMul,
    /// Grouped/segmented packed GEMM over rank-3 expert slices (R-PACK-01A:
    /// name frozen). Trailing GGML type-id const (`[left, right, layout_id]`).
    TensorGroupedMatMul,
    /// Internal M3 head-batched attention primitive. This is compiler-owned,
    /// not a source intrinsic: the target-neutral contract resolves explicit
    /// group/head axes, GQA mapping, strides, dispatch, and layout.
    TensorBatchedAttention,
    /// Hybrid SSM conv1d (R-PACK-01A: name frozen). Trailing GGML type-id
    /// const (`[input, kernel, layout_id]`).
    TensorSsmConv1d,
    /// Hybrid SSM linear-recurrence scan / state update (R-PACK-01A: name
    /// frozen). Trailing GGML type-id const (`[input, layout_id]`).
    TensorSsmScan,
    TensorDiv,
    TensorNeg,
    /// Gaussian Error Linear Unit using the tanh approximation:
    /// 0.5*x*(1+tanh(√(2/π)*(x+0.044715*x³))). Error < 1e-6 vs exact Gelu.
    /// Elementwise unary activation on tensor values. All finite f32 inputs valid.
    TensorGelu,
    /// Rectified linear unit: max(0, x), elementwise unary activation on tensor
    /// values. All finite inputs valid; no domain constraints.
    TensorRelu,
    /// Softmax: `exp(x_i` - max(x)) / `sum(exp(x_j` - max(x))) with numerical
    /// stability. Operates on rank-1 (vector) or rank-2 (batched row-wise,
    /// axis 1 — the last axis). The AIR VJP exists
    /// (`radix-air/src/reverse_ad/vjp.rs`, softmax walk: Mul → `TensorSumAxis`
    /// → Fill → Sub → Mul); the MIR op itself is a forward collection op and
    /// the device recipe (`RowSoftmax`) is admitted by the stage-6 P1 unit.
    TensorSoftmax,
    TensorTranspose,
    /// Categorical cross-entropy loss: forward-only. Takes (logits, targets)
    /// as 2-arg collection op and returns scalar f32 loss.
    TensorCruxEntropia,
    /// Number of explicitly stored nonzero entries in a sparse tensor.
    SparseNonzeroCount,
    /// Materialize a sparse tensor as a dense tensor with the same element and shape.
    SparseDensify,
    /// Read a sparse coordinate, returning the element zero when absent.
    SparseGet,
    /// Store or remove a sparse coordinate; storing zero removes it.
    SparseSet,
    IntervalIntersect,
    IntervalUnion,
    IntervalLength,
    IntervalContains,
    IntervalClamp,
    IntervalMaterialize,
    /// Map key snapshot used by `itera de` over `tabula<K,V>`.
    Keys,
    /// Map value snapshot used by `itera ex` over `tabula<K,V>`.
    Values,
    /// Regex `consentit`: whether the `regex` receiver's pattern matches a
    /// `textus` argument (the Rust `regex` crate `is_match` semantics).
    RegexMatches,
    /// Cord walk: `ascii.cursor()` / later `textus.cursor()` → `cursor<ascii<1>>`.
    /// One buffer of one-byte ascii values; not a per-step heap slice.
    CordCursor,
}

impl MirCollectionOp {
    /// llama pair-rotate (`TensorRope`) or Qwen/NeoX `rotate_half`
    /// (`TensorRopeHalf`). Same `[x, pos, dim]` const layout; the variant
    /// selects the pairing.
    #[must_use]
    pub const fn is_rope(self) -> bool {
        matches!(self, Self::TensorRope | Self::TensorRopeHalf)
    }
}

/// A classified reason a MIR-backed target cannot lower a shape.
///
/// One gap per rejection site. Categories are the five named in
/// `goals/02-target-capability-validation.md`; the owning stage column is the
/// campaign stage that supplies the missing fact or removes the rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityGap {
    /// A MIR shape the target does not print at all (e.g. indirect value
    /// callee). Owner: callable/representation stages, or target policy.
    UnsupportedMirShape { shape: String, slug: &'static str },
    /// The target needs a layout fact MIR does not yet carry (e.g. an
    /// unresolved `Type::Param`/`Infer`/`Error`, or a missing `MirLayoutId`).
    /// Owner: Stage 3 (layout-facts).
    MissingLayoutFact { shape: String, slug: &'static str },
    /// The target needs a function/runtime ABI fact MIR does not yet carry
    /// (e.g. `try_call` failable CFG contract). Owner: Stage 4/8
    /// (function-runtime-abi / failable-call-cfg).
    MissingAbiFact { shape: String, slug: &'static str },
    /// The target needs a runtime-boundary classification (core semantics vs
    /// host integration vs device-forbidden) before it can emit the call.
    /// Owner: Stage 1 (runtime-boundaries) ledger; this stage is complete, so a
    /// gap here means a helper family lacks a classification.
    MissingRuntimeBoundary { shape: String, slug: &'static str },
    /// The target deliberately rejects a shape by policy (e.g. device-subset
    /// forbids host-integration helpers). Owner: target policy / MIR GPU.
    TargetPolicyRejection { shape: String, slug: &'static str },
}

/// Pre-emission lowerability verdict for a MIR program against one target.
///
/// `Capable` means the target's classifier found no structural gap; deep emit
/// may still reject shapes the pre-pass does not enumerate (the deep-emit
/// `unsupported()` path remains the safety net, classified to the default
/// category). `Rejected` carries the classified gaps; the first gap drives the
/// diagnostic shape substring so corpus guards stay byte-stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lowerability {
    Capable,
    Rejected(Vec<CapabilityGap>),
}

impl CapabilityGap {
    /// Human-readable shape text, e.g. `"try_call"`. Stable enough for corpus
    /// guards; emitters format the final diagnostic by combining this with a
    /// target prefix.
    #[must_use]
    pub fn shape(&self) -> &str {
        match self {
            Self::UnsupportedMirShape { shape, .. }
            | Self::MissingLayoutFact { shape, .. }
            | Self::MissingAbiFact { shape, .. }
            | Self::MissingRuntimeBoundary { shape, .. }
            | Self::TargetPolicyRejection { shape, .. } => shape,
        }
    }

    /// Stable category slug for machine classification and filtering
    /// (e.g. device-subset queries). Matches the variant name in kebab-case.
    #[must_use]
    pub fn slug(&self) -> &'static str {
        match self {
            Self::UnsupportedMirShape { slug, .. }
            | Self::MissingLayoutFact { slug, .. }
            | Self::MissingAbiFact { slug, .. }
            | Self::MissingRuntimeBoundary { slug, .. }
            | Self::TargetPolicyRejection { slug, .. } => slug,
        }
    }

    /// Default slug for the catch-all deep-emit fallback category.
    pub const UNSUPPORTED_MIR_SHAPE_SLUG: &'static str = "unsupported-mir-shape";
}

impl Lowerability {
    /// `true` when the target reports no structural lowerability gap.
    #[must_use]
    pub fn is_capable(&self) -> bool {
        matches!(self, Self::Capable)
    }

    /// First gap, if any. Diagnostics use this for a stable shape substring.
    #[must_use]
    pub fn first_gap(&self) -> Option<&CapabilityGap> {
        match self {
            Self::Capable => None,
            Self::Rejected(gaps) => gaps.first(),
        }
    }

    /// All classified gaps when rejected; empty when capable.
    #[must_use]
    pub fn gaps(&self) -> &[CapabilityGap] {
        match self {
            Self::Capable => &[],
            Self::Rejected(gaps) => gaps.as_slice(),
        }
    }
}

/// Closed v1 kernel module identity carried on MIR providers.
///
/// Adding a variant is a wire-format addition: `KernelModule` is embedded in
/// `MirProviderKind::Kernel` inside `MirProvider` (both `Serialize`/
/// `Deserialize`), so new-binary ↔ old-binary image pairs must be
/// release-paired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KernelModule {
    Solum,
    Processus,
    Aleator,
    Json,
    Consolum,
    Toml,
}

impl KernelModule {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Solum => "solum",
            Self::Processus => "processus",
            Self::Aleator => "aleator",
            Self::Json => "json",
            Self::Consolum => "consolum",
            Self::Toml => "toml",
        }
    }

    #[must_use]
    pub const fn import_path(self) -> &'static str {
        match self {
            Self::Solum => "faber:solum",
            Self::Processus => "faber:processus",
            Self::Aleator => "faber:aleator",
            Self::Json => "faber:json",
            Self::Consolum => "faber:consolum",
            Self::Toml => "faber:toml",
        }
    }
}
