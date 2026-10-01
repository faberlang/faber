//! The unbounded integer `inf` inside the type checker (inf-delivery
//! rulings 32, 34–41).
//!
//! `inf` is `SizedNumeric(Numerus, Inf)`: an integer width with no interval.
//! The range rule, the store rule and the conversion table each answer it
//! explicitly (`integer_range.rs`, `store.rs`, `convert.rs`); this module
//! holds the shared predicates and the one place a bounded integer is widened
//! into it.
//!
//! WIDENING NODE
//! =============
//! A bounded integer that lands in an `inf` slot (a store, an argument, a
//! return, a field, an element) is wrapped in `HirExpressionKind::Widen`
//! right where the slot unifies with it, so the wrapped expression keeps its
//! real bounded type and every backend converts from that type instead of
//! reading it back from a side table. An untyped constant is not wrapped: it
//! joins by exact value (ruling 34) and is typed by the slot.

use super::TypeChecker;
use super::integer_range::const_int_exact;
use super::ops::IntFamily;
use radix_hir::{HirExpression, HirExpressionKind};
use radix_types::{Type, TypeId};

impl TypeChecker<'_> {
    /// Whether `ty` is the unbounded integer (through aliases, access markers
    /// and solved inference holes).
    pub(super) fn is_inf_type(&self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        let (_, resolved) = self.types.peel_access_marker(resolved);
        radix_types::numeric_width::is_unbounded_integer_type(self.types, resolved)
    }

    /// Whether `slot_ty` is an `inf` slot: `inf` itself, or an optional of it.
    fn is_inf_slot(&self, slot_ty: TypeId) -> bool {
        if self.is_inf_type(slot_ty) {
            return true;
        }
        let resolved = self.resolve_inferred_type(slot_ty);
        match self.types.get(self.types.peel_access_marker(resolved).1) {
            Type::Option(inner) => self.is_inf_type(*inner),
            _ => false,
        }
    }

    /// The `inf` type a value of type `ty` widens into when it lands in
    /// `slot_ty`, when it needs a widening node: `ty` is a bounded trapping
    /// integer, the slot is `inf`, and the expression is not an untyped
    /// constant (those join by exact value).
    fn inf_widening_target(
        &mut self,
        expr: &HirExpression,
        ty: TypeId,
        slot_ty: TypeId,
    ) -> Option<TypeId> {
        if matches!(expr.kind, HirExpressionKind::Widen { .. }) || const_int_exact(expr).is_some() {
            return None;
        }
        let (_, source) = self.store_int_slot(ty)?;
        if source.family != IntFamily::Plain || !self.is_inf_slot(slot_ty) {
            return None;
        }
        Some(self.inf_type())
    }

    /// Wrap `expr` (already checked, of bounded type `ty`) in a widening node
    /// when it lands in the `inf` slot `slot_ty`. Returns the node's type
    /// (`inf`), or `None` when no node is needed and `expr` is untouched.
    pub(super) fn widen_into_inf(
        &mut self,
        expr: &mut HirExpression,
        ty: TypeId,
        slot_ty: TypeId,
    ) -> Option<TypeId> {
        let to = self.inf_widening_target(expr, ty, slot_ty)?;
        let from = self.resolve_inferred_type(ty);
        let operand = HirExpression {
            id: expr.id,
            kind: std::mem::replace(&mut expr.kind, HirExpressionKind::Error),
            ty: Some(from),
            span: expr.span,
        };
        expr.id = self.fresh_hir_id();
        expr.kind = HirExpressionKind::Widen {
            operand: Box::new(operand),
            from,
            to,
        };
        expr.ty = Some(to);
        Some(to)
    }
}
