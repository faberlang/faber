//! M2 reduction typing for the sibling `maxima`/`minima` expressions.
//!
//! Max/min accept shaped iterables, use the shared `apud` arity law, and carry
//! no term body. A statically non-empty shape produces `T`; a dynamic or
//! statically empty shape produces `T ∪ nihil` unless the caller supplies a
//! coalesce identity, in which case the expression produces `T`.

use super::expr::{ApudArity, ISSUE_ITERA_APUD_ARITY_MISMATCH, apud_arity};
#[allow(clippy::wildcard_imports)]
use super::*;
use radix_hir::HirExtremumOp;

const ISSUE_EXTREMUM_REQUIRES_SHAPED_ITERABLE: &str = "extremum_requires_shaped_iterable";
const ISSUE_EXTREMUM_TERM_TYPE_NON_SCALAR: &str = "extremum_term_type_non_scalar";
const ISSUE_EXTREMUM_IDENTITY_TYPE_MISMATCH: &str = "extremum_identity_type_mismatch";

/// Check one sibling max/min reduction and produce its result type.
pub(super) fn check_extremum_expr(
    checker: &mut TypeChecker<'_>,
    _op: HirExtremumOp,
    source: &mut HirExpression,
    coordinates: &[(DefId, Symbol)],
    coalesce: Option<&mut Box<HirExpression>>,
    expected: Option<TypeId>,
    span: radix_lexer::Span,
) -> TypeId {
    let source_ty = checker.check_expr(source);
    let dispatch_ty = checker
        .types
        .peel_access_marker(checker.resolve_inferred_type(source_ty))
        .1;

    let (elem_ty, statically_non_empty) = match checker.types.get(dispatch_ty) {
        Type::Tensor(elem, shape) | Type::Matrix(elem, shape) => {
            (*elem, shape_is_statically_non_empty(checker, *shape))
        }
        _ => {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_EXTREMUM_REQUIRES_SHAPED_ITERABLE,
                span,
            );
            if let Some(identity) = coalesce {
                checker.check_expr(identity);
            }
            return checker.error_type;
        }
    };
    let elem_ty = checker.resolve_inferred_type(elem_ty);

    match apud_arity(
        checker.types,
        dispatch_ty,
        !coordinates.is_empty(),
        coordinates.len(),
    ) {
        ApudArity::Admitted => {}
        ApudArity::RequiresShaped => checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            super::expr::ISSUE_ITERA_APUD_REQUIRES_SHAPED_ITERABLE,
            span,
        ),
        ApudArity::Mismatch { expected, actual } => checker.errors.push(
            SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                .with_arg("issue", ISSUE_ITERA_APUD_ARITY_MISMATCH)
                .with_arg("expected", expected.to_string())
                .with_arg("actual", actual.to_string()),
        ),
    }

    if !is_numeric_scalar(checker.types, elem_ty) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_EXTREMUM_TERM_TYPE_NON_SCALAR,
            source.span,
        );
    }

    if let Some(identity) = coalesce {
        let identity_ty = checker.check_expr_with_expected(identity, Some(elem_ty));
        checker.unify_assign_issue(
            identity_ty,
            elem_ty,
            identity.span,
            ISSUE_EXTREMUM_IDENTITY_TYPE_MISMATCH,
        );
        return elem_ty;
    }

    if statically_non_empty {
        elem_ty
    } else {
        let result_ty = checker.types.option(elem_ty);
        if let Some(expected) = expected {
            let expected = checker.resolve_inferred_type(expected);
            if !matches!(checker.types.get(expected), Type::Option(_)) {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    "extremum_nullable_result_requires_coalesce",
                    span,
                );
            }
        }
        result_ty
    }
}

fn shape_is_statically_non_empty(checker: &TypeChecker<'_>, shape: crate::IndexId) -> bool {
    let Some(dims) = crate::i64s_from_literal_index(shape, checker.types) else {
        return false;
    };
    dims.iter().all(|dim| *dim > 0)
}

fn is_numeric_scalar(types: &TypeTable, ty: TypeId) -> bool {
    matches!(
        types.get(ty),
        Type::Primitive(Primitive::Numerus | Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
    )
}
