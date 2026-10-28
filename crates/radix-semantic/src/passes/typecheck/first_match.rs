//! AFM-3 (air-first-match): base-lane `primus_quem` admission laws.
//!
//! Replaces the AFM-2 fail-closed gate (`first_match_expression_unsupported`)
//! with the real base-lane typing: the source must be a shaped iterable
//! (array / tensor / matrix), the `ubi` predicate body must produce a
//! `bivalens`, and the expression type is `T ∪ nihil` where `T` is the
//! source element type. Predicate errors ride the error channel (they are
//! ordinary body expressions); the nullable result covers only no-match and
//! empty sources. AIR admission (bounded sources, first-live mask) stays
//! fail-closed until AFM-4.

use super::expr::{ApudArity, ISSUE_ITERA_APUD_ARITY_MISMATCH, apud_arity};
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_lexer::Span;

const ISSUE_FIRST_MATCH_REQUIRES_SHAPED_ITERABLE: &str = "first_match_requires_shaped_iterable";
const ISSUE_FIRST_MATCH_PREDICATE_TYPE: &str = "first_match_predicate_type";

/// Check one `primus_quem` expression and produce its `T ∪ nihil` result type.
pub(super) fn check_first_match_expr(
    checker: &mut TypeChecker<'_>,
    source: &mut HirExpression,
    coordinates: &[(DefId, Symbol)],
    binding: (DefId, Symbol),
    body: &mut HirBlock,
    span: Span,
) -> TypeId {
    let source_ty = checker.check_expr(source);
    let dispatch_ty = checker
        .types
        .peel_access_marker(checker.resolve_inferred_type(source_ty))
        .1;

    // Base-lane iterable surface: array (including exact-bound literals),
    // tensor, and matrix sources. The
    // shared apud arity rule (coordinate count = rank) applies unchanged.
    let elem_ty = match checker.types.get(dispatch_ty) {
        Type::Array(inner)
        | Type::BoundedArray { element: inner, .. }
        | Type::RangedArray { element: inner, .. }
        | Type::Tensor(inner, _)
        | Type::Matrix(inner, _) => checker.resolve_inferred_type(*inner),
        _ => {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_FIRST_MATCH_REQUIRES_SHAPED_ITERABLE,
                span,
            );
            check_first_match_body(checker, coordinates, binding, body, checker.error_type);
            return checker.error_type;
        }
    };

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

    check_first_match_body(checker, coordinates, binding, body, elem_ty);

    // Result contract: `T ∪ nihil` — nihil covers no-match and empty input
    // only, never a predicate error.
    let nihil = checker.types.primitive(Primitive::Nihil);
    checker.types.intern(Type::Union(vec![elem_ty, nihil]))
}

/// Bind the predicate scope (coordinates as `numerus`, the candidate binder
/// as the element type) and check the body under the `bivalens` predicate
/// contract.
fn check_first_match_body(
    checker: &mut TypeChecker<'_>,
    coordinates: &[(DefId, Symbol)],
    binding: (DefId, Symbol),
    body: &mut HirBlock,
    elem_ty: TypeId,
) {
    checker.push_scope();
    let numerus = checker.numerus_type();
    for (def_id, _) in coordinates {
        checker.insert_binding(*def_id, numerus);
    }
    checker.insert_binding(binding.0, elem_ty);

    let bivalens = checker.types.primitive(Primitive::Bivalens);
    let body_ty = checker.check_block(body, Some(bivalens));
    checker.unify_assign_issue(
        body_ty,
        bivalens,
        body.span,
        ISSUE_FIRST_MATCH_PREDICATE_TYPE,
    );
    checker.pop_scope();
}
