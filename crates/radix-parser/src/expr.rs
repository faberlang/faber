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
//! - Speculative parses (compact closures, call type arguments, `↦` targets,
//!   long-form and qualified typed constructors) roll back through
//!   `checkpoint`/`restore`, which rewind token position, node-id allocation,
//!   and recorded errors.
//! - `vacua` is recognized as a value-level empty collection marker; required
//!   collection types are supplied by declaration/type contexts upstream.
//! - Field separators in object-like expression syntax are canonical `=`, not
//!   colon aliases.

use super::{ParseError, ParseErrorKind, Parser};
use radix_lexer::{KeywordOwner, KeywordScope, Span, Symbol, TokenKind, lookup_keyword_spec};
#[allow(clippy::wildcard_imports)]
use radix_syntax::*;
use std::collections::HashSet;

/// Internal HIR bridge for the source-only `ᵀ` suffix. Semantic checking
/// rewrites this marker to the canonical `transpone` intrinsic after applying
/// the glyph's stricter rank law.
pub(super) const GLYPH_TRANSPOSE_METHOD: &str = "__faber_glyph_transpose";

#[derive(Clone, Copy)]
enum PostfixFamily {
    Optional,
    NonNull,
}

#[derive(Clone, Copy)]
enum PostfixPayload {
    Member,
    Index,
    Call,
}

// EXPRESSION ENTRY POINT

impl Parser {
    /// Parse an expression from the lowest-precedence level.
    ///
    /// GRAMMAR:
    ///   expression := assignment
    ///
    /// Every nested expression (`(...)`, call arguments, initializers) enters
    /// here again, so this is where the depth budget is charged. The operator
    /// budget resets only at a new root, so sub-expressions accumulate into it.
    pub(super) fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        if self.expression_contexts == 0 {
            self.folded_operators = 0;
        }
        let previous_conversio_postfix_enabled = self.conversio_postfix_enabled;
        self.conversio_postfix_enabled = true;
        self.expression_contexts = self.expression_contexts.saturating_add(1);
        let result = self.nested(
            ParseErrorKind::ExpressionInvalid,
            start,
            Self::parse_assignment,
        );
        self.expression_contexts = self.expression_contexts.saturating_sub(1);
        self.conversio_postfix_enabled = previous_conversio_postfix_enabled;
        result
    }

    /// Fold one left-associative binary node.
    ///
    /// The fold charges the operator budget here so every tier that folds in a
    /// loop (`and`/`or`/comparison/bitwise/shift as well as additive and
    /// multiplicative) is bounded by the same counter — see
    /// [`MAX_PARSE_OPERATORS`].
    fn binary_expr(
        &mut self,
        start: Span,
        lhs: Expr,
        op: BinOp,
        rhs: Expr,
    ) -> Result<Expr, ParseError> {
        self.charge_fold(start)?;
        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Binary(BinaryExpr {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            }),
            span,
        })
    }

    // PRECEDENCE CLIMBING

    /// Parse assignment expression (lowest precedence).
    ///
    /// GRAMMAR:
    ///   assignment := ternary ('←' assignment | '↤' assignment inlineDefault?)?
    ///   inlineDefault := '⊥' unary
    ///
    /// WHY: Right-associative via recursion on `RHS`. Allows `a ← b ← c` and
    /// `a ↤ b ↤ c`. `↤` is conversion-directed assignment (U+21A4) and sits at
    /// the same precedence level as `←`; the `⊥` default (D17.7) is legal only
    /// on `↤` (and after `↦ T`) and attaches to the nearest `↤` in a
    /// right-associated chain.
    ///
    /// A `⊥` left over after a complete assignment follows neither a call
    /// (whose default the postfix tier claims) nor a conversion, so it gets a
    /// structured rejection rather than a leftover-token error.
    fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_assignment_open()?;
        if self.check(&TokenKind::UpTack) {
            return Err(self.invalid_expression_error("default_requires_failable"));
        }
        Ok(expr)
    }

    /// Parse an assignment without the trailing-`⊥` rejection, so a `↤` tail
    /// can claim the `⊥` default that follows its value.
    fn parse_assignment_open(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let expr = self.parse_ternary()?;
        let expr = self.parse_copy_into(expr, start)?;
        let expr = self.parse_sink_into(expr, start)?;
        self.parse_assignment_tail(expr, start)
    }

    /// Parse the `⇇` (U+21C7) copy-into suffix at the locked low precedence —
    /// below assignment, above ternary.
    ///
    /// GRAMMAR:
    ///   transfer := ternary ('⇇' ternary)?
    ///
    /// WHY: `target ⇇ value` copies every value of `value` into the existing
    /// storage of `target`. The target is the complete expression already
    /// parsed (calls, conversions, and string-constructor applications
    /// finished inside `parse_ternary`'s postfix level); the value is a
    /// complete ternary so binary operators group into the value, not around
    /// the statement. The form is a statement, not a pipeline of results, so a
    /// second `⇇` is rejected here rather than parsed as a chain.
    fn parse_copy_into(&mut self, target: Expr, start: Span) -> Result<Expr, ParseError> {
        if !self.check(&TokenKind::Transfer) {
            return Ok(target);
        }

        let target = Box::new(target);
        self.advance();
        if !Self::can_start_expression(&self.peek().kind) {
            return Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "copy_into_value_expected")
                    .with_arg("operator", "⇇"),
            );
        }
        let value = Box::new(self.parse_ternary()?);
        if self.check(&TokenKind::Transfer) {
            return Err(self.invalid_expression_error("copy_into_chain"));
        }

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::CopyInto(CopyIntoExpr { target, value }),
            span,
        })
    }

    /// Parse the `⇜` (U+21DC) sink-into suffix at the locked low precedence —
    /// below assignment, above ternary.
    ///
    /// GRAMMAR:
    ///   `sink_into` := ternary ('⇜' ternary)?
    ///
    /// WHY: `sink ⇜ item` delivers one `item` to the sinking contract of
    /// `sink`. The sink is the complete expression already parsed (calls,
    /// conversions, and string-constructor applications finished inside
    /// `parse_ternary`'s postfix level); the item is a complete ternary so
    /// binary operators group into the item, not around the statement. One `⇜`
    /// is one write, so a second `⇜`, a following `⇇`, or a sink that is
    /// already a copy-into is rejected here as a chain.
    fn parse_sink_into(&mut self, sink: Expr, start: Span) -> Result<Expr, ParseError> {
        if !self.check(&TokenKind::SinkInto) {
            return Ok(sink);
        }
        if matches!(sink.kind, ExprKind::CopyInto(_)) {
            return Err(self.invalid_expression_error("sink_into_chain"));
        }

        let sink = Box::new(sink);
        self.advance();
        if !Self::can_start_expression(&self.peek().kind) {
            return Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "sink_into_item_expected")
                    .with_arg("operator", "⇜"),
            );
        }
        let item = Box::new(self.parse_ternary()?);
        if self.check(&TokenKind::SinkInto) || self.check(&TokenKind::Transfer) {
            return Err(self.invalid_expression_error("sink_into_chain"));
        }

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::SinkInto(SinkIntoExpr { sink, item }),
            span,
        })
    }

    /// Continue an assignment whose left operand is already parsed.
    ///
    /// Shared by the expression grammar (ternary LHS) and typed declaration
    /// initializers (binding LHS) so the `↤` contract — same precedence as
    /// `←`, right-associative, `⊥` default on `↤` only — lives in one place.
    /// `⇥` never takes a value (D17.1): a `⇥` here is the retired inline
    /// recovery spelling and gets the migration diagnostic.
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
        let is_conversio = matches!(op, AssignOp::ConversioAssign);
        let value_marker = if matches!(op, AssignOp::Assign) {
            self.parse_value_marker(Some(TypeMode::Copy))
        } else {
            None
        };
        let parse_value: fn(&mut Self) -> Result<Expr, ParseError> = if is_conversio {
            Self::parse_assignment_open
        } else {
            Self::parse_assignment
        };
        let value = Box::new(self.nested(
            ParseErrorKind::ExpressionInvalid,
            self.current_span(),
            parse_value,
        )?);

        if self.check(&TokenKind::ExitArrow) {
            return Err(self.invalid_expression_error("recovery_uses_exit_arrow"));
        }
        let via = if is_conversio {
            self.parse_via_clause()?
        } else {
            None
        };
        if via.is_some() && self.check(&TokenKind::ExitArrow) {
            return Err(self.invalid_expression_error("recovery_uses_exit_arrow"));
        }
        let recovery = if is_conversio {
            self.parse_inline_default()?
        } else {
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
                value_marker,
                recovery,
                via,
            }),
            span,
        })
    }

    /// Parse the conditional tier.
    ///
    /// GRAMMAR:
    ///   ternary := or ['✓' expr '✗' ternary]
    ///
    /// WHY: `c ✓ a ✗ b` (D4.5) is the one value conditional, the same glyphs
    /// in every locale. It is one level only (D4.6): a `✓ ✗` anywhere inside
    /// another is rejected with `conditional_nested`. The else branch still
    /// recurses through this tier so a chained `✗ d ✓ …` reaches that named
    /// diagnostic instead of a leftover-token error. The removed spellings
    /// `? :` and `sic … secus` (D4.7) get migration diagnostics here; `sic`
    /// is an ordinary identifier recognised by its text alone.
    fn parse_ternary(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let first_inner_id = self.next_node_id;
        let cond = self.parse_or()?;

        if self.eat(&TokenKind::Check) {
            return self.parse_check_ballot_tail(cond, start, first_inner_id);
        }
        if self.check(&TokenKind::Question) {
            return Err(self.invalid_expression_error("conditional_question_colon_removed"));
        }
        if self.at_retired_sic_conditional() {
            return Err(self.invalid_expression_error("conditional_sic_removed"));
        }

        Ok(cond)
    }

    /// Whether the current word is the retired `sic` of `c sic a secus b`.
    ///
    /// FABER-2-SUNSET: delete with the retired-word diagnostics.
    ///
    /// `sic` left the keyword registry (KW-1), so it is recognised by identifier
    /// text, in the spelling each reader pack gave it. The shape guard keeps a
    /// variable that happens to be named `sic` legal: the word must sit on the
    /// operand's line and be followed by a token that starts a value.
    fn at_retired_sic_conditional(&self) -> bool {
        // FABER-2-SUNSET: delete with the retired-word diagnostics.
        const RETIRED_SIC_SPELLINGS: [&str; 8] =
            ["sic", "yields", "乃", "如此", "thế", "فإذا", "ऐसा", "เช่นนั้น"];
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        RETIRED_SIC_SPELLINGS.contains(&self.interner.resolve(sym))
            && !self.raw_newline_before_next_word()
            && Self::can_start_expression(&self.peek_at(1).kind)
    }

    /// Finish `cond ✓ then ✗ else` after the `✓` (D4.5, D4.6).
    ///
    /// Kept out of line so the deeply recursive conditional tier keeps a
    /// small stack frame.
    #[inline(never)]
    fn parse_check_ballot_tail(
        &mut self,
        cond: Expr,
        start: Span,
        first_inner_id: NodeId,
    ) -> Result<Expr, ParseError> {
        let then = Box::new(self.parse_expression()?);
        self.expect_issue(&TokenKind::Ballot, "conditional_else_marker")?;
        let else_span = self.current_span();
        let else_ = Box::new(self.nested(
            ParseErrorKind::ExpressionInvalid,
            else_span,
            Self::parse_ternary,
        )?);
        if let Some(&(_, inner)) = self
            .conditionals
            .iter()
            .find(|&&(id, _)| id >= first_inner_id)
        {
            return Err(
                ParseError::structured(ParseErrorKind::ExpressionInvalid, inner)
                    .with_arg("issue", "conditional_nested"),
            );
        }
        let span = start.merge(self.previous_span());
        let id = self.next_id();
        self.conditionals.push((id, span));
        Ok(Expr {
            id,
            kind: ExprKind::Ternary(TernaryExpr {
                cond: Box::new(cond),
                then,
                else_,
            }),
            span,
        })
    }

    /// Logical OR (`aut` only). Nullable defaulting `vel` is a tighter tier.
    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_and()?;

        while self.check_keyword_ident(TokenKind::Aut) {
            self.advance();
            let right = self.parse_and()?;
            left = self.binary_expr(start, left, BinOp::Or, right)?;
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_equality()?;

        while self.check_keyword_ident(TokenKind::Et) {
            self.advance();
            let right = self.parse_equality()?;
            left = self.binary_expr(start, left, BinOp::And, right)?;
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
                    TokenKind::EqEq => BinOp::Eq,                 // ≡
                    TokenKind::ApproxEq => BinOp::ApproxEq,       // ≅
                    TokenKind::ApproxNotEq => BinOp::ApproxNotEq, // ≇
                    TokenKind::FuzzyEq => BinOp::FuzzyEq,         // ≈
                    TokenKind::FuzzyNotEq => BinOp::FuzzyNotEq,   // ≉
                    TokenKind::BangEq => BinOp::NotEq,            // ≢
                    TokenKind::BangEqRetired => {
                        self.record_retired_neq_glyph();
                        BinOp::NotEq
                    }
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

            // `est`/`non est` is a type test only (DECISIONS D23.1): the
            // right-hand side is always parsed as a type, so there is no
            // type-vs-value lookahead here (D23.5). A right-hand side that
            // names a value is a semantic error naming the equality operator,
            // not a second meaning for `est`.
            let positive = matches!(op, BinOp::Is);
            if matches!(op, BinOp::Is | BinOp::IsNot) {
                self.charge_fold(start)?;
                let ty = self.parse_est_type_rhs()?;
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
            left = self.binary_expr(start, left, op, right)?;
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_format()?;

        loop {
            let op = match self.peek().kind {
                // Canonical strict comparison is `Lt`/`Gt` (≺/≻); `<`/`>` are
                // generic delimiters only. An ASCII one in operator position is
                // still parsed as the comparison so recovery continues, but it
                // records `PARSE030 comparison_ascii_retired`.
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::LtAscii => {
                    self.record_retired_ascii_comparison("<", "≺");
                    BinOp::Lt
                }
                TokenKind::GtAscii => {
                    self.record_retired_ascii_comparison(">", "≻");
                    BinOp::Gt
                }
                TokenKind::LtEq => BinOp::LtEq,
                TokenKind::GtEq => BinOp::GtEq,
                TokenKind::ElementOf => BinOp::Member,
                TokenKind::NotElementOf => BinOp::NotMember,
                _ if self.check_keyword_in_owner(KeywordOwner::RetiredMembership) => {
                    self.record_retired_membership_keyword();
                    BinOp::Member
                }
                _ => break,
            };
            self.advance();
            let right = self.parse_format()?;
            left = self.binary_expr(start, left, op, right)?;
        }

        Ok(left)
    }

    /// Record the migration diagnostic for an ASCII `<` / `>` in comparison
    /// position: `PARSE030 comparison_ascii_retired`, args `operator` (as
    /// written) and `glyph` (`≺` / `≻`).
    ///
    /// FABER-2-SUNSET: delete with the retired-word diagnostics.
    ///
    /// The caller still parses the operand as the comparison, so one file
    /// reports every site. Generic angle brackets never reach this tier.
    fn record_retired_ascii_comparison(&mut self, operator: &str, glyph: &str) {
        let span = self.current_span();
        // A speculative sub-parse that is re-run must not report the same operator twice.
        if self.errors.iter().any(|err| {
            err.span == span
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "comparison_ascii_retired")
        }) {
            return;
        }
        self.errors.push(
            ParseError::structured(ParseErrorKind::ExpressionInvalid, span)
                .with_arg("issue", "comparison_ascii_retired")
                .with_arg("operator", operator)
                .with_arg("glyph", glyph),
        );
    }

    /// Record the migration diagnostic for the retired inequality glyph `≠`
    /// (U+2260) in equality position: `PARSE030 neq_glyph_retired`, args
    /// `glyph` (as written) and `replacement` (`≢`, the one negation of `≡`).
    ///
    /// FABER-2-SUNSET: delete with the retired-word diagnostics.
    ///
    /// The caller still parses the operand as `NotEq`, so one file reports
    /// every site.
    fn record_retired_neq_glyph(&mut self) {
        let span = self.current_span();
        // A speculative sub-parse that is re-run must not report the same glyph twice.
        if self.errors.iter().any(|err| {
            err.span == span
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "neq_glyph_retired")
        }) {
            return;
        }
        self.errors.push(
            ParseError::structured(ParseErrorKind::ExpressionInvalid, span)
                .with_arg("issue", "neq_glyph_retired")
                .with_arg("glyph", "≠")
                .with_arg("replacement", "≢"),
        );
    }

    /// Record the migration diagnostic for a retired membership word
    /// (`intra` / `inter` and every reader pack's spelling of them) sitting in
    /// operator position: `PARSE001 membership_keyword_retired`, args `keyword`
    /// (as written) and `glyph` (`∈`).
    ///
    /// The caller still parses the operand as `Member`, so recovery continues
    /// past the word and nothing downstream runs on a recorded error (the
    /// `static_decl_retired` pattern). The words stay ordinary identifiers
    /// everywhere else; only this operator slot claims them.
    fn record_retired_membership_keyword(&mut self) {
        let span = self.current_span();
        let keyword = match self.peek().kind {
            TokenKind::Ident(sym) => self.interner.resolve(sym).to_owned(),
            _ => String::new(),
        };
        // A speculative sub-parse that is re-run must not report the same word twice.
        if self.errors.iter().any(|err| {
            err.span == span
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "membership_keyword_retired")
        }) {
            return;
        }
        self.errors.push(
            ParseError::structured(ParseErrorKind::Expected, span)
                .with_arg("issue", "membership_keyword_retired")
                .with_arg("keyword", keyword)
                .with_arg("glyph", "∈"),
        );
    }

    /// Parse the format tier.
    ///
    /// GRAMMAR:
    ///   format := `bitwise_or` ['¶' STRING]
    ///
    /// WHY: `¶` (D2.1) is looser than arithmetic and tighter than comparison,
    /// so `a + b ¶ ".2"` is `(a + b) ¶ ".2"` (D2.5). The spec must be a string
    /// literal (D2.2): pass 1 checks only that one is present. `¶` does not
    /// chain; a second `¶` is `format_chained`.
    fn parse_format(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let value = self.parse_bitwise_or()?;
        if !self.eat(&TokenKind::Pilcrow) {
            return Ok(value);
        }
        let spec_span = self.current_span();
        let TokenKind::String(spec) = self.peek().kind else {
            return Err(
                ParseError::structured(ParseErrorKind::ExpressionInvalid, spec_span)
                    .with_arg("issue", "format_spec_not_literal"),
            );
        };
        self.advance();
        if self.check(&TokenKind::Pilcrow) {
            return Err(ParseError::structured(
                ParseErrorKind::ExpressionInvalid,
                self.current_span(),
            )
            .with_arg("issue", "format_chained"));
        }
        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Format(FormatExpr {
                value: Box::new(value),
                spec,
                spec_span,
            }),
            span,
        })
    }

    fn parse_bitwise_or(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_bitwise_xor()?;

        while self.check(&TokenKind::BitwiseOr) {
            self.advance();
            let right = self.parse_bitwise_xor()?;
            left = self.binary_expr(start, left, BinOp::BitOr, right)?;
        }

        Ok(left)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_bitwise_and()?;

        while self.check(&TokenKind::BitwiseXor) {
            self.advance();
            let right = self.parse_bitwise_and()?;
            left = self.binary_expr(start, left, BinOp::BitXor, right)?;
        }

        Ok(left)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_shift()?;

        while self.check(&TokenKind::BitwiseAnd) {
            self.advance();
            let right = self.parse_shift()?;
            left = self.binary_expr(start, left, BinOp::BitAnd, right)?;
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
            left = self.binary_expr(start, left, op, right)?;
        }

        Ok(left)
    }

    /// Consume the range operator at the cursor, if any: `‥`/`ante` exclusive,
    /// `…`/`usque` inclusive. Shared by full range literals and the `vel`
    /// fallback interval so both accept the same spellings.
    fn parse_range_kind(&mut self) -> Option<RangeKind> {
        if self.check(&TokenKind::DotDot) {
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
        }
    }

    fn parse_range(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let left = self.parse_range_bound()?;

        let kind = self.parse_range_kind();

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
        let previous = self.conversio_postfix_enabled;
        self.conversio_postfix_enabled = false;
        let result = self.parse_additive_inner();
        self.conversio_postfix_enabled = previous;
        result
    }

    /// Parse additive arithmetic and infix extrema at one left-associative tier.
    ///
    /// GRAMMAR:
    ///   additive := multiplicative (('+' | '-' | '⤒' | '⤓') multiplicative)*
    ///
    /// WHY: `⤒` and `⤓` are pure arithmetic extrema, not multiplicative
    /// operators. Keeping them in this loop makes `a ⤒ b ⤓ c` left-nested and
    /// leaves the operator identity available for D3's semantic lowering.
    fn parse_additive_inner(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_multiplicative_inner()?;

        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                TokenKind::Maximum => BinOp::Maximum,
                TokenKind::Minimum => BinOp::Minimum,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative_inner()?;
            left = self.binary_expr(start, left, op, right)?;
        }

        Ok(left)
    }

    /// Multiplicative over local nullable defaulting (`vel`).
    ///
    /// GRAMMAR:
    ///   multiplicative := coalesce (('*' | '/' | '%' | '÷' | '·' | '×' | '⊗' | '⊙' | '⊘') coalesce)*
    ///   coalesce       := unary (`vel` `vel_rhs`)*
    ///   `vel_rhs`        := unary (`range_op` unary (`per` unary)?)?
    ///
    /// `vel` is local nullable elimination: tighter than `+` and `*`, so
    /// `prefix + item vel ""` and `scale * factor vel 1` both default only the
    /// adjacent term. The `vel` `RHS` may complete an interval constructor
    /// (`0‥0`) so fallback ranges are not split as `(x vel 0)‥0`.
    fn parse_multiplicative_inner(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_coalesce_inner()?;

        loop {
            let op = match self.peek().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Obelus => BinOp::TrueDiv,
                TokenKind::Percent => BinOp::Mod,
                TokenKind::MiddleDot => BinOp::Dot,
                TokenKind::Cross => BinOp::Cross,
                TokenKind::CircledTimes => BinOp::TensorProduct,
                TokenKind::CircledDot => BinOp::Hadamard,
                TokenKind::CircledDivide => BinOp::HadamardDiv,
                _ => break,
            };
            self.advance();
            let right = self.parse_coalesce_inner()?;
            let binary = self.binary_expr(start, left, op, right)?;
            left = self.reassociate_gradient_selection(binary);
        }

        Ok(left)
    }

    /// The settled tensor-glyph precedence law keeps a postfix transpose inside
    /// the selected value: `a · bᵀ ∇ [x]` is `(a · bᵀ) ∇ [x]`, not
    /// `a · (bᵀ ∇ [x])`. The ordinary gradient parser still owns the suffix;
    /// this narrow reassociation only moves a gradient that was consumed while
    /// parsing the right multiplicative operand.
    fn reassociate_gradient_selection(&mut self, expr: Expr) -> Expr {
        let Expr {
            id,
            kind: ExprKind::Binary(BinaryExpr { op, lhs, rhs }),
            span,
        } = expr
        else {
            return expr;
        };
        let ExprKind::Gradient(gradient) = rhs.kind else {
            return Expr {
                id,
                kind: ExprKind::Binary(BinaryExpr { op, lhs, rhs }),
                span,
            };
        };
        let call_span = gradient.call.span;
        let inner = Expr {
            id,
            kind: ExprKind::Binary(BinaryExpr {
                op,
                lhs,
                rhs: gradient.call,
            }),
            span: span.merge(call_span),
        };
        Expr {
            id: self.next_id(),
            kind: ExprKind::Gradient(GradientExpr {
                call: Box::new(inner),
                selectors: gradient.selectors,
            }),
            span,
        }
    }

    /// Local nullable elimination (`vel`). Left-associative: `a vel b vel c`.
    fn parse_coalesce_inner(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_unary_inner()?;

        while self.check_keyword_ident(TokenKind::Vel) {
            self.advance();
            let right = self.parse_vel_rhs()?;
            left = self.binary_expr(start, left, BinOp::Coalesce, right)?;
        }

        Ok(left)
    }

    /// `RHS` of `vel`: a unary value, or an interval constructor `lo‥hi` / `lo…hi`.
    ///
    /// Interval bounds stay unary-tight so broad bound expressions require
    /// parentheses: `maybeRange vel (lo + 1‥hi - 1)`.
    fn parse_vel_rhs(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let left = self.parse_unary_inner()?;

        if let Some(kind) = self.parse_range_kind() {
            let end = Box::new(self.parse_unary_inner()?);
            let step = if self.eat_keyword_ident(TokenKind::Per) {
                Some(Box::new(self.parse_unary_inner()?))
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
    ///   unary-op := '-' | '¬' | `non` | '√' | '∛' | '∜'
    ///
    /// EDGE: `finge` is a unary-position variant constructor, not a prefix operator.
    fn parse_unary_inner(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();

        self.reject_retired_literal_prefix_predicate()?;

        let op = match self.peek().kind {
            TokenKind::Minus => Some(UnOp::Neg),
            TokenKind::BitwiseNot => Some(UnOp::BitNot),
            TokenKind::RadicalSqrt => Some(UnOp::RadicalSqrt),
            TokenKind::RadicalCbrt => Some(UnOp::RadicalCbrt),
            TokenKind::RadicalFourth => Some(UnOp::RadicalFourth),
            _ if self.check_keyword_ident(TokenKind::Non) => Some(UnOp::Not),
            _ => None,
        };

        if let Some(op) = op {
            self.advance();
            // A prefix chain nests one `unary` level per operator and never
            // re-enters `parse_expression`, so the depth budget is charged at
            // this recursion rather than at the function entry — the same shape
            // as the right-associative `? :` and `←` recursions.
            let operand = Box::new(self.nested(
                ParseErrorKind::ExpressionInvalid,
                start,
                super::Parser::parse_unary_inner,
            )?);
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

        if matches!(self.peek().kind, TokenKind::FloorOpen | TokenKind::CeilOpen) {
            return self.parse_rounding_glyph();
        }

        self.parse_postfix_inner()
    }

    /// Parse the paired rounding delimiters.
    ///
    /// GRAMMAR:
    ///   rounding := '⌊' expression '⌋'   floor
    ///             | '⌈' expression '⌉'   ceiling
    ///             | '⌊' expression '⌉'   nearest (ties to the even neighbour)
    ///
    /// `⌈x⌋` is a named error (`PARSE030 ceiling_floor_delimiters`) that
    /// points at `⌊x⌉`; the parse continues as nearest so one file reports
    /// every site.
    fn parse_rounding_glyph(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let open = self.peek().kind.clone();
        self.advance();
        let operand = Box::new(self.parse_expression()?);
        let close_span = self.current_span();
        let op = match (&open, &self.peek().kind) {
            (TokenKind::FloorOpen, TokenKind::FloorClose) => UnOp::Floor,
            (TokenKind::CeilOpen, TokenKind::CeilClose) => UnOp::Ceil,
            (TokenKind::FloorOpen, TokenKind::CeilClose) => UnOp::Nearest,
            (TokenKind::CeilOpen, TokenKind::FloorClose) => {
                self.errors.push(
                    ParseError::structured(
                        ParseErrorKind::ExpressionInvalid,
                        start.merge(close_span),
                    )
                    .with_arg("issue", "ceiling_floor_delimiters")
                    .with_arg("replacement", "⌊x⌉"),
                );
                UnOp::Nearest
            }
            _ => {
                return Err(ParseError::structured(
                    ParseErrorKind::UnterminatedGroup,
                    start,
                ));
            }
        };
        self.advance();
        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Unary(UnaryExpr { op, operand }),
            span,
        })
    }

    fn can_start_expression(kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Ident(_)
                | TokenKind::Underscore(_)
                | TokenKind::Integer(_)
                | TokenKind::BigInteger(_)
                | TokenKind::Float(_)
                | TokenKind::FloatText { .. }
                | TokenKind::String(_)
                | TokenKind::AsciiString(_)
                | TokenKind::BacktickString(_)
                | TokenKind::OctetiString(_)
                | TokenKind::RegexString(_)
                | TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::LBrace
                | TokenKind::Solum
                | TokenKind::Minus
                | TokenKind::BitwiseNot
                | TokenKind::RadicalSqrt
                | TokenKind::RadicalCbrt
                | TokenKind::RadicalFourth
                | TokenKind::FloorOpen
                | TokenKind::CeilOpen
                | TokenKind::Nulla
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
    /// claims (owners `Constructor`, `BuiltinCall`, `Literal`,
    /// `ExpressionIntroducer`) start value operands; statement, declaration,
    /// and type keywords are grammar structure, not operands. This keeps the retired `nihil x` detection
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
        // A boolean literal is a value, so the rewrite is the equality operator
        // (DECISIONS D23.1 — `est` takes a type). The null *literal* (D23.2:
        // `nulla`) is a value too, but its rewrite names the null *type*
        // (`nihil`), so the rewrite stays a type test.
        if self.check_keyword_ident(TokenKind::Verum) {
            Some(("verum", "value ≡ verum"))
        } else if self.check_keyword_ident(TokenKind::Falsum) {
            Some(("falsum", "value ≡ falsum"))
        } else if self.check_keyword_ident(TokenKind::Nihil) {
            Some(("nulla", "value est nihil"))
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
    ///   transpose := `ᵀ`
    ///   gradient := ∇ [ [ place (, place)* ] ]
    ///   cast := `∷` type
    ///   optional-chain := '?.' ident | '?[' expr `]` | '?(' args `)`
    ///   conversion := '↦' type [type-params] ['⊥' default]
    ///
    /// Keeping every postfix form in one loop preserves left-to-right chaining
    /// (`a.b(c)[`d`]`) and prevents casts/conversions from accidentally binding at
    /// a different level than calls or optional chaining. The gradient suffix
    /// consumes its optional selector bracket here, before the loop resumes, so
    /// later postfix operations apply to the gradient result — never to the
    /// selector list.
    fn parse_postfix_inner(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut expr = self.parse_primary()?;

        loop {
            if self.budget_exhausted {
                return Err(self.spent_budget_error(ParseErrorKind::ExpressionInvalid, start));
            }
            let has_conversio_postfix =
                self.conversio_postfix_enabled && self.check_keyword(TokenKind::Conversio);
            let has_postfix = self.check(&TokenKind::LtAscii)
                || self.check(&TokenKind::LParen)
                || self.check(&TokenKind::Dot)
                || self.check(&TokenKind::LBracket)
                || self.check(&TokenKind::Transpose)
                || self.check(&TokenKind::Nabla)
                || self.check(&TokenKind::QuestionDot)
                || self.check(&TokenKind::QuestionBracket)
                || self.check(&TokenKind::QuestionParen)
                || self.check(&TokenKind::BangDot)
                || self.check(&TokenKind::BangBracket)
                || self.check(&TokenKind::BangParen)
                || self.check_keyword(TokenKind::Verte)
                || has_conversio_postfix;
            if has_postfix {
                self.charge_fold(start)?;
            }
            if self.check(&TokenKind::LtAscii) {
                let checkpoint = self.checkpoint();
                if let Ok(type_args) = self.parse_call_type_args()
                    && self.check(&TokenKind::LParen)
                {
                    expr = self.parse_call_postfix(start, expr, type_args)?;
                    continue;
                }
                self.restore(checkpoint);
                break;
            } else if self.check(&TokenKind::LParen) {
                expr = self.parse_call_postfix(start, expr, Vec::new())?;
            } else if self.check(&TokenKind::Dot) {
                expr = self.parse_member_postfix(start, expr)?;
            } else if self.check(&TokenKind::LBracket) {
                expr = self.parse_index_postfix(start, expr)?;
            } else if self.check(&TokenKind::Transpose) {
                expr = self.parse_transpose_postfix(start, expr);
            } else if self.check(&TokenKind::Nabla) {
                expr = self.parse_gradient_postfix(start, expr)?;
            } else if self.check(&TokenKind::QuestionDot) {
                expr = self.parse_postfix_payload(
                    start,
                    expr,
                    PostfixFamily::Optional,
                    PostfixPayload::Member,
                )?;
            } else if self.check(&TokenKind::QuestionBracket) {
                expr = self.parse_postfix_payload(
                    start,
                    expr,
                    PostfixFamily::Optional,
                    PostfixPayload::Index,
                )?;
            } else if self.check(&TokenKind::QuestionParen) {
                expr = self.parse_postfix_payload(
                    start,
                    expr,
                    PostfixFamily::Optional,
                    PostfixPayload::Call,
                )?;
            } else if self.check(&TokenKind::BangDot) {
                expr = self.parse_postfix_payload(
                    start,
                    expr,
                    PostfixFamily::NonNull,
                    PostfixPayload::Member,
                )?;
            } else if self.check(&TokenKind::BangBracket) {
                expr = self.parse_postfix_payload(
                    start,
                    expr,
                    PostfixFamily::NonNull,
                    PostfixPayload::Index,
                )?;
            } else if self.check(&TokenKind::BangParen) {
                expr = self.parse_postfix_payload(
                    start,
                    expr,
                    PostfixFamily::NonNull,
                    PostfixPayload::Call,
                )?;
            } else if self.check_keyword(TokenKind::Verte) {
                expr = self.parse_verte_postfix(start, expr)?;
            } else if has_conversio_postfix {
                expr = self.parse_conversio_postfix(expr, start)?;
            } else {
                break;
            }
        }

        if matches!(expr.kind, ExprKind::Call(_)) && self.check(&TokenKind::UpTack) {
            return self.parse_call_default(start, expr);
        }
        Ok(expr)
    }

    /// Parse the `⊥ default` (D17.7) that follows a complete call chain.
    ///
    /// GRAMMAR:
    ///   callDefault := call '⊥' unary
    ///
    /// WHY: the default binds at the postfix tier, like the conversion
    /// default, so `f() ⊥ 0 + 1` is `(f() ⊥ 0) + 1`. Only a call operand is
    /// admitted here; whether the call is failable is a semantic question.
    fn parse_call_default(&mut self, start: Span, call: Expr) -> Result<Expr, ParseError> {
        self.advance();
        let default = Box::new(self.parse_unary_inner()?);
        if self.check(&TokenKind::ExitArrow) {
            return Err(self.invalid_expression_error("default_with_propagation"));
        }
        Ok(self.postfix_expr(
            start,
            ExprKind::CallDefault(CallDefaultExpr {
                call: Box::new(call),
                default,
            }),
        ))
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
        type_args: Vec<GenericArgExpr>,
    ) -> Result<Expr, ParseError> {
        self.advance();
        let allows_template_labels = matches!(
            &callee.kind,
            ExprKind::Literal(Literal::String(_) | Literal::Forma(_))
        );
        let args = self.parse_argument_list(allows_template_labels)?;
        self.expect_issue(&TokenKind::RParen, "call_arguments_end")?;
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
        // `m[r, c]` is a coordinate read spelled as one bracketed list,
        // `m[[r, c]]`. The comma after the first index is the did-you-mean
        // hinge; the receiver's rank is unknown at parse time, so the hint
        // applies to every rank-N coordinate index.
        if self.check(&TokenKind::Comma) {
            return Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "index_coordinate_comma"),
            );
        }
        self.expect_issue(&TokenKind::RBracket, "index_end")?;
        Ok(self.postfix_expr(
            start,
            ExprKind::Index(IndexExpr {
                object: Box::new(object),
                index,
            }),
        ))
    }

    /// Parse the source-only `ᵀ` suffix as a zero-argument call marker. The
    /// semantic pass rewrites the marker to the existing `transpone` intrinsic
    /// only after applying the rank-2 glyph law.
    fn parse_transpose_postfix(&mut self, start: Span, object: Expr) -> Expr {
        let glyph_span = self.current_span();
        self.advance();
        // The private marker and canonical intrinsic spellings are already in
        // the source interner (interned once at parser construction) so
        // semantic checking can rewrite the HIR method name without inventing
        // a second registry or plan entry.
        let method = Ident {
            name: self.transpose_marker_glyph_method,
            span: glyph_span,
        };
        let callee = Expr {
            id: self.next_id(),
            kind: ExprKind::Member(MemberExpr {
                object: Box::new(object),
                member: method,
            }),
            span: start.merge(glyph_span),
        };
        Expr {
            id: self.next_id(),
            kind: ExprKind::Call(CallExpr {
                callee: Box::new(callee),
                type_args: Vec::new(),
                args: Vec::new(),
            }),
            span: start.merge(glyph_span),
        }
    }

    /// Parse the postfix gradient suffix `∇ [places]?` bound to an already-parsed
    /// forward expression.
    ///
    /// The gradient glyph is a structural postfix operator, never a prefix unary
    /// form. The optional selector bracket is consumed here, before the ordinary
    /// postfix loop resumes, so later postfix operations apply to the gradient
    /// result. The grammar is whitespace-insensitive: `loss(x) ∇[x]` parses and
    /// lowers to the same selectors as `loss(x) ∇ [x]`; both formatters re-emit
    /// the canonical spaced form. A non-empty selector list is required once `[` follows `∇`.
    fn parse_gradient_postfix(&mut self, start: Span, call: Expr) -> Result<Expr, ParseError> {
        self.advance();
        let selectors = if self.check(&TokenKind::LBracket) {
            self.advance();
            if self.check(&TokenKind::RBracket) {
                return Err(self.expected_expression_error("gradient_empty_selector_list"));
            }
            let mut selectors = Vec::new();
            loop {
                selectors.push(self.parse_expression()?);
                if !self.consume_item_separator(&TokenKind::RBracket, "gradient_selectors")? {
                    break;
                }
            }
            self.expect_issue(&TokenKind::RBracket, "gradient_selectors_end")?;
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

    fn parse_postfix_payload(
        &mut self,
        start: Span,
        object: Expr,
        family: PostfixFamily,
        payload: PostfixPayload,
    ) -> Result<Expr, ParseError> {
        self.advance();
        match family {
            PostfixFamily::Optional => {
                let chain = match payload {
                    PostfixPayload::Member => OptionalChainKind::Member(self.parse_member_ident()?),
                    PostfixPayload::Index => {
                        let index = Box::new(self.parse_expression()?);
                        self.expect_issue(&TokenKind::RBracket, "optional_index_end")?;
                        OptionalChainKind::Index(index)
                    }
                    PostfixPayload::Call => {
                        let args = self.parse_argument_list(false)?;
                        self.expect_issue(&TokenKind::RParen, "optional_call_arguments_end")?;
                        OptionalChainKind::Call(args)
                    }
                };
                Ok(self.postfix_expr(
                    start,
                    ExprKind::OptionalChain(OptionalChainExpr {
                        object: Box::new(object),
                        chain,
                    }),
                ))
            }
            PostfixFamily::NonNull => {
                let chain = match payload {
                    PostfixPayload::Member => NonNullKind::Member(self.parse_member_ident()?),
                    PostfixPayload::Index => {
                        let index = Box::new(self.parse_expression()?);
                        self.expect_issue(&TokenKind::RBracket, "non_null_index_end")?;
                        NonNullKind::Index(index)
                    }
                    PostfixPayload::Call => {
                        let args = self.parse_argument_list(false)?;
                        self.expect_issue(&TokenKind::RParen, "non_null_call_arguments_end")?;
                        NonNullKind::Call(args)
                    }
                };
                Ok(self.postfix_expr(
                    start,
                    ExprKind::NonNull(NonNullExpr {
                        object: Box::new(object),
                        chain,
                    }),
                ))
            }
        }
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

    /// Parse `↦ type [<params>] [⊥ default]` bound to an already-parsed operand.
    ///
    /// The `⊥` default (D17.7) binds looser than `↦ T`: the target type and its
    /// parameters finish first, then the default fills the `recovery` slot.
    fn parse_conversio_postfix(
        &mut self,
        operand: Expr,
        span_start: Span,
    ) -> Result<Expr, ParseError> {
        self.advance();
        let target = self.parse_conversio_target()?;
        if let ConversioTarget::Explicit(ty) = &target {
            self.reject_retired_conversio_hint(ty)?;
        }
        let via = self.parse_via_clause()?;
        if self.check(&TokenKind::ExitArrow) {
            return Err(self.invalid_expression_error("recovery_uses_exit_arrow"));
        }
        let recovery = self.parse_inline_default()?;
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
                via,
                recovery,
            }),
            span,
        })
    }

    /// Reject the retired hint spellings on an explicit `↦` target (D11.9).
    ///
    /// A convert hint is a `via` clause, never a type argument. Two pre-D11.9
    /// spellings are retired and get a specific diagnostic pointing at the
    /// clause: the hint as a further type argument of the scalar head
    /// (`ascii<N, Hex>`, `littera<Code>`) and the hint as a bracketed tail
    /// after the target (`lista<b8, magnitudo 16><Le>`). The wrapped numeric spelling
    /// (`int<u32, Bits>`) is rejected earlier, in type position, as a whole.
    ///
    /// WHY: only the scalar heads that ever took a hint are checked, so a
    /// user type named `Code` or `Bits` stays a legal type argument
    /// (`↦ lista<Code>`).
    fn reject_retired_conversio_hint(&mut self, ty: &TypeExpr) -> Result<(), ParseError> {
        if let Some(span) = self.retired_type_argument_hint(ty) {
            return Err(
                ParseError::structured(ParseErrorKind::ExpressionInvalid, span)
                    .with_arg("issue", "conversio_hint_type_argument"),
            );
        }
        if self.check(&TokenKind::LtAscii) {
            let span = self.current_span();
            let checkpoint = self.checkpoint();
            self.advance();
            let is_hint_tail = matches!(
                self.peek().kind,
                TokenKind::Ident(sym)
                    if radix_types::numeric_width::is_conversio_radix_hint(
                        self.interner.resolve(sym)
                    )
            ) && matches!(self.peek_at(1).kind, TokenKind::GtAscii);
            if is_hint_tail {
                return Err(
                    ParseError::structured(ParseErrorKind::ExpressionInvalid, span)
                        .with_arg("issue", "conversio_hint_tail_argument"),
                );
            }
            self.restore(checkpoint);
        }
        Ok(())
    }

    /// Span of a hint spelled as a type argument of a scalar `↦` target head.
    fn retired_type_argument_hint(&self, ty: &TypeExpr) -> Option<Span> {
        let TypeExprKind::Named(head, args) = &ty.kind else {
            return None;
        };
        // Slot 1 of `ascii` is the capacity; `littera` has no other argument,
        // so its slot 0 is the hint. The numeric heads never carry arguments
        // here: the wrapped numeric spelling is rejected in type position.
        let first_hint_slot =
            match radix_types::Primitive::from_name(self.interner.resolve(head.name))? {
                radix_types::Primitive::Ascii => 1,
                radix_types::Primitive::Littera => 0,
                _ => return None,
            };
        args.iter().skip(first_hint_slot).find_map(|arg| {
            let ident = match arg {
                GenericArgExpr::Name(ident) => ident,
                GenericArgExpr::Type(TypeExpr {
                    kind: TypeExprKind::Named(ident, inner),
                    ..
                }) if inner.is_empty() => ident,
                _ => return None,
            };
            radix_types::numeric_width::is_conversio_radix_hint(self.interner.resolve(ident.name))
                .then_some(ident.span)
        })
    }

    /// Parse the optional `via HINT` clause after a `↦` target (D11.9).
    ///
    /// GRAMMAR:
    ///   `via_clause` := 'via' IDENTIFIER
    ///
    /// WHY: `via` is contextual (never reserved): real programs bind it as an
    /// ordinary name. It is claimed only on the same line as the conversion
    /// target, so a following statement that starts with the identifier `via`
    /// stays a new statement. The clause binds tighter than the `⊥` default:
    /// `x ↦ u32 via Hex ⊥ 0` is `(x ↦ u32 via Hex) ⊥ 0`. Whether the hint is
    /// known, and whether the target takes one, is semantic (lowering).
    fn parse_via_clause(&mut self) -> Result<Option<Ident>, ParseError> {
        if !self.check_keyword_ident(TokenKind::Via) || self.raw_newline_before_next_word() {
            return Ok(None);
        }
        let via_span = self.current_span();
        self.advance();
        if !self.raw_newline_before_next_word()
            && let TokenKind::Ident(name) = self.peek().kind
        {
            let span = self.current_span();
            self.advance();
            return Ok(Some(Ident { name, span }));
        }
        Err(
            ParseError::structured(ParseErrorKind::ExpressionInvalid, via_span)
                .with_arg("issue", "conversio_via_missing_hint"),
        )
    }

    /// Parse an optional `⊥ default` (D17.7) into the `recovery` slot.
    ///
    /// One expression either propagates or defaults, never both: a `⇥` after
    /// the default is rejected with `default_with_propagation`.
    fn parse_inline_default(&mut self) -> Result<Option<Box<Expr>>, ParseError> {
        if !self.eat(&TokenKind::UpTack) {
            return Ok(None);
        }
        let default = self.parse_unary_inner()?;
        if self.check(&TokenKind::ExitArrow) {
            return Err(self.invalid_expression_error("default_with_propagation"));
        }
        Ok(Some(Box::new(default)))
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
            // pack), so `↦ int` routes to an explicit type target.
            let latin = self
                .reader_type_latin_keys
                .get(name)
                .map_or(name, String::as_str);
            // Type-position keywords (`modulus`, `iuncta`) must still route to
            // an explicit type target rather than the range fallback — in Latin
            // or a reader-pack surface spelling. The `latin` catch covers the
            // canonical Latin spelling under a non-Latin pack, where the
            // keyword-claim path is pack-authoritative and rejects it (a
            // formatter may re-emit the canonical type spelling).
            let is_type_slot_keyword = self.check_keyword_ident(TokenKind::Modulus)
                || self.check_keyword_ident(TokenKind::Iuncta)
                || matches!(latin, "modulus" | "iuncta");
            if !Self::is_conversio_type_keyword(latin)
                && !Self::is_capitalized_type_name(name)
                && !self.is_qualified_capitalized_type_name()
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
            TokenKind::Integer(_)
                | TokenKind::BigInteger(_)
                | TokenKind::Float(_)
                | TokenKind::FloatText { .. }
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
                | "saturatus"
                | "exactus"
                | "littera"
                | "regex"
                | "json"
                | "valor"
                | "instans"
                | "lista"
                | "tabula"
                | "tabulae"
                | "copia"
                | "promissum"
                | "cursor"
                | "queue"
                | "stack"
                | "tensor"
                | "vector"
                | "matrix"
                | "intervallum"
                | "sparsa"
                | "series"
                | "ratio"
        )
    }

    fn is_capitalized_type_name(name: &str) -> bool {
        name.chars().next().is_some_and(char::is_uppercase)
    }

    /// A qualified user type may start with a lowercase namespace binding,
    /// while its terminal nominal name still follows the capitalized type
    /// spelling convention (`types.Board`). The caller has already matched an
    /// identifier head, so only the dot and the terminal spelling are checked.
    fn is_qualified_capitalized_type_name(&self) -> bool {
        if !matches!(self.peek_at(1).kind, TokenKind::Dot) {
            return false;
        }

        let mut offset = 1;
        loop {
            offset += 1;
            let TokenKind::Ident(sym) = self.peek_at(offset).kind else {
                return false;
            };
            let name = self.interner.resolve(sym);
            offset += 1;
            if matches!(self.peek_at(offset).kind, TokenKind::Dot) {
                continue;
            }
            return Self::is_capitalized_type_name(name);
        }
    }

    // PRIMARY EXPRESSIONS

    fn extent_constructor_head(&self) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        if self.peek_at(1).kind != TokenKind::LBracket {
            return false;
        }
        let name = self.interner.resolve(sym);
        let latin = self
            .reader_type_latin_keys
            .get(name)
            .map_or(name, String::as_str);
        radix_types::numeric_width::is_numeric_width_type_sugar(latin)
            || radix_types::numeric_width::vector_sugar_width_marker(latin).is_some()
    }

    /// Long-form built-in type word followed by `<`: `tensor<..>`,
    /// `vector<..>`, `matrix<..>`, and `list<..>` (claimed only to refuse
    /// `list<T, size N> {}` by name). Only a claim candidate; the caller parses
    /// the type speculatively and keeps `<` as the ASCII comparison when the
    /// type does not parse (`vector < n`).
    fn long_form_constructor_head(&self) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        if self.peek_at(1).kind != TokenKind::LtAscii {
            return false;
        }
        let name = self.interner.resolve(sym);
        let latin = self
            .reader_type_latin_keys
            .get(name)
            .map_or(name, String::as_str);
        matches!(latin, "tensor" | "vector" | "matrix" | "lista")
    }

    /// Speculatively parse a long-form type word as a value-position type
    /// expression; `None` (position restored) when it is not a complete type.
    fn try_parse_long_form_constructor_type(&mut self) -> Option<TypeExpr> {
        let checkpoint = self.checkpoint();
        let error_count = self.errors.len();
        match self.parse_type() {
            Ok(ty) if self.errors.len() == error_count && Self::is_extent_type(&ty) => Some(ty),
            // A collection word is claimed as `list<..> {}` or as the stated
            // bracket constructor `list<..> [ … ]`; anything else keeps `<`
            // as the ASCII comparison.
            Ok(ty)
                if self.errors.len() == error_count
                    && (self.check(&TokenKind::LBracket)
                        || (self.check(&TokenKind::LBrace)
                            && self.peek_at(1).kind == TokenKind::RBrace)) =>
            {
                Some(ty)
            }
            _ => {
                self.errors.truncate(error_count);
                self.restore(checkpoint);
                None
            }
        }
    }

    fn is_extent_type(ty: &TypeExpr) -> bool {
        matches!(
            ty.kind,
            TypeExprKind::Tensor(..) | TypeExprKind::Vector(..) | TypeExprKind::Matrix(..)
        )
    }

    /// Return whether the current token claims a registry keyword identity.
    fn is_keyword_key(&self, key: &str) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(sym) if self.keyword_claim_key(sym) == Some(key))
    }

    /// Parse the `reducta via Op` expression.
    fn parse_reducta_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.advance();
        if !self.eat_keyword_ident(TokenKind::Via) {
            return Err(self.reducta_operator_error("reducta_requires_via"));
        }
        let op = match self.peek().kind {
            TokenKind::Ident(sym) => match self.keyword_claim_key(sym) {
                Some("summa") => ReductaOp::Sum,
                Some("factum") => ReductaOp::Product,
                Some("maxima") => ReductaOp::Max,
                Some("minima") => ReductaOp::Min,
                Some("argmaxima") => ReductaOp::ArgMax,
                Some("argminima") => ReductaOp::ArgMin,
                Some("omnia") => ReductaOp::All,
                Some("quilibet") => ReductaOp::Any,
                Some("numeratio") => ReductaOp::Count,
                Some("prima") => ReductaOp::First,
                _ => return Err(self.reducta_operator_error("reducta_unknown_operator")),
            },
            _ => return Err(self.reducta_operator_error("reducta_unknown_operator")),
        };
        self.advance();
        self.expect_keyword_issue(TokenKind::Ex, "reducta_source")?;
        // Captured before the source parses (see `parse_sort_expr`): a loop
        // head's `fixum name {` binder is not this expression's body.
        let body_barred = self.sort_body_barred_at == Some(self.expression_contexts);
        let mut source = self.parse_expression()?;
        let apud = if self.eat_keyword_ident(TokenKind::Apud) {
            self.parse_apud_clause()?
        } else {
            Vec::new()
        };
        let group = if self.is_keyword_key("pro") {
            self.advance();
            let group = if self.check(&TokenKind::LBracket) {
                self.parse_primary()?
            } else {
                // GRAMMAR: group_key := unary. Stopping above `vel` leaves
                // the reduction's optional identity for `reducta_identity`.
                self.parse_unary_inner()?
            };
            Some(Box::new(group))
        } else {
            None
        };
        let filum = if self.is_keyword_key("filum") {
            self.advance();
            Some(self.parse_ident()?)
        } else {
            None
        };
        let has_body = !body_barred && self.inline_body_head();
        let (mutability, binding, body) = if has_body {
            (
                self.parse_binder_mutability()?,
                Some(self.parse_ident()?),
                Some(self.parse_block()?),
            )
        } else {
            (AstMutability::Immutable, None, None)
        };
        let mut identity = if self.eat_keyword_ident(TokenKind::Vel) {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };
        if identity.is_none()
            && body.is_none()
            && group.is_none()
            && apud.is_empty()
            && let ExprKind::Binary(BinaryExpr {
                op: BinOp::Coalesce,
                lhs,
                rhs,
            }) = source.kind
        {
            source = *lhs;
            identity = Some(rhs);
        }
        Ok(Expr {
            id: self.next_id(),
            kind: ExprKind::Reducta(Box::new(ReductaExpr {
                op,
                source: Box::new(source),
                apud,
                group,
                filum,
                mutability,
                binding,
                body,
                identity,
            })),
            span: start.merge(self.previous_span()),
        })
    }

    /// Parse the ordered prefix form. Its body is mandatory, unlike the
    /// scalar reduction's optional identity form. A following `vel` remains
    /// for the outer expression parser to wrap around the complete scan.
    fn parse_scan_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.advance();
        self.expect_keyword_ident(TokenKind::Via)?;
        let op = match self.peek().kind {
            TokenKind::Ident(sym) => match self.keyword_claim_key(sym) {
                Some("summa") => ReductaOp::Sum,
                Some("factum") => ReductaOp::Product,
                Some("maxima") => ReductaOp::Max,
                Some("minima") => ReductaOp::Min,
                Some("omnia") => ReductaOp::All,
                Some("quilibet") => ReductaOp::Any,
                Some("numeratio") => ReductaOp::Count,
                _ => return Err(self.scan_operator_error()),
            },
            _ => return Err(self.scan_operator_error()),
        };
        self.advance();

        let total = if self.is_keyword_key("scan_total") {
            self.advance();
            let total = match self.peek().kind {
                TokenKind::Ident(sym) => match self.keyword_claim_key(sym) {
                    Some("scan_inclusive") => ScanTotal::Inclusive,
                    Some("scan_exclusive") => ScanTotal::Exclusive,
                    _ => return Err(self.scan_total_error()),
                },
                _ => return Err(self.scan_total_error()),
            };
            self.advance();
            total
        } else {
            ScanTotal::Inclusive
        };

        self.expect_keyword_ident(TokenKind::Ex)?;
        let source = Box::new(self.parse_expression()?);
        if !self.eat_keyword_ident(TokenKind::Apud) {
            return Err(self.scan_expected_keyword_error(&["apud"]));
        }
        let apud = self.parse_apud_clause()?;
        let group = if self.is_keyword_key("pro") {
            self.advance();
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };
        let order = if self.is_keyword_key("scan_order") {
            self.advance();
            let order = match self.peek().kind {
                TokenKind::Ident(sym) => match self.keyword_claim_key(sym) {
                    Some("ascendens") => SortDirection::Ascending,
                    Some("descendens") => SortDirection::Descending,
                    _ => return Err(self.scan_order_error()),
                },
                _ => return Err(self.scan_order_error()),
            };
            self.advance();
            order
        } else {
            SortDirection::Ascending
        };

        if !self.is_keyword_key("filum") {
            return Err(self.scan_expected_keyword_error(&["filum"]));
        }
        self.advance();
        let filum = self.parse_ident()?;
        self.expect_keyword_ident(TokenKind::Sit)?;
        let binding = self.parse_ident()?;
        let body = self.parse_block()?;

        Ok(Expr {
            id: self.next_id(),
            kind: ExprKind::Scan(Box::new(ScanExpr {
                op,
                total,
                source,
                apud,
                group,
                order,
                filum,
                binding,
                body,
            })),
            span: start.merge(self.previous_span()),
        })
    }

    fn scan_operator_error(&self) -> ParseError {
        self.scan_expected_keyword_error(&[
            "summa",
            "factum",
            "maxima",
            "minima",
            "omnia",
            "quilibet",
            "numeratio",
        ])
    }

    fn scan_total_error(&self) -> ParseError {
        self.scan_expected_keyword_error(&["scan_inclusive", "scan_exclusive"])
    }

    fn scan_order_error(&self) -> ParseError {
        self.scan_expected_keyword_error(&["ascendens", "descendens"])
    }

    fn scan_expected_keyword_error(&self, keys: &[&str]) -> ParseError {
        let keyword = keys
            .iter()
            .map(|key| self.keyword_surface(key))
            .collect::<Vec<_>>()
            .join(&format!(" {} ", self.keyword_surface("aut")));
        ParseError::structured(ParseErrorKind::Expected, self.current_span())
            .with_arg("issue", "expected_keyword")
            .with_arg("keyword", keyword)
    }

    /// Parse the `ordinata via Direction (ex | de) source [body]` expression.
    ///
    /// The direction word is claimed by Latin key after `via`; the source word
    /// is `ex` (values) or `de` (keys); the optional body is the reduce body
    /// form `(fixum | varia) IDENTIFIER block` and returns the sort key.
    fn parse_sort_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.advance();
        if !self.eat_keyword_ident(TokenKind::Via) {
            return Err(self.sort_error("sort_requires_via", "directions"));
        }
        let direction = match self.peek().kind {
            TokenKind::Ident(sym) => match self.keyword_claim_key(sym) {
                Some("ascendens") => SortDirection::Ascending,
                Some("descendens") => SortDirection::Descending,
                _ => return Err(self.sort_error("sort_unknown_direction", "directions")),
            },
            _ => return Err(self.sort_error("sort_unknown_direction", "directions")),
        };
        self.advance();
        let source_mode = if self.eat_keyword_ident(TokenKind::Ex) {
            SortSourceMode::Values
        } else if self.eat_keyword_ident(TokenKind::De) {
            SortSourceMode::Keys
        } else {
            return Err(self.sort_error("sort_requires_source", "sources"));
        };
        // Captured before the source parses: a nested expression context
        // moves the counter, and the bar applies to this expression's depth.
        let body_barred = self.sort_body_barred_at == Some(self.expression_contexts);
        let source = self.parse_expression()?;
        let has_body = !body_barred && self.inline_body_head();
        let (mutability, binding, body) = if has_body {
            (
                self.parse_binder_mutability()?,
                Some(self.parse_ident()?),
                Some(self.parse_block()?),
            )
        } else {
            (AstMutability::Immutable, None, None)
        };
        Ok(Expr {
            id: self.next_id(),
            kind: ExprKind::Sort(Box::new(SortExpr {
                direction,
                source_mode,
                source: Box::new(source),
                mutability,
                binding,
                body,
            })),
            span: start.merge(self.previous_span()),
        })
    }

    /// Malformed sort expression: a named `PARSE001` issue listing the words
    /// the active locale pack accepts in the failing slot.
    fn sort_error(&self, issue: &'static str, list: &'static str) -> ParseError {
        let keys: &[&str] = if list == "directions" {
            &["ascendens", "descendens"]
        } else {
            &["ex", "de"]
        };
        let words = keys
            .iter()
            .map(|key| {
                self.reader_keyword_surfaces
                    .get(*key)
                    .map_or(*key, String::as_str)
            })
            .collect::<Vec<_>>()
            .join(", ");
        ParseError::structured(ParseErrorKind::Expected, self.current_span())
            .with_arg("issue", issue)
            .with_arg(list, words)
    }

    fn reducta_operator_error(&self, issue: &'static str) -> ParseError {
        let operators = [
            "summa",
            "factum",
            "maxima",
            "minima",
            "argmaxima",
            "argminima",
            "omnia",
            "quilibet",
            "numeratio",
            "prima",
        ]
        .iter()
        .map(|key| {
            self.reader_keyword_surfaces
                .get(*key)
                .map_or(*key, String::as_str)
        })
        .collect::<Vec<_>>()
        .join(", ");
        ParseError::structured(ParseErrorKind::Expected, self.current_span())
            .with_arg("issue", issue)
            .with_arg("operators", operators)
    }

    /// Whether the cursor is an inline body head: `(sit | varia) NAME {`.
    ///
    /// Reduce and sort expressions attach such a head only outside a barred
    /// loop body; that guard stays with the caller.
    fn inline_body_head(&self) -> bool {
        self.binder_head_word_at(0)
            && matches!(self.peek_at(1).kind, TokenKind::Ident(_))
            && self.peek_at(2).kind == TokenKind::LBrace
    }

    /// Return the head word when a former reduction occurs in its old expression shape.
    ///
    /// The words remain ordinary identifiers everywhere else. Only the old
    /// head-plus-source-keyword pair is claimed, so the diagnostic can point
    /// directly to the migration site without reserving the operator word.
    fn retired_reduction_expr_head(&self) -> Option<Symbol> {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return None;
        };
        (matches!(
            self.keyword_claim_key(sym),
            Some("summa" | "maxima" | "minima")
        ) && self.keyword_ident_at(1) == Some(TokenKind::Ex))
        .then_some(sym)
    }

    /// Consume a former reduction expression and report its named migration issue.
    fn parse_retired_reduction_expr(&mut self, sym: Symbol) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let head = self.interner.resolve(sym).to_owned();
        self.advance();
        self.expect_keyword_issue(TokenKind::Ex, "reducta_source")?;
        let _source = self.parse_expression()?;
        if self.eat_keyword_ident(TokenKind::Apud) {
            self.parse_apud_clause()?;
        }
        if self.is_keyword_key("filum") {
            self.advance();
            self.parse_ident()?;
        }
        let has_term_body = self.inline_body_head();
        if has_term_body {
            self.parse_binder_mutability()?;
            self.parse_ident()?;
            self.parse_block()?;
        } else if self.eat_keyword_ident(TokenKind::Vel) {
            self.parse_expression()?;
        }
        Err(
            ParseError::structured(ParseErrorKind::DeclarationInvalid, start)
                .with_arg("issue", "reducta_legacy_head")
                .with_arg("head", head),
        )
    }

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
        if self.is_keyword_key("scan")
            && matches!(
                self.keyword_ident_at(1),
                Some(TokenKind::Via | TokenKind::Ex)
            )
        {
            return self.parse_scan_expr();
        }
        if self.is_keyword_key("reducta")
            && matches!(
                self.keyword_ident_at(1),
                Some(TokenKind::Via | TokenKind::Ex)
            )
        {
            return self.parse_reducta_expr();
        }
        if self.is_keyword_key("ordinata")
            && matches!(
                self.keyword_ident_at(1),
                Some(TokenKind::Via | TokenKind::Ex)
            )
        {
            return self.parse_sort_expr();
        }
        if let Some(head) = self.retired_reduction_expr_head() {
            return self.parse_retired_reduction_expr(head);
        }
        if let Some(expr) = self.try_parse_compact_clausura_expr()? {
            return Ok(expr);
        }

        let start = self.current_span();
        let long_form_type = if self.long_form_constructor_head() {
            self.try_parse_long_form_constructor_type()
        } else {
            None
        };
        if long_form_type.is_some() || self.extent_constructor_head() {
            // Only the typed-zero heads are store-position-only; a `list<..>`
            // bracket constructor is a plain value expression at any depth.
            let typed_zero_head = self.extent_constructor_head()
                || long_form_type.as_ref().is_some_and(Self::is_extent_type);
            if self.expression_contexts > 1 && typed_zero_head {
                return Err(
                    ParseError::structured(ParseErrorKind::ExpressionInvalid, start)
                        .with_arg("issue", "constructor_store_position_only"),
                );
            }
            let long_form = long_form_type.is_some();
            let ty = match long_form_type {
                Some(ty) => ty,
                None => self.parse_type()?,
            };
            // `list<T, size N> {}` is not claimed: only tensor, vector and matrix
            // types have a typed zero. A collection head followed by `[` is
            // the stated bracket constructor instead, with the element list
            // as its `verte` target.
            if long_form && !Self::is_extent_type(&ty) {
                if self.check(&TokenKind::LBracket) {
                    self.advance();
                    let elements = self.parse_array_elements()?;
                    self.expect_issue(&TokenKind::RBracket, "typed_constructor_elements_end")?;
                    let array = Expr {
                        id: self.next_id(),
                        kind: ExprKind::Array(ArrayExpr { elements }),
                        span: ty.span.merge(self.previous_span()),
                    };
                    return Ok(Expr {
                        id: self.next_id(),
                        kind: ExprKind::Verte(VerteExpr {
                            expr: Box::new(array),
                            ty,
                        }),
                        span: start.merge(self.previous_span()),
                    });
                }
                return Err(
                    ParseError::structured(ParseErrorKind::ExpressionInvalid, start)
                        .with_arg("issue", "constructor_type_not_claimed"),
                );
            }
            if matches!(
                self.peek().kind,
                TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Star
                    | TokenKind::Slash
                    | TokenKind::LParen
            ) {
                return Err(
                    ParseError::structured(ParseErrorKind::ExpressionInvalid, start)
                        .with_arg("issue", "constructor_store_position_only"),
                );
            }
            // The typed zero is written `Type {}`; the bare form is retired.
            if !(self.check(&TokenKind::LBrace) && self.peek_at(1).kind == TokenKind::RBrace) {
                return Err(
                    ParseError::structured(ParseErrorKind::ExpressionInvalid, start)
                        .with_arg("issue", "constructor_requires_empty_braces"),
                );
            }
            self.advance();
            self.advance();
            let id = self.next_id();
            return Ok(Expr {
                id,
                kind: ExprKind::Construct(ty),
                span: start.merge(self.previous_span()),
            });
        }
        let id = self.next_id();

        // `regex /pattern/` (the locale's regex type word) and `_ /pattern/`:
        // the lexer emits `RegexString` only directly after the type word or
        // `_`, so the pair claims this literal slot before any identifier or
        // inferred-expression handling. A `RegexString` anywhere else stays the
        // fallback arm's `expected expression` error. The explicit `regex`
        // form is the self-typed literal; `_ /pattern/` builds the
        // inferred-constructor hole — the same `Verte` node as `_ [ … ]` —
        // which needs an expected type from the checker (rulings 44def1ae /
        // d381ae56, need).
        if matches!(
            self.peek().kind,
            TokenKind::Ident(_) | TokenKind::Underscore(_)
        ) && let TokenKind::RegexString(pattern) = self.peek_at(1).kind
        {
            if matches!(self.peek().kind, TokenKind::Underscore(_)) {
                let hole_span = self.peek().span;
                self.advance();
                self.advance();
                let literal = Expr {
                    id: self.next_id(),
                    kind: ExprKind::Literal(Literal::Regex(pattern)),
                    span: hole_span.merge(self.previous_span()),
                };
                let ty = TypeExpr {
                    nullable: false,
                    mode: None,
                    kind: TypeExprKind::Infer,
                    span: hole_span,
                    form: TypeFormLevel::LONG,
                    iuncta_label: None,
                };
                return Ok(Expr {
                    id,
                    kind: ExprKind::Verte(VerteExpr {
                        expr: Box::new(literal),
                        ty,
                    }),
                    span: start.merge(self.previous_span()),
                });
            }
            self.advance();
            self.advance();
            return Ok(Expr {
                id,
                kind: ExprKind::Literal(Literal::Regex(pattern)),
                span: start.merge(self.previous_span()),
            });
        }

        let kind = match self.peek().kind.clone() {
            TokenKind::Integer(n) => {
                self.advance();
                ExprKind::Literal(Literal::Int(n))
            }
            TokenKind::BigInteger(text) => {
                self.advance();
                ExprKind::Literal(Literal::BigInt(text))
            }
            TokenKind::Float(n) => {
                self.advance();
                ExprKind::Literal(Literal::Float(n))
            }
            TokenKind::FloatText { value, text } => {
                self.advance();
                ExprKind::Literal(Literal::FloatText { value, text })
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
                // `insere` (en `embed`) claims the spelling only directly
                // followed by a string literal, so `insere(21)` and bare
                // identifier uses keep their ordinary meaning. Whether an
                // embed is legal at this position is lowering's call.
                if self.check_keyword_ident(TokenKind::Insere)
                    && let TokenKind::String(path) = self.peek_at(1).kind
                {
                    return Ok(self.parse_insere_expr(path));
                }
                if self.check_keyword_ident(TokenKind::Clausura) {
                    return self.parse_clausura_expr();
                }
                // Capture boundary: `capta { ... }` claims the spelling only
                // in expression-primary position directly followed by a
                // block, so calls `capta(…)` and bare identifier uses keep
                // their ordinary meaning.
                if self.check_keyword_ident(TokenKind::Trap)
                    && self.peek_at(1).kind == TokenKind::LBrace
                {
                    return self.parse_trap_expr();
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
                    let ident = self.parse_primary_ident()?;
                    return Ok(Expr {
                        id,
                        kind: ExprKind::KeywordLiteral(KeywordLiteralExpr { literal, ident }),
                        span: start.merge(self.previous_span()),
                    });
                }
                // `nihil` was the null value spelling until D23.2 split the
                // value (`nulla`) from the type (`nihil`). In value position
                // it is a named error, not an unknown identifier. A call,
                // member access or constructor keeps the word usable as an
                // ordinary name.
                if self.is_retired_null_value_spelling() {
                    return Err(self.retired_null_value_error());
                }
                let ident = self.parse_primary_ident()?;
                // Locale-aware `nonnumerus` claim: NaN is a literal only in
                // the primary literal slot. A following `(` keeps the
                // spelling available for ordinary `nan(...)` calls.
                if self.keyword_claim_key(ident.name) == Some("nonnumerus")
                    && self.peek().kind != TokenKind::LParen
                {
                    ExprKind::KeywordLiteral(KeywordLiteralExpr {
                        literal: Literal::Float(f64::NAN),
                        ident,
                    })
                // Locale-aware `vacua` claim: the empty-collection marker
                // claims only through the pack-authoritative surface gate.
                // Under `la`/no-pack the registry-direct fallback lets the
                // Latin spelling claim; under a non-`la` pack only a spelling
                // the lexer recorded a latin key for (the pack's own `vacua`
                // row) claims — an undeclared Latin `vacua` stays an ordinary
                // identifier.
                } else if self.keyword_claim_key(ident.name) == Some("vacua") {
                    ExprKind::Vacua(ident)
                } else if self.is_ratio_constructor_surface(ident.name)
                    && self.check(&TokenKind::LtAscii)
                    && self.ratio_constructor_head()
                {
                    self.parse_ratio_constructor(ident)?
                } else if let Some(constructor) =
                    self.try_parse_qualified_typed_constructor(&ident)?
                {
                    constructor
                } else if self.check(&TokenKind::LBrace)
                    && self.looks_like_typed_constructor_fields()
                {
                    let type_span = ident.span;
                    self.advance();
                    let mut fields = self.parse_object_fields()?;
                    self.expect_issue(&TokenKind::RBrace, "typed_constructor_fields_end")?;
                    self.parse_construction_source(&mut fields)?;
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
                        form: TypeFormLevel::LONG,
                        iuncta_label: None,
                    };
                    ExprKind::Verte(VerteExpr {
                        expr: Box::new(object),
                        ty,
                    })
                } else {
                    ExprKind::Ident(ident)
                }
            }

            // `_ { field = value, … }`: the inferred constructor. The hole
            // type before braces builds the expected type; it lowers to the
            // same `Verte` node as `Type { … }` with an `Infer` target.
            TokenKind::Underscore(_)
                if matches!(self.peek_at(1).kind, TokenKind::LBrace)
                    && self.looks_like_typed_constructor_fields_at(1) =>
            {
                let hole_span = self.peek().span;
                self.advance();
                self.advance();
                self.finish_inferred_field_constructor(hole_span, false)?
            }

            // Bare `{ field = value, … }`: the type slot is left out, which
            // implies the hole (operator ruling 2026-10-10, need).
            // Same `Verte` node as `_ { … }`; the cell supplies the type.
            TokenKind::LBrace
                if matches!(self.peek_at(1).kind, TokenKind::Ident(_))
                    && matches!(self.peek_at(2).kind, TokenKind::Eq) =>
            {
                let hole_span = self.peek().span;
                self.advance();
                self.finish_inferred_field_constructor(hole_span, true)?
            }

            // `_ [ a, b, … ]`: the inferred constructor with ELEMENTS. The
            // hole takes the expected bracket type (tuple, list); the checker
            // decides, and names the error when no type is expected.
            TokenKind::Underscore(_) if matches!(self.peek_at(1).kind, TokenKind::LBracket) => {
                let hole_span = self.peek().span;
                self.advance();
                self.advance();
                let elements = self.parse_array_elements()?;
                self.expect_issue(&TokenKind::RBracket, "inferred_constructor_elements_end")?;
                let array = Expr {
                    id: self.next_id(),
                    kind: ExprKind::Array(ArrayExpr { elements }),
                    span: hole_span.merge(self.previous_span()),
                };
                let ty = TypeExpr {
                    nullable: false,
                    mode: None,
                    kind: TypeExprKind::Infer,
                    span: hole_span,
                    form: TypeFormLevel::LONG,
                    iuncta_label: None,
                };
                ExprKind::Verte(VerteExpr {
                    expr: Box::new(array),
                    ty,
                })
            }

            // `_ { "key": value, … }`: the inferred constructor with ENTRIES.
            // The entries are the same constant JSON members the bare `{ … }`
            // literal takes; the hole target lets the checker pick the shape
            // from the expected type (a map builds what the bare literal
            // builds; any other type is a named error).
            TokenKind::Underscore(_)
                if matches!(self.peek_at(1).kind, TokenKind::LBrace)
                    && matches!(self.peek_at(2).kind, TokenKind::String(_))
                    && matches!(self.peek_at(3).kind, TokenKind::Colon) =>
            {
                let hole_span = self.peek().span;
                self.advance();
                self.advance();
                let literal = self.parse_json_literal()?;
                let object = Expr {
                    id: self.next_id(),
                    kind: literal,
                    span: hole_span.merge(self.previous_span()),
                };
                let ty = TypeExpr {
                    nullable: false,
                    mode: None,
                    kind: TypeExprKind::Infer,
                    span: hole_span,
                    form: TypeFormLevel::LONG,
                    iuncta_label: None,
                };
                ExprKind::Verte(VerteExpr {
                    expr: Box::new(object),
                    ty,
                })
            }

            TokenKind::LBracket => {
                self.advance();
                let elements = self.parse_array_elements()?;
                self.expect_issue(&TokenKind::RBracket, "array_literal_end")?;
                ExprKind::Array(ArrayExpr { elements })
            }

            // Bare `{ }` is JSON grammar, not a Faber anonymous record.
            // Genus construction (`Type { ... }`) keeps Faber `=` grammar via
            // the `Ident` dispatch above.
            TokenKind::LBrace => {
                self.advance();
                self.parse_json_literal()?
            }

            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expression()?;
                self.expect_issue(&TokenKind::RParen, "parenthesized_expression_end")?;
                ExprKind::Paren(Box::new(inner))
            }

            _ => {
                return Err(self.invalid_expression_error("expected_expression"));
            }
        };

        let span = start.merge(self.previous_span());
        Ok(Expr { id, kind, span })
    }

    /// Locale claim for the ratio constructor head: resolve the surface
    /// spelling to its latin identity through the type rows (map-in-both,
    /// mirroring the retired-call guards), so the en pack's `record<g: …>`
    /// constructs like `ratio<g: …>`; an unmapped spelling (`record` under
    /// `la`) stays an ordinary identifier.
    fn is_ratio_constructor_surface(&self, sym: Symbol) -> bool {
        let name = self.interner.resolve(sym);
        let latin = self
            .reader_type_latin_keys
            .get(name)
            .map_or(name, String::as_str);
        latin == "ratio"
    }

    /// Two-token lookahead for the expression-position `ratio` constructor
    /// claim (mirrors the contextual reduction-head check): commit only when the tokens after
    /// `<` open a labeled element (`IDENT/Tag` then `:`), the same
    /// `is_label` shape `parse_ratio_type_argument` requires, or the whole
    /// field-list hole (`*` then `>`) followed by the construction braces.
    /// Anything else (`ratio < 5`, `a * b`) is an ordinary identifier
    /// followed by operators.
    fn ratio_constructor_head(&self) -> bool {
        (matches!(self.peek_at(1).kind, TokenKind::Ident(_) | TokenKind::Tag)
            && matches!(self.peek_at(2).kind, TokenKind::Colon))
            || (matches!(self.peek_at(1).kind, TokenKind::Star)
                && matches!(self.peek_at(2).kind, TokenKind::GtAscii)
                && matches!(self.peek_at(3).kind, TokenKind::LBrace))
    }

    /// Parse the ratio constructor claim: `ratio<label: T, …> { field = value, … }`
    /// or the whole-field-list hole `ratio<*> { field = value, … }`.
    ///
    /// The head is claimed only in this angle-bracket slot, so ordinary
    /// bindings named `ratio` (or a pack surface like `record`) remain
    /// ordinary identifiers. The head canonicalizes to its latin identity so
    /// the constructed type matches the annotation path under any reader pack.
    fn parse_ratio_constructor(&mut self, name: Ident) -> Result<ExprKind, ParseError> {
        let name = self.canonical_type_ident(name);
        let head_span = name.span;
        self.expect_issue(&TokenKind::LtAscii, "ratio_type_arguments_start")?;

        let ty_kind = if self.check(&TokenKind::Star) {
            // `ratio<*>`: the whole field list is the hole, solved from the
            // braced shape; it stays the sole argument by construction.
            self.advance();
            self.expect_issue(&TokenKind::GtAscii, "ratio_type_arguments_end")?;
            TypeExprKind::FieldListHole
        } else {
            let mut type_args = Vec::new();
            let mut labels = HashSet::new();
            loop {
                type_args.push(GenericArgExpr::Type(
                    self.parse_ratio_type_argument(&mut labels)?,
                ));
                if !self.consume_item_separator(&TokenKind::GtAscii, "ratio_type_arguments")? {
                    break;
                }
            }
            self.expect_issue(&TokenKind::GtAscii, "ratio_type_arguments_end")?;
            TypeExprKind::Named(name, type_args)
        };
        let type_span = head_span.merge(self.previous_span());

        self.expect_issue(&TokenKind::LBrace, "ratio_fields_start")?;
        let mut fields = self.parse_object_fields()?;
        self.expect_issue(&TokenKind::RBrace, "ratio_fields_end")?;
        self.parse_construction_source(&mut fields)?;

        let object = Expr {
            id: self.next_id(),
            kind: ExprKind::Object(ObjectExpr { fields }),
            span: type_span.merge(self.previous_span()),
        };
        let ty = TypeExpr {
            nullable: false,
            mode: None,
            kind: ty_kind,
            span: type_span,
            form: TypeFormLevel::LONG,
            iuncta_label: None,
        };
        Ok(ExprKind::Verte(VerteExpr {
            expr: Box::new(object),
            ty,
        }))
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
        // Applied head (`Gen<3> { … }`, `pkg.Box<N> { … }`): the argument
        // list parses speculatively and survives only when a constructor
        // field block follows. Any other continuation restores, so `<`
        // stays owned by comparison (`a < b`) and generic call
        // (`f<T>(…)`) parsing.
        let mut type_params = Vec::new();
        if self.check(&TokenKind::LtAscii) {
            let args_checkpoint = self.checkpoint();
            match self.parse_constructor_type_args() {
                Ok(args) if self.check(&TokenKind::LBrace) => {
                    type_params = args;
                }
                _ => {
                    self.restore(args_checkpoint);
                }
            }
        }
        // `Type { "k": v, … }` is the stated keyed constructor: the same
        // `Verte`/JSON-literal node the ascription `{ "k": v } ∷ Type`
        // parses to. The quoted-key plus colon lookahead keeps the `=` field
        // form with the object-constructor branch below.
        let keyed_entries = self.looks_like_keyed_constructor_entries();
        if (path.len() == 1 && type_params.is_empty() && !keyed_entries)
            || !self.check(&TokenKind::LBrace)
            || !(keyed_entries || self.looks_like_typed_constructor_fields())
        {
            self.restore(checkpoint);
            return Ok(None);
        }

        let mut type_span = path
            .first()
            .map(|segment| segment.span)
            .unwrap_or_default()
            .merge(path.last().map(|segment| segment.span).unwrap_or_default());
        if !type_params.is_empty() {
            type_span = type_span.merge(self.previous_span());
        }
        let object_start = self.current_span();
        self.advance();
        let kind = if path.len() == 1 {
            let head = path.remove(0);
            // The ascription head canonicalizes through the reader pack, so
            // the stated spelling must produce the same type identity (the
            // keyed form is the ascription's source). Qualified paths keep
            // their authored namespace spelling.
            let head = if keyed_entries {
                self.canonical_type_ident(head)
            } else {
                head
            };
            TypeExprKind::Named(head, type_params)
        } else {
            TypeExprKind::Qualified(path, type_params)
        };
        let ty = TypeExpr {
            nullable: false,
            mode: None,
            kind,
            span: type_span,
            form: TypeFormLevel::LONG,
            iuncta_label: None,
        };
        if keyed_entries {
            let literal = self.parse_json_literal()?;
            let source = Expr {
                id: self.next_id(),
                kind: literal,
                span: object_start.merge(self.previous_span()),
            };
            return Ok(Some(ExprKind::Verte(VerteExpr {
                expr: Box::new(source),
                ty,
            })));
        }
        let mut fields = self.parse_object_fields()?;
        self.expect_issue(&TokenKind::RBrace, "type_constructor_fields_end")?;
        self.parse_construction_source(&mut fields)?;
        let object = Expr {
            id: self.next_id(),
            kind: ExprKind::Object(ObjectExpr { fields }),
            span: type_span.merge(self.previous_span()),
        };
        Ok(Some(ExprKind::Verte(VerteExpr {
            expr: Box::new(object),
            ty,
        })))
    }

    #[inline(never)]
    fn retired_null_value_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::ExpressionInvalid, self.peek().span)
            .with_arg("issue", "retired_null_value_spelling")
            .with_arg("replacement", self.null_value_surface())
    }

    /// The current word is the bare Latin `nihil` in value position and no
    /// value binding or parameter named `nihil` has been declared (a binding
    /// may shadow the retired spelling; operator ruling 2026-10-01).
    fn is_retired_null_value_spelling(&self) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        self.interner.resolve(sym) == "nihil"
            && !self.nihil_value_bound
            && !matches!(
                self.peek_at(1).kind,
                TokenKind::LParen | TokenKind::Dot | TokenKind::LBrace | TokenKind::LtAscii
            )
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
        self.expect_keyword_ident(TokenKind::Finge)?;

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
                if !self.consume_item_separator(&TokenKind::RBrace, "variant_field_initializers")? {
                    break;
                }
            }
            self.expect_issue(&TokenKind::RBrace, "finge_fields_end")?;
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
        self.expect_keyword_ident(TokenKind::Iuncta)?;

        self.expect_issue(&TokenKind::LtAscii, "iuncta_type_arguments_start")?;
        let type_args = self.finish_iuncta_type_args_list()?;

        self.expect_issue(&TokenKind::LBracket, "iuncta_elements_start")?;
        let mut elements = Vec::new();
        while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
            elements.push(self.parse_expression()?);
            if !self.consume_item_separator(&TokenKind::RBracket, "iuncta_elements")? {
                break;
            }
        }
        self.expect_issue(&TokenKind::RBracket, "iuncta_elements_end")?;

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
        self.expect_keyword_ident(TokenKind::Clausura)?;

        let mut params = Vec::new();
        loop {
            if self.check(&TokenKind::Arrow)
                || self.check(&TokenKind::Colon)
                || self.check(&TokenKind::LBrace)
            {
                break;
            }

            params.push(self.parse_clausura_param()?);

            if !self.consume_item_separator_before(
                &[TokenKind::Arrow, TokenKind::Colon, TokenKind::LBrace],
                "closure_parameters",
            )? {
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
            kind: ExprKind::Clausura(Box::new(ClausuraExpr {
                params,
                ret,
                err,
                body,
            })),
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
        let saved = self.checkpoint();
        match self.parse_compact_clausura_expr() {
            Ok(expr) => Ok(Some(expr)),
            Err(err)
                if matches!(
                    err.kind,
                    ParseErrorKind::ErgoWrongContext
                        | ParseErrorKind::ThereforeWrongContext
                        | ParseErrorKind::ModifierDuplicate
                ) || self.tokens[saved_pos..self.pos]
                    .iter()
                    .any(|token| token.kind == TokenKind::Therefore) =>
            {
                Err(err)
            }
            Err(_) => {
                self.restore(saved);
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
            self.expect_issue(&TokenKind::RParen, "closure_parameters_end")?;
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
        self.expect_issue(&TokenKind::Therefore, "closure_body_marker")?;
        let body = self.parse_compact_clausura_body()?;

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Clausura(Box::new(ClausuraExpr {
                params,
                ret,
                err,
                body,
            })),
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
            if !self.consume_item_separator(end, "closure_parameters")? {
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
        self.expect_keyword_ident(TokenKind::Ad)?;
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
            self.expect_issue(&TokenKind::RParen, "ad_opener_end")?;
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
    ///   `template_argument` := (IDENTIFIER ':')? expression
    ///
    /// Arguments can use `sparge` to unpack arrays or objects. Labeled actuals
    /// are admitted only when the caller has identified a direct string or
    /// forma literal callee. The closing `)` is left to the caller so this
    /// helper can be shared by normal and optional call postfix parsing.
    pub(super) fn parse_argument_list(
        &mut self,
        allows_template_labels: bool,
    ) -> Result<Vec<Argument>, ParseError> {
        let mut args = Vec::new();
        let mut labels = HashSet::new();

        while !self.check(&TokenKind::RParen) && !self.is_at_end() {
            let start = self.current_span();
            let labeled = if self.argument_label_start() {
                let label = self.parse_contextual_ident(KeywordOwner::DeclarationName)?;
                let label_span = label.span;
                if !allows_template_labels {
                    return Err(ParseError::structured(ParseErrorKind::Expected, label_span)
                        .with_arg("issue", "labeled_actual_non_template_call")
                        .with_arg("label", self.interner.resolve(label.name).to_owned()));
                }
                self.expect_issue(&TokenKind::Colon, "argument_label_separator")?;
                Some((label.name, label_span))
            } else {
                None
            };

            let axis = self.parse_axis_marker();
            let marker = if axis {
                None
            } else {
                self.parse_argument_marker()
            };
            let spread = self.eat_keyword_ident(TokenKind::Sparge);
            if let Some((_, label_span)) = labeled
                && spread
            {
                return Err(ParseError::structured(ParseErrorKind::Expected, label_span)
                    .with_arg("issue", "labeled_actual_spread_unsupported"));
            }

            if self.argument_bind_start() {
                let bind_span = self.peek_at(1).span;
                let label = match self.peek().kind {
                    TokenKind::Ident(symbol) => self.interner.resolve(symbol).to_owned(),
                    TokenKind::Tag => "tag".to_owned(),
                    _ => String::new(),
                };
                return Err(ParseError::structured(ParseErrorKind::Expected, bind_span)
                    .with_arg("issue", "label_bind_distinction")
                    .with_arg("label", label)
                    .with_arg("label_operator", ":")
                    .with_arg("binding_operator", "="));
            }

            if let Some((label, label_span)) = labeled
                && !labels.insert(label)
            {
                return Err(ParseError::structured(ParseErrorKind::Expected, label_span)
                    .with_arg("issue", "duplicate_tuple_element_label")
                    .with_arg("label", self.interner.resolve(label).to_owned()));
            }

            let value = Box::new(self.parse_expression()?);
            let span = start.merge(self.previous_span());
            args.push(Argument {
                label: labeled.map(|(name, _)| name),
                is_spread: spread,
                marker,
                axis,
                value,
                span,
            });

            if !self.consume_item_separator(&TokenKind::RParen, "call_arguments")? {
                break;
            }
        }

        Ok(args)
    }

    /// Parse an optional call-site access marker (`ref`/`mut`/`own`/`copy`;
    /// Latin `de`/`in`/`own`/`copy`) before an argument expression.
    ///
    /// The word is a marker only when an expression start follows (an
    /// identifier, literal or `[`) and the pair is not the head of a compact closure
    /// (`in x ∴ body`); otherwise it stays an ordinary identifier argument.
    fn parse_argument_marker(&mut self) -> Option<TypeMode> {
        let mode = if self.check_keyword_ident(TokenKind::De) {
            TypeMode::De
        } else if self.check_keyword_ident(TokenKind::Varia) {
            TypeMode::In
        } else if self.check_keyword_ident(TokenKind::Own) {
            TypeMode::Own
        } else if self.check_keyword_ident(TokenKind::Copy) {
            // Operator ruling 2026-10-08: call-site `copy` is a plain echo of the
            // parameter marker, not the parked assignment-site copy.
            TypeMode::Copy
        } else {
            return None;
        };
        if !self.marker_word_precedes_operand() {
            return None;
        }
        self.advance();
        Some(mode)
    }

    /// Parse an optional value-side marker (`copy x`, `own x`) at the start of
    /// an initializer or assignment right side.
    ///
    /// The word is a marker only when an operand follows, exactly as at a call
    /// argument (`copy` and `own` stay contextual: a binding named `copy` with
    /// no operand after it is still an identifier). `allowed` restricts the
    /// markers claimed here; `None` claims `copy` and `own` (declaration
    /// initializers), `Some(mode)` claims only that mode (assignment).
    pub(super) fn parse_value_marker(&mut self, allowed: Option<TypeMode>) -> Option<TypeMode> {
        let mode = if self.check_keyword_ident(TokenKind::Copy) {
            TypeMode::Copy
        } else if self.check_keyword_ident(TokenKind::Own) {
            TypeMode::Own
        } else {
            return None;
        };
        if allowed.is_some_and(|only| only != mode) || !self.marker_word_precedes_operand() {
            return None;
        }
        self.advance();
        Some(mode)
    }

    /// Parse the `axis` marker word before a call argument or a parameter
    /// type (`t.mean(axis 1)`, `axis int a`).
    ///
    /// `axis` is a marker only when an operand follows and the pair is not a
    /// compact closure head; otherwise it stays an ordinary identifier. In a
    /// parameter list the operand is a type, so only the word's own position
    /// is checked there.
    pub(super) fn parse_axis_marker(&mut self) -> bool {
        if !self.check_keyword_ident(TokenKind::Axis) || !self.marker_word_precedes_operand() {
            return false;
        }
        self.advance();
        true
    }

    /// Whether the marker word at the cursor is followed by an operand start
    /// and is not the head of a compact closure (`in x ∴ body`).
    ///
    /// A negative integer literal counts as an operand when the sign is glued
    /// to the digits and separated from the word by whitespace (`axis -1`);
    /// `axis - 1` and `axis-1` stay a binary subtraction of the word. In that
    /// negative form the closure head sits after the integer (`peek_at(3)`).
    fn marker_word_precedes_operand(&self) -> bool {
        if matches!(self.peek_at(1).kind, TokenKind::Minus)
            && matches!(self.peek_at(2).kind, TokenKind::Integer(_))
        {
            let word = self.peek_at(0).span;
            let minus = self.peek_at(1).span;
            let integer = self.peek_at(2).span;
            return word.end < minus.start
                && minus.end == integer.start
                && !matches!(
                    self.peek_at(3).kind,
                    TokenKind::Therefore | TokenKind::Arrow
                );
        }
        matches!(
            self.peek_at(1).kind,
            TokenKind::Ident(_)
                | TokenKind::Tag
                | TokenKind::Integer(_)
                | TokenKind::Float(_)
                | TokenKind::FloatText { .. }
                | TokenKind::String(_)
                | TokenKind::LBracket
        ) && !matches!(
            self.peek_at(2).kind,
            TokenKind::Therefore | TokenKind::Arrow
        )
    }

    fn argument_label_start(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::Tag)
            && self.peek_at(1).kind == TokenKind::Colon
    }

    fn argument_bind_start(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::Tag)
            && self.peek_at(1).kind == TokenKind::Eq
    }

    fn parse_array_elements(&mut self) -> Result<Vec<ArrayElement>, ParseError> {
        let mut elements = Vec::new();

        while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
            if self.eat_keyword_ident(TokenKind::Sparge) {
                elements.push(ArrayElement::Spread(Box::new(self.parse_expression()?)));
            } else if let TokenKind::Underscore(_) = self.peek().kind {
                let span = self.current_span();
                self.advance();
                elements.push(ArrayElement::Infer(span));
            } else {
                elements.push(ArrayElement::Expr(Box::new(self.parse_expression()?)));
            }

            if !self.consume_item_separator(&TokenKind::RBracket, "array_elements")? {
                break;
            }
        }

        Ok(elements)
    }

    /// Parse the optional copy-with-changes source after a construction
    /// literal's closing `}` (DECISIONS D15.1–D15.2).
    ///
    /// GRAMMAR:
    ///   construction-source := `ex` postfix-expr
    ///
    /// The source becomes the literal's last field (`ObjectKey::Spread`):
    /// every field the braces do not name is copied from it. The suffix must
    /// start on the closing brace's line, because a line-leading `ex` is the
    /// extraction statement (`ex p fixum x`). Exactly one source is legal, so
    /// a second same-line `ex` is `construction_source_repeated`.
    fn parse_construction_source(
        &mut self,
        fields: &mut Vec<ObjectField>,
    ) -> Result<(), ParseError> {
        if !self.check_keyword_ident(TokenKind::Ex) || self.raw_newline_before_next_word() {
            return Ok(());
        }
        let start = self.current_span();
        self.advance();
        let previous = self.conversio_postfix_enabled;
        self.conversio_postfix_enabled = false;
        let parsed_source = self.parse_postfix_inner();
        self.conversio_postfix_enabled = previous;
        let source = parsed_source?;
        if self.check_keyword_ident(TokenKind::Ex) && !self.raw_newline_before_next_word() {
            return Err(
                ParseError::structured(ParseErrorKind::Unexpected, self.current_span())
                    .with_arg("issue", "construction_source_repeated"),
            );
        }
        let span = start.merge(self.previous_span());
        fields.push(ObjectField {
            key: ObjectKey::Spread(Box::new(source)),
            value: None,
            span,
        });
        Ok(())
    }

    fn parse_object_fields(&mut self) -> Result<Vec<ObjectField>, ParseError> {
        let mut fields = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let start = self.current_span();

            // `sparge` left construction literals (DECISIONS D15.4); copying
            // a value with changes is `Genus { … } ex source` (D15.1).
            if self.check_keyword_ident(TokenKind::Sparge) {
                return Err(ParseError::structured(ParseErrorKind::Unexpected, start)
                    .with_arg("issue", "construction_spread_removed"));
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
                self.expect_issue(&TokenKind::RBracket, "computed_key_end")?;
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

            if !self.consume_item_separator(&TokenKind::RBrace, "object_fields")? {
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
    fn parse_json_literal(&mut self) -> Result<ExprKind, ParseError> {
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
            self.expect_issue(&TokenKind::Colon, "json_key_separator")?;
            let value = self.parse_json_value()?;
            members.push(JsonMember {
                key_span,
                key,
                value,
            });
            if !self.consume_item_separator(&TokenKind::RBrace, "json_object_members")? {
                break;
            }
        }
        self.expect_issue(&TokenKind::RBrace, "json_object_end")?;
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
    ///
    /// Nested objects and arrays re-enter here through
    /// [`Parser::parse_json_object_value`] and [`Parser::parse_json_array_value`],
    /// so the depth budget is charged at this entry point.
    fn parse_json_value(&mut self) -> Result<JsonValue, ParseError> {
        let start = self.current_span();
        self.nested(ParseErrorKind::ExpressionInvalid, start, |parser| {
            parser.parse_json_value_body()
        })
    }

    /// JSON value body beneath the depth budget; see [`Parser::parse_json_value`].
    fn parse_json_value_body(&mut self) -> Result<JsonValue, ParseError> {
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
            TokenKind::BigInteger(_) => Err(self.json_unexpected_error("json_integer_overflow")),
            TokenKind::Float(n) => {
                let n = *n;
                self.advance();
                Ok(JsonValue::Fractus(n))
            }
            TokenKind::FloatText { value, .. } => {
                let n = *value;
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
            if !self.consume_item_separator(&TokenKind::RBracket, "json_array_elements")? {
                break;
            }
        }
        self.expect_issue(&TokenKind::RBracket, "json_array_end")?;
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
            TokenKind::BigInteger(_) => Err(self.json_unexpected_error("json_integer_underflow")),
            TokenKind::Float(n) => {
                let n = *n;
                self.advance();
                Ok(JsonValue::Fractus(-n))
            }
            TokenKind::FloatText { value, .. } => {
                let n = -*value;
                self.advance();
                Ok(JsonValue::Fractus(n))
            }
            _ => Err(self.json_unexpected_error("expected_json_number_after_minus")),
        }
    }

    fn json_unexpected_error(&self, issue: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::Unexpected, self.current_span())
            .with_arg("issue", issue)
    }

    /// Finish `_ { field = value, … }` (or the bare `{ … }` form) once the
    /// opening brace is consumed: fields, closing brace, and the `Verte` node
    /// with an `Infer` target. `bare` is `true` for the bare literal, whose
    /// type slot was left out; that records [`TypeFormLevel::BARE_LITERAL`] so
    /// the checker can default it to a record, while the explicit `_ { … }`
    /// hole keeps [`TypeFormLevel::LONG`] and no default.
    fn finish_inferred_field_constructor(
        &mut self,
        hole_span: Span,
        bare: bool,
    ) -> Result<ExprKind, ParseError> {
        let mut fields = self.parse_object_fields()?;
        self.expect_issue(&TokenKind::RBrace, "inferred_field_constructor_end")?;
        self.parse_construction_source(&mut fields)?;
        let object = Expr {
            id: self.next_id(),
            kind: ExprKind::Object(ObjectExpr { fields }),
            span: hole_span.merge(self.previous_span()),
        };
        let ty = TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Infer,
            span: hole_span,
            form: if bare {
                TypeFormLevel::BARE_LITERAL
            } else {
                TypeFormLevel::LONG
            },
            iuncta_label: None,
        };
        Ok(ExprKind::Verte(VerteExpr {
            expr: Box::new(object),
            ty,
        }))
    }

    /// Keyed-entries lookahead for the `{` at the cursor: the first member
    /// token is a quoted key followed by `:`. That is JSON member grammar, so
    /// the block is the bare literal of the stated keyed constructor; the
    /// `name = value` field form stays with the typed-constructor lookahead.
    fn looks_like_keyed_constructor_entries(&self) -> bool {
        matches!(self.peek().kind, TokenKind::LBrace)
            && matches!(self.peek_at(1).kind, TokenKind::String(_))
            && matches!(self.peek_at(2).kind, TokenKind::Colon)
    }

    fn looks_like_typed_constructor_fields(&self) -> bool {
        self.looks_like_typed_constructor_fields_at(0)
    }

    /// Field-list lookahead for a `{` at offset `brace` from the cursor.
    fn looks_like_typed_constructor_fields_at(&self, brace: usize) -> bool {
        // A leading `sparge` routes here so the removed construction spread
        // (D15.4) gets its named diagnostic instead of a generic parse error.
        matches!(self.peek_at(brace + 1).kind, TokenKind::RBrace)
            || self.keyword_ident_at(brace + 1) == Some(TokenKind::Sparge)
            || matches!(
                self.peek_at(brace + 1).kind,
                TokenKind::Ident(_) | TokenKind::String(_)
            ) && matches!(self.peek_at(brace + 2).kind, TokenKind::Eq)
    }

    fn expect_field_value_separator(&mut self) -> Result<(), ParseError> {
        if self.eat(&TokenKind::Eq) {
            return Ok(());
        }

        Err(self.expected_expression_error("field_value_separator"))
    }

    /// Parse `< typeArgument (, typeArgument)* >` after the opening `<` has been consumed.
    ///
    /// A call-site list is unambiguously a `typeArguments` position, so every
    /// argument keeps the explicit `Type` shape — including NATURAL
    /// (`linear_from_raw<size 2, size 2, size 2>(…)`) — instead of the kinded split used by
    /// generic-head applications.
    fn finish_type_args_list(&mut self) -> Result<Vec<GenericArgExpr>, ParseError> {
        let mut args = Vec::new();
        loop {
            args.push(self.parse_explicit_type_argument()?);
            if !self.consume_item_separator(&TokenKind::GtAscii, "type_arguments")? {
                break;
            }
        }

        self.expect_issue(&TokenKind::GtAscii, "call_type_arguments_end")?;
        Ok(args)
    }

    /// Parse a generic-head argument list `< arg (, arg)* >` for an applied
    /// constructor head (`Gen<3> { … }`).
    ///
    /// Mirrors the generic-head production in type position: natural literals
    /// and shape tuples are size expressions, other arguments are types.
    fn parse_constructor_type_args(&mut self) -> Result<Vec<GenericArgExpr>, ParseError> {
        self.expect_issue(&TokenKind::LtAscii, "constructor_type_arguments_start")?;
        let mut args = Vec::new();
        loop {
            args.push(self.parse_type_argument()?);
            if !self.consume_item_separator(&TokenKind::GtAscii, "type_arguments")? {
                break;
            }
        }
        self.expect_issue(&TokenKind::GtAscii, "constructor_type_arguments_end")?;
        Ok(args)
    }

    /// Parse required call-site type arguments after speculative `<` lookahead.
    ///
    /// The postfix loop has already confirmed `<` is present; this helper consumes
    /// it and requires a well-formed `< type (, type)* >` list before `(`.
    fn parse_call_type_args(&mut self) -> Result<Vec<GenericArgExpr>, ParseError> {
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
        let TokenKind::Ident(symbol) = self.peek().kind else {
            return Err(self.expected_expression_error("scriptum"));
        };
        self.expect_keyword_ident(TokenKind::Scriptum)?;
        self.expect_issue(&TokenKind::LParen, "scriptum_start")?;

        let template = self.parse_string()?;

        let mut args = Vec::new();
        loop {
            if !self.eat(&TokenKind::Comma) {
                if self.starts_item_list_entry() {
                    return Err(ParseError::structured(
                        ParseErrorKind::Expected,
                        self.current_span(),
                    )
                    .with_arg("issue", "item_list_missing_separator")
                    .with_arg("family", "item_list")
                    .with_arg("position", "scriptum_arguments")
                    .with_arg("expected", "comma between items"));
                }
                break;
            }
            let comma_span = self.previous_span();
            if self.check(&TokenKind::RParen) {
                return Err(ParseError::structured(ParseErrorKind::Expected, comma_span)
                    .with_arg("issue", "item_list_trailing_comma")
                    .with_arg("family", "item_list")
                    .with_arg("position", "scriptum_arguments"));
            }
            args.push(self.parse_expression()?);
        }

        self.expect_issue(&TokenKind::RParen, "scriptum_end")?;

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Scriptum(ScriptumExpr {
                symbol,
                template,
                args,
            }),
            span,
        })
    }

    /// Parse `lege` and `lineam` input builtins.
    fn parse_lege_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let TokenKind::Ident(symbol) = self.peek().kind else {
            return Err(self.expected_expression_error("lege"));
        };
        let line = if self.eat_keyword_ident(TokenKind::Lineam) {
            true
        } else if self.eat_keyword_ident(TokenKind::Lege) {
            false
        } else {
            return Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "lege_lineam")
                    .with_arg("lege", self.keyword_surface("lege"))
                    .with_arg("lineam", self.keyword_surface("lineam")),
            );
        };

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Lege(LegeExpr {
                symbol,
                is_line: line,
                span,
            }),
            span,
        })
    }

    /// Parse `praefixum { … }`, the build-time initializer body of a static
    /// (DECISIONS D8.4). The retired Zig-era `praefixum(expr)` form gets the
    /// migration diagnostic `praefixum_expression_form_retired`.
    ///
    /// GRAMMAR:
    ///   praefixum-expr := `praefixum` block
    fn parse_praefixum_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Praefixum)?;
        if self.check(&TokenKind::LParen) {
            return Err(self.invalid_expression_error("praefixum_expression_form_retired"));
        }
        let body = self.parse_block()?;

        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Praefixum(PraefixumExpr { body }),
            span,
        })
    }

    /// Parse `insere "path"`, the build-time file embed (DECISIONS D8.10).
    /// The caller has matched the keyword followed by the string literal
    /// `path`.
    ///
    /// GRAMMAR:
    ///   insere-expr := `insere` STRING
    fn parse_insere_expr(&mut self, path: Symbol) -> Expr {
        let start = self.current_span();
        self.advance();
        let path_span = self.current_span();
        self.advance();
        Expr {
            id: self.next_id(),
            kind: ExprKind::Insere(InsereExpr { path, path_span }),
            span: start.merge(path_span),
        }
    }

    /// Parse `capta { ... }`, the capture-boundary expression.
    ///
    /// GRAMMAR:
    ///   trap-expr := `capta` block
    ///
    /// The trailing expression statement of the block is the success value;
    /// `parse_block` owns the brace boundary. No `cape` clause, `dum` tail, or
    /// early-success form attaches here — those belong to `fac` and open row 1.
    fn parse_trap_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Trap)?;
        let body = self.parse_block()?;
        let span = start.merge(self.previous_span());
        let id = self.next_id();
        Ok(Expr {
            id,
            kind: ExprKind::Trap(Box::new(TrapExpr { body, span })),
            span,
        })
    }
}

#[cfg(test)]
#[path = "expr_test.rs"]
mod tests;
