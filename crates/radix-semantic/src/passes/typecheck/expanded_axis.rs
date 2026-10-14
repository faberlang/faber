//! Probe and validation for the `expande([...], axis n)` broadcast-back mark.
//!
//! Need (operator ruling 2026-10-13): `s.expande([A, B, C], axis 1)`
//! on a rank-2 `[A, C]` tensor inserts a new dimension at `axis`; the other
//! dimensions line up in order. This is the mirror of `desugar_axis_reduction`
//! and follows its probe shape: the receiver is checked exactly once, and the
//! target list and axis are validated with named diagnostics. This unit only
//! probes and refuses; the rewrite lands in the next unit.
#[allow(clippy::wildcard_imports)]
use super::*;

const ISSUE_EXPANDED_AXIS_NOT_CONSTANT: &str = "expanded_axis_not_constant";
const ISSUE_EXPANDED_AXIS_RANK_MISMATCH: &str = "expanded_axis_rank_mismatch";
const ISSUE_EXPANDED_AXIS_OUT_OF_RANGE: &str = "expanded_axis_out_of_range";

/// Outcome of the `t.expande([...], axis n)` expansion probe.
pub(super) enum ExpandedAxis {
    /// Not an axis expansion call: checked as written.
    NotApplicable,
    /// A named diagnostic was reported; the node is typed as an error.
    Refused,
    /// The call was rewritten to the broadcast form.
    #[allow(dead_code)] // next: rewrite constructs this
    Expanded,
}

impl TypeChecker<'_> {
    /// Need: on a rank-R tensor, `t.expande([...], axis n)` inserts a
    /// new dimension at `n`, and the other dimensions line up in order. The
    /// target list must hold exactly R + 1 extents and `n` must be a
    /// compile-time constant in `0..=R`; each violation has a named
    /// diagnostic. Validation passing only admits the call; the rewrite is
    /// the next unit.
    pub(super) fn desugar_expanded_axis(&mut self, expr: &mut HirExpression) -> ExpandedAxis {
        let HirExpressionKind::MethodCall(receiver, name, type_args, args, conversion) =
            &mut expr.kind
        else {
            return ExpandedAxis::NotApplicable;
        };
        if !type_args.is_empty()
            || conversion.is_some()
            || args.len() != 2
            || args[1].marker != Some(radix_hir::HirArgMarker::Axis)
            || args[0].marker.is_some()
            || args[0].spread
            || args[0].name.is_some()
            || args[1].spread
            || args[1].name.is_some()
        {
            return ExpandedAxis::NotApplicable;
        }
        let Some(interner) = self.interner else {
            return ExpandedAxis::NotApplicable;
        };
        let surface = interner.resolve(*name).to_owned();
        let canonical = self
            .locale_pack
            .and_then(|pack| {
                pack.intrinsic_canonical(radix_intrinsics::IntrinsicReceiver::Tensor, &surface)
            })
            .map_or(surface, str::to_owned);
        if canonical != "expande" {
            return ExpandedAxis::NotApplicable;
        }
        let receiver_ty = self.check_expr(receiver);
        // The receiver is checked exactly once, whether the probe declines or
        // the call is rewritten (see `twin_probed_receiver`).
        self.twin_probed_receiver = Some((receiver.id, receiver_ty));
        let resolved = self.resolve_inferred_type(receiver_ty);
        let dispatch = self.types.peel_access_marker(resolved).1;
        let shape = match self.types.get(dispatch) {
            Type::Tensor(_, shape) => *shape,
            _ => return ExpandedAxis::NotApplicable,
        };
        let dims = match self.types.get_index(shape) {
            IndexExpr::Tuple(dims) => dims.clone(),
            _ => return ExpandedAxis::NotApplicable,
        };
        let rank = dims.len();
        let target_rank = match &args[0].expr.kind {
            HirExpressionKind::Array(elements) => {
                let mut count = 0usize;
                for element in elements {
                    match element {
                        HirArrayElement::Expr(_) => count += 1,
                        HirArrayElement::Spread(_) | HirArrayElement::Infer(_) => {
                            return ExpandedAxis::NotApplicable;
                        }
                    }
                }
                count
            }
            _ => return ExpandedAxis::NotApplicable,
        };
        let Some(axis) =
            super::integer_range::const_int_value_resolved(&args[1].expr, &self.const_int_values)
        else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_EXPANDED_AXIS_NOT_CONSTANT,
                args[1].span,
            );
            return ExpandedAxis::Refused;
        };
        if target_rank != rank + 1 {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_EXPANDED_AXIS_RANK_MISMATCH,
                args[0].span,
            );
            return ExpandedAxis::Refused;
        }
        if !(0..=rank as i128).contains(&axis) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_EXPANDED_AXIS_OUT_OF_RANGE,
                args[1].span,
            );
            return ExpandedAxis::Refused;
        }
        // next: rewrite
        ExpandedAxis::NotApplicable
    }
}
