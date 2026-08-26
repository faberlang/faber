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
//! - `,` and `et` separate patterns, but body-starting tokens stop the pattern
//!   list so `ergo` and `{` remain statement-body syntax.
//! - Pattern literals are parsed independently from expression literals because
//!   this context accepts constants, not operators or calls.
//! - `fixum` and `varia` in pattern binds describe captured binding mutability;
//!   they are not declaration statements inside the arm head.

use super::{ParseError, ParseErrorKind, Parser};
use radix_lexer::TokenKind;
use radix_syntax::{AstMutability, ExField, Literal, PathPattern, Pattern, PatternBind};

// PATTERN PARSING

impl Parser {
    /// Parse one or more arm-head patterns.
    ///
    /// GRAMMAR:
    ///   patterns := pattern (',' pattern | `et` pattern)*
    ///
    /// Both comma and `et` are accepted as pattern separators. The parser stops
    /// before `{` or `ergo` so the arm body is not consumed as another pattern.
    pub(super) fn parse_patterns(&mut self) -> Result<Vec<Pattern>, ParseError> {
        let mut patterns = Vec::new();

        loop {
            patterns.push(self.parse_pattern()?);
            if self.eat(&TokenKind::Comma) {
                let comma_span = self.previous_span();
                if self.check(&TokenKind::LBrace) || self.check_keyword_ident(TokenKind::Ergo) {
                    return Err(ParseError::structured(ParseErrorKind::Expected, comma_span)
                        .with_arg("issue", "item_list_trailing_comma")
                        .with_arg("family", "item_list")
                        .with_arg("position", "pattern_list"));
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
                    .with_arg("expected", "comma between patterns"),
            );
        }

        Ok(patterns)
    }

    /// Parse one pattern atom plus its optional binding suffix.
    ///
    /// GRAMMAR:
    ///   pattern := `_` | literal | ident [`bind`] | path bind
    ///   bind := `ut` ident | (`fixum`|`varia`) binding-list
    ///   binding-list := ident [`ut` ident] (',' ident [`ut` ident])*
    ///
    /// Qualified paths are kept syntactic here; later phases decide whether a
    /// path names an enum variant, union variant, or invalid pattern target.
    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        if let TokenKind::Underscore(_) = self.peek().kind {
            let span = self.peek().span;
            self.advance();
            return Ok(Pattern::Wildcard(span));
        }

        if let Some(literal) = self.try_parse_pattern_literal() {
            return Ok(literal);
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
    ///   bind := `ut` ident | (`fixum`|`varia`) ident (',' ident)*
    ///
    /// `ut name` aliases the whole matched value. `fixum` and `varia` capture a
    /// shallow list of names with the requested mutability; deeper shape checks
    /// belong to semantic analysis.
    fn parse_pattern_bind(&mut self) -> Result<Option<PatternBind>, ParseError> {
        let bind = if self.eat_keyword_ident(TokenKind::Ut) {
            let alias = self.parse_ident()?;
            Some(PatternBind::Alias(alias))
        } else if self.eat_keyword_ident(TokenKind::Fixum) {
            let bindings = self.parse_pattern_bind_list()?;
            Some(PatternBind::Bindings {
                mutability: AstMutability::Immutable,
                bindings,
            })
        } else if self.eat_keyword_ident(TokenKind::Varia) {
            let bindings = self.parse_pattern_bind_list()?;
            Some(PatternBind::Bindings {
                mutability: AstMutability::Mutable,
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

    /// Try to parse a literal pattern in arm-head context.
    ///
    /// This intentionally stops at constants. Operators, calls, and object/array
    /// expressions are not pattern syntax.
    fn try_parse_pattern_literal(&mut self) -> Option<Pattern> {
        match self.peek().kind {
            TokenKind::Integer(n) => {
                let span = self.peek().span;
                self.advance();
                Some(Pattern::Literal(Literal::Int(n), span))
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
            // verum/falsum/nihil are claimed by spelling so `casu verum` stays a
            // literal pattern instead of falling through to a name binding.
            TokenKind::Ident(_) => {
                let literal = if self.check_keyword_ident(TokenKind::Verum) {
                    Some(Literal::Bool(true))
                } else if self.check_keyword_ident(TokenKind::Falsum) {
                    Some(Literal::Bool(false))
                } else if self.check_keyword_ident(TokenKind::Nihil) {
                    Some(Literal::Nil)
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
}

#[cfg(test)]
#[path = "pattern_test.rs"]
mod tests;

#[cfg(test)]
#[path = "pattern_bind_alias_test.rs"]
mod bind_alias_tests;
