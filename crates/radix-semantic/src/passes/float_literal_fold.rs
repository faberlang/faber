//! Literal-only float expressions fold to one literal, rounded once.
//!
//! F9 ruling 31 (inf-U7): an expression made only of literals, `+ - * / % ÷`,
//! unary `-` and parentheses is an exact rational, whatever the literals'
//! spelling. It is rounded to nearest-even once, at the float width of the
//! slot it lands in, never per operation: `0.1 + 0.2` is 0.3, and `0.1 + 0.2`
//! into an `f32` slot is the `f32` nearest the exact 3/10 (not the `f32`
//! nearest an `f64` sum). Typed operands are not constants and stay IEEE per
//! operation; a method call on a literal is not a constant expression, so
//! `(0.1 + 0.2).power(2)` folds the parenthesised receiver and calls at run
//! time.
//!
//! The fold rewrites the expression to one `Float` literal (the variant static
//! folding also produces; only decimal widths need literal text), so the runner
//! and every backend read the same number and none relies on its own constant
//! folding (Go's untyped constants are not the authority). It runs after
//! typecheck because only finished types name the slot width, and it does not
//! run for source-preserving targets (Faber re-emit, FHIR): `faber format` and
//! `faber convert` keep the author's expression text.
//!
//! What folds: a tree whose every node is float-typed, whose leaves are
//! integer or float literals and whose interior nodes are the operators above,
//! with at least one operator. An integer constant expression stored into a
//! float slot (`let f64 c ← 200 + 100`) folds too (ruling 20): typecheck leaves
//! its integer literals integer-typed under a float-typed root, so it is the
//! integer constant, computed exactly with integer operators (`/` and `%`
//! floor, shifts and bit operations on the exact value), converted to the float
//! once. Without the fold the runner would keep the integer in the float slot
//! and print `300`, not `300.0`. What does not: anything with a path, call,
//! conversion, comparison or integer-typed node (integer `/` is a floor
//! division, a different operation); a zero divisor (the run-time NaN or ∞
//! stays); a result that overflows the slot (the run-time ∞ stays); a value
//! past the exact-rational size cap. Exact zero carries no sign, so
//! `0.0 * -1.0` is `0.0`.

use radix_hir::visit::{HirVisitorMut, walk_expr_mut};
use radix_hir::{HirBinOp, HirExpression, HirExpressionKind, HirLiteral, HirModule, HirUnOp};
use radix_lexer::Interner;
use radix_types::big_literal::parse_big_literal;
use radix_types::exact_rational::ExactRational;
use radix_types::{Magnus, NumericWidth, Primitive, Type, TypeId, TypeTable};

/// Fold every literal-only float expression in `hir` to one literal.
pub fn fold(hir: &mut HirModule, types: &TypeTable, interner: &Interner) {
    let mut pass = Fold { types, interner };
    pass.visit_program_mut(hir);
}

struct Fold<'a> {
    types: &'a TypeTable,
    interner: &'a Interner,
}

impl Fold<'_> {
    /// The float width of `ty`, `None` for any other type.
    fn float_width(&self, mut ty: TypeId) -> Option<NumericWidth> {
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::Primitive(Primitive::Fractus) => return Some(NumericWidth::F64),
                Type::SizedNumeric(Primitive::Fractus, width) if width.is_float() => {
                    return Some(*width);
                }
                _ => return None,
            }
        }
    }

    /// Exact value of a literal-only float tree; `operators` counts the
    /// operators met. `None` when any node is not foldable.
    fn exact(&self, expr: &HirExpression, operators: &mut usize) -> Option<ExactRational> {
        self.float_width(expr.ty?)?;
        match &expr.kind {
            HirExpressionKind::Literal(HirLiteral::Int(value)) => {
                Some(ExactRational::from_integer(Magnus::from_u64(*value)))
            }
            HirExpressionKind::Literal(HirLiteral::BigInt(digits)) => {
                Some(ExactRational::from_integer(parse_big_literal(digits)?))
            }
            HirExpressionKind::Literal(HirLiteral::Float(value)) => ExactRational::from_f64(*value),
            HirExpressionKind::Literal(HirLiteral::FloatText { text, .. }) => {
                ExactRational::from_decimal_text(self.interner.resolve(*text))
            }
            HirExpressionKind::Unary(HirUnOp::Neg, inner) => {
                Some(self.exact(inner, operators)?.neg())
            }
            HirExpressionKind::Binary(op, lhs, rhs) => {
                let lhs = self.exact(lhs, operators)?;
                let rhs = self.exact(rhs, operators)?;
                *operators += 1;
                match op {
                    HirBinOp::Add => lhs.add(&rhs),
                    HirBinOp::Sub => lhs.sub(&rhs),
                    HirBinOp::Mul => lhs.mul(&rhs),
                    HirBinOp::Div | HirBinOp::TrueDiv => lhs.div(&rhs),
                    HirBinOp::Mod => lhs.rem_floor(&rhs),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The literal `expr` folds to, or `None` when it does not fold.
    fn folded(&self, expr: &HirExpression) -> Option<HirExpressionKind> {
        let (HirExpressionKind::Binary(..) | HirExpressionKind::Unary(HirUnOp::Neg, _)) =
            &expr.kind
        else {
            return None;
        };
        let width = self.float_width(expr.ty?)?;
        let mut operators = 0;
        let rounded = if let Some(exact) = self.exact(expr, &mut operators) {
            if operators == 0 {
                return None;
            }
            exact.round_to_width(width)?
        } else {
            let value = integer_constant(expr)?;
            ExactRational::from_integer(Magnus::from_i128(value)).round_to_width(width)?
        };
        if !rounded.is_finite() {
            return None;
        }
        let magnitude = HirLiteral::Float(rounded.abs());
        if !rounded.is_sign_negative() {
            return Some(HirExpressionKind::Literal(magnitude));
        }
        // `-x` keeps the shape the parser gives a negative literal; the
        // operand reuses the id of a node the fold removed.
        let id = match &expr.kind {
            HirExpressionKind::Binary(_, lhs, _) => lhs.id,
            HirExpressionKind::Unary(_, inner) => inner.id,
            _ => return None,
        };
        let operand = HirExpression {
            id,
            kind: HirExpressionKind::Literal(magnitude),
            ty: expr.ty,
            span: expr.span,
        };
        Some(HirExpressionKind::Unary(HirUnOp::Neg, Box::new(operand)))
    }
}

/// The exact value of an integer constant expression: integer literals joined
/// by the integer operators, `/` and `%` flooring. `None` for anything else, a
/// zero divisor, a bad shift count, or a value past `i128`; those stay run-time.
fn integer_constant(expr: &HirExpression) -> Option<i128> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => Some(i128::from(*value)),
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => integer_constant(inner)?.checked_neg(),
        HirExpressionKind::Unary(HirUnOp::BitNot, inner) => Some(!integer_constant(inner)?),
        HirExpressionKind::Binary(op, lhs, rhs) => {
            let (lhs, rhs) = (integer_constant(lhs)?, integer_constant(rhs)?);
            match op {
                HirBinOp::Add => lhs.checked_add(rhs),
                HirBinOp::Sub => lhs.checked_sub(rhs),
                HirBinOp::Mul => lhs.checked_mul(rhs),
                HirBinOp::Div => {
                    let quotient = lhs.checked_div(rhs)?;
                    let remainder = lhs.checked_rem(rhs)?;
                    Some(if remainder != 0 && (remainder < 0) != (rhs < 0) {
                        quotient - 1
                    } else {
                        quotient
                    })
                }
                HirBinOp::Mod => {
                    let remainder = lhs.checked_rem(rhs)?;
                    Some(if remainder != 0 && (remainder < 0) != (rhs < 0) {
                        remainder + rhs
                    } else {
                        remainder
                    })
                }
                HirBinOp::BitAnd => Some(lhs & rhs),
                HirBinOp::BitOr => Some(lhs | rhs),
                HirBinOp::BitXor => Some(lhs ^ rhs),
                HirBinOp::Shl => {
                    let count = u32::try_from(rhs).ok().filter(|count| *count < 127)?;
                    let shifted = lhs.checked_shl(count)?;
                    (shifted >> count == lhs).then_some(shifted)
                }
                HirBinOp::Shr => Some(lhs >> u32::try_from(rhs).ok()?.min(127)),
                _ => None,
            }
        }
        _ => None,
    }
}

impl HirVisitorMut for Fold<'_> {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        if let Some(kind) = self.folded(expr) {
            expr.kind = kind;
            return;
        }
        walk_expr_mut(self, expr);
    }
}

#[cfg(test)]
#[path = "float_literal_fold_test.rs"]
mod tests;
