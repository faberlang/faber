//! Tensor shape constraints during typechecking.
//!
//! Wires index inference to tensor intrinsics: literal shape witnesses from
//! `lista<numerus>` initializers, output-shape rules for constructors,
//! `sectio` axis-0 slicing, explicit tensor expansion, broadcast elementwise arithmetic, and rank-2
//! `matmul` contraction.

use crate::index::{IndexExpr, IndexId};
use crate::{SemanticErrorKind, Type, TypeId};
use radix_hir::{
    DefId, HirArrayElement, HirBinOp, HirExpression, HirExpressionKind, HirLiteral, HirRangeKind,
};
use radix_lexer::Span;

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

const ISSUE_INCOMPATIBLE_TENSOR_INDEX: &str = "incompatible_tensor_index";
const ISSUE_UNPROVEN_KERNEL_INDEX: &str = "unproven_kernel_index";
pub(super) const ISSUE_PROVABLY_OUT_OF_BOUNDS: &str = "index_provably_out_of_bounds";
const ISSUE_BRACKET_RANGE_STATIC_BOUNDS_REQUIRED: &str = "bracket_range_static_bounds_required";

const TENSOR_FORMA: &str = "forma";
const TENSOR_GATHER: &str = "gather";
const TENSOR_SECTIO: &str = "sectio";
const TENSOR_EXPANDE: &str = "expande";
pub(super) const TENSOR_TRANSPONE: &str = "transpone";
pub(super) const GLYPH_TRANSPOSE_METHOD: &str = "__faber_glyph_transpose";
pub(super) const ISSUE_GLYPH_TRANSPOSE_OPERANDS_UNSUPPORTED: &str =
    "glyph_transpose_operands_unsupported";
pub(super) const ISSUE_GLYPH_TRANSPOSE_RANK1: &str =
    "glyph_transpose_rank1_no_row_column_distinction";
pub(super) const ISSUE_GLYPH_TRANSPOSE_RANK_REQUIRED: &str = "glyph_transpose_static_rank_required";
const TENSOR_ADDITA: &str = "addita";
const TENSOR_SUBTRAHE: &str = "subtracta";
const TENSOR_MULTIPLICA: &str = "multiplicata";
const TENSOR_DIVISIO: &str = "divisa";
const TENSOR_MATMUL: &str = "matmul";
const ISSUE_MATMUL_RECEIVER_SHAPE_MUST_BE_TUPLE: &str = "matmul_receiver_shape_must_be_tuple";
const ISSUE_MATMUL_ARGUMENT_SHAPE_MUST_BE_TUPLE: &str = "matmul_argument_shape_must_be_tuple";
const ISSUE_MATMUL_RECEIVER_RANK_MUST_BE_TWO: &str = "matmul_receiver_rank_must_be_two";
const ISSUE_MATMUL_ARGUMENT_RANK_MUST_BE_TWO: &str = "matmul_argument_rank_must_be_two";

/// Broadcast elementwise tensor methods whose result shape merges receiver and argument.
fn is_tensor_broadcast_elementwise(method_name: &str) -> bool {
    matches!(
        method_name,
        TENSOR_ADDITA | TENSOR_SUBTRAHE | TENSOR_MULTIPLICA | TENSOR_DIVISIO
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
    if let Some(length) = static_collection_length(checker, init) {
        checker.static_collection_lengths.insert(def_id, length);
    }
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
    tensor_with_fresh_shape_hole(checker, elem).0
}

/// Like [`tensor_with_fresh_shape`], also returning the fresh shape hole.
pub(super) fn tensor_with_fresh_shape_hole(
    checker: &mut TypeChecker<'_>,
    elem: TypeId,
) -> (TypeId, IndexId) {
    let shape = checker.fresh_index_hole();
    (checker.types.tensor_with_shape(elem, shape), shape)
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
        IndexExpr::Literal(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Affine { .. }
        | IndexExpr::Divide { .. } => Some(ISSUE_GLYPH_TRANSPOSE_RANK1),
        IndexExpr::Tuple(dims) => match dims.len() {
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
        TENSOR_FORMA | TENSOR_SECTIO | TENSOR_EXPANDE | TENSOR_TRANSPONE
    ) || is_tensor_broadcast_elementwise(method_name)
        || is_tensor_matmul(method_name)
}

/// Apply index constraints after tensor intrinsic arguments have been checked.
pub(super) fn apply_tensor_intrinsic_shape_constraints(
    checker: &mut TypeChecker<'_>,
    method_name: &str,
    receiver: &HirExpression,
    receiver_ty: TypeId,
    ret_ty: TypeId,
    args: &[radix_hir::HirCallArg],
    span: Span,
) {
    let Some(result_shape) = tensor_shape_id(checker, ret_ty) else {
        return;
    };

    match method_name {
        TENSOR_FORMA if !args.is_empty() => {
            constrain_reshape_result_shape(checker, receiver_ty, result_shape, &args[0].expr, span);
        }
        TENSOR_SECTIO if args.len() >= 2 => {
            constrain_sectio_result_shape(checker, receiver_ty, result_shape, args, span);
        }
        TENSOR_EXPANDE if args.len() == 1 => {
            constrain_expanded_result_shape(
                checker,
                receiver,
                receiver_ty,
                result_shape,
                &args[0].expr,
                span,
            );
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

/// Axis-0 `sectio(start, end[, step])` result shape when the receiver rank and
/// bounds are static. A non-literal bound or an empty/zero-step range leaves the
/// result shape unconstrained.
fn constrain_sectio_result_shape(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    result_shape: IndexId,
    args: &[radix_hir::HirCallArg],
    span: Span,
) {
    let Some(input_shape) = tensor_shape_id(checker, receiver_ty) else {
        return;
    };
    let (Some(start), Some(end)) = (
        static_numerus_literal(&args[0].expr),
        static_numerus_literal(&args[1].expr),
    ) else {
        return;
    };
    let step = match args.get(2) {
        Some(step) => {
            let Some(step) = static_numerus_literal(&step.expr) else {
                return;
            };
            step
        }
        None => 1,
    };
    if let Some(output) = sectio_axis0_shape(checker, input_shape, start, end, step) {
        checker.constrain_index_equal(result_shape, output, span);
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

/// Destructure a resolved matmul operand shape as its dim tuple. A rank-agnostic
/// shape (whole-shape param or hole) emits the caller's typed issue instead.
fn matmul_operand_tuple(
    checker: &mut TypeChecker<'_>,
    resolved_shape: IndexId,
    issue: &'static str,
    span: Span,
) -> Option<Vec<IndexId>> {
    if let IndexExpr::Tuple(dims) = checker.types.get_index(resolved_shape) {
        Some(dims.clone())
    } else {
        checker.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
        None
    }
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
    let Some(receiver_tuple) = matmul_operand_tuple(
        checker,
        resolved_receiver,
        ISSUE_MATMUL_RECEIVER_SHAPE_MUST_BE_TUPLE,
        span,
    ) else {
        return;
    };
    let Some(arg_tuple) = matmul_operand_tuple(
        checker,
        resolved_arg,
        ISSUE_MATMUL_ARGUMENT_SHAPE_MUST_BE_TUPLE,
        span,
    ) else {
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
/// This is the named elementwise method path only; the numeric `+` glyph
/// enforces exact shape equality in `ops.rs` instead.
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

fn tensor_shape_id(checker: &mut TypeChecker<'_>, ty: TypeId) -> Option<IndexId> {
    let resolved = checker.resolve_inferred_type(ty);
    let resolved = checker.types.peel_access_marker(resolved).1;
    match checker.types.get(resolved) {
        Type::Tensor(_, shape) => Some(*shape),
        _ => None,
    }
}

/// Conversion-row twin of the retired `from_flat` method form's flat-count
/// solve: in `flat ↦ tensor<…, [.., _]>` the target type is authoritative, so
/// a single `_` extent in the target figura is solved from the flat source's
/// static element count. Count-less sources and
/// multi-hole or whole-shape holes stay unsolved for the finalize hole check
/// (`cannot_infer_tensor_shape`).
pub(super) fn constrain_conversio_target_hole_from_flat_count(
    checker: &mut TypeChecker<'_>,
    flat_expr: &HirExpression,
    target_shape: IndexId,
    span: Span,
) {
    if let Some(count) = static_collection_length(checker, flat_expr) {
        infer_single_shape_hole_from_count(checker, target_shape, count, span);
    }
}

/// Conversion-row twin of the `from_flat` method form's shape-VALUE argument:
/// a target figura that is a single `list<int>` value name
/// (`data ↦ tensor<f32, shape>`) is a runtime value shape. When the binding
/// carries a literal-list shape witness — the same fact [`shape_list_index`]
/// recovers for the method form — the witness becomes the conversion's
/// authoritative static extents. Runtime-computed shape lists keep their
/// runtime extent; the conversion's runtime call checks the flat count
/// against the value's product. `None` keeps the written target.
pub(super) fn conversio_value_shape_target(
    checker: &mut TypeChecker<'_>,
    source_ty: TypeId,
    target: TypeId,
) -> Option<TypeId> {
    if crate::lista_tensor_conversio::validate_lista_to_tensor_source(
        checker.resolve_inferred_type(source_ty),
        checker.resolve_inferred_type(target),
        checker.types,
    )
    .is_err()
    {
        return None;
    }
    let resolved = checker.resolve_inferred_type(target);
    let Type::Tensor(elem, shape) = checker.types.get(resolved) else {
        return None;
    };
    let elem = *elem;
    let shape = checker.resolve_index(*shape);
    // The figura bracket always wraps a tuple, so the single-value form is
    // `Tuple([extent])`; a scalar extent keeps the scalar-extent reading.
    let shape = match checker.types.get_index(shape).clone() {
        IndexExpr::Tuple(dims) if dims.len() == 1 => checker.resolve_index(dims[0]),
        _ => shape,
    };
    let IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(def_id)) = checker.types.get_index(shape)
    else {
        return None;
    };
    let def_id = *def_id;
    // Only a list-typed binding is a value shape; a scalar extent keeps the
    // scalar-extent reading of the same index form.
    let binding = checker.lookup_binding(def_id)?;
    if !crate::numeric_width::is_integer_index_vector(
        checker.types,
        checker.resolve_inferred_type(binding.ty),
    ) {
        return None;
    }
    let witness = checker.shape_witnesses.get(&def_id).copied()?;
    Some(checker.types.tensor_with_shape(elem, witness))
}

fn constrain_reshape_result_shape(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    result_shape: IndexId,
    shape_expr: &HirExpression,
    span: Span,
) {
    let Some(shape) = shape_list_index(checker, shape_expr) else {
        return;
    };
    if let Some(count) = static_tensor_element_count(checker, receiver_ty) {
        infer_single_shape_hole_from_count(checker, shape, count, span);
    }
    checker.constrain_index_equal(result_shape, shape, span);
}

/// Resolve one structural shape-list hole when the flat element count and all
/// other extents are statically known. Runtime constructors still validate the
/// final count; this contributes a static result shape only when the division
/// is unique and exact.
fn infer_single_shape_hole_from_count(
    checker: &mut TypeChecker<'_>,
    shape: IndexId,
    element_count: u64,
    span: Span,
) {
    let shape = checker.resolve_index(shape);
    let IndexExpr::Tuple(dims) = checker.types.get_index(shape).clone() else {
        return;
    };
    let holes: Vec<_> = dims
        .iter()
        .enumerate()
        .filter_map(|(axis, dim)| {
            let resolved = checker.resolve_index(*dim);
            matches!(checker.types.get_index(resolved), IndexExpr::Infer(_)).then_some(axis)
        })
        .collect();
    let [hole_axis] = holes.as_slice() else {
        return;
    };
    let mut known_product = 1_u64;
    for (axis, dim) in dims.iter().enumerate() {
        if axis == *hole_axis {
            continue;
        }
        let resolved_dim = checker.resolve_index(*dim);
        let Ok(Some(extent)) = radix_types::evaluate_index_dimension(resolved_dim, checker.types)
        else {
            return;
        };
        let Some(product) = known_product.checked_mul(extent) else {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_INCOMPATIBLE_TENSOR_INDEX,
                span,
            );
            return;
        };
        known_product = product;
    }
    if known_product == 0 {
        return;
    }
    if !element_count.is_multiple_of(known_product) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_INCOMPATIBLE_TENSOR_INDEX,
            span,
        );
        return;
    }
    let inferred = element_count / known_product;
    let inferred = checker.types.intern_index(IndexExpr::Literal(inferred));
    checker.constrain_index_equal(dims[*hole_axis], inferred, span);
}

fn static_tensor_element_count(checker: &mut TypeChecker<'_>, ty: TypeId) -> Option<u64> {
    let shape = tensor_shape_id(checker, ty)?;
    let shape = checker.resolve_index(shape);
    let dims = match checker.types.get_index(shape).clone() {
        IndexExpr::Tuple(dims) => dims,
        IndexExpr::Literal(_) => vec![shape],
        _ => return None,
    };
    dims.into_iter().try_fold(1_u64, |count, dim| {
        let resolved_dim = checker.resolve_index(dim);
        let extent = radix_types::evaluate_index_dimension(resolved_dim, checker.types).ok()??;
        count.checked_mul(extent)
    })
}

fn static_collection_length(checker: &TypeChecker<'_>, expr: &HirExpression) -> Option<u64> {
    match &expr.kind {
        HirExpressionKind::Array(elements)
            if elements
                .iter()
                .all(|element| matches!(element, HirArrayElement::Expr(_))) =>
        {
            u64::try_from(elements.len()).ok()
        }
        HirExpressionKind::Path(def_id) => checker.static_collection_lengths.get(def_id).copied(),
        _ => None,
    }
}

/// What a static range proof says about an index against a receiver's bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StaticIndexRange {
    /// No proof either way (runtime index, symbolic bound, rank mismatch).
    Unknown,
    /// Every literal coordinate is within the literal extents.
    InRange,
    /// A literal coordinate contradicts the extent on `axis`.
    OutOfRange {
        indices: Vec<i128>,
        bound: u64,
        axis: usize,
    },
}

/// Classify a literal index against a receiver's literal static bound.
///
/// This is deliberately literal-only. Index parameters, inference holes,
/// dynamic shapes, and arithmetic expressions all decline here and remain
/// runtime policy.
pub(super) fn classify_static_index_range(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    index: &HirExpression,
) -> StaticIndexRange {
    let resolved = checker.resolve_inferred_type(receiver_ty);
    let (_, payload) = checker.types.peel_access_marker(resolved);
    let receiver = checker.types.get(payload).clone();
    let is_tensor = matches!(receiver, Type::Tensor(..));
    let bounds = match receiver {
        Type::BoundedArray { capacity, .. }
        | Type::BoundedTextus { capacity }
        | Type::BoundedAscii { capacity }
        | Type::BoundedOcteti { capacity } => {
            let capacity = checker.resolve_index(capacity);
            let IndexExpr::Literal(value) = checker.types.get_index(capacity) else {
                return StaticIndexRange::Unknown;
            };
            vec![*value]
        }
        Type::Tensor(_, shape) | Type::Matrix(_, shape) => static_tensor_bounds(checker, shape),
        _ => return StaticIndexRange::Unknown,
    };
    if is_tensor
        && bounds.len() == 1
        && let HirExpressionKind::Intervallum {
            start,
            end,
            step: _,
            kind,
        } = &index.kind
    {
        let Some(start) = static_signed_numerus_literal(start) else {
            return StaticIndexRange::Unknown;
        };
        let Some(end) = static_signed_numerus_literal(end) else {
            return StaticIndexRange::Unknown;
        };
        let bound = bounds[0];
        let end_eff = if *kind == HirRangeKind::Inclusive {
            end.saturating_add(1)
        } else {
            end
        };
        let out_of_bounds = |value: i128| {
            value < 0 || u128::try_from(value).map_or(true, |value| value >= u128::from(bound))
        };
        if out_of_bounds(start) || out_of_bounds(end_eff - 1) {
            return StaticIndexRange::OutOfRange {
                indices: vec![start, end_eff - 1],
                bound,
                axis: 0,
            };
        }
        return StaticIndexRange::InRange;
    }
    let Some(indices) = static_index_values(checker, index) else {
        return StaticIndexRange::Unknown;
    };
    if indices.len() != bounds.len() {
        return StaticIndexRange::Unknown;
    }
    for (axis, (value, bound)) in indices.iter().zip(bounds).enumerate() {
        if *value < 0 || u128::try_from(*value).map_or(true, |value| value >= u128::from(bound)) {
            return StaticIndexRange::OutOfRange {
                indices,
                bound,
                axis,
            };
        }
    }
    StaticIndexRange::InRange
}

/// Report a provably out-of-range index at the index expression's span.
pub(super) fn report_static_index_oob(
    checker: &mut TypeChecker<'_>,
    index: &HirExpression,
    range: &StaticIndexRange,
) {
    if let StaticIndexRange::OutOfRange {
        indices,
        bound,
        axis,
    } = range
    {
        checker.invalid_operand_types_with_args(
            ISSUE_PROVABLY_OUT_OF_BOUNDS,
            index.span,
            &[
                ("index", format_static_index(indices)),
                ("bound", bound.to_string()),
                ("axis", axis.to_string()),
            ],
        );
    }
}

/// Reject a literal index that contradicts a receiver's literal static bound.
///
/// The diagnostic span is the index expression itself so the source location
/// identifies the proven failure.
pub(super) fn reject_static_bounded_index_oob(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    index: &HirExpression,
) {
    let range = classify_static_index_range(checker, receiver_ty, index);
    report_static_index_oob(checker, index, &range);
}

/// Check a two-axis matrix bracket list whose range slots retain their axes.
/// Integer slots select coordinates; two ranges produce a matrix block, while
/// one range produces a row or column vector.
pub(super) fn check_matrix_bracket_slot_list_index(
    checker: &mut TypeChecker<'_>,
    _receiver_ty: TypeId,
    elem: TypeId,
    shape: IndexId,
    index: &mut HirExpression,
) -> TypeId {
    let HirExpressionKind::Array(elements) = &mut index.kind else {
        return checker.error_type;
    };
    if elements.len() != 2
        || elements.iter().any(|element| {
            matches!(
                element,
                HirArrayElement::Spread(_) | HirArrayElement::Infer(_)
            )
        })
    {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_INCOMPATIBLE_TENSOR_INDEX,
            index.span,
        );
        return checker.error_type;
    }

    let matrix_bounds = crate::literal_matrix_shape(shape, checker.types)
        .map(|(rows, columns)| [u64::from(rows), u64::from(columns)]);
    let mut range_axes = [false; 2];
    let mut kept_extents = [None; 2];
    let mut missing_static_bounds = false;
    let mut static_oob = Vec::new();
    let mut index_elem_ty: Option<TypeId> = None;

    for (axis, element) in elements.iter_mut().enumerate() {
        // Non-expression slots were rejected above with the tensor-index
        // diagnostic, so they carry no axis to check here.
        let HirArrayElement::Expr(slot) = element else {
            continue;
        };
        if let HirExpressionKind::Intervallum {
            start,
            end,
            step,
            kind,
        } = &mut slot.kind
        {
            range_axes[axis] = true;
            let range_kind = *kind;
            for part in [start.as_mut(), end.as_mut()] {
                let ty = checker.check_expr(part);
                index_elem_ty = Some(match index_elem_ty {
                    None => ty,
                    Some(existing) => checker.array_common_type(existing, ty, part.span),
                });
                if !crate::numeric_width::is_integer_index_scalar(
                    checker.types,
                    checker.resolve_inferred_type(ty),
                ) {
                    checker
                        .invalid_operand_types("tensor_bracket_slot_integer_or_range", part.span);
                }
            }
            if let Some(step_expr) = step {
                let previous = checker.allow_intervallum_step;
                checker.allow_intervallum_step = true;
                let step_ty = checker.check_expr(step_expr);
                checker.allow_intervallum_step = previous;
                if !crate::numeric_width::is_integer_index_scalar(
                    checker.types,
                    checker.resolve_inferred_type(step_ty),
                ) {
                    checker.invalid_operand_types(
                        "tensor_bracket_slot_integer_or_range",
                        step_expr.span,
                    );
                }
            }

            let static_step = step
                .as_deref()
                .map_or(Some(1), static_signed_numerus_literal);
            let literal_window = static_signed_numerus_literal(start)
                .zip(static_signed_numerus_literal(end))
                .zip(static_step);
            let Some(((start_value, end_value), step_value)) = literal_window else {
                missing_static_bounds = true;
                continue;
            };
            let Some(end_eff) = (if range_kind == HirRangeKind::Inclusive {
                end_value.checked_add(1)
            } else {
                Some(end_value)
            }) else {
                missing_static_bounds = true;
                continue;
            };

            if let Some(bound) = matrix_bounds.map(|bounds| bounds[axis]) {
                if start_value < 0
                    || end_eff < 0
                    || start_value > end_eff
                    || u128::try_from(end_eff).map_or(true, |end| end > u128::from(bound))
                    || step_value <= 0
                {
                    static_oob.push((vec![start_value, end_eff.saturating_sub(1)], bound, axis));
                }
            } else if step_value <= 0 {
                missing_static_bounds = true;
            }

            if step_value > 0
                && start_value <= end_eff
                && let Some(distance) = end_eff.checked_sub(start_value)
            {
                let extent = distance / step_value + i128::from(distance % step_value != 0);
                if let Ok(extent) = u64::try_from(extent) {
                    kept_extents[axis] =
                        Some(checker.types.intern_index(IndexExpr::Literal(extent)));
                }
            }
        } else {
            let slot_ty = checker.check_expr(slot);
            index_elem_ty = Some(match index_elem_ty {
                None => slot_ty,
                Some(existing) => checker.array_common_type(existing, slot_ty, slot.span),
            });
            if !crate::numeric_width::is_integer_index_scalar(
                checker.types,
                checker.resolve_inferred_type(slot_ty),
            ) {
                checker.invalid_operand_types("tensor_bracket_slot_integer_or_range", slot.span);
            }
            if let (Some(bounds), Some(value)) =
                (matrix_bounds, static_signed_numerus_literal(slot))
                && (value < 0
                    || u128::try_from(value)
                        .map_or(true, |value| value >= u128::from(bounds[axis])))
            {
                static_oob.push((vec![value], bounds[axis], axis));
            }
        }
    }

    let has_static_oob = !static_oob.is_empty();
    for (indices, bound, axis) in static_oob {
        report_static_index_oob(
            checker,
            index,
            &StaticIndexRange::OutOfRange {
                indices,
                bound,
                axis,
            },
        );
    }

    // The bracket list lowers as a single coordinate list; retain its typed
    // list annotation even though range expressions are handled axis-wise.
    let index_elem = index_elem_ty.unwrap_or_else(|| checker.types.primitive(Primitive::Numerus));
    let capacity = checker.types.intern_index(IndexExpr::Literal(2));
    index.ty = Some(checker.types.bounded_array(index_elem, capacity));

    if missing_static_bounds {
        checker.invalid_operand_types(ISSUE_BRACKET_RANGE_STATIC_BOUNDS_REQUIRED, index.span);
        return checker.error_type;
    }
    if has_static_oob {
        return checker.error_type;
    }
    if range_axes[0] && range_axes[1] {
        let (Some(rows), Some(columns)) = (kept_extents[0], kept_extents[1]) else {
            checker.invalid_operand_types(ISSUE_BRACKET_RANGE_STATIC_BOUNDS_REQUIRED, index.span);
            return checker.error_type;
        };
        let result_shape = checker
            .types
            .intern_index(IndexExpr::Tuple(vec![rows, columns]));
        checker.types.matrix_with_shape(elem, result_shape)
    } else if range_axes[0] {
        let Some(width) = kept_extents[0] else {
            checker.invalid_operand_types(ISSUE_BRACKET_RANGE_STATIC_BOUNDS_REQUIRED, index.span);
            return checker.error_type;
        };
        checker.types.vector_with_width(elem, width)
    } else {
        let Some(width) = kept_extents[1] else {
            checker.invalid_operand_types(ISSUE_BRACKET_RANGE_STATIC_BOUNDS_REQUIRED, index.span);
            return checker.error_type;
        };
        checker.types.vector_with_width(elem, width)
    }
}

/// Check a bracket list whose elements independently retain or drop tensor axes.
/// Integer slots select coordinates; range slots slice and retain their axis.
pub(super) fn check_tensor_bracket_slot_list_index(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    elem: TypeId,
    shape: IndexId,
    index: &mut HirExpression,
) -> TypeId {
    let HirExpressionKind::Array(elements) = &mut index.kind else {
        return tensor_with_fresh_shape(checker, elem);
    };
    if elements.iter().any(|element| {
        matches!(
            element,
            HirArrayElement::Spread(_) | HirArrayElement::Infer(_)
        )
    }) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_INCOMPATIBLE_TENSOR_INDEX,
            index.span,
        );
        return tensor_with_fresh_shape(checker, elem);
    }

    let slots_len = elements.len();
    let resolved_shape = checker.resolve_index(shape);
    if matches!(
        checker.types.get_index(resolved_shape),
        IndexExpr::Tuple(dims) if dims.len() != slots_len
    ) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_INCOMPATIBLE_TENSOR_INDEX,
            index.span,
        );
        return tensor_with_fresh_shape(checker, elem);
    }

    let static_bounds = static_tensor_bounds(checker, shape);
    let mut has_range = false;
    let mut all_kept_extents_static = true;
    let mut kept_extents = Vec::new();
    let mut static_oob = Vec::new();
    let mut index_elem_ty: Option<TypeId> = None;
    for (axis, element) in elements.iter_mut().enumerate() {
        let HirArrayElement::Expr(slot) = element else {
            continue;
        };
        if let HirExpressionKind::Intervallum {
            start,
            end,
            step,
            kind,
        } = &mut slot.kind
        {
            has_range = true;
            let range_kind = *kind;
            for part in [start.as_mut(), end.as_mut()] {
                let ty = checker.check_expr(part);
                index_elem_ty = Some(match index_elem_ty {
                    None => ty,
                    Some(existing) => checker.array_common_type(existing, ty, part.span),
                });
                if !crate::numeric_width::is_integer_index_scalar(
                    checker.types,
                    checker.resolve_inferred_type(ty),
                ) {
                    checker
                        .invalid_operand_types("tensor_bracket_slot_integer_or_range", part.span);
                }
            }
            if let Some(step_expr) = step {
                let previous = checker.allow_intervallum_step;
                checker.allow_intervallum_step = true;
                let step_ty = checker.check_expr(step_expr);
                checker.allow_intervallum_step = previous;
                if !crate::numeric_width::is_integer_index_scalar(
                    checker.types,
                    checker.resolve_inferred_type(step_ty),
                ) {
                    checker.invalid_operand_types(
                        "tensor_bracket_slot_integer_or_range",
                        step_expr.span,
                    );
                }
            }

            let static_step = step.as_deref().map_or(Some(1), static_numerus_literal);
            let literal_window = static_numerus_literal(start)
                .zip(static_numerus_literal(end))
                .zip(static_step);
            if let (Some(bound), Some(((start_value, end_value), step_value))) =
                (static_bounds.get(axis).copied(), literal_window)
            {
                let end_eff = if range_kind == HirRangeKind::Inclusive {
                    end_value.checked_add(1)
                } else {
                    Some(end_value)
                };
                if let Some(end_eff) = end_eff {
                    let out_of_range = start_value > end_eff || end_eff > bound || step_value == 0;
                    if out_of_range {
                        static_oob.push((
                            vec![
                                i128::from(start_value),
                                i128::from(end_eff.saturating_sub(1)),
                            ],
                            bound,
                            axis,
                        ));
                    }
                    if step_value > 0 && start_value <= end_eff {
                        let extent = (end_eff - start_value).div_ceil(step_value);
                        kept_extents.push(checker.types.intern_index(IndexExpr::Literal(extent)));
                    } else {
                        all_kept_extents_static = false;
                    }
                } else {
                    static_oob.push((
                        vec![i128::from(start_value), i128::from(end_value)],
                        bound,
                        axis,
                    ));
                    all_kept_extents_static = false;
                }
            } else {
                all_kept_extents_static = false;
            }
        } else {
            let slot_ty = checker.check_expr(slot);
            index_elem_ty = Some(match index_elem_ty {
                None => slot_ty,
                Some(existing) => checker.array_common_type(existing, slot_ty, slot.span),
            });
            if !crate::numeric_width::is_integer_index_scalar(
                checker.types,
                checker.resolve_inferred_type(slot_ty),
            ) {
                checker.invalid_operand_types("tensor_bracket_slot_integer_or_range", slot.span);
            }
            if let (Some(bound), Some(value)) = (
                static_bounds.get(axis).copied(),
                static_signed_numerus_literal(slot),
            ) && (value < 0 || u128::try_from(value).map_or(true, |v| v >= u128::from(bound)))
            {
                static_oob.push((vec![value], bound, axis));
            }
        }
    }

    for (indices, bound, axis) in static_oob {
        report_static_index_oob(
            checker,
            index,
            &StaticIndexRange::OutOfRange {
                indices,
                bound,
                axis,
            },
        );
    }
    // The bracket list lowers as one `lista` value at the MIR boundary, so
    // the index node must carry its list type here: the generic index route
    // typed this literal through the array-literal rule, and skipping it
    // left the node untyped for lowering. Unify the checked slot element
    // types; a list of only range slots still lowers its axis coordinates,
    // so fall back to the integer index scalar.
    let index_elem = index_elem_ty.unwrap_or_else(|| checker.types.primitive(Primitive::Numerus));
    let capacity = checker
        .types
        .intern_index(IndexExpr::Literal(slots_len as u64));
    index.ty = Some(checker.types.bounded_array(index_elem, capacity));
    if !has_range {
        warn_unproven_kernel_index(checker, receiver_ty, index);
        return elem;
    }
    if all_kept_extents_static && !kept_extents.is_empty() {
        let result_shape = checker.types.intern_index(IndexExpr::Tuple(kept_extents));
        return checker.types.tensor_with_shape(elem, result_shape);
    }
    // WHY: dynamic range bounds leave the retained axes' extents unknown;
    // preserve the tensor family with a fresh shape hole rather than guessing.
    tensor_with_fresh_shape(checker, elem)
}

/// Check a bracket range read on a rank-1 tensor: `t[lo ‥ hi]` reads a sliced
/// tensor, not a list window (operator ruling 2026-10-17: every slot of a
/// bracket index takes a range, with step; out of range, the bracket dies).
///
/// Direct scalar ranges are admitted only on rank-1 tensors; rank-N bracket
/// lists use [`check_tensor_bracket_slot_list_index`] to retain/drop axes.
/// The caller guarantees the receiver is a tensor and passes its element type
/// and shape.
pub(super) fn check_tensor_bracket_range_index(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    elem: TypeId,
    shape: IndexId,
    index: &mut HirExpression,
) -> TypeId {
    let HirExpressionKind::Intervallum {
        start,
        end,
        step,
        kind,
    } = &mut index.kind
    else {
        return tensor_with_fresh_shape(checker, elem);
    };
    let range_kind = *kind;
    // WHY: a scalar range indexes one axis, so a statically-known rank other
    // than one is the same rank decline a scalar index reports. A bare
    // `Literal` shape is rank 1; anything else (param, runtime, hole) is an
    // unknown rank and proceeds rather than guessing.
    let resolved_shape = checker.resolve_index(shape);
    if matches!(
        checker.types.get_index(resolved_shape),
        IndexExpr::Tuple(dims) if dims.len() != 1
    ) {
        checker.invalid_operand_types(super::access::ISSUE_TENSOR_SCALAR_INDEX_RANK, index.span);
        return tensor_with_fresh_shape(checker, elem);
    }
    let start_ty = checker.check_expr(start);
    let start_ty = checker.resolve_inferred_type(start_ty);
    if !crate::numeric_width::is_integer_index_scalar(checker.types, start_ty) {
        checker.invalid_operand_types(
            super::access::ISSUE_TENSOR_BRACKET_INDEX_INTEGER_OR_LISTA,
            start.span,
        );
    }
    let end_ty = checker.check_expr(end);
    let end_ty = checker.resolve_inferred_type(end_ty);
    if !crate::numeric_width::is_integer_index_scalar(checker.types, end_ty) {
        checker.invalid_operand_types(
            super::access::ISSUE_TENSOR_BRACKET_INDEX_INTEGER_OR_LISTA,
            end.span,
        );
    }
    if let Some(step_expr) = step {
        // WHY: the step of an `intervallum` here is a direct step by
        // construction, so the flag admits it the same way the range
        // expression does.
        let prev = checker.allow_intervallum_step;
        checker.allow_intervallum_step = true;
        let step_ty = checker.check_expr(step_expr);
        checker.allow_intervallum_step = prev;
        let step_ty = checker.resolve_inferred_type(step_ty);
        if !crate::numeric_width::is_integer_index_scalar(checker.types, step_ty) {
            checker.invalid_operand_types(
                super::access::ISSUE_TENSOR_BRACKET_INDEX_INTEGER_OR_LISTA,
                step_expr.span,
            );
        }
    }
    // WHY: a static result shape needs every part of the range static -- a
    // non-literal bound or step leaves the sliced length unknown, so the shape
    // stays a fresh hole, the same posture `sectio` takes.
    let static_step: Option<u64> = match step.as_deref() {
        Some(step_expr) => static_numerus_literal(step_expr),
        None => Some(1),
    };
    let literal_extent = static_tensor_bounds(checker, shape).first().copied();
    let bounds = static_numerus_literal(start).zip(static_numerus_literal(end));
    if literal_extent.is_some()
        && let (Some((start, end)), Some(step_value)) = (bounds, static_step)
    {
        let end_eff = if range_kind == HirRangeKind::Inclusive {
            end.saturating_add(1)
        } else {
            end
        };
        if let Some(output) = sectio_axis0_shape(checker, shape, start, end_eff, step_value) {
            // WHY: the slice shape only says how long the window is; it does
            // not say the window lies inside the extent, so the static
            // out-of-range proof still runs on this path.
            let result = checker.types.tensor_with_shape(elem, output);
            reject_static_bounded_index_oob(checker, receiver_ty, index);
            return result;
        }
    }
    let result = tensor_with_fresh_shape(checker, elem);
    reject_static_bounded_index_oob(checker, receiver_ty, index);
    result
}

/// Check a bracket range read on a register vector: `v[lo ‥ hi]` slices to a
/// vector whose width is computed at compile time from literal bounds and step
/// (operator ruling 2026-10-17: a slice stays in its family, and its shape is
/// computed at compile time when the bounds are constant).
///
/// A register vector carries a literal width, so a non-literal bound or step
/// cannot leave the slice width unknown the way a tensor shape hole can; that
/// decline is its own issue rather than a silent `lista` window.
///
/// The caller guarantees the receiver is a vector and passes its element type
/// and width.
pub(super) fn check_vector_bracket_range_index(
    checker: &mut TypeChecker<'_>,
    elem: TypeId,
    width: IndexId,
    index: &mut HirExpression,
) -> TypeId {
    let HirExpressionKind::Intervallum {
        start,
        end,
        step,
        kind,
    } = &mut index.kind
    else {
        return checker.error_type;
    };
    let range_kind = *kind;
    let start_ty = checker.check_expr(start);
    let start_ty = checker.resolve_inferred_type(start_ty);
    if !crate::numeric_width::is_integer_index_scalar(checker.types, start_ty) {
        checker.invalid_operand_types(
            super::access::ISSUE_TENSOR_BRACKET_INDEX_INTEGER_OR_LISTA,
            start.span,
        );
    }
    let end_ty = checker.check_expr(end);
    let end_ty = checker.resolve_inferred_type(end_ty);
    if !crate::numeric_width::is_integer_index_scalar(checker.types, end_ty) {
        checker.invalid_operand_types(
            super::access::ISSUE_TENSOR_BRACKET_INDEX_INTEGER_OR_LISTA,
            end.span,
        );
    }
    if let Some(step_expr) = step {
        // WHY: the step of an `intervallum` here is a direct step by
        // construction, so the flag admits it the same way the range
        // expression does.
        let prev = checker.allow_intervallum_step;
        checker.allow_intervallum_step = true;
        let step_ty = checker.check_expr(step_expr);
        checker.allow_intervallum_step = prev;
        let step_ty = checker.resolve_inferred_type(step_ty);
        if !crate::numeric_width::is_integer_index_scalar(checker.types, step_ty) {
            checker.invalid_operand_types(
                super::access::ISSUE_TENSOR_BRACKET_INDEX_INTEGER_OR_LISTA,
                step_expr.span,
            );
        }
    }
    // WHY: a register vector slice keeps the vector family at a computed
    // width, so every part of the range must be static; a non-literal bound or
    // step is a decline, not a shape hole.
    let static_step: Option<u64> = match step.as_deref() {
        Some(step_expr) => static_numerus_literal(step_expr),
        None => Some(1),
    };
    let static_width = crate::literal_vector_width(width, checker.types).map(u64::from);
    let bounds = static_numerus_literal(start).zip(static_numerus_literal(end));
    let (Some(width_value), Some((start_value, end_value)), Some(step_value)) =
        (static_width, bounds, static_step)
    else {
        checker.invalid_operand_types(ISSUE_BRACKET_RANGE_STATIC_BOUNDS_REQUIRED, index.span);
        return checker.error_type;
    };
    let end_eff = if range_kind == HirRangeKind::Inclusive {
        end_value.saturating_add(1)
    } else {
        end_value
    };
    // WHY: the static bound proof uses the same OutOfRange form as
    // `classify_static_index_range`, so `index_provably_out_of_bounds` carries
    // the same `index`/`bound`/`axis` arguments here. A literal step of 0 is
    // equally unusable and reports through the same issue.
    if start_value > end_eff || end_eff > width_value || step_value == 0 {
        report_static_index_oob(
            checker,
            index,
            &StaticIndexRange::OutOfRange {
                indices: vec![
                    i128::from(start_value),
                    i128::from(end_eff.saturating_sub(1)),
                ],
                bound: width_value,
                axis: 0,
            },
        );
        return checker.error_type;
    }
    let sliced = (end_eff - start_value).div_ceil(step_value);
    let sliced_width = checker.types.intern_index(IndexExpr::Literal(sliced));
    checker.types.vector_with_width(elem, sliced_width)
}

/// Axis extents of a tensor type, resolved; `None` when the type is not a
/// tensor or its shape is not a literal/tuple of dimensions.
fn tensor_axis_extents(checker: &mut TypeChecker<'_>, ty: TypeId) -> Option<Vec<IndexExpr>> {
    let resolved = checker.resolve_inferred_type(ty);
    let (_, payload) = checker.types.peel_access_marker(resolved);
    let (Type::Tensor(_, shape) | Type::Matrix(_, shape)) = checker.types.get(payload).clone()
    else {
        return None;
    };
    let shape = checker.resolve_index(shape);
    let dims = match checker.types.get_index(shape).clone() {
        IndexExpr::Tuple(dims) => dims,
        IndexExpr::Literal(_) => vec![shape],
        _ => return None,
    };
    Some(
        dims.into_iter()
            .map(|dim| {
                let dim = checker.resolve_index(dim);
                checker.types.get_index(dim).clone()
            })
            .collect(),
    )
}

/// An extent that names one definite size, so equality of two such extents
/// is a proof (an unsolved hole or an unspecified shape proves nothing).
fn is_definite_extent(extent: &IndexExpr) -> bool {
    !matches!(extent, IndexExpr::Infer(_) | IndexExpr::Unspecified)
}

/// Record the extent each `apud` coordinate binder of a tensor iteration
/// walks, so an index by that binder is provably in range of a tensor of the
/// same axis extent.
pub(super) fn record_coordinate_binder_extents(
    checker: &mut TypeChecker<'_>,
    iterable_ty: TypeId,
    coordinates: &[(DefId, radix_lexer::Symbol)],
) {
    let Some(extents) = tensor_axis_extents(checker, iterable_ty) else {
        return;
    };
    if extents.len() != coordinates.len() {
        return;
    }
    for ((def_id, _), extent) in coordinates.iter().zip(extents) {
        if is_definite_extent(&extent) {
            let id = checker.types.intern_index(extent);
            checker.coordinate_binder_extents.insert(*def_id, id);
        }
    }
}

/// Warn when `t[i]` inside a kernel indexes a tensor with an index the
/// checker cannot prove in range (gpu-reset K11).
///
/// The form stays checked at run time; the warning suggests the range type.
/// A component is proven when it is an in-range literal, or an `apud`
/// coordinate binder of a tensor iteration whose same-position axis has the
/// indexed tensor's extent on that axis. Anything else (arithmetic, a
/// parameter, a local, a list value) is unproven. Never an error.
pub(super) fn warn_unproven_kernel_index(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    index: &HirExpression,
) {
    if !checker.current_nucleum_entry {
        return;
    }
    let Some(extents) = tensor_axis_extents(checker, receiver_ty) else {
        return;
    };
    // A fully literal index is the static range classifier's: in range needs
    // no warning, out of range is already an error.
    if !matches!(
        classify_static_index_range(checker, receiver_ty, index),
        StaticIndexRange::Unknown
    ) {
        return;
    }
    let components: Vec<&HirExpression> = match &index.kind {
        HirExpressionKind::Array(elements) => {
            let mut parts = Vec::with_capacity(elements.len());
            for element in elements {
                let HirArrayElement::Expr(inner) = element else {
                    return warn_kernel_index(checker, index.span);
                };
                parts.push(inner);
            }
            parts
        }
        _ => vec![index],
    };
    if components.len() != extents.len() {
        return warn_kernel_index(checker, index.span);
    }
    let proven = components.iter().zip(&extents).all(|(component, extent)| {
        if let Some(value) = static_index_value(checker, component) {
            return matches!(extent, IndexExpr::Literal(bound)
                if u128::try_from(value).is_ok_and(|v| v < u128::from(*bound)));
        }
        let HirExpressionKind::Path(def_id) = &component.kind else {
            return false;
        };
        let Some(binder_extent) = checker.coordinate_binder_extents.get(def_id).copied() else {
            return false;
        };
        is_definite_extent(extent) && checker.types.get_index(binder_extent) == extent
    });
    if !proven {
        warn_kernel_index(checker, index.span);
    }
}

fn warn_kernel_index(checker: &mut TypeChecker<'_>, span: Span) {
    checker.errors.push(
        SemanticError::structured(
            SemanticErrorKind::Warning(crate::WarningKind::UnprovenKernelIndex),
            span,
        )
        .with_arg("issue", ISSUE_UNPROVEN_KERNEL_INDEX),
    );
}

/// Check the index of a vector lane access `v[k]`.
///
/// A constant lane outside the literal width is provably out of bounds (the
/// shared index-bounds family). A run-time index is the checked lane select:
/// it is bounds-checked when the value runs, exactly like a `lista` index, so
/// there is nothing to reject here. A symbolic width cannot be bounds-checked
/// here either; the lowered projection is validated against the concrete width.
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
        IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Affine { .. }
        | IndexExpr::Divide { .. }
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified => Vec::new(),
    }
}

/// The compile-time value of each index component: a literal, or a name or
/// expression that folds to a compile-time integer (a module constant, a
/// local `const T K = e`, `K + 1`). A constant index is proven or refused
/// exactly like the literal it stands for; a run-time binding (`let`) stays a
/// run-time index even when its initializer is a literal.
fn static_index_values(checker: &TypeChecker<'_>, expr: &HirExpression) -> Option<Vec<i128>> {
    match &expr.kind {
        HirExpressionKind::Array(elements) => elements
            .iter()
            .map(|element| match element {
                HirArrayElement::Expr(inner) => static_index_value(checker, inner),
                HirArrayElement::Spread(_) | HirArrayElement::Infer(_) => None,
            })
            .collect(),
        _ => Some(vec![static_index_value(checker, expr)?]),
    }
}

fn static_index_value(checker: &TypeChecker<'_>, expr: &HirExpression) -> Option<i128> {
    if !names_only_compile_time_values(checker, expr) {
        return None;
    }
    super::integer_range::const_int_value_resolved(expr, &checker.const_int_values)
}

/// Whether every name `expr` reads is a compile-time binding.
fn names_only_compile_time_values(checker: &TypeChecker<'_>, expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Path(def_id) => {
            checker.module_const_ints.contains(def_id)
                || checker.compile_time_locals.contains(def_id)
        }
        HirExpressionKind::Unary(_, inner) => names_only_compile_time_values(checker, inner),
        HirExpressionKind::Binary(_, lhs, rhs) => {
            names_only_compile_time_values(checker, lhs)
                && names_only_compile_time_values(checker, rhs)
        }
        _ => true,
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

/// Apply the explicit ordered-axis expansion contract when both shapes carry
/// static witnesses. Inserted target axes do not consume a source axis. A
/// source singleton may stretch with stride zero; all other mapped axes must
/// have equal extents. The selected injection minimizes stretches, then picks
/// the rightmost target axes to make singleton expansion deterministic.
fn constrain_expanded_result_shape(
    checker: &mut TypeChecker<'_>,
    receiver: &HirExpression,
    receiver_ty: TypeId,
    result_shape: IndexId,
    target_expr: &HirExpression,
    span: Span,
) {
    let Some(source_shape) = tensor_shape_id(checker, receiver_ty) else {
        return;
    };
    let Some(target_shape) = shape_list_index(checker, target_expr) else {
        return;
    };
    checker.constrain_index_equal(result_shape, target_shape, span);

    let source_shape = checker.resolve_index(source_shape);
    let target_shape = checker.resolve_index(target_shape);
    let (IndexExpr::Tuple(source_dims), IndexExpr::Tuple(target_dims)) = (
        checker.types.get_index(source_shape).clone(),
        checker.types.get_index(target_shape).clone(),
    ) else {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_INCOMPATIBLE_TENSOR_INDEX,
            span,
        );
        return;
    };
    let Some(mapping) = best_expanded_axis_mapping(checker, &source_dims, &target_dims) else {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_INCOMPATIBLE_TENSOR_INDEX,
            span,
        );
        return;
    };
    for (source_axis, source, target, stretched) in mapping {
        let target_resolved = checker.resolve_index(target);
        if matches!(
            checker.types.get_index(target_resolved),
            IndexExpr::Infer(_)
        ) {
            let source = checker.resolve_index(source);
            let source_term = checker.types.get_index(source).clone();
            let source_extent =
                if matches!(source_term, IndexExpr::Infer(_) | IndexExpr::Unspecified) {
                    checker.runtime_expression_axis(
                        receiver.id,
                        &[],
                        u32::try_from(source_axis).unwrap_or(u32::MAX),
                    )
                } else {
                    source
                };
            checker.constrain_index_equal(target, source_extent, span);
        } else if !stretched && dimensions_provably_equal(checker, source, target) {
            checker.constrain_index_equal(source, target, span);
        }
    }
}

fn dimensions_provably_equal(checker: &mut TypeChecker<'_>, left: IndexId, right: IndexId) -> bool {
    let left = checker.resolve_index(left);
    let right = checker.resolve_index(right);
    left == right
        || matches!(
            (checker.types.get_index(left), checker.types.get_index(right)),
            (IndexExpr::Literal(a), IndexExpr::Literal(b)) if a == b
        )
        || matches!(
            (checker.types.get_index(left), checker.types.get_index(right)),
            (IndexExpr::Param(a), IndexExpr::Param(b)) if a == b
        )
        || matches!(
            (checker.types.get_index(left), checker.types.get_index(right)),
            (IndexExpr::Runtime(a), IndexExpr::Runtime(b)) if a == b
        )
}

/// One chosen source axis: (source axis, source index, target index, target axis, stretched).
type PickedAxis = (usize, IndexId, IndexId, usize, bool);

/// Best candidate so far: (stretch count, target axes, picks).
type BestAxisMapping = (usize, Vec<usize>, Vec<PickedAxis>);

/// Source-axis to target-axis choices, ranked by stretch count and then by
/// rightmost target positions. Unknown dimensions are potential runtime matches;
/// the carrier performs their exact-or-singleton check after selecting the same
/// deterministic map. Static incompatible dimensions still fail here.
fn best_expanded_axis_mapping(
    checker: &mut TypeChecker<'_>,
    source: &[IndexId],
    target: &[IndexId],
) -> Option<Vec<(usize, IndexId, IndexId, bool)>> {
    fn visit(
        checker: &mut TypeChecker<'_>,
        source: &[IndexId],
        target: &[IndexId],
        source_axis: usize,
        target_start: usize,
        picked: &mut Vec<PickedAxis>,
        best: &mut Option<BestAxisMapping>,
    ) {
        if source_axis == source.len() {
            let target_axes = picked
                .iter()
                .map(|(_, _, _, target_axis, _)| *target_axis)
                .collect::<Vec<_>>();
            let stretches = picked
                .iter()
                .filter(|(_, _, _, _, stretched)| *stretched)
                .count();
            let candidate = (stretches, target_axes, picked.clone());
            let is_better = best.as_ref().is_none_or(|current| {
                candidate.0 < current.0 || (candidate.0 == current.0 && candidate.1 > current.1)
            });
            if is_better {
                *best = Some(candidate);
            }
            return;
        }
        let remaining = source.len() - source_axis;
        if target.len().saturating_sub(target_start) < remaining {
            return;
        }
        let source_id = source[source_axis];
        let source_resolved = checker.resolve_index(source_id);
        let source_term = checker.types.get_index(source_resolved).clone();
        for target_axis in target_start..=target.len() - remaining {
            let target_id = target[target_axis];
            let target_resolved = checker.resolve_index(target_id);
            let target_term = checker.types.get_index(target_resolved).clone();
            let exact = dimensions_provably_equal(checker, source_id, target_id);
            let stretched = !exact
                && matches!(&source_term, IndexExpr::Literal(1))
                && !matches!(&target_term, IndexExpr::Literal(1));
            let unresolved = matches!(
                (&source_term, &target_term),
                (
                    IndexExpr::Infer(_)
                        | IndexExpr::Runtime(_)
                        | IndexExpr::Param(_)
                        | IndexExpr::Affine { .. }
                        | IndexExpr::Divide { .. },
                    _
                ) | (
                    _,
                    IndexExpr::Infer(_)
                        | IndexExpr::Runtime(_)
                        | IndexExpr::Param(_)
                        | IndexExpr::Affine { .. }
                        | IndexExpr::Divide { .. }
                )
            );
            if !exact && !stretched && !unresolved {
                continue;
            }
            picked.push((source_axis, source_id, target_id, target_axis, stretched));
            visit(
                checker,
                source,
                target,
                source_axis + 1,
                target_axis + 1,
                picked,
                best,
            );
            picked.pop();
        }
    }

    if source.len() > target.len() {
        return None;
    }
    let mut picks = Vec::with_capacity(source.len());
    let mut best = None;
    visit(checker, source, target, 0, 0, &mut picks, &mut best);
    best.map(|(_, _, picks)| {
        picks
            .into_iter()
            .map(|(source_axis, source, target, _, stretched)| {
                (source_axis, source, target, stretched)
            })
            .collect()
    })
}

fn index_term_for_shape_expression(
    checker: &mut TypeChecker<'_>,
    expr: &HirExpression,
) -> Option<IndexId> {
    if let Some(value) = static_signed_numerus_literal(expr) {
        let value = u64::try_from(value).ok()?;
        return Some(checker.types.intern_index(IndexExpr::Literal(value)));
    }
    if let HirExpressionKind::Binary(HirBinOp::Div, numerator, divisor) = &expr.kind
        && let Some(divisor) = static_signed_numerus_literal(divisor)
    {
        let divisor = u64::try_from(divisor).ok().filter(|divisor| *divisor > 0)?;
        let numerator = index_term_for_shape_expression(checker, numerator)?;
        return radix_types::divide_index_exact(checker.types, numerator, divisor).ok();
    }
    let mut offset = 0_i128;
    let base = flatten_shape_affine_expression(expr, &mut offset);
    if !std::ptr::eq(base, expr) {
        let base = index_term_for_shape_expression(checker, base)?;
        let term = radix_types::compose_index_affine_constant(checker.types, base, offset).ok()?;
        return match radix_types::evaluate_index_dimension(term, checker.types) {
            Ok(Some(value)) => Some(checker.types.intern_index(IndexExpr::Literal(value))),
            Ok(None) => Some(term),
            Err(_) => None,
        };
    }
    if let HirExpressionKind::Path(def_id) = &expr.kind {
        if let Some(symbol) = checker.resolver.get_symbol(*def_id)
            && let Some(ty) = symbol.ty
            && let Type::Param(param) = checker.types.get(ty)
        {
            return Some(checker.types.intern_index(IndexExpr::Param(*param)));
        }
        if let Some(binding) = checker.lookup_binding(*def_id) {
            let ty = checker.resolve_inferred_type(binding.ty);
            if crate::numeric_width::is_integer_index_scalar(checker.types, ty) {
                // The same scalar binding can occur in both a tensor type
                // annotation and a constructor shape list. Preserve its
                // binding identity so the constructor result witnesses the
                // annotation exactly; an expression-local identity would
                // make these equal values appear to have unrelated extents.
                let source = radix_types::RuntimeExtentId::ScalarBinding(*def_id);
                return Some(checker.types.intern_index(IndexExpr::Runtime(source)));
            }
        }
    }
    let ty = expr.ty?;
    if !crate::numeric_width::is_integer_index_scalar(
        checker.types,
        checker.resolve_inferred_type(ty),
    ) {
        return None;
    }
    let runtime = radix_types::RuntimeExtentId::ScalarExpression {
        expression: expr.id.0,
    };
    Some(checker.types.intern_index(IndexExpr::Runtime(runtime)))
}

/// Flatten a chain of addition/subtraction by signed integer literals, just
/// as type-position Figura lowering does. Other binary forms remain one
/// expression and use the scalar-expression fallback below.
fn flatten_shape_affine_expression<'a>(
    expr: &'a HirExpression,
    offset: &mut i128,
) -> &'a HirExpression {
    match &expr.kind {
        HirExpressionKind::Binary(HirBinOp::Add, value, amount) => {
            if let Some(amount) = static_signed_numerus_literal(amount)
                && let Some(total) = offset.checked_add(amount)
            {
                *offset = total;
                return flatten_shape_affine_expression(value, offset);
            }
            if let Some(literal) = static_signed_numerus_literal(value)
                && let Some(total) = offset.checked_add(literal)
            {
                *offset = total;
                return flatten_shape_affine_expression(amount, offset);
            }
            expr
        }
        HirExpressionKind::Binary(HirBinOp::Sub, value, amount) => {
            if let Some(amount) = static_signed_numerus_literal(amount)
                && let Some(total) = offset.checked_sub(amount)
            {
                *offset = total;
                return flatten_shape_affine_expression(value, offset);
            }
            expr
        }
        _ => expr,
    }
}

/// Shape arguments may contain source holes and runtime extents. Keep the
/// literal-only extractor above for initializer witnesses, where carrying an
/// expression-local runtime id past its evaluation boundary would be unsound.
fn shape_list_index(checker: &mut TypeChecker<'_>, expr: &HirExpression) -> Option<IndexId> {
    match &expr.kind {
        HirExpressionKind::Array(elements) => {
            let mut dims = Vec::with_capacity(elements.len());
            for element in elements {
                let dim = match element {
                    HirArrayElement::Expr(inner) => {
                        index_term_for_shape_expression(checker, inner)?
                    }
                    HirArrayElement::Infer(_) => checker.fresh_index_hole(),
                    HirArrayElement::Spread(_) => return None,
                };
                dims.push(dim);
            }
            Some(checker.types.intern_index(IndexExpr::Tuple(dims)))
        }
        HirExpressionKind::Path(def_id) => checker.shape_witnesses.get(def_id).copied(),
        _ => None,
    }
}

/// Output shape for axis-0 `sectio(start, end[, step])` when input rank and
/// bounds are static.
fn sectio_axis0_shape(
    checker: &mut TypeChecker<'_>,
    input_shape: IndexId,
    start: u64,
    end: u64,
    step: u64,
) -> Option<IndexId> {
    if end < start || step == 0 {
        return None;
    }
    let resolved = checker.resolve_index(input_shape);
    let dims = match checker.types.get_index(resolved).clone() {
        IndexExpr::Tuple(dims) => dims,
        // WHY: axis reductions keep the surviving dim as the result shape
        // itself (`reduction.rs` returns `dims[only]`), so a reduced tensor's
        // rank-1 shape is a `Literal` rather than a singleton tuple. Read it as
        // the one-element dim list it means, as the element-count solver does.
        IndexExpr::Literal(_) => vec![resolved],
        _ => return None,
    };
    if dims.is_empty() {
        return None;
    }
    let distance = end - start;
    let sliced = distance.div_ceil(step);
    let mut output = vec![checker.types.intern_index(IndexExpr::Literal(sliced))];
    output.extend_from_slice(&dims[1..]);
    Some(checker.types.intern_index(IndexExpr::Tuple(output)))
}
