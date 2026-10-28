//! Test-only producer and observer for interval flow facts (need,
//! slices 1-2). The checker has no real range producer yet; this seam lets the
//! flow tests drive the machinery from source, and is compiled out of every
//! other build.
//!
//! PRODUCER: a leaf condition `x <op> bound`, with `x` an integer binding and
//! `bound` an integer literal, an immutable binding, or `s.longitudo()`. The
//! else edge is the integer-order negation (`x < b` else `x ≥ b`).
//!
//! OBSERVER: every read of a named binding records the interval the checker
//! holds for it at that point.

#![cfg(test)]

use std::cell::RefCell;

use super::control::LeafRefinement;
use super::flow_range::{BoundKey, FactValue, IntRange};
use super::*;

use super::control::FactKey;

thread_local! {
    static READS: RefCell<Vec<(String, Option<IntRange>)>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn clear_reads() {
    READS.with(|reads| reads.borrow_mut().clear());
}

pub(super) fn reads_of(name: &str) -> Vec<Option<IntRange>> {
    READS.with(|reads| {
        reads
            .borrow()
            .iter()
            .filter(|(read, _)| read == name)
            .map(|(_, range)| range.clone())
            .collect()
    })
}

pub(super) fn observe(checker: &TypeChecker<'_>, def_id: DefId) {
    let (Some(interner), Some(name)) = (checker.interner, checker.binding_names.get(&def_id))
    else {
        return;
    };
    let range = checker.range_flow.get(def_id).cloned();
    READS.with(|reads| {
        reads
            .borrow_mut()
            .push((interner.resolve(*name).to_owned(), range));
    });
}

fn bound_of(checker: &TypeChecker<'_>, expr: &HirExpression) -> Option<BoundKey> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => {
            Some(BoundKey::Const(i128::from(*value)))
        }
        HirExpressionKind::Path(def_id) => {
            let binding = checker.lookup_binding(*def_id)?;
            (!binding.mutable).then_some(BoundKey::Binding(*def_id))
        }
        HirExpressionKind::MethodCall(receiver, name, _, args, _) if args.is_empty() => {
            let HirExpressionKind::Path(source) = &receiver.kind else {
                return None;
            };
            (checker.interner?.resolve(*name) == "longitudo")
                .then(|| checker.range_flow.live_length(*source))
        }
        _ => None,
    }
}

pub(super) fn leaf(checker: &TypeChecker<'_>, cond: &HirExpression) -> Option<LeafRefinement> {
    let HirExpressionKind::Binary(op, lhs, rhs) = &cond.kind else {
        return None;
    };
    let HirExpressionKind::Path(subject) = &lhs.kind else {
        return None;
    };
    if !checker.is_integer(checker.lookup_binding(*subject)?.ty) {
        return None;
    }
    let bound = bound_of(checker, rhs)?;
    let (then_range, else_range) = match op {
        HirBinOp::Lt => (
            IntRange::from_hi(bound.clone(), true),
            IntRange::from_lo(bound, false),
        ),
        HirBinOp::LtEq => (
            IntRange::from_hi(bound.clone(), false),
            IntRange::from_lo(bound, true),
        ),
        HirBinOp::Gt => (
            IntRange::from_lo(bound.clone(), true),
            IntRange::from_hi(bound, false),
        ),
        HirBinOp::GtEq => (
            IntRange::from_lo(bound.clone(), false),
            IntRange::from_hi(bound, true),
        ),
        _ => return None,
    };
    let fact = |range| (FactKey::Binding(*subject), FactValue::range(range));
    Some((Some(fact(then_range)), Some(fact(else_range))))
}
