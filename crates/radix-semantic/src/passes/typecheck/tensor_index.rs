//! Tensor shape constraints during typechecking.
//!
//! Wires index inference to tensor intrinsics: literal shape witnesses from
//! `lista<numerus>` initializers, output-shape rules for constructors,
//! `sectio` axis-0 slicing, broadcast elementwise arithmetic, and rank-2
//! `matmul` contraction.

use crate::index::{IndexExpr, IndexId};
use crate::{SemanticErrorKind, Type, TypeId};
use radix_hir::{DefId, HirArrayElement, HirExpression, HirExpressionKind, HirLiteral};
use radix_lexer::Span;

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

const ISSUE_INCOMPATIBLE_TENSOR_INDEX: &str = "incompatible_tensor_index";
const ISSUE_VECTOR_RUNTIME_INDEX_UNSUPPORTED: &str = "vector_runtime_index_unsupported";
pub(super) const ISSUE_PROVABLY_OUT_OF_BOUNDS: &str = "index_provably_out_of_bounds";

const TENSOR_STRUE: &str = "strue";
const TENSOR_FORMA: &str = "forma";
const TENSOR_CREA: &str = "crea";
const TENSOR_GATHER: &str = "gather";
const TENSOR_SECTIO: &str = "sectio";
pub(super) const TENSOR_TRANSPONE: &str = "transpone";
pub(super) const GLYPH_TRANSPOSE_METHOD: &str = "__faber_glyph_transpose";
pub(super) const ISSUE_GLYPH_TRANSPOSE_OPERANDS_UNSUPPORTED: &str =
    "glyph_transpose_operands_unsupported";
pub(super) const ISSUE_GLYPH_TRANSPOSE_RANK1: &str =
    "glyph_transpose_rank1_no_row_column_distinction";
pub(super) const ISSUE_GLYPH_TRANSPOSE_RANK_REQUIRED: &str = "glyph_transpose_static_rank_required";
const TENSOR_ADDITA: &str = "addita";
const TENSOR_SUBTRAHE: &str = "subtrahe";
const TENSOR_MULTIPLICA: &str = "multiplica";
const TENSOR_MATMUL: &str = "matmul";
const ISSUE_MATMUL_RECEIVER_SHAPE_MUST_BE_TUPLE: &str = "matmul_receiver_shape_must_be_tuple";
const ISSUE_MATMUL_ARGUMENT_SHAPE_MUST_BE_TUPLE: &str = "matmul_argument_shape_must_be_tuple";
const ISSUE_MATMUL_RECEIVER_RANK_MUST_BE_TWO: &str = "matmul_receiver_rank_must_be_two";
const ISSUE_MATMUL_ARGUMENT_RANK_MUST_BE_TWO: &str = "matmul_argument_rank_must_be_two";

/// Broadcast elementwise tensor methods whose result shape merges receiver and argument.
fn is_tensor_broadcast_elementwise(method_name: &str) -> bool {
    matches!(
        method_name,
        TENSOR_ADDITA | TENSOR_SUBTRAHE | TENSOR_MULTIPLICA
    )
}

/// Whether a tensor intrinsic is rank-2 matrix multiply.
fn is_tensor_matmul(method_name: &str) -> bool {
    method_name == TENSOR_MATMUL
}

/// Record a compile-time shape witness when a local binds a literal numerus list.
pub(super) fn record_shape_witness_from_init(
    checker: &mut TypeChecker<'_>,
    def_id: DefId,
    init: &HirExpression,
) {
    if let Some(index) = static_numerus_list_index(checker, init) {
        checker.shape_witnesses.insert(def_id, index);
    }
}

/// The two raw dimension terms of a matrix shape, literal or parametric.
///
/// A matrix shape is always a two-dim tuple; the terms are the interned row
/// and column index expressions as written (literals or size params).
pub(super) fn matrix_shape_dims(
    checker: &TypeChecker<'_>,
    index_id: IndexId,
) -> Option<(IndexId, IndexId)> {
    let IndexExpr::Tuple(dims) = checker.types.get_index(index_id) else {
        return None;
    };
    let [rows, columns] = dims.as_slice() else {
        return None;
    };
    Some((*rows, *columns))
}

/// Tensor return type with a fresh shape hole for ops that may change rank/size.
pub(super) fn tensor_with_fresh_shape(checker: &mut TypeChecker<'_>, elem: TypeId) -> TypeId {
    let shape = checker.fresh_index_hole();
    checker.types.tensor_with_shape(elem, shape)
}

/// Return the named rank-law decline for the source-only `ᵀ` glyph, when the
/// receiver is not an admitted tensor or matrix. Rank-1 is a permanent
/// decline: there is no row/column distinction. Rank-3+ is the U3 head-axis
/// batched transpose (leading batch axes stay, the trailing `[M, N]` swaps)
/// — the `glyph_transpose_rank3_plus_deferred_batched_consumer` deferral is
/// closed by the batched-transpose consumer contract; rank 5+ fails closed
/// at the MIR transpose contract's typed rank gate.
pub(super) fn glyph_transpose_rank_issue(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
) -> Option<&'static str> {
    let resolved = checker.resolve_inferred_type(receiver_ty);
    let (_, payload) = checker.types.peel_access_marker(resolved);
    let shape = match checker.types.get(payload) {
        Type::Tensor(_, shape) => *shape,
        // S0-G9: matrices admit the glyph. A matrix shape is always a
        // two-dim tuple, so only a shape hole can decline.
        Type::Matrix(_, shape) => {
            let shape = checker.resolve_index(*shape);
            return match checker.types.get_index(shape) {
                IndexExpr::Tuple(_) => None,
                _ => Some(ISSUE_GLYPH_TRANSPOSE_RANK_REQUIRED),
            };
        }
        _ => return Some(ISSUE_GLYPH_TRANSPOSE_OPERANDS_UNSUPPORTED),
    };
    let shape = checker.resolve_index(shape);
    match checker.types.get_index(shape) {
        IndexExpr::Literal(_) => Some(ISSUE_GLYPH_TRANSPOSE_RANK1),
        IndexExpr::Tuple(dims) => match dims.len() {
            2 => None,
            0 | 1 => Some(ISSUE_GLYPH_TRANSPOSE_RANK1),
            _ => None,
        },
        IndexExpr::Param(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => {
            Some(ISSUE_GLYPH_TRANSPOSE_RANK_REQUIRED)
        }
    }
}

/// Whether a tensor intrinsic changes the result shape relative to the receiver.
pub(super) fn tensor_intrinsic_changes_shape(method_name: &str) -> bool {
    matches!(
        method_name,
        TENSOR_STRUE | TENSOR_FORMA | TENSOR_CREA | TENSOR_SECTIO | TENSOR_TRANSPONE
    ) || is_tensor_broadcast_elementwise(method_name)
        || is_tensor_matmul(method_name)
}

/// Apply index constraints after tensor intrinsic arguments have been checked.
pub(super) fn apply_tensor_intrinsic_shape_constraints(
    checker: &mut TypeChecker<'_>,
    method_name: &str,
    receiver_ty: TypeId,
    ret_ty: TypeId,
    args: &[radix_hir::HirCallArg],
    span: Span,
) {
    let Some(result_shape) = tensor_shape_id(checker, ret_ty) else {
        return;
    };

    match method_name {
        TENSOR_STRUE if args.len() >= 2 => {
            constrain_result_from_static_list(checker, result_shape, &args[1].expr, span);
        }
        TENSOR_FORMA | TENSOR_CREA if !args.is_empty() => {
            constrain_result_from_static_list(
                checker,
                result_shape,
                &args[args.len() - 1].expr,
                span,
            );
        }
        TENSOR_SECTIO if args.len() >= 2 => {
            let Some(input_shape) = tensor_shape_id(checker, receiver_ty) else {
                return;
            };
            if let (Some(start), Some(end)) = (
                static_numerus_literal(&args[0].expr),
                static_numerus_literal(&args[1].expr),
            ) && let Some(output) = sectio_axis0_shape(checker, input_shape, start, end)
            {
                checker.constrain_index_equal(result_shape, output, span);
            }
        }
        name if is_tensor_broadcast_elementwise(name) && args.len() == 1 => {
            // WHY: the argument has been typechecked by this point, so its tensor
            // shape is available. Broadcast-unify receiver and argument shapes,
            // then pin the result shape hole to the merged shape.
            let Some(arg_tensor_ty) = args[0].expr.ty else {
                return;
            };
            constrain_elementwise_result_shape(
                checker,
                receiver_ty,
                arg_tensor_ty,
                result_shape,
                span,
            );
        }
        name if is_tensor_matmul(name) && args.len() == 1 => {
            // WHY: matmul is a rank-2 contraction, not broadcast. Receiver [M,K]
            // × argument [K,N] → result [M,N]. The inner dim K must unify exactly;
            // outer dims come from receiver row count and argument column count.
            let Some(arg_tensor_ty) = args[0].expr.ty else {
                return;
            };
            constrain_matmul_result_shape(checker, receiver_ty, arg_tensor_ty, result_shape, span);
        }
        TENSOR_TRANSPONE => {
            // WHY: rank-2 transpose swaps rows and columns: [R, C] → [C, R].
            let Some(receiver_shape) = tensor_shape_id(checker, receiver_ty) else {
                return;
            };
            constrain_transpose_result_shape(checker, receiver_shape, result_shape, span);
        }
        TENSOR_GATHER if args.len() == 1 => {
            // WHY: row gather `table[V, D]` + `ids[n]` → `[n, D]`. The row
            // count is the ids tensor's own extent, so the extent travels with
            // the value instead of being recovered from a literal.
            let Some(ids_ty) = args[0].expr.ty else {
                return;
            };
            constrain_gather_result_shape(checker, receiver_ty, ids_ty, result_shape, span);
        }
        _ => {}
    }
}

/// Strictly unify two resolved tensor shape indices for type unification.
pub(super) fn unify_tensor_shape_indices(
    checker: &mut TypeChecker<'_>,
    left_shape: IndexId,
    right_shape: IndexId,
    span: Span,
) -> Result<IndexId, ()> {
    let shape_a = checker.resolve_index(left_shape);
    let shape_b = checker.resolve_index(right_shape);
    let mut unifier = checker.index_infer.unifier(checker.types);
    unifier.unify(shape_a, shape_b).map_err(|_| {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_INCOMPATIBLE_TENSOR_INDEX,
            span,
        );
    })
}

/// Constrain the rank-2 `matmul` intrinsic result shape. The intrinsic keeps
/// its existing rank-2 peer contract; the glyph's rank-1 promotion uses the
/// explicitly separate [`constrain_glyph_matmul_result_shape`] wrapper below.
pub(super) fn constrain_matmul_result_shape(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    arg_ty: TypeId,
    result_shape: IndexId,
    span: Span,
) {
    constrain_matmul_result_shape_with_rank1(
        checker,
        receiver_ty,
        arg_ty,
        result_shape,
        span,
        false,
    );
}

/// Constrain the glyph `·` result shape, admitting a rank-1 RHS `[K]` as the
/// GEMV promotion of `[K, 1]` and demoting the observable result to `[M]`.
pub(super) fn constrain_glyph_matmul_result_shape(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    arg_ty: TypeId,
    result_shape: IndexId,
    span: Span,
) {
    constrain_matmul_result_shape_with_rank1(
        checker,
        receiver_ty,
        arg_ty,
        result_shape,
        span,
        true,
    );
}

fn constrain_matmul_result_shape_with_rank1(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    arg_ty: TypeId,
    result_shape: IndexId,
    span: Span,
    allow_rank1_argument: bool,
) {
    let Some(receiver_shape) = tensor_shape_id(checker, receiver_ty) else {
        return;
    };
    let Some(arg_shape) = tensor_shape_id(checker, arg_ty) else {
        return;
    };
    let resolved_receiver = checker.resolve_index(receiver_shape);
    let resolved_arg = checker.resolve_index(arg_shape);
    let receiver_tuple = if let IndexExpr::Tuple(dims) = checker.types.get_index(resolved_receiver)
    {
        dims.clone()
    } else {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_MATMUL_RECEIVER_SHAPE_MUST_BE_TUPLE,
            span,
        );
        return;
    };
    let arg_tuple = if let IndexExpr::Tuple(dims) = checker.types.get_index(resolved_arg) {
        dims.clone()
    } else {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_MATMUL_ARGUMENT_SHAPE_MUST_BE_TUPLE,
            span,
        );
        return;
    };
    // U1 head-axis: a rank-3/4 receiver over a rank-2+ argument is the
    // batched contraction (leading head axes). The argument's leading
    // batch dims must be a PREFIX of the receiver's (the GQA share: K/V
    // per kv-head broadcast over q-per-kv heads); the result carries the
    // receiver's full batch dims over `[M, N]`. The rank-1 GEMV promotion
    // stays rank-2-only — a batched contraction requires a trailing
    // `[K, N]` matrix on the argument.
    if receiver_tuple.len() >= 3 && receiver_tuple.len() <= 4 {
        constrain_batched_matmul_result_shape(
            checker,
            &receiver_tuple,
            &arg_tuple,
            result_shape,
            span,
        );
        return;
    }
    if receiver_tuple.len() != 2 {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_MATMUL_RECEIVER_RANK_MUST_BE_TWO,
            span,
        );
        return;
    }
    let (argument_k, result_rank1) = match arg_tuple.as_slice() {
        [argument_k] if allow_rank1_argument => (*argument_k, true),
        [argument_k, _] => (*argument_k, false),
        _ => {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_MATMUL_ARGUMENT_RANK_MUST_BE_TWO,
                span,
            );
            return;
        }
    };
    // WHY: enforce K (receiver cols) == K (argument rows) before publishing the
    // result shape. A failed inner-dimension check must not also let downstream
    // constraints treat the contraction as valid.
    if unify_tensor_shape_indices(checker, receiver_tuple[1], argument_k, span).is_err() {
        return;
    }
    // WHY: the rank-1 argument is promoted to [K, 1] for the tiled recipe, but
    // its observable GEMV result is demoted back to [M]. Rank-2 matmul retains
    // the conventional [M, N] result shape.
    let m_expr = checker.types.get_index(receiver_tuple[0]).clone();
    let m_id = checker.types.intern_index(m_expr);
    let result_dims = if result_rank1 {
        vec![m_id]
    } else {
        let n_expr = checker.types.get_index(arg_tuple[1]).clone();
        let n_id = checker.types.intern_index(n_expr);
        vec![m_id, n_id]
    };
    let result = checker.types.intern_index(IndexExpr::Tuple(result_dims));
    checker.constrain_index_equal(result_shape, result, span);
}

/// Constrain the batched matmul result shape (U1 head-axis): receiver
/// `[B…, M, K]` × argument `[b…, K, N]` → `[B…, M, N]`, where the
/// argument's batch dims `b…` are a PREFIX of the receiver's `B…`. A
/// prefix-axis mismatch, a too-high argument rank, or a rank-1 argument
/// reuses the existing typed matmul issues (`incompatible_tensor_index`
/// from the unifier, or the rank codes) — no new locale-backed codes.
fn constrain_batched_matmul_result_shape(
    checker: &mut TypeChecker<'_>,
    receiver_tuple: &[IndexId],
    arg_tuple: &[IndexId],
    result_shape: IndexId,
    span: Span,
) {
    if arg_tuple.len() < 2 || arg_tuple.len() > receiver_tuple.len() {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_MATMUL_ARGUMENT_RANK_MUST_BE_TWO,
            span,
        );
        return;
    }
    let batch_axis_count = arg_tuple.len() - 2;
    // WHY: unify each shared leading axis exactly, like the rank-2 inner-dim
    // check — a failed batch-axis unification must not let downstream
    // constraints treat the contraction as valid (the unifier emits
    // `incompatible_tensor_index`).
    for axis in 0..batch_axis_count {
        if unify_tensor_shape_indices(checker, receiver_tuple[axis], arg_tuple[axis], span).is_err()
        {
            return;
        }
    }
    let receiver_rows = receiver_tuple[receiver_tuple.len() - 2];
    let receiver_cols = receiver_tuple[receiver_tuple.len() - 1];
    let argument_rows = arg_tuple[arg_tuple.len() - 2];
    let argument_cols = arg_tuple[arg_tuple.len() - 1];
    if unify_tensor_shape_indices(checker, receiver_cols, argument_rows, span).is_err() {
        return;
    }
    // The result carries the receiver's batch dims verbatim (the argument's
    // shorter batch prefix broadcast-shares them), then `[M, N]`.
    let mut result_dims = receiver_tuple[..receiver_tuple.len() - 2].to_vec();
    result_dims.push(receiver_rows);
    result_dims.push(argument_cols);
    let result = checker.types.intern_index(IndexExpr::Tuple(result_dims));
    checker.constrain_index_equal(result_shape, result, span);
}

/// Constrain the `gather` result shape: table `[.., D]` and ids `[n]` give
/// `[n, D]`. Shape holes on either operand leave the result unconstrained.
fn constrain_gather_result_shape(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    ids_ty: TypeId,
    result_shape: IndexId,
    span: Span,
) {
    let Some(receiver_shape) = tensor_shape_id(checker, receiver_ty) else {
        return;
    };
    let Some(ids_shape) = tensor_shape_id(checker, ids_ty) else {
        return;
    };
    let resolved_receiver = checker.resolve_index(receiver_shape);
    let resolved_ids = checker.resolve_index(ids_shape);
    let IndexExpr::Tuple(receiver_dims) = checker.types.get_index(resolved_receiver).clone() else {
        return;
    };
    let IndexExpr::Tuple(ids_dims) = checker.types.get_index(resolved_ids).clone() else {
        return;
    };
    let [_, .., columns] = receiver_dims.as_slice() else {
        return;
    };
    let [rows] = ids_dims.as_slice() else {
        return;
    };
    let result = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![*rows, *columns]));
    checker.constrain_index_equal(result_shape, result, span);
}

/// Constrain `result_shape` to the transpose of `receiver`'s shape.
///
/// For a rank-2 receiver [R, C], the result is [C, R]. For rank-1, it is
/// identity. For rank-3+ (U3 head-axis batched transpose), the leading
/// batch axes stay verbatim and the trailing `[M, N]` pair swaps:
/// `[B…, M, N] → [B…, N, M]`. Shape holes are preserved.
fn constrain_transpose_result_shape(
    checker: &mut TypeChecker<'_>,
    receiver_shape: IndexId,
    result_shape: IndexId,
    span: Span,
) {
    let resolved = checker.resolve_index(receiver_shape);
    let receiver_tuple = if let IndexExpr::Tuple(dims) = checker.types.get_index(resolved) {
        dims.clone()
    } else {
        // Shape holes pass through: leave result unconstrained.
        return;
    };
    if receiver_tuple.len() < 2 {
        // Transpose of a rank-0/1 tensor is a no-op; pin result to receiver.
        checker.constrain_index_equal(result_shape, receiver_shape, span);
        return;
    }
    // Swap the trailing pair; the leading batch axes (if any) stay
    // verbatim — the batched transpose law.
    let rows = checker
        .types
        .get_index(receiver_tuple[receiver_tuple.len() - 2])
        .clone();
    let columns = checker
        .types
        .get_index(receiver_tuple[receiver_tuple.len() - 1])
        .clone();
    let rows_id = checker.types.intern_index(rows);
    let columns_id = checker.types.intern_index(columns);
    let mut result_dims = receiver_tuple[..receiver_tuple.len() - 2].to_vec();
    result_dims.push(columns_id);
    result_dims.push(rows_id);
    let result = checker.types.intern_index(IndexExpr::Tuple(result_dims));
    checker.constrain_index_equal(result_shape, result, span);
}

/// Pin `result_shape` to the broadcast merge of `receiver_ty` and `arg_ty` shapes.
///
/// Reuses the broadcast constraint solver; element strict-equality is already
/// enforced by the `TensorPeer` parameter resolution against the argument.
/// Shared by the `addita` method and numeric tensor `+` so both surfaces
/// publish the same `Figura'` and cannot diverge.
pub(super) fn constrain_elementwise_result_shape(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    arg_ty: TypeId,
    result_shape: IndexId,
    span: Span,
) {
    let Some(receiver_shape) = tensor_shape_id(checker, receiver_ty) else {
        return;
    };
    let Some(arg_shape) = tensor_shape_id(checker, arg_ty) else {
        return;
    };
    if let Some(merged) = checker.constrain_index_broadcast(receiver_shape, arg_shape, span) {
        checker.constrain_index_equal(result_shape, merged, span);
    }
}

fn tensor_parts(checker: &mut TypeChecker<'_>, ty: TypeId) -> Option<(TypeId, IndexId)> {
    let resolved = checker.resolve_inferred_type(ty);
    match checker.types.get(resolved) {
        Type::Tensor(elem, shape) => Some((*elem, *shape)),
        _ => None,
    }
}

fn tensor_shape_id(checker: &mut TypeChecker<'_>, ty: TypeId) -> Option<IndexId> {
    tensor_parts(checker, ty).map(|(_, shape)| shape)
}

fn constrain_result_from_static_list(
    checker: &mut TypeChecker<'_>,
    result_shape: IndexId,
    arg_expr: &HirExpression,
    span: Span,
) {
    if let Some(arg_shape) = static_numerus_list_index(checker, arg_expr) {
        checker.constrain_index_equal(result_shape, arg_shape, span);
    }
}

/// Reject a literal index that contradicts a receiver's literal static bound.
///
/// This is deliberately literal-only. Index parameters, inference holes,
/// dynamic shapes, and arithmetic expressions all decline here and remain
/// runtime policy. The diagnostic span is the index expression itself so the
/// source location identifies the proven failure.
pub(super) fn reject_static_bounded_index_oob(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    index: &HirExpression,
) {
    let resolved = checker.resolve_inferred_type(receiver_ty);
    let (_, payload) = checker.types.peel_access_marker(resolved);
    let receiver = checker.types.get(payload).clone();
    let bounds = match receiver {
        Type::BoundedArray { capacity, .. }
        | Type::BoundedTextus { capacity }
        | Type::BoundedAscii { capacity }
        | Type::BoundedOcteti { capacity } => {
            let capacity = checker.resolve_index(capacity);
            let IndexExpr::Literal(value) = checker.types.get_index(capacity) else {
                return;
            };
            vec![*value]
        }
        Type::Tensor(_, shape) => static_tensor_bounds(checker, shape),
        _ => return,
    };
    let Some(indices) = static_index_literals(index) else {
        return;
    };
    if indices.len() != bounds.len() {
        return;
    }
    for (axis, (value, bound)) in indices.iter().zip(bounds).enumerate() {
        if *value < 0 || u128::try_from(*value).map_or(true, |value| value >= u128::from(bound)) {
            checker.invalid_operand_types_with_args(
                ISSUE_PROVABLY_OUT_OF_BOUNDS,
                index.span,
                &[
                    ("index", format_static_index(&indices)),
                    ("bound", bound.to_string()),
                    ("axis", axis.to_string()),
                ],
            );
            return;
        }
    }
}

/// Check the index of a vector lane access `v[k]`.
///
/// A vector is a register value, so a lane read is a constant projection: the
/// index must be a compile-time integer constant, and a constant outside the
/// literal width is provably out of bounds (the shared index-bounds family).
/// A runtime index is a named unsupported diagnostic until the checked lane
/// select lands. A symbolic width cannot be bounds-checked here; the lowered
/// projection is validated against the concrete width.
pub(super) fn check_vector_lane_index(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    index: &HirExpression,
) {
    let resolved = checker.resolve_inferred_type(receiver_ty);
    let (_, payload) = checker.types.peel_access_marker(resolved);
    let Type::Vector(_, width) = checker.types.get(payload).clone() else {
        return;
    };
    let Some(lane) = crate::passes::const_int_value(index) else {
        checker.invalid_operand_types(ISSUE_VECTOR_RUNTIME_INDEX_UNSUPPORTED, index.span);
        return;
    };
    let width = checker.resolve_index(width);
    let IndexExpr::Literal(bound) = checker.types.get_index(width) else {
        return;
    };
    let bound = *bound;
    if lane < 0 || u128::try_from(lane).map_or(true, |lane| lane >= u128::from(bound)) {
        checker.invalid_operand_types_with_args(
            ISSUE_PROVABLY_OUT_OF_BOUNDS,
            index.span,
            &[
                ("index", lane.to_string()),
                ("bound", bound.to_string()),
                ("axis", "0".to_owned()),
            ],
        );
    }
}

fn static_tensor_bounds(checker: &mut TypeChecker<'_>, shape: IndexId) -> Vec<u64> {
    let shape = checker.resolve_index(shape);
    match checker.types.get_index(shape).clone() {
        IndexExpr::Literal(value) => vec![value],
        IndexExpr::Tuple(dims) => dims
            .into_iter()
            .map(|dim| match checker.types.get_index(dim) {
                IndexExpr::Literal(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default(),
        IndexExpr::Param(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => Vec::new(),
    }
}

fn static_index_literals(expr: &HirExpression) -> Option<Vec<i128>> {
    match &expr.kind {
        HirExpressionKind::Array(elements) => elements
            .iter()
            .map(|element| match element {
                HirArrayElement::Expr(inner) => static_signed_numerus_literal(inner),
                HirArrayElement::Spread(_) => None,
            })
            .collect(),
        _ => Some(vec![static_signed_numerus_literal(expr)?]),
    }
}

fn static_signed_numerus_literal(expr: &HirExpression) -> Option<i128> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => Some(i128::from(*value)),
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => {
            static_signed_numerus_literal(inner)?.checked_neg()
        }
        _ => None,
    }
}

fn format_static_index(indices: &[i128]) -> String {
    if indices.len() == 1 {
        return indices[0].to_string();
    }
    let values = indices.iter().map(ToString::to_string).collect::<Vec<_>>();
    format!("[{}]", values.join(", "))
}

fn static_numerus_literal(expr: &HirExpression) -> Option<u64> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => Some(*value),
        _ => None,
    }
}

fn static_numerus_list_index(
    checker: &mut TypeChecker<'_>,
    expr: &HirExpression,
) -> Option<IndexId> {
    match &expr.kind {
        HirExpressionKind::Array(elements) => {
            let mut dims = Vec::with_capacity(elements.len());
            for element in elements {
                let HirArrayElement::Expr(inner) = element else {
                    return None;
                };
                dims.push(
                    checker
                        .types
                        .intern_index(IndexExpr::Literal(static_numerus_literal(inner)?)),
                );
            }
            Some(checker.types.intern_index(IndexExpr::Tuple(dims)))
        }
        HirExpressionKind::Path(def_id) => checker.shape_witnesses.get(def_id).copied(),
        _ => None,
    }
}

/// Output shape for axis-0 `sectio(start, end)` when input rank and bounds are static.
fn sectio_axis0_shape(
    checker: &mut TypeChecker<'_>,
    input_shape: IndexId,
    start: u64,
    end: u64,
) -> Option<IndexId> {
    if end < start {
        return None;
    }
    let resolved = checker.resolve_index(input_shape);
    let IndexExpr::Tuple(dims) = checker.types.get_index(resolved).clone() else {
        return None;
    };
    if dims.is_empty() {
        return None;
    }
    let sliced = end - start;
    let mut output = vec![checker.types.intern_index(IndexExpr::Literal(sliced))];
    output.extend_from_slice(&dims[1..]);
    Some(checker.types.intern_index(IndexExpr::Tuple(output)))
}
