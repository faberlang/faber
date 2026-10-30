//! Producers of integer interval facts (need, slice 3).
//!
//! The engine in `flow_range.rs` holds, joins and kills interval facts; this
//! module decides what the checker may put in it. The one consumer so far is
//! `flow_range_access.rs` (a proven list/text `at`/`slice` needs no handler);
//! nothing here removes a runtime check.
//!
//! WHERE FACTS COME FROM
//! =====================
//! - Integer guards `≺ ≤ ≻ ≥ ≡ ≢` between an integer binding and an integer
//!   expression. The else edge is the negation in integer total order. A float
//!   or any non-integer operand produces nothing. A guard alone never proves a
//!   lower bound for the subject: `i ≺ n` has an upper bound only, and a lower
//!   bound needs its own origin (a range start, an unsigned or ranged cell, a
//!   `⤒`), because the other operand's own bounds are all a guard can pass on.
//! - Range loops (`itera ab a‥b`) and range products: the binder lies between
//!   the endpoints captured at loop entry, in the direction the loop takes.
//!   The direction is a run-time comparison of the endpoints, so it is used
//!   only when it is proven; otherwise the two directions are joined and a side
//!   that cannot be compared is dropped (`0‥n` with a signed `n` of unknown
//!   sign proves nothing).
//! - List coordinates (`itera ex xs apud i`): `0 ≤ i < len(xs)`, for a source
//!   that is a named binding. Tensor and reduction/scan coordinates: `0 ≤ iₖ <`
//!   the resolved extent of axis k, via `axis_bound`; an unresolved or
//!   mismatched extent leaves the upper bound off.
//! - Immutable `let` bindings of an integer: the initializer's interval.
//!
//! VALUE RANGES OF EXPRESSIONS (`range_of_expr`)
//! ============================================
//! An expression's interval combines the absolute interval of the existing
//! `value_range` (declared types, ranged cells, constants) with symbolic
//! knowledge: a binding's flow fact, an immutable binding's own value, the live
//! length of a list/text, a size parameter's extent, `⤒`/`⤓`, `% e` for a
//! divisor proven positive, and `e ± c` for a literal `c`. Each side holds one
//! endpoint: a lower endpoint prefers an absolute floor (consumers need
//! `0 ≤ i`), an upper one prefers a relation (consumers need `i < len`).
//!
//! `FactKey::Field` joins are not built, so no field-path range fact is made.

use radix_hir::{HirBinOp, HirCallArg, HirExpression, HirExpressionKind, HirRangeKind};
use radix_intrinsics::{IntrinsicReceiver, IntrinsicReturnKind};

use super::control::{FactKey, LeafRefinement};
use super::flow_range::{AxisIdentity, BoundKey, Endpoint, FactValue, IntRange, axis_bound};

use super::{DefId, IndexExpr, IndexId, Primitive, Symbol, Type, TypeChecker, TypeId};

/// An absolute bound out at the 64-bit representation limit is that limit, not
/// a fact (a bare `numerus` is unbounded for this purpose).
fn finite(value: i128) -> Option<i128> {
    const LIMIT: i128 = 1 << 62;
    (-LIMIT..=LIMIT).contains(&value).then_some(value)
}

fn const_of(endpoint: &Endpoint) -> Option<i128> {
    match endpoint.key {
        BoundKey::Const(value) => Some(value),
        _ => None,
    }
}

/// The lower endpoint to keep from several valid ones: the strongest absolute
/// floor if any, else the first relation.
fn pick_lower(candidates: impl IntoIterator<Item = Endpoint>) -> Option<Endpoint> {
    let candidates: Vec<Endpoint> = candidates.into_iter().collect();
    candidates
        .iter()
        .filter_map(|endpoint| const_of(endpoint).map(|value| (value, endpoint)))
        .max_by_key(|(value, _)| *value)
        .map(|(_, endpoint)| endpoint.clone())
        .or_else(|| candidates.into_iter().next())
}

/// The upper endpoint to keep from several valid ones: the first relation if
/// any, else the strongest absolute ceiling.
fn pick_upper(candidates: impl IntoIterator<Item = Endpoint>) -> Option<Endpoint> {
    let candidates: Vec<Endpoint> = candidates.into_iter().collect();
    candidates
        .iter()
        .find(|endpoint| const_of(endpoint).is_none())
        .cloned()
        .or_else(|| {
            candidates
                .iter()
                .filter_map(|endpoint| const_of(endpoint).map(|value| (value, endpoint)))
                .min_by_key(|(value, _)| *value)
                .map(|(_, endpoint)| endpoint.clone())
        })
}

pub(super) fn lo_const(range: &IntRange) -> Option<i128> {
    range.lo.as_ref().and_then(const_of)
}

pub(super) fn hi_const(range: &IntRange) -> Option<i128> {
    range.hi.as_ref().and_then(const_of)
}

/// `value ≥ x ≥ lo` (`>` when either step is strict).
fn lower_through(strict: bool, via: &Option<Endpoint>) -> Option<Endpoint> {
    via.as_ref()
        .map(|endpoint| Endpoint::new(endpoint.key.clone(), strict || endpoint.strict))
}

impl TypeChecker<'_> {
    /// The type an expression's value has. A binding read keeps the binding's
    /// declared type: the node's own type can be the slot it is stored into.
    pub(super) fn value_type(&self, expr: &HirExpression) -> Option<TypeId> {
        if let HirExpressionKind::Path(def_id) = &expr.kind
            && let Some(binding) = self.lookup_binding(*def_id)
        {
            return Some(binding.ty);
        }
        expr.ty
    }

    pub(super) fn is_integer_expr(&self, expr: &HirExpression) -> bool {
        self.value_type(expr).is_some_and(|ty| self.is_integer(ty))
    }

    /// Whether `ty` is a list or text: the sources whose live length is a
    /// bound.
    pub(super) fn has_live_length(&self, ty: TypeId) -> bool {
        let peeled = self
            .types
            .peel_access_marker(self.resolve_inferred_type(ty))
            .1;
        matches!(
            self.types.get(peeled),
            Type::Array(_) | Type::BoundedArray { .. }
        ) || self.is_text_family(peeled)
    }

    /// The named binding a list/text expression stands for, if it is one.
    pub(super) fn length_source(&self, expr: &HirExpression) -> Option<DefId> {
        let HirExpressionKind::Path(def_id) = &expr.kind else {
            return None;
        };
        let binding = self.lookup_binding(*def_id)?;
        self.has_live_length(binding.ty).then_some(*def_id)
    }

    /// A zero-argument registry `longitudo` reads the live length and writes
    /// nothing, so it is the one call that does not end interval facts.
    pub(super) fn is_pure_length_call(
        &self,
        receiver: &HirExpression,
        name: Symbol,
        args: &[HirCallArg],
    ) -> bool {
        let (Some(receiver_ty), Some(interner), true) =
            (receiver.ty, self.interner, args.is_empty())
        else {
            return false;
        };
        radix_intrinsics::lookup_method_for_type_id(
            receiver_ty,
            self.types,
            interner.resolve(name),
            0,
        )
        .is_some_and(|entry| {
            matches!(
                entry.mir_kind,
                radix_intrinsics::IntrinsicMirKind::Collection(
                    radix_types::MirCollectionOp::Length
                )
            )
        })
    }

    /// The intrinsic receiver families a method receiver belongs to, before or
    /// after it was checked. A checked expression has its own type; a name has
    /// its binding's declared type; a call chain of read-only intrinsics (a loop
    /// body is scanned before it is checked) has the family its return kind
    /// names. Anything else, a user type included, is unknown.
    fn builtin_families(&self, expr: &HirExpression) -> Option<Vec<IntrinsicReceiver>> {
        let typed = expr.ty.or_else(|| match &expr.kind {
            HirExpressionKind::Path(def_id) => self.lookup_binding(*def_id).map(|b| b.ty),
            _ => None,
        });
        if let Some(ty) = typed {
            let ty = self.resolve_inferred_type(ty);
            return Some(radix_intrinsics::receivers_for_type_id(ty, self.types).to_vec());
        }
        let HirExpressionKind::MethodCall(receiver, name, _, args, _) = &expr.kind else {
            return None;
        };
        let entry = self.read_only_entry(receiver, *name, args.len())?;
        match entry.return_kind {
            IntrinsicReturnKind::SameAsReceiver => self.builtin_families(receiver),
            IntrinsicReturnKind::ArrayInner => Some(vec![IntrinsicReceiver::Lista]),
            IntrinsicReturnKind::Textus => Some(vec![IntrinsicReceiver::Textus]),
            IntrinsicReturnKind::Numerus => Some(vec![IntrinsicReceiver::Numeric]),
            _ => None,
        }
    }

    /// The registry row of a call that only reads: the catalog does not mark it
    /// as mutating its receiver and it takes no closure (a closure argument runs
    /// arbitrary code). `None` for an unresolved receiver or an ordinary method.
    fn read_only_entry(
        &self,
        receiver: &HirExpression,
        name: Symbol,
        arg_count: usize,
    ) -> Option<&'static radix_intrinsics::MethodIntrinsic> {
        use radix_intrinsics::IntrinsicParamKind as Param;
        let interner = self.interner?;
        let method = interner.resolve(name);
        let entry = self
            .builtin_families(receiver)?
            .into_iter()
            .find_map(|family| radix_intrinsics::lookup_method(family, method, arg_count))?;
        let closure_taking = entry.params.iter().any(|param| {
            matches!(
                param,
                Param::Predicate
                    | Param::Mapper
                    | Param::Reducer
                    | Param::Accumulator
                    | Param::TextusOrReplacer
            )
        });
        (!radix_intrinsics::is_mutating_receiver(entry)
            && !radix_intrinsics::mir_is_higher_order_lista(entry)
            && !closure_taking)
            .then_some(entry)
    }

    /// A registry intrinsic that only reads cannot write any list, so it ends
    /// no interval fact (`longitudo`, `apud`, `sectio`, ...).
    pub(super) fn is_read_only_intrinsic_call(
        &self,
        receiver: &HirExpression,
        name: Symbol,
        arg_count: usize,
    ) -> bool {
        self.read_only_entry(receiver, name, arg_count).is_some()
    }

    /// A `numerus` or sized integer with no `wrapping`/`saturating` policy: a
    /// store into it keeps the value or traps.
    fn is_plain_integer(&self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        matches!(
            self.types.get(resolved),
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _)
        )
    }

    /// The extent a size-parameter name denotes.
    fn size_param_bound(&self, def_id: DefId) -> Option<BoundKey> {
        let name = if self.current_size_param_defs.contains(&def_id) {
            self.current_size_param_by_name
                .iter()
                .find_map(|(name, def)| (*def == def_id).then_some(*name))?
        } else {
            let symbol = self.resolver.get_symbol(def_id)?;
            (symbol.kind == crate::SymbolKind::IndexParam).then_some(symbol.name)?
        };
        Some(BoundKey::Axis(AxisIdentity::Param(name)))
    }

    fn path_range(&self, def_id: DefId) -> IntRange {
        if let Some(key) = self.size_param_bound(def_id) {
            let mut range = IntRange::exact(key);
            range.lo = Some(Endpoint::new(BoundKey::Const(0), false));
            return range;
        }
        let Some(binding) = self.lookup_binding(def_id) else {
            return IntRange::default();
        };
        let fact = self.range_flow.get(def_id).cloned().unwrap_or_default();
        if binding.mutable {
            return fact;
        }
        // An immutable binding is its own bound.
        // A fact the flow holds beats the binding's own name: that only
        // restates the value, and would displace a literal ceiling a
        // consumer compares against an extent.
        let own = Endpoint::new(BoundKey::Binding(def_id), false);
        IntRange {
            lo: fact.lo.or_else(|| Some(own.clone())),
            hi: fact.hi.or(Some(own)),
        }
    }

    /// The interval of an integer expression's value, symbolic where a name is
    /// stable. Empty for a non-integer expression.
    pub(super) fn range_of_expr(&self, expr: &HirExpression) -> IntRange {
        if !self.is_integer_expr(expr) {
            return IntRange::default();
        }
        let (abs_lo, abs_hi) = self
            .declared_cell_range(expr)
            .or_else(|| self.value_range(expr, self.value_type(expr)))
            .map_or((None, None), |(lo, hi)| (finite(lo), finite(hi)));
        let absolute = IntRange {
            lo: abs_lo.map(|value| Endpoint::new(BoundKey::Const(value), false)),
            hi: abs_hi.map(|value| Endpoint::new(BoundKey::Const(value), false)),
        };
        let symbolic = self.symbolic_range(expr);
        IntRange {
            lo: pick_lower(symbolic.lo.into_iter().chain(absolute.lo)),
            hi: pick_upper(symbolic.hi.into_iter().chain(absolute.hi)),
        }
        .normalized()
    }

    /// A read of a ranged cell lies in its declared range, whatever policy the
    /// cell applies at a store.
    fn declared_cell_range(&self, expr: &HirExpression) -> Option<(i128, i128)> {
        let HirExpressionKind::Path(def_id) = &expr.kind else {
            return None;
        };
        let ty = self.resolve_inferred_type(self.lookup_binding(*def_id)?.ty);
        let ty = self.types.peel_access_marker(ty).1;
        match self.types.get(ty) {
            Type::RangedInt { lo, hi, .. } => Some((*lo, *hi)),
            _ => None,
        }
    }

    fn symbolic_range(&self, expr: &HirExpression) -> IntRange {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.path_range(*def_id),
            // An implicit store node into a plain trapping slot either traps
            // or keeps the value; a lossless `inf` widening always keeps it.
            // A `wrapping` or `saturating` slot reduces: nothing carries over.
            HirExpressionKind::Widen { operand, .. } => self.range_of_expr(operand),
            HirExpressionKind::Conversio {
                source,
                target: radix_hir::HirConversioTarget::Type(slot),
                implicit: Some(_),
                ..
            } if self.is_plain_integer(*slot) => self.range_of_expr(source),
            HirExpressionKind::MethodCall(receiver, name, _, args, _) => {
                self.method_range(receiver, *name, args)
            }
            HirExpressionKind::Binary(HirBinOp::Mod, _, divisor) => {
                let divisor = self.range_of_expr(divisor);
                // Floor remainder: `[0, d)` only for a divisor proven positive.
                if lo_const(&divisor).is_none_or(|lo| lo < 1) {
                    return IntRange::default();
                }
                IntRange {
                    lo: Some(Endpoint::new(BoundKey::Const(0), false)),
                    hi: lower_through(true, &divisor.hi),
                }
            }
            HirExpressionKind::Binary(HirBinOp::Add, lhs, rhs) => {
                // `e + c` for a literal `c` (either side): the literal shifts
                // every literal endpoint. A relation keeps its key only where
                // integer order proves it: `e < K` gives `e + 1 ≤ K`, and a
                // non-positive `c` only lowers the value. A variable addend
                // has no fact (`column + shift` stays unproven).
                let (operand, shift) = match (
                    super::integer_range::const_int_value(rhs),
                    super::integer_range::const_int_value(lhs),
                ) {
                    (Some(shift), _) => (lhs, shift),
                    (None, Some(shift)) => (rhs, shift),
                    (None, None) => return IntRange::default(),
                };
                let operand = self.range_of_expr(operand);
                let shifted = |endpoint: &Option<Endpoint>| {
                    endpoint.as_ref().and_then(|endpoint| {
                        let value = const_of(endpoint)?.checked_add(shift)?;
                        Some(Endpoint::new(BoundKey::Const(value), endpoint.strict))
                    })
                };
                let relation = operand.hi.as_ref().and_then(|hi| match shift {
                    ..=0 => Some(hi.clone()),
                    1 if hi.strict => Some(Endpoint::new(hi.key.clone(), false)),
                    _ => None,
                });
                IntRange {
                    lo: shifted(&operand.lo),
                    hi: shifted(&operand.hi).or(relation),
                }
            }
            HirExpressionKind::Binary(HirBinOp::Sub, lhs, rhs) => {
                let Some(shift) = super::integer_range::const_int_value(rhs) else {
                    return IntRange::default();
                };
                let lhs = self.range_of_expr(lhs);
                let shifted = |endpoint: &Option<Endpoint>| {
                    endpoint.as_ref().and_then(|endpoint| {
                        let value = const_of(endpoint)?.checked_sub(shift)?;
                        Some(Endpoint::new(BoundKey::Const(value), endpoint.strict))
                    })
                };
                IntRange {
                    lo: shifted(&lhs.lo),
                    // `e ≤ K` gives `e - c < K` for a positive `c`; a relation
                    // has no offset form, so a non-positive `c` drops it.
                    hi: shifted(&lhs.hi)
                        .or_else(|| (shift >= 1).then(|| lower_through(true, &lhs.hi)).flatten()),
                }
            }
            _ => IntRange::default(),
        }
    }

    fn method_range(
        &self,
        receiver: &HirExpression,
        name: Symbol,
        args: &[HirCallArg],
    ) -> IntRange {
        let Some(interner) = self.interner else {
            return IntRange::default();
        };
        let Some(receiver_ty) = receiver.ty else {
            return IntRange::default();
        };
        let Some(entry) = radix_intrinsics::lookup_method_for_type_id(
            receiver_ty,
            self.types,
            interner.resolve(name),
            args.len(),
        ) else {
            return IntRange::default();
        };
        if self.is_pure_length_call(receiver, name, args) {
            let Some(source) = self.length_source(receiver) else {
                return IntRange::default();
            };
            return IntRange {
                lo: Some(Endpoint::new(BoundKey::Const(0), false)),
                hi: Some(Endpoint::new(self.range_flow.live_length(source), false)),
            };
        }
        let numeric = matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Numeric);
        let [arg] = args else {
            return IntRange::default();
        };
        if !numeric || !self.is_integer_expr(receiver) || !self.is_integer_expr(&arg.expr) {
            return IntRange::default();
        }
        let (a, b) = (self.range_of_expr(receiver), self.range_of_expr(&arg.expr));
        match entry.faber_name {
            // `max(a, b)`: a lower bound from either operand; an upper one only
            // when both are bounded. When one operand provably dominates, the
            // result is that operand.
            "maximus" => match (lo_const(&a), hi_const(&b), lo_const(&b), hi_const(&a)) {
                (Some(a_lo), Some(b_hi), _, _) if a_lo >= b_hi => a,
                (_, _, Some(b_lo), Some(a_hi)) if b_lo >= a_hi => b,
                _ => a.max_of(&b),
            },
            "minimus" => match (hi_const(&a), lo_const(&b), hi_const(&b), lo_const(&a)) {
                (Some(a_hi), Some(b_lo), _, _) if a_hi <= b_lo => a,
                (_, _, Some(b_hi), Some(a_lo)) if b_hi <= a_lo => b,
                _ => a.min_of(&b),
            },
            _ => IntRange::default(),
        }
    }

    // ── guards ────────────────────────────────────────────────────────────

    /// The `(then, else)` facts of an integer comparison leaf, or `None` when
    /// the leaf is not one (so the null/member probes get their turn).
    pub(super) fn guard_refinement(&self, cond: &HirExpression) -> Option<LeafRefinement> {
        let HirExpressionKind::Binary(op, lhs, rhs) = &cond.kind else {
            return None;
        };
        if !matches!(
            op,
            HirBinOp::Lt
                | HirBinOp::LtEq
                | HirBinOp::Gt
                | HirBinOp::GtEq
                | HirBinOp::Eq
                | HirBinOp::StrictEq
                | HirBinOp::NotEq
                | HirBinOp::StrictNotEq
        ) || !self.is_integer_expr(lhs)
            || !self.is_integer_expr(rhs)
        {
            return None;
        }
        // The subject is the binding operand; the comparison is read with it
        // on the left.
        let (subject, other, op) = match (&lhs.kind, &rhs.kind) {
            (HirExpressionKind::Path(def), _) if self.lookup_binding(*def).is_some() => {
                (*def, rhs, *op)
            }
            (_, HirExpressionKind::Path(def)) if self.lookup_binding(*def).is_some() => {
                (*def, lhs, flipped(*op))
            }
            _ => return None,
        };
        let bound = self.range_of_expr(other);
        let (then_range, else_range) = match op {
            HirBinOp::Lt => (upper_from(&bound, true), lower_from(&bound, false)),
            HirBinOp::LtEq => (upper_from(&bound, false), lower_from(&bound, true)),
            HirBinOp::Gt => (lower_from(&bound, true), upper_from(&bound, false)),
            HirBinOp::GtEq => (lower_from(&bound, false), upper_from(&bound, true)),
            HirBinOp::Eq | HirBinOp::StrictEq => (bound.clone(), IntRange::default()),
            _ => (IntRange::default(), bound.clone()),
        };
        let fact = |range: IntRange| {
            let range = range.normalized();
            (!range.is_unbounded()).then(|| (FactKey::Binding(subject), FactValue::range(range)))
        };
        Some((fact(then_range), fact(else_range)))
    }

    // ── loops and coordinates ─────────────────────────────────────────────

    /// The interval a range loop's binder lies in, from the endpoints captured
    /// at entry.
    fn range_loop_interval(&self, iter: &HirExpression) -> Option<IntRange> {
        let HirExpressionKind::Intervallum {
            start, end, kind, ..
        } = &iter.kind
        else {
            return None;
        };
        let (start, end) = (self.range_of_expr(start), self.range_of_expr(end));
        let inclusive = matches!(kind, HirRangeKind::Inclusive);
        // Ascending: start ≤ i, and i < end (i ≤ end when inclusive).
        let ascending = IntRange {
            lo: lower_through(false, &start.lo),
            hi: lower_through(!inclusive, &end.hi),
        };
        // Descending: i ≤ start, and end < i (end ≤ i when inclusive).
        let descending = IntRange {
            lo: lower_through(!inclusive, &end.lo),
            hi: lower_through(false, &start.hi),
        };
        let ascends = matches!((hi_const(&start), lo_const(&end)), (Some(s), Some(e)) if s <= e);
        let descends = matches!((lo_const(&start), hi_const(&end)), (Some(s), Some(e)) if s > e);
        let interval = match (ascends, descends) {
            (true, _) => ascending,
            (false, true) => descending,
            (false, false) => ascending.hull(&descending)?,
        };
        Some(interval.normalized()).filter(|range| !range.is_unbounded())
    }

    fn assume_binder(&mut self, binder: DefId, range: IntRange) {
        let range = range.normalized();
        if !range.is_unbounded() {
            self.range_flow.assume(binder, &range);
        }
    }

    /// Seed a range loop's binder before its body is checked.
    pub(super) fn seed_range_loop_binder(
        &mut self,
        binder: DefId,
        name: Symbol,
        iter: &HirExpression,
    ) {
        self.record_binding_name(binder, name);
        let integer = self
            .lookup_binding(binder)
            .is_some_and(|binding| self.is_integer(binding.ty));
        if !integer {
            return;
        }
        if let Some(interval) = self.range_loop_interval(iter) {
            self.assume_binder(binder, interval);
        }
    }

    /// Seed the coordinates of an indexed iteration (`apud`): a list's single
    /// coordinate or a tensor's per-axis ones.
    pub(super) fn seed_coordinates(
        &mut self,
        dispatch_ty: TypeId,
        source: Option<&HirExpression>,
        coordinates: &[(DefId, Symbol)],
    ) {
        for (coordinate, name) in coordinates {
            self.record_binding_name(*coordinate, *name);
        }
        let resolved = self.resolve_inferred_type(dispatch_ty);
        let peeled = self.types.peel_access_marker(resolved).1;
        match self.types.get(peeled).clone() {
            Type::Array(_) | Type::BoundedArray { .. } => {
                let [(coordinate, _)] = coordinates else {
                    return;
                };
                let hi = source
                    .and_then(|source| self.length_source(source))
                    .map(|source| Endpoint::new(self.range_flow.live_length(source), true));
                self.assume_binder(
                    *coordinate,
                    IntRange {
                        lo: Some(Endpoint::new(BoundKey::Const(0), false)),
                        hi,
                    },
                );
            }
            Type::Tensor(_, shape) | Type::Matrix(_, shape) => {
                let Some(axes) = self.axis_bounds(shape) else {
                    return;
                };
                if axes.len() != coordinates.len() {
                    return;
                }
                for ((coordinate, _), axis) in coordinates.iter().zip(axes) {
                    self.assume_binder(
                        *coordinate,
                        IntRange {
                            lo: Some(Endpoint::new(BoundKey::Const(0), false)),
                            hi: axis.map(|key| Endpoint::new(key, true)),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    /// The resolved extent of each axis of a shape; `None` per axis whose
    /// extent is an unsolved hole or unspecified, `None` overall for a shape
    /// that is not a literal or a tuple of dimensions.
    pub(super) fn axis_bounds(&mut self, shape: IndexId) -> Option<Vec<Option<BoundKey>>> {
        let shape = self.resolve_index(shape);
        let dims = match self.types.get_index(shape).clone() {
            IndexExpr::Tuple(dims) => dims,
            IndexExpr::Literal(_) => vec![shape],
            _ => return None,
        };
        Some(
            dims.into_iter()
                .map(|dim| axis_bound(self.types, &mut self.index_infer, dim))
                .collect(),
        )
    }

    /// Forget the facts about loop or reduction binders that leave scope.
    pub(super) fn release_range_binders(&mut self, binders: impl IntoIterator<Item = DefId>) {
        for binder in binders {
            self.range_flow.drop_subject(binder);
        }
    }

    /// An immutable integer `let` holds its initializer's interval. A `wrapping`
    /// or `saturating` destination reduces at the store, so only a plain
    /// destination qualifies.
    pub(super) fn seed_let_range(&mut self, local: &radix_hir::HirLocal) {
        let (false, Some(init), Some(binding)) = (
            local.mutable,
            local.init.as_ref(),
            self.lookup_binding(local.def_id),
        ) else {
            return;
        };
        if self.is_plain_integer(binding.ty) {
            let range = self.range_of_expr(init);
            self.assume_binder(local.def_id, range);
        }
    }
}

/// `x ⋈ y` read with the operands swapped.
fn flipped(op: HirBinOp) -> HirBinOp {
    match op {
        HirBinOp::Lt => HirBinOp::Gt,
        HirBinOp::Gt => HirBinOp::Lt,
        HirBinOp::LtEq => HirBinOp::GtEq,
        HirBinOp::GtEq => HirBinOp::LtEq,
        other => other,
    }
}

/// `x < y` / `x ≤ y` from `y`'s upper bound: `x` is below it too.
fn upper_from(bound: &IntRange, strict: bool) -> IntRange {
    IntRange {
        lo: None,
        hi: lower_through(strict, &bound.hi),
    }
}

/// `x > y` / `x ≥ y` from `y`'s lower bound: `x` is above it too.
fn lower_from(bound: &IntRange, strict: bool) -> IntRange {
    IntRange {
        lo: lower_through(strict, &bound.lo),
        hi: None,
    }
}
