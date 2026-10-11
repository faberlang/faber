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

use crate::surface::KeywordSurface;
use radix_hir::{HirBinOp, HirExpression, HirExpressionKind, HirUnOp};
use radix_lexer::TokenKind;

impl super::FaberCodegen<'_> {
    /// Return the binding strength used by the pretty-printer.
    ///
    /// Lower numbers bind more weakly. The expression writer parenthesizes a
    /// child when its precedence would otherwise be ambiguous under the current
    /// parent expression.
    pub(super) fn expr_precedence(&self, expr: &HirExpression) -> u8 {
        match &expr.kind {
            // A numeric twin re-emits as a postfix method call, whatever the
            // desugared node (assignment, operator, or hoisted block).
            HirExpressionKind::Assign(..)
            | HirExpressionKind::Binary(..)
            | HirExpressionKind::Unary(..)
                if self.numeric_twin_call(expr).is_some() =>
            {
                14
            }
            HirExpressionKind::Block(..) if self.hoisted_numeric_twin(expr).is_some() => 14,
            HirExpressionKind::Assign(_, _) | HirExpressionKind::ConversioAssign { .. } => 1,
            // `⇇` sits between assignment and the ternary level: the target
            // and value are complete ternaries (parser locks it just above
            // assignment), so it shares the Si/Conversio level.
            //
            // `reducta via … ex <source>` and `ordinata via … ex <source>` end
            // in an open tail (the source, then `vel <identity>`): a following
            // operator or postfix member would be absorbed into that tail
            // (`reduce via sum from a / 4.0` re-parses as a sum of `a / 4.0`).
            // Parenthesize them as an operand of anything tighter than the
            // ternary level.
            HirExpressionKind::Si { .. }
            | HirExpressionKind::Conversio { .. }
            | HirExpressionKind::CallDefault { .. }
            | HirExpressionKind::CopyInto { .. }
            | HirExpressionKind::Reduction { .. }
            | HirExpressionKind::Sort { .. } => 2,
            HirExpressionKind::Binary(op, _, _) => self.binop_precedence(*op),
            // TypeCheck (`expr est <type>`) behaves like Is (precedence 4):
            // both are relational-style checks spelled with `est`.
            HirExpressionKind::TypeCheck { .. } => 4,
            // `¶` sits between comparison and `|` (D2.5). Sharing the
            // comparison level only adds parentheses where a comparison
            // operand is a format — still the same parse — and keeps a
            // nested format parenthesized (no chaining).
            HirExpressionKind::Format { .. } => 5,
            HirExpressionKind::Unary(_, _)
            | HirExpressionKind::Ref(_, _)
            | HirExpressionKind::Deref(_) => 12,
            // `∷` ascription is postfix, but the target type greedily consumes
            // a trailing member/type-argument tail (`f32.power(x)` parses as
            // one qualified type), so a Verte receiver of a postfix member or
            // call form must regroup. 13 keeps every postfix *owner*
            // (method/field/index/call receiver, parent_prec 13)
            // parenthesizing it while leaving it tight against binary forms.
            HirExpressionKind::Verte { .. } => 13,
            // Postfix (call/method/field/index/optional/nonnull/readline)
            // and residual forms share precedence 14.
            _ => 14,
        }
    }

    /// Canonical Faber spelling for unary operators.
    ///
    /// Keyword operators (`non`) localize through the reader surface; glyph
    /// operators are locale-invariant.
    pub(super) fn unop_to_faber(&self, op: HirUnOp, surface: &impl KeywordSurface) -> String {
        match op {
            HirUnOp::Neg => "-".to_owned(),
            HirUnOp::Not => format!("{} ", surface.keyword(TokenKind::Non)),
            HirUnOp::BitNot => "¬".to_owned(),
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
            | HirBinOp::FuzzyEq
            | HirBinOp::FuzzyNotEq
            | HirBinOp::StrictEq
            | HirBinOp::StrictNotEq
            | HirBinOp::Is
            | HirBinOp::IsNot => 4,
            HirBinOp::Lt
            | HirBinOp::Gt
            | HirBinOp::LtEq
            | HirBinOp::GtEq
            | HirBinOp::InRange
            | HirBinOp::Between
            | HirBinOp::NotInRange
            | HirBinOp::NotBetween => 5,
            HirBinOp::BitOr => 6,
            HirBinOp::BitXor => 7,
            HirBinOp::BitAnd => 8,
            HirBinOp::Shl | HirBinOp::Shr => 9,
            HirBinOp::Add | HirBinOp::Sub => 10,
            HirBinOp::Mul
            | HirBinOp::Div
            | HirBinOp::TrueDiv
            | HirBinOp::Mod
            | HirBinOp::Dot
            | HirBinOp::Cross
            | HirBinOp::TensorProduct
            | HirBinOp::Hadamard
            | HirBinOp::HadamardDiv => 11,
            // Local nullable elimination: tighter than arithmetic.
            HirBinOp::Coalesce => 12,
        }
    }

    /// Canonical Faber spelling for binary operators.
    ///
    /// Strict and non-strict equality currently share glyphs in canonical Faber
    /// output because HIR has already carried the semantic distinction.
    /// Keyword operators (`et`, `aut`, `vel`, `est`, `non est`, `inter`,
    /// `intra`) localize through the reader surface; glyph operators are
    /// locale-invariant.
    pub(super) fn binop_to_faber(&self, op: HirBinOp, surface: &impl KeywordSurface) -> String {
        match op {
            HirBinOp::Add => "+".to_owned(),
            HirBinOp::Sub => "-".to_owned(),
            HirBinOp::Mul => "*".to_owned(),
            HirBinOp::Div => "/".to_owned(),
            HirBinOp::TrueDiv => "÷".to_owned(),
            HirBinOp::Mod => "%".to_owned(),
            HirBinOp::Dot => "·".to_owned(),
            HirBinOp::Cross => "×".to_owned(),
            HirBinOp::TensorProduct => "⊗".to_owned(),
            HirBinOp::Hadamard => "⊙".to_owned(),
            HirBinOp::HadamardDiv => "⊘".to_owned(),
            HirBinOp::Eq | HirBinOp::StrictEq => "≡".to_owned(),
            HirBinOp::ApproxEq => "≅".to_owned(),
            HirBinOp::ApproxNotEq => "≇".to_owned(),
            HirBinOp::FuzzyEq => "≈".to_owned(),
            HirBinOp::FuzzyNotEq => "≉".to_owned(),
            HirBinOp::NotEq | HirBinOp::StrictNotEq => "≢".to_owned(),
            HirBinOp::Lt => "≺".to_owned(),
            HirBinOp::Gt => "≻".to_owned(),
            HirBinOp::LtEq => "≤".to_owned(),
            HirBinOp::GtEq => "≥".to_owned(),
            HirBinOp::And => surface.keyword(TokenKind::Et).to_owned(),
            HirBinOp::Or => surface.keyword(TokenKind::Aut).to_owned(),
            HirBinOp::Coalesce => surface.keyword(TokenKind::Vel).to_owned(),
            HirBinOp::BitAnd => "∧".to_owned(),
            HirBinOp::BitOr => "∨".to_owned(),
            HirBinOp::BitXor => "⊻".to_owned(),
            HirBinOp::Shl => "⇐".to_owned(),
            HirBinOp::Shr => "⇒".to_owned(),
            HirBinOp::Is => surface.keyword(TokenKind::Est).to_owned(),
            HirBinOp::IsNot => format!(
                "{} {}",
                surface.keyword(TokenKind::Non),
                surface.keyword(TokenKind::Est)
            ),
            // WHY: membership is locale-invariant glyph surface (ruling 4); the
            // keyword spellings `intra`/`inter` are parse-only until unit 1c retires them.
            HirBinOp::InRange | HirBinOp::Between => "∈".to_owned(),
            HirBinOp::NotInRange | HirBinOp::NotBetween => "∉".to_owned(),
        }
    }
}

/// Whether the parser folds this operator's tier left to right, so a
/// same-precedence left operand re-parses to the same tree without parens
/// (`a + b - c` is `(a + b) - c`, `a * b / c` is `(a * b) / c`). Only the
/// right operand needs the equal-precedence parenthesis, so the tree shape is
/// kept. The comparison tiers do not chain and keep their parens.
pub(super) fn binop_left_associative(op: HirBinOp) -> bool {
    matches!(
        op,
        HirBinOp::Add
            | HirBinOp::Sub
            | HirBinOp::Mul
            | HirBinOp::Div
            | HirBinOp::TrueDiv
            | HirBinOp::Mod
            | HirBinOp::Dot
            | HirBinOp::Cross
            | HirBinOp::TensorProduct
            | HirBinOp::Hadamard
            | HirBinOp::HadamardDiv
            | HirBinOp::Shl
            | HirBinOp::Shr
            | HirBinOp::And
            | HirBinOp::Or
            | HirBinOp::BitAnd
            | HirBinOp::BitOr
            | HirBinOp::BitXor
    )
}
