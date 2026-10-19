//! Checked stores (d11-6 rulings 2, 5, 17, 21).
//!
//! Every position where a value lands in a typed slot applies the slot's
//! policy. Integer expressions are typed by the range rule, so a store into a
//! type that contains the expression's type never fails and needs no node. A
//! store into a narrower integer slot of the same policy family gets an
//! **implicit store node** (`Conversio` with `implicit: Some(site)`), carrying
//! the slot as its target type: a trapping slot traps, a `wrapping` slot
//! reduces, a `saturating` slot clamps. Crossing policy families still needs
//! an authored `↦`. An indexed container (`tensor`, `vector`, `matrix`,
//! `sparsa`) takes its element's scalar store rule, wrapped once against the
//! slot container.
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
const OPTIONAL_INTO_PLAIN_CELL: &str = "optional_into_plain_cell";

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
    /// The receiving slot is a `sponte` parameter or field: it is an optional
    /// target, so an optional value stored into it is not an unwrap.
    pub(super) optional_slot: bool,
}

impl StoreSite {
    pub(super) fn new(kind: HirStoreKind) -> Self {
        Self {
            kind,
            slot: None,
            inferred_from: None,
            compile_time: false,
            optional_slot: false,
        }
    }

    pub(super) fn named(mut self, slot: Symbol) -> Self {
        self.slot = Some(slot);
        self
    }

    /// Mark a `sponte` parameter or field slot, which holds an absent value.
    pub(super) fn optional_slot(mut self, optional: bool) -> Self {
        self.optional_slot = optional;
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
            IntFamily::Policy(policy) => format!("{}<{width}>", policy.word()),
        }
    }
}

impl TypeChecker<'_> {
    /// Reject an optional value (`T ∪ none`) stored into a declared cell that
    /// cannot hold `none`. The assignability relation unwraps
    /// `Option(T)` into `T` silently; a store into a plain cell is where that
    /// unwrap would trap far from the source of the optional, so it is a named
    /// error here. Returns `true` when it reported, so the caller skips the
    /// ordinary assignment unification. A flow-narrowed read, a `vel` result
    /// and an `x!` unwrap already have the payload type and never reach this.
    pub(super) fn reject_optional_into_plain_cell(
        &mut self,
        value_ty: TypeId,
        slot_ty: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        let peel = |checker: &Self, ty: TypeId| {
            let ty = checker.peel_nominal_aliases(ty);
            let ty = checker.types.peel_access_marker(ty).1;
            checker.peel_nominal_aliases(ty)
        };
        let value = peel(self, value_ty);
        let slot = peel(self, slot_ty);
        let Type::Option(inner) = self.types.get(value) else {
            return false;
        };
        let inner = *inner;
        let nihil = self.types.primitive(Primitive::Nihil);
        if self.is_infer(slot)
            || self.infer_union_var_of(slot).is_some()
            || self.types.is_assignable(nihil, slot)
            || !self.types.is_assignable(inner, slot)
        {
            return false;
        }
        let found = self.format_type_for_diagnostic(value_ty);
        let expected = self.format_type_for_diagnostic(slot_ty);
        self.error_issue_with_args(
            SemanticErrorKind::TypeMismatch,
            OPTIONAL_INTO_PLAIN_CELL,
            span,
            &[("found", found), ("expected", expected)],
        );
        true
    }

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
                let ty = self.word_inferred_slot(expr, ty, site);
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
            // Need: a thrown, yielded or element-stored optional
            // does not unwrap silently into a plain slot.
            if self.reject_optional_into_plain_cell(ty, slot_ty, expr.span) {
                return slot_ty;
            }
            if let Some(widened) = self.widen_into_inf(expr, ty, slot_ty) {
                return widened;
            }
            if let Some(target) = self.implicit_store_target(expr, ty, slot_ty) {
                return self.wrap_implicit_store(expr, ty, target, site);
            }
            if let Some(landed) = self.import_store_door(expr, ty, slot_ty, site) {
                return landed;
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
        match self.implicit_store_target(expr, ty, slot_ty) {
            Some(target) => self.wrap_implicit_store(expr, ty, target, site),
            None => {
                if let Some(landed) = self.import_store_door(expr, ty, slot_ty, site) {
                    return landed;
                }
                self.unify_expected(ty, slot_ty, expr.span)
            }
        }
    }

    /// The scalar door for target-native imports: an `ignotum`
    /// value observed at a scalar cell converts with implicit syntax but forced
    /// semantics, so the store wraps an implicit node carrying the coercion the
    /// emitter lowers for the target. Only values arriving through a trusted
    /// import are admitted, and this unit opens only integer cells: a float,
    /// text or bool cell is a later unit, an aggregate slot stays refused by the
    /// ordinary unification.
    fn import_store_door(
        &mut self,
        expr: &mut HirExpression,
        ty: TypeId,
        slot_ty: TypeId,
        site: StoreSite,
    ) -> Option<TypeId> {
        if self.trusted_imports.is_empty() || !self.is_declared_slot(slot_ty) {
            return None;
        }
        let peel = |checker: &Self, ty: TypeId| {
            let ty = checker.peel_nominal_aliases(ty);
            let ty = checker.types.peel_access_marker(ty).1;
            checker.peel_nominal_aliases(ty)
        };
        let value = peel(self, ty);
        if !matches!(self.types.get(value), Type::Primitive(Primitive::Ignotum)) {
            return None;
        }
        let (int_ty, _) = self.store_int_slot(peel(self, slot_ty))?;
        Some(self.wrap_implicit_store(expr, ty, int_ty, site))
    }

    /// The implicit store node a value of type `ty` needs to land in `slot_ty`,
    /// trying each store rule in order: an integer narrowing, the 64-bit cap, a
    /// `wrapping`/`saturating` word, a `trapping` float cell, or a computed
    /// decimal. `None` keeps the ordinary assignment unification.
    fn implicit_store_target(
        &self,
        expr: &HirExpression,
        ty: TypeId,
        slot_ty: TypeId,
    ) -> Option<TypeId> {
        self.store_narrowing_target(ty, slot_ty)
            .or_else(|| self.capped_store_target(expr, ty, slot_ty))
            .or_else(|| self.word_store_target(expr, ty, slot_ty))
            .or_else(|| self.trapping_float_store_target(expr, ty, slot_ty))
            .or_else(|| self.decimal_store_target(expr, ty, slot_ty))
    }

    /// The `wrapping` or `saturating` word whose arithmetic `expr` is, if any:
    /// the first word leaf of a `+ - * / % ∧ ∨ ⊻ ⇐ ⇒` / unary `-` `¬` tree. Such
    /// an expression computes exactly, so its type is the range rule's plain
    /// integer type; the word is what a store or an inferred slot reduces or
    /// clamps it back into (d11-6 ruling 3: no per-operation reduction).
    pub(super) fn word_origin(&self, expr: &HirExpression) -> Option<TypeId> {
        if let Some((word, slot)) = expr.ty.and_then(|ty| self.store_int_slot(ty))
            && matches!(slot.family, IntFamily::Saturating | IntFamily::Modular)
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
            ) => self.word_origin(lhs).or_else(|| self.word_origin(rhs)),
            // A shift count never gives the result its word.
            HirExpressionKind::Binary(
                radix_hir::HirBinOp::Shl | radix_hir::HirBinOp::Shr,
                lhs,
                _,
            ) => self.word_origin(lhs),
            HirExpressionKind::Unary(
                radix_hir::HirUnOp::Neg | radix_hir::HirUnOp::BitNot,
                operand,
            ) => self.word_origin(operand),
            _ => None,
        }
    }

    /// Both operands come from the same word (a leaf or exact arithmetic over
    /// it), so a comparison joins them by value.
    pub(super) fn word_pair(&self, lhs: &HirExpression, rhs: &HirExpression) -> bool {
        matches!(
            (self.word_origin(lhs), self.word_origin(rhs)),
            (Some(a), Some(b)) if a == b
        )
    }

    /// Both operands of an arithmetic operator agree on the word family: all
    /// word operands are one word, and a word operand never meets a trapping
    /// integer variable. Reports and returns `false` otherwise.
    pub(super) fn word_operands_agree(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        span: radix_lexer::Span,
    ) -> bool {
        let family = |checker: &Self, expr: &HirExpression| {
            checker
                .word_origin(expr)
                .and_then(|word| checker.store_int_slot(word))
                .map(|(word, slot)| (word, slot.family))
        };
        let (l, r) = (family(self, lhs), family(self, rhs));
        let issue = match (l, r) {
            (None, None) => return true,
            (Some((a, fa)), Some((b, fb))) if fa == fb => {
                if a == b {
                    return true;
                }
                super::ops::NUMERIC_ARITHMETIC_OPERANDS_IDENTICAL
            }
            _ if [l, r]
                .into_iter()
                .flatten()
                .any(|(_, f)| f == IntFamily::Saturating) =>
            {
                super::ops::SATURATING_WORD_FAMILY_MISMATCH
            }
            _ => super::ops::MODULAR_WORD_FAMILY_MISMATCH,
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

    /// Mark the ring operators of an expression stored into a `wrapping<W>`
    /// slot: `+ - * ⇐ ∧ ∨ ⊻`, unary `- ¬`, and the operators reached through
    /// them. Reducing modulo 2^W once at the store equals reducing every step
    /// for these, so such a tree may keep its per-operation word arithmetic;
    /// the left operand of `⇒`, `/` and `%` is observable and stays exact
    /// (numeric-model §8). A shift count joins the ring so it keeps its word.
    pub(super) fn mark_word_ring(&mut self, expr: &HirExpression, slot_ty: TypeId) {
        let Some((word, slot)) = self.store_int_target(slot_ty) else {
            return;
        };
        if slot.family == IntFamily::Modular {
            self.mark_word_ring_node(expr, word);
        }
    }

    fn mark_word_ring_node(&mut self, expr: &HirExpression, word: TypeId) {
        use radix_hir::{HirBinOp, HirUnOp};
        match &expr.kind {
            HirExpressionKind::Binary(op, lhs, rhs) => {
                self.word_ring.insert(lhs.id, word);
                match op {
                    HirBinOp::Add
                    | HirBinOp::Sub
                    | HirBinOp::Mul
                    | HirBinOp::BitAnd
                    | HirBinOp::BitOr
                    | HirBinOp::BitXor
                    | HirBinOp::Shl => {
                        self.mark_word_ring_node(lhs, word);
                        self.mark_word_ring_node(rhs, word);
                    }
                    HirBinOp::Shr => self.mark_word_ring_node(rhs, word),
                    _ => {}
                }
            }
            HirExpressionKind::Unary(HirUnOp::Neg | HirUnOp::BitNot, operand) => {
                self.word_ring.insert(operand.id, word);
                self.mark_word_ring_node(operand, word);
            }
            _ if super::integer_range::is_const_int(expr) => {
                self.word_ring.insert(expr.id, word);
            }
            _ => {}
        }
    }

    /// Whether `expr`, a written integer constant, is an operand of a ring
    /// operator of a `wrapping<W>` store beside a `peer` of that word and fits
    /// it: it then takes the word. A constant that does not fit stays exact.
    pub(super) fn constant_joins_word_ring(&self, expr: &HirExpression, peer: TypeId) -> bool {
        let Some(&word) = self.word_ring.get(&expr.id) else {
            return false;
        };
        self.types.equals(self.resolve_inferred_type(peer), word)
            && super::integer_range::const_int_value(expr).is_some_and(|value| {
                self.store_int_slot(word)
                    .is_some_and(|(_, slot)| slot.holds(value))
            })
    }

    /// Whether the operator whose first operand is `first` is a ring operator
    /// of a `wrapping<W>` store and all its operands are that word: it is then
    /// typed in the word instead of exactly.
    pub(super) fn in_word_ring(&self, first: &HirExpression, operand_tys: &[TypeId]) -> bool {
        let Some(&word) = self.word_ring.get(&first.id) else {
            return false;
        };
        operand_tys
            .iter()
            .all(|&ty| self.types.equals(self.resolve_inferred_type(ty), word))
    }

    /// The `wrapping` or `saturating` slot an exactly computed word expression
    /// lands in: its plain integer type is reduced or clamped into the slot at
    /// the store.
    fn word_store_target(
        &self,
        expr: &HirExpression,
        ty: TypeId,
        slot_ty: TypeId,
    ) -> Option<TypeId> {
        let (_, src) = self.store_int_slot(ty)?;
        let (dst_ty, dst) = self.store_int_target(slot_ty)?;
        let origin_family = self
            .word_origin(expr)
            .and_then(|word| self.store_int_slot(word))
            .map(|(_, slot)| slot.family)?;
        (src.family == IntFamily::Plain
            && matches!(dst.family, IntFamily::Saturating | IntFamily::Modular)
            && dst.family == origin_family)
            .then_some(dst_ty)
    }

    /// An inferred slot (`_`, or no annotation) initialised from `saturating`
    /// arithmetic keeps the word: the slot is `saturating<W>` and the exact
    /// value clamps at the declaration (ruling 12, "saturating keeps its
    /// width"). Any other initialiser keeps its own type.
    pub(super) fn word_inferred_slot(
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
        match self.word_origin(init) {
            Some(word) if is_arithmetic && plain => self.wrap_implicit_store(init, ty, word, site),
            _ => match self.trapping_float_origin(init) {
                // Arithmetic over a `trapping` float is a bare float until it
                // is stored; an inferred slot keeps the cell, so the
                // declaration is the store that checks it (N7e D1, D4).
                Some(cell) if is_arithmetic && self.is_plain_float(ty) => {
                    self.wrap_implicit_store(init, ty, cell, site)
                }
                _ => self.capped_inferred_slot(init, ty, site),
            },
        }
    }

    /// The `trapping<W>` float cell whose arithmetic `expr` is, if any: the
    /// first `trapping` operand of a `+ - * / % ÷` / unary `-` tree. Such an
    /// expression is a number, not a slot, so its type is the bare float `W`
    /// and its value may be non-finite; the cell is what a store checks it
    /// against (N7e D1, D4: no operation traps).
    pub(super) fn trapping_float_origin(&self, expr: &HirExpression) -> Option<TypeId> {
        if let Some(cell) = expr.ty.and_then(|ty| self.trapping_cell(ty)) {
            return Some(cell);
        }
        match &expr.kind {
            HirExpressionKind::Binary(
                radix_hir::HirBinOp::Add
                | radix_hir::HirBinOp::Sub
                | radix_hir::HirBinOp::Mul
                | radix_hir::HirBinOp::Div
                | radix_hir::HirBinOp::Mod
                | radix_hir::HirBinOp::TrueDiv,
                lhs,
                rhs,
            ) => self
                .trapping_float_origin(lhs)
                .or_else(|| self.trapping_float_origin(rhs)),
            HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, operand) => {
                self.trapping_float_origin(operand)
            }
            _ => None,
        }
    }

    /// The `trapping<W>` cell a type is, aliases and access markers peeled.
    fn trapping_cell(&self, ty: TypeId) -> Option<TypeId> {
        let mut ty = self.resolve_inferred_type(ty);
        ty = self.types.peel_access_marker(ty).1;
        while let Type::Alias(_, inner) = self.types.get(ty) {
            ty = self.resolve_inferred_type(*inner);
        }
        matches!(
            self.types.get(ty),
            Type::PolicyFloat(radix_types::FloatPolicy::Trapping, _)
        )
        .then_some(ty)
    }

    /// Whether `ty` is a bare float (no policy cell): the type of arithmetic
    /// over a `trapping` operand.
    fn is_plain_float(&self, ty: TypeId) -> bool {
        self.numeric_shape(self.resolve_inferred_type(ty))
            .is_some_and(|shape| {
                shape.prim == Primitive::Fractus && shape.family == IntFamily::Plain
            })
    }

    /// The `trapping<W>` slot a store lands in: the slot itself, or the single
    /// numeric member of an optional or union (as for an integer slot).
    fn trapping_float_slot(&self, slot_ty: TypeId) -> Option<TypeId> {
        if let Some(cell) = self.trapping_cell(slot_ty) {
            return Some(cell);
        }
        let resolved = self.resolve_inferred_type(slot_ty);
        let members: Vec<TypeId> = match self.types.get(self.types.peel_access_marker(resolved).1) {
            Type::Union(members) => members.clone(),
            Type::Option(inner) => vec![*inner],
            _ => return None,
        };
        let mut numeric = members.into_iter().filter(|member| {
            self.numeric_shape(self.resolve_inferred_type(*member))
                .is_some()
        });
        let only = numeric.next()?;
        if numeric.next().is_some() {
            return None;
        }
        self.trapping_cell(only)
    }

    /// The `trapping<W>` slot a computed float lands in: arithmetic over
    /// `trapping<W>` operands is a bare `W`, and the slot checks the value is
    /// finite where it is stored (N7e D1). A `trapping<W>` leaf stored into
    /// its own cell is finite by construction and needs no node, a bare float
    /// or another cell never lands in a `trapping` slot without an authored
    /// `↦`, and a `saturating` slot never gets a node.
    fn trapping_float_store_target(
        &self,
        expr: &HirExpression,
        ty: TypeId,
        slot_ty: TypeId,
    ) -> Option<TypeId> {
        if !self.is_plain_float(ty) {
            return None;
        }
        let target = self.trapping_float_slot(slot_ty)?;
        // Arithmetic over written constants alone (`-(1.5 + 2.5)`) is typed
        // at the slot's width and has no cell of its own: the slot is its
        // cell, and the store checks the folded value.
        let origin = self.trapping_float_origin(expr).or_else(|| {
            (Self::is_untyped_float_constant(expr)
                && !super::ops::is_numeric_literal(expr)
                && self
                    .numeric_shape(self.resolve_inferred_type(ty))
                    .map(|s| s.width)
                    == self.numeric_shape(target).map(|s| s.width))
            .then_some(target)
        })?;
        self.types.equals(origin, target).then_some(target)
    }

    /// The expected type handed to the operand of a unary `-` whose own
    /// expected type is a `trapping<W>` cell. A written constant keeps the
    /// cell (it adopts it, ruling 20); constant arithmetic is hinted the bare
    /// `W` it will have; any other operand is a value of its own type, so the
    /// cell is no hint for it (N7e D4).
    pub(super) fn unary_float_cell_expected(
        &mut self,
        operand: &HirExpression,
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        let Some(cell) = expected.and_then(|ty| self.trapping_cell(ty)) else {
            return expected;
        };
        if super::ops::is_numeric_literal(operand) {
            return expected;
        }
        if !Self::is_untyped_float_constant(operand) {
            return None;
        }
        let Type::PolicyFloat(_, width) = *self.types.get(cell) else {
            return expected;
        };
        Some(self.bare_float_type(width))
    }

    /// The bare float type of a width.
    pub(super) fn bare_float_type(&mut self, width: NumericWidth) -> TypeId {
        if width == NumericWidth::F64 {
            self.fractus_type()
        } else {
            self.types.sized_numeric(Primitive::Fractus, width)
        }
    }

    /// Both operands of a float operator agree on the `trapping` cell: an
    /// operand that is `trapping` arithmetic (a bare float of the cell's
    /// width) never meets a bare float variable or another cell. An untyped
    /// constant joins either side. Reports and returns `false` otherwise.
    pub(super) fn trapping_float_operands_agree(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        span: radix_lexer::Span,
    ) -> bool {
        let bare = |checker: &Self, expr: &HirExpression| {
            expr.ty.is_some_and(|ty| checker.is_plain_float(ty))
                && !Self::is_untyped_float_constant(expr)
        };
        let agree = match (
            self.trapping_float_origin(lhs),
            self.trapping_float_origin(rhs),
        ) {
            (Some(a), Some(b)) => self.types.equals(a, b),
            (Some(_), None) => !bare(self, rhs),
            (None, Some(_)) => !bare(self, lhs),
            (None, None) => true,
        };
        if !agree {
            self.invalid_operand_types(super::ops::POLICY_FLOAT_FAMILY_MISMATCH, span);
        }
        agree
    }

    /// The type of arithmetic whose operands both come from one `trapping<W>`
    /// cell (agreement is checked by [`Self::trapping_float_operands_agree`]):
    /// the bare float `W`, whichever operand is the leaf cell and whichever is
    /// already bare arithmetic over it. `None` when an operand has no cell.
    pub(super) fn trapping_float_join(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
    ) -> Option<TypeId> {
        let cell = self.trapping_float_origin(lhs)?;
        self.trapping_float_origin(rhs)?;
        let Type::PolicyFloat(_, width) = *self.types.get(cell) else {
            return None;
        };
        Some(self.bare_float_type(width))
    }

    /// A written numeric constant, or arithmetic over written constants: it
    /// takes the type of whatever it stands beside (ruling 20).
    fn is_untyped_float_constant(expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::Binary(
                radix_hir::HirBinOp::Add
                | radix_hir::HirBinOp::Sub
                | radix_hir::HirBinOp::Mul
                | radix_hir::HirBinOp::Div
                | radix_hir::HirBinOp::Mod
                | radix_hir::HirBinOp::TrueDiv,
                lhs,
                rhs,
            ) => Self::is_untyped_float_constant(lhs) && Self::is_untyped_float_constant(rhs),
            HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, operand) => {
                Self::is_untyped_float_constant(operand)
            }
            _ => super::ops::is_numeric_literal(expr),
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
        if let Some(container) = self.indexed_container_narrowing_target(ty, slot_ty) {
            return Some(container);
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

    /// The indexed-container twin of the scalar narrowing rule: a value of the
    /// same container kind and shape takes its element's scalar store rule,
    /// with the whole value wrapped once against the slot container. Different
    /// kinds or shapes, float elements, and widening or equal elements keep the
    /// ordinary assignment unification (`None`).
    fn indexed_container_narrowing_target(&self, ty: TypeId, slot_ty: TypeId) -> Option<TypeId> {
        let ty = self
            .types
            .peel_access_marker(self.resolve_inferred_type(ty))
            .1;
        let slot_ty = self
            .types
            .peel_access_marker(self.resolve_inferred_type(slot_ty))
            .1;
        let (from_elem, to_elem) = match (self.types.get(ty), self.types.get(slot_ty)) {
            (Type::Tensor(from, from_shape), Type::Tensor(to, to_shape))
            | (Type::Vector(from, from_shape), Type::Vector(to, to_shape))
            | (Type::Matrix(from, from_shape), Type::Matrix(to, to_shape))
            | (Type::Sparsa(from, from_shape), Type::Sparsa(to, to_shape))
                if self.types.index_equals(*from_shape, *to_shape) =>
            {
                (*from, *to)
            }
            _ => return None,
        };
        if self.types.is_assignable(ty, slot_ty) {
            return None;
        }
        self.store_narrowing_target(from_elem, to_elem)?;
        Some(slot_ty)
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
