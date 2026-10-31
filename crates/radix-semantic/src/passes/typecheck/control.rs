//! Typechecking for control expressions whose value type depends on nested
//! scopes, catches, or multiple arms.
//!
//! Control forms are where Faber's ordinary expression typing meets alternate
//! exits and pattern-sensitive scopes. This module keeps those concerns local:
//! handled blocks create a temporary error sink for `iace`, catches bind the
//! resulting error value, `si` merges branch result types, and `discerne`
//! checks pattern bindings before typing each arm body.
//!
//! INVARIANTS
//! ==========
//! - Catch bindings are immutable locals scoped only to the catch body.
//! - A handled body's alternate-exit type is inferred independently from its
//!   normal result type.
//! - Conditions must be `bivalens`; expected expression type propagation does
//!   not make non-boolean conditions acceptable.
//! - Match arms are checked in isolated scopes so pattern bindings never leak
//!   between arms.
//! - Empty matches type to `vacuum`; non-empty matches use the common type of
//!   arm bodies.
//! - Null-check branches carry a temporary flow refinement: `x non est nihil`
//!   records `x : T` in the taken branch and `x est nihil` records it in the
//!   else branch (nn-u1). Branch edges are joined after checking: a fact
//!   persists only when every normal edge carries the same selected member;
//!   diverging edges contribute nothing.
//!   Compound conditions are conservative (nn-u2): `x non est nihil et …`
//!   narrows the taken branch only (the conjunction requires the left
//!   null-check conjunct), while `or` and non-`and` shapes never narrow.
//!   A genus-field path (`g.nomen`) narrows inside a branch body only; joins
//!   and the implicit fallthrough conservatively drop field facts.
//! - U1R generalizes the seam to nominal member tests: `r est M` narrows the
//!   taken branch to `M` and the fallthrough to the union minus `M` (`non est
//!   M` mirrors), on immutable bindings only (varia refinement is open row 2).
//!   `adfirma <cond>` narrows the rest of its block through an override frame
//!   restored at block exit — a failed assert aborts, so control never
//!   continues past it with the un-narrowed type.
//! - N3 drops all recorded facts when a binding is assigned. Loop bodies drop
//!   facts for bindings they touch before entry; untouched immutable facts may
//!   enter as loop-invariant knowledge.

use super::flow_range::{Endpoint, FactValue, IntRange, RangeSnapshot};
use super::store::StoreSite;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::HirStoreKind;
use radix_hir::HirUnOp;
use radix_hir::visit::{HirVisitor, walk_expr, walk_stmt};

/// A snapshot of the visible binding state at one control-flow edge.
///
/// The map is deliberately keyed by bindings, not types as an algebraic
/// lattice. Assignment markers are kept separately so a branch that writes a
/// binding cannot accidentally rejoin with a stale fact from another edge.
#[derive(Clone, Default)]
struct FlowState {
    bindings: FxHashMap<DefId, BindingInfo>,
    invalidated: FxHashSet<DefId>,
    /// Integer interval facts and mutation versions at this edge. Joined by
    /// interval hull, independently of the type facts in `bindings`.
    ranges: RangeSnapshot,
}

/// One leaf condition's `(then_refinement, else_refinement)` pair (nn-u1 null
/// checks, U1R nominal `est M` tests): each side is an optional
/// `(FactKey, FactValue)` fact, keyed by the binding or the field path rooted
/// at a binding that the fact refines. The value carries a narrowed type
/// and/or an integer interval.
pub(super) type LeafRefinement = (Option<(FactKey, FactValue)>, Option<(FactKey, FactValue)>);

/// The facts that hold on one edge of a condition: every `(FactKey, FactValue)`
/// that is true when the edge is taken, keyed by the binding or the field path
/// rooted at a binding that the fact refines.
type Facts = Vec<(FactKey, FactValue)>;

/// The subject of a flow fact: a visible binding, or a field path rooted at a
/// visible binding (`base.path…`).
// Field facts are applied only inside their proving branch; joins and assert
// frames currently carry binding facts only.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum FactKey {
    Binding(DefId),
    Field { base: DefId, path: Vec<Symbol> },
}

/// `(then_facts, else_facts)` for a whole condition, including compound
/// `and` / `or` / `not` shapes.
type ConditionRefinement = (Facts, Facts);

pub(super) const ISSUE_TRAP_MEMBER_TYPE_MISMATCH: &str = "trap_member_type_mismatch";
const ISSUE_TRAP_ERROR_CHANNEL_EMPTY: &str = "trap_error_channel_empty";
const ISSUE_CALL_DEFAULT_ON_INFALLIBLE_CALL: &str = "call_default_on_infallible_call";
const ISSUE_CALL_DEFAULT_TYPE_MISMATCH: &str = "call_default_type_mismatch";

impl TypeChecker<'_> {
    /// Check a catch clause for a handled alternate exit.
    ///
    /// If no `iace` expression constrained the local error inference variable,
    /// the catch still receives a usable binding typed as `ignotum`. That keeps
    /// error-channel handling explicit without forcing unreachable catches to
    /// manufacture a concrete error type.
    pub fn check_cape(&mut self, catch: &mut HirCape, err_ty: TypeId) {
        let err_ty = match self.resolve_inferred_type(err_ty) {
            resolved if self.is_infer(resolved) => self.types.primitive(Primitive::Ignotum),
            resolved => resolved,
        };
        catch.binding_ty = Some(err_ty);

        self.push_scope();
        self.insert_binding(catch.binding_def_id, err_ty);
        self.check_block(&mut catch.body, None);
        self.pop_scope();
    }

    /// Check a block with an attached catch and return the block's normal type.
    ///
    /// The temporary `ErrorSink::Local` captures the type of values thrown by
    /// `iace` inside the body. The catch consumes that alternate-exit type, while
    /// the expression as a whole keeps the body's normal result type.
    pub fn check_handled_block(
        &mut self,
        body: &mut HirBlock,
        catch: &mut HirCape,
        expected: Option<TypeId>,
    ) -> TypeId {
        let err_ty = self.fresh_infer();
        let prev_error = self.current_error;
        self.current_error = Some(ErrorSink::Local(err_ty));
        let body_ty = self.check_block(body, expected);
        self.current_error = prev_error;

        self.check_cape(catch, err_ty);
        body_ty
    }

    /// Check a `capta { ... }` capture boundary (UT): the error channel
    /// reified into the tagged built-in genus `trapped<R, T>` (ruling
    /// f9d8a611).
    ///
    /// Typing is all reuse — no new inference: the body's success type is the
    /// `R` slot; every failable call's error type and `iace` payload inside
    /// the body accumulates into one fresh `Type::InferUnion` hole through
    /// the existing U-02 arms (`ErrorSink::Trap` + the third
    /// `reject_failable_call` answer), and that solved hole is the `T` slot;
    /// finalize solves it through the existing normalization. `R` and `T` are
    /// separate slots, so a success and an error of the same type stay
    /// distinguishable — the variants carry the discrimination, and the old
    /// union-collapse guard is retired with the untagged result form.
    ///
    /// An infallible body leaves the `T` slot uninhabited: it types as
    /// `numquam` (the `thrown` variant can never occur) and carries its own
    /// warning (`trap_error_channel_empty`), not an inference error.
    pub fn check_trap(&mut self, body: &mut HirBlock, span: radix_lexer::Span) -> TypeId {
        let var = self.fresh_union_hole();

        let prev_error = self.current_error.replace(ErrorSink::Trap(var));
        let prev_flow = std::mem::replace(&mut self.trap_error_flow, false);
        let success_ty = self.check_block(body, None);
        self.current_error = prev_error;
        let error_flowed = std::mem::replace(&mut self.trap_error_flow, prev_flow);

        // The T slot solves to exactly the escaping error types — one member
        // is the healthy single-error-channel case, so WARN017 never applies
        // to it (the empty-channel warning below owns the degenerate case).
        self.trap_error_holes.insert(var);

        let error_ty = if error_flowed {
            self.solve_trap_error_slot(var)
        } else {
            self.push_warning(
                WarningKind::TrapErrorChannelEmpty,
                ISSUE_TRAP_ERROR_CHANNEL_EMPTY,
                span,
                &[],
            );
            self.types.primitive(Primitive::Numquam)
        };

        let Some(trapped_def) = self.resolver.builtin_trapped_def_id() else {
            // The genus registers before any declaration is collected, so
            // this is a compiler-ordering bug, not a source error; recover
            // with the error type so later phases fail closed.
            debug_assert!(
                false,
                "builtin trapped genus must register before check_trap"
            );
            return self.error_type;
        };

        crate::builtins::trapped::applied_type(self.types, trapped_def, success_ty, error_ty)
    }

    /// Solve a closed `capta` boundary's `T` slot from its accumulated member
    /// set.
    ///
    /// The set is complete when the boundary closes (`ErrorSink::Trap` is
    /// restored), so the slot can solve before finalize: arms over the
    /// `trapped` result then bind payloads against the solved error type,
    /// not an open hole. Mirrors the finalize union-hole law (resolve each
    /// member, drop error recovery, dedup via `equals`, keep accumulation
    /// order) without touching the canonicalization cache: a still-pending
    /// member set stays the open hole so finalize owns the
    /// missing-annotation path.
    fn solve_trap_error_slot(&mut self, var: InferVar) -> TypeId {
        let members: Vec<TypeId> = {
            let borrowed = self.union_members.borrow();
            borrowed.get(&var).cloned().unwrap_or_default()
        };
        let mut distinct: Vec<TypeId> = Vec::new();
        for member in members {
            let member = self.resolve_inferred_type(member);
            if member != self.error_type
                && !distinct.iter().any(|seen| self.types.equals(*seen, member))
            {
                distinct.push(member);
            }
        }
        match distinct.as_slice() {
            [] => self.union_hole_type_id(var),
            [only] => *only,
            _ => self.types.intern(Type::Union(distinct)),
        }
    }

    /// Check `call ⊥ default` (D17.7).
    ///
    /// The body (the call) runs under a local error sink, exactly like a
    /// `fac` body: the call's `⇥` channel is the handled channel. A sink that
    /// never received an error type means nothing inside can fail, which is
    /// the infallible-call rejection. The default is checked afterwards,
    /// outside the sink (a failable default needs its own handler), against
    /// the call's success type. The solved error type is recorded on the
    /// node for backends.
    pub fn check_call_default(
        &mut self,
        body: &mut HirBlock,
        default: &mut HirExpression,
        err_slot: &mut Option<TypeId>,
        expected: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> TypeId {
        let err_ty = self.fresh_infer();
        let prev_error = self.current_error.replace(ErrorSink::Local(err_ty));
        let body_ty = self.check_block(body, expected);
        self.current_error = prev_error;

        let resolved_err = self.resolve_inferred_type(err_ty);
        if self.is_infer(resolved_err) {
            self.invalid_operand_types(ISSUE_CALL_DEFAULT_ON_INFALLIBLE_CALL, span);
            self.check_expr(default);
            return self.error_type;
        }
        *err_slot = Some(resolved_err);

        let success_ty = self.resolve_inferred_type(body_ty);
        let site = StoreSite::new(HirStoreKind::ConversionDefault);
        let default_ty = self.check_store_expr(default, success_ty, site);
        self.unify_assign_issue(
            default_ty,
            success_ty,
            default.span,
            ISSUE_CALL_DEFAULT_TYPE_MISMATCH,
        );
        body_ty
    }

    /// Check a `discerne` expression and merge arm result types.
    ///
    /// Scrutinees are typed once, then each arm receives a fresh scope for its
    /// pattern bindings and guard. Pattern checking binds names to the matched
    /// scrutinee/field types, while the arm body is checked against the caller's
    /// expected expression type so match expressions participate in
    /// bidirectional inference.
    pub fn check_match(
        &mut self,
        scrutinees: &mut [HirExpression],
        arms: &mut [HirCasuArm],
        expected: Option<TypeId>,
    ) -> TypeId {
        let scrutinee_tys: Vec<_> = scrutinees
            .iter_mut()
            .map(|scrutinee| self.check_expr(scrutinee))
            .collect();
        let mut result_ty = None;
        let mut result_const = None;

        for arm in arms {
            self.push_scope();
            let mut refinements = Vec::new();
            for (index, (pattern, scrutinee_ty)) in arm
                .patterns
                .iter_mut()
                .zip(scrutinee_tys.iter().copied())
                .enumerate()
            {
                let narrowed =
                    super::pattern::resolve_type_test_target(self, pattern, scrutinee_ty);
                self.check_pattern(pattern, scrutinee_ty, arm.span);
                if let (Some(narrowed), Some(HirExpressionKind::Path(def_id))) = (
                    narrowed,
                    scrutinees.get(index).map(|scrutinee| &scrutinee.kind),
                ) {
                    refinements.push((*def_id, narrowed));
                }
            }
            for (def_id, narrowed) in refinements {
                self.override_binding(def_id, narrowed);
            }
            let body_ty = self.check_expr_with_expected(&mut arm.body, expected);
            let body_const = Self::const_join_range(&arm.body);
            result_ty = Some(match result_ty {
                None => {
                    result_const = body_const;
                    body_ty
                }
                Some(existing) => {
                    let joined = self.common_type_with_consts(
                        (existing, result_const),
                        (body_ty, body_const),
                        arm.span,
                    );
                    result_const = match (result_const, body_const) {
                        (Some(a), Some(b)) => Some(radix_types::numeric_range::hull(a, b)),
                        _ => None,
                    };
                    joined
                }
            });
            self.pop_scope();
        }

        result_ty.unwrap_or_else(|| self.vacuum_type())
    }

    /// Check a condition expression under the language's boolean requirement.
    ///
    /// The expected type helps shape unresolved expressions, but the final
    /// predicate still checks the resolved result so permissive inference cannot
    /// silently accept a non-`bivalens` condition. A condition that observes a
    /// target-native import value (need, the condition half of the
    /// observation law) is the one exemption: an `ignotum` condition converts
    /// with implicit syntax but forced semantics, so the target's own truthiness
    /// judges it and the checker accepts it instead of repainting it.
    pub fn check_condition(&mut self, cond: &mut HirExpression) {
        let bivalens = self.bool_type();
        // WHY: no `bivalens` hint when an import value is observed — the hint
        // would repaint the `ignotum` result and report the mismatch it was
        // supposed to exempt.
        let cond_ty = if self.condition_may_observe_import(cond) {
            self.check_expr(cond)
        } else {
            self.check_expr_with_expected(cond, Some(bivalens))
        };
        let resolved = self.resolve_inferred_type(cond_ty);
        if resolved == self.error_type {
            return;
        }
        if self.is_bool(cond_ty) {
            return;
        }
        if matches!(
            self.types.get(resolved),
            Type::Primitive(Primitive::Ignotum)
        ) {
            return;
        }
        // An open hole binds to `bivalens`; anything else stays a mask report.
        if self.is_infer(resolved) {
            self.unify_assign_issue(cond_ty, bivalens, cond.span, "expression_type_mismatch");
            return;
        }
        self.report_mask_not_truth_value(cond_ty, cond.span);
    }

    /// Whether `cond` observes a target-native import value: a path to an
    /// `ignotum` binding anywhere in the condition, including either side of an
    /// ordering comparison, since an ordering with an `ignotum` operand yields
    /// `ignotum` rather than `bivalens`.
    fn condition_may_observe_import(&self, cond: &HirExpression) -> bool {
        struct IgnotumPathProbe<'a, 'b> {
            checker: &'a TypeChecker<'b>,
            found: bool,
        }

        impl HirVisitor for IgnotumPathProbe<'_, '_> {
            fn visit_expr(&mut self, expr: &HirExpression) {
                if self.found {
                    return;
                }
                if let HirExpressionKind::Path(def_id) = &expr.kind
                    && self.checker.path_reads_as_ignotum(*def_id)
                {
                    self.found = true;
                    return;
                }
                walk_expr(self, expr);
            }
        }

        let mut probe = IgnotumPathProbe {
            checker: self,
            found: false,
        };
        probe.visit_expr(cond);
        probe.found
    }

    /// Whether a resolved path reads as `ignotum`, either because it names a
    /// target-native import receiver or because its visible binding is typed
    /// `ignotum`.
    fn path_reads_as_ignotum(&self, def_id: DefId) -> bool {
        if self.import_receivers.contains(&def_id) {
            return true;
        }
        let Some(binding) = self.lookup_binding(def_id) else {
            return false;
        };
        let ty = self.resolve_inferred_type(binding.ty);
        let ty = self.peel_nominal_aliases(ty);
        let ty = self.types.peel_access_marker(ty).1;
        matches!(
            self.types.get(self.peel_nominal_aliases(ty)),
            Type::Primitive(Primitive::Ignotum)
        )
    }

    /// [`Self::check_condition`] for a condition that may be a select mask: a
    /// tensor, vector or matrix condition is returned checked and unreported, so the
    /// caller can treat it as the mask of a `✓ ✗` select; any other condition
    /// is checked as `bivalens` and yields `None`.
    fn check_condition_or_mask(&mut self, cond: &mut HirExpression) -> Option<TypeId> {
        // WHY: checked without the `bivalens` hint, because a hint mismatch
        // replaces the checked type with the error type and the mask type
        // would be lost. The hint only shapes still-open holes, which the
        // unification below binds to `bivalens` all the same.
        let cond_ty = self.check_expr(cond);
        let resolved = self.resolve_inferred_type(cond_ty);
        if resolved == self.error_type {
            return None;
        }
        if self.is_bool(cond_ty) {
            return None;
        }
        // A target-native import value observed at the condition is not a
        // select mask and not a truth value: the target's own truthiness
        // judges it (need, the condition half of the observation
        // law), so it is accepted unreported.
        if matches!(
            self.types.get(self.peel_nominal_aliases(resolved)),
            Type::Primitive(Primitive::Ignotum)
        ) {
            return None;
        }
        if self.is_aggregate_mask_type(cond_ty) {
            return Some(cond_ty);
        }
        // A `lista<bivalens>` is a mask, never a truth value and never a
        // select mask: name the reductions.
        if self.is_bool_list_mask(cond_ty) {
            self.report_mask_not_truth_value(cond_ty, cond.span);
            return None;
        }
        // Same report as the hinted check: an open hole binds to `bivalens`,
        // anything else is one expression mismatch (no follow-on error).
        let bivalens = self.bool_type();
        self.unify_assign_issue(cond_ty, bivalens, cond.span, "expression_type_mismatch");
        None
    }

    /// Check an `si` expression and compute its normal result type.
    ///
    /// A catch attached to the then-branch captures only alternate exits from
    /// that branch. The final `si` type is still the common type of the normal
    /// then and else results, with a missing else branch contributing `vacuum`.
    ///
    /// nn-u1 flow refinement: when the condition is a direct null check on a
    /// visible binding (`x non est nihil` / `x est nihil`), the taken branch is
    /// checked with the binding temporarily recorded as its non-null core
    /// `T`; the fallthrough keeps `T ∪ null`. Compound conditions stay
    /// compositional: `et` narrows the taken branch with every
    /// conjunct, `aut` narrows the fallthrough with every disjunct's negation,
    /// `non` swaps. The override is restored before the branch
    /// type is joined, so the narrowed type never escapes its block.
    pub fn check_if(
        &mut self,
        cond: &mut HirExpression,
        then_block: &mut HirBlock,
        then_catch: Option<&mut HirCape>,
        else_block: Option<&mut HirBlock>,
        expected: Option<TypeId>,
    ) -> TypeId {
        // The condition is checked once against the incoming facts. Both edge
        // refinements must be computed before either branch can mutate the
        // visible binding map.
        let mut incoming = self.snapshot_flow_state();
        self.remember_flow_bases(&incoming);
        // A `tensor<bivalens, S>` condition is the mask of the `✓ ✗` select
        // (K4), not a branch condition: both branches are evaluated.
        let mask = self.check_condition_or_mask(cond);
        // The condition runs on every path: a call inside it kills interval
        // facts before either edge starts, so both edges begin from the facts
        // that survive it, not from the pre-condition snapshot.
        incoming.ranges = self.range_flow.snapshot();
        if let Some(mask_ty) = mask {
            if then_catch.is_some() {
                self.report_mask_not_truth_value(mask_ty, cond.span);
                return self.error_type;
            }
            return self.check_tensor_select(cond, mask_ty, then_block, else_block, expected);
        }
        let (then_refine, else_refine) = self.condition_refinement(cond);

        let block_expected = self.branch_expected(expected);
        let then_const = then_block.expr.as_deref().and_then(Self::const_join_range);
        let else_const = else_block
            .as_deref()
            .and_then(|block| block.expr.as_deref())
            .and_then(Self::const_join_range);
        let (then_ty, then_state) = if let Some(catch) = then_catch {
            let err_ty = self.fresh_infer();
            let prev_error = self.current_error;
            self.current_error = Some(ErrorSink::Local(err_ty));
            let result = self.check_block_with_refinement(then_block, block_expected, then_refine);
            self.current_error = prev_error;
            self.check_cape(catch, err_ty);
            result
        } else {
            self.check_block_with_refinement(then_block, block_expected, then_refine)
        };
        let then_diverges = self.block_diverges(then_block);

        // Restore the incoming edge before checking the fallthrough. A missing
        // else is still a real normal edge: it carries the incoming facts plus
        // any refinement proven by the negated condition.
        self.restore_flow_state(&incoming);
        let (else_ty, else_state, else_diverges) = if let Some(block) = else_block {
            let (ty, state) = self.check_block_with_refinement(block, block_expected, else_refine);
            (ty, state, self.block_diverges(block))
        } else {
            let state = self.state_with_refinement(else_refine);
            (self.vacuum_type(), state, false)
        };

        // Apply the ruled join to the statement's continuation. Diverging
        // edges contribute nothing; normal edges retain a fact only when every
        // surviving edge carries the same selected member.
        self.restore_flow_state(&incoming);
        if !(then_diverges && else_diverges) {
            let normal_states: Vec<&FlowState> = [
                (!then_diverges).then_some(&then_state),
                (!else_diverges).then_some(&else_state),
            ]
            .into_iter()
            .flatten()
            .collect();
            let joined = self.join_flow_states(&incoming, &normal_states);
            self.apply_flow_state(&incoming, &joined);
        }

        // U-04: under a `∪`-hole expectation the branch join is an
        // accumulation, not a monomorphic `common_type` join. Each branch
        // already flowed its type into the hole var through `check_block`
        // (with `expected`); the final join keeps the hole open. The issue
        // tag mirrors `common_type`'s; a hole-vs-hole join short-circuits in
        // `unify_with_policy` before any diagnostic can fire.
        if let Some(expected) = expected
            && self
                .infer_union_var_of(self.resolve_inferred_type(expected))
                .is_some()
        {
            return self.unify_join(then_ty, else_ty, cond.span, "common_type_mismatch");
        }

        self.common_type_with_consts((then_ty, then_const), (else_ty, else_const), cond.span)
    }

    /// Detect a narrowing condition on a visible immutable binding and compute
    /// the refinement for each branch of the `si` (nn-u1/nn-u2, U1R).
    ///
    /// `x non est nihil` narrows the binding to its non-null core in the taken
    /// (then) branch; `x est nihil` narrows identically in the fallthrough
    /// (else) branch (Q3). The untaken side of each form keeps `T ∪ null`, so
    /// only one side of the null pair is ever set.
    ///
    /// U1R adds the `TypeCheck` node arm: `r est M` narrows the taken branch
    /// to `M` and the fallthrough to the union minus `M`; `r non est M`
    /// mirrors. A member test on a union that lacks `M` refines nothing (the
    /// dead-test WARN fires at the expression site).
    ///
    /// Compound conditions compose the facts of their parts.
    /// `A et B` holds both parts' then-facts in the taken branch, at any
    /// depth; its falsity forces neither part, so the else edge learns
    /// nothing (`x est nihil et y` can be false while `x` is null). `A aut B`
    /// is the mirror: both parts' else-facts hold when it is false (the
    /// fallthrough of a diverging `si A aut B { redde }`), and its truth
    /// forces neither part. `non (...)` swaps the two edges.
    ///
    /// Returns `(then_facts, else_facts)` for the condition. Each edge carries
    /// `(FactKey, FactValue)` pairs for visible bindings or rooted field paths.
    fn condition_refinement(&mut self, cond: &HirExpression) -> ConditionRefinement {
        match &cond.kind {
            // Both conjuncts hold when `A and B` is true, at any depth. Its
            // falsity forces neither conjunct, so the else edge learns nothing.
            //
            // The facts are read from the flow state as it stands once the
            // whole condition has been checked, so an effect in a conjunct
            // evaluated later than a fact's conjunct has already moved the
            // state under it: that fact's interval half dies.
            HirExpressionKind::Binary(HirBinOp::And, lhs, rhs) => {
                let (mut then_facts, _) = self.condition_refinement(lhs);
                self.drop_ranges_killed_by(&mut then_facts, rhs);
                then_facts.extend(self.condition_refinement(rhs).0);
                (then_facts, Vec::new())
            }
            // Both disjuncts are false when `A or B` is false, so the else
            // edge (and the fallthrough of a diverging `si A aut B`) carries
            // both negations. Its truth forces neither disjunct.
            HirExpressionKind::Binary(HirBinOp::Or, lhs, rhs) => {
                let (_, mut else_facts) = self.condition_refinement(lhs);
                self.drop_ranges_killed_by(&mut else_facts, rhs);
                else_facts.extend(self.condition_refinement(rhs).1);
                (Vec::new(), else_facts)
            }
            // `non (...)` swaps the edges.
            HirExpressionKind::Unary(HirUnOp::Not, inner) => {
                let (then_facts, else_facts) = self.condition_refinement(inner);
                (else_facts, then_facts)
            }
            _ => {
                let (then_fact, else_fact) = self.leaf_refinement(cond);
                let mut then_facts: Facts = then_fact.into_iter().collect();
                let mut else_facts: Facts = else_fact.into_iter().collect();
                // An effect inside the leaf itself (`i ≺ f(mut xs)`) runs
                // after the operands the fact was read from.
                self.drop_ranges_killed_by(&mut then_facts, cond);
                self.drop_ranges_killed_by(&mut else_facts, cond);
                (then_facts, else_facts)
            }
        }
    }

    /// Take the interval half out of every fact that `later`, an expression
    /// evaluated after the fact was established, can invalidate: a fact about
    /// a binding it stores to or (when it calls, or stores through a
    /// projection) about any mutable source, and every endpoint bounded by the
    /// live length of a list or mutable source. The same rule as
    /// [`Self::kill_mutable_source_ranges`], applied to facts the checker
    /// rebuilds from syntax after the effects already ran. A narrowed type is
    /// left alone: this is the interval half only.
    fn drop_ranges_killed_by(&self, facts: &mut Facts, later: &HirExpression) {
        if facts.iter().all(|(_, value)| value.range.is_none()) {
            return;
        }
        let read_only = |receiver: &HirExpression, name: Symbol, arg_count: usize| {
            self.is_read_only_intrinsic_call(receiver, name, arg_count)
        };
        let mut effects = AssignmentTargetCollector {
            read_only: Some(&read_only),
            ..Default::default()
        };
        effects.visit_expr(later);
        if effects.targets.is_empty() && !effects.kills_ranges {
            return;
        }
        let is_mutable = |def_id: DefId| {
            self.binding_shadow
                .get(&def_id)
                .is_none_or(|info| info.mutable)
        };
        for (key, value) in facts.iter_mut() {
            let Some(range) = value.range.take() else {
                continue;
            };
            let FactKey::Binding(subject) = key else {
                continue;
            };
            if effects.targets.contains(subject) || (effects.kills_ranges && is_mutable(*subject)) {
                continue;
            }
            let strip = |endpoint: Option<Endpoint>| {
                endpoint.filter(|endpoint| {
                    !endpoint
                        .key
                        .live_length_source()
                        .is_some_and(|(source, _)| {
                            effects.kills_ranges
                                && (is_mutable(source) || self.is_list_source(source))
                        })
                })
            };
            let range = IntRange {
                lo: strip(range.lo),
                hi: strip(range.hi),
            };
            if !range.is_unbounded() {
                value.range = Some(range);
            }
        }
        facts.retain(|(_, value)| value.ty.is_some() || value.range.is_some());
    }

    /// Refinement for one non-compound condition (null probe, member test,
    /// or null equality).
    fn leaf_refinement(&mut self, cond: &HirExpression) -> LeafRefinement {
        // Integer guards (`x ≺ n`, `x ≡ y`, …) carry interval facts; any other
        // leaf falls through to the null and member probes.
        if let Some(guard) = self.guard_refinement(cond) {
            return guard;
        }
        if let HirExpressionKind::TypeCheck { positive, expr, ty } = &cond.kind {
            // A type test against the null type is the null-presence probe:
            // `x non est nihil` narrows the taken branch, `x est nihil` the
            // fallthrough. Every other target is a member test.
            if self.type_is_nihil(*ty) {
                return self.nil_probe_refinement(*positive, expr);
            }
            return self.type_check_refinement(*positive, expr, *ty);
        }
        let HirExpressionKind::Binary(op, lhs, rhs) = &cond.kind else {
            return (None, None);
        };
        // `x ≡ nulla` / `x ≢ nulla` (either operand order) is the null probe
        // spelled as value equality; it narrows exactly like `x est nihil` /
        // `x non est nihil`.
        if let Some(equals) = match op {
            HirBinOp::Eq | HirBinOp::StrictEq => Some(true),
            HirBinOp::NotEq | HirBinOp::StrictNotEq => Some(false),
            _ => None,
        } {
            let is_nil = |expr: &HirExpression| {
                matches!(expr.kind, HirExpressionKind::Literal(HirLiteral::Nil))
            };
            let operand = match (is_nil(lhs), is_nil(rhs)) {
                (false, true) => lhs,
                (true, false) => rhs,
                _ => return (None, None),
            };
            return self.nil_probe_refinement(equals, operand);
        }
        (None, None)
    }

    /// Return whether a type id is exactly the null type `nihil`.
    fn type_is_nihil(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Primitive(Primitive::Nihil)
        )
    }

    /// Refinement for a type test against the null type (`x non est nihil`
    /// narrows the taken branch, `x est nihil` the fallthrough, nn-u1).
    ///
    /// The operand is a binding, or a genus-field path rooted at a binding,
    /// whose leaf type peels a null member. An access-marker wrapper (Option
    /// A: `ref Node ∪ none cur` lowers to `Ref(Option(Node))`) rides along:
    /// the probe peels it, narrows the payload, and re-wraps the marker so a
    /// narrowed `ref` cursor stays read-only through (the write-through
    /// refusal still sees the marker).
    fn nil_probe_refinement(&mut self, positive: bool, expr: &HirExpression) -> LeafRefinement {
        let Some((base, path)) = field_path_key(expr) else {
            return (None, None);
        };
        let Some(mut ty) = self.lookup_binding(base).map(|binding| binding.ty) else {
            return (None, None);
        };
        for index in 1..=path.len() {
            let known = self
                .field_facts
                .get(&(base, path[..index].to_vec()))
                .copied();
            if let Some(known) = known {
                ty = known;
                continue;
            }
            let Some(next) = self.try_genus_field_type(ty, path[index - 1]) else {
                return (None, None);
            };
            ty = next;
        }
        let (marker, payload_ty) = self.types.peel_access_marker(ty);
        let nihil = self.types.primitive(Primitive::Nihil);
        let Some(refined_payload) = self.peel_member_type(payload_ty, nihil) else {
            return (None, None);
        };
        let refined = match marker {
            Some(marker) => self.types.access_marker(marker, refined_payload),
            None => refined_payload,
        };
        let key = if path.is_empty() {
            FactKey::Binding(base)
        } else {
            FactKey::Field { base, path }
        };
        if positive {
            (None, Some((key, FactValue::ty(refined))))
        } else {
            (Some((key, FactValue::ty(refined))), None)
        }
    }

    /// Refinement for a `TypeCheck` condition (`r est M` / `r non est M`,
    /// U1R).
    ///
    /// Only an immutable binding whose type is a union containing `M`
    /// narrows (varia refinement needs reassignment invalidation — open row
    /// 2, immutable-first). The taken branch of the positive test is `M`
    /// itself; the fallthrough is the union minus `M`, collapsing to the
    /// single remaining member and left unrefined when nothing remains (the
    /// statically-unreachable branch). The negative test mirrors.
    fn type_check_refinement(
        &mut self,
        positive: bool,
        expr: &HirExpression,
        member: TypeId,
    ) -> LeafRefinement {
        let HirExpressionKind::Path(def_id) = &expr.kind else {
            return (None, None);
        };
        let Some(binding) = self.lookup_binding(*def_id) else {
            return (None, None);
        };
        if binding.mutable {
            return (None, None);
        }
        let binding_ty = self.resolve_inferred_type(binding.ty);
        let Some(members) = self.union_members_of(binding_ty) else {
            return (None, None);
        };
        let member = self.resolve_inferred_type(member);
        if !members
            .iter()
            .copied()
            .any(|candidate| self.member_type_equals(candidate, member))
        {
            return (None, None);
        }
        let remainder = self.peel_member_type(binding_ty, member);
        if positive {
            (
                Some((FactKey::Binding(*def_id), FactValue::ty(member))),
                remainder.map(|ty| (FactKey::Binding(*def_id), FactValue::ty(ty))),
            )
        } else {
            (
                remainder.map(|ty| (FactKey::Binding(*def_id), FactValue::ty(ty))),
                Some((FactKey::Binding(*def_id), FactValue::ty(member))),
            )
        }
    }

    /// WARN when a `TypeCheck` tests a member its operand union cannot hold
    /// (U1R membership validation).
    ///
    /// A statically-false test is a dead test — a warning, not an error, so
    /// the `Binary(Is)` value-equality semantics stay untouched; only the
    /// `TypeCheck` node gains this check. Non-union operands keep today's
    /// pass-through behavior.
    pub(crate) fn warn_dead_type_test(
        &mut self,
        operand_ty: TypeId,
        member: TypeId,
        span: radix_lexer::Span,
    ) {
        let Some(members) = self.union_members_of(operand_ty) else {
            return;
        };
        let member = self.resolve_inferred_type(member);
        if members
            .iter()
            .copied()
            .any(|candidate| self.member_type_equals(candidate, member))
        {
            return;
        }
        let member_display = self.display_union_constituent(member);
        let operand_display = members
            .iter()
            .map(|candidate| self.display_union_constituent(*candidate))
            .collect::<Vec<_>>()
            .join(" ∪ ");
        self.push_warning(
            WarningKind::TypeTestMemberAbsent,
            "type_test_member_absent",
            span,
            &[("type", member_display), ("union", operand_display)],
        );
    }

    /// Alias-transparent structural equality for union member comparisons.
    pub(crate) fn member_type_equals(&self, lhs: TypeId, rhs: TypeId) -> bool {
        let lhs = self.resolve_inferred_type(lhs);
        let rhs = self.resolve_inferred_type(rhs);
        lhs == rhs || self.types.equals(lhs, rhs)
    }

    /// Peel one member from a union type, when the type is representable as a
    /// member-test narrowing (nn-u1 generalized by U1R).
    ///
    /// `Option(T)` narrows to `T` only for the implicit `nihil` member; a
    /// union narrows to the union of its remaining members (collapsed to the
    /// single member when only one remains). Types with no such layer — and
    /// the degenerate union that is entirely the peeled member, which has
    /// nothing left to narrow to — return `None` so callers leave the binding
    /// unchanged.
    fn peel_member_type(&mut self, ty: TypeId, member: TypeId) -> Option<TypeId> {
        let resolved = self.resolve_inferred_type(ty);
        if let Type::Option(inner) = self.types.get(resolved) {
            // Only the implicit `nihil` member peels an `Option` layer.
            let nihil = self.types.primitive(Primitive::Nihil);
            let member = self.resolve_inferred_type(member);
            return (self.types.equals(member, nihil) || member == nihil).then_some(*inner);
        }
        let mut remaining: Vec<TypeId> = self
            .union_members_of(resolved)?
            .into_iter()
            .filter(|candidate| !self.member_type_equals(*candidate, member))
            .collect();
        match remaining.len() {
            0 => None,
            1 => Some(remaining.remove(0)),
            _ => {
                remaining.sort_unstable_by_key(|candidate| candidate.0);
                Some(self.types.intern(Type::Union(remaining)))
            }
        }
    }

    /// Check a branch block under an optional flow refinement.
    ///
    /// Branches need one extra observation point: nested control forms may
    /// establish a fact in the branch's own scope, and the ruled outer join
    /// must see it before that scope is discarded. The ordinary block checker
    /// remains the owner of normal block entry/exit; this helper mirrors its
    /// small traversal so it can snapshot the edge immediately before cleanup.
    fn check_block_with_refinement(
        &mut self,
        block: &mut HirBlock,
        expected: Option<TypeId>,
        refine: Facts,
    ) -> (TypeId, FlowState) {
        let mut saved: Vec<(DefId, Option<BindingInfo>)> = Vec::new();
        let mut field_saved: Vec<(DefId, Vec<Symbol>)> = Vec::new();
        // Interval facts are scoped to the branch the same way: whatever the
        // block establishes or kills is captured in `state` below, and the
        // entry facts return at block exit.
        let ranges_at_entry = self.range_flow.snapshot();
        for (key, value) in refine {
            self.assume_range_fact(&key, &value);
            let Some(ty) = value.ty else { continue };
            match key {
                // A binding fact overrides the visible binding for the branch
                // body; the incoming binding returns when the scope ends.
                FactKey::Binding(def_id) => {
                    saved.push((def_id, self.override_binding(def_id, ty)));
                }
                // A field fact is visible only inside the branch body: it is
                // inserted here and removed at block exit, so it never reaches
                // the branch join or the implicit fallthrough.
                FactKey::Field { base, path } => {
                    self.field_facts.insert((base, path.clone()), ty);
                    field_saved.push((base, path));
                }
            }
        }

        self.push_scope();
        self.push_assert_refinement_frame();
        for stmt in &mut block.statements {
            self.check_stmt(stmt);
        }
        let ty = if let Some(expr) = &mut block.expr {
            self.check_expr_with_expected(expr, expected)
        } else {
            self.vacuum_type()
        };
        let mut state = self.snapshot_flow_state();
        state.invalidated.extend(assignment_targets_in_block(block));
        self.pop_assert_refinement_frame();
        self.pop_scope();
        self.range_flow.restore(&ranges_at_entry);

        for (def_id, original) in saved.into_iter().rev() {
            if let Some(original) = original {
                self.restore_binding(def_id, original);
            }
        }
        for (base, path) in field_saved.into_iter().rev() {
            self.field_facts.remove(&(base, path));
        }
        (ty, state)
    }

    /// Check a loop body after applying the loop-entry policy.
    ///
    /// Facts for bindings assigned anywhere in the body are dropped before the
    /// first iteration. Facts for untouched immutable bindings remain available
    /// because they are loop-invariant under this first-cut policy.
    pub fn check_loop_block(&mut self, block: &mut HirBlock) {
        let touched = assignment_targets_in_block(block);
        for def_id in touched {
            self.invalidate_flow_fact(def_id);
        }
        self.kill_ranges_for_loop_effects(block);
        // The loop can be left from the top, or from any `break`, so what the
        // body establishes does not hold after it: the continuation resumes
        // from the facts that survived the loop-entry kills.
        let at_entry = self.range_flow.snapshot();
        self.check_block(block, None);
        self.range_flow.restore(&at_entry);
    }

    /// Check a `dum` body under its condition's taken-edge facts (Option A,
    /// operator ruling 2026-10-15).
    ///
    /// The condition is re-tested before every iteration, so its then-facts
    /// hold at body entry each time. The narrowing runs from the test to the
    /// first re-point: `invalidate_assignment_target` (N3) restores the flow
    /// base at each store, so `cur ← cur.next` inside the body ends the
    /// narrowing for the rest of that iteration, and the next test re-arms
    /// it. Facts never escape the loop body.
    pub fn check_dum_block(&mut self, cond: &HirExpression, block: &mut HirBlock) {
        let incoming = self.snapshot_flow_state();
        self.remember_flow_bases(&incoming);
        // Loop-entry policy (N3): facts proven before the loop are dropped
        // for bindings the body touches — the body may re-point one, and the
        // next iteration's reads would see a stale fact.
        let touched = assignment_targets_in_block(block);
        for def_id in touched {
            self.invalidate_flow_fact(def_id);
        }
        self.kill_ranges_for_loop_effects(block);
        let at_entry = self.range_flow.snapshot();
        // The condition is re-tested before every iteration, so its
        // then-facts hold at body entry each time and are seeded fresh; a
        // re-point inside the body ends them for the rest of that iteration
        // (the store restores the flow base), and the next test re-arms them.
        let (then_facts, _) = self.condition_refinement(cond);
        self.check_block_with_refinement(block, None, then_facts);
        self.restore_flow_state(&incoming);
        // The loop-entry kills hold after the loop too: the body that ran
        // zero or more times may have written what `incoming` still proves
        //.
        self.range_flow.restore(&at_entry);
    }

    /// Snapshot the currently visible binding state for one control-flow edge.
    fn snapshot_flow_state(&self) -> FlowState {
        FlowState {
            bindings: self.binding_shadow.clone(),
            invalidated: FxHashSet::default(),
            ranges: self.range_flow.snapshot(),
        }
    }

    /// Restore a visible state without changing lexical scope ownership.
    fn restore_flow_state(&mut self, state: &FlowState) {
        self.binding_shadow.clone_from(&state.bindings);
        self.range_flow.restore(&state.ranges);
        for (&def_id, &info) in &state.bindings {
            if let Some(scope) = self
                .scopes
                .iter_mut()
                .rev()
                .find(|scope| scope.contains_key(&def_id))
            {
                scope.insert(def_id, info);
            }
        }
    }

    /// Remember the declared type behind a future persistent fact.
    fn remember_flow_bases(&mut self, state: &FlowState) {
        for (&def_id, &info) in &state.bindings {
            if self.flow_bases.contains_key(&def_id) {
                continue;
            }
            let saved = self.saved_assert_refinement(def_id);
            self.flow_bases.insert(def_id, saved.unwrap_or(info));
        }
    }

    /// Apply a joined state only where it differs from the incoming state.
    fn apply_flow_state(&mut self, incoming: &FlowState, joined: &FlowState) {
        self.range_flow.restore(&joined.ranges);
        for (&def_id, &info) in &joined.bindings {
            let Some(before) = incoming.bindings.get(&def_id) else {
                continue;
            };
            if !self.member_type_equals(before.ty, info.ty) {
                self.set_current_binding(def_id, info);
            }
        }
    }

    /// The ruled join: intersect selected members across normal edges. A
    /// singleton intersection is represented by that existing member type;
    /// when a multi-member intersection would require constructing a new
    /// type-level meet, the conservative incoming fact is retained.
    fn join_flow_states(&mut self, incoming: &FlowState, states: &[&FlowState]) -> FlowState {
        let Some(first) = states.first() else {
            return incoming.clone();
        };
        // Interval facts join by hull, independently of the type facts below.
        let edge_ranges: Vec<&RangeSnapshot> = states.iter().map(|state| &state.ranges).collect();
        let ranges = self.range_flow.join(&incoming.ranges, &edge_ranges);
        if states.len() == 1 {
            let mut joined = (*first).clone();
            for def_id in &first.invalidated {
                if let Some(base) = self.flow_bases.get(def_id).copied() {
                    joined.bindings.insert(*def_id, base);
                }
            }
            joined.ranges = ranges;
            return joined;
        }
        let mut joined = incoming.clone();
        joined.ranges = ranges;
        for (&def_id, &before) in &incoming.bindings {
            let Some(candidate) = first.bindings.get(&def_id).copied() else {
                continue;
            };
            if states
                .iter()
                .any(|state| state.invalidated.contains(&def_id))
            {
                if let Some(base) = self.flow_bases.get(&def_id).copied() {
                    joined.bindings.insert(def_id, base);
                } else {
                    joined.bindings.insert(def_id, before);
                }
                continue;
            }
            let changed_edges = states.iter().all(|state| {
                state
                    .bindings
                    .get(&def_id)
                    .is_some_and(|info| !self.member_type_equals(before.ty, info.ty))
            });
            if !changed_edges {
                continue;
            }
            let mut common = self.flow_members(candidate.ty);
            for state in &states[1..] {
                let Some(other) = state.bindings.get(&def_id) else {
                    common.clear();
                    break;
                };
                let members = self.flow_members(other.ty);
                common.retain(|member| {
                    members
                        .iter()
                        .copied()
                        .any(|other| self.member_type_equals(*member, other))
                });
            }
            match common.as_slice() {
                [member] => {
                    let mut refined = candidate;
                    refined.ty = *member;
                    joined.bindings.insert(def_id, refined);
                }
                _ if states[1..].iter().all(|state| {
                    state
                        .bindings
                        .get(&def_id)
                        .is_some_and(|other| self.member_type_equals(candidate.ty, other.ty))
                }) =>
                {
                    joined.bindings.insert(def_id, candidate);
                }
                _ => {}
            }
        }
        joined
    }

    /// Return the existing member set represented by a flow type. The join
    /// never creates an intersection type; a singleton result can select an
    /// existing member, while wider disagreements conservatively drop.
    fn flow_members(&mut self, ty: TypeId) -> Vec<TypeId> {
        let resolved = self.resolve_inferred_type(ty);
        self.union_members_of(resolved)
            .unwrap_or_else(|| vec![resolved])
    }

    /// Return an edge snapshot with one condition refinement applied.
    fn state_with_refinement(&mut self, refine: Facts) -> FlowState {
        let mut saved: Vec<(DefId, Option<BindingInfo>)> = Vec::new();
        let ranges_at_entry = self.range_flow.snapshot();
        for (key, value) in refine {
            self.assume_range_fact(&key, &value);
            // Field facts are not carried on the implicit fallthrough edge in
            // this need.
            let (FactKey::Binding(def_id), Some(ty)) = (key, value.ty) else {
                continue;
            };
            saved.push((def_id, self.override_binding(def_id, ty)));
        }
        let state = self.snapshot_flow_state();
        for (def_id, original) in saved.into_iter().rev() {
            if let Some(original) = original {
                self.restore_binding(def_id, original);
            }
        }
        self.range_flow.restore(&ranges_at_entry);
        state
    }

    /// Check the right conjunct of `et` under the left conjunct's then-facts.
    ///
    /// `et` short-circuits: the right conjunct is evaluated only when the left
    /// holds, so every then-fact the left conjunct establishes is in force
    /// while the right conjunct is typed. The facts are applied to the
    /// bindings, the right conjunct is checked, and every saved original is
    /// restored in reverse so the caller sees the un-narrowed state.
    pub(super) fn check_conjunct_rhs_narrowed(
        &mut self,
        lhs: &HirExpression,
        rhs: &mut HirExpression,
    ) -> TypeId {
        let (facts, _) = self.condition_refinement(lhs);
        let mut saved: Vec<(DefId, Option<BindingInfo>)> = Vec::new();
        let mut range_undo = Vec::new();
        for (key, value) in facts {
            if let Some(undo) = self.assume_range_fact(&key, &value) {
                range_undo.push(undo);
            }
            // Field facts do not join across the short-circuit edge in this
            // need.
            let (FactKey::Binding(def_id), Some(ty)) = (key, value.ty) else {
                continue;
            };
            saved.push((def_id, self.override_binding(def_id, ty)));
        }
        let rhs_ty = self.check_expr(rhs);
        for (def_id, original) in saved.into_iter().rev() {
            if let Some(original) = original {
                self.restore_binding(def_id, original);
            }
        }
        // The right conjunct may or may not have run, so a kill inside it
        // stays killed; only an interval it left untouched is taken back.
        for (def_id, undo) in range_undo.into_iter().rev() {
            self.range_flow.retract(def_id, undo);
        }
        rhs_ty
    }

    /// The saved declared type a currently open assert-refinement frame holds
    /// for `def_id`, if any frame registered one.
    fn saved_assert_refinement(&self, def_id: DefId) -> Option<BindingInfo> {
        self.assert_refinements
            .iter()
            .rev()
            .flat_map(|frame| frame.iter().rev())
            .find_map(|(saved_def, saved)| (*saved_def == def_id).then_some(*saved).flatten())
    }

    /// Restore the declared binding state and remove all active assert facts
    /// for a binding whose cell has been assigned.
    pub(crate) fn invalidate_flow_fact(&mut self, def_id: DefId) {
        // A store re-points the cell: its interval and every fact bounded by
        // its old live length are dead whether or not a type fact existed.
        self.range_flow.drop_subject(def_id);
        let Some(current) = self.lookup_binding(def_id) else {
            return;
        };
        let original = self
            .flow_bases
            .get(&def_id)
            .copied()
            .or_else(|| self.saved_assert_refinement(def_id))
            .unwrap_or(current);
        for scope in &mut self.scopes {
            if scope.contains_key(&def_id) {
                scope.insert(def_id, original);
            }
        }
        self.binding_shadow.insert(def_id, original);
        for frame in &mut self.assert_refinements {
            frame.retain(|(saved_def, _)| *saved_def != def_id);
        }
        // A store rooted at this binding re-points the whole place, so every
        // field fact rooted here is dropped too. Sibling-field facts go with
        // it: conservative over-invalidation, and sound.
        self.field_facts.retain(|(base, _), _| *base != def_id);
    }

    /// Re-install a flow fact saved immediately before an assignment's
    /// target invalidation: the value expression is evaluated before the
    /// store takes effect, so the pre-store fact still holds for it
    /// (`cur ← cur.next` reads the narrowed cursor).
    pub(crate) fn reinstate_flow_fact(&mut self, def_id: DefId, info: BindingInfo) {
        for scope in &mut self.scopes {
            if scope.contains_key(&def_id) {
                scope.insert(def_id, info);
            }
        }
        self.binding_shadow.insert(def_id, info);
    }

    /// Drop facts before checking an assignment, so its target is typed from
    /// the declared cell rather than from a stale pre-assignment refinement.
    pub(crate) fn invalidate_assignment_target(&mut self, target: &HirExpression) {
        // A bare-path store re-points the binding: its own fact and every
        // field fact rooted at it go. A field-chain store (`p.nomen ← v`)
        // drops every field fact rooted at the base (conservative across
        // sibling fields) but leaves the base's own fact: writing through a
        // narrowed `ref Node ∪ none cur` does not make `cur` none again.
        match field_path_key(target) {
            Some((base, path)) if path.is_empty() => self.invalidate_flow_fact(base),
            Some((base, _)) => {
                self.field_facts.retain(|(root, _), _| *root != base);
                self.kill_mutable_source_ranges();
            }
            // An indexed or dereferenced place: a store through a projection
            // may reach any mutable source through an alias.
            None => self.kill_mutable_source_ranges(),
        }
    }

    /// Drop any assignment facts produced by nested expression forms after the
    /// expression has been checked. Direct assignment checks invalidate before
    /// their target is typed; this post-pass covers assignments nested in a
    /// larger statement expression and inc/dec targets.
    pub(crate) fn invalidate_assignments_in_expr(&mut self, expr: &HirExpression) {
        let effects = assignment_targets_in_expr(expr);
        for def_id in effects.targets {
            self.invalidate_flow_fact(def_id);
        }
        for base in effects.field_roots {
            self.field_facts.retain(|(root, _), _| *root != base);
        }
        // Calls and projected stores were already killed where they were
        // checked (`kills_ranges` serves the loop-entry scan only).
    }

    /// Kill every interval fact about a mutable source (need, the
    /// version-1 closed rule). Called after any call, mutating intrinsic or
    /// closure call, and for a store through an alias or projection. A fact on
    /// a `let` binding, a captured loop endpoint or a fixed tensor extent
    /// survives: its subject cannot be written. A binding that is not visible
    /// any more counts as mutable, which only over-kills.
    pub(crate) fn kill_mutable_source_ranges(&mut self) {
        // A list bounding a fact may be the very object a `mut` name writes
        // (`let xs ← ys`, an unmarked parameter beside a `mut` one, a field or
        // element read), so every list source is killed, `let` or not. Only the
        // sources some fact names matter, so the type is read for those.
        let lists: Vec<DefId> = self
            .range_flow
            .live_length_sources()
            .into_iter()
            .filter(|source| self.is_list_source(*source))
            .collect();
        let shadow = &self.binding_shadow;
        self.range_flow.kill_mutable_sources(
            |def_id| shadow.get(&def_id).is_none_or(|info| info.mutable),
            |def_id| lists.contains(&def_id),
        );
    }

    /// Whether `def_id` is a list binding (a reference collection) rather than
    /// a text, whose value is never written in place.
    fn is_list_source(&self, def_id: DefId) -> bool {
        let Some(binding) = self.lookup_binding(def_id) else {
            return true;
        };
        let peeled = self
            .types
            .peel_access_marker(self.resolve_inferred_type(binding.ty))
            .1;
        !self.is_text_family(peeled)
    }

    /// Loop-entry policy for interval facts: a body that can write a mutable
    /// source (any call or projected store) may do so before the next
    /// iteration reads a fact established before the loop, so those facts do
    /// not enter the loop. Bindings the body rebinds directly are dropped by
    /// the existing per-binding policy.
    fn kill_ranges_for_loop_effects(&mut self, block: &HirBlock) {
        let read_only = |receiver: &HirExpression, name: Symbol, arg_count: usize| {
            self.is_read_only_intrinsic_call(receiver, name, arg_count)
        };
        let mut collector = AssignmentTargetCollector {
            read_only: Some(&read_only),
            ..Default::default()
        };
        collector.visit_block(block);
        let kills = collector.kills_ranges;
        if kills {
            self.kill_mutable_source_ranges();
        }
    }

    /// Apply the interval half of a fact on the branch being entered. Only a
    /// binding key carries one: `FactKey::Field` joins are not built, so a
    /// range value on a field path is ignored.
    fn assume_range_fact(
        &mut self,
        key: &FactKey,
        value: &FactValue,
    ) -> Option<(DefId, super::flow_range::RangeUndo)> {
        let (FactKey::Binding(def_id), Some(range)) = (key, &value.range) else {
            return None;
        };
        Some((*def_id, self.range_flow.assume(*def_id, range)))
    }

    /// Install a joined fact in the scope containing the current statement.
    fn set_current_binding(&mut self, def_id: DefId, info: BindingInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(def_id, info);
        }
        self.binding_shadow.insert(def_id, info);
    }

    /// Temporarily record a narrowed type for a visible binding.
    ///
    /// Returns the original [`BindingInfo`] so the caller can restore it
    /// exactly. The override is written into the current scope and the shadow
    /// map; the scope machinery keeps it visible through any nested block
    /// pushed during the branch check.
    fn override_binding(&mut self, def_id: DefId, ty: TypeId) -> Option<BindingInfo> {
        let original = self.lookup_binding(def_id)?;
        let refined = BindingInfo { ty, ..original };
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(def_id, refined);
        }
        self.binding_shadow.insert(def_id, refined);
        Some(original)
    }

    /// Restore a binding overridden by [`Self::override_binding`].
    fn restore_binding(&mut self, def_id: DefId, original: BindingInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(def_id, original);
        }
        self.binding_shadow.insert(def_id, original);
    }

    /// Open one assert-remainder frame for the block about to be checked
    /// (U1R).
    ///
    /// `adfirma r est M` narrows the rest of its block: assert failure aborts
    /// the program, so control never continues past the statement with the
    /// un-narrowed type. Overrides registered while the block is checked are
    /// restored in reverse here, so a narrowed type never escapes its block.
    pub fn push_assert_refinement_frame(&mut self) {
        self.assert_refinements.push(Vec::new());
    }

    /// Close the innermost assert-remainder frame, restoring every override
    /// it registered (U1R).
    pub fn pop_assert_refinement_frame(&mut self) {
        let Some(frame) = self.assert_refinements.pop() else {
            return;
        };
        for (def_id, saved) in frame.into_iter().rev() {
            if let Some(original) = saved {
                self.restore_binding(def_id, original);
            }
        }
    }

    /// Apply an assertion expression's taken-branch refinement for the rest
    /// of its block (U1R).
    ///
    /// Only `adfirma` conditions with a narrowing `then` side register an
    /// override (`adfirma r est M`, the null forms, and their conservative
    /// compounds); other assertion shapes are checked and forgotten.
    pub fn apply_assert_refinement(&mut self, expr: &HirExpression) {
        let HirExpressionKind::Adfirma(cond, _) = &expr.kind else {
            return;
        };
        let facts = self.condition_refinement(cond).0;
        if self.assert_refinements.is_empty() {
            return;
        }
        for (key, value) in facts {
            // Field facts are not carried through `adfirma` in this need, and
            // neither are interval facts: no producer exists yet, and the
            // assert frames restore binding types only.
            let (FactKey::Binding(def_id), Some(ty)) = (key, value.ty) else {
                continue;
            };
            let saved = self.override_binding(def_id, ty);
            if let Some(frame) = self.assert_refinements.last_mut() {
                frame.push((def_id, saved));
            }
        }
    }

    /// Whether a checked block has no normal continuation.
    fn block_diverges(&self, block: &HirBlock) -> bool {
        if let Some(expr) = &block.expr {
            return self.expression_diverges(expr);
        }
        block
            .statements
            .last()
            .is_some_and(|stmt| self.statement_diverges(stmt))
    }

    fn statement_diverges(&self, stmt: &HirStatement) -> bool {
        match &stmt.kind {
            // `perge` / `rumpe` leave the loop body, so the statements that
            // follow in the enclosing block are reached only through the
            // other edge. The loop-entry policy already drops facts for every
            // binding the body assigns, and a body-scope fact never survives
            // the loop, so ignoring the edge here cannot leak a stale fact.
            HirStatementKind::Redde(_)
            | HirStatementKind::ReddeFilum(_)
            | HirStatementKind::Rumpe
            | HirStatementKind::Perge => true,
            HirStatementKind::Expr(expr) => self.expression_diverges(expr),
            _ => false,
        }
    }

    fn expression_diverges(&self, expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::Throw(_) | HirExpressionKind::Panic(_) => true,
            HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => {
                self.block_diverges(block)
            }
            HirExpressionKind::Si {
                then_block,
                else_block: Some(else_block),
                ..
            } => self.block_diverges(then_block) && self.block_diverges(else_block),
            HirExpressionKind::Discerne { arms, .. } => {
                !arms.is_empty() && arms.iter().all(|arm| self.expression_diverges(&arm.body))
            }
            HirExpressionKind::Handled { body, catch } => {
                self.block_diverges(body) && self.block_diverges(&catch.body)
            }
            _ => false,
        }
    }
}

/// The field path of a null-probe operand, rooted at a binding.
///
/// A bare binding path keys the fact by that binding; a chain of field
/// accesses (`base.a.b`) keys it by the root binding plus the root-to-leaf
/// name path. Any other root yields no key.
pub(super) fn field_path_key(expr: &HirExpression) -> Option<(DefId, Vec<Symbol>)> {
    let mut names = Vec::new();
    let mut current = expr;
    loop {
        match &current.kind {
            HirExpressionKind::Field(object, name) => {
                names.push(*name);
                current = object;
            }
            HirExpressionKind::Path(def_id) => {
                names.reverse();
                return Some((*def_id, names));
            }
            _ => return None,
        }
    }
}

/// Collect bindings assigned by a loop body before its first iteration.
fn assignment_targets_in_block(block: &HirBlock) -> FxHashSet<DefId> {
    let mut collector = AssignmentTargetCollector::default();
    collector.visit_block(block);
    collector.targets
}

fn assignment_targets_in_expr(expr: &HirExpression) -> AssignmentTargetCollector<'_> {
    let mut collector = AssignmentTargetCollector::default();
    collector.visit_expr(expr);
    collector
}

/// Whether a method call (receiver, name, argument count) only reads.
type ReadOnlyCall<'a> = dyn Fn(&HirExpression, Symbol, usize) -> bool + 'a;

#[derive(Default)]
struct AssignmentTargetCollector<'a> {
    /// Bindings re-pointed by a bare-path store.
    targets: FxHashSet<DefId>,
    /// Bindings that root a field-chain store; only their field facts go.
    field_roots: FxHashSet<DefId>,
    /// A call, a store through a projection, or a copy/sink into a place was
    /// seen: any of them may write a mutable source, so interval facts about
    /// mutable sources are killed.
    kills_ranges: bool,
    /// Whether a method call is a registry intrinsic that only reads and so
    /// writes no list. Absent where only assignment targets are wanted.
    read_only: Option<&'a ReadOnlyCall<'a>>,
}

impl HirVisitor for AssignmentTargetCollector<'_> {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if let HirStatementKind::IncDec(inc_dec) = &stmt.kind {
            self.collect_target(&inc_dec.target);
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Assign(target, _)
            | HirExpressionKind::ConversioAssign { target, .. } => {
                self.collect_target(target);
            }
            HirExpressionKind::MethodCall(receiver, name, _, args, _) => {
                let read_only = self
                    .read_only
                    .is_some_and(|is_read_only| is_read_only(receiver, *name, args.len()));
                if !read_only {
                    self.kills_ranges = true;
                }
            }
            HirExpressionKind::Call(..)
            | HirExpressionKind::CopyInto { .. }
            | HirExpressionKind::SinkInto { .. } => self.kills_ranges = true,
            _ => {}
        }
        walk_expr(self, expr);
    }
}

impl AssignmentTargetCollector<'_> {
    fn collect_target(&mut self, target: &HirExpression) {
        // A field-chain store (`p.nomen ← v`, `p.a.b ← v`) has its place
        // rooted at the base binding: only the field facts rooted there go.
        match field_path_key(target) {
            Some((base, path)) if path.is_empty() => {
                self.targets.insert(base);
            }
            Some((base, _)) => {
                self.field_roots.insert(base);
                self.kills_ranges = true;
            }
            None => self.kills_ranges = true,
        }
    }
}
