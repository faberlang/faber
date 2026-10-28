//! Pattern grammar for `discerne` arms.
//!
//! Pattern parsing is deliberately narrower than expression parsing. It accepts
//! the shapes that can appear after `casu` inside `discerne`: wildcards,
//! literals, identifiers, qualified paths, aliases, and shallow binding lists.
//! It does not parse arbitrary expressions or nested destructuring trees.
//!
//! The parser records syntactic pattern intent and leaves type-aware matching,
//! variant resolution, and exhaustiveness to later phases.
//!
//! - `et` separates patterns (a comma is a named migration diagnostic), and
//!   body-starting tokens stop the pattern list so `ergo` and `{` remain
//!   statement-body syntax.
//! - Pattern literals are parsed independently from expression literals because
//!   this context accepts constants, not operators or calls.
//! - `sit` and `varia` (en `let`/`mut`) in pattern binds describe captured binding mutability;
//!   they are not declaration statements inside the arm head.

use super::{ParseError, ParseErrorKind, Parser};
use radix_lexer::TokenKind;
use radix_syntax::{ExField, Literal, PathPattern, Pattern, PatternBind};

// PATTERN PARSING

impl Parser {
    /// Parse one or more arm-head patterns.
    ///
    /// GRAMMAR:
    ///   patterns := pattern (`et` pattern)*
    ///   pattern := `pattern_atom` (`aut` `pattern_atom`)*
    ///
    /// `et` is the only subject-pattern separator. A comma is the retired
    /// separator: it is reported once per arm head (`discerne_pattern_comma_retired`)
    /// and read as `et`. `aut` groups alternatives for one subject into a
    /// single AST node. The parser stops before `{` or `ergo` so the arm body
    /// is not consumed as another pattern.
    pub(super) fn parse_patterns(&mut self) -> Result<Vec<Pattern>, ParseError> {
        let mut patterns = Vec::new();
        let mut comma_reported = false;

        loop {
            patterns.push(self.parse_pattern()?);
            if self.eat(&TokenKind::Comma) {
                if !comma_reported {
                    comma_reported = true;
                    self.errors.push(
                        ParseError::structured(
                            ParseErrorKind::PatternInvalid,
                            self.previous_span(),
                        )
                        .with_arg("issue", "discerne_pattern_comma_retired"),
                    );
                }
                if self.check(&TokenKind::LBrace) || self.check_keyword_ident(TokenKind::Ergo) {
                    break;
                }
                continue;
            }
            if self.eat_keyword_ident(TokenKind::Et) {
                continue;
            }
            if self.check(&TokenKind::LBrace) || self.check_keyword_ident(TokenKind::Ergo) {
                break;
            }
            return Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "item_list_missing_separator")
                    .with_arg("family", "item_list")
                    .with_arg("position", "pattern_list")
                    .with_arg("expected", "et between patterns"),
            );
        }

        Ok(patterns)
    }

    /// Parse one subject pattern and its `aut` alternatives.
    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let first = self.parse_pattern_atom()?;
        if !self.eat_keyword_ident(TokenKind::Aut) {
            return Ok(first);
        }

        let mut alternatives = vec![first];
        loop {
            alternatives.push(self.parse_pattern_atom()?);
            if !self.eat_keyword_ident(TokenKind::Aut) {
                break;
            }
        }
        Ok(Pattern::Or(alternatives))
    }

    /// Parse one pattern atom plus its optional binding suffix.
    ///
    /// GRAMMAR:
    ///   `pattern_atom` := `_` | `-` number | literal | type-pattern [`bind`] | ident [`bind`] | path bind
    ///   type-pattern := type-name [type-arguments]
    ///   bind := `ut` ident | (`fixum`|`varia`) binding-list
    ///   binding-list := ident [`ut` ident] (',' ident [`ut` ident])*
    ///
    /// Qualified paths are kept syntactic here; later phases decide whether a
    /// path names an enum variant, union variant, or invalid pattern target.
    fn parse_pattern_atom(&mut self) -> Result<Pattern, ParseError> {
        if let TokenKind::Underscore(_) = self.peek().kind {
            let span = self.peek().span;
            self.advance();
            return Ok(Pattern::Wildcard(span));
        }

        if let Some(literal) = self.try_parse_pattern_literal() {
            return Ok(literal);
        }

        // Type-name lookahead mirrors `est` classification. `nihil` is a
        // deliberate exception to the ordinary literal table: in a `discerne`
        // arm it denotes the null member of the scrutinee union. Parse the
        // complete type expression so bounded families (`lista<T, magnitudo N>`) and
        // tensor figures remain available to lowering.
        if (self.check_keyword_ident(TokenKind::Nihil)
            || self.check_null_type_word()
            || self.peek_starts_type_name())
            && !matches!(self.peek_at(1).kind, TokenKind::Dot)
        {
            let ty = self.parse_type()?;
            let bind = self.parse_pattern_bind()?;
            return Ok(Pattern::Type(ty, bind));
        }

        if let TokenKind::Ident(_) = self.peek().kind {
            let start = self.current_span();
            let mut segments = Vec::new();
            segments.push(self.parse_ident()?);

            while self.eat(&TokenKind::Dot) {
                segments.push(self.parse_ident()?);
            }

            let bind = self.parse_pattern_bind()?;

            if segments.len() == 1 {
                return Ok(Pattern::Ident(segments.remove(0), bind));
            }

            let span = start.merge(self.previous_span());
            return Ok(Pattern::Path(PathPattern {
                segments,
                bind,
                span,
            }));
        }

        Err(
            ParseError::structured(ParseErrorKind::PatternInvalid, self.current_span())
                .with_arg("wildcard", "_")
                .with_arg("identifier", "identifier"),
        )
    }

    /// Parse an optional binding suffix for a pattern.
    ///
    /// GRAMMAR:
    ///   bind := `ut` ident | (`sit`|`varia`) ident (',' ident)*
    ///
    /// `ut name` aliases the whole matched value. `sit` and `varia` capture a
    /// shallow list of names with the requested mutability; deeper shape checks
    /// belong to semantic analysis.
    fn parse_pattern_bind(&mut self) -> Result<Option<PatternBind>, ParseError> {
        let bind = if self.eat_keyword_ident(TokenKind::Ut) {
            let alias = self.parse_ident()?;
            Some(PatternBind::Alias(alias))
        } else if self.binder_head_word_at(0) {
            let mutability = self.parse_binder_mutability()?;
            let bindings = self.parse_pattern_bind_list()?;
            Some(PatternBind::Bindings {
                mutability,
                bindings,
            })
        } else {
            None
        };

        Ok(bind)
    }

    /// Parse comma-separated payload bindings with optional `ut` aliases.
    fn parse_pattern_bind_list(&mut self) -> Result<Vec<ExField>, ParseError> {
        let mut bindings = Vec::new();
        loop {
            let name = self.parse_ident()?;
            let alias = if self.eat_keyword_ident(TokenKind::Ut) {
                Some(self.parse_ident()?)
            } else {
                None
            };
            bindings.push(ExField { name, alias });
            // L-2: a comma stays a binding comma, except where the next two
            // words are a name and a binder word (`sit`/`varia`) — that is the retired
            // pattern comma, handled by `parse_patterns`.
            if self.check(&TokenKind::Comma) && self.comma_starts_next_pattern() {
                break;
            }
            if self.eat(&TokenKind::Comma) {
                let comma_span = self.previous_span();
                if self.check(&TokenKind::LBrace)
                    || self.check_keyword_ident(TokenKind::Ergo)
                    || self.check_keyword_ident(TokenKind::Et)
                {
                    return Err(ParseError::structured(ParseErrorKind::Expected, comma_span)
                        .with_arg("issue", "item_list_trailing_comma")
                        .with_arg("family", "item_list")
                        .with_arg("position", "pattern_bindings"));
                }
                continue;
            }
            if self.check(&TokenKind::LBrace)
                || self.check_keyword_ident(TokenKind::Ergo)
                || self.check_keyword_ident(TokenKind::Et)
                || self.check_keyword_ident(TokenKind::Aut)
            {
                break;
            }
            return Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "item_list_missing_separator")
                    .with_arg("family", "item_list")
                    .with_arg("position", "pattern_bindings")
                    .with_arg("expected", "comma between bindings"),
            );
        }
        Ok(bindings)
    }

    /// Whether the comma at the cursor is followed by a name and a binding
    /// introducer (`, Name sit`), which can only begin another pattern.
    fn comma_starts_next_pattern(&self) -> bool {
        matches!(self.peek_at(1).kind, TokenKind::Ident(_))
            && matches!(
                self.keyword_ident_at(2),
                Some(TokenKind::Sit | TokenKind::Varia | TokenKind::Fixum | TokenKind::De)
            )
    }

    /// Try to parse a literal pattern in arm-head context.
    ///
    /// This intentionally stops at constants. Operators, calls, and object/array
    /// expressions are not pattern syntax.
    fn try_parse_pattern_literal(&mut self) -> Option<Pattern> {
        // D22.2: the lexer never produces a signed number, so a leading minus
        // directly before a number literal is claimed here as the pattern's
        // sign. `-` before anything else is not pattern syntax.
        if self.check(&TokenKind::Minus) {
            let literal = match self.peek_at(1).kind {
                TokenKind::Integer(n) => Literal::Int(n),
                TokenKind::BigInteger(text) => Literal::BigInt(text),
                TokenKind::Float(n) => Literal::Float(n),
                TokenKind::FloatText { value, text } => Literal::FloatText { value, text },
                _ => return None,
            };
            let start = self.peek().span;
            self.advance();
            let span = start.merge(self.peek().span);
            self.advance();
            return Some(Pattern::NegatedLiteral(literal, span));
        }
        match self.peek().kind {
            TokenKind::Integer(n) => {
                let span = self.peek().span;
                self.advance();
                Some(Pattern::Literal(Literal::Int(n), span))
            }
            TokenKind::BigInteger(text) => {
                let span = self.peek().span;
                self.advance();
                Some(Pattern::Literal(Literal::BigInt(text), span))
            }
            TokenKind::Float(n) => {
                let span = self.peek().span;
                self.advance();
                Some(Pattern::Literal(Literal::Float(n), span))
            }
            TokenKind::FloatText { value, text } => {
                let span = self.peek().span;
                self.advance();
                Some(Pattern::Literal(Literal::FloatText { value, text }, span))
            }
            TokenKind::String(sym) => {
                let span = self.peek().span;
                self.advance();
                Some(Pattern::Literal(Literal::String(sym), span))
            }
            TokenKind::AsciiString(sym) => {
                let span = self.peek().span;
                self.advance();
                Some(Pattern::Literal(Literal::Ascii(sym), span))
            }
            TokenKind::OctetiString(sym) => {
                let span = self.peek().span;
                self.advance();
                Some(Pattern::Literal(Literal::Octeti(sym), span))
            }
            // verum/falsum are claimed by spelling so boolean literals stay
            // literal patterns instead of falling through to name bindings.
            // `nihil` is classified as a type-pattern atom above.
            TokenKind::Ident(_) => {
                let literal = if self.check_keyword_ident(TokenKind::Verum) {
                    Some(Literal::Bool(true))
                } else if self.check_keyword_ident(TokenKind::Falsum) {
                    Some(Literal::Bool(false))
                } else if self.is_nan_literal_claim() {
                    // D22.3: NaN claims the literal slot exactly as in
                    // expressions, so lowering can reject it by name rather
                    // than binding a catch-all local called `nonnumerus`.
                    Some(Literal::Float(f64::NAN))
                } else {
                    None
                };
                literal.map(|literal| {
                    let span = self.peek().span;
                    self.advance();
                    Pattern::Literal(literal, span)
                })
            }
            _ => None,
        }
    }

    /// Whether the current identifier is the locale's NaN spelling in a
    /// literal slot (not a call head), mirroring the expression-primary claim.
    fn is_nan_literal_claim(&self) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        self.keyword_claim_key(sym) == Some("nonnumerus")
            && self.peek_at(1).kind != TokenKind::LParen
    }
}

#[cfg(test)]
#[path = "pattern_test.rs"]
mod tests;

#[cfg(test)]
#[path = "pattern_bind_alias_test.rs"]
mod bind_alias_tests;
