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
//!   else branch (nn-u1). The override is restored immediately after the
//!   branch block — a narrowed type must never escape its branch scope.
//!   Compound conditions are conservative (nn-u2): `x non est nihil et …`
//!   narrows the taken branch only (the conjunction requires the left
//!   null-check conjunct), while `or` and non-`and` shapes never narrow.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

/// `(then_refinement, else_refinement)` pair for the null-check narrowing
/// (nn-u1): each side is an optional `(DefId, narrowed TypeId)` for the
/// visible binding; the untaken side is always `None`.
type NullConditionRefinement = (Option<(DefId, TypeId)>, Option<(DefId, TypeId)>);

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

        for arm in arms {
            self.push_scope();
            for (pattern, scrutinee_ty) in arm.patterns.iter().zip(scrutinee_tys.iter().copied()) {
                self.check_pattern(pattern, scrutinee_ty, arm.span);
            }
            if let Some(guard) = &mut arm.guard {
                self.check_condition(guard);
            }
            let body_ty = self.check_expr_with_expected(&mut arm.body, expected);
            result_ty = Some(match result_ty {
                None => body_ty,
                Some(existing) => self.common_type(existing, body_ty, arm.span),
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
            self.invalid_operand_types("condition_must_be_bivalens", cond.span);
        }
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
    /// conservative (nn-u2): `x non est nihil et …` narrows the taken branch
    /// only; `or` never narrows. The override is restored before the branch
    /// type is joined, so the narrowed type never escapes its block.
    pub fn check_if(
        &mut self,
        cond: &mut HirExpression,
        then_block: &mut HirBlock,
        then_catch: Option<&mut HirCape>,
        else_block: Option<&mut HirBlock>,
        expected: Option<TypeId>,
    ) -> TypeId {
        let then_ty = if let Some(catch) = then_catch {
            let err_ty = self.fresh_infer();
            let prev_error = self.current_error;
            self.current_error = Some(ErrorSink::Local(err_ty));
            self.check_condition(cond);
            let (then_refine, _) = self.null_condition_refinement(cond);
            let then_ty = self.check_block_with_refinement(then_block, expected, then_refine);
            self.current_error = prev_error;
            self.check_cape(catch, err_ty);
            then_ty
        } else {
            self.check_condition(cond);
            let (then_refine, _) = self.null_condition_refinement(cond);
            self.check_block_with_refinement(then_block, expected, then_refine)
        };
        let (_, else_refine) = self.null_condition_refinement(cond);
        let else_ty = match else_block {
            Some(block) => self.check_block_with_refinement(block, expected, else_refine),
            None => self.vacuum_type(),
        };

        // U-04: under a `∪`-hole expectation the branch join is an
        // accumulation, not a monomorphic `common_type` join. Each branch
        // already flowed its type into the hole var through `check_block`
        // (with `expected`); the final join keeps the hole open. The issue
        // tag mirrors `common_type`'s; a hole-vs-hole join short-circuits in
        // `unify_with_policy` before any diagnostic can fire.
        if let Some(expected) = expected {
            if self
                .infer_union_var_of(self.resolve_inferred_type(expected))
                .is_some()
            {
                return self.unify_join(then_ty, else_ty, cond.span, "common_type_mismatch");
            }
        }

        self.common_type(then_ty, else_ty, cond.span)
    }

    /// Detect a direct null check on a visible binding and compute the
    /// narrowing for each branch of the `si` (nn-u1/nn-u2).
    ///
    /// `x non est nihil` narrows the binding to its non-null core in the taken
    /// (then) branch; `x est nihil` narrows identically in the fallthrough
    /// (else) branch (Q3). The untaken side of each form keeps `T ∪ null`, so
    /// only one side of the returned pair is ever set.
    ///
    /// Compound conditions are conservative (nn-u2). `x non est nihil et …`
    /// narrows the taken branch only: the whole conjunction being true
    /// requires the left null-check conjunct to hold, but its falsity does
    /// not force the conjunct's falsity, so the else branch gets no
    /// refinement (the mirror `x est nihil et …` else-narrowing would be
    /// unsound — `x` can be null while `x est nihil et y` is false). `or`
    /// never narrows: the disjunction's truth does not force the left
    /// operand's truth, so neither branch is refined. Non-`and` compound
    /// shapes (and null checks that are not the leftmost `and` conjunct)
    /// stay un-narrowed.
    ///
    /// Returns `(then_refinement, else_refinement)`, each an optional
    /// `(DefId, narrowed TypeId)` for the visible binding.
    fn null_condition_refinement(&mut self, cond: &HirExpression) -> NullConditionRefinement {
        let HirExpressionKind::Binary(op, lhs, rhs) = &cond.kind else {
            return (None, None);
        };
        // Compound conjunction: keep only the taken-branch refinement of the
        // left conjunct; the fallthrough of `and` is never narrowed.
        if matches!(op, HirBinOp::And) {
            let (then_refine, _) = self.null_condition_refinement(lhs);
            return (then_refine, None);
        }
        // `x non est nihil` narrows the taken branch; `x est nihil` narrows
        // the fallthrough.
        let taken_branch_narrows = match op {
            HirBinOp::IsNot => true,
            HirBinOp::Is => false,
            _ => return (None, None),
        };
        // The null probe must be the literal `nihil` on the right.
        if !matches!(&rhs.kind, HirExpressionKind::Literal(HirLiteral::Nil)) {
            return (None, None);
        }
        let HirExpressionKind::Path(def_id) = &lhs.kind else {
            return (None, None);
        };
        let Some(binding_ty) = self.lookup_binding(*def_id).map(|binding| binding.ty) else {
            return (None, None);
        };
        let Some(refined) = self.peel_nullable_type(binding_ty) else {
            return (None, None);
        };
        if taken_branch_narrows {
            (Some((*def_id, refined)), None)
        } else {
            (None, Some((*def_id, refined)))
        }
    }

    /// Peel one nullable layer from a type to its non-null core, when the type
    /// is representable as a null-check narrowing (nn-u1).
    ///
    /// `Option(T)` narrows to `T`; a union containing `nihil` narrows to the
    /// union of its remaining members (collapsed to the single member when
    /// only one remains). Types with no nullable layer — and the degenerate
    /// all-`nihil` union, which has nothing left to narrow to — return `None`
    /// so callers leave the binding unchanged.
    fn peel_nullable_type(&mut self, ty: TypeId) -> Option<TypeId> {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Option(inner) => Some(*inner),
            Type::Union(members) => {
                let nihil = self.types.primitive(Primitive::Nihil);
                let mut remaining: Vec<TypeId> = members
                    .iter()
                    .copied()
                    .filter(|member| !self.types.equals(*member, nihil))
                    .collect();
                match remaining.len() {
                    0 => None,
                    1 => Some(remaining[0]),
                    _ => {
                        remaining.sort_unstable_by_key(|member| member.0);
                        Some(self.types.intern(Type::Union(remaining)))
                    }
                }
            }
            _ => None,
        }
    }

    /// Check a branch block under an optional flow refinement.
    ///
    /// The refinement is a temporary binding override that lives exactly for
    /// the duration of the block check and is restored immediately after —
    /// a narrowed type must never escape its branch scope. `BindingInfo` is
    /// `Copy`, so the exact original entry is put back (mutable and
    /// scalar-text flags included).
    fn check_block_with_refinement(
        &mut self,
        block: &mut HirBlock,
        expected: Option<TypeId>,
        refine: Option<(DefId, TypeId)>,
    ) -> TypeId {
        let (refined_def, saved) = match refine {
            Some((def_id, ty)) => (Some(def_id), self.override_binding(def_id, ty)),
            None => (None, None),
        };
        let ty = self.check_block(block, expected);
        if let (Some(def_id), Some(original)) = (refined_def, saved) {
            self.restore_binding(def_id, original);
        }
        ty
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
}
