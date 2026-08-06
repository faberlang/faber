//! Operator spelling and precedence policy for canonical Faber output.
//!
//! The expression writer delegates operator decisions here so syntax validity is
//! kept in one place. The numeric precedence table is not a parser table copy;
//! it is the printer's contract for when child expressions need parentheses to
//! remain parseable after HIR has normalized the original surface form.
//!
//! The backend emits canonical Faber operator spellings. That means source
//! aliases and target-language conveniences are not preserved here. Assignment
//! operators use Faber's compound glyphs when the HIR operator has a dedicated
//! assignment spelling; otherwise the binary spelling is reused so output stays
//! syntactically recognizable rather than inventing new tokens. Equal-precedence
//! child expressions are parenthesized by the expression writer, which favors
//! unambiguous reparsing over preserving associativity-derived minimality.

use radix_hir::{HirBinOp, HirExpression, HirExpressionKind, HirUnOp};

impl super::FaberCodegen {
    /// Return the binding strength used by the pretty-printer.
    ///
    /// Lower numbers bind more weakly. The expression writer parenthesizes a
    /// child when its precedence would otherwise be ambiguous under the current
    /// parent expression.
    pub(super) fn expr_precedence(&self, expr: &HirExpression) -> u8 {
        match &expr.kind {
            HirExpressionKind::Assign(_, _) => 1,
            HirExpressionKind::Si { .. } | HirExpressionKind::Conversio { .. } => 2,
            HirExpressionKind::Binary(op, _, _) => self.binop_precedence(*op),
            // TypeCheck (`expr est <type>`) behaves like Is (precedence 4):
            // both are relational-style checks spelled with `est`.
            HirExpressionKind::TypeCheck { .. } => 4,
            HirExpressionKind::Unary(_, _)
            | HirExpressionKind::Ref(_, _)
            | HirExpressionKind::Deref(_) => 12,
            HirExpressionKind::Call(..)
            | HirExpressionKind::MethodCall(..)
            | HirExpressionKind::Field(_, _)
            | HirExpressionKind::Index(_, _)
            | HirExpressionKind::OptionalChain(_, _)
            | HirExpressionKind::NonNull(_, _)
            | HirExpressionKind::ReadLine(_) => 14,
            _ => 14,
        }
    }

    /// Canonical Faber spelling for unary operators.
    pub(super) fn unop_to_faber(&self, op: HirUnOp) -> &'static str {
        match op {
            HirUnOp::Neg => "-",
            HirUnOp::Not => "non ",
            HirUnOp::BitNot => "¬",
        }
    }

    /// Binding strength for canonical binary operator emission.
    pub(super) fn binop_precedence(&self, op: HirBinOp) -> u8 {
        match op {
            HirBinOp::Or => 2,
            HirBinOp::And => 3,
            HirBinOp::Eq
            | HirBinOp::NotEq
            | HirBinOp::ApproxEq
            | HirBinOp::ApproxNotEq
            | HirBinOp::StrictEq
            | HirBinOp::StrictNotEq
            | HirBinOp::Is
            | HirBinOp::IsNot => 4,
            HirBinOp::Lt
            | HirBinOp::Gt
            | HirBinOp::LtEq
            | HirBinOp::GtEq
            | HirBinOp::InRange
            | HirBinOp::Between => 5,
            HirBinOp::BitOr => 6,
            HirBinOp::BitXor => 7,
            HirBinOp::BitAnd => 8,
            HirBinOp::Shl | HirBinOp::Shr => 9,
            HirBinOp::Add | HirBinOp::Sub => 10,
            HirBinOp::Mul | HirBinOp::Div | HirBinOp::Mod => 11,
            // Local nullable elimination: tighter than arithmetic.
            HirBinOp::Coalesce => 12,
        }
    }

    /// Canonical Faber spelling for binary operators.
    ///
    /// Strict and non-strict equality currently share glyphs in canonical Faber
    /// output because HIR has already carried the semantic distinction.
    pub(super) fn binop_to_faber(&self, op: HirBinOp) -> &'static str {
        match op {
            HirBinOp::Add => "+",
            HirBinOp::Sub => "-",
            HirBinOp::Mul => "*",
            HirBinOp::Div => "/",
            HirBinOp::Mod => "%",
            HirBinOp::Eq | HirBinOp::StrictEq => "≡",
            HirBinOp::ApproxEq => "≈",
            HirBinOp::ApproxNotEq => "≉",
            HirBinOp::NotEq | HirBinOp::StrictNotEq => "≠",
            HirBinOp::Lt => "≺",
            HirBinOp::Gt => "≻",
            HirBinOp::LtEq => "≤",
            HirBinOp::GtEq => "≥",
            HirBinOp::And => "et",
            HirBinOp::Or => "aut",
            HirBinOp::Coalesce => "vel",
            HirBinOp::BitAnd => "∧",
            HirBinOp::BitOr => "∨",
            HirBinOp::BitXor => "⊻",
            HirBinOp::Shl => "⇐",
            HirBinOp::Shr => "⇒",
            HirBinOp::Is => "est",
            HirBinOp::IsNot => "non est",
            HirBinOp::InRange => "intra",
            HirBinOp::Between => "inter",
        }
    }
}
