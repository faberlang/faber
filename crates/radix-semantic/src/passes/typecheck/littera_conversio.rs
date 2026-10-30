//! `littera` text conversions and the `Code` hint (DECISIONS D10.6).
//!
//! `littera` means a one-letter string, so its conversions are text
//! conversions:
//!
//! - `numerus ↦ ascii` — the digits; infallible (digits are ASCII).
//! - `numerus ↦ littera` — the one digit; failable outside 0–9.
//! - `littera ↦ numerus` — parse the digit; failable otherwise.
//! - `textus ↦ littera` — the only letter; failable unless exactly one.
//! - `littera ↦ textus` — the one-letter string; the D10.5 widening chain,
//!   left to the ordinary fallthrough.
//!
//! Code points are reached only through the `Code` hint in the conversio hint
//! slot: `littera ↦ u32 via Code` (infallible) and
//! `numerus ↦ littera via Code` (failable above U+10FFFF and on surrogates). Any
//! other pair carrying `Code` rejects with `code_hint_pair_mismatch`.
//!
//! Integer sources only: a decimal width is not a digit count and a `fractus`
//! has no digit rule here. D10.4 `octetus ≡ numerus<u8>`, so a single byte
//! converts by the digits rule, never to its character. Failable rows follow
//! D17: a `⊥` default, or `textus` propagation through the active channel.

use super::{HirExpression, NumericWidth, Primitive, Symbol, Type, TypeChecker, TypeId};

pub(super) const ISSUE_CODE_HINT_PAIR_MISMATCH: &str = "code_hint_pair_mismatch";

/// Failure behavior of one admitted D10.6 row.
enum LitteraRow {
    Infallible,
    Failable,
}

impl TypeChecker<'_> {
    /// Check a D10.6 row. Returns `None` when the pair is not a `littera`
    /// text row and carries no `Code` hint, so ordinary dispatch continues.
    pub(super) fn check_littera_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        params: &[Symbol],
        recovery: Option<&mut HirExpression>,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let source = self.resolve_inferred_type(source_ty);
        let row = if self.conversio_has_code_hint(params) {
            let Some(row) = self.littera_code_row(source, target_resolved) else {
                self.invalid_conversion(ISSUE_CODE_HINT_PAIR_MISMATCH, span);
                return Some(self.error_type);
            };
            row
        } else {
            self.littera_text_row(source, target_resolved)?
        };
        match (row, recovery) {
            (_, Some(recovery)) => self.note_conversio_recovery_checked(recovery, target_resolved),
            (LitteraRow::Failable, None) => {
                self.check_text_failure_channel(source, target_resolved, span);
            }
            (LitteraRow::Infallible, None) => {}
        }
        Some(target_resolved)
    }

    fn littera_text_row(&self, source: TypeId, target: TypeId) -> Option<LitteraRow> {
        let integer_source = self.is_integer_numerus(source);
        if integer_source && self.is_bare_ascii(target) {
            return Some(LitteraRow::Infallible);
        }
        if integer_source && self.is_littera(target) {
            return Some(LitteraRow::Failable);
        }
        if self.is_littera(source) && self.is_integer_numerus(target) {
            return Some(LitteraRow::Failable);
        }
        if self.is_littera(target)
            && matches!(self.types.get(source), Type::Primitive(Primitive::Textus))
        {
            return Some(LitteraRow::Failable);
        }
        None
    }

    fn littera_code_row(&self, source: TypeId, target: TypeId) -> Option<LitteraRow> {
        if self.is_littera(source)
            && matches!(
                self.types.get(target),
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
            )
        {
            return Some(LitteraRow::Infallible);
        }
        if self.is_integer_numerus(source) && self.is_littera(target) {
            return Some(LitteraRow::Failable);
        }
        None
    }

    fn is_integer_numerus(&self, ty: TypeId) -> bool {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Primitive(Primitive::Numerus) => true,
            Type::SizedNumeric(Primitive::Numerus, width) => !width.is_decimal(),
            _ => false,
        }
    }

    fn is_bare_ascii(&self, ty: TypeId) -> bool {
        matches!(self.types.get(ty), Type::Primitive(Primitive::Ascii))
    }

    fn conversio_has_code_hint(&self, params: &[Symbol]) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        params
            .iter()
            .any(|param| crate::numeric_width::is_code_point_hint(interner.resolve(*param)))
    }
}
