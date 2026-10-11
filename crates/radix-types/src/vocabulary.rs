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

/// How `@ kernel` (and nucleum fragment) functions lower.
///
/// `Device` is the form the device translators consume: device views for
/// tensors, device-checked arithmetic, `⇇` as the output-buffer store,
/// kernel-to-kernel calls composed (inlined) into their callers, and the
/// `filum` reduction recipe. `Host` lowers every kernel exactly as an
/// ordinary function: tensor brackets are `TensorGet`/`TensorSet`, integer
/// and vector-lane stores keep the host store checks, `⇇` keeps its
/// storage-preserving host copy, a call to a helper kernel stays a plain
/// call, and a `thread` reduction is the sequential fold. The CPU route
/// (`faber run --device cpu`) selects `Host`; the selection is a property of
/// the route, never of a source name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KernelLowering {
    /// Device-translatable form (the default).
    #[default]
    Device,
    /// Ordinary host-function form, run by the MIR runner on the CPU.
    Host,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Elementwise `%` over a `lista`: the scalar remainder per element.
    ArrayMod,
    /// Elementwise `÷` over a `lista`: integers convert to the result float
    /// first (ruling 23); the result element type is that float.
    ArrayTrueDiv,
    /// Elementwise unary `-` over a `lista`: one operand.
    ArrayNeg,
    /// Elementwise ordering over a numeric `lista` (`≺ ≻ ≤ ≥`): two operands,
    /// a list pair or a list and a scalar in either order; the result is a
    /// `lista<bivalens>` of the live length.
    ArrayLt,
    ArrayGt,
    ArrayLtEq,
    ArrayGtEq,
    /// Elementwise value equality over a numeric `lista` (`≅ ≇`): the same
    /// pairing and result as the orderings.
    ArrayValueEq,
    ArrayValueNotEq,
    /// Elementwise `et` / `aut` over a `lista<bivalens>` (Q-C of the
    /// 2026-10-04 comparisons ruling): two operands, a list pair or a list and
    /// a scalar bool in either order; both sides are always evaluated.
    ArrayAnd,
    ArrayOr,
    /// Elementwise `non` over a `lista<bivalens>`: one operand.
    ArrayNot,
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
    /// Stable sort of a `lista` by a parallel `lista` of keys (D1.7
    /// `ordina(key)`): args `[receiver, keys]`, same length; keys are
    /// numbers or `textus`. Returns the reordered receiver elements.
    SortByKeys,
    /// Three-way compare through the ordering contract (D1.4): args
    /// `[a, b]` of one conforming built-in or tuple type; returns `numerus`
    /// negative, zero, or positive. `fractus` uses IEEE 754 totalOrder,
    /// `textus` code-point order, tuples lexicographic order.
    Compare,
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
    /// Root-mean-square normalization (the CCI-1 `rms_norm` source contract):
    /// `y = x·(1/sqrt(mean(x²)+eps))·gamma`,
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
    /// operand (`[x, ids]`); out-of-range/negative *literal* ids fail closed
    /// at the source surface (no silent clamping), and the emitted kernel
    /// body bounds-checks a *runtime* token against the table row count
    /// (out of range ⇒ zero output, never an out-of-bounds table read).
    /// Shape-transforming — not `TensorGet`/`TensorSlice`.
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
    /// Elementwise built-in math function lifted over a tensor (gpu-reset
    /// rule 15, K5): `[x]` → a tensor of the receiver's shape. Each element
    /// is the scalar function of the same name. Values are IEEE 754 per
    /// element: a domain miss is NaN, an overflow is infinity, and the result
    /// is stored under the result element type. This is NOT the trapping
    /// device-domain policy of the MIR unary `Exp`/`Sqrt`/`Log` ops.
    TensorMath(MirMathFunction),
    /// Built-in math function on a scalar float (`x.sqrt()`, `x.sin()`):
    /// `[x]` → a float of the receiver's type. The function is the same
    /// [`MirMathFunction`] the tensor lift applies per element, with the same
    /// IEEE semantics (a domain miss is NaN, an overflow is infinity); the
    /// result is rounded to the result width only where it is stored. Scalar
    /// `abs` is the existing `Absolute` op, so `Abs` is never carried here.
    ScalarMath(MirMathFunction),
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
    /// Regex `inventa` (en `find`): the leftmost-first match of the `regex`
    /// receiver in a `textus` argument, as `match ∪ nihil`. Offsets are code
    /// points (RX-3b).
    RegexFind,
    /// Match `inventum` (en `text`): the matched text.
    MatchText,
    /// Match `principium` (en `start`): inclusive start, in code points.
    MatchStart,
    /// Match `terminus` (en `end`): exclusive end, in code points.
    MatchEnd,
    /// Match `coetus` (en `group`): capture group `n` text, `0` the whole
    /// match; `nihil` for a non-participating or out-of-range group.
    MatchGroup,
    /// Match `nominatus` (en `named`): text of a `(?P<name>…)` group; `nihil`
    /// for a non-participating or unknown name.
    MatchNamed,
    /// Regex `collecta` (en `find_all`): every non-overlapping match of the
    /// `regex` receiver in a `textus` argument, as `lista<match>`, in one pass
    /// with RE2's empty-match iteration rule (RX-4a).
    RegexFindAll,
    /// Regex `divide` (en `split`): the pieces of a `textus` argument between
    /// the iteration's matches, as `lista<textus>`; leading and trailing empty
    /// pieces are kept and capture groups are not spliced (RX-4a).
    RegexSplit,
    /// Regex `muta` (en `replace`): every non-overlapping match of the `regex`
    /// receiver in a `textus` argument replaced by a `textus` replacement taken
    /// literally (`$1`, `${name}` and `\1` stay text), as `textus` (RX-4b).
    RegexReplace,
    /// Text `munita` (en `escape`): the `textus` receiver as a pattern text that
    /// matches it literally, escaping exactly the dialect's metacharacters
    /// (RX-4b).
    TextEscape,
    /// Regex closure `muta` (RX-5): operands `[regex, text, replacements]`
    /// where `replacements` is the `lista<textus>` of closure results, one per
    /// match of the receiver in the text in left-to-right order (the same
    /// iteration as `RegexFindAll`); returns the text with the i-th match
    /// replaced by the i-th replacement, each taken literally and never
    /// re-scanned. The closure itself runs in the lowered loop, not here.
    RegexReplaceEach,
    /// Cord walk: `ascii.cursor()` / later `textus.cursor()` → `cursor<ascii<1>>`.
    /// One buffer of one-byte ascii values; not a per-step heap slice.
    CordCursor,
    /// Explicit zero-copy tensor broadcast view. `args` are the source tensor
    /// followed by its target extents; axes map in order, with inserted or
    /// stretched size-one axes carried by zero strides.
    TensorExpand,
    /// Lazily shifted tensor edge view. `args` are `[source, offsets]`; an
    /// unresolvable read remains optional until `TensorLimes` or fused coalesce.
    TensorShift,
    /// `target ⇇ value` copy-into: `args` are `[dest, src]`, returns `vacuum`.
    /// Copies the logical values of `src` through the destination's strides
    /// into the destination's existing storage; the destination's storage
    /// identity is preserved. Receiver-mutating: neither `TensorSet` (one
    /// element) nor `TensorFill` (tiles, returns a new tensor) fits.
    TensorCopyInto,
}

/// The built-in unary math functions: methods on a scalar float and, lifted
/// elementwise, on a tensor (gpu-reset rule 15). One member per function; the
/// scalar meaning of each is the per-element meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirMathFunction {
    /// `absolutum` (en `abs`): magnitude. Integer elements are exact.
    Abs,
    /// `radix` (en `sqrt`): square root. `sqrt(x < 0)` is NaN.
    Sqrt,
    /// `exponentia` (en `exp`): `e^x`. Overflow is infinity.
    Exp,
    /// `logarithmus` (en `ln`): natural logarithm. `ln(0)` is `-inf`,
    /// `ln(x < 0)` is NaN.
    Ln,
    /// `logarithmus_decimalis` (en `log10`): base-10 logarithm.
    Log10,
    /// `sinus` (en `sin`).
    Sin,
    /// `cosinus` (en `cos`).
    Cos,
    /// `tangens` (en `tan`).
    Tan,
}

impl MirMathFunction {
    /// Every lifted function, in catalog order.
    pub const ALL: [Self; 8] = [
        Self::Abs,
        Self::Sqrt,
        Self::Exp,
        Self::Ln,
        Self::Log10,
        Self::Sin,
        Self::Cos,
        Self::Tan,
    ];

    /// Stable lowercase name used by MIR dumps and target import names.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Abs => "abs",
            Self::Sqrt => "sqrt",
            Self::Exp => "exp",
            Self::Ln => "ln",
            Self::Log10 => "log10",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
        }
    }

    /// The canonical Latin method name of the function (the registry key).
    #[must_use]
    pub const fn latin_name(self) -> &'static str {
        match self {
            Self::Abs => "absolutum",
            Self::Sqrt => "radix",
            Self::Exp => "exponentia",
            Self::Ln => "logarithmus",
            Self::Log10 => "logarithmus_decimalis",
            Self::Sin => "sinus",
            Self::Cos => "cosinus",
            Self::Tan => "tangens",
        }
    }

    /// The function a canonical Latin method name spells, if any.
    #[must_use]
    pub fn from_latin_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|function| function.latin_name() == name)
    }

    /// The functions that are scalar float methods through
    /// [`MirCollectionOp::ScalarMath`]: everything except `Abs`, whose scalar
    /// form is the shared numeric `Absolute` op.
    pub const SCALAR: [Self; 7] = [
        Self::Sqrt,
        Self::Exp,
        Self::Ln,
        Self::Log10,
        Self::Sin,
        Self::Cos,
        Self::Tan,
    ];

    /// Whether the function is defined only on floating-point elements
    /// (everything except `abs`, which also accepts integers).
    #[must_use]
    pub const fn float_only(self) -> bool {
        !matches!(self, Self::Abs)
    }
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
/// [`CapabilityGap::RuntimePanic`] is the exception: it reports a crash in the
/// classification run itself, not a verdict about the target.
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
    /// The classification run crashed — the runner panicked with a payload
    /// that is not one of the harness's own traps. This is a defect signal:
    /// "this target broke", never "this target cannot do it". Consumers must
    /// fail the row rather than count it as a capability refusal.
    RuntimePanic { shape: String, slug: &'static str },
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
            | Self::TargetPolicyRejection { shape, .. }
            | Self::RuntimePanic { shape, .. } => shape,
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
            | Self::TargetPolicyRejection { slug, .. }
            | Self::RuntimePanic { slug, .. } => slug,
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
