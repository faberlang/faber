//! Inference-variable unification and substitution resolution.
//!
//! This file is the checker-local inference engine. `Type::Infer` nodes live in
//! the shared [`TypeTable`], but their meaning is completed by the
//! `TypeChecker` substitution map here. Any code that needs the current meaning
//! of a type must resolve through `TypeChecker::resolve_inferred_type`, not by inspecting
//! the table entry alone.
//!
//! ERROR RECOVERY
//! ==============
//! Unification reports hard type errors and returns the table-local error type
//! when compatibility fails. The checker then keeps walking the HIR so one bad
//! expression does not hide unrelated diagnostics. Unresolved inference
//! variables are diagnosed later by finalization as missing annotations.
//!
//! INVARIANTS
//! ==========
//! - `InferVar` IDs are scoped to one `TypeChecker` run.
//! - Substitutions may point at aliases or other inferred types; resolving must
//!   chase both substitution and alias chains.
//! - The occurs check prevents recursive inference substitutions from creating
//!   an infinite semantic type.

use super::index_infer::ISSUE_INCOMPATIBLE_TENSOR_INDEX;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

/// Whether unification is a symmetric inference join or a directional assignment.
#[derive(Copy, Clone)]
enum UnifyPolicy {
    /// Accept widening in either direction (`is_assignable(a, b) || is_assignable(b, a)`).
    Join,
    /// Enforce source-to-destination flow (`is_assignable(left, right)` only).
    Assign,
    /// A type-argument position: the two sides must be the same type, with
    /// only inference holes free to bind. A container of one element type is
    /// never a container of another (ruling 21 row 25, ruling 51 A7), because
    /// both names would share one storage and the store policy belongs to the
    /// destination cell, not to the alias that happens to write through it.
    Invariant,
}

impl TypeChecker<'_> {
    /// Return whether `var` occurs inside `ty` after current substitutions.
    ///
    /// This is the guard that keeps `T = lista<T>`-style inference cycles out of
    /// the type table. It walks structural type shapes because compound entries
    /// are allocated in the shared arena rather than expanded inline.
    ///
    /// BO-S-6: memoize by `(var, resolved TypeId)`. Cache is cleared on each
    /// successful bind so substitution growth cannot stale a negative result.
    pub fn occurs_in(&mut self, var: InferVar, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        if let Some(&cached) = self.occurs_cache.get(&(var, resolved)) {
            return cached;
        }
        let result = self.occurs_in_uncached(var, resolved);
        self.occurs_cache.insert((var, resolved), result);
        result
    }

    fn occurs_in_uncached(&mut self, var: InferVar, resolved: TypeId) -> bool {
        if let Some(found) = self.infer_var_of(resolved) {
            return found == var;
        }
        if let Some(found) = self.infer_union_var_of(resolved) {
            return found == var;
        }
        if let Type::MarkerHole { var: found, .. } = self.types.get(resolved) {
            return *found == var;
        }
        // Clone children out of the table first — recursive occurs needs `&mut self`.
        let children: Vec<TypeId> = match self.types.get(resolved) {
            Type::Array(inner)
            | Type::Queue(inner)
            | Type::Stack(inner)
            | Type::BoundedArray { element: inner, .. }
            | Type::RangedArray { element: inner, .. }
            | Type::BoundedQueue { element: inner, .. }
            | Type::BoundedStack { element: inner, .. }
            | Type::Option(inner)
            | Type::Ref(_, inner)
            | Type::Promissum(inner)
            | Type::Cursor(inner)
            | Type::Atomic(inner)
            | Type::Intervallum(inner, _)
            | Type::Set(inner)
            | Type::Alias(_, inner)
            | Type::Tensor(inner, _)
            | Type::Vector(inner, _)
            | Type::Matrix(inner, _)
            | Type::Sparsa(inner, _) => vec![*inner],
            Type::Map(key, value)
            | Type::Ledger(key, value)
            | Type::PromissumFailable(key, value)
            | Type::AsyncCursor(key, value) => {
                vec![*key, *value]
            }
            Type::Func(sig) => {
                let mut kids: Vec<TypeId> = sig.params.iter().map(|p| p.ty).collect();
                kids.push(sig.ret);
                if let Some(err) = sig.err {
                    kids.push(err);
                }
                for param in &sig.type_params {
                    if let TypeParamConstraint::OneOf(types) = &param.constraint {
                        kids.extend(types.iter().copied());
                    }
                }
                kids
            }
            Type::Applied(base, args) => {
                let mut kids = vec![*base];
                kids.extend(args.iter().filter_map(|arg| match arg {
                    radix_types::AppliedArg::Type(id) => Some(*id),
                    radix_types::AppliedArg::Size(_) => None,
                }));
                kids
            }
            Type::Union(types) | Type::Intersection(types) => types.clone(),
            Type::Tuple(elems) => elems.clone(),
            Type::Record(fields) => fields.values().copied().collect(),
            _ => return false,
        };
        children.into_iter().any(|child| self.occurs_in(var, child))
    }

    /// Bind an inference variable to a resolved type or merge with its old binding.
    ///
    /// Existing substitutions are joined rather than overwritten so multiple
    /// constraints on the same inferred type converge through the normal
    /// compatibility rules.
    ///
    /// U-02: union-hole variables never reach this path. `unify_with_policy`
    /// routes `Type::InferUnion` through the accumulation arms before the
    /// monomorphic bind path, so `var` here is always a monomorphic `Type::Infer`.
    pub(crate) fn bind_infer_with_diagnostic(
        &mut self,
        var: InferVar,
        ty: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> TypeId {
        let resolved = self.resolve_inferred_type(ty);
        let existing = self.substitutions.borrow().get(&var).copied();
        if let Some(existing) = existing {
            return self.unify_with_policy(existing, resolved, span, issue, UnifyPolicy::Join);
        }

        if self.occurs_in(var, resolved) {
            self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
            return self.error_type;
        }

        self.occurs_cache.clear();
        self.substitutions.borrow_mut().insert(var, resolved);
        resolved
    }

    /// Bind `numerus<_>` / twins to a same-family witness (exact marker, P6).
    fn bind_marker_hole(
        &mut self,
        var: InferVar,
        family: MarkerFamily,
        witness: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> TypeId {
        let Some(witness) = self.materialize_marker_witness(family, witness) else {
            self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
            return self.error_type;
        };
        let before = self.errors.len();
        self.bind_infer_exact_issue(var, witness, span, issue);
        if self.errors.len() > before {
            return self.error_type;
        }
        self.substitutions
            .borrow()
            .get(&var)
            .copied()
            .unwrap_or(witness)
    }

    fn materialize_marker_witness(
        &mut self,
        family: MarkerFamily,
        witness: TypeId,
    ) -> Option<TypeId> {
        let witness = self.resolve_inferred_type(witness);
        match (family, self.types.get(witness).clone()) {
            (MarkerFamily::Modulus, Type::ModularWord(width)) => {
                Some(self.types.intern(Type::ModularWord(width)))
            }
            (MarkerFamily::Saturatus, Type::SaturatingWord(width)) => {
                Some(self.types.intern(Type::SaturatingWord(width)))
            }
            (
                MarkerFamily::Saturatus,
                Type::PolicyFloat(radix_types::FloatPolicy::Saturating, width),
            ) => Some(self.types.intern(Type::PolicyFloat(
                radix_types::FloatPolicy::Saturating,
                width,
            ))),
            (MarkerFamily::Instans, Type::SizedInstans(precision)) => {
                Some(self.types.sized_instans(precision))
            }
            (MarkerFamily::Instans, Type::Primitive(Primitive::Instans)) => {
                Some(self.types.primitive(Primitive::Instans))
            }
            (expected, Type::MarkerHole { family: other, .. }) if expected == other => {
                Some(witness)
            }
            _ => None,
        }
    }

    /// Bind an inference variable for equality without assignability widening.
    ///
    /// WHY: [`Self::bind_infer`] merges via [`Self::unify_join`], which accepts
    /// assignable-but-distinct pairs (`ascii`/`textus`, `numerus`/`fractus`, …).
    /// Equality must keep deliberate type annotations intact.
    pub fn bind_infer_exact_issue(
        &mut self,
        var: InferVar,
        ty: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) {
        let resolved = self.resolve_inferred_type(ty);
        let existing = self.substitutions.borrow().get(&var).copied();
        if let Some(existing) = existing {
            let existing_resolved = self.resolve_inferred_type(existing);
            if !self.types.equals(existing_resolved, resolved) {
                self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
            }
            return;
        }

        if self.occurs_in(var, resolved) {
            self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
            return;
        }

        self.occurs_cache.clear();
        self.substitutions.borrow_mut().insert(var, resolved);
    }

    /// Merge two table-local types symmetrically (inference join).
    ///
    /// This is the **symmetric** variant used when joining two partial types
    /// during inference (collection element types, map key/value types,
    /// closure parameter types, function return types, etc.). It accepts
    /// both directions of the widening rules so that inference succeeds
    /// regardless of argument order.
    ///
    /// For **directional assignment** (source must be assignable to
    /// destination), use [`Self::unify_assign_issue`] instead.
    pub fn unify_join(
        &mut self,
        a: TypeId,
        b: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> TypeId {
        self.unify_with_policy(a, b, span, issue, UnifyPolicy::Join)
    }

    pub fn unify_assign_issue(
        &mut self,
        a: TypeId,
        b: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> TypeId {
        // Contract subsumption enters at one insertion point, Assign only,
        // before the `types.is_assignable` delegation. Implemented as a
        // pre-check at this public funnel entry rather than an arm inside
        // `unify_with_policy`'s fallthrough: every funnel site (argument,
        // initializer, return, `⇥` channel) calls here, while the structural
        // arms recurse through `unify_with_policy` directly — so conformance
        // never leaks into type-argument positions and parameterized types
        // stay invariant (`lista<Concrete>` is not `lista<Contract>`).
        if let Some(rejected) = self.sermo_assign_precheck(a, b, span) {
            return rejected;
        }
        if let Some(destination) = self.conformance_destination(a, b) {
            return destination;
        }
        let issue = if self.is_integer_into_float(a, b) {
            INTEGER_TO_FLOAT_REQUIRES_CONVERSIO
        } else {
            issue
        };
        self.unify_with_policy(a, b, span, issue, UnifyPolicy::Assign)
    }

    /// d11-6 ruling 16: a typed integer stored into a float slot. The issue's
    /// help points at the explicit `↦` conversion.
    pub(super) fn is_integer_into_float(&self, a: TypeId, b: TypeId) -> bool {
        let from = self.peel_nominal_aliases(a);
        let to = self.peel_nominal_aliases(b);
        matches!(
            (self.types.get(from), self.types.get(to)),
            (
                Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _),
                Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _)
            )
        ) && !matches!(self.types.get(from), Type::SizedNumeric(_, w) if w.is_decimal())
    }

    /// The contract-accepted destination type for `a → b`, if any.
    ///
    /// Covers bare nominal subsumption (Concrete→Interface through `implet`),
    /// the `(_, Intersection)` slot decomposition (every constituent holds),
    /// and union destinations member-by-member so a concrete value flows into
    /// a `⇥` interface-channel union. Joins and inference binding never
    /// consult this. There is no genus-to-genus subsumption: class
    /// inheritance was removed (DECISIONS D8.1).
    fn conformance_destination(&self, a: TypeId, b: TypeId) -> Option<TypeId> {
        let from = self.peel_nominal_aliases(a);
        let to = self.peel_nominal_aliases(b);
        if self.accepts_via_conformance(from, to) {
            return Some(to);
        }
        None
    }

    /// Peel alias wrappers (and inference substitutions) down to the payload
    /// type without touching structural wrappers.
    pub(super) fn peel_nominal_aliases(&self, ty: TypeId) -> TypeId {
        let mut resolved = self.resolve_inferred_type(ty);
        loop {
            match self.types.get(resolved) {
                Type::Alias(_, inner) => resolved = *inner,
                _ => return resolved,
            }
        }
    }

    /// Whether `from` flows into `to` through declared `implet` contracts.
    fn accepts_via_conformance(&self, from: TypeId, to: TypeId) -> bool {
        match self.types.get(to) {
            // Slot law: a union destination accepts when any member accepts.
            // `types.is_assignable` already covers non-contract members; this
            // arm adds contract acceptance per member (the `⇥` channel case).
            Type::Union(members) => members.iter().any(|member| {
                self.accepts_via_conformance(from, *member)
                    || self.types.is_assignable(from, *member)
            }),
            // `(_, Intersection(to))` under Assign: every constituent holds.
            Type::Intersection(members) => members.iter().all(|member| {
                self.accepts_via_conformance(from, *member)
                    || self.types.is_assignable(from, *member)
            }),
            Type::Struct(base) => self
                .struct_def_from_type(from)
                .is_some_and(|from_def| from_def == *base),
            Type::Interface(iface) => self
                .struct_def_from_type(from)
                .is_some_and(|from_def| self.struct_implements(from_def, *iface)),
            _ => false,
        }
    }

    /// Whether `from` declares `iface` in its `implet` clause.
    fn struct_implements(&self, from: DefId, iface: DefId) -> bool {
        self.implemented_interfaces
            .get(&from)
            .is_some_and(|interfaces| interfaces.contains(&iface))
    }

    /// Unify assignment on carrier payloads, ignoring access-marker wrappers.
    ///
    /// `ref`/`mut`/`own` are type facts, not a different payload shape. Own-bind
    /// legality (MU-3) is enforced in typecheck plus bind-site moves in borrow.
    pub fn unify_carrier_assign(
        &mut self,
        a: TypeId,
        b: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> TypeId {
        let a = self
            .types
            .peel_access_marker(self.resolve_inferred_type(a))
            .1;
        let b = self
            .types
            .peel_access_marker(self.resolve_inferred_type(b))
            .1;
        self.unify_assign_issue(a, b, span, issue)
    }

    /// Concrete bounded ASCII capacities widen only from a smaller buffer to
    /// a larger one. Unknown/index-parameter capacities fail closed here;
    /// equality is handled by the `TypeTable` index relation before this
    /// helper is needed.
    fn is_bounded_ascii_capacity_assignable(
        &mut self,
        from: crate::IndexId,
        to: crate::IndexId,
    ) -> bool {
        let from = self.resolve_index(from);
        let to = self.resolve_index(to);
        match (self.types.get_index(from), self.types.get_index(to)) {
            (IndexExpr::Literal(from), IndexExpr::Literal(to)) => from <= to,
            _ => false,
        }
    }

    fn bounded_ascii_capacity_is_one(&mut self, capacity: crate::IndexId) -> bool {
        let capacity = self.resolve_index(capacity);
        matches!(self.types.get_index(capacity), IndexExpr::Literal(1))
    }

    fn unify_with_policy(
        &mut self,
        a: TypeId,
        b: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
        policy: UnifyPolicy,
    ) -> TypeId {
        let left = self.normalize_applied_alias(self.resolve_inferred_type(a));
        let right = self.normalize_applied_alias(self.resolve_inferred_type(b));
        if left == right {
            return left;
        }

        // U-02: union-hole accumulation arms run before the monomorphic bind
        // path. A `∪` hole (`Type::InferUnion`) never binds through
        // `substitutions`; direct flows into the slot accumulate members and
        // the hole itself is returned so later flows keep accumulating.
        if let Some(var) = self.infer_union_var_of(left) {
            return self.accumulate_into_union_hole(var, right, span, issue);
        }
        if let Some(var) = self.infer_union_var_of(right) {
            return self.accumulate_into_union_hole(var, left, span, issue);
        }

        if let Some(var) = self.infer_var_of(left) {
            return self.bind_infer_with_diagnostic(var, right, span, issue);
        }
        if let Some(var) = self.infer_var_of(right) {
            return self.bind_infer_with_diagnostic(var, left, span, issue);
        }

        // WHY: `Type::Param` is not special-cased here. Call sites rewrite params
        // to fresh infers in `generic.rs` before assignment-shaped checks run.

        let left_ty = self.types.get(left).clone();
        let right_ty = self.types.get(right).clone();

        if let Type::MarkerHole { family, var } = &left_ty {
            return self.bind_marker_hole(*var, *family, right, span, issue);
        }
        if let Type::MarkerHole { family, var } = &right_ty {
            return self.bind_marker_hole(*var, *family, left, span, issue);
        }

        match (left_ty, right_ty) {
            (Type::Primitive(Primitive::Numerus), Type::Primitive(Primitive::Fractus))
            | (Type::Primitive(Primitive::Fractus), Type::Primitive(Primitive::Numerus))
                if matches!(policy, UnifyPolicy::Join) =>
            {
                return self.fractus_type();
            }
            (Type::Primitive(a), Type::Primitive(b)) if a == b => return left,
            (Type::Array(a), Type::Array(b)) => {
                let inner = self.unify_with_policy(a, b, span, issue, UnifyPolicy::Invariant);
                return self.types.array(inner);
            }
            (Type::Queue(a), Type::Queue(b)) => {
                let inner = self.unify_with_policy(a, b, span, issue, UnifyPolicy::Invariant);
                return self.types.queue(inner);
            }
            (Type::Stack(a), Type::Stack(b)) => {
                let inner = self.unify_with_policy(a, b, span, issue, UnifyPolicy::Invariant);
                return self.types.stack(inner);
            }
            (Type::Map(ka, va), Type::Map(kb, vb)) => {
                let key = self.unify_with_policy(ka, kb, span, issue, UnifyPolicy::Invariant);
                let value = self.unify_with_policy(va, vb, span, issue, UnifyPolicy::Invariant);
                return self.types.map(key, value);
            }
            (Type::Ledger(ka, va), Type::Ledger(kb, vb)) => {
                let key = self.unify_with_policy(ka, kb, span, issue, UnifyPolicy::Invariant);
                let value = self.unify_with_policy(va, vb, span, issue, UnifyPolicy::Invariant);
                return self.types.ledger(key, value);
            }
            (Type::Set(a), Type::Set(b)) => {
                let inner = self.unify_with_policy(a, b, span, issue, UnifyPolicy::Invariant);
                return self.types.set(inner);
            }
            (Type::Promissum(a), Type::Promissum(b)) => {
                let inner = self.unify_with_policy(a, b, span, issue, UnifyPolicy::Invariant);
                return self.types.promissum(inner);
            }
            (Type::PromissumFailable(asuccess, aerr), Type::PromissumFailable(bsuccess, berr)) => {
                let success =
                    self.unify_with_policy(asuccess, bsuccess, span, issue, UnifyPolicy::Invariant);
                let alternate =
                    self.unify_with_policy(aerr, berr, span, issue, UnifyPolicy::Invariant);
                return self.types.promissum_failable(success, alternate);
            }
            (Type::Cursor(a), Type::Cursor(b)) => {
                let inner = self.unify_with_policy(a, b, span, issue, UnifyPolicy::Invariant);
                return self.types.cursor(inner);
            }
            (Type::AsyncCursor(aitem, aerr), Type::AsyncCursor(bitem, berr)) => {
                let item =
                    self.unify_with_policy(aitem, bitem, span, issue, UnifyPolicy::Invariant);
                let alternate =
                    self.unify_with_policy(aerr, berr, span, issue, UnifyPolicy::Invariant);
                return self.types.async_cursor(item, alternate);
            }
            (Type::Option(a), Type::Option(b)) => {
                let inner = self.unify_with_policy(a, b, span, issue, policy);
                return self.types.option(inner);
            }
            (Type::Tuple(from), Type::Tuple(to)) => {
                if from.len() != to.len() {
                    self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
                    return self.error_type;
                }
                let elements = from
                    .into_iter()
                    .zip(to)
                    .map(|(from, to)| {
                        self.unify_with_policy(from, to, span, issue, UnifyPolicy::Invariant)
                    })
                    .collect();
                return self.types.tuple(elements);
            }
            (Type::Intersection(from), Type::Intersection(to)) => {
                return self.unify_intersection_members(from, to, span, issue, policy);
            }
            // `T ∩ Contract` after call-site substitution is an intersection
            // whose `T` position is a fresh inference hole. Bind the hole to
            // the source and check the remaining nominal members through the
            // ordinary assignment path, exactly as `lista<T>` binds its
            // element. Without an inference hole the conformance pre-check
            // already covers the intersection, so the guard keeps this arm
            // out of plain structural joins.
            (_, Type::Intersection(members))
                if members.iter().any(|member| {
                    self.infer_var_of(self.resolve_inferred_type(*member))
                        .is_some()
                }) =>
            {
                return self.unify_into_destination_intersection(left, members, span, issue);
            }
            (Type::Record(from), Type::Record(to)) => {
                if from.len() != to.len() {
                    self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
                    return self.error_type;
                }
                let mut fields = Vec::with_capacity(from.len());
                for (name, from_ty) in from {
                    let Some(to_ty) = to.get(&name).copied() else {
                        self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
                        return self.error_type;
                    };
                    self.current_shape_fields.push(name);
                    let unified = self.unify_with_policy(from_ty, to_ty, span, issue, policy);
                    self.current_shape_fields.pop();
                    fields.push((name, unified));
                }
                return self.types.record(fields);
            }
            (Type::Ref(ma, a), Type::Ref(mb, b)) if ma == mb => {
                let inner = self.unify_with_policy(a, b, span, issue, UnifyPolicy::Invariant);
                return self.types.reference(ma, inner);
            }
            (Type::Tensor(ea, sa), Type::Tensor(eb, sb)) => {
                let elem = self.unify_with_policy(ea, eb, span, issue, UnifyPolicy::Invariant);
                let shape = match self.unify_shape_at_boundary(sa, sb, span) {
                    Some(Ok(shape)) => shape,
                    Some(Err(())) => return self.error_type,
                    None => {
                        match super::tensor_index::unify_tensor_shape_indices(self, sa, sb, span) {
                            Ok(shape) => shape,
                            Err(()) => return self.error_type,
                        }
                    }
                };
                return self.types.tensor_with_shape(elem, shape);
            }
            (Type::Applied(base_a, args_a), Type::Applied(base_b, args_b))
                if self.types.equals(base_a, base_b) && args_a.len() == args_b.len() =>
            {
                let Some(args) =
                    self.unify_applied_args(args_a, args_b, span, issue, UnifyPolicy::Invariant)
                else {
                    return self.error_type;
                };
                return self.types.intern(Type::Applied(base_a, args));
            }
            // F-010: structural unify for Vector, Matrix, Sparsa, Atomic, Intervallum.
            (Type::Vector(ea, wa), Type::Vector(eb, wb)) => {
                let elem = self.unify_with_policy(ea, eb, span, issue, UnifyPolicy::Invariant);
                let Ok(width) = super::tensor_index::unify_tensor_shape_indices(self, wa, wb, span)
                else {
                    return self.error_type;
                };
                return self.types.vector_with_width(elem, width);
            }
            (Type::Matrix(ea, sa), Type::Matrix(eb, sb)) => {
                let elem = self.unify_with_policy(ea, eb, span, issue, UnifyPolicy::Invariant);
                let Ok(shape) = super::tensor_index::unify_tensor_shape_indices(self, sa, sb, span)
                else {
                    return self.error_type;
                };
                return self.types.matrix_with_shape(elem, shape);
            }
            (Type::Sparsa(ea, sa), Type::Sparsa(eb, sb)) => {
                let elem = self.unify_with_policy(ea, eb, span, issue, UnifyPolicy::Invariant);
                let Ok(shape) = super::tensor_index::unify_tensor_shape_indices(self, sa, sb, span)
                else {
                    return self.error_type;
                };
                return self.types.sparsa_with_shape(elem, shape);
            }
            // D17: the one-byte bounded carrier produced by a char literal or
            // scalar index may flow into bare unbounded ascii. The reverse
            // direction remains explicit-only.
            (Type::BoundedAscii { capacity }, Type::Primitive(Primitive::Ascii))
                if matches!(policy, UnifyPolicy::Assign)
                    && self.bounded_ascii_capacity_is_one(capacity) =>
            {
                return right;
            }
            (Type::BoundedAscii { capacity: ca }, Type::BoundedAscii { capacity: cb }) => {
                // Capacity widening is directional assignment only. Joins and
                // equality-shaped inference remain invariant in `N`.
                if matches!(policy, UnifyPolicy::Assign)
                    && self.is_bounded_ascii_capacity_assignable(ca, cb)
                {
                    return right;
                }
                let Ok(capacity) =
                    super::tensor_index::unify_tensor_shape_indices(self, ca, cb, span)
                else {
                    return self.error_type;
                };
                return self.types.bounded_ascii(capacity);
            }
            (Type::BoundedTextus { capacity: ca }, Type::BoundedTextus { capacity: cb }) => {
                let Ok(capacity) =
                    super::tensor_index::unify_tensor_shape_indices(self, ca, cb, span)
                else {
                    return self.error_type;
                };
                return self.types.bounded_textus(capacity);
            }
            (Type::BoundedOcteti { capacity: ca }, Type::BoundedOcteti { capacity: cb }) => {
                let Ok(capacity) =
                    super::tensor_index::unify_tensor_shape_indices(self, ca, cb, span)
                else {
                    return self.error_type;
                };
                return self.types.bounded_octeti(capacity);
            }
            (
                Type::BoundedArray {
                    element: ea,
                    capacity: ca,
                },
                Type::BoundedArray {
                    element: eb,
                    capacity: cb,
                },
            ) => {
                let element = self.unify_with_policy(ea, eb, span, issue, UnifyPolicy::Invariant);
                let Ok(capacity) =
                    super::tensor_index::unify_tensor_shape_indices(self, ca, cb, span)
                else {
                    return self.error_type;
                };
                return self.types.bounded_array(element, capacity);
            }
            (
                Type::BoundedQueue {
                    element: ea,
                    capacity: ca,
                },
                Type::BoundedQueue {
                    element: eb,
                    capacity: cb,
                },
            ) => {
                let element = self.unify_with_policy(ea, eb, span, issue, UnifyPolicy::Invariant);
                let Ok(capacity) =
                    super::tensor_index::unify_tensor_shape_indices(self, ca, cb, span)
                else {
                    return self.error_type;
                };
                return self.types.bounded_queue(element, capacity);
            }
            (
                Type::BoundedStack {
                    element: ea,
                    capacity: ca,
                },
                Type::BoundedStack {
                    element: eb,
                    capacity: cb,
                },
            ) => {
                let element = self.unify_with_policy(ea, eb, span, issue, UnifyPolicy::Invariant);
                let Ok(capacity) =
                    super::tensor_index::unify_tensor_shape_indices(self, ca, cb, span)
                else {
                    return self.error_type;
                };
                return self.types.bounded_stack(element, capacity);
            }
            (Type::Atomic(inner_a), Type::Atomic(inner_b)) => {
                let inner =
                    self.unify_with_policy(inner_a, inner_b, span, issue, UnifyPolicy::Invariant);
                return self.types.atomic(inner);
            }
            (Type::Intervallum(inner_a, a_step), Type::Intervallum(inner_b, b_step)) => {
                let inner = self.unify_with_policy(inner_a, inner_b, span, issue, policy);
                // Steps are part of the type identity (`TypeTable::is_assignable`),
                // so an assignment across steps is a type error, never a silent
                // conversion to step 1.
                if !self.types.index_equals(a_step, b_step) {
                    self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
                    return self.error_type;
                }
                return self.types.intervallum_with_step(inner, b_step);
            }
            (Type::Func(sig_a), Type::Func(sig_b)) => {
                return self.unify_function_signatures(&sig_a, &sig_b, span, issue);
            }
            _ => match policy {
                UnifyPolicy::Join => {
                    if self.types.is_assignable(left, right) {
                        return right;
                    }
                    if self.types.is_assignable(right, left) {
                        return left;
                    }
                }
                UnifyPolicy::Assign => {
                    if self.types.is_assignable(left, right) {
                        return right;
                    }
                }
                UnifyPolicy::Invariant => {
                    if self.types.equals(left, right) {
                        return right;
                    }
                }
            },
        }

        self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
        self.error_type
    }

    /// Unify two canonical intersection member lists by structural pairing.
    ///
    /// `TypeTable::intern` sorts members by `TypeId` ordinal, which follows
    /// arena allocation order, so list position says nothing about which
    /// source member a destination member corresponds to. Every destination
    /// member that is not an inference hole is first paired with the source
    /// member of the same `TypeId`. Only the members left over, which is where
    /// the call-site holes (`T ∩ Contract` → `Infer ∩ Contract`) bind, are
    /// paired in their remaining list order.
    fn unify_intersection_members(
        &mut self,
        from: Vec<TypeId>,
        to: Vec<TypeId>,
        span: radix_lexer::Span,
        issue: &'static str,
        policy: UnifyPolicy,
    ) -> TypeId {
        if from.len() != to.len() {
            self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
            return self.error_type;
        }
        let mut from_pool = from;
        let mut open_to = Vec::with_capacity(to.len());
        let mut pairs = Vec::with_capacity(from_pool.len());
        for to_member in to {
            let resolved = self.resolve_inferred_type(to_member);
            if self.infer_var_of(resolved).is_none()
                && let Some(index) = from_pool
                    .iter()
                    .position(|member| self.resolve_inferred_type(*member) == resolved)
            {
                pairs.push((from_pool.remove(index), to_member));
                continue;
            }
            open_to.push(to_member);
        }
        pairs.extend(from_pool.into_iter().zip(open_to));

        let mut members = Vec::with_capacity(pairs.len());
        for (from_member, to_member) in pairs {
            let unified = self.unify_with_policy(from_member, to_member, span, issue, policy);
            if unified == self.error_type {
                return self.error_type;
            }
            members.push(unified);
        }
        self.types.intern(Type::Intersection(members))
    }

    /// Bind the inference holes of a destination intersection from `source`,
    /// and check the nominal members as ordinary assignment destinations.
    fn unify_into_destination_intersection(
        &mut self,
        source: TypeId,
        members: Vec<TypeId>,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> TypeId {
        let mut unified = Vec::with_capacity(members.len());
        for member in members {
            let resolved = self.resolve_inferred_type(member);
            if let Some(var) = self.infer_var_of(resolved) {
                unified.push(self.bind_infer_with_diagnostic(var, source, span, issue));
                continue;
            }
            let checked = self.unify_assign_issue(source, resolved, span, issue);
            if checked == self.error_type {
                return self.error_type;
            }
            unified.push(checked);
        }
        self.types.intern(Type::Intersection(unified))
    }

    /// Unify paired arguments of two same-base `Type::Applied` entries.
    ///
    /// The caller has already checked base identity and equal argument
    /// counts. Size slots keep boundary provenance through
    /// [`Self::unify_size_at_boundary`]. Returns `None` after emitting
    /// `issue` when the paired argument kinds differ.
    fn unify_applied_args(
        &mut self,
        args_a: Vec<radix_types::AppliedArg>,
        args_b: Vec<radix_types::AppliedArg>,
        span: radix_lexer::Span,
        issue: &'static str,
        policy: UnifyPolicy,
    ) -> Option<Vec<radix_types::AppliedArg>> {
        let mut args = Vec::with_capacity(args_a.len());
        for (arg_a, arg_b) in args_a.into_iter().zip(args_b) {
            match (arg_a, arg_b) {
                (radix_types::AppliedArg::Type(a), radix_types::AppliedArg::Type(b)) => {
                    args.push(radix_types::AppliedArg::Type(
                        self.unify_with_policy(a, b, span, issue, policy),
                    ));
                }
                (
                    radix_types::AppliedArg::Size(actual),
                    radix_types::AppliedArg::Size(expected),
                ) => {
                    args.push(radix_types::AppliedArg::Size(
                        self.unify_size_at_boundary(actual, expected, span)
                            .unwrap_or_else(|| {
                                super::tensor_index::unify_tensor_shape_indices(
                                    self, actual, expected, span,
                                )
                                .unwrap_or(actual)
                            }),
                    ));
                }
                _ => {
                    self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
                    return None;
                }
            }
        }
        Some(args)
    }

    /// Compare a nominal size argument at a call/return boundary. Unlike a
    /// tensor axis, this witness is already a scalar index term, so keep its
    /// actual provenance instead of fabricating an axis read from the record.
    fn unify_size_at_boundary(
        &mut self,
        actual: crate::IndexId,
        expected: crate::IndexId,
        span: radix_lexer::Span,
    ) -> Option<crate::IndexId> {
        let boundary = if self.current_shape_argument.is_some() {
            ShapeRelationBoundary::Call
        } else if self.current_shape_return_expr.is_some() {
            ShapeRelationBoundary::FunctionReturn
        } else {
            return None;
        };
        let actual = self.resolve_index(actual);
        let expected = self.runtime_substitute_shape_params(expected);
        let mut unifier = self.index_infer.unifier(self.types);
        if let Ok(result) = unifier.unify(actual, expected) {
            return Some(result);
        }
        let actual_static = radix_types::evaluate_index_dimension(actual, self.types)
            .ok()
            .flatten();
        let expected_static = radix_types::evaluate_index_dimension(expected, self.types)
            .ok()
            .flatten();
        if let (Some(left), Some(right)) = (actual_static, expected_static) {
            if left != right {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_INCOMPATIBLE_TENSOR_INDEX,
                    span,
                );
                return Some(actual);
            }
            return Some(self.types.intern_index(IndexExpr::Literal(left)));
        }
        self.record_shape_relation(actual, expected, span, boundary);
        Some(actual)
    }

    fn unify_function_signatures(
        &mut self,
        sig_a: &FuncSig,
        sig_b: &FuncSig,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> TypeId {
        if sig_a.params.len() != sig_b.params.len() {
            self.error_issue(SemanticErrorKind::ArityMismatch, issue, span);
            return self.error_type;
        }
        if sig_a.is_async != sig_b.is_async || sig_a.is_generator != sig_b.is_generator {
            self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
            return self.error_type;
        }
        let mut params = Vec::with_capacity(sig_a.params.len());
        for (param_a, param_b) in sig_a.params.iter().zip(sig_b.params.iter()) {
            if param_a.mode != param_b.mode
                || param_a.optional != param_b.optional
                || param_a.sponte != param_b.sponte
            {
                self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
                return self.error_type;
            }
            // Function types are invariant (ruling 21 row 25, ruling 51 A7):
            // a parameter or result of another type is another function type.
            let param_ty =
                self.unify_with_policy(param_b.ty, param_a.ty, span, issue, UnifyPolicy::Invariant);
            if self.resolve_inferred_type(param_ty) == self.error_type {
                return self.error_type;
            }
            params.push(ParamType {
                ty: param_ty,
                mode: param_a.mode,
                optional: param_a.optional,
                sponte: param_a.sponte,
            });
        }
        let ret = self.unify_with_policy(sig_a.ret, sig_b.ret, span, issue, UnifyPolicy::Invariant);
        let err = match (sig_a.err, sig_b.err) {
            (Some(a_err), Some(b_err)) => {
                Some(self.unify_with_policy(a_err, b_err, span, issue, UnifyPolicy::Invariant))
            }
            (None, None) => None,
            _ => {
                self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
                return self.error_type;
            }
        };
        self.types.function(FuncSig {
            type_params: sig_a.type_params.clone(),
            params,
            ret,
            err,
            is_async: sig_a.is_async,
            is_generator: sig_a.is_generator,
        })
    }

    /// Return whether a type still names an inference variable after resolution.
    ///
    /// Deliberately monomorphic-only: `∪` holes ([`Type::InferUnion`]) are
    /// inference variables too, but call sites that use `is_infer` to route
    /// an unresolved slot into the monomorphic bind/join path must not see
    /// them (U-02 accumulation arms in `unify_with_policy` handle those).
    pub fn is_infer(&self, ty: TypeId) -> bool {
        self.infer_var_of(ty).is_some()
    }

    /// Extract the inference variable token from a table entry if present.
    pub fn infer_var_of(&self, ty: TypeId) -> Option<InferVar> {
        match self.types.get(ty) {
            Type::Infer(var) => Some(*var),
            _ => None,
        }
    }

    /// Extract the union-hole variable token if the table entry is a `∪` hole.
    ///
    /// Distinct from [`Self::infer_var_of`]: union holes accumulate member
    /// types and are never bound through `substitutions`, so they must not
    /// take the monomorphic bind path in `unify_with_policy` or the equality
    /// checks.
    pub fn infer_union_var_of(&self, ty: TypeId) -> Option<InferVar> {
        match self.types.get(ty) {
            Type::InferUnion(var) => Some(*var),
            _ => None,
        }
    }

    /// Resolve checker-local substitutions and semantic aliases to a usable type.
    ///
    /// `TypeId` itself is only an arena index. For inferred or aliased types, the
    /// current semantic meaning is the fixed point reached by following the
    /// checker substitution map and `Type::Alias` entries.
    ///
    /// Union holes ([`Type::InferUnion`]) are **not** collapsed here: they are
    /// never present in `substitutions` (U-02 accumulates members instead of
    /// binding), and resolving them to their current member set would hide the
    /// open hole from later `unify_with_policy` accumulation arms. Finalization
    /// owns the solve.
    ///
    /// BO-S-2: path-compress intermediate `InferVar` substitutions so repeated
    /// reads are amortized O(α(n)). Alias chains live in the shared table and
    /// are followed but not rewritten.
    pub fn resolve_inferred_type(&self, ty: TypeId) -> TypeId {
        let mut current = ty;
        let mut path: Vec<InferVar> = Vec::new();
        loop {
            if let Some(infer) = self.infer_var_of(current)
                && let Some(subst) = self.substitutions.borrow().get(&infer).copied()
            {
                path.push(infer);
                current = subst;
                continue;
            }
            match self.types.get(current) {
                Type::Alias(_, resolved) => current = *resolved,
                Type::MarkerHole { var, .. } => {
                    if let Some(subst) = self.substitutions.borrow().get(var).copied() {
                        path.push(*var);
                        current = subst;
                        continue;
                    }
                    break;
                }
                _ => break,
            }
        }
        let representative = current;
        if !path.is_empty() {
            let mut subs = self.substitutions.borrow_mut();
            for var in path {
                if subs.get(&var).copied() != Some(representative) {
                    subs.insert(var, representative);
                }
            }
        }
        representative
    }

    /// Expand an applied generic type alias (`X<int>`) into its substituted
    /// body so unification sees the resolved shape.
    ///
    /// The resolve pass lowers `type X<T> = T` into `Type::Alias(def, body)`;
    /// a use `X<int>` becomes `Type::Applied(Type::Alias(def, body), [int])`.
    /// The alias's own type params are recorded in `alias_type_params` during
    /// item collection; here the args are substituted for the param names in
    /// the body (mirroring generic call-site substitution). An imported alias
    /// application whose base is the imported body — a register carrier whose
    /// width/shape expressions are placeholders (free declaration params, or
    /// the index holes a generic signature substitution already wrote) —
    /// expands by filling the carrier's shape slots with the applied size
    /// args in order; see [`Self::rebuild_carrier_with_size_args`] for the
    /// arity/type-kind guard. Other applied types (structs, enums,
    /// interfaces) are returned unchanged — generic nominal application is
    /// invariant and must not be peeled here.
    fn normalize_applied_alias(&mut self, ty: TypeId) -> TypeId {
        let Type::Applied(base, args) = self.types.get(ty).clone() else {
            return ty;
        };
        let base_resolved = self.resolve_inferred_type(base);
        // Imported alias applications carry the alias body (a register
        // carrier with free index params) as the base — the interface route
        // stores no `Type::Alias` wrapper — so they expand by binding the
        // applied size args onto the base's free params (S0-G2).
        if matches!(
            self.types.get(base_resolved),
            Type::Vector(_, _) | Type::Matrix(_, _) | Type::Tensor(_, _) | Type::Sparsa(_, _)
        ) {
            return self
                .rebuild_carrier_with_size_args(base_resolved, &args)
                .unwrap_or(ty);
        }
        let Type::Alias(def, body) = self.types.get(base).clone() else {
            return ty;
        };
        let Some(param_names) = self.alias_type_params.get(&def).cloned() else {
            return ty;
        };
        if param_names.len() != args.len() {
            return ty;
        }
        let mut vars = FxHashMap::default();
        let mut shape_vars = FxHashMap::default();
        for (param, arg) in param_names.iter().zip(args.iter()) {
            match arg {
                radix_types::AppliedArg::Type(id) => {
                    vars.insert(*param, *id);
                }
                radix_types::AppliedArg::Size(id) => {
                    shape_vars.insert(*param, *id);
                }
            }
        }
        let substituted = self.substitute_type_params(
            body,
            &mut vars,
            (!shape_vars.is_empty()).then_some(&mut shape_vars),
        );
        let substituted = self.resolve_inferred_type(substituted);
        // Peel chained alias applications (`type Y<T> = X<T>`).
        match self.types.get(substituted).clone() {
            Type::Applied(base, _) => {
                match self.types.get(self.resolve_inferred_type(base)).clone() {
                    Type::Alias(_, _) => self.normalize_applied_alias(substituted),
                    _ => substituted,
                }
            }
            _ => substituted,
        }
    }

    /// Value-level carrier of `ty` for operator, method, conversion, and
    /// assignment dispatch (S0-G2 alias value transparency).
    ///
    /// A transparent alias is value-transparent: applied alias uses expand to
    /// their substituted builtin carrier so glyph operands, intrinsic method
    /// receivers, `↦` shape arms, and builtin↔alias assignment see the
    /// register shape the alias declares. Local applications expand through
    /// the declaration param names recorded at collection; imported
    /// applications and generic-signature applications (whose base is the
    /// alias body with placeholder widths) expand by shape-slot filling —
    /// see [`Self::normalize_applied_alias`] for that guard.
    ///
    /// Applied *nominal* genera (`Type::Applied(Type::Struct.., ..)` and
    /// friends) never expand here: their application is invariant, and
    /// transparency is for aliases, not nominals.
    pub(crate) fn value_carrier_type(&mut self, ty: TypeId) -> TypeId {
        let resolved = self.resolve_inferred_type(ty);
        self.normalize_applied_alias(resolved)
    }

    /// Rebuild a register carrier with the applied size args in its shape
    /// slots (S0-G2 imported/signature alias expansion).
    ///
    /// The base's own width/shape expressions are placeholders at this point —
    /// free declaration params on an imported alias body, or the fresh index
    /// holes a generic signature substitution already wrote in — so the
    /// applied arguments are the authoritative slot values: a vector base
    /// takes one size arg, a matrix base two, a tensor/sparsa base one per
    /// declared dimension. Returns `None` (stays unexpanded) when any
    /// argument is a type or the arity does not fill the base's slots.
    fn rebuild_carrier_with_size_args(
        &mut self,
        base: TypeId,
        args: &[radix_types::AppliedArg],
    ) -> Option<TypeId> {
        let size_args: Vec<crate::IndexId> = args
            .iter()
            .map(|arg| match arg {
                radix_types::AppliedArg::Size(index) => Some(*index),
                radix_types::AppliedArg::Type(_) => None,
            })
            .collect::<Option<Vec<_>>>()?;
        match self.types.get(base).clone() {
            Type::Vector(element, _) if size_args.len() == 1 => {
                Some(self.types.vector_with_width(element, size_args[0]))
            }
            Type::Sparsa(element, _) if size_args.len() == 1 => {
                let shape = self
                    .types
                    .intern_index(radix_types::IndexExpr::Tuple(size_args));
                Some(self.types.sparsa_with_shape(element, shape))
            }
            Type::Matrix(element, _) if size_args.len() == 2 => {
                let shape = self
                    .types
                    .intern_index(radix_types::IndexExpr::Tuple(size_args));
                Some(self.types.matrix_with_shape(element, shape))
            }
            Type::Tensor(element, _) if !size_args.is_empty() => {
                let shape = self
                    .types
                    .intern_index(radix_types::IndexExpr::Tuple(size_args));
                Some(self.types.tensor_with_shape(element, shape))
            }
            _ => None,
        }
    }

    /// Allocate a fresh inference variable in the shared type table.
    ///
    /// The returned `TypeId` can be stored on HIR nodes immediately, but it is
    /// only final when `substitutions` later resolves its `InferVar`.
    pub fn fresh_infer(&mut self) -> TypeId {
        let var = InferVar(self.next_infer);
        self.next_infer += 1;
        let id = self.types.intern(Type::Infer(var));
        self.infer_ids.insert(var, id);
        id
    }

    /// Canonical `TypeId` of the `Type::InferUnion(var)` hole.
    ///
    /// Interning is value-based, so this is the same table entry HIR lowering
    /// produced for the source `∪` annotation.
    pub(crate) fn union_hole_type_id(&mut self, var: InferVar) -> TypeId {
        self.types.intern(Type::InferUnion(var))
    }

    /// Allocate a fresh `Type::InferUnion` hole for checker-synthesized union
    /// slots (UT: the `capta` trap result).
    ///
    /// Same allocation space as [`Self::fresh_infer`] (checker-side counter,
    /// disjoint from lowering's def-id-space holes), so the trap hole never
    /// collides with a source-authored `∪` annotation hole.
    pub(crate) fn fresh_union_hole(&mut self) -> InferVar {
        let var = InferVar(self.next_infer);
        self.next_infer += 1;
        var
    }

    /// Sink type for an active `capta` trap hole (UT).
    ///
    /// Marks that an alternate-exit payload is flowing toward the trap (the
    /// T ≡ E guard input) and returns the hole itself: flows into it route
    /// through the U-02 accumulation arms, never a new unify insertion point.
    pub(crate) fn trap_sink_type(&mut self, var: InferVar) -> TypeId {
        self.trap_error_flow = true;
        self.union_hole_type_id(var)
    }

    /// Accumulate one member type into a union hole, deduplicated via
    /// structural `TypeTable::equals` so repeated flows do not inflate the set.
    fn accumulate_union_member(&mut self, var: InferVar, ty: TypeId) {
        let mut members = self.union_members.borrow_mut();
        let entry = members.entry(var).or_default();
        if !entry
            .iter()
            .any(|existing| self.types.equals(*existing, ty))
        {
            entry.push(ty);
        }
    }

    /// Occurs-guarded member push: accumulating a type that contains the hole
    /// itself would form a recursive semantic type when finalize builds the
    /// `Union`, so the flow is rejected with a type mismatch instead.
    ///
    /// Returns `true` when the member was stored.
    fn push_union_member_guarded(
        &mut self,
        var: InferVar,
        member: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> bool {
        if member == self.error_type {
            return false;
        }
        if self.occurs_in(var, member) {
            self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
            return false;
        }
        self.accumulate_union_member(var, member);
        true
    }

    /// Contribute a type-pattern member to an open inference hole.
    ///
    /// Type-patterns are a producer of constraints rather than a use-site
    /// assignment. When the expected type is a monomorphic source hole, route
    /// it through a fresh `InferUnion` hole and bind the source variable to
    /// that hole. Existing union accumulation and finalization then remain the
    /// only solver and normalization paths. A concrete expected type returns
    /// `false` so the caller keeps its ordinary pattern-membership check.
    pub(crate) fn constrain_type_pattern(
        &mut self,
        expected: TypeId,
        member: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> bool {
        let expected = self.resolve_inferred_type(expected);
        let union_var = if let Some(var) = self.infer_union_var_of(expected) {
            var
        } else if let Some(source_var) = self.infer_var_of(expected) {
            let union_var = self.fresh_union_hole();
            let union_hole = self.union_hole_type_id(union_var);
            let before = self.errors.len();
            self.bind_infer_with_diagnostic(source_var, union_hole, span, issue);
            if self.errors.len() != before {
                return true;
            }
            union_var
        } else {
            return false;
        };

        self.accumulate_into_union_hole(union_var, member, span, issue);
        true
    }

    /// Merge a resolved incoming type into a `∪` hole and return the hole.
    ///
    /// U-02 accumulation semantics (per binding / per type node):
    /// - a concrete type flowing into the slot contributes one member;
    /// - an explicit `Union` contributes each of its members (flatten);
    /// - another `∪` hole contributes its accumulated member set (copy);
    /// - a monomorphic `_` hole contributes its current resolution when bound,
    ///   or is kept as a pending member so finalize resolves it later;
    /// - the occurs check rejects recursive members.
    ///
    /// The hole itself stays open (never bound through `substitutions`), so
    /// later direct flows keep accumulating into the same slot.
    pub(crate) fn accumulate_into_union_hole(
        &mut self,
        var: InferVar,
        incoming: TypeId,
        span: radix_lexer::Span,
        issue: &'static str,
    ) -> TypeId {
        let incoming = self.resolve_inferred_type(incoming);
        if incoming == self.error_type {
            return self.error_type;
        }
        if let Some(other_var) = self.infer_union_var_of(incoming) {
            if other_var == var {
                // Same hole — `unify_with_policy` already short-circuits `left == right`.
                return self.union_hole_type_id(var);
            }
            // Union-with-union: copy the other hole's accumulated members in.
            let borrowed = self.union_members.borrow();
            let other_members: Vec<TypeId> = borrowed.get(&other_var).cloned().unwrap_or_default();
            drop(borrowed);
            for member in other_members {
                if !self.push_union_member_guarded(var, member, span, issue) {
                    return self.error_type;
                }
            }
            return self.union_hole_type_id(var);
        }
        match self.types.get(incoming).clone() {
            Type::Union(members) => {
                for member in members {
                    if !self.push_union_member_guarded(var, member, span, issue) {
                        return self.error_type;
                    }
                }
            }
            Type::Infer(mono_var) => {
                // Cross-shape `_` → `∪`: contribute the monomorphic resolution
                // when known; otherwise keep the pending hole as a member.
                let subst = self.substitutions.borrow().get(&mono_var).copied();
                match subst {
                    Some(ty) => {
                        if !self.push_union_member_guarded(var, ty, span, issue) {
                            return self.error_type;
                        }
                    }
                    None => {
                        if !self.push_union_member_guarded(var, incoming, span, issue) {
                            return self.error_type;
                        }
                    }
                }
            }
            _ => {
                if !self.push_union_member_guarded(var, incoming, span, issue) {
                    return self.error_type;
                }
            }
        }
        self.union_hole_type_id(var)
    }
}

const INTEGER_TO_FLOAT_REQUIRES_CONVERSIO: &str = "integer_to_float_requires_conversio";
