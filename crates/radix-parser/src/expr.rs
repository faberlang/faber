//! Expression grammar and precedence construction.
//!
//! Expressions are the densest recursive-descent surface in the parser. This
//! module encodes precedence as a call chain from assignment down to primaries,
//! then folds postfix operations in a loop so calls, indexing, member access,
//! optional chaining, non-null chaining, and conversions all bind tighter than
//! infix operators.
//!
//! The parser prefers explicit grammar functions over a data-driven operator
//! table. That makes the accepted syntax visible at the cost of boilerplate, and
//! it keeps contextual Faber forms close to their diagnostics. Binary operators
//! associate left through loops; assignment and ternary forms recurse on the
//! right where the language expects right associativity.
//!
//! - No expression form may invent declaration syntax; constructors and casts
//!   are parsed only through tokenized grammar surfaces.
//! - Compact closures are the only expression form that uses speculative
//!   rollback, and rollback restores both token position and node-id allocation.
//! - `vacua` is recognized as a value-level empty collection marker; required
//!   collection types are supplied by declaration/type contexts upstream.
//! - Field separators in object-like expression syntax are canonical `=`, not
//!   colon aliases.

use super::{ParseError, ParseErrorKind, Parser};
use radix_lexer::{lookup_keyword_spec, KeywordOwner, KeywordScope, Span, Symbol, TokenKind};
#[allow(clippy::wildcard_imports)]
use radix_syntax::*;

// EXPRESSION ENTRY POINT

impl Parser {
    /// Parse an expression from the lowest-precedence level.
    ///
    /// GRAMMAR:
    ///   expression := assignment
    pub(super) fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_assignment()
    }

    fn binary_expr(&mut self, start: Span, lhs: Expr, op: BinOp, rhs: Expr) -> Expr {
        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Expr {
            id,
            kind: ExprKind::Binary(BinaryExpr {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            }),
            span,
        }
    }

    // PRECEDENCE CLIMBING

    /// Parse assignment expression (lowest precedence).
    ///
    /// GRAMMAR:
    ///   assignment := ternary ('←' assignment | '↤' assignment inlineRecovery?)?
    ///   inlineRecovery := '⇥' unary
    ///
    /// WHY: Right-associative via recursion on `RHS`. Allows `a ← b ← c` and
    /// `a ↤ b ↤ c`. `↤` is conversion-directed assignment (U+21A4) and sits at
    /// the same precedence level as `←`; `⇥` inline recovery is legal only on
    /// `↤` and attaches to the nearest `↤` in a right-associated chain.
    fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let expr = self.parse_ternary()?;
        self.parse_assignment_tail(expr, start)
    }

    /// Continue an assignment whose left operand is already parsed.
    ///
    /// Shared by the expression grammar (ternary LHS) and typed declaration
    /// initializers (binding LHS) so the `↤` contract — same precedence as
    /// `←`, right-associative, `⇥` recovery on `↤` only — lives in one place.
    pub(super) fn parse_assignment_tail(
        &mut self,
        target: Expr,
        start: Span,
    ) -> Result<Expr, ParseError> {
        let op = match self.peek().kind {
            TokenKind::Assign => AssignOp::Assign,
            TokenKind::ConversioAssign => AssignOp::ConversioAssign,
            _ => return Ok(target),
        };

        self.advance();
        let value = Box::new(self.parse_assignment()?);

        let recovery = if matches!(op, AssignOp::ConversioAssign) {
            if self.eat(&TokenKind::ExitArrow) {
                Some(Box::new(self.parse_unary()?))
            } else {
                None
            }
        } else {
            if self.check(&TokenKind::ExitArrow) {
                return Err(ParseError::structured(
                    ParseErrorKind::Unexpected,
                    self.current_span(),
                )
                .with_arg("issue", "conversio_recovery_on_assign"));
            }
            None
        };

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Assign(AssignExpr {
                op,
                target: Box::new(target),
                value,
                recovery,
            }),
            span,
        })
    }

    /// Parse ternary conditional expression.
    ///
    /// GRAMMAR:
    ///   ternary := or ['?' expr ':' ternary | `sic` expr `secus` ternary]
    ///
    /// WHY: Faber supports both C-style (? :) and keyword-style (sic secus) ternary.
    /// Right-associative via recursion on else branch.
    fn parse_ternary(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let cond = self.parse_or()?;

        if self.eat(&TokenKind::Question) {
            let then = Box::new(self.parse_expression()?);
            self.expect(&TokenKind::Colon, "expected ':' in ternary")?;
            let else_ = Box::new(self.parse_ternary()?);
            let span = start.merge(self.previous_span());
            let id = self.next_id();
            return Ok(Expr {
                id,
                kind: ExprKind::Ternary(TernaryExpr {
                    cond: Box::new(cond),
                    then,
                    else_,
                    style: TernaryStyle::QuestionColon,
                }),
                span,
            });
        }

        if self.eat_keyword_ident(TokenKind::Sic) {
            let then = Box::new(self.parse_expression()?);
            self.expect_keyword_ident(TokenKind::Secus, "expected 'secus' after 'sic'")?;
            let else_ = Box::new(self.parse_ternary()?);
            let span = start.merge(self.previous_span());
            let id = self.next_id();
            return Ok(Expr {
                id,
                kind: ExprKind::Ternary(TernaryExpr {
                    cond: Box::new(cond),
                    then,
                    else_,
                    style: TernaryStyle::SicSecus,
                }),
                span,
            });
        }

        Ok(cond)
    }

    /// Logical OR (`aut` only). Nullable defaulting `vel` is a tighter tier.
    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_and()?;

        while self.check_keyword_ident(TokenKind::Aut) {
            self.advance();
            let right = self.parse_and()?;
            left = self.binary_expr(start, left, BinOp::Or, right);
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_equality()?;

        while self.check_keyword_ident(TokenKind::Et) {
            self.advance();
            let right = self.parse_equality()?;
            left = self.binary_expr(start, left, BinOp::And, right);
        }

        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_comparison()?;

        loop {
            let op = if self.eat_keyword_ident(TokenKind::Non) {
                if self.eat_keyword_ident(TokenKind::Est) {
                    BinOp::IsNot
                } else {
                    return Err(self.expected_expression_error("equality_non_missing_est"));
                }
            } else {
                let op = match self.peek().kind {
                    TokenKind::EqEq => BinOp::Eq,
                    TokenKind::ApproxEq => BinOp::ApproxEq,
                    TokenKind::ApproxNotEq => BinOp::ApproxNotEq,
                    TokenKind::BangEq => BinOp::NotEq,
                    _ if self.check_keyword_ident(TokenKind::Est) => BinOp::Is,
                    _ => break,
                };
                self.advance();
                // EDGE: `expr est non ...` is a common misspelling of `expr non est ...`.
                // Reject eagerly so the user gets a clear diagnostic instead of a silent
                // parse as `expr est (non ...)` with a later type error.
                if matches!(op, BinOp::Is) && self.check_keyword_ident(TokenKind::Non) {
                    return Err(self.expected_expression_error("est_non_spelling"));
                }
                op
            };

            // `est`/`non est` with a type RHS produces a TypeCheck node (variant
            // tag check). `est nihil` and value-RHS forms stay as Binary(Is).
            let positive = matches!(op, BinOp::Is);
            if self.peek_starts_type_name() {
                let ty = self.parse_type()?;
                let span = start.merge(self.previous_span());
                let id = self.next_id();
                left = Expr {
                    id,
                    kind: ExprKind::TypeCheck(TypeCheckExpr {
                        positive,
                        expr: Box::new(left),
                        ty,
                    }),
                    span,
                };
                continue;
            }

            let right = self.parse_comparison()?;
            left = self.binary_expr(start, left, op, right);
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_bitwise_or()?;

        loop {
            let op = match self.peek().kind {
                // Canonical strict comparison is `Lt`/`Gt` (≺/≻).
                // `LtAscii`/`GtAscii` (`<`/`>`) as comparison are legacy during
                // the glyph migration; `<`/`>` remain generic delimiters.
                TokenKind::Lt | TokenKind::LtAscii => BinOp::Lt,
                TokenKind::Gt | TokenKind::GtAscii => BinOp::Gt,
                TokenKind::LtEq => BinOp::LtEq,
                TokenKind::GtEq => BinOp::GtEq,
                _ if self.check_keyword_ident(TokenKind::Intra) => BinOp::InRange,
                _ if self.check_keyword_ident(TokenKind::Inter) => BinOp::Between,
                _ => break,
            };
            self.advance();
            let right = self.parse_bitwise_or()?;
            left = self.binary_expr(start, left, op, right);
        }

        Ok(left)
    }

    fn parse_bitwise_or(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_bitwise_xor()?;

        while self.check(&TokenKind::BitwiseOr) {
            self.advance();
            let right = self.parse_bitwise_xor()?;
            left = self.binary_expr(start, left, BinOp::BitOr, right);
        }

        Ok(left)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_bitwise_and()?;

        while self.check(&TokenKind::BitwiseXor) {
            self.advance();
            let right = self.parse_bitwise_and()?;
            left = self.binary_expr(start, left, BinOp::BitXor, right);
        }

        Ok(left)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_shift()?;

        while self.check(&TokenKind::BitwiseAnd) {
            self.advance();
            let right = self.parse_shift()?;
            left = self.binary_expr(start, left, BinOp::BitAnd, right);
        }

        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_range_with_conversio()?;

        loop {
            let op = if self.check(&TokenKind::BitwiseShl) {
                self.advance();
                BinOp::Shl
            } else if self.check(&TokenKind::BitwiseShr) {
                self.advance();
                BinOp::Shr
            } else {
                break;
            };

            let right = self.parse_range_with_conversio()?;
            left = self.binary_expr(start, left, op, right);
        }

        Ok(left)
    }

    fn parse_range(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let left = self.parse_range_bound()?;

        let kind = if self.check(&TokenKind::DotDot) {
            self.advance();
            Some(RangeKind::Exclusive)
        } else if self.check(&TokenKind::Ellipsis) {
            self.advance();
            Some(RangeKind::Inclusive)
        } else if self.check_keyword_ident(TokenKind::Ante) {
            self.advance();
            Some(RangeKind::Exclusive)
        } else if self.check_keyword_ident(TokenKind::Usque) {
            self.advance();
            Some(RangeKind::Inclusive)
        } else {
            None
        };

        if let Some(kind) = kind {
            let end = Box::new(self.parse_range_bound()?);
            let step = if self.eat_keyword_ident(TokenKind::Per) {
                Some(Box::new(self.parse_range_bound()?))
            } else {
                None
            };
            let span = start.merge(self.previous_span());
            let id = self.next_id();
            return Ok(Expr {
                id,
                kind: ExprKind::Intervallum(IntervallumExpr {
                    start: Box::new(left),
                    end,
                    step,
                    kind,
                }),
                span,
            });
        }

        Ok(left)
    }

    /// Range literal or bound expression, then optional postfix `↦` (materialize / range clamp).
    fn parse_range_with_conversio(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_range()?;
        let start = expr.span;
        while self.check_keyword(TokenKind::Conversio) {
            expr = self.parse_conversio_postfix(expr, start)?;
        }
        Ok(expr)
    }

    /// EBNF `additive` for range bounds — excludes postfix `↦` so `0‥10 ↦ T` parses as conversio.
    fn parse_range_bound(&mut self) -> Result<Expr, ParseError> {
        self.parse_additive_inner(false)
    }

    fn parse_additive_inner(&mut self, allow_conversio: bool) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_multiplicative_inner(allow_conversio)?;

        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative_inner(allow_conversio)?;
            left = self.binary_expr(start, left, op, right);
        }

        Ok(left)
    }

    /// Multiplicative over local nullable defaulting (`vel`).
    ///
    /// GRAMMAR:
    ///   multiplicative := coalesce (('*' | '/' | '%' | '·' | '×' | '⊗' | '⊙') coalesce)*
    ///   coalesce       := unary (`vel` `vel_rhs`)*
    ///   `vel_rhs`        := unary (`range_op` unary (`per` unary)?)?
    ///
    /// `vel` is local nullable elimination: tighter than `+` and `*`, so
    /// `prefix + item vel ""` and `scale * factor vel 1` both default only the
    /// adjacent term. The `vel` `RHS` may complete an interval constructor
    /// (`0‥0`) so fallback ranges are not split as `(x vel 0)‥0`.
    fn parse_multiplicative_inner(&mut self, allow_conversio: bool) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_coalesce_inner(allow_conversio)?;

        loop {
            let op = match self.peek().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                TokenKind::MiddleDot => BinOp::Dot,
                TokenKind::Cross => BinOp::Cross,
                TokenKind::CircledTimes => BinOp::TensorProduct,
                TokenKind::CircledDot => BinOp::Hadamard,
                _ => break,
            };
            self.advance();
            let right = self.parse_coalesce_inner(allow_conversio)?;
            left = self.binary_expr(start, left, op, right);
        }

        Ok(left)
    }

    /// Local nullable elimination (`vel`). Left-associative: `a vel b vel c`.
    fn parse_coalesce_inner(&mut self, allow_conversio: bool) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_unary_inner(allow_conversio)?;

        while self.check_keyword_ident(TokenKind::Vel) {
            self.advance();
            let right = self.parse_vel_rhs(allow_conversio)?;
            left = self.binary_expr(start, left, BinOp::Coalesce, right);
        }

        Ok(left)
    }

    /// `RHS` of `vel`: a unary value, or an interval constructor `lo‥hi` / `lo…hi`.
    ///
    /// Interval bounds stay unary-tight so broad bound expressions require
    /// parentheses: `maybeRange vel (lo + 1‥hi - 1)`.
    fn parse_vel_rhs(&mut self, allow_conversio: bool) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let left = self.parse_unary_inner(allow_conversio)?;

        let kind = if self.check(&TokenKind::DotDot) {
            self.advance();
            Some(RangeKind::Exclusive)
        } else if self.check(&TokenKind::Ellipsis) {
            self.advance();
            Some(RangeKind::Inclusive)
        } else if self.check_keyword_ident(TokenKind::Ante) {
            self.advance();
            Some(RangeKind::Exclusive)
        } else if self.check_keyword_ident(TokenKind::Usque) {
            self.advance();
            Some(RangeKind::Inclusive)
        } else {
            None
        };

        if let Some(kind) = kind {
            let end = Box::new(self.parse_unary_inner(allow_conversio)?);
            let step = if self.eat_keyword_ident(TokenKind::Per) {
                Some(Box::new(self.parse_unary_inner(allow_conversio)?))
            } else {
                None
            };
            let span = start.merge(self.previous_span());
            let id = self.next_id();
            return Ok(Expr {
                id,
                kind: ExprKind::Intervallum(IntervallumExpr {
                    start: Box::new(left),
                    end,
                    step,
                    kind,
                }),
                span,
            });
        }

        Ok(left)
    }

    /// Parse unary prefix operators.
    ///
    /// GRAMMAR:
    ///   unary := unary-op unary | postfix
    ///   unary-op := '-' | '¬' | `non`
    ///
    /// EDGE: `finge` is a unary-position variant constructor, not a prefix operator.
    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        self.parse_unary_inner(true)
    }

    fn parse_unary_inner(&mut self, allow_conversio: bool) -> Result<Expr, ParseError> {
        let start = self.current_span();

        self.reject_retired_literal_prefix_predicate()?;
        self.reject_retired_required_prefix_predicate()?;

        let op = match self.peek().kind {
            TokenKind::Minus => Some(UnOp::Neg),
            TokenKind::BitwiseNot => Some(UnOp::BitNot),
            _ if self.check_keyword_ident(TokenKind::Non) => Some(UnOp::Not),
            _ => None,
        };

        if let Some(op) = op {
            self.advance();
            let operand = Box::new(self.parse_unary_inner(allow_conversio)?);
            let span = start.merge(self.previous_span());
            let id = self.next_id();
            return Ok(Expr {
                id,
                kind: ExprKind::Unary(UnaryExpr { op, operand }),
                span,
            });
        }

        if self.check_keyword_ident(TokenKind::Finge) {
            return self.parse_variant_expr();
        }

        self.parse_postfix_inner(allow_conversio)
    }

    fn can_start_expression(kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Ident(_)
                | TokenKind::Underscore(_)
                | TokenKind::Integer(_)
                | TokenKind::Float(_)
                | TokenKind::String(_)
                | TokenKind::AsciiString(_)
                | TokenKind::BacktickString(_)
                | TokenKind::OctetiString(_)
                | TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::LBrace
                | TokenKind::Solum
                | TokenKind::Minus
                | TokenKind::BitwiseNot
                | TokenKind::Nulla
                | TokenKind::Nonnulla
                | TokenKind::Negativum
                | TokenKind::Positivum
                | TokenKind::Cede
        )
    }

    fn can_start_prefix_operand(kind: &TokenKind) -> bool {
        Self::can_start_expression(kind) && !matches!(kind, TokenKind::LBrace)
    }

    fn reject_retired_literal_prefix_predicate(&self) -> Result<(), ParseError> {
        let Some((predicate, replacement)) = self.literal_prefix_predicate_replacement() else {
            return Ok(());
        };
        if !self.peek_at_one_starts_prefix_operand()
            || self.looks_like_following_field_member()
            || !self.operand_starts_same_line()
        {
            return Ok(());
        }
        Err(self
            .expected_expression_error("retired_prefix_predicate")
            .with_arg("predicate", predicate)
            .with_arg("replacement", replacement))
    }

    /// Whether the token after the current one starts on the same source line.
    ///
    /// The retired prefix-predicate forms (`falsum <operand>`) are legacy
    /// single-line expressions. A literal at the end of a statement followed
    /// by the next statement's first token (`return false` then `j ← j + 1`)
    /// reads identically at the token level, so firing across a line break
    /// would reject valid statement sequences.
    fn operand_starts_same_line(&self) -> bool {
        let slot = self.non_trivia_indices.partition_point(|&i| i < self.pos);
        let Some(&this_idx) = self.non_trivia_indices.get(slot) else {
            return false;
        };
        let Some(&next_idx) = self.non_trivia_indices.get(slot + 1) else {
            return true;
        };
        !self.tokens[this_idx + 1..next_idx]
            .iter()
            .any(|token| token.kind == TokenKind::Newline)
    }

    /// Whether the token one ahead would start a value (prefix) operand.
    ///
    /// Token-shaped tokens use the static [`Self::can_start_prefix_operand`]
    /// predicate. A tokenless keyword arrives as an `Ident`, so resolve its
    /// Latin identity against the keyword registry: only expression-primary
    /// claims (owners `Constructor`, `BuiltinCall`, `Literal`) start value
    /// operands; statement, declaration, and type keywords are grammar
    /// structure, not operands. This keeps the retired `nihil x` detection
    /// from firing on a value `nihil` followed by a migrated keyword (e.g.
    /// `nota`) on the next line.
    fn peek_at_one_starts_prefix_operand(&self) -> bool {
        match self.peek_at(1).kind {
            TokenKind::Ident(sym) => {
                match self.keyword_claim_key(sym).and_then(lookup_keyword_spec) {
                    Some(spec) => matches!(
                        spec.scope,
                        KeywordScope::Contextual(owners)
                            if owners.iter().any(|o| matches!(
                                o,
                                KeywordOwner::Constructor
                                    | KeywordOwner::BuiltinCall
                                    | KeywordOwner::Literal
                                    | KeywordOwner::ExpressionIntroducer
                            ))
                    ),
                    _ => true,
                }
            }
            ref kind => Self::can_start_prefix_operand(kind),
        }
    }

    fn literal_prefix_predicate_replacement(&self) -> Option<(&'static str, &'static str)> {
        if self.check_keyword_ident(TokenKind::Verum) {
            Some(("verum", "value est verum"))
        } else if self.check_keyword_ident(TokenKind::Falsum) {
            Some(("falsum", "value est falsum"))
        } else if self.check_keyword_ident(TokenKind::Nihil) {
            Some(("nihil", "value est nihil"))
        } else {
            None
        }
    }

    fn reject_retired_required_prefix_predicate(&self) -> Result<(), ParseError> {
        let Some((predicate, replacement)) = self.required_prefix_predicate_replacement() else {
            return Ok(());
        };
        // Only the retired predicate form (`negativum <value>`) fires: a bare
        // identifier use of the same spelling (e.g. a variable named
        // `negative`) followed by anything that does not start a value operand
        // is an ordinary expression. The retired form is a legacy single-line
        // expression, so the operand must start on the same line.
        if !self.peek_at_one_starts_prefix_operand()
            || self.looks_like_following_field_member()
            || !self.operand_starts_same_line()
        {
            return Ok(());
        }
        Err(self
            .expected_expression_error("retired_prefix_predicate")
            .with_arg("predicate", predicate)
            .with_arg("replacement", replacement))
    }

    fn required_prefix_predicate_replacement(&self) -> Option<(&'static str, &'static str)> {
        if self.check_keyword_ident(TokenKind::Nulla) {
            Some(("nulla", "value est nihil"))
        } else if self.check_keyword_ident(TokenKind::Nonnulla) {
            Some(("nonnulla", "value non est nihil"))
        } else if self.check_keyword_ident(TokenKind::Nonnihil) {
            Some(("nonnihil", "value non est nihil"))
        } else if self.check_keyword_ident(TokenKind::Negativum) {
            Some(("negativum", "value < 0"))
        } else if self.check_keyword_ident(TokenKind::Positivum) {
            Some(("positivum", "value > 0"))
        } else {
            None
        }
    }

    fn looks_like_following_field_member(&self) -> bool {
        self.can_start_type_name(&self.peek_at(1).kind)
            && matches!(self.peek_at(2).kind, TokenKind::Ident(_))
    }

    fn can_start_type_name(&self, kind: &TokenKind) -> bool {
        match kind {
            TokenKind::Ident(sym) => {
                let name = self.interner.resolve(*sym);
                // Resolve a localized type spelling to its latin identity
                // before matching the conversio-type list (`string` →
                // `textus` under the en pack), so the field-boundary guard
                // fires under every reader locale.
                let latin = self
                    .reader_type_latin_keys
                    .get(name)
                    .map_or(name, String::as_str);
                Self::is_conversio_type_keyword(latin) || Self::is_capitalized_type_name(name)
            }
            _ => false,
        }
    }

    /// Parse postfix operations that bind tighter than every infix operator.
    ///
    /// GRAMMAR:
    ///   postfix := primary (call | member | index | gradient | cast | optional-chain | conversion)*
    ///   call := `(` args `)`
    ///   member := `.` ident
    ///   index := `[` expr `]`
    ///   gradient := '∇' ['[' place (',' place)* ']']
    ///   cast := `∷` type
    ///   optional-chain := '?.' ident | '?[' expr `]` | '?(' args `)`
    ///   conversion := '↦' type [type-params] ['⇥' recovery]
    ///
    /// Keeping every postfix form in one loop preserves left-to-right chaining
    /// (`a.b(c)[`d`]`) and prevents casts/conversions from accidentally binding at
    /// a different level than calls or optional chaining. The gradient suffix
    /// consumes its optional selector bracket here, before the loop resumes, so
    /// later postfix operations apply to the gradient result — never to the
    /// selector list.
    fn parse_postfix_inner(&mut self, allow_conversio: bool) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut expr = self.parse_primary()?;

        loop {
            if self.check(&TokenKind::LtAscii) {
                self.reject_retired_angle_type_call_constructor(&expr)?;
                let checkpoint = self.checkpoint();
                if let Ok(type_args) = self.parse_call_type_args() {
                    if self.check(&TokenKind::LParen) {
                        expr = self.parse_call_postfix(start, expr, type_args)?;
                        continue;
                    }
                }
                self.restore(checkpoint);
                break;
            } else if self.check(&TokenKind::LParen) {
                self.reject_retired_bare_type_call_constructor(&expr)?;
                expr = self.parse_call_postfix(start, expr, Vec::new())?;
            } else if self.check(&TokenKind::Dot) {
                expr = self.parse_member_postfix(start, expr)?;
            } else if self.check(&TokenKind::LBracket) {
                self.reject_retired_shape_sugar_type_call_constructor(&expr)?;
                expr = self.parse_index_postfix(start, expr)?;
            } else if self.check(&TokenKind::Nabla) {
                expr = self.parse_gradient_postfix(start, expr)?;
            } else if self.check(&TokenKind::QuestionDot) {
                expr = self.parse_optional_member_postfix(start, expr)?;
            } else if self.check(&TokenKind::QuestionBracket) {
                expr = self.parse_optional_index_postfix(start, expr)?;
            } else if self.check(&TokenKind::QuestionParen) {
                expr = self.parse_optional_call_postfix(start, expr)?;
            } else if self.check(&TokenKind::BangDot) {
                expr = self.parse_non_null_member_postfix(start, expr)?;
            } else if self.check(&TokenKind::BangBracket) {
                expr = self.parse_non_null_index_postfix(start, expr)?;
            } else if self.check(&TokenKind::BangParen) {
                expr = self.parse_non_null_call_postfix(start, expr)?;
            } else if self.check_keyword(TokenKind::Verte) {
                expr = self.parse_verte_postfix(start, expr)?;
            } else if allow_conversio && self.check_keyword(TokenKind::Conversio) {
                expr = self.parse_conversio_postfix(expr, start)?;
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn postfix_expr(&mut self, start: Span, kind: ExprKind) -> Expr {
        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Expr { id, kind, span }
    }

    fn parse_call_postfix(
        &mut self,
        start: Span,
        callee: Expr,
        type_args: Vec<TypeExpr>,
    ) -> Result<Expr, ParseError> {
        self.advance();
        let args = self.parse_argument_list()?;
        self.expect(&TokenKind::RParen, "expected ')'")?;
        Ok(self.postfix_expr(
            start,
            ExprKind::Call(CallExpr {
                callee: Box::new(callee),
                type_args,
                args,
            }),
        ))
    }

    fn parse_member_postfix(&mut self, start: Span, object: Expr) -> Result<Expr, ParseError> {
        self.advance();
        let member = self.parse_member_ident()?;
        Ok(self.postfix_expr(
            start,
            ExprKind::Member(MemberExpr {
                object: Box::new(object),
                member,
            }),
        ))
    }

    fn parse_index_postfix(&mut self, start: Span, object: Expr) -> Result<Expr, ParseError> {
        self.advance();
        let index = Box::new(self.parse_expression()?);
        self.expect(&TokenKind::RBracket, "expected ']'")?;
        Ok(self.postfix_expr(
            start,
            ExprKind::Index(IndexExpr {
                object: Box::new(object),
                index,
            }),
        ))
    }

    /// Parse the postfix gradient suffix `∇ [places]?` bound to an already-parsed
    /// forward expression.
    ///
    /// The gradient glyph is a structural postfix operator, never a prefix unary
    /// form. The optional selector bracket is consumed here, before the ordinary
    /// postfix loop resumes, so later postfix operations apply to the gradient
    /// result rather than to the selector list. The grammar is
    /// whitespace-insensitive: `loss(x) ∇[x]` parses and lowers to the same
    /// selectors as `loss(x) ∇ [x]`; both formatters re-emit the canonical spaced
    /// form. A non-empty selector list is required once `[` follows `∇`.
    fn parse_gradient_postfix(&mut self, start: Span, call: Expr) -> Result<Expr, ParseError> {
        self.advance(); // consume `∇`
        let selectors = if self.check(&TokenKind::LBracket) {
            self.advance();
            if self.check(&TokenKind::RBracket) {
                return Err(self.expected_expression_error("gradient_empty_selector_list"));
            }
            let mut selectors = Vec::new();
            loop {
                selectors.push(self.parse_expression()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(
                &TokenKind::RBracket,
                "expected ']' after gradient selectors",
            )?;
            selectors
        } else {
            Vec::new()
        };
        Ok(self.postfix_expr(
            start,
            ExprKind::Gradient(GradientExpr {
                call: Box::new(call),
                selectors,
            }),
        ))
    }

    fn parse_optional_member_postfix(
        &mut self,
        start: Span,
        object: Expr,
    ) -> Result<Expr, ParseError> {
        self.advance();
        let member = self.parse_member_ident()?;
        Ok(self.optional_chain_expr(start, object, OptionalChainKind::Member(member)))
    }

    fn parse_optional_index_postfix(
        &mut self,
        start: Span,
        object: Expr,
    ) -> Result<Expr, ParseError> {
        self.advance();
        let index = Box::new(self.parse_expression()?);
        self.expect(&TokenKind::RBracket, "expected ']'")?;
        Ok(self.optional_chain_expr(start, object, OptionalChainKind::Index(index)))
    }

    fn parse_optional_call_postfix(
        &mut self,
        start: Span,
        object: Expr,
    ) -> Result<Expr, ParseError> {
        self.advance();
        let args = self.parse_argument_list()?;
        self.expect(&TokenKind::RParen, "expected ')'")?;
        Ok(self.optional_chain_expr(start, object, OptionalChainKind::Call(args)))
    }

    fn optional_chain_expr(&mut self, start: Span, object: Expr, chain: OptionalChainKind) -> Expr {
        self.postfix_expr(
            start,
            ExprKind::OptionalChain(OptionalChainExpr {
                object: Box::new(object),
                chain,
            }),
        )
    }

    fn parse_non_null_member_postfix(
        &mut self,
        start: Span,
        object: Expr,
    ) -> Result<Expr, ParseError> {
        self.advance();
        let member = self.parse_member_ident()?;
        Ok(self.non_null_expr(start, object, NonNullKind::Member(member)))
    }

    fn parse_non_null_index_postfix(
        &mut self,
        start: Span,
        object: Expr,
    ) -> Result<Expr, ParseError> {
        self.advance();
        let index = Box::new(self.parse_expression()?);
        self.expect(&TokenKind::RBracket, "expected ']'")?;
        Ok(self.non_null_expr(start, object, NonNullKind::Index(index)))
    }

    fn parse_non_null_call_postfix(
        &mut self,
        start: Span,
        object: Expr,
    ) -> Result<Expr, ParseError> {
        self.advance();
        let args = self.parse_argument_list()?;
        self.expect(&TokenKind::RParen, "expected ')'")?;
        Ok(self.non_null_expr(start, object, NonNullKind::Call(args)))
    }

    fn non_null_expr(&mut self, start: Span, object: Expr, chain: NonNullKind) -> Expr {
        self.postfix_expr(
            start,
            ExprKind::NonNull(NonNullExpr {
                object: Box::new(object),
                chain,
            }),
        )
    }

    fn parse_verte_postfix(&mut self, start: Span, expr: Expr) -> Result<Expr, ParseError> {
        self.advance();
        let ty = self.parse_type()?;
        Ok(self.postfix_expr(
            start,
            ExprKind::Verte(VerteExpr {
                expr: Box::new(expr),
                ty,
            }),
        ))
    }

    fn reject_retired_bare_type_call_constructor(
        &mut self,
        callee: &Expr,
    ) -> Result<(), ParseError> {
        let Some(name) = self.callee_ident_name(callee) else {
            return Ok(());
        };

        if !Self::is_conversio_type_keyword(name) {
            return Ok(());
        }

        Err(self.expected_expression_error("retired_type_call_constructor"))
    }

    fn reject_retired_angle_type_call_constructor(
        &mut self,
        callee: &Expr,
    ) -> Result<(), ParseError> {
        let Some(name) = self.callee_ident_name(callee) else {
            return Ok(());
        };
        if !Self::is_conversio_type_keyword(name) {
            return Ok(());
        }
        // Capture shape kind before mutable token ops so the intern borrow ends.
        let shape_issue = match name {
            "tensor" => Some("tensor_figura_after_element"),
            "sparsa" => Some("sparsa_figura_after_element"),
            "matrix" => Some("matrix_figura_after_element"),
            "vector" => Some("vector_width_after_element"),
            _ => None,
        };

        let checkpoint = self.checkpoint();
        self.expect(&TokenKind::LtAscii, "expected '<'")?;
        if let Some(issue) = shape_issue {
            let _elem = self.parse_type()?;
            if !self.eat(&TokenKind::Comma) {
                return Err(self.expected_expression_error(issue));
            }
            let _figura = self.parse_figura()?;
            self.expect(&TokenKind::GtAscii, "expected '>'")?;
        } else {
            let _params = self.finish_type_args_list()?;
        }

        if self.check(&TokenKind::LParen) {
            return Err(self.expected_expression_error("retired_type_call_constructor"));
        }
        self.restore(checkpoint);
        Ok(())
    }

    fn reject_retired_shape_sugar_type_call_constructor(
        &mut self,
        callee: &Expr,
    ) -> Result<(), ParseError> {
        let Some(name) = self.callee_ident_name(callee) else {
            return Ok(());
        };
        if radix_types::numeric_width::tensor_sugar_width_marker(name).is_none()
            && radix_types::numeric_width::vector_sugar_width_marker(name).is_none()
            && radix_types::numeric_width::matrix_sugar_width_marker(name).is_none()
            && radix_types::numeric_width::sparsa_sugar_width_marker(name).is_none()
        {
            return Ok(());
        }

        let checkpoint = self.checkpoint();
        let _width = self.parse_figura()?;
        if self.check(&TokenKind::LParen) {
            return Err(self.expected_expression_error("retired_type_call_constructor"));
        }
        self.restore(checkpoint);
        Ok(())
    }

    fn callee_ident_name<'a>(&'a self, callee: &Expr) -> Option<&'a str> {
        let ExprKind::Ident(ident) = &callee.kind else {
            return None;
        };
        Some(self.interner.resolve(ident.name))
    }

    /// Parse `↦ type [<params>] [⇥ recovery]` bound to an already-parsed operand.
    fn parse_conversio_postfix(
        &mut self,
        operand: Expr,
        span_start: Span,
    ) -> Result<Expr, ParseError> {
        self.advance();
        let target = self.parse_conversio_target()?;
        let type_params = if matches!(target, ConversioTarget::Explicit(_)) {
            self.try_parse_type_args()?
        } else {
            Vec::new()
        };
        let recovery = if self.eat(&TokenKind::ExitArrow) {
            Some(Box::new(self.parse_unary()?))
        } else {
            None
        };
        if self.check_keyword_ident(TokenKind::Vel) {
            return Err(self.invalid_expression_error("conversio_recovery_uses_vel"));
        }
        let span = span_start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Conversio(ConversioExpr {
                expr: Box::new(operand),
                target,
                type_params,
                recovery,
            }),
            span,
        })
    }

    /// Parse the target of `↦`: declared type or `intervallum` clamp operand.
    fn parse_conversio_target(&mut self) -> Result<ConversioTarget, ParseError> {
        // Speculative Integer/Float range parse must restore state if no
        // range operator follows, so the token isn't silently consumed.
        if self.conversio_target_starts_inline_range() {
            let checkpoint = self.checkpoint();
            let expr = self.parse_range()?;
            if matches!(expr.kind, ExprKind::Intervallum(_)) {
                return Ok(ConversioTarget::Intervallum(Box::new(expr)));
            }
            self.restore(checkpoint);
        }

        if let TokenKind::Ident(sym) = self.peek().kind {
            let name = self.interner.resolve(sym);
            // Resolve a localized type spelling to its latin identity before
            // the conversio-type list match (`int` → `numerus` under the en
            // pack), so `↦ int<u32>` routes to an explicit type target.
            let latin = self
                .reader_type_latin_keys
                .get(name)
                .map_or(name, String::as_str);
            // Type-position keywords (`modulus`, `iuncta`) must still route to
            // an explicit type target rather than the range fallback — in Latin
            // or a reader-pack surface spelling.
            let is_type_slot_keyword = self.check_keyword_ident(TokenKind::Modulus)
                || self.check_keyword_ident(TokenKind::Iuncta);
            if !Self::is_conversio_type_keyword(latin)
                && !Self::is_capitalized_type_name(name)
                && !is_type_slot_keyword
            {
                // Use parse_range (not parse_additive) so range tails
                // like `..finem` are captured in the Intervallum target.
                let expr = self.parse_range()?;
                return Ok(ConversioTarget::Intervallum(Box::new(expr)));
            }
        }

        Ok(ConversioTarget::Explicit(self.parse_type()?))
    }

    fn conversio_target_starts_inline_range(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::Integer(_) | TokenKind::Float(_)
        )
    }

    fn is_conversio_type_keyword(name: &str) -> bool {
        if radix_types::numeric_width::is_numeric_width_type_sugar(name)
            || radix_types::numeric_width::vector_sugar_width_marker(name).is_some()
        {
            return true;
        }

        matches!(
            name,
            "ascii"
                | "textus"
                | "numerus"
                | "fractus"
                | "bivalens"
                | "nihil"
                | "vacuum"
                | "numquam"
                | "ignotum"
                | "octeti"
                | "octet"
                | "regex"
                | "json"
                | "valor"
                | "instans"
                | "lista"
                | "tabula"
                | "copia"
                | "promissum"
                | "cursor"
                | "tensor"
                | "vector"
                | "matrix"
                | "intervallum"
                | "sparsa"
                | "series"
        )
    }

    fn is_capitalized_type_name(name: &str) -> bool {
        name.chars().next().is_some_and(char::is_uppercase)
    }

    // PRIMARY EXPRESSIONS

    /// Parse primary expressions, the atoms that postfix and infix forms compose.
    ///
    /// GRAMMAR:
    ///   primary := literal | ident | array | json | paren | closure | collection-dsl
    ///   literal := integer | float | string | bool | nil
    ///   array := `[` (expr | `sparge` expr) (',' (expr | `sparge` expr))* `]`
    ///   json := '{' member (',' member)* '}'
    ///   paren := `(` expr `)`
    ///   closure := `clausura` params ['→' type] ('{' block '}' | ':' expr)
    ///
    /// This is the only expression entry point that accepts literals, object and
    /// array literals, builtin expression forms, `vacua`, and constructor-looking
    /// object initialization. Anything that starts with a non-primary token must
    /// be introduced by an earlier precedence layer.
    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        if let Some(expr) = self.try_parse_compact_clausura_expr()? {
            return Ok(expr);
        }

        let start = self.current_span();
        let id = self.next_id();

        let kind = match self.peek().kind.clone() {
            TokenKind::Integer(n) => {
                self.advance();
                ExprKind::Literal(Literal::Int(n))
            }
            TokenKind::Float(n) => {
                self.advance();
                ExprKind::Literal(Literal::Float(n))
            }
            TokenKind::String(s) => {
                self.advance();
                ExprKind::Literal(Literal::String(s))
            }
            TokenKind::AsciiString(s) => {
                self.advance();
                ExprKind::Literal(Literal::Ascii(s))
            }
            TokenKind::BacktickString(s) => {
                self.advance();
                ExprKind::Literal(Literal::Forma(s))
            }
            TokenKind::OctetiString(s) => {
                self.advance();
                ExprKind::Literal(Literal::Octeti(s))
            }

            TokenKind::Ident(_) | TokenKind::Tag | TokenKind::Solum => {
                // `iuncta` is tokenless, so the constructor slot claims the
                // spelling here — Latin text or a reader-pack surface.
                if self.check_keyword_ident(TokenKind::Iuncta) {
                    return self.parse_iuncta_expr();
                }
                if self.check_keyword_ident(TokenKind::Ego) {
                    let span = self.peek().span;
                    self.advance();
                    return Ok(Expr {
                        id,
                        kind: ExprKind::Ego(span),
                        span: start.merge(self.previous_span()),
                    });
                }
                if self.check_keyword_ident(TokenKind::Praefixum) {
                    return self.parse_praefixum_expr();
                }
                if self.check_keyword_ident(TokenKind::Clausura) {
                    return self.parse_clausura_expr();
                }
                // Expression-form `ad` is blockless — a following `{` belongs
                // to the statement parser.
                if self.check_keyword_ident(TokenKind::Ad) {
                    return self.parse_ad_expr();
                }
                // `scriptum` is claimed only when followed by `(` so the
                // spelling stays usable as an ordinary identifier (`discerne
                // format { … }`). `lege`/`lineam` are bare value builtins.
                if self.check_keyword_ident(TokenKind::Scriptum)
                    && self.peek_at(1).kind == TokenKind::LParen
                {
                    return self.parse_scriptum_expr();
                }
                if self.check_keyword_ident(TokenKind::Lege)
                    || self.check_keyword_ident(TokenKind::Lineam)
                {
                    return self.parse_lege_expr();
                }
                if self.check_keyword_ident(TokenKind::Verum)
                    || self.check_keyword_ident(TokenKind::Falsum)
                    || self.check_keyword_ident(TokenKind::Nihil)
                {
                    let literal = if self.check_keyword_ident(TokenKind::Verum) {
                        Literal::Bool(true)
                    } else if self.check_keyword_ident(TokenKind::Falsum) {
                        Literal::Bool(false)
                    } else {
                        Literal::Nil
                    };
                    self.advance();
                    return Ok(Expr {
                        id,
                        kind: ExprKind::Literal(literal),
                        span: start.merge(self.previous_span()),
                    });
                }
                let ident = self.parse_primary_ident()?;
                // Locale-aware `vacua` claim: the empty-collection marker
                // claims only through the pack-authoritative surface gate.
                // Under `la`/no-pack the registry-direct fallback lets the
                // Latin spelling claim; under a non-`la` pack only a spelling
                // the lexer recorded a latin key for (the pack's own `vacua`
                // row) claims — an undeclared Latin `vacua` stays an ordinary
                // identifier.
                if self.keyword_claim_key(ident.name) == Some("vacua") {
                    ExprKind::Vacua(ident.span)
                } else if let Some(constructor) =
                    self.try_parse_qualified_typed_constructor(&ident)?
                {
                    constructor
                } else if self.check(&TokenKind::LBrace)
                    && self.looks_like_typed_constructor_fields()
                {
                    let type_span = ident.span;
                    self.advance();
                    let fields = self.parse_object_fields()?;
                    self.expect(&TokenKind::RBrace, "expected '}'")?;
                    let object = Expr {
                        id: self.next_id(),
                        kind: ExprKind::Object(ObjectExpr { fields }),
                        span: ident.span.merge(self.previous_span()),
                    };
                    let ty = TypeExpr {
                        nullable: false,
                        mode: None,
                        kind: TypeExprKind::Named(ident, Vec::new()),
                        span: type_span,
                    };
                    ExprKind::Verte(VerteExpr {
                        expr: Box::new(object),
                        ty,
                    })
                } else {
                    ExprKind::Ident(ident)
                }
            }

            TokenKind::LBracket => {
                self.advance();
                let elements = self.parse_array_elements()?;
                self.expect(&TokenKind::RBracket, "expected ']'")?;
                ExprKind::Array(ArrayExpr { elements })
            }

            // Bare `{ }` is JSON grammar, not a Faber anonymous record.
            // Genus construction (`Type { ... }`) keeps Faber `=` grammar via
            // the `Ident` dispatch above.
            TokenKind::LBrace => {
                let start = self.current_span();
                self.advance();
                self.parse_json_literal(start)?
            }

            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expression()?;
                self.expect(&TokenKind::RParen, "expected ')'")?;
                ExprKind::Paren(Box::new(inner))
            }

            _ => {
                return Err(self.invalid_expression_error("expected_expression"));
            }
        };

        let span = start.merge(self.previous_span());
        Ok(Expr { id, kind, span })
    }

    fn try_parse_qualified_typed_constructor(
        &mut self,
        first: &Ident,
    ) -> Result<Option<ExprKind>, ParseError> {
        let checkpoint = self.checkpoint();
        let mut path = vec![Ident {
            name: first.name,
            span: first.span,
        }];
        while self.eat(&TokenKind::Dot) {
            if !matches!(self.peek().kind, TokenKind::Ident(_)) {
                self.restore(checkpoint);
                return Ok(None);
            }
            path.push(self.parse_ident()?);
        }
        if path.len() == 1
            || !self.check(&TokenKind::LBrace)
            || !self.looks_like_typed_constructor_fields()
        {
            self.restore(checkpoint);
            return Ok(None);
        }

        let type_span = path
            .first()
            .map(|segment| segment.span)
            .unwrap_or_default()
            .merge(path.last().map(|segment| segment.span).unwrap_or_default());
        self.advance();
        let fields = self.parse_object_fields()?;
        self.expect(&TokenKind::RBrace, "expected '}'")?;
        let object = Expr {
            id: self.next_id(),
            kind: ExprKind::Object(ObjectExpr { fields }),
            span: type_span.merge(self.previous_span()),
        };
        let ty = TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Qualified(path, Vec::new()),
            span: type_span,
        };
        Ok(Some(ExprKind::Verte(VerteExpr {
            expr: Box::new(object),
            ty,
        })))
    }

    fn parse_primary_ident(&mut self) -> Result<Ident, ParseError> {
        if self.check_keyword_ident(TokenKind::Solum) {
            let span = self.peek().span;
            self.advance();
            return Ok(self.keyword_ident("solum", span));
        }

        self.parse_ident()
    }

    fn parse_variant_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Finge, "expected 'finge'")?;

        // Qualified variant path: `finge Bonum { … }` or `finge pkg.Bonum { … }`
        // (imported union under an alias). Terminal segment names the variant.
        let mut variant = vec![self.parse_ident()?];
        while self.eat(&TokenKind::Dot) {
            variant.push(self.parse_ident()?);
        }

        let fields = if self.check(&TokenKind::LBrace) {
            self.advance();
            let mut fields = Vec::new();
            while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                let field_start = self.current_span();
                let name = self.parse_ident()?;
                self.expect_field_value_separator()?;
                let value = Box::new(self.parse_expression()?);
                let span = field_start.merge(self.previous_span());
                fields.push(FingeFieldInit { name, value, span });
                self.eat(&TokenKind::Comma);
            }
            self.expect(&TokenKind::RBrace, "expected '}'")?;
            fields
        } else {
            Vec::new()
        };

        // The cast is a full type annotation (`'∷' typeAnnotation` per the
        // EBNF), so a qualified type (`∷ pkg.Exitus`) parses through the type
        // grammar rather than a bare identifier.
        let cast = if self.eat_keyword(TokenKind::Verte) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Finge(FingeExpr {
                variant,
                fields,
                cast,
            }),
            span,
        })
    }

    /// Parse iuncta tuple construction: `iuncta<A, B> [a, b]`.
    fn parse_iuncta_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Iuncta, "expected 'iuncta'")?;

        self.expect(&TokenKind::LtAscii, "expected '<' after 'iuncta'")?;
        let type_args = self.finish_type_args_list()?;

        self.expect(
            &TokenKind::LBracket,
            "expected '[' after iuncta type arguments",
        )?;
        let mut elements = Vec::new();
        while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
            elements.push(self.parse_expression()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBracket, "expected ']' after iuncta elements")?;

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Iuncta(IunctaExpr {
                type_args,
                elements,
            }),
            span,
        })
    }

    fn parse_clausura_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Clausura, "expected 'clausura'")?;

        let mut params = Vec::new();
        loop {
            if self.check(&TokenKind::Arrow) || self.check(&TokenKind::Colon) {
                break;
            }

            params.push(self.parse_clausura_param()?);

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        let (ret, err) = self.parse_optional_clausura_signature()?;

        let body = if self.eat(&TokenKind::Colon) {
            ClausuraBody::Expr(Box::new(self.parse_expression()?))
        } else if self.check(&TokenKind::LBrace) {
            ClausuraBody::Block(self.parse_block()?)
        } else {
            return Err(self.expected_expression_error("closure_body_start"));
        };

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Clausura(ClausuraExpr {
                params,
                ret,
                err,
                body,
            }),
            span,
        })
    }

    /// Speculatively parse the compact closure form without stealing ordinary expressions.
    ///
    /// Compact closure syntax overlaps with parenthesized expressions and
    /// type-first expression starts. Failed speculation rolls back parser
    /// position and node allocation unless `∴` was consumed, in which case
    /// the user clearly wrote a closure and should receive its real error.
    /// Wrong-joint `ergo` (PARSE027) is also kept so it is not silently rolled back.
    fn try_parse_compact_clausura_expr(&mut self) -> Result<Option<Expr>, ParseError> {
        if !self.can_start_compact_clausura() {
            return Ok(None);
        }

        let saved_pos = self.pos;
        let saved_next_id = self.next_node_id;
        match self.parse_compact_clausura_expr() {
            Ok(expr) => Ok(Some(expr)),
            Err(err)
                if matches!(
                    err.kind,
                    ParseErrorKind::ErgoWrongContext | ParseErrorKind::ThereforeWrongContext
                ) || self.tokens[saved_pos..self.pos]
                    .iter()
                    .any(|token| token.kind == TokenKind::Therefore) =>
            {
                Err(err)
            }
            Err(_) => {
                self.restore((saved_pos, saved_next_id));
                Ok(None)
            }
        }
    }

    /// Whether an identifier resolves to a tokenless keyword that is grammar
    /// structure rather than expression/param material. Type-position words
    /// (`modulus`, `iuncta`) remain material; every other migrated keyword is
    /// structural and must not trigger expression speculation.
    fn ident_is_structure_keyword(&self, sym: Symbol) -> bool {
        let Some(latin) = self.keyword_claim_key(sym) else {
            // Undeclared under a pack-authoritative surface: an ordinary
            // identifier is expression material, never structure.
            return false;
        };
        matches!(
            lookup_keyword_spec(latin),
            Some(spec)
                if matches!(
                    spec.scope,
                    KeywordScope::Contextual(owners)
                        if !owners.iter().any(|o| matches!(
                            o,
                            KeywordOwner::TypeName | KeywordOwner::Constructor
                        ))
                )
        )
    }

    fn can_start_compact_clausura(&self) -> bool {
        // A tokenless keyword in a param slot is grammar structure (e.g. a
        // control-flow word following a value), not closure-param material.
        // Without this exclusion the speculation would consume `value ceterum`
        // as a closure param pair and then hard-fail on the `ergo`
        // wrong-context rejection, which is intentionally not rolled back.
        let second_slot = |kind: &TokenKind| match kind {
            TokenKind::Underscore(_) | TokenKind::Tag => true,
            TokenKind::Ident(sym) => !self.ident_is_structure_keyword(*sym),
            _ => false,
        };
        match self.peek().kind {
            TokenKind::LParen => true,
            TokenKind::Underscore(_) => second_slot(&self.peek_at(1).kind),
            TokenKind::Ident(sym) => {
                // Param-mode keywords (`de`/`in`) are claimed by their
                // ParameterMode owner in the registry; a mode marker in the
                // first closure slot means the parameter name follows.
                let is_param_mode = self.check_keyword_in_owner(KeywordOwner::ParameterMode);
                if is_param_mode {
                    match self.peek_at(1).kind {
                        TokenKind::Ident(name) => !self.ident_is_structure_keyword(name),
                        TokenKind::Underscore(_) => true,
                        _ => false,
                    }
                } else {
                    !self.ident_is_structure_keyword(sym) && second_slot(&self.peek_at(1).kind)
                }
            }
            _ => false,
        }
    }

    fn parse_compact_clausura_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let params = if self.eat(&TokenKind::LParen) {
            let params = self.parse_clausura_param_list_until(&TokenKind::RParen)?;
            self.expect(&TokenKind::RParen, "expected ')' after closure parameters")?;
            params
        } else {
            vec![self.parse_clausura_param()?]
        };

        let (ret, err) = self.parse_optional_clausura_signature()?;
        if self.check_keyword_ident(TokenKind::Ergo) {
            return Err(ParseError::structured(
                ParseErrorKind::ErgoWrongContext,
                self.current_span(),
            ));
        }
        self.expect(
            &TokenKind::Therefore,
            "expected '∴' after closure signature",
        )?;
        let body = self.parse_compact_clausura_body()?;

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Clausura(ClausuraExpr {
                params,
                ret,
                err,
                body,
            }),
            span,
        })
    }

    fn parse_clausura_param_list_until(
        &mut self,
        end: &TokenKind,
    ) -> Result<Vec<ClausuraParam>, ParseError> {
        let mut params = Vec::new();
        while !self.check(end) && !self.is_at_end() {
            params.push(self.parse_clausura_param()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        Ok(params)
    }

    fn parse_clausura_param(&mut self) -> Result<ClausuraParam, ParseError> {
        let param_start = self.current_span();
        let ty = self.parse_type()?;
        let name = self.parse_ident()?;
        let span = param_start.merge(self.previous_span());
        Ok(ClausuraParam { ty, name, span })
    }

    /// Parse expression-form blockless `ad`: `ad `route` (opener)?`
    ///
    /// GRAMMAR:
    ///   adExpr := `ad` asciiLiteral adOpener?
    ///   adOpener := `(` expression `)`
    ///
    /// WHY: `parse_primary` claims `ad` by spelling without consuming it,
    /// so this function must consume the keyword itself (matches
    /// `parse_clausura_expr`). This function must not consume a `{` block.
    fn parse_ad_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Ad, "expected 'ad'")?;
        let id = self.next_id();

        let path = match &self.peek().kind {
            TokenKind::AsciiString(s) => {
                let s = *s;
                self.advance();
                s
            }
            _ => {
                return Err(self.expected_expression_error("ad_ascii_route"));
            }
        };

        let opener = if self.eat(&TokenKind::LParen) {
            let expr = self.parse_expression()?;
            self.expect(&TokenKind::RParen, "expected ')' after ad opener")?;
            Some(Box::new(expr))
        } else {
            None
        };

        if self.check(&TokenKind::LBrace) {
            return Err(self.invalid_expression_error("expression_ad_block"));
        }

        let span = start.merge(self.previous_span());
        Ok(Expr {
            id,
            kind: ExprKind::Ad(Box::new(AdExpr { path, opener, span })),
            span,
        })
    }

    fn parse_optional_clausura_signature(
        &mut self,
    ) -> Result<(Option<TypeExpr>, Option<TypeExpr>), ParseError> {
        let ret = if self.eat(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let err = if self.eat(&TokenKind::ExitArrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        Ok((ret, err))
    }

    fn parse_compact_clausura_body(&mut self) -> Result<ClausuraBody, ParseError> {
        if self.check_keyword_ident(TokenKind::Fac) {
            return Ok(ClausuraBody::Fac(self.parse_fac_construct(false)?));
        }
        if self.check(&TokenKind::LBrace) {
            return Err(self
                .invalid_expression_error("closure_block_body")
                .with_arg("fac_body", "fac { ... }"));
        }
        Ok(ClausuraBody::Expr(Box::new(self.parse_expression()?)))
    }

    // SPECIAL EXPRESSION FORMS

    /// Parse a function-call argument list after `(` has been consumed.
    ///
    /// GRAMMAR:
    ///   args := [[`sparge`] expr (',' [`sparge`] expr)*]
    ///
    /// Arguments can use `sparge` to unpack arrays or objects. The closing `)` is
    /// left to the caller so this helper can be shared by normal and optional
    /// call postfix parsing.
    pub(super) fn parse_argument_list(&mut self) -> Result<Vec<Argument>, ParseError> {
        let mut args = Vec::new();

        while !self.check(&TokenKind::RParen) && !self.is_at_end() {
            let start = self.current_span();
            let spread = self.eat_keyword_ident(TokenKind::Sparge);
            let value = Box::new(self.parse_expression()?);
            let span = start.merge(self.previous_span());
            args.push(Argument {
                spread,
                value,
                span,
            });

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        Ok(args)
    }

    fn parse_array_elements(&mut self) -> Result<Vec<ArrayElement>, ParseError> {
        let mut elements = Vec::new();

        while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
            if self.eat_keyword_ident(TokenKind::Sparge) {
                elements.push(ArrayElement::Spread(Box::new(self.parse_expression()?)));
            } else {
                elements.push(ArrayElement::Expr(Box::new(self.parse_expression()?)));
            }

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        Ok(elements)
    }

    fn parse_object_fields(&mut self) -> Result<Vec<ObjectField>, ParseError> {
        let mut fields = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let start = self.current_span();

            if self.eat_keyword_ident(TokenKind::Sparge) {
                let expr = Box::new(self.parse_expression()?);
                let span = start.merge(self.previous_span());
                fields.push(ObjectField {
                    key: ObjectKey::Spread(expr),
                    value: None,
                    span,
                });
            } else if let TokenKind::String(s) = self.peek().kind {
                self.advance();
                self.expect_field_value_separator()?;
                let value = Some(Box::new(self.parse_expression()?));
                let span = start.merge(self.previous_span());
                fields.push(ObjectField {
                    key: ObjectKey::String(s),
                    value,
                    span,
                });
            } else if self.check(&TokenKind::LBracket) {
                self.advance();
                let key_expr = Box::new(self.parse_expression()?);
                self.expect(&TokenKind::RBracket, "expected ']'")?;
                self.expect_field_value_separator()?;
                let value = Some(Box::new(self.parse_expression()?));
                let span = start.merge(self.previous_span());
                fields.push(ObjectField {
                    key: ObjectKey::Computed(key_expr),
                    value,
                    span,
                });
            } else {
                let ident = self.parse_ident()?;
                let value = if self.eat(&TokenKind::Eq) {
                    Some(Box::new(self.parse_expression()?))
                } else {
                    None
                };
                let span = start.merge(self.previous_span());
                fields.push(ObjectField {
                    key: ObjectKey::Ident(ident),
                    value,
                    span,
                });
            }

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        Ok(fields)
    }

    // JSON literal parser (bare `{` in expression position)
    //
    // Bare `{ }` is compile-time JSON, not a Faber anonymous record. This is a
    // dedicated parser — `parse_object_fields` stays the genus `=` specialist
    // for `Type { ... }`.

    /// Parse a JSON object literal whose opening `{` has already been consumed.
    ///
    /// Caller has advanced past `{`; this method consumes the closing `}`.
    /// `start` is the `{` span, retained for future spanned diagnostics.
    fn parse_json_literal(&mut self, _start: Span) -> Result<ExprKind, ParseError> {
        let members = self.parse_json_object_members()?;
        Ok(ExprKind::Literal(Literal::JsonValor(JsonValue::Object(
            members,
        ))))
    }

    /// Parse `"key": value` members until `}` after the opening `{` was consumed.
    fn parse_json_object_members(&mut self) -> Result<Vec<JsonMember>, ParseError> {
        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let key_span = self.current_span();
            let key = self.parse_json_key()?;
            self.expect(&TokenKind::Colon, "expected ':' after JSON key")?;
            let value = self.parse_json_value()?;
            members.push(JsonMember {
                key_span,
                key,
                value,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "expected '}' after JSON object")?;
        Ok(members)
    }

    /// JSON keys must be quoted strings. Bare identifiers are rejected to keep
    /// JSON grammar unambiguous with Faber shorthand.
    fn parse_json_key(&mut self) -> Result<Symbol, ParseError> {
        match self.peek().kind {
            TokenKind::String(sym) => {
                self.advance();
                Ok(sym)
            }
            TokenKind::Ident(_) => Err(self.expected_expression_error("json_quoted_key")),
            _ => Err(self.expected_expression_error("json_string_key")),
        }
    }

    /// Parse one JSON value: object, array, string, number, `true`, `false`,
    /// `null`. The wire tokens `true`/`false`/`null` arrive as `Ident` because
    /// only `verum`/`falsum`/`nihil` are Faber keywords.
    fn parse_json_value(&mut self) -> Result<JsonValue, ParseError> {
        match &self.peek().kind {
            TokenKind::LBrace => {
                self.advance();
                self.parse_json_object_value()
            }
            TokenKind::LBracket => self.parse_json_array_value(),
            TokenKind::String(sym) => {
                let sym = *sym;
                self.advance();
                Ok(JsonValue::String(sym))
            }
            TokenKind::Integer(n) => {
                let n = *n;
                self.advance();
                // JSON numbers keep a signed i64 wire contract; the u64
                // magnitude crosses only through this checked boundary.
                let n = i64::try_from(n)
                    .map_err(|_| self.json_unexpected_error("json_integer_overflow"))?;
                Ok(JsonValue::Numerus(n))
            }
            TokenKind::Float(n) => {
                let n = *n;
                self.advance();
                Ok(JsonValue::Fractus(n))
            }
            TokenKind::Minus => self.parse_json_negative_number(),
            TokenKind::Ident(sym) => {
                let sym = *sym;
                let text = self.interner.resolve(sym).to_owned();
                self.advance();
                match text.as_str() {
                    "true" => Ok(JsonValue::Bool(true)),
                    "false" => Ok(JsonValue::Bool(false)),
                    "null" => Ok(JsonValue::Null),
                    _ if lookup_keyword_spec(&text).is_some() => {
                        // A Faber keyword leaked into a JSON island — tokenless
                        // keywords like `verum` arrive as identifiers. JSON
                        // values use wire tokens `true`/`false`/`null`.
                        Err(self.json_unexpected_error("json_faber_keyword_value"))
                    }
                    _ => Err(self.json_unexpected_error("json_unknown_identifier_value")),
                }
            }
            _ => Err(self.json_unexpected_error("expected_json_value")),
        }
    }

    /// Parse a nested JSON object value `{ ... }` inside an array or another
    /// object. The opening `{` has already been consumed by `parse_json_value`.
    fn parse_json_object_value(&mut self) -> Result<JsonValue, ParseError> {
        let members = self.parse_json_object_members()?;
        Ok(JsonValue::Object(members))
    }

    /// Parse a nested JSON array value `[ ... ]`. Arrays appear only as values
    /// inside objects (never at expression root, which stays Faber `lista<T>`).
    fn parse_json_array_value(&mut self) -> Result<JsonValue, ParseError> {
        self.advance();
        let mut elements = Vec::new();
        while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
            elements.push(self.parse_json_value()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBracket, "expected ']' after JSON array")?;
        Ok(JsonValue::Array(elements))
    }

    /// Parse a negative JSON number. `-7` arrives as `Minus` + `Integer(7)`;
    /// assemble the signed value. The lexer validated the digit run and the
    /// u64 range of the magnitude; the signed wire range is checked here.
    fn parse_json_negative_number(&mut self) -> Result<JsonValue, ParseError> {
        self.advance();
        match &self.peek().kind {
            TokenKind::Integer(n) => {
                let n = *n;
                self.advance();
                // The magnitude must fit the signed JSON wire domain. The
                // i64::MIN magnitude (2^63) is rejected rather than wrapped.
                let n = i64::try_from(n)
                    .map_err(|_| self.json_unexpected_error("json_integer_underflow"))?;
                Ok(JsonValue::Numerus(-n))
            }
            TokenKind::Float(n) => {
                let n = *n;
                self.advance();
                Ok(JsonValue::Fractus(-n))
            }
            _ => Err(self.json_unexpected_error("expected_json_number_after_minus")),
        }
    }

    fn json_unexpected_error(&self, issue: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::Unexpected, self.current_span())
            .with_arg("issue", issue)
    }

    fn looks_like_typed_constructor_fields(&self) -> bool {
        matches!(self.peek_at(1).kind, TokenKind::RBrace)
            || matches!(
                self.peek_at(1).kind,
                TokenKind::Ident(_) | TokenKind::String(_)
            ) && matches!(self.peek_at(2).kind, TokenKind::Eq)
    }

    fn expect_field_value_separator(&mut self) -> Result<(), ParseError> {
        if self.eat(&TokenKind::Eq) {
            return Ok(());
        }

        Err(self.expected_expression_error("field_value_separator"))
    }

    fn try_parse_type_args(&mut self) -> Result<Vec<TypeExpr>, ParseError> {
        if !self.eat(&TokenKind::LtAscii) {
            return Ok(Vec::new());
        }
        self.finish_type_args_list()
    }

    /// Parse `< type (, type)* >` after the opening `<` has been consumed.
    fn finish_type_args_list(&mut self) -> Result<Vec<TypeExpr>, ParseError> {
        let mut args = Vec::new();
        loop {
            args.push(self.parse_type()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        self.expect(&TokenKind::GtAscii, "expected '>'")?;
        Ok(args)
    }

    /// Parse required call-site type arguments after speculative `<` lookahead.
    ///
    /// The postfix loop has already confirmed `<` is present; this helper consumes
    /// it and requires a well-formed `< type (, type)* >` list before `(`.
    fn parse_call_type_args(&mut self) -> Result<Vec<TypeExpr>, ParseError> {
        if !self.eat(&TokenKind::LtAscii) {
            return Err(self.expected_expression_error("call_type_args_start"));
        }
        self.finish_type_args_list()
    }

    fn expected_expression_error(&self, issue: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::Expected, self.current_span())
            .with_arg("issue", issue)
    }

    /// Parse the `scriptum("template", args...)` formatting builtin.
    fn parse_scriptum_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Scriptum, "expected 'scriptum'")?;
        self.expect(&TokenKind::LParen, "expected '(' after 'scriptum'")?;

        let template = self.parse_string()?;

        let mut args = Vec::new();
        while self.eat(&TokenKind::Comma) {
            args.push(self.parse_expression()?);
        }

        self.expect(&TokenKind::RParen, "expected ')'")?;

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Scriptum(ScriptumExpr { template, args }),
            span,
        })
    }

    /// Parse `lege` and `lineam` input builtins.
    fn parse_lege_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let line = if self.eat_keyword_ident(TokenKind::Lineam) {
            true
        } else {
            self.expect_keyword_ident(TokenKind::Lege, "expected 'lege' or 'lineam'")?;
            false
        };

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Lege(LegeExpr { line, span }),
            span,
        })
    }

    /// Parse `praefixum(expr)`, the source-level compile-time evaluation marker.
    fn parse_praefixum_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Praefixum, "expected 'praefixum'")?;
        self.expect(&TokenKind::LParen, "expected '(' after 'praefixum'")?;

        let body = PraefixumBody::Expr(Box::new(self.parse_expression()?));

        self.expect(&TokenKind::RParen, "expected ')'")?;

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Praefixum(PraefixumExpr { body }),
            span,
        })
    }
}

#[cfg(test)]
#[path = "expr_test.rs"]
mod tests;
