//! The first consumer of interval facts: a list or text `at` / `slice` whose
//! arguments are proven in range needs no handler (need, slice 5).
//!
//! The runtime contract is the one the runner enforces (`window_inside` in the
//! runtime contract crate, `at` read in the runner):
//!
//! - `at(i)` succeeds exactly when `0 ≤ i < live length`.
//! - `slice(start, end)` succeeds exactly when `0 ≤ start ≤ end ≤ live length`;
//!   the empty window at the end (`start = end = length`) is inside.
//!
//! A call is discharged only when the whole predicate is proven from the facts
//! the checker holds at the call. Anything unproven, and anything provably
//! outside the contract, keeps the handler requirement and its diagnostic.
//!
//! `start ≤ end` has no relational fact between two bindings, so it is proven
//! from the argument shapes only: literal intervals that do not overlap, two
//! reads of one binding shifted by literals (`i`, `i + 1`), or a start bounded
//! by the exact value the end names (`len`, an immutable binding).

use radix_hir::{HirBinOp, HirCallArg, HirConversioTarget, HirExpression, HirExpressionKind};
use radix_intrinsics::{IntrinsicReceiver, MethodIntrinsic};

use super::flow_range::{BoundKey, IntRange};
use super::flow_range_produce::{hi_const, lo_const};
use super::{DefId, ErrorSink, TypeChecker};

/// The expression a store node wraps, when the node only moves the value into
/// its slot (the interval is the operand's).
fn through_store(expr: &HirExpression) -> &HirExpression {
    match &expr.kind {
        HirExpressionKind::Widen { operand, .. } => through_store(operand),
        HirExpressionKind::Conversio {
            source,
            target: HirConversioTarget::Type(_),
            implicit: Some(_),
            ..
        } => through_store(source),
        _ => expr,
    }
}

/// `expr` as `binding + offset` for a literal offset, or `None`.
fn binding_offset(expr: &HirExpression) -> Option<(DefId, i128)> {
    let expr = through_store(expr);
    let literal = super::integer_range::const_int_value;
    match &expr.kind {
        HirExpressionKind::Path(def) => Some((*def, 0)),
        HirExpressionKind::Binary(HirBinOp::Add, lhs, rhs) => match (literal(rhs), literal(lhs)) {
            (Some(offset), _) => binding_offset(lhs).map(|(def, base)| (def, base + offset)),
            (None, Some(offset)) => binding_offset(rhs).map(|(def, base)| (def, base + offset)),
            (None, None) => None,
        },
        HirExpressionKind::Binary(HirBinOp::Sub, lhs, rhs) => {
            let offset = literal(rhs)?;
            binding_offset(lhs).map(|(def, base)| (def, base - offset))
        }
        _ => None,
    }
}

impl TypeChecker<'_> {
    /// Whether this failable list/text `at` or `slice` call is proven to stay
    /// inside its receiver, so it needs no handler. Call it after the
    /// arguments are checked and before the call's own kill.
    pub(super) fn list_text_access_proven(
        &self,
        entry: &MethodIntrinsic,
        receiver: &HirExpression,
        args: &[HirCallArg],
    ) -> bool {
        if !matches!(
            entry.receiver,
            IntrinsicReceiver::Lista | IntrinsicReceiver::Textus
        ) {
            return false;
        }
        let Some(source) = self.length_source(through_store(receiver)) else {
            return false;
        };
        let len = self.range_flow.live_length(source);
        let zero = BoundKey::Const(0);
        match (entry.faber_name, args) {
            ("apud", [index]) => {
                let index = self.range_of_expr(&index.expr);
                index.proves_at_least(&zero, false) && index.proves_at_most(&len, true)
            }
            ("sectio", [start, end]) => {
                let (start_range, end_range) = (
                    self.range_of_expr(&start.expr),
                    self.range_of_expr(&end.expr),
                );
                start_range.proves_at_least(&zero, false)
                    && end_range.proves_at_most(&len, false)
                    && self.window_ordered(&start.expr, &end.expr, &start_range, &end_range)
            }
            _ => false,
        }
    }

    /// Whether a `⊥` default or `fac`/`cape` body is open around the call.
    pub(super) fn has_local_error_sink(&self) -> bool {
        matches!(self.current_error, Some(ErrorSink::Local(_)))
    }

    /// Whether `start ≤ end` is proven.
    fn window_ordered(
        &self,
        start: &HirExpression,
        end: &HirExpression,
        start_range: &IntRange,
        end_range: &IntRange,
    ) -> bool {
        // Literal intervals that do not overlap.
        if let (Some(start_hi), Some(end_lo)) = (hi_const(start_range), lo_const(end_range))
            && start_hi <= end_lo
        {
            return true;
        }
        // One binding read twice, the end shifted no lower than the start.
        if let (Some((a, from)), Some((b, to))) = (binding_offset(start), binding_offset(end))
            && a == b
            && from <= to
        {
            return true;
        }
        // The start is bounded by the exact value the end names.
        match (&start_range.hi, self.exact_bound(end)) {
            (Some(hi), Some(exact)) => hi.key == exact,
            _ => false,
        }
    }

    /// The bound an expression's value equals exactly: a list/text live
    /// length, an immutable binding, or a literal.
    fn exact_bound(&self, expr: &HirExpression) -> Option<BoundKey> {
        let expr = through_store(expr);
        if let Some(value) = super::integer_range::const_int_value(expr) {
            return Some(BoundKey::Const(value));
        }
        match &expr.kind {
            HirExpressionKind::Path(def) => self
                .lookup_binding(*def)
                .is_some_and(|binding| !binding.mutable)
                .then_some(BoundKey::Binding(*def)),
            HirExpressionKind::MethodCall(receiver, name, _, args, _)
                if self.is_pure_length_call(receiver, *name, args) =>
            {
                let source = self.length_source(through_store(receiver))?;
                Some(self.range_flow.live_length(source))
            }
            _ => None,
        }
    }
}
