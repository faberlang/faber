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

use super::integer_range::const_int_exact;
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
}

impl StoreSite {
    pub(super) fn new(kind: HirStoreKind) -> Self {
        Self {
            kind,
            slot: None,
            inferred_from: None,
            compile_time: false,
        }
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
    pub(super) family: IntFamily,
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
            // Arithmetic over a `saturating` word fills an open slot with the
            // word, not with the exact type the range rule gave the value.
            if self.is_infer(slot_ty)
                && matches!(
                    expr.kind,
                    HirExpressionKind::Binary(..) | HirExpressionKind::Unary(..)
                )
            {
                let ty = self.check_expr(expr);
                let ty = self.saturating_inferred_slot(expr, ty, site);
                return self.unify_expected(ty, slot_ty, expr.span);
            }
            let ty = self.check_expr_with_expected(expr, Some(slot_ty));
            // A conditional, `discerne` or block that yields exact decimal
            // arithmetic rounds at the inferred `d64` slot too (n5b).
            if self.is_infer(slot_ty) && self.is_decimal_arithmetic(expr, ty) {
                return self.wrap_implicit_store(expr, ty, ty, site);
            }
            if self.is_infer(slot_ty) {
                return self.capped_inferred_slot(expr, ty, site);
            }
            return ty;
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
            if let Some(widened) = self.widen_into_inf(expr, ty, slot_ty) {
                return widened;
            }
            if let Some(target) = self
                .store_narrowing_target(ty, slot_ty)
                .or_else(|| self.capped_store_target(expr, ty, slot_ty))
                .or_else(|| self.saturating_store_target(expr, ty, slot_ty))
                .or_else(|| self.decimal_store_target(expr, ty, slot_ty))
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
            .or_else(|| self.capped_store_target(expr, ty, slot_ty))
            .or_else(|| self.saturating_store_target(expr, ty, slot_ty))
            .or_else(|| self.decimal_store_target(expr, ty, slot_ty))
        {
            Some(target) => self.wrap_implicit_store(expr, ty, target, site),
            None => self.unify_expected(ty, slot_ty, expr.span),
        }
    }

    /// The `saturating` word whose arithmetic `expr` is, if any: the first
    /// `saturating` leaf of a `+ - * / % ∧ ∨ ⊻ ⇐ ⇒` / unary `-` `¬` tree. Such an expression
    /// computes exactly, so its type is the range rule's plain integer type;
    /// the word is what a store or an inferred slot clamps it back into
    /// (d11-6 ruling 3: no per-operation clamp).
    pub(super) fn saturating_origin(&self, expr: &HirExpression) -> Option<TypeId> {
        if let Some((word, slot)) = expr.ty.and_then(|ty| self.store_int_slot(ty))
            && slot.family == IntFamily::Saturating
        {
            return Some(word);
        }
        match &expr.kind {
            HirExpressionKind::Binary(
                radix_hir::HirBinOp::Add
                | radix_hir::HirBinOp::Sub
                | radix_hir::HirBinOp::Mul
                | radix_hir::HirBinOp::Div
                | radix_hir::HirBinOp::Mod
                | radix_hir::HirBinOp::BitAnd
                | radix_hir::HirBinOp::BitOr
                | radix_hir::HirBinOp::BitXor,
                lhs,
                rhs,
            ) => self
                .saturating_origin(lhs)
                .or_else(|| self.saturating_origin(rhs)),
            // A shift count never gives the result its word.
            HirExpressionKind::Binary(
                radix_hir::HirBinOp::Shl | radix_hir::HirBinOp::Shr,
                lhs,
                _,
            ) => self.saturating_origin(lhs),
            HirExpressionKind::Unary(
                radix_hir::HirUnOp::Neg | radix_hir::HirUnOp::BitNot,
                operand,
            ) => self.saturating_origin(operand),
            _ => None,
        }
    }

    /// Both operands come from the same `saturating` word (a leaf or exact
    /// arithmetic over it), so a comparison joins them by value.
    pub(super) fn saturating_pair(&self, lhs: &HirExpression, rhs: &HirExpression) -> bool {
        matches!(
            (self.saturating_origin(lhs), self.saturating_origin(rhs)),
            (Some(a), Some(b)) if a == b
        )
    }

    /// Both operands of an arithmetic operator agree on the `saturating`
    /// family: all `saturating` operands are one word, and a `saturating`
    /// operand never meets a trapping integer variable. Reports and returns
    /// `false` otherwise.
    pub(super) fn saturating_operands_agree(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        span: radix_lexer::Span,
    ) -> bool {
        let issue = match (self.saturating_origin(lhs), self.saturating_origin(rhs)) {
            (Some(a), Some(b)) if a != b => super::ops::NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL,
            (Some(_), None) | (None, Some(_)) => super::ops::SATURATING_WORD_FAMILY_MISMATCH,
            _ => return true,
        };
        self.invalid_operand_types(issue, span);
        false
    }

    /// Whether `expr` (of type `ty`) is decimal arithmetic: its value is
    /// computed exactly (D11.7), so a slot must round and range-check it.
    fn is_decimal_arithmetic(&self, expr: &HirExpression, ty: TypeId) -> bool {
        self.decimal_slot_type(ty).is_some() && Self::carries_exact_decimal(expr)
    }

    /// Whether `expr` can evaluate to an unrounded decimal intermediate:
    /// decimal `+ - * / %` and unary `-`, or a conditional, `discerne` or
    /// block that yields one from any branch (n5b).
    fn carries_exact_decimal(expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::Binary(
                radix_hir::HirBinOp::Add
                | radix_hir::HirBinOp::Sub
                | radix_hir::HirBinOp::Mul
                | radix_hir::HirBinOp::Div
                | radix_hir::HirBinOp::Mod,
                ..,
            )
            | HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, _) => true,
            _ => radix_hir::tails::branch_tails(expr)
                .into_iter()
                .any(Self::carries_exact_decimal),
        }
    }

    /// The `d64` type of a concrete slot type (aliases and access markers
    /// peeled), or of the single decimal member of an optional or union.
    fn decimal_slot_type(&self, ty: TypeId) -> Option<TypeId> {
        let direct = |ty: TypeId| {
            let mut ty = self.resolve_inferred_type(ty);
            ty = self.types.peel_access_marker(ty).1;
            while let Type::Alias(_, inner) = self.types.get(ty) {
                ty = self.resolve_inferred_type(*inner);
            }
            matches!(
                self.types.get(ty),
                Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal()
            )
            .then_some(ty)
        };
        if let Some(found) = direct(ty) {
            return Some(found);
        }
        let resolved = self.resolve_inferred_type(ty);
        let members: Vec<TypeId> = match self.types.get(self.types.peel_access_marker(resolved).1) {
            Type::Union(members) => members.clone(),
            Type::Option(inner) => vec![*inner],
            _ => return None,
        };
        let mut numeric = members.into_iter().filter_map(direct);
        let only = numeric.next()?;
        numeric.next().is_none().then_some(only)
    }

    /// The `d64` slot a computed decimal lands in (D11.7): the slot rounds the
    /// exact value half-even to scale 8 and traps outside the carrier range.
    fn decimal_store_target(
        &self,
        expr: &HirExpression,
        ty: TypeId,
        slot_ty: TypeId,
    ) -> Option<TypeId> {
        if !self.is_decimal_arithmetic(expr, ty) {
            return None;
        }
        self.decimal_slot_type(slot_ty)
    }

    /// The `saturating` slot an exactly computed `saturating` expression lands
    /// in: its plain integer type is clamped into the slot at the store.
    fn saturating_store_target(
        &self,
        expr: &HirExpression,
        ty: TypeId,
        slot_ty: TypeId,
    ) -> Option<TypeId> {
        let (_, src) = self.store_int_slot(ty)?;
        let (dst_ty, dst) = self.store_int_target(slot_ty)?;
        (src.family == IntFamily::Plain
            && dst.family == IntFamily::Saturating
            && self.saturating_origin(expr).is_some())
        .then_some(dst_ty)
    }

    /// An inferred slot (`_`, or no annotation) initialised from `saturating`
    /// arithmetic keeps the word: the slot is `saturating<W>` and the exact
    /// value clamps at the declaration (ruling 12, "saturating keeps its
    /// width"). Any other initialiser keeps its own type.
    pub(super) fn saturating_inferred_slot(
        &mut self,
        init: &mut HirExpression,
        ty: TypeId,
        site: StoreSite,
    ) -> TypeId {
        // A decimal computed exactly lands in the inferred `d64` slot rounded
        // at the store (D11.7): the slot's type is the expression's type.
        if self.is_decimal_arithmetic(init, ty) {
            return self.wrap_implicit_store(init, ty, ty, site);
        }
        let is_arithmetic = matches!(
            init.kind,
            HirExpressionKind::Binary(..) | HirExpressionKind::Unary(..)
        );
        let plain = self
            .store_int_slot(ty)
            .is_some_and(|(_, slot)| slot.family == IntFamily::Plain);
        match self.saturating_origin(init) {
            Some(word) if is_arithmetic && plain => self.wrap_implicit_store(init, ty, word, site),
            _ => self.capped_inferred_slot(init, ty, site),
        }
    }

    /// An inferred slot takes an `i64`-typed value at the 64-bit cap as `i64`,
    /// so the declaration is the store that checks it (ruling 17b, ss6).
    fn capped_inferred_slot(
        &mut self,
        init: &mut HirExpression,
        ty: TypeId,
        site: StoreSite,
    ) -> TypeId {
        match self.capped_store_target(init, ty, ty) {
            Some(target) => self.wrap_implicit_store(init, ty, target, site),
            None => ty,
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
    ) -> Option<TypeId> {
        let (_, src) = self.store_int_slot(ty)?;
        let (dst_ty, dst) = self.store_int_target(slot_ty)?;
        let plain_i64 = |s: IntSlot| s.family == IntFamily::Plain && s.width == NumericWidth::I64;
        (plain_i64(src) && plain_i64(dst) && self.exceeds_i64(expr, ty)).then_some(dst_ty)
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
        // Deliberate: the unbounded `inf` has no range, so it is not a checked
        // integer slot. A store into it is total (nothing to fit, ruling 38);
        // a store out of it is the checked narrowing of
        // `store_narrowing_target`, which answers it before this lookup.
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
        if self.is_inf_type(ty) {
            // Ruling 38: an `inf` value into a bounded trapping slot is the
            // implicit checked narrowing (a trap, never `⇥`). An `inf` slot
            // is total (no node), and a word slot needs an authored `↦`.
            let (dst_ty, dst) = self.store_int_target(slot_ty)?;
            return (dst.family == IntFamily::Plain).then_some(dst_ty);
        }
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
        let exact = const_int_exact(expr)?;
        let (slot_int_ty, slot) = self.store_int_target(slot_ty)?;
        let big_literal =
            |e: &HirExpression| matches!(e.kind, HirExpressionKind::Literal(HirLiteral::BigInt(_)));
        let literal_shaped = big_literal(expr)
            || matches!(
                &expr.kind,
                HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) if big_literal(inner)
            );
        if literal_shaped {
            // Ruling 34: a (negated) literal beyond `u64` fits no bounded
            // slot; the literal rule reports it (`numerus_literal_out_of_range`).
            return None;
        }
        if let Some(value) = exact.to_i128()
            && slot.holds(value)
        {
            return self.fold_word_slot_constant(expr, value, slot_int_ty, slot);
        }
        if site.compile_time || slot.family == IntFamily::Plain {
            self.check_const_operand(expr);
            self.errors.push(
                SemanticError::structured(SemanticErrorKind::TypeMismatch, expr.span)
                    .with_arg("issue", STORE_CONSTANT_OUT_OF_RANGE)
                    .with_arg("value", exact.to_string())
                    .with_arg("slot", slot.spelling()),
            );
            expr.ty = Some(slot_int_ty);
            return Some(slot_int_ty);
        }
        // `←` into a `wrapping` or `saturating` slot: the constant follows the
        // slot's policy. A value outside every 64-bit carrier has nothing to
        // convert from, so the policy is applied here, exactly (ruling 21).
        if !exact
            .to_i128()
            .is_some_and(|v| radix_types::numeric_range::fits_container((v, v)))
        {
            let applied = Self::apply_slot_policy(&exact, slot);
            return self.rewrite_as_slot_literal(expr, applied, slot_int_ty);
        }
        // It is checked as a plain `int` and wrapped.
        let ty = self.check_const_operand(expr);
        Some(self.wrap_implicit_store(expr, ty, slot_int_ty, site))
    }

    /// The exact value a `wrapping` slot reduces to, or a `saturating` slot
    /// clamps to, for a constant of any size.
    fn apply_slot_policy(value: &radix_types::Magnus, slot: IntSlot) -> i128 {
        use radix_types::Magnus;
        let (lo, hi) = slot.range;
        let (lo_m, hi_m) = (Magnus::from_i128(lo), Magnus::from_i128(hi));
        if slot.family == IntFamily::Saturating {
            let clamped = value.clone().clamp(lo_m, hi_m);
            return clamped.to_i128().unwrap_or(lo);
        }
        let span = Magnus::from_i128(hi - lo + 1);
        let offset = (value - &lo_m).mod_floor(&span).unwrap_or_default();
        lo + offset.to_i128().unwrap_or(0)
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
