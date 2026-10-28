//! Cartesian `itera ab` range-product admission laws (NR-2).
//!
//! One walk over N >= 2 declared numeric domains with N name binders — a
//! product, never a zip. The laws, all fail-closed and distinct from the
//! for-from lockstep family:
//!
//! - N ranges carry N binders — count mismatch is a type error, never a
//!   partial bind (`itera_range_source_binder_arity_mismatch`).
//! - No extent/binder shadowing: ranges lower outside the binder scope, so
//!   a binder spelling that also names an extent reference reads as two
//!   different bindings and is declined
//!   (`itera_range_extent_binder_shadowing`).
//! - Every member must be a range expression — a mixed series such as
//!   `0‥m, n` is the dedicated mixed-source decline, never the single-range
//!   `ab` issue and never a zip fallback (`itera_range_mixed_source`).
//! - Order is structural: the vectors keep source order and the first range
//!   is the outermost walk (leftmost-outermost; enforced by the node shape,
//!   not re-derived here).
//!
//! `IteraLockstep` and its list laws are untouched: this checker never
//! consults lengths, and a range product never reaches `lockstep.rs`.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_lexer::Span;

/// Every range member must be a range expression (`0‥m, n` declines here).
const ISSUE_ITERA_RANGE_MIXED_SOURCE: &str = "itera_range_mixed_source";
/// N ranges bind N binders; a count mismatch never binds partially.
const ISSUE_ITERA_RANGE_SOURCE_BINDER_ARITY_MISMATCH: &str =
    "itera_range_source_binder_arity_mismatch";
/// A binder spelling may not also name an extent reference.
const ISSUE_ITERA_RANGE_EXTENT_BINDER_SHADOWING: &str = "itera_range_extent_binder_shadowing";

/// Check one Cartesian range product and produce its (vacuum) statement type.
pub(super) fn check_range_product_expr(
    checker: &mut TypeChecker<'_>,
    ranges: &mut [HirExpression],
    bindings: &[(DefId, Symbol)],
    body: &mut HirBlock,
    span: Span,
) -> TypeId {
    // Arity law: N ranges, N binders — same never-a-partial-bind stance as
    // the lockstep series, but a dedicated product identity.
    if ranges.len() != bindings.len() {
        checker.errors.push(
            SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                .with_arg("issue", ISSUE_ITERA_RANGE_SOURCE_BINDER_ARITY_MISMATCH)
                .with_arg("expected", ranges.len().to_string())
                .with_arg("actual", bindings.len().to_string()),
        );
    }

    // Shadowing law: ranges lowered outside the binder scope keep their outer
    // references, so a binder spelling that also occurs as a reference inside
    // any extent would name two different bindings. Compare spellings, not
    // DefIds — the extent reference and the binder are distinct defs by
    // construction.
    let binder_names: FxHashSet<Symbol> = bindings.iter().map(|(_, name)| *name).collect();
    if !binder_names.is_empty() {
        for range in ranges.iter() {
            let mut collector = ExtentReferenceCollector::default();
            collector.visit_expr(range);
            for def in collector.defs {
                let referenced = checker
                    .binding_names
                    .get(&def)
                    .copied()
                    .or_else(|| checker.resolver.get_symbol(def).map(|symbol| symbol.name));
                if referenced.is_some_and(|name| binder_names.contains(&name)) {
                    checker.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_ITERA_RANGE_EXTENT_BINDER_SHADOWING,
                        range.span,
                    );
                    break;
                }
            }
        }
    }

    // Membership law: every member is a range expression. Reuse the
    // single-range bound machinery (step-direct extents included); a
    // non-range member is the mixed-source decline, not the single-`ab`
    // issue and not a zip fallback.
    let mut element_tys = Vec::with_capacity(bindings.len());
    for range in ranges.iter_mut() {
        let range_ty = checker.check_expr_allowing_direct_intervallum_step(range);
        if let Some(bound) = checker.intervallum_bound_type(range_ty) {
            element_tys.push(bound);
        } else {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_ITERA_RANGE_MIXED_SOURCE,
                range.span,
            );
            element_tys.push(checker.numerus_type());
        }
    }

    checker.push_scope();
    for ((def_id, _), element) in bindings.iter().zip(element_tys.iter()) {
        checker.insert_binding(*def_id, *element);
    }
    for ((def_id, name), range) in bindings.iter().zip(ranges.iter()) {
        checker.seed_range_loop_binder(*def_id, *name, range);
    }
    checker.check_loop_block(body);
    checker.release_range_binders(bindings.iter().map(|(def_id, _)| *def_id));
    checker.pop_scope();
    checker.vacuum_type()
}

/// Collects every definition referenced inside one extent expression.
#[derive(Default)]
struct ExtentReferenceCollector {
    defs: Vec<DefId>,
}

impl HirVisitor for ExtentReferenceCollector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Path(def) = &expr.kind {
            self.defs.push(*def);
        }
        walk_expr(self, expr);
    }
}
