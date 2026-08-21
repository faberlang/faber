//! Type-position grammar for declaration and signature parsing.
//!
//! This module is the parser boundary for syntax that is known to be in type
//! position: variable and field declarations, parameters, aliases, function
//! signatures, casts, and annotation schemas. It does not decide whether a type
//! exists or whether generic arity is valid; it only builds the source shape that
//! later phases resolve.
//!
//! Faber type syntax is explicit and conservative. Ownership modes are prefix
//! markers (`de`, `in`, `own`, `copy`), arrays use `lista<T>`, function types use arrow
//! syntax, and nullable value slots are expressed by unioning with `nihil`.
//! Optional declaration slots are not type suffixes; they are parsed as
//! post-name `sponte` markers by declaration parsers.
//!
//! - There is no `Type?` grammar and no nullable prefix shorthand.
//! - `ignotum` is just a named type at this layer, not a nullability marker.
//! - `nihil` may appear as a type member because it is accepted in type position.
//! - Union parsing flattens nested union ASTs when no member-level mode or legacy
//!   nullable flag would be lost.

use super::{ParseError, ParseErrorKind, Parser};
use radix_lexer::{KeywordOwner, Span, Symbol, TokenKind};
use radix_syntax::{FiguraExpr, FuncTypeExpr, Ident, TypeExpr, TypeExprKind, TypeMode};
use std::collections::HashSet;

// TYPE PARSING

impl Parser {
    /// Try to parse a type expression without requiring one.
    ///
    /// This helper is intentionally conservative: it only enters type parsing on
    /// tokens that can begin canonical type syntax. Callers that know they are in
    /// type position should call `parse_type` and receive a diagnostic instead.
    #[allow(dead_code)]
    pub(super) fn try_parse_type(&mut self) -> Result<Option<TypeExpr>, ParseError> {
        if self.check_keyword_ident(TokenKind::De)
            || self.check_keyword_ident(TokenKind::In)
            || self.check_keyword_ident(TokenKind::Own)
            || self.check_keyword_ident(TokenKind::Copy)
            || self.check_keyword_ident(TokenKind::Iuncta)
            || self.check_keyword_ident(TokenKind::Modulus)
            || matches!(self.peek().kind, TokenKind::Underscore(_))
            || matches!(self.peek().kind, TokenKind::Ident(_))
            || self.check(&TokenKind::LParen)
            || self.check(&TokenKind::Cup)
        {
            Ok(Some(self.parse_type()?))
        } else {
            Ok(None)
        }
    }

    /// Parse a required type expression.
    ///
    /// GRAMMAR:
    ///   type := [de|in|own|copy] (func-type | named-type | union-type)
    ///   func-type := ( type-list ) arrow type
    ///   named-type := ident [ < type-argument (, type-argument)* > ]
    ///   type-argument := type | NATURAL
    ///   union-type := type cup type (cup type)*
    ///
    /// Nullable value types are represented in pure type syntax as `T ∪ nihil`.
    /// Declaration-level optional slots are parsed outside this module as
    /// `sponte` markers after the binding name. `nihil` is accepted here as a
    /// keyword-backed type name because it also exists as a value literal.
    pub(super) fn parse_type(&mut self) -> Result<TypeExpr, ParseError> {
        let start = self.current_span();

        // Access marker: de/in/own/copy (prefix). Nullable is no longer a prefix; use T ∪ nihil.
        let mode = if self.eat_keyword_ident(TokenKind::De) {
            Some(TypeMode::De)
        } else if self.eat_keyword_ident(TokenKind::In) {
            Some(TypeMode::In)
        } else if self.eat_keyword_ident(TokenKind::Own) {
            Some(TypeMode::Own)
        } else if self.eat_keyword_ident(TokenKind::Copy) {
            Some(TypeMode::Copy)
        } else {
            None
        };

        if self.check(&TokenKind::LParen) {
            let func = self.parse_func_type()?;
            let span = start.merge(self.previous_span());
            let core = TypeExpr {
                nullable: false,
                mode,
                kind: TypeExprKind::Func(func),
                span,
                iuncta_label: None,
            };
            return self.parse_union_tail(core, start);
        }

        // Standalone `∪` is a union hole. A second cup is a parse error; the
        // hole consumes the whole type expression.
        let kind = if matches!(self.peek().kind, TokenKind::Underscore(_)) {
            self.advance();
            TypeExprKind::Infer
        } else if self.check(&TokenKind::Cup) {
            let cup_span = self.peek().span;
            self.advance();
            if self.check(&TokenKind::Cup) {
                return Err(
                    ParseError::structured(ParseErrorKind::InvalidType, cup_span)
                        .with_arg("issue", "unexpected_cup_after_union_hole"),
                );
            }
            TypeExprKind::UnionHole
        } else {
            let name = if self.check_keyword_ident(TokenKind::Nihil) {
                let span = self.peek().span;
                self.advance();
                self.keyword_ident("nihil", span)
            } else if self.check_keyword_ident(TokenKind::Modulus) {
                let span = self.peek().span;
                self.advance();
                self.keyword_ident("modulus", span)
            } else if self.check_keyword_ident(TokenKind::Iuncta) {
                let span = self.peek().span;
                self.advance();
                self.keyword_ident("iuncta", span)
            } else {
                let ident = self.parse_ident()?;
                // Canonicalize a type spelling to its latin identity only for
                // a single-part type. A qualified prefix is an import binding
                // (namespace) and must keep its authored spelling — a binding
                // may coincide with a localized type spelling (e.g. `object`).
                if self.check(&TokenKind::Dot) {
                    ident
                } else {
                    self.canonical_type_ident(ident)
                }
            };

            let mut path = vec![name];
            while self.eat(&TokenKind::Dot) {
                path.push(self.parse_ident()?);
            }

            let Some(last) = path.last() else {
                return Err(self.invalid_type_error("expected_type_name"));
            };
            let name = Ident {
                name: last.name,
                span: last.span,
            };
            let name_str = self.interner.resolve(name.name).to_owned();
            let kind = if let Some(width) =
                radix_types::numeric_width::lista_sugar_width_marker(&name_str)
            {
                self.parse_lista_width_sugar(name, width)?
            } else if let Some(width) =
                radix_types::numeric_width::tensor_sugar_width_marker(&name_str)
            {
                self.parse_tensor_width_sugar(name, width)?
            } else if let Some(width) =
                radix_types::numeric_width::vector_sugar_width_marker(&name_str)
            {
                self.parse_vector_width_sugar(name, width)?
            } else if let Some(width) =
                radix_types::numeric_width::matrix_sugar_width_marker(&name_str)
            {
                self.parse_matrix_width_sugar(name, width)?
            } else if let Some(width) =
                radix_types::numeric_width::sparsa_sugar_width_marker(&name_str)
            {
                self.parse_sparsa_width_sugar(name, width)?
            } else if self.eat(&TokenKind::LtAscii) {
                if name_str == "atomic" {
                    let elem = Box::new(self.parse_type()?);
                    if self.eat(&TokenKind::Comma) {
                        return Err(ParseError::structured(
                            ParseErrorKind::Expected,
                            self.current_span(),
                        )
                        .with_arg("issue", "atomic_single_element"));
                    }
                    self.expect(&TokenKind::GtAscii, "expected '>'")?;
                    TypeExprKind::Atomic(elem)
                } else if name_str == "promissum" {
                    let success = Box::new(self.parse_type()?);
                    let kind = if self.eat(&TokenKind::ExitArrow) {
                        let alternate = Box::new(self.parse_type()?);
                        self.expect(&TokenKind::GtAscii, "expected '>'")?;
                        TypeExprKind::PromissumFailable(success, alternate)
                    } else {
                        if self.eat(&TokenKind::Comma) {
                            return Err(ParseError::structured(
                                ParseErrorKind::InvalidType,
                                self.current_span(),
                            )
                            .with_arg("issue", "promissum_alternate_uses_exit_arrow"));
                        }
                        self.expect(&TokenKind::GtAscii, "expected '>'")?;
                        TypeExprKind::Named(name, vec![*success])
                    };
                    kind
                } else if name_str == "tensor"
                    || name_str == "sparsa"
                    || name_str == "vector"
                    || name_str == "matrix"
                {
                    let elem = Box::new(self.parse_type()?);
                    let issue = if name_str == "tensor" {
                        "tensor_figura_after_element"
                    } else if name_str == "sparsa" {
                        "sparsa_figura_after_element"
                    } else if name_str == "matrix" {
                        "matrix_figura_after_element"
                    } else {
                        "vector_width_after_element"
                    };
                    if !self.eat(&TokenKind::Comma) {
                        return Err(ParseError::structured(
                            ParseErrorKind::Expected,
                            self.current_span(),
                        )
                        .with_arg("issue", issue));
                    }
                    let shape = self.parse_figura()?;
                    self.expect(&TokenKind::GtAscii, "expected '>'")?;
                    if name_str == "tensor" {
                        TypeExprKind::Tensor(elem, shape)
                    } else if name_str == "sparsa" {
                        TypeExprKind::Sparsa(elem, shape)
                    } else if name_str == "matrix" {
                        TypeExprKind::Matrix(elem, shape)
                    } else {
                        TypeExprKind::Vector(elem, shape)
                    }
                } else {
                    let mut params = Vec::new();
                    let mut labels = HashSet::new();
                    loop {
                        let param = if name_str == "iuncta" {
                            self.parse_iuncta_type_argument(&mut labels)?
                        } else {
                            self.parse_type_argument()?
                        };
                        params.push(param);
                        if !self.consume_item_separator(&TokenKind::GtAscii, "type_parameters")? {
                            break;
                        }
                    }
                    self.expect(&TokenKind::GtAscii, "expected '>'")?;
                    if name_str == "iuncta" && params.len() > 16 {
                        return Err(
                            ParseError::structured(ParseErrorKind::InvalidType, name.span)
                                .with_arg("issue", "iuncta_arity_exceeded")
                                .with_arg("arity", params.len().to_string()),
                        );
                    }
                    if path.len() == 1 {
                        TypeExprKind::Named(name, params)
                    } else {
                        TypeExprKind::Qualified(path, params)
                    }
                }
            } else {
                if path.len() == 1 {
                    TypeExprKind::Named(name, Vec::new())
                } else {
                    TypeExprKind::Qualified(path, Vec::new())
                }
            };
            kind
        };

        if self.check(&TokenKind::LBracket) {
            return Err(self.invalid_type_error("postfix_array_syntax"));
        }

        let span = start.merge(self.previous_span());
        let core = TypeExpr {
            nullable: false,
            mode,
            kind,
            span,
            iuncta_label: None,
        };
        self.parse_union_tail(core, start)
    }

    /// Consume a trailing union chain after a core type.
    ///
    /// The parser flattens nested union members so later phases do not need to
    /// distinguish `A ∪ (B ∪ C)` from `A ∪ B ∪ C`. Member-level mode/nullable
    /// state is preserved by declining to flatten when it would change meaning.
    fn parse_union_tail(&mut self, first: TypeExpr, start: Span) -> Result<TypeExpr, ParseError> {
        if !self.eat(&TokenKind::Cup) {
            return Ok(first);
        }
        // A union hole consumes the whole type expression and can never be a
        // binary-cup member. The hole arm rejects a directly-following cup;
        // this guards the `first` slot defensively against future callers.
        if matches!(first.kind, TypeExprKind::UnionHole) {
            return Err(ParseError::structured(ParseErrorKind::InvalidType, start)
                .with_arg("issue", "unexpected_cup_after_union_hole"));
        }
        let mut members = Vec::new();
        flatten_union_member(first, &mut members);
        loop {
            let member = self.parse_type()?;
            if matches!(member.kind, TypeExprKind::UnionHole) {
                return Err(
                    ParseError::structured(ParseErrorKind::InvalidType, member.span)
                        .with_arg("issue", "unexpected_cup_after_union_hole"),
                );
            }
            flatten_union_member(member, &mut members);
            if !self.eat(&TokenKind::Cup) {
                break;
            }
        }
        let span = start.merge(self.previous_span());
        Ok(TypeExpr {
            nullable: false,
            mode: None, // modes belong on members or parenthesized forms (future)
            kind: TypeExprKind::Union(members),
            span,
            iuncta_label: None,
        })
    }

    /// Parse a function type: `(A, B) → C ⇥ E`.
    ///
    /// GRAMMAR:
    ///   func-type := `(` [type (',' type)*] `)` '→' type ['⇥' type]
    ///
    /// Function type syntax is only recognized after an opening parenthesis in a
    /// known type position, avoiding ambiguity with parenthesized expressions.
    fn parse_func_type(&mut self) -> Result<FuncTypeExpr, ParseError> {
        self.expect(&TokenKind::LParen, "expected '('")?;

        let mut params = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.is_at_end() {
            params.push(self.parse_type()?);
            if !self.consume_item_separator(&TokenKind::RParen, "function_type_parameters")? {
                break;
            }
        }

        self.expect(&TokenKind::RParen, "expected ')'")?;
        self.expect(&TokenKind::Arrow, "expected '→'")?;

        let ret = Box::new(self.parse_type()?);
        let err = if self.eat(&TokenKind::ExitArrow) {
            Some(Box::new(self.parse_type()?))
        } else {
            None
        };

        Ok(FuncTypeExpr { params, ret, err })
    }

    fn parse_lista_width_sugar(
        &mut self,
        name: Ident,
        width: &str,
    ) -> Result<TypeExprKind, ParseError> {
        self.reject_type_sugar_angle_bracket("lista")?;
        if self.check(&TokenKind::LBracket) {
            return Err(self.invalid_type_error("lista_sugar_bracket_form"));
        }
        let elem = self.width_element_type_expr(width, name.span);
        Ok(TypeExprKind::Named(
            self.keyword_ident("lista", name.span),
            vec![elem],
        ))
    }

    fn parse_tensor_width_sugar(
        &mut self,
        name: Ident,
        width: &str,
    ) -> Result<TypeExprKind, ParseError> {
        self.reject_type_sugar_angle_bracket("tensor")?;
        let elem = self.width_element_type_expr(width, name.span);
        let shape = if self.check(&TokenKind::LBracket) {
            self.parse_figura()?
        } else {
            FiguraExpr::Infer
        };
        Ok(TypeExprKind::Tensor(Box::new(elem), shape))
    }

    fn parse_vector_width_sugar(
        &mut self,
        name: Ident,
        width: &str,
    ) -> Result<TypeExprKind, ParseError> {
        self.reject_type_sugar_angle_bracket("vector")?;
        let elem = self.width_element_type_expr(width, name.span);
        let shape = if self.check(&TokenKind::LBracket) {
            self.parse_figura()?
        } else {
            FiguraExpr::Infer
        };
        Ok(TypeExprKind::Vector(Box::new(elem), shape))
    }

    fn parse_matrix_width_sugar(
        &mut self,
        name: Ident,
        width: &str,
    ) -> Result<TypeExprKind, ParseError> {
        self.reject_type_sugar_angle_bracket("matrix")?;
        if !self.check(&TokenKind::LBracket) {
            return Err(self.invalid_type_error("matrix_sugar_missing_shape"));
        }
        let elem = self.width_element_type_expr(width, name.span);
        let shape = self.parse_figura()?;
        Ok(TypeExprKind::Matrix(Box::new(elem), shape))
    }

    fn parse_sparsa_width_sugar(
        &mut self,
        name: Ident,
        width: &str,
    ) -> Result<TypeExprKind, ParseError> {
        self.reject_type_sugar_angle_bracket("sparsa")?;
        let elem = self.width_element_type_expr(width, name.span);
        let shape = if self.check(&TokenKind::LBracket) {
            self.parse_figura()?
        } else {
            FiguraExpr::Infer
        };
        Ok(TypeExprKind::Sparsa(Box::new(elem), shape))
    }

    fn reject_type_sugar_angle_bracket(&mut self, sugar: &'static str) -> Result<(), ParseError> {
        if self.check(&TokenKind::LtAscii) {
            let issue = match sugar {
                "lista" => "lista_sugar_angle_bracket_form",
                "vector" => "vector_sugar_angle_bracket_form",
                _ => "type_sugar_angle_bracket_form",
            };
            let mut err = self.invalid_type_error(issue);
            if issue == "type_sugar_angle_bracket_form" {
                err = err.with_arg("sugar", sugar);
            }
            return Err(err);
        }
        Ok(())
    }

    fn invalid_type_error(&self, issue: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::InvalidType, self.current_span())
            .with_arg("issue", issue)
    }

    fn width_element_type_expr(&mut self, width: &str, span: Span) -> TypeExpr {
        TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(self.keyword_ident(width, span), Vec::new()),
            span,
            iuncta_label: None,
        }
    }

    /// Parse one `iuncta` element, optionally preceded by its contextual label.
    ///
    /// The label rides `TypeExpr::iuncta_label` so later phases can build a
    /// checker-side table without changing `TypeExprKind` identity. `_` is
    /// deliberately excluded from the label lookahead: an unlabeled element is
    /// represented by absence.
    fn parse_iuncta_type_argument(
        &mut self,
        labels: &mut HashSet<Symbol>,
    ) -> Result<TypeExpr, ParseError> {
        let is_label = matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::Tag)
            && matches!(self.peek_at(1).kind, TokenKind::Colon);
        let iuncta_label = if is_label {
            let label = self.parse_contextual_ident(KeywordOwner::DeclarationName)?;
            if !labels.insert(label.name) {
                return Err(ParseError::structured(ParseErrorKind::Expected, label.span)
                    .with_arg("issue", "duplicate_tuple_element_label")
                    .with_arg("label", self.interner.resolve(label.name).to_owned()));
            }
            self.expect(&TokenKind::Colon, "expected ':' after tuple element label")?;
            Some(label)
        } else {
            None
        };
        let mut ty = self.parse_type_argument()?;
        ty.iuncta_label = iuncta_label;
        Ok(ty)
    }

    /// Parse one applied type argument: a type, or a NATURAL capacity.
    ///
    /// NATURAL is interned as a named ident so later phases can read the
    /// decimal spelling. Arity and primitive-params stay resolve errors.
    /// Call-site `typeArguments` share this production with named types.
    pub(super) fn parse_type_argument(&mut self) -> Result<TypeExpr, ParseError> {
        if let TokenKind::Integer(value) = self.peek().kind {
            let span = self.current_span();
            self.advance();
            return Ok(TypeExpr {
                nullable: false,
                mode: None,
                kind: TypeExprKind::Named(self.keyword_ident(&value.to_string(), span), Vec::new()),
                span,
                iuncta_label: None,
            });
        }
        self.parse_type()
    }

    /// Parse a tensor `Figura` in type position.
    ///
    /// GRAMMAR: figura := `_` | natural | ident | [ figura (, figura)* ]
    pub(super) fn parse_figura(&mut self) -> Result<FiguraExpr, ParseError> {
        if matches!(self.peek().kind, TokenKind::Underscore(_)) {
            self.advance();
            return Ok(FiguraExpr::Infer);
        }

        if self.eat(&TokenKind::LBracket) {
            let mut dims = Vec::new();
            if !self.check(&TokenKind::RBracket) {
                loop {
                    dims.push(self.parse_figura()?);
                    if !self.consume_item_separator(&TokenKind::RBracket, "figura_dimensions")? {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RBracket, "expected ']'")?;
            return Ok(FiguraExpr::Tuple(dims));
        }

        if let TokenKind::Integer(value) = self.peek().kind {
            self.advance();
            return Ok(FiguraExpr::Literal(value));
        }

        Ok(FiguraExpr::Var(self.parse_ident()?))
    }
}

fn flatten_union_member(member: TypeExpr, members: &mut Vec<TypeExpr>) {
    if member.nullable || member.mode.is_some() {
        members.push(member);
        return;
    }

    let TypeExpr {
        nullable,
        mode,
        kind,
        span,
        iuncta_label,
    } = member;
    match kind {
        TypeExprKind::Union(nested) => {
            for nested_member in nested {
                flatten_union_member(nested_member, members);
            }
        }
        kind => members.push(TypeExpr {
            nullable,
            mode,
            kind,
            span,
            iuncta_label,
        }),
    }
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
