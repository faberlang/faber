//! Lockstep `itera ex` N-ary admission laws.
//!
//! One walk over N >= 2 aligned `lista` sources with N name binders. The
//! laws, all fail-closed and shared with the single-source `apud` family:
//!
//! - N sources carry N binders — count mismatch is a type error, never a
//!   partial bind.
//! - The optional `apud [i]` clause stays ONE shared coordinate; it does not
//!   grow with N (`at [i, j]` on a list zip is declined).
//! - Every source must be a `lista` (`list<T>` or literal-capacity
//!   `lista<T, magnitudo N>`); map, tensor, queue, stack, text, and cursor surfaces
//!   stay single-source until separately proven.
//! - Equal length: statically known lengths (literal-capacity witnesses)
//!   must already be equal here; dynamic `list<T>` lengths are guarded at
//!   the MIR walk admission. The walk never truncates to the minimum.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_lexer::Span;

use super::expr::{
    ISSUE_ITERA_LOCKSTEP_AT_COORDINATE_CARDINALITY, ISSUE_ITERA_LOCKSTEP_LENGTH_MISMATCH,
    ISSUE_ITERA_LOCKSTEP_SOURCE_BINDER_ARITY_MISMATCH, ISSUE_ITERA_LOCKSTEP_SOURCE_REQUIRES_LISTA,
};

/// Check one lockstep walk and produce its (vacuum) statement type.
pub(super) fn check_lockstep_expr(
    checker: &mut TypeChecker<'_>,
    sources: &mut [HirExpression],
    coordinates: &[(DefId, Symbol)],
    bindings: &[(DefId, Symbol)],
    body: &mut HirBlock,
    span: Span,
) -> TypeId {
    // Arity law: N sources, N binders — same never-a-partial-bind stance as
    // the `apud` coordinate arity rule.
    if sources.len() != bindings.len() {
        checker.errors.push(
            SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                .with_arg("issue", ISSUE_ITERA_LOCKSTEP_SOURCE_BINDER_ARITY_MISMATCH)
                .with_arg("expected", sources.len().to_string())
                .with_arg("actual", bindings.len().to_string()),
        );
    }

    // Shared `apud` cardinality: one coordinate names the shared index; more
    // than one never grows with the series.
    if coordinates.len() > 1 {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_ITERA_LOCKSTEP_AT_COORDINATE_CARDINALITY,
            span,
        );
    }

    let numerus = checker.numerus_type();
    let mut element_tys = Vec::with_capacity(bindings.len());
    let mut literal_lengths: Vec<Option<u64>> = Vec::with_capacity(sources.len());
    for source in sources.iter_mut() {
        let source_ty = checker.check_expr(source);
        let dispatch_ty = checker
            .types
            .peel_access_marker(checker.resolve_inferred_type(source_ty))
            .1;
        let (element, literal_length) = match checker.types.get(dispatch_ty) {
            Type::Array(inner) => (*inner, None),
            Type::BoundedArray { element, .. } | Type::RangedArray { element, .. } => {
                // Exact static length only when the expression is an array
                // literal: unannotated literals are exact witnesses of their
                // element count, while a `lista<T, magnitudo N>` binding keeps a
                // runtime `len <= N` carrier (the same exact-bound lattice
                // the elementwise lista operators consult).
                let literal = match &source.kind {
                    HirExpressionKind::Array(elements)
                        if elements
                            .iter()
                            .all(|entry| matches!(entry, HirArrayElement::Expr(_))) =>
                    {
                        Some(elements.len() as u64)
                    }
                    _ => None,
                };
                (*element, literal)
            }
            _ => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_ITERA_LOCKSTEP_SOURCE_REQUIRES_LISTA,
                    source.span,
                );
                (checker.error_type, None)
            }
        };
        element_tys.push(element);
        literal_lengths.push(literal_length);
    }

    // Static equal-length admission: every statically known length must be
    // the same value; unknown (dynamic `list<T>`) lengths defer to the MIR
    // walk guard, which fails closed rather than truncating.
    let expected_length = literal_lengths.iter().flatten().copied().next();
    if let Some(expected) = expected_length {
        for length in literal_lengths.iter().flatten() {
            if *length != expected {
                checker.errors.push(
                    SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                        .with_arg("issue", ISSUE_ITERA_LOCKSTEP_LENGTH_MISMATCH)
                        .with_arg("expected", expected.to_string())
                        .with_arg("actual", length.to_string()),
                );
                break;
            }
        }
    }

    checker.push_scope();
    for (def_id, _) in coordinates {
        checker.insert_binding(*def_id, numerus);
    }
    for ((def_id, _), element) in bindings.iter().zip(element_tys.iter()) {
        checker.insert_binding(*def_id, *element);
    }
    // The walk guard fails closed on unequal lengths, so the shared coordinate
    // is below every source's length: the first named source states it.
    if let Some(first) = sources.first()
        && let Some(first_ty) = first.ty
    {
        checker.seed_coordinates(first_ty, Some(first), coordinates);
    }
    checker.check_loop_block_over(sources.iter(), body);
    checker.release_range_binders(coordinates.iter().map(|(def_id, _)| *def_id));
    checker.pop_scope();
    checker.vacuum_type()
}
