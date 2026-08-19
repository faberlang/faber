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

const TENSOR_STRUE: &str = "strue";
const TENSOR_FORMA: &str = "forma";
const TENSOR_CREA: &str = "crea";
const TENSOR_SECTIO: &str = "sectio";
const TENSOR_TRANSPONE: &str = "transpone";
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

/// Tensor return type with a fresh shape hole for ops that may change rank/size.
pub(super) fn tensor_with_fresh_shape(checker: &mut TypeChecker<'_>, elem: TypeId) -> TypeId {
    let shape = checker.fresh_index_hole();
    checker.types.tensor_with_shape(elem, shape)
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
            ) {
                if let Some(output) = sectio_axis0_shape(checker, input_shape, start, end) {
                    checker.constrain_index_equal(result_shape, output, span);
                }
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

/// Constrain `result_shape` to `[M, N]` for rank-2 matmul where `receiver` is
/// `[M, K]` and `arg` is `[K, N]`. Enforces rank-2 on both operands and that
/// the shared inner dimension `K` unifies. Rejects on mismatch.
///
/// Shared by the `matmul` intrinsic and the glyph `·` tensor arm so both
/// surfaces accept or fail closed identically.
pub(super) fn constrain_matmul_result_shape(
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
    if receiver_tuple.len() != 2 {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_MATMUL_RECEIVER_RANK_MUST_BE_TWO,
            span,
        );
        return;
    }
    if arg_tuple.len() != 2 {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_MATMUL_ARGUMENT_RANK_MUST_BE_TWO,
            span,
        );
        return;
    }
    // WHY: enforce K (receiver cols) == K (argument rows) before publishing the
    // `[M, N]` result shape. A failed inner-dimension check must not also let
    // downstream constraints treat the contraction as valid.
    if unify_tensor_shape_indices(checker, receiver_tuple[1], arg_tuple[0], span).is_err() {
        return;
    }
    // WHY: result shape is [M, N] — receiver's first dim and argument's last dim.
    // The dims are IndexIds; look up the resolved expressions to build a new tuple.
    let m_expr = checker.types.get_index(receiver_tuple[0]).clone();
    let n_expr = checker.types.get_index(arg_tuple[1]).clone();
    let m_id = checker.types.intern_index(m_expr);
    let n_id = checker.types.intern_index(n_expr);
    let result = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![m_id, n_id]));
    checker.constrain_index_equal(result_shape, result, span);
}

/// Constrain `result_shape` to the transpose of `receiver`'s shape.
///
/// For a rank-2 receiver [R, C], the result is [C, R]. For rank-1, it is
/// identity. Shape holes are preserved.
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
    if receiver_tuple.len() != 2 {
        // Transpose of non-rank-2 tensor is a no-op; pin result to receiver.
        checker.constrain_index_equal(result_shape, receiver_shape, span);
        return;
    }
    let r_expr = checker.types.get_index(receiver_tuple[0]).clone();
    let c_expr = checker.types.get_index(receiver_tuple[1]).clone();
    let r_id = checker.types.intern_index(r_expr);
    let c_id = checker.types.intern_index(c_expr);
    let result = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![c_id, r_id]));
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
