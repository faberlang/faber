//! Recipe-op classification.

use crate::{MirCollectionOp, MirUnOp};
use radix_types::MirMathFunction;

// ---------------------------------------------------------------------------
// RECIPE CLASSIFICATION
// ---------------------------------------------------------------------------

/// Whether a collection op is a pointwise tensor transform the generic
/// elementwise kernel body covers (no recipe). This is the closed set that
/// decides the no-recipe plan: an op outside it has no kernel plan and fails
/// closed with a typed diagnostic (N3.2 — never a silent `Elementwise`).
///
/// The set is grounded in the emitters' elementwise arms and the reverse-AD
/// op surface the companion materializer consumes (elementwise
/// mul/div/fill): every member is a uniform pointwise transform.
pub(crate) fn is_elementwise_collection_op(op: MirCollectionOp) -> bool {
    matches!(
        op,
        MirCollectionOp::TensorAdd
            | MirCollectionOp::TensorSub
            | MirCollectionOp::TensorMul
            | MirCollectionOp::TensorDiv
            | MirCollectionOp::TensorNeg
            | MirCollectionOp::TensorFill
            // Explicit static expansion is a reindexed tensor expression.
            // Fusion remains a typed barrier until its planner represents reindexing.
            | MirCollectionOp::TensorExpand
            | MirCollectionOp::TensorRelu
            | MirCollectionOp::TensorGelu
            // The math functions the Metal elementwise lane lowers per
            // element (`t.sqrt()` / `t.exp()` — the LayerNorm sqrt lane and
            // the softmax/sigmoid exp lane as author-level calls). The other
            // `MirMathFunction` members have no elementwise emitter arm and
            // stay unplannable (the typed refusal above).
            | MirCollectionOp::TensorMath(MirMathFunction::Sqrt | MirMathFunction::Exp)
    )
}

/// Whether a collection op is a recipe-bearing op — the closed recipe-op
/// set the plan pass resolves a single [`CollectionKernelPlan`] for under
/// its dispatch contract (S5-U1: reduction, matmul, and the new transpose;
/// S6-P1: the transformer recipes `TensorSumAxis` / `TensorSoftmax` /
/// `TensorLayerNorm`).
///
/// This is the shared boundary predicate for the single-plan scan
/// ([`kernel_plan_for_function`]) and the decomposition slice
/// ([`crate::kernel_decomposition::decompose_kernel_function`]): every op
/// outside it is either an elementwise transform
/// ([`is_elementwise_collection_op`]) or unplannable (a typed diagnostic).
#[must_use]
pub fn is_recipe_op(op: MirCollectionOp) -> bool {
    matches!(
        op,
        MirCollectionOp::Sum
            | MirCollectionOp::TensorMean
            | MirCollectionOp::TensorMax
            | MirCollectionOp::TensorMin
            | MirCollectionOp::TensorMatMul
            | MirCollectionOp::TensorTranspose
            | MirCollectionOp::TensorSumAxis
            | MirCollectionOp::TensorSoftmax
            | MirCollectionOp::TensorLayerNorm
            // GI3-1: the pinned-row inference recipes are recipe boundaries
            // like the transformer ops (admission/resolution is GI3-1).
            | MirCollectionOp::TensorRmsNorm
            | MirCollectionOp::TensorRope
            | MirCollectionOp::TensorRopeHalf
            | MirCollectionOp::TensorGather
            | MirCollectionOp::TensorCausalMaskedSoftmax
            | MirCollectionOp::TensorQuantizedMatMul
            | MirCollectionOp::TensorGroupedMatMul
            | MirCollectionOp::TensorBatchedAttention
            | MirCollectionOp::TensorSsmConv1d
            | MirCollectionOp::TensorSsmScan
    )
}

/// Whether a collection op is one of the S6-P1 transformer recipes — the
/// recipe ops resolved from the function's typed facts by
/// [`CollectionKernelPlan::from_transformer_op`] (no `CollectionOpContract`
/// variant exists for them, so the plan reads the trailing const operands
/// and the operand tensor dims directly).
#[must_use]
pub fn is_transformer_recipe_op(op: MirCollectionOp) -> bool {
    matches!(
        op,
        MirCollectionOp::TensorSumAxis
            | MirCollectionOp::TensorSoftmax
            | MirCollectionOp::TensorLayerNorm
            // GI3-1: the inference recipes follow the same
            // `from_transformer_op`-sibling admission path (no
            // `CollectionOpContract` variant exists for them either).
            | MirCollectionOp::TensorRmsNorm
            | MirCollectionOp::TensorRope
            | MirCollectionOp::TensorRopeHalf
            | MirCollectionOp::TensorGather
            | MirCollectionOp::TensorCausalMaskedSoftmax
    )
}

/// Whether a collection op is a contract-shaped recipe (reduction, matmul,
/// or transpose). Transformer recipes have no `CollectionOpContract` variant
/// and resolve through [`CollectionKernelPlan::from_transformer_op`].
pub(super) fn is_contract_recipe_op(op: MirCollectionOp) -> bool {
    is_recipe_op(op) && !is_transformer_recipe_op(op)
}

/// Whether a tensor-level unary assign (`MirValueKind::Unary` on a
/// tensor-typed operand) is admitted in the device elementwise surface —
/// the MIR-level admission seam declared by `S6-P1` for the emitter units
/// (`S6-P2`/`S6-P3`) to dispatch tensor-unary emission from.
///
/// `Exp` and `Sqrt` are the `S6-P1` transformer additions (the softmax exp
/// and the `LayerNorm` sqrt/rsqrt lanes); the pre-existing elementwise tensor
/// unaries (`Tanh` / `Relu` / `Gelu`) stay admitted alongside. A unary
/// outside this set on a tensor operand has no elementwise arm and stays
/// fail-closed at the emitters — never a silent fallback.
#[must_use]
pub fn is_admitted_tensor_unary(op: MirUnOp) -> bool {
    matches!(
        op,
        MirUnOp::Tanh | MirUnOp::Relu | MirUnOp::Gelu | MirUnOp::Exp | MirUnOp::Sqrt
            // GI3-1 amendment: the device K/V f16 rounding
            // unary (the comparator's `kv_f16_round` register rounding).
            | MirUnOp::F16Round
    )
}
