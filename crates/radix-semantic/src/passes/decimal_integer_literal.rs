//! Integer constants that typecheck into a decimal slot become decimal text.
//!
//! F9 ruling 30: an untyped integer constant beside a `d64` operand or in a
//! `d64` slot is a decimal with no fraction. Typecheck gives it the decimal
//! type after the range check; this pass then respells the literal as the
//! decimal text of its value, so lowering and every backend see the same
//! `FloatText` literal they already scale for `4.2`, and never an integer
//! literal carrying a decimal type. It runs after typecheck because only
//! analysis owns a mutable interner.

use radix_hir::visit::{HirVisitorMut, walk_expr_mut};
use radix_hir::{HirExpression, HirExpressionKind, HirLiteral, HirModule};
use radix_lexer::Interner;
use radix_types::{Primitive, Type, TypeId, TypeTable};

/// Respell every decimal-typed integer literal in `hir` as decimal text.
pub fn respell(hir: &mut HirModule, types: &TypeTable, interner: &mut Interner) {
    let mut pass = Respell { types, interner };
    pass.visit_program_mut(hir);
}

struct Respell<'a> {
    types: &'a TypeTable,
    interner: &'a mut Interner,
}

impl Respell<'_> {
    fn is_decimal(&self, mut ty: TypeId) -> bool {
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::SizedNumeric(Primitive::Numerus, width) => return width.is_decimal(),
                _ => return false,
            }
        }
    }
}

impl HirVisitorMut for Respell<'_> {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        walk_expr_mut(self, expr);
        let HirExpressionKind::Literal(HirLiteral::Int(value)) = &expr.kind else {
            return;
        };
        if !expr.ty.is_some_and(|ty| self.is_decimal(ty)) {
            return;
        }
        expr.kind = HirExpressionKind::Literal(HirLiteral::FloatText {
            value: radix_types::Magnus::from_u64(*value).to_f64(),
            text: self.interner.intern(&value.to_string()),
        });
    }
}
