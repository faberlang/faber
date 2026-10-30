//! Single-scalar `textus`/`ascii` proofs for ordering (`<`/`≤`/…) and interval bounds.
//!
//! Seam B-lite: literals, unit `sectio`, unary-`textus` predicate params, and
//! chorda interval bound parameters and predicate helpers under both the
//! canonical Latin names (`retenta` / `expurgata`) and the English canonicals
//! (`keep_between` / `drop_between`, `keep_if` / `drop_if`).
//!
//! After BB-2, `'x'` is `BoundedAscii{1}`. Relational compare accepts that
//! char pair (and bare `ascii` / `ascii<1>`) by code-point order. Mixed
//! char/string still uses the single-scalar convention; full cords stay
//! rejected.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::{HirBinOp, HirExpression, HirExpressionKind, HirLiteral};

const SCALAR_INTERVAL_METHODS: &[&str] = &[
    "retenta",
    "expurgata",
    "retenta_iudicio",
    "expurgata_iudicio",
    "keep_if",
    "keep_between",
    "drop_if",
    "drop_between",
];
const INTERVAL_BOUND_PARAM_INDICES: [usize; 2] = [1, 2];
const ISSUE_INTERVAL_BOUND_SINGLE_SCALAR: &str = "interval_bound_single_scalar";
const ISSUE_SCALAR_TEXT_OPERANDS_REQUIRED: &str = "scalar_text_operands_required";

impl TypeChecker<'_> {
    pub fn is_interval_bound_helper_name(&self, name: &str) -> bool {
        SCALAR_INTERVAL_METHODS.contains(&name)
    }

    pub fn marks_scalar_predicate_context(&self, method_name: Symbol) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        SCALAR_INTERVAL_METHODS.contains(&interner.resolve(method_name))
    }

    pub fn set_chorda_scalar_closure_context(&mut self, method: Symbol) {
        self.mark_chorda_scalar_closure_param = self.marks_scalar_predicate_context(method);
    }

    pub fn interval_bound_param_is_scalar_text(
        &self,
        func_name: &str,
        param_idx: usize,
        param_ty: TypeId,
    ) -> bool {
        self.is_interval_bound_helper_name(func_name)
            && INTERVAL_BOUND_PARAM_INDICES.contains(&param_idx)
            && self.is_textus(param_ty)
    }

    pub fn function_display_name(&self, def_id: radix_hir::DefId) -> Option<String> {
        let symbol = self.resolver.get_symbol(def_id)?;
        self.interner
            .map(|interner| interner.resolve(symbol.name).to_owned())
    }

    pub fn validate_interval_bound_call_args(
        &mut self,
        method_name: radix_lexer::Symbol,
        sig: &crate::FuncSig,
        args: &[radix_hir::HirCallArg],
    ) {
        let Some(interner) = self.interner else {
            return;
        };
        if !self.is_interval_bound_helper_name(interner.resolve(method_name)) {
            return;
        }
        let Some(second_param) = sig.params.get(1) else {
            return;
        };
        if !self.is_textus(second_param.ty) {
            return;
        }
        for idx in INTERVAL_BOUND_PARAM_INDICES {
            let Some(arg) = args.get(idx) else {
                continue;
            };
            if !self.is_provably_single_scalar_text(&arg.expr) {
                self.invalid_operand_types(ISSUE_INTERVAL_BOUND_SINGLE_SCALAR, arg.expr.span);
            }
        }
    }

    pub fn is_provably_single_scalar_text(&mut self, expr: &HirExpression) -> bool {
        if expr
            .ty
            .is_some_and(|ty| self.resolve_inferred_type(ty) == self.error_type)
        {
            return false;
        }
        match &expr.kind {
            HirExpressionKind::Literal(lit) => self.literal_is_single_scalar_text(lit),
            HirExpressionKind::Path(def_id) => self
                .lookup_binding(*def_id)
                .is_some_and(|info| info.scalar_text),
            HirExpressionKind::MethodCall(_, name, _, args, _) if args.len() == 2 => {
                self.interner
                    .is_some_and(|interner| interner.resolve(*name) == "sectio")
                    && Self::is_unit_sectio_span(&args[0].expr, &args[1].expr)
            }
            _ => false,
        }
    }

    fn literal_is_single_scalar_text(&mut self, lit: &HirLiteral) -> bool {
        match lit {
            HirLiteral::Ascii(sym) => self.ascii_literal_payload_is_single_scalar(*sym),
            HirLiteral::String(sym) => self.textus_literal_payload_is_single_scalar(*sym),
            _ => false,
        }
    }

    fn ascii_literal_payload_is_single_scalar(&mut self, sym: Symbol) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        let raw = interner.resolve(sym);
        radix_lexer::ascii_literal::decode_ascii_literal(raw)
            .is_ok_and(|decoded| decoded.chars().count() == 1)
    }

    fn textus_literal_payload_is_single_scalar(&mut self, sym: Symbol) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        radix_lexer::textus_literal::decode_textus_literal(interner.resolve(sym))
            .chars()
            .count()
            == 1
    }

    fn is_unit_sectio_span(start: &HirExpression, end: &HirExpression) -> bool {
        match (&start.kind, &end.kind) {
            (
                HirExpressionKind::Path(start_id),
                HirExpressionKind::Binary(HirBinOp::Add, lhs, rhs),
            ) if Self::paths_same(*start_id, lhs) => {
                matches!(&rhs.kind, HirExpressionKind::Literal(HirLiteral::Int(1)))
            }
            (
                HirExpressionKind::Literal(HirLiteral::Int(a)),
                HirExpressionKind::Literal(HirLiteral::Int(b)),
            ) => a.checked_add(1) == Some(*b),
            _ => false,
        }
    }

    fn paths_same(left: DefId, rhs: &HirExpression) -> bool {
        matches!(&rhs.kind, HirExpressionKind::Path(right) if *right == left)
    }

    pub fn is_text_family(&self, ty: TypeId) -> bool {
        self.is_textus(ty) || self.is_ascii(ty) || self.is_bounded_ascii_one(ty)
    }

    /// Char-order types: `BoundedAscii{1}`, `ascii<1>`, and bare `ascii`.
    ///
    /// WHY: BB-2 types `'x'` as `BoundedAscii{1}`. Relational compare is
    /// code-point order for those pairs; multi-char ascii literals stay
    /// rejected by [`Self::char_order_operand_ok`].
    pub fn is_char_order_type(&self, ty: TypeId) -> bool {
        self.is_bounded_ascii_one(ty) || self.is_ascii(ty) || self.is_littera(ty)
    }

    /// Check a comparison's operands so a one-letter literal facing a
    /// `littera` types as that scalar (D10.5 `ascii<1> → littera`): `c ≡ 'o'`
    /// and `'a' ≤ c` compare two `littera`s, and backends emit a character
    /// literal. With `decimal_constants` (ordering; F9 ruling 30) an untyped
    /// constant beside a `d64` operand joins the decimal family; `≡` stays
    /// exact-type and passes `false`. Any other operand pair is checked
    /// unchanged.
    pub fn check_littera_operands(
        &mut self,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
        decimal_constants: bool,
    ) -> (TypeId, TypeId) {
        let lhs_ty = self.check_comparison_operand(lhs);
        let rhs_ty = if let Some(ty) = self.bits_peer_literal(rhs, lhs_ty) {
            ty
        } else if self.is_littera(lhs_ty) && is_ascii_literal(rhs) {
            self.check_expr_with_expected(rhs, Some(lhs_ty))
        } else if let Some(ty) = decimal_constants
            .then(|| self.check_constant_beside_decimal(rhs, lhs_ty))
            .flatten()
        {
            ty
        } else {
            self.check_comparison_operand(rhs)
        };
        if let Some(ty) = self.bits_peer_literal(lhs, rhs_ty) {
            return (ty, rhs_ty);
        }
        let lhs_ty = decimal_constants
            .then(|| self.check_constant_beside_decimal(lhs, rhs_ty))
            .flatten()
            .unwrap_or(lhs_ty);
        if self.is_littera(rhs_ty) && is_ascii_literal(lhs) && self.is_bounded_ascii_one(lhs_ty) {
            let lhs_ty = self.check_expr_with_expected(lhs, Some(rhs_ty));
            return (lhs_ty, rhs_ty);
        }
        (lhs_ty, rhs_ty)
    }

    fn is_bounded_ascii_one(&self, ty: TypeId) -> bool {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::BoundedAscii { capacity } => {
                matches!(self.types.get_index(*capacity), IndexExpr::Literal(1))
            }
            _ => false,
        }
    }

    fn char_order_operand_ok(&mut self, expr: &HirExpression, ty: TypeId) -> bool {
        if self.is_bounded_ascii_one(ty) || self.is_littera(ty) {
            return true;
        }
        if self.is_ascii(ty) {
            // Multi-char ascii literals (`'ab'`) stay rejected.
            if matches!(&expr.kind, HirExpressionKind::Literal(_)) {
                return self.is_provably_single_scalar_text(expr);
            }
            return true;
        }
        false
    }

    pub fn scalar_ordered_text_bin(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
    ) -> bool {
        if self.is_char_order_type(lhs_ty)
            && self.is_char_order_type(rhs_ty)
            && self.char_order_operand_ok(lhs, lhs_ty)
            && self.char_order_operand_ok(rhs, rhs_ty)
        {
            return true;
        }
        if !self.is_text_family(lhs_ty) || !self.is_text_family(rhs_ty) {
            return false;
        }
        // A whole `textus` cord and an `ascii`/`ascii<1>` char never order
        // against each other: the runner has no such comparison row, so the
        // mixed pair falls through to the numeric-operand rejection instead.
        if self.is_textus(lhs_ty) != self.is_textus(rhs_ty) {
            return false;
        }
        if !self.is_provably_single_scalar_text(lhs) || !self.is_provably_single_scalar_text(rhs) {
            return false;
        }
        if self.is_infer(self.resolve_inferred_type(lhs_ty)) {
            let textus = self.textus_type();
            self.unify_assign_issue(
                lhs_ty,
                textus,
                lhs.span,
                ISSUE_SCALAR_TEXT_OPERANDS_REQUIRED,
            );
        }
        if self.is_infer(self.resolve_inferred_type(rhs_ty)) {
            let textus = self.textus_type();
            self.unify_assign_issue(
                rhs_ty,
                textus,
                rhs.span,
                ISSUE_SCALAR_TEXT_OPERANDS_REQUIRED,
            );
        }
        if !self.is_text_family(lhs_ty) || !self.is_text_family(rhs_ty) {
            self.invalid_operand_types(ISSUE_SCALAR_TEXT_OPERANDS_REQUIRED, span);
        }
        true
    }
}

fn is_ascii_literal(expr: &HirExpression) -> bool {
    matches!(expr.kind, HirExpressionKind::Literal(HirLiteral::Ascii(_)))
}
