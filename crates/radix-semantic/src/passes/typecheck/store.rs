//! Checked stores (d11-6 rulings 2, 5, 17, 21).
//!
//! Every position where a value lands in a typed slot applies the slot's
//! policy. Integer expressions are typed by the range rule, so a store into a
//! type that contains the expression's type never fails and needs no node. A
//! store into a narrower integer slot of the same policy family gets an
//! **implicit store node** (`Conversio` with `implicit: Some(site)`), carrying
//! the slot as its target type: a trapping slot traps, a `wrapping` slot
//! reduces, a `saturating` slot clamps. Crossing policy families still needs
//! an authored `↦`.
//!
//! A value known at compile time is checked at compile time: a constant that
//! certainly traps in a trapping slot is a compile error, a constant in an `=`
//! position (`generis`, field default, enum member) must fit whatever the
//! policy, and a constant stored with `←` into a `wrapping` or `saturating`
//! slot follows the slot's policy (`saturating<u8> w ← 300` is 255).
//!
//! INVARIANTS
//! ==========
//! - Only positions that call [`TypeChecker::check_store_expr`] are stores;
//!   an expected type passed as a mere hint (operand shaping) never narrows.
//! - Type-argument positions never come through here: invariance stands.
//! - A union slot with several numeric members takes no implicit narrowing;
//!   with exactly one it narrows into that member.

use super::integer_range::const_int_value;
use super::ops::IntFamily;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::{HirStoreKind, HirStoreSite};
use radix_types::numeric_range::IntRange;

const STORE_CONSTANT_OUT_OF_RANGE: &str = "store_constant_out_of_range";

/// One store position of the d11-6 table (ruling 21) as the checker sees it.
#[derive(Clone, Copy)]
pub(super) struct StoreSite {
    pub(super) kind: HirStoreKind,
    /// Name of the receiving binding, field or parameter, when it has one.
    pub(super) slot: Option<Symbol>,
    /// For an inferred (`_`) slot: the expression its type came from.
    pub(super) inferred_from: Option<radix_lexer::Span>,
    /// An `=` position states a compile-time fact: a constant must fit.
    pub(super) compile_time: bool,
    /// A store into a plain local: the backends already check the value at
    /// the local's assignment, so a capped `i64` needs no separate node.
    pub(super) local_slot: bool,
}

impl StoreSite {
    pub(super) fn new(kind: HirStoreKind) -> Self {
        Self {
            kind,
            slot: None,
            inferred_from: None,
            compile_time: false,
            local_slot: false,
        }
    }

    /// Mark a store into a plain local binding.
    pub(super) fn local_slot(mut self) -> Self {
        self.local_slot = true;
        self
    }

    pub(super) fn named(mut self, slot: Symbol) -> Self {
        self.slot = Some(slot);
        self
    }

    pub(super) fn inferred_from(mut self, from: Option<radix_lexer::Span>) -> Self {
        self.inferred_from = from;
        self
    }

    /// Mark an `=` position: a constant that does not fit is a compile error
    /// whatever the slot's policy.
    pub(super) fn compile_time(mut self) -> Self {
        self.compile_time = true;
        self
    }
}

/// The store position of one call argument, named when written `name: value`.
pub(super) fn argument_site(name: Option<Symbol>) -> StoreSite {
    let site = StoreSite::new(HirStoreKind::Argument);
    match name {
        Some(name) => site.named(name),
        None => site,
    }
}

/// An integer slot: policy family, declared width and its exact range.
#[derive(Clone, Copy)]
pub(super) struct IntSlot {
    family: IntFamily,
    width: NumericWidth,
    range: IntRange,
}

impl IntSlot {
    fn holds(self, value: i128) -> bool {
        self.range.0 <= value && value <= self.range.1
    }

    /// Spelling used in diagnostics, `u8` or `saturating<u8>`.
    fn spelling(self) -> String {
        let width = format!("{:?}", self.width).to_lowercase();
        match self.family {
            IntFamily::Plain => width,
            IntFamily::Modular => format!("wrapping<{width}>"),
            IntFamily::Saturating => format!("saturating<{width}>"),
        }
    }
}

impl TypeChecker<'_> {
    /// Check `expr` against a slot as a store: the slot's type is the expected
    /// type, and the value is narrowed, reduced or clamped at the slot under
    /// the slot's policy.
    pub(super) fn check_store_expr(
        &mut self,
        expr: &mut HirExpression,
        slot_ty: TypeId,
        site: StoreSite,
    ) -> TypeId {
        // A slot that is still an inference variable is a generic witness or
        // an open `_`, not a declared type: it binds instead of narrowing.
        // Only a reassignment reaches a `_` slot after its type is settled.
        let open_slot = self.is_infer(slot_ty) || self.infer_union_var_of(slot_ty).is_some();
        if open_slot && site.kind != HirStoreKind::Assignment {
            return self.check_expr_with_expected(expr, Some(slot_ty));
        }
        if let Some(ty) = self.constant_store(expr, slot_ty, site) {
            return ty;
        }
        self.pending_store = Some(site);
        let ty = self.check_expr_with_expected(expr, Some(slot_ty));
        self.pending_store = None;
        ty
    }

    /// Check `expr` on its own type, then apply the store rule: a constant is
    /// checked at compile time, and a narrower integer slot gets an implicit
    /// store node. Unlike [`Self::check_store_expr`] the slot is never handed to
    /// the expression as an expected type, so the expression keeps its own type
    /// (a thrown or yielded member of a union slot stays the member). Any other
    /// mismatch is left to the caller's assignment check.
    pub(super) fn check_store_by_type(
        &mut self,
        expr: &mut HirExpression,
        slot_ty: TypeId,
        site: StoreSite,
    ) -> TypeId {
        if self.is_declared_slot(slot_ty) {
            if let Some(ty) = self.constant_store(expr, slot_ty, site) {
                return ty;
            }
            let ty = self.check_expr(expr);
            if let Some(target) = self
                .store_narrowing_target(ty, slot_ty)
                .or_else(|| self.capped_store_target(expr, ty, slot_ty, site))
            {
                return self.wrap_implicit_store(expr, ty, target, site);
            }
            return ty;
        }
        self.check_expr(expr)
    }

    /// Land an already-checked value of type `ty` in `slot_ty`: an implicit
    /// store node when the slot is a narrower integer of the same policy
    /// family, otherwise the ordinary assignment unification.
    pub(super) fn coerce_store(
        &mut self,
        expr: &mut HirExpression,
        ty: TypeId,
        slot_ty: TypeId,
        site: StoreSite,
    ) -> TypeId {
        match self
            .store_narrowing_target(ty, slot_ty)
            .or_else(|| self.capped_store_target(expr, ty, slot_ty, site))
        {
            Some(target) => self.wrap_implicit_store(expr, ty, target, site),
            None => self.unify_expected(ty, slot_ty, expr.span),
        }
    }

    /// An `i64`-typed expression at the 64-bit cap (`i64 + i64`) stored into an
    /// `i64` slot: the types agree but the value may not fit, so the store
    /// still needs its check (ruling 17b). `None` when the expression is
    /// proven to fit or the slot is not a plain `i64`.
    fn capped_store_target(
        &self,
        expr: &HirExpression,
        ty: TypeId,
        slot_ty: TypeId,
        site: StoreSite,
    ) -> Option<TypeId> {
        let (_, src) = self.store_int_slot(ty)?;
        let (dst_ty, dst) = self.store_int_target(slot_ty)?;
        let plain_i64 = |s: IntSlot| s.family == IntFamily::Plain && s.width == NumericWidth::I64;
        (!site.local_slot && plain_i64(src) && plain_i64(dst) && self.exceeds_i64(expr, ty))
            .then_some(dst_ty)
    }

    /// Whether a slot type is declared (no open inference variable at the top).
    pub(super) fn is_declared_slot(&self, slot_ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(slot_ty);
        !self.is_infer(resolved) && self.infer_union_var_of(resolved).is_none()
    }

    /// Integer shape of a concrete slot type (aliases and access markers
    /// peeled); `None` for floats, decimals, holes and non-numerics.
    pub(super) fn store_int_slot(&self, ty: TypeId) -> Option<(TypeId, IntSlot)> {
        let mut ty = self.resolve_inferred_type(ty);
        ty = self.types.peel_access_marker(ty).1;
        while let Type::Alias(_, inner) = self.types.get(ty) {
            ty = self.resolve_inferred_type(*inner);
        }
        let shape = self.numeric_shape(ty)?;
        if shape.prim != Primitive::Numerus {
            return None;
        }
        let range = shape.width.integer_range()?;
        Some((
            ty,
            IntSlot {
                family: shape.family,
                width: shape.width,
                range,
            },
        ))
    }

    /// The integer type a store into `slot_ty` is checked against: the slot
    /// itself, or the single numeric member of a union (row 23). A union with
    /// several numeric members has none (row 24).
    fn store_int_target(&self, slot_ty: TypeId) -> Option<(TypeId, IntSlot)> {
        if let Some(found) = self.store_int_slot(slot_ty) {
            return Some(found);
        }
        let resolved = self.resolve_inferred_type(slot_ty);
        let members: Vec<TypeId> = match self.types.get(self.types.peel_access_marker(resolved).1) {
            Type::Union(members) => members.clone(),
            Type::Option(inner) => vec![*inner],
            _ => return None,
        };
        let mut numeric = members.into_iter().filter_map(|m| self.store_int_slot(m));
        let only = numeric.next()?;
        numeric.next().is_none().then_some(only)
    }

    /// The type a narrowing store lands in, when a value of type `ty` needs an
    /// implicit store node to reach `slot_ty`.
    pub(super) fn store_narrowing_target(&self, ty: TypeId, slot_ty: TypeId) -> Option<TypeId> {
        let (src_ty, src) = self.store_int_slot(ty)?;
        let (dst_ty, dst) = self.store_int_target(slot_ty)?;
        let slot_ty = self
            .types
            .peel_access_marker(self.resolve_inferred_type(slot_ty))
            .1;
        if src.family != dst.family || self.types.is_assignable(src_ty, slot_ty) {
            return None;
        }
        Some(dst_ty)
    }

    /// Compile-time half of the store rule for a constant in an integer slot.
    ///
    /// `None` means the constant fits (or the slot is not an integer) and the
    /// ordinary path runs. Otherwise the store is resolved here: a compile
    /// error, or an implicit store node that reduces or clamps.
    fn constant_store(
        &mut self,
        expr: &mut HirExpression,
        slot_ty: TypeId,
        site: StoreSite,
    ) -> Option<TypeId> {
        let value = const_int_value(expr)?;
        let (slot_int_ty, slot) = self.store_int_target(slot_ty)?;
        if slot.holds(value) {
            return self.fold_word_slot_constant(expr, value, slot_int_ty, slot);
        }
        if site.compile_time || slot.family == IntFamily::Plain {
            self.check_const_operand(expr);
            self.errors.push(
                SemanticError::structured(SemanticErrorKind::TypeMismatch, expr.span)
                    .with_arg("issue", STORE_CONSTANT_OUT_OF_RANGE)
                    .with_arg("value", value.to_string())
                    .with_arg("slot", slot.spelling()),
            );
            expr.ty = Some(slot_int_ty);
            return Some(slot_int_ty);
        }
        // `←` into a `wrapping` or `saturating` slot: the constant follows the
        // slot's policy. A value below `i64::MIN` has no 64-bit carrier to
        // convert from, so the policy is applied here, exactly (ruling 21).
        if value < i128::from(i64::MIN) {
            let (lo, hi) = slot.range;
            let applied = match slot.family {
                IntFamily::Saturating => value.clamp(lo, hi),
                _ => lo + (value - lo).rem_euclid(hi - lo + 1),
            };
            return self.rewrite_as_slot_literal(expr, applied, slot_int_ty);
        }
        // It is checked as a plain `int` and wrapped.
        let ty = self.check_const_operand(expr);
        Some(self.wrap_implicit_store(expr, ty, slot_int_ty, site))
    }

    /// A constant-only expression that fits a `wrapping` or `saturating` slot
    /// is computed exactly first (ruling 12a); the slot then takes the value.
    /// It is folded to the literal of that value, typed as the slot. A bare
    /// literal, a negated literal whose magnitude fits the slot, and any other
    /// slot follow the ordinary path. A negated literal whose magnitude does
    /// not fit but whose value does (`-2⁶³` in `saturating<i64>`) is folded:
    /// the value is exact (ruling 12), the magnitude alone is not the slot's.
    fn fold_word_slot_constant(
        &mut self,
        expr: &mut HirExpression,
        value: i128,
        slot_ty: TypeId,
        slot: IntSlot,
    ) -> Option<TypeId> {
        let bare = match &expr.kind {
            HirExpressionKind::Literal(_) => true,
            HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => match inner.kind {
                HirExpressionKind::Literal(HirLiteral::Int(magnitude)) => {
                    slot.holds(i128::from(magnitude))
                }
                HirExpressionKind::Literal(_) => true,
                _ => false,
            },
            _ => false,
        };
        if slot.family == IntFamily::Plain || bare {
            return None;
        }
        self.rewrite_as_slot_literal(expr, value, slot_ty)
    }

    /// Replace a constant expression by the literal of `value`, typed as the slot.
    fn rewrite_as_slot_literal(
        &mut self,
        expr: &mut HirExpression,
        value: i128,
        slot_ty: TypeId,
    ) -> Option<TypeId> {
        let magnitude = u64::try_from(value.unsigned_abs()).ok()?;
        let span = expr.span;
        let mut literal = HirExpression {
            id: self.fresh_hir_id(),
            kind: HirExpressionKind::Literal(HirLiteral::Int(magnitude)),
            ty: Some(slot_ty),
            span,
        };
        if value < 0 {
            literal = HirExpression {
                id: self.fresh_hir_id(),
                kind: HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, Box::new(literal)),
                ty: Some(slot_ty),
                span,
            };
        }
        expr.kind = literal.kind;
        expr.ty = Some(slot_ty);
        Some(slot_ty)
    }

    /// The HIR record of a store position, quoting the inferring expression
    /// when the analysis has the source text (ruling 19(i)).
    pub(super) fn hir_store_site(&self, site: StoreSite) -> HirStoreSite {
        let inferred_text = site.inferred_from.and_then(|from| {
            let text = self
                .source_text
                .as_deref()?
                .get(from.start as usize..from.end as usize)?;
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let text = if text.chars().count() > 80 {
                format!("{}…", text.chars().take(79).collect::<String>())
            } else {
                text
            };
            Some(text.into_boxed_str())
        });
        HirStoreSite {
            kind: site.kind,
            slot: site.slot,
            inferred_from: site.inferred_from,
            inferred_text,
        }
    }

    /// Replace `expr` by an implicit store node targeting `target`.
    pub(super) fn wrap_implicit_store(
        &mut self,
        expr: &mut HirExpression,
        source_ty: TypeId,
        target: TypeId,
        site: StoreSite,
    ) -> TypeId {
        let source = HirExpression {
            id: expr.id,
            kind: std::mem::replace(&mut expr.kind, HirExpressionKind::Error),
            ty: Some(self.resolve_inferred_type(source_ty)),
            span: expr.span,
        };
        expr.id = self.fresh_hir_id();
        expr.kind = HirExpressionKind::Conversio {
            source: Box::new(source),
            target: HirConversioTarget::Type(target),
            params: Vec::new(),
            recovery: None,
            route: None,
            implicit: Some(self.hir_store_site(site)),
        };
        expr.ty = Some(target);
        target
    }
}

/// Whether an expression is a `saturating` arithmetic operator node.
fn is_arithmetic_node(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Binary(op, ..) => matches!(
            op,
            radix_hir::HirBinOp::Add
                | radix_hir::HirBinOp::Sub
                | radix_hir::HirBinOp::Mul
                | radix_hir::HirBinOp::Div
                | radix_hir::HirBinOp::Mod
        ),
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, _) => true,
        _ => false,
    }
}

impl TypeChecker<'_> {
    /// Whether `slot` is a `saturating` word and `source` an arithmetic node:
    /// the shape of a saturating root store node.
    pub(super) fn is_saturating_arithmetic(&self, source: &HirExpression, slot: TypeId) -> bool {
        is_arithmetic_node(source)
            && self
                .store_int_slot(slot)
                .is_some_and(|(_, slot)| slot.family == IntFamily::Saturating)
    }

    /// Whether `expr` is a `saturating` arithmetic node (bare, or already
    /// wrapped in its root store node).
    fn is_saturating_tree(&self, expr: &HirExpression) -> bool {
        let node = match &expr.kind {
            HirExpressionKind::Conversio {
                source,
                implicit: Some(_),
                ..
            } => source,
            _ => expr,
        };
        is_arithmetic_node(node)
            && expr
                .ty
                .and_then(|ty| self.store_int_slot(ty))
                .is_some_and(|(_, slot)| slot.family == IntFamily::Saturating)
    }

    /// d11-6 ruling 3: a `saturating<W>` expression is computed exactly and
    /// clamped once, where the value lands. A tree of arithmetic nodes (or a
    /// node with a constant outside `W`) is retyped `int` (exact, ruling 17)
    /// and its root is wrapped in one implicit store node into
    /// `saturating<W>`, which clamps. A wrapped child is unwrapped again when
    /// its parent is also saturating arithmetic, so a chain clamps once, at
    /// its root. A single operation over in-range leaves is already exact
    /// until its own clamp and stays as it is.
    ///
    /// `source` is set when `expr` is already the source of such a node (a
    /// re-check): its children are unwrapped and it stays `int`.
    pub(super) fn saturating_arithmetic_root(
        &mut self,
        expr: &mut HirExpression,
        ty: TypeId,
        source: bool,
    ) -> TypeId {
        if self.current_device_function || !is_arithmetic_node(expr) {
            return ty;
        }
        let Some((word, slot)) = self.store_int_slot(ty) else {
            return ty;
        };
        if slot.family != IntFamily::Saturating {
            return ty;
        }
        let children: Vec<&HirExpression> = match &expr.kind {
            HirExpressionKind::Binary(_, lhs, rhs) => vec![lhs, rhs],
            HirExpressionKind::Unary(_, operand) => vec![operand],
            _ => Vec::new(),
        };
        let exact_operands = children.iter().any(|child| {
            self.is_saturating_tree(child)
                || const_int_value(child).is_some_and(|value| !slot.holds(value))
        });
        if !exact_operands && !source {
            return ty;
        }
        let exact = self.numerus_type();
        match &mut expr.kind {
            HirExpressionKind::Binary(_, lhs, rhs) => {
                self.unwrap_saturating_tree(lhs, exact);
                self.unwrap_saturating_tree(rhs, exact);
            }
            HirExpressionKind::Unary(_, operand) => self.unwrap_saturating_tree(operand, exact),
            _ => {}
        }
        if source {
            return exact;
        }
        let site = StoreSite::new(HirStoreKind::Declaration);
        self.wrap_implicit_store(expr, exact, word, site)
    }

    /// Turn a saturating child into its exact arithmetic: a root store node is
    /// replaced by the node it wraps, and a bare node is retyped `int`.
    fn unwrap_saturating_tree(&self, expr: &mut HirExpression, exact: TypeId) {
        if !self.is_saturating_tree(expr) {
            return;
        }
        if let HirExpressionKind::Conversio {
            source,
            implicit: Some(_),
            ..
        } = &mut expr.kind
        {
            let placeholder = HirExpression {
                id: source.id,
                kind: HirExpressionKind::Error,
                ty: None,
                span: source.span,
            };
            let inner = std::mem::replace(&mut **source, placeholder);
            *expr = inner;
        }
        expr.ty = Some(exact);
    }
}
