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
//! - U1R generalizes the seam to nominal member tests: `r est M` narrows the
//!   taken branch to `M` and the fallthrough to the union minus `M` (`non est
//!   M` mirrors), on immutable bindings only (varia refinement is open row 2).
//!   `adfirma <cond>` narrows the rest of its block through an override frame
//!   restored at block exit — a failed assert aborts, so control never
//!   continues past it with the un-narrowed type.
//! - N3 drops all recorded facts when a binding is assigned. Loop bodies drop
//!   facts for bindings they touch before entry; untouched immutable facts may
//!   enter as loop-invariant knowledge.

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
}

/// One leaf condition's `(then_refinement, else_refinement)` pair (nn-u1 null
/// checks, U1R nominal `est M` tests): each side is an optional
/// `(DefId, narrowed TypeId)` for the visible binding.
type LeafRefinement = (Option<(DefId, TypeId)>, Option<(DefId, TypeId)>);

/// The facts that hold on one edge of a condition: every `(DefId, narrowed
/// TypeId)` that is true when the edge is taken.
type Facts = Vec<(DefId, TypeId)>;

/// `(then_facts, else_facts)` for a whole condition, including compound
/// `and` / `or` / `not` shapes.
type ConditionRefinement = (Facts, Facts);

pub(super) const ISSUE_TRAP_MEMBER_TYPE_MISMATCH: &str = "trap_member_type_mismatch";
pub(super) const ISSUE_TRAP_UNION_COLLAPSED: &str = "trap_union_collapsed";
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
    /// reified into a value union.
    ///
    /// Typing is all reuse — no new inference: the result is a fresh
    /// `Type::InferUnion` hole; every failable call's error type and `iace`
    /// payload inside the body accumulates into it through the existing
    /// U-02 arms (`ErrorSink::Trap` + the third `reject_failable_call`
    /// answer); the body's success type unifies in the same way; finalize
    /// solves through the existing normalization (WARN017 covers the
    /// degenerate single-member solve).
    ///
    /// Fail-closed guard: when the synthesized union collapses below two
    /// members *because success and error are the same type* (T ≡ E, after
    /// the existing dedup/alias-transparency law), the collapsed value
    /// cannot distinguish success from failure and the trap contract is
    /// unstateable — dedicated diagnostic. Partial overlap (shared members,
    /// still ≥2 members) stays legal: narrowing keeps it sound.
    pub fn check_trap(
        &mut self,
        body: &mut HirBlock,
        expected: Option<TypeId>,
        span: radix_lexer::Span,
    ) -> TypeId {
        // An explicit `∪` annotation hole (`const ∪ r ← capta { … }`) owns the
        // accumulation directly; anything else gets a fresh hole.
        let expected_var = expected
            .map(|ty| self.resolve_inferred_type(ty))
            .and_then(|resolved| match self.types.get(resolved) {
                Type::InferUnion(var) => Some(*var),
                _ => None,
            });
        let var = expected_var.unwrap_or_else(|| self.fresh_union_hole());

        let prev_error = self.current_error.replace(ErrorSink::Trap(var));
        let prev_flow = std::mem::replace(&mut self.trap_error_flow, false);
        let body_ty = self.check_block(body, None);
        self.current_error = prev_error;
        let error_flowed = std::mem::replace(&mut self.trap_error_flow, prev_flow);

        // The success member rides the same accumulation arm as the errors.
        self.accumulate_into_union_hole(var, body_ty, span, ISSUE_TRAP_MEMBER_TYPE_MISMATCH);

        self.closed_trap_holes.insert(var);

        let collapsed = if error_flowed {
            self.trap_collapsed_member(var)
        } else {
            None
        };
        if let Some(collapsed) = collapsed {
            let member = self.format_union_constituent_type(collapsed);
            self.invalid_operand_types_with_args(
                ISSUE_TRAP_UNION_COLLAPSED,
                span,
                &[("type", member)],
            );
            return self.error_type;
        }

        // `const _ r ← capta { ... }`: the monomorphic `_` hole binds to the
        // trap hole so the local's type is the solved union (the U-02 arm
        // would otherwise strand the `_` as an unbound pending member).
        if expected_var.is_none()
            && let Some(expected) = expected
        {
            let resolved = self.resolve_inferred_type(expected);
            let mono_var = match self.types.get(resolved) {
                Type::Infer(var) => Some(*var),
                _ => None,
            };
            if let Some(mono_var) = mono_var {
                let hole = self.union_hole_type_id(var);
                self.bind_infer_with_diagnostic(
                    mono_var,
                    hole,
                    span,
                    ISSUE_TRAP_MEMBER_TYPE_MISMATCH,
                );
            }
        }

        self.union_hole_type_id(var)
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

    /// The single surviving member of a trap hole after the existing
    /// canonicalization law (BO-S-4 sort/dedup over `union_member_sort_key`
    /// + `equals`), or `None` when two or more members remain.
    fn trap_collapsed_member(&mut self, var: InferVar) -> Option<TypeId> {
        let members: Vec<TypeId> = {
            let borrowed = self.union_members.borrow();
            borrowed.get(&var).cloned().unwrap_or_default()
        };
        let mut cleaned: Vec<TypeId> = members
            .into_iter()
            .filter(|member| *member != self.error_type)
            .collect();
        cleaned.sort_by_key(|id| (self.types.union_member_sort_key(*id).0, id.0));
        cleaned.dedup_by(|a, b| self.types.equals(*a, *b));
        match cleaned.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
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
    /// silently accept a non-`bivalens` condition.
    pub fn check_condition(&mut self, cond: &mut HirExpression) {
        let bivalens = self.bool_type();
        let cond_ty = self.check_expr_with_expected(cond, Some(bivalens));
        if self.resolve_inferred_type(cond_ty) == self.error_type {
            return;
        }
        if !self.is_bool(cond_ty) {
            self.report_mask_not_truth_value(cond_ty, cond.span);
        }
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
        let incoming = self.snapshot_flow_state();
        self.remember_flow_bases(&incoming);
        // A `tensor<bivalens, S>` condition is the mask of the `✓ ✗` select
        // (K4), not a branch condition: both branches are evaluated.
        if let Some(mask_ty) = self.check_condition_or_mask(cond) {
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
        let (else_ty, else_state, else_diverges) = match else_block {
            Some(block) => {
                let (ty, state) =
                    self.check_block_with_refinement(block, block_expected, else_refine);
                (ty, state, self.block_diverges(block))
            }
            None => {
                let state = self.state_with_refinement(else_refine);
                (self.vacuum_type(), state, false)
            }
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
    /// Returns `(then_facts, else_facts)`, each a list of
    /// `(DefId, narrowed TypeId)` for visible bindings.
    fn condition_refinement(&mut self, cond: &HirExpression) -> ConditionRefinement {
        match &cond.kind {
            // Both conjuncts hold when `A and B` is true, at any depth. Its
            // falsity forces neither conjunct, so the else edge learns nothing.
            HirExpressionKind::Binary(HirBinOp::And, lhs, rhs) => {
                let (mut then_facts, _) = self.condition_refinement(lhs);
                then_facts.extend(self.condition_refinement(rhs).0);
                (then_facts, Vec::new())
            }
            // Both disjuncts are false when `A or B` is false, so the else
            // edge (and the fallthrough of a diverging `si A aut B`) carries
            // both negations. Its truth forces neither disjunct.
            HirExpressionKind::Binary(HirBinOp::Or, lhs, rhs) => {
                let (_, mut else_facts) = self.condition_refinement(lhs);
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
                (
                    then_fact.into_iter().collect(),
                    else_fact.into_iter().collect(),
                )
            }
        }
    }

    /// Refinement for one non-compound condition (null probe, member test,
    /// or null equality).
    fn leaf_refinement(&mut self, cond: &HirExpression) -> LeafRefinement {
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
    /// The operand must be a binding path whose type peels a null member.
    fn nil_probe_refinement(&mut self, positive: bool, expr: &HirExpression) -> LeafRefinement {
        let HirExpressionKind::Path(def_id) = &expr.kind else {
            return (None, None);
        };
        let Some(binding_ty) = self.lookup_binding(*def_id).map(|binding| binding.ty) else {
            return (None, None);
        };
        let nihil = self.types.primitive(Primitive::Nihil);
        let Some(refined) = self.peel_member_type(binding_ty, nihil) else {
            return (None, None);
        };
        if positive {
            (None, Some((*def_id, refined)))
        } else {
            (Some((*def_id, refined)), None)
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
        let binding_ty = self
            .closed_trap_hole_union(binding_ty)
            .unwrap_or(binding_ty);
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
            (Some((*def_id, member)), remainder.map(|ty| (*def_id, ty)))
        } else {
            (remainder.map(|ty| (*def_id, ty)), Some((*def_id, member)))
        }
    }

    /// The member set of a trap result hole, as a union type, once the trap
    /// body has been fully checked.
    ///
    /// A `_` or `∪` binding of `trap { … }` keeps the open `InferUnion` hole
    /// until finalize, so `is` narrowing would otherwise see no members and
    /// leave the binding at the whole union. The hole's accumulated set is
    /// complete when the trap boundary closes, so the narrowing reads it
    /// directly. Returns `None` for any other type, and for a set that has
    /// fewer than two distinct members (the collapse diagnostic owns that).
    fn closed_trap_hole_union(&mut self, ty: TypeId) -> Option<TypeId> {
        let Type::InferUnion(var) = *self.types.get(ty) else {
            return None;
        };
        if !self.closed_trap_holes.contains(&var) {
            return None;
        }
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
        (distinct.len() >= 2).then(|| self.types.intern(Type::Union(distinct)))
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
        let saved: Vec<(DefId, Option<BindingInfo>)> = refine
            .into_iter()
            .map(|(def_id, ty)| (def_id, self.override_binding(def_id, ty)))
            .collect();

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

        for (def_id, original) in saved.into_iter().rev() {
            if let Some(original) = original {
                self.restore_binding(def_id, original);
            }
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
        self.check_block(block, None);
    }

    /// Snapshot the currently visible binding state for one control-flow edge.
    fn snapshot_flow_state(&self) -> FlowState {
        FlowState {
            bindings: self.binding_shadow.clone(),
            invalidated: FxHashSet::default(),
        }
    }

    /// Restore a visible state without changing lexical scope ownership.
    fn restore_flow_state(&mut self, state: &FlowState) {
        self.binding_shadow.clone_from(&state.bindings);
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
            let saved = self
                .assert_refinements
                .iter()
                .rev()
                .flat_map(|frame| frame.iter().rev())
                .find_map(|(saved_def, saved)| (*saved_def == def_id).then_some(*saved).flatten());
            self.flow_bases.insert(def_id, saved.unwrap_or(info));
        }
    }

    /// Apply a joined state only where it differs from the incoming state.
    fn apply_flow_state(&mut self, incoming: &FlowState, joined: &FlowState) {
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
        if states.len() == 1 {
            let mut joined = (*first).clone();
            for def_id in &first.invalidated {
                if let Some(base) = self.flow_bases.get(def_id).copied() {
                    joined.bindings.insert(*def_id, base);
                }
            }
            return joined;
        }
        let mut joined = incoming.clone();
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
            let Some(before_state) = incoming.bindings.get(&def_id) else {
                continue;
            };
            let changed_edges = states.iter().all(|state| {
                state
                    .bindings
                    .get(&def_id)
                    .is_some_and(|info| !self.member_type_equals(before_state.ty, info.ty))
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
        let saved: Vec<(DefId, Option<BindingInfo>)> = refine
            .into_iter()
            .map(|(def_id, ty)| (def_id, self.override_binding(def_id, ty)))
            .collect();
        let state = self.snapshot_flow_state();
        for (def_id, original) in saved.into_iter().rev() {
            if let Some(original) = original {
                self.restore_binding(def_id, original);
            }
        }
        state
    }

    /// Restore the declared binding state and remove all active assert facts
    /// for a binding whose cell has been assigned.
    pub(crate) fn invalidate_flow_fact(&mut self, def_id: DefId) {
        let Some(current) = self.lookup_binding(def_id) else {
            return;
        };
        let original = self
            .flow_bases
            .get(&def_id)
            .copied()
            .or_else(|| {
                self.assert_refinements
                    .iter()
                    .rev()
                    .flat_map(|frame| frame.iter().rev())
                    .find_map(|(saved_def, saved)| {
                        (*saved_def == def_id).then_some(*saved).flatten()
                    })
            })
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
    }

    /// Drop facts before checking an assignment, so its target is typed from
    /// the declared cell rather than from a stale pre-assignment refinement.
    pub(crate) fn invalidate_assignment_target(&mut self, target: &HirExpression) {
        if let HirExpressionKind::Path(def_id) = target.kind {
            self.invalidate_flow_fact(def_id);
        }
    }

    /// Drop any assignment facts produced by nested expression forms after the
    /// expression has been checked. Direct assignment checks invalidate before
    /// their target is typed; this post-pass covers assignments nested in a
    /// larger statement expression and inc/dec targets.
    pub(crate) fn invalidate_assignments_in_expr(&mut self, expr: &HirExpression) {
        for def_id in assignment_targets_in_expr(expr) {
            self.invalidate_flow_fact(def_id);
        }
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
        for (def_id, ty) in facts {
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
            HirStatementKind::Redde(_) | HirStatementKind::Rumpe | HirStatementKind::Perge => true,
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

/// Collect bindings assigned by a loop body before its first iteration.
fn assignment_targets_in_block(block: &HirBlock) -> FxHashSet<DefId> {
    let mut collector = AssignmentTargetCollector::default();
    collector.visit_block(block);
    collector.targets
}

fn assignment_targets_in_expr(expr: &HirExpression) -> FxHashSet<DefId> {
    let mut collector = AssignmentTargetCollector::default();
    collector.visit_expr(expr);
    collector.targets
}

#[derive(Default)]
struct AssignmentTargetCollector {
    targets: FxHashSet<DefId>,
}

impl HirVisitor for AssignmentTargetCollector {
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
            _ => {}
        }
        walk_expr(self, expr);
    }
}

impl AssignmentTargetCollector {
    fn collect_target(&mut self, target: &HirExpression) {
        if let HirExpressionKind::Path(def_id) = target.kind {
            self.targets.insert(def_id);
        }
    }
}
