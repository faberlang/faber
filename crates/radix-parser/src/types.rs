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
use radix_syntax::{
    FiguraExpr, FuncTypeExpr, GenericArgExpr, Ident, LiftExtent, RangeArgExpr, RangeBoundExpr,
    RangeKind, TypeExpr, TypeExprKind, TypeFormLevel, TypeMode,
};
use std::collections::HashSet;

/// Heads whose arguments are capacities spelled in the type surface itself.
///
/// `lista`, `queue`, `stack`, `textus`, and `ascii` carry their bound in the
/// type, so their applied arguments keep the explicit `Type` shape. Generic heads
/// (`Box<T>`, user genera) use the kinded split instead: their parameter kinds
/// are only known from declaration metadata.
fn is_bounded_capacity_head(name: &str) -> bool {
    matches!(name, "lista" | "queue" | "stack" | "textus" | "ascii")
}

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
            || self.check_keyword_ident(TokenKind::Varia)
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
    ///
    /// Nested types (function-type parameters, generic arguments) re-enter
    /// here, so the depth budget is charged at this entry point.
    pub(super) fn parse_type(&mut self) -> Result<TypeExpr, ParseError> {
        let start = self.current_span();
        self.nested(ParseErrorKind::TypeInvalid, start, |parser| {
            parser.parse_type_body(start)
        })
    }

    /// Parse the right-hand side of `est` / `non est` (DECISIONS D23.1).
    ///
    /// The right-hand side is always a type, so this is the ordinary type
    /// parse with one exception: a boolean literal keyword (`verum`/`falsum`)
    /// is not type syntax, but it still parses so the semantic layer can name
    /// the equality operator instead. It is recorded under its canonical latin
    /// spelling, exactly as `nihil` is, so the diagnostic does not depend on
    /// the reader locale.
    pub(super) fn parse_est_type_rhs(&mut self) -> Result<TypeExpr, ParseError> {
        if let Some(error) = self.est_value_literal_error() {
            return Err(error);
        }
        let literal = if self.check_keyword_ident(TokenKind::Verum) {
            Some("verum")
        } else if self.check_keyword_ident(TokenKind::Falsum) {
            Some("falsum")
        } else {
            None
        };
        let Some(latin) = literal else {
            return self.parse_type();
        };
        let span = self.current_span();
        self.advance();
        let name = self.keyword_ident(latin, span);
        Ok(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(name, Vec::new()),
            span,
            form: TypeFormLevel::LONG,
            iuncta_label: None,
        })
    }

    /// A null, number or string literal on the right of `est` / `non est`
    /// (D23-V1): the literal is a value, so the diagnostic names the equality
    /// family (`≡`, `≅`, `≈`) and, for the null value, the null type.
    fn est_value_literal_error(&self) -> Option<ParseError> {
        let (value, span) = if self.check_keyword_ident(TokenKind::Nihil) {
            (self.current_ident_source(), self.current_span())
        } else {
            let sign = matches!(self.peek().kind, TokenKind::Minus);
            let literal = if sign { self.peek_at(1) } else { self.peek() };
            let text = match &literal.kind {
                TokenKind::Integer(n) => n.to_string(),
                TokenKind::Float(f) => f.to_string(),
                TokenKind::FloatText { text, .. } | TokenKind::BigInteger(text) => {
                    self.interner.resolve(*text).to_owned()
                }
                TokenKind::String(sym) => format!("\"{}\"", self.interner.resolve(*sym)),
                _ => return None,
            };
            let value = if sign { format!("-{text}") } else { text };
            (value, self.current_span())
        };
        Some(
            ParseError::structured(ParseErrorKind::TypeInvalid, span)
                .with_arg("issue", "est_value_literal_names_equality")
                .with_arg("value", value)
                .with_arg("type", self.null_type_surface()),
        )
    }

    /// Type body beneath the depth budget; see [`Parser::parse_type`].
    fn parse_type_body(&mut self, start: Span) -> Result<TypeExpr, ParseError> {
        let core = self.parse_type_atom(start)?;
        let core = self.parse_intersection_tail(core, start)?;
        self.parse_union_tail(core, start)
    }

    /// Parse a union-member type: core plus any `∩` chain, but no `∪` tail.
    ///
    /// `∩` binds tighter than `∪`, so a cup member is itself an
    /// intersection-level expression (`A ∩ B ∪ C` is `(A ∩ B) ∪ C`).
    fn parse_union_member(&mut self) -> Result<TypeExpr, ParseError> {
        let start = self.current_span();
        let core = self.parse_type_atom(start)?;
        self.parse_intersection_tail(core, start)
    }

    /// Parse one mode-prefixed, tail-free type core.
    fn parse_type_atom(&mut self, start: Span) -> Result<TypeExpr, ParseError> {
        // Access marker: de/in/own/copy (prefix). Nullable is no longer a prefix; use T ∪ nihil.
        let mode = if self.eat_keyword_ident(TokenKind::De) {
            Some(TypeMode::De)
        } else if self.eat_keyword_ident(TokenKind::Varia) {
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
                form: TypeFormLevel::LONG,
                iuncta_label: None,
            };
            return Ok(core);
        }

        // Standalone `∪` is a union hole. A second cup is a parse error; the
        // hole consumes the whole type expression.
        let (kind, mut form) = self.parse_type_atom_kind()?;

        // `exactus<W>` (en `trapping<W>`) is the policy word spelled out; keep
        // that authored spelling apart from the bare marker for the formatter.
        if matches!(&kind, TypeExprKind::Named(head, args)
            if !args.is_empty() && self.interner.resolve(head.name) == "exactus")
        {
            form = TypeFormLevel::POLICY;
        }
        // `wrapping<inf>` / `saturating<inf>` denote bare `inf` (ruling 32);
        // the level keeps which policy word the author wrote (ruling 49).
        if let TypeExprKind::Named(head, args) = &kind
            && args
                .first()
                .is_some_and(|arg| self.generic_arg_is_inf_marker(arg))
        {
            match self.interner.resolve(head.name) {
                "modulus" => form = TypeFormLevel::WRAPPING_INF,
                "saturatus" => form = TypeFormLevel::SATURATING_INF,
                _ => {}
            }
        }

        let span = start.merge(self.previous_span());
        let core = TypeExpr {
            nullable: false,
            mode,
            kind,
            span,
            form,
            iuncta_label: None,
        };
        Ok(core)
    }

    /// Parse the kind of one tail-free, mode-stripped type core.
    ///
    /// Authored compression capture: sugar roots and bare width markers carry
    /// SHORT; every other spelling is LONG. Child levels come from each branch's
    /// own parse (width children are always bare widths). The head word's
    /// authored spelling is kept for the retired-wrapper diagnostic.
    fn parse_type_atom_kind(&mut self) -> Result<(TypeExprKind, TypeFormLevel), ParseError> {
        let mut form = TypeFormLevel::LONG;
        let kind = if matches!(self.peek().kind, TokenKind::Underscore(_)) {
            self.advance();
            TypeExprKind::Infer
        } else if self.check(&TokenKind::Cup) {
            let cup_span = self.peek().span;
            self.advance();
            if self.check(&TokenKind::Cup) {
                return Err(
                    ParseError::structured(ParseErrorKind::TypeInvalid, cup_span)
                        .with_arg("issue", "unexpected_cup_after_union_hole"),
                );
            }
            TypeExprKind::UnionHole
        } else {
            let (name, path, name_str, authored_word) = self.parse_type_head()?;
            if let Some(width) = radix_types::numeric_width::lista_sugar_width_marker(&name_str) {
                form = TypeFormLevel::SHORT;
                self.parse_lista_width_sugar(&name, width)?
            } else if let Some(width) =
                radix_types::numeric_width::tensor_sugar_width_marker(&name_str)
            {
                form = TypeFormLevel::SHORT;
                self.parse_tensor_width_sugar(&name, width)?
            } else if let Some(width) =
                radix_types::numeric_width::vector_sugar_width_marker(&name_str)
            {
                form = TypeFormLevel::SHORT;
                self.parse_vector_width_sugar(&name, width)?
            } else if let Some(width) =
                radix_types::numeric_width::matrix_sugar_width_marker(&name_str)
            {
                form = TypeFormLevel::SHORT;
                self.parse_matrix_width_sugar(&name, width)?
            } else if let Some(width) =
                radix_types::numeric_width::sparsa_sugar_width_marker(&name_str)
            {
                form = TypeFormLevel::SHORT;
                self.parse_sparsa_width_sugar(&name, width)?
            } else if path.len() == 1
                && matches!(name_str.as_str(), "numerus" | "fractus")
                && self.check(&TokenKind::LtAscii)
            {
                let (kind, level) = self.parse_retired_numeric_wrapper(name, &authored_word)?;
                form = level;
                kind
            } else if self.eat(&TokenKind::LtAscii) {
                self.parse_angle_type_args(name, path, &name_str)?
            } else if path.len() == 1 {
                if radix_types::numeric_width::is_standalone_width_name(&name_str) {
                    form = TypeFormLevel::SHORT;
                }
                TypeExprKind::Named(name, Vec::new())
            } else {
                TypeExprKind::Qualified(path, Vec::new())
            }
        };
        Ok((kind, form))
    }

    /// Parse a type head word and its dotted path.
    ///
    /// Returns the final path segment as the head [`Ident`], the full path, the
    /// head's resolved spelling, and the head's authored spelling (kept for the
    /// retired-wrapper diagnostic; canonicalization rewrites it to its latin
    /// identity).
    fn parse_type_head(&mut self) -> Result<(Ident, Vec<Ident>, String, String), ParseError> {
        let mut authored_word = String::new();
        let name = if self.check_keyword_ident(TokenKind::Nihil) {
            // The null *type* is `nihil` (D23.2) and reaches the type
            // grammar as an ordinary type word. The null *value* literal
            // (`null` / `nulla`) is the only word claiming the `Nihil`
            // identity, so seeing it where a type is required is an error
            // (D23.1/D23.2).
            let span = self.peek().span;
            let source = self.current_ident_source();
            self.advance();
            let type_surface = self.null_type_surface();
            return Err(ParseError::structured(ParseErrorKind::TypeInvalid, span)
                .with_arg("issue", "null_value_in_type_position")
                .with_arg("value", source)
                .with_arg("type", type_surface));
        } else if self.check_keyword_ident(TokenKind::Modulus) {
            let span = self.peek().span;
            let source = self.current_ident_source();
            self.advance();
            if source == "modulus" {
                self.reject_latin_type_word("modulus", span);
            }
            self.keyword_ident("modulus", span)
        } else if self.check_keyword_ident(TokenKind::Iuncta) {
            let span = self.peek().span;
            let source = self.current_ident_source();
            self.advance();
            if source == "iuncta" {
                self.reject_latin_type_word("iuncta", span);
            }
            self.keyword_ident("iuncta", span)
        } else {
            let ident = self.parse_ident()?;
            self.interner
                .resolve(ident.name)
                .clone_into(&mut authored_word);
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
        Ok((name, path, name_str, authored_word))
    }

    /// Parse the angle-bracketed arguments of an already-consumed type head.
    fn parse_angle_type_args(
        &mut self,
        name: Ident,
        path: Vec<Ident>,
        name_str: &str,
    ) -> Result<TypeExprKind, ParseError> {
        if name_str == "atomic" {
            let elem = Box::new(self.parse_type()?);
            if self.eat(&TokenKind::Comma) {
                return Err(
                    ParseError::structured(ParseErrorKind::Expected, self.current_span())
                        .with_arg("issue", "atomic_single_element"),
                );
            }
            self.expect_issue(&TokenKind::GtAscii, "type_arguments_end")?;
            Ok(TypeExprKind::Atomic(elem))
        } else if name_str == "promissum" {
            let success = Box::new(self.parse_type()?);
            if self.eat(&TokenKind::ExitArrow) {
                let alternate = Box::new(self.parse_type()?);
                self.expect_issue(&TokenKind::GtAscii, "type_arguments_end")?;
                Ok(TypeExprKind::PromissumFailable(success, alternate))
            } else {
                if self.eat(&TokenKind::Comma) {
                    return Err(ParseError::structured(
                        ParseErrorKind::TypeInvalid,
                        self.current_span(),
                    )
                    .with_arg("issue", "promissum_alternate_uses_exit_arrow"));
                }
                self.expect_issue(&TokenKind::GtAscii, "type_arguments_end")?;
                Ok(TypeExprKind::Named(
                    name,
                    vec![GenericArgExpr::Type(*success)],
                ))
            }
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
                return Err(
                    ParseError::structured(ParseErrorKind::Expected, self.current_span())
                        .with_arg("issue", issue),
                );
            }
            let shape = self.parse_figura()?;
            self.expect_issue(&TokenKind::GtAscii, "type_arguments_end")?;
            if name_str == "tensor" {
                Ok(TypeExprKind::Tensor(elem, shape))
            } else if name_str == "sparsa" {
                Ok(TypeExprKind::Sparsa(elem, shape))
            } else if name_str == "matrix" {
                Ok(TypeExprKind::Matrix(elem, shape))
            } else {
                Ok(TypeExprKind::Vector(elem, shape))
            }
        } else {
            self.parse_named_type_params(name, path, name_str)
        }
    }

    /// Parse the generic argument list of a named or qualified type.
    fn parse_named_type_params(
        &mut self,
        name: Ident,
        path: Vec<Ident>,
        name_str: &str,
    ) -> Result<TypeExprKind, ParseError> {
        // `record<*>` (la `ratio<*>`) is the whole-field-list hole. The `*`
        // is legal only as the sole argument, so anything other than `>`
        // after it is a targeted error rather than a generic one.
        if path.len() == 1 && name_str == "ratio" && self.check(&TokenKind::Star) {
            self.advance();
            if !self.check(&TokenKind::GtAscii) {
                return Err(
                    ParseError::structured(ParseErrorKind::Expected, self.current_span())
                        .with_arg("issue", "ratio_field_list_hole_not_sole"),
                );
            }
            self.expect_issue(&TokenKind::GtAscii, "type_arguments_end")?;
            return Ok(TypeExprKind::FieldListHole);
        }
        let mut params = Vec::new();
        let mut labels = HashSet::new();
        loop {
            let param =
                if path.len() == 1 && name_str == "lista" && self.looks_like_sized_range_arg() {
                    // A count range after the `size` marker: `lista<T, magnitudo 1…8>`.
                    self.advance();
                    let mut range = self.parse_range_arg()?;
                    range.sized = true;
                    GenericArgExpr::Range(range)
                } else if path.len() == 1
                    && name_str != "iuncta"
                    && name_str != "ratio"
                    && self.looks_like_range_arg()
                {
                    // A range is its own argument slot (`saturating<u32, 0‥W>`):
                    // it never reads as a type or a size, so it is unambiguous
                    // beside both. Which heads take one is decided at lowering.
                    GenericArgExpr::Range(self.parse_range_arg()?)
                } else if name_str == "iuncta" {
                    GenericArgExpr::Type(self.parse_iuncta_type_argument(&mut labels)?)
                } else if path.len() == 1 && name_str == "ratio" {
                    GenericArgExpr::Type(self.parse_ratio_type_argument(&mut labels)?)
                } else if path.len() == 1 && is_bounded_capacity_head(name_str) {
                    // lista/queue/stack/textus/ascii spell their capacity in
                    // the type surface itself, so their arguments keep
                    // the explicit `Type` shape (NATURAL interns as a
                    // named type) rather than the kinded split.
                    self.parse_explicit_type_argument()?
                } else {
                    self.parse_type_argument()?
                };
            params.push(param);
            if !self.consume_item_separator(&TokenKind::GtAscii, "type_parameters")? {
                break;
            }
        }
        self.expect_issue(&TokenKind::GtAscii, "type_arguments_end")?;
        if name_str == "iuncta" && params.len() > 16 {
            return Err(
                ParseError::structured(ParseErrorKind::TypeInvalid, name.span)
                    .with_arg("issue", "iuncta_arity_exceeded")
                    .with_arg("arity", params.len().to_string()),
            );
        }
        if path.len() == 1 {
            Ok(TypeExprKind::Named(name, params))
        } else {
            Ok(TypeExprKind::Qualified(path, params))
        }
    }

    /// Consume a trailing intersection chain after a core type.
    ///
    /// Mirrors [`Self::parse_union_tail`]: nested intersection members are
    /// flattened so `A ∩ (B ∩ C)` and `A ∩ B ∩ C` share one AST shape.
    fn parse_intersection_tail(
        &mut self,
        first: TypeExpr,
        start: Span,
    ) -> Result<TypeExpr, ParseError> {
        if !self.eat(&TokenKind::Cap) {
            return Ok(first);
        }
        if matches!(first.kind, TypeExprKind::UnionHole) {
            return Err(ParseError::structured(ParseErrorKind::TypeInvalid, start)
                .with_arg("issue", "unexpected_cap_after_union_hole"));
        }
        let mut members = Vec::new();
        flatten_intersection_member(first, &mut members);
        loop {
            let member = self.parse_union_member()?;
            flatten_intersection_member(member, &mut members);
            if !self.eat(&TokenKind::Cap) {
                break;
            }
        }
        let span = start.merge(self.previous_span());
        Ok(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Intersection(members),
            span,
            form: TypeFormLevel::LONG,
            iuncta_label: None,
        })
    }

    /// Whether a generic argument is the bare `inf` width marker.
    fn generic_arg_is_inf_marker(&self, arg: &GenericArgExpr) -> bool {
        let ident = match arg {
            GenericArgExpr::Name(ident) => ident,
            GenericArgExpr::Type(TypeExpr {
                kind: TypeExprKind::Named(ident, nested),
                ..
            }) if nested.is_empty() => ident,
            _ => return false,
        };
        self.interner.resolve(ident.name) == radix_types::NumericWidth::Inf.faber_marker()
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
            return Err(ParseError::structured(ParseErrorKind::TypeInvalid, start)
                .with_arg("issue", "unexpected_cup_after_union_hole"));
        }
        let mut members = Vec::new();
        flatten_union_member(first, &mut members);
        loop {
            let member = self.parse_union_member()?;
            if matches!(member.kind, TypeExprKind::UnionHole) {
                return Err(
                    ParseError::structured(ParseErrorKind::TypeInvalid, member.span)
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
            form: TypeFormLevel::LONG,
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
        self.expect_issue(&TokenKind::LParen, "function_type_start")?;

        let mut params = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.is_at_end() {
            params.push(self.parse_type()?);
            if !self.consume_item_separator(&TokenKind::RParen, "function_type_parameters")? {
                break;
            }
        }

        self.expect_issue(&TokenKind::RParen, "function_type_parameters_end")?;
        self.expect_issue(&TokenKind::Arrow, "function_type_return_arrow")?;

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
        name: &Ident,
        width: &str,
    ) -> Result<TypeExprKind, ParseError> {
        self.reject_type_sugar_angle_bracket("lista")?;
        if self.check(&TokenKind::LBracket) {
            return Err(self.invalid_type_error("lista_sugar_bracket_form"));
        }
        let elem = self.width_element_type_expr(width, name.span);
        Ok(TypeExprKind::Named(
            self.keyword_ident("lista", name.span),
            vec![GenericArgExpr::Type(elem)],
        ))
    }

    fn parse_tensor_width_sugar(
        &mut self,
        name: &Ident,
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
        name: &Ident,
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
        name: &Ident,
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
        name: &Ident,
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

    /// Reject the retired wrapped numeric spelling `numerus<W>` / `fractus<W>`.
    ///
    /// A width marker is its own type (`i32`, `f32`, `d64`, `inf`); the wrapped
    /// form, the marker holes (`numerus<_>`) and the wrong-family forms
    /// (`numerus<f32>`) are all retired. The policy words (`saturatus<W>`,
    /// `modulus<W>`, `exactus<W>`) keep their argument and never reach here.
    ///
    /// WHY: the whole wrapper is consumed and the error is recorded without
    /// failing the parse, so the type recovers as the bare marker (or the bare
    /// `numerus`/`fractus` word for a hole) and later diagnostics stay sensible.
    fn parse_retired_numeric_wrapper(
        &mut self,
        name: Ident,
        authored_word: &str,
    ) -> Result<(TypeExprKind, TypeFormLevel), ParseError> {
        self.expect_issue(&TokenKind::LtAscii, "numeric_wrapper_start")?;
        let mut args = Vec::new();
        loop {
            args.push(self.parse_type_argument()?);
            if !self.consume_item_separator(&TokenKind::GtAscii, "type_parameters")? {
                break;
            }
        }
        self.expect_issue(&TokenKind::GtAscii, "type_arguments_end")?;
        let span = name.span.merge(self.previous_span());

        let rendered: Vec<String> = args
            .iter()
            .map(|arg| self.render_wrapper_arg(arg))
            .collect();
        let written = format!("{authored_word}<{}>", rendered.join(", "));
        let marker = match rendered.as_slice() {
            [only] if radix_types::numeric_width::standalone_width_type(only).is_some() => {
                Some(only.clone())
            }
            _ => None,
        };
        let bare = marker.clone().unwrap_or_else(|| authored_word.to_owned());
        let already_reported = self.errors.iter().any(|err| {
            err.span == span
                && err
                    .args
                    .iter()
                    .any(|a| a.value == "numeric_wrapper_retired")
        });
        if !already_reported {
            self.errors.push(
                ParseError::structured(ParseErrorKind::TypeInvalid, span)
                    .with_arg("issue", "numeric_wrapper_retired")
                    .with_arg("form", written)
                    .with_arg("bare", bare),
            );
        }
        Ok(match marker {
            Some(marker) => (
                TypeExprKind::Named(self.keyword_ident(&marker, name.span), Vec::new()),
                TypeFormLevel::SHORT,
            ),
            None => (TypeExprKind::Named(name, Vec::new()), TypeFormLevel::LONG),
        })
    }

    /// Authored spelling of one wrapped-numeric argument, for the diagnostic.
    fn render_wrapper_arg(&self, arg: &GenericArgExpr) -> String {
        match arg {
            GenericArgExpr::Name(ident) => self.interner.resolve(ident.name).to_owned(),
            GenericArgExpr::Type(TypeExpr {
                kind: TypeExprKind::Named(ident, nested),
                ..
            }) if nested.is_empty() => self.interner.resolve(ident.name).to_owned(),
            GenericArgExpr::Type(TypeExpr {
                kind: TypeExprKind::Infer,
                ..
            }) => "_".to_owned(),
            _ => "…".to_owned(),
        }
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
        ParseError::structured(ParseErrorKind::TypeInvalid, self.current_span())
            .with_arg("issue", issue)
    }

    fn width_element_type_expr(&mut self, width: &str, span: Span) -> TypeExpr {
        TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(self.keyword_ident(width, span), Vec::new()),
            span,
            // The element of any width sugar is authored as a bare width.
            form: TypeFormLevel::SHORT,
            iuncta_label: None,
        }
    }

    /// Parse the type-argument list in an `iuncta` construction.
    ///
    /// Construction type arguments use the same labeled-element production as
    /// declaration type positions. Keeping the label set local to the list
    /// preserves the per-tuple uniqueness rule without changing erased type
    /// identity.
    pub(super) fn finish_iuncta_type_args_list(&mut self) -> Result<Vec<TypeExpr>, ParseError> {
        let mut args = Vec::new();
        let mut labels = HashSet::new();
        loop {
            args.push(self.parse_iuncta_type_argument(&mut labels)?);
            if !self.consume_item_separator(&TokenKind::GtAscii, "type_arguments")? {
                break;
            }
        }
        self.expect_issue(&TokenKind::GtAscii, "type_arguments_end")?;
        Ok(args)
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
            self.expect_issue(&TokenKind::Colon, "tuple_type_label_separator")?;
            Some(label)
        } else {
            None
        };
        let mut ty = self.parse_type()?;
        ty.iuncta_label = iuncta_label;
        Ok(ty)
    }

    /// Parse one `ratio` element, which must have its contextual label.
    ///
    /// The label rides the same per-position syntax carrier as `iuncta`, while
    /// the ratio grammar rejects the unlabeled inverse form at parse time.
    pub(super) fn parse_ratio_type_argument(
        &mut self,
        labels: &mut HashSet<Symbol>,
    ) -> Result<TypeExpr, ParseError> {
        // A `*` here followed a labeled argument (`record<g: f32, *>`); the
        // field-list hole is legal only as the sole argument.
        if self.check(&TokenKind::Star) {
            return Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "ratio_field_list_hole_not_sole"),
            );
        }
        let is_label = matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::Tag)
            && matches!(self.peek_at(1).kind, TokenKind::Colon);
        if !is_label {
            return Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "ratio_type_arg_label_required"),
            );
        }

        let label = self.parse_contextual_ident(KeywordOwner::DeclarationName)?;
        if !labels.insert(label.name) {
            return Err(ParseError::structured(ParseErrorKind::Expected, label.span)
                .with_arg("issue", "duplicate_tuple_element_label")
                .with_arg("label", self.interner.resolve(label.name).to_owned()));
        }
        self.expect_issue(&TokenKind::Colon, "ratio_type_label_separator")?;

        let mut ty = self.parse_type()?;
        ty.iuncta_label = Some(label);
        Ok(ty)
    }

    /// Number of tokens the range bound starting `offset` tokens ahead
    /// occupies, or `None` when no bound starts there.
    ///
    /// A bound is a natural literal, `∞`, either one negated, or a name
    /// (optionally spelled as a `size` lift).
    fn range_bound_len(&self, offset: usize) -> Option<usize> {
        let negative = matches!(self.peek_at(offset).kind, TokenKind::Minus);
        let at = offset + usize::from(negative);
        match &self.peek_at(at).kind {
            TokenKind::Integer(_) => Some(at + 1 - offset),
            TokenKind::Float(value) if value.is_infinite() => Some(at + 1 - offset),
            TokenKind::Ident(_) if !negative => {
                if self.keyword_ident_at(at) == Some(TokenKind::Magnitudo) {
                    matches!(self.peek_at(at + 1).kind, TokenKind::Ident(_))
                        .then_some(at + 2 - offset)
                } else {
                    Some(at + 1 - offset)
                }
            }
            _ => None,
        }
    }

    /// Whether the next tokens spell a range argument, `lo‥hi` or `lo…hi`.
    fn looks_like_range_arg(&self) -> bool {
        self.range_bound_len(0).is_some_and(|len| {
            matches!(
                self.peek_at(len).kind,
                TokenKind::DotDot | TokenKind::Ellipsis
            )
        })
    }

    /// Whether the next tokens spell a count range, `size lo‥hi` or
    /// `size lo…hi`: the `size` marker, then a whole range. The marker covers
    /// the range, so a bound after it is a plain bound (`size 1…8`), never a
    /// lifted name.
    fn looks_like_sized_range_arg(&self) -> bool {
        self.keyword_ident_at(0) == Some(TokenKind::Magnitudo)
            && self.range_bound_len(1).is_some_and(|len| {
                matches!(
                    self.peek_at(1 + len).kind,
                    TokenKind::DotDot | TokenKind::Ellipsis
                )
            })
    }

    /// Parse the range argument of a ranged store policy.
    ///
    /// GRAMMAR:
    ///   range-argument := range-bound ('‥' | '…') range-bound
    ///   range-bound    := [`-`] NATURAL | [`-`] `∞` | [`size`] IDENTIFIER
    ///
    /// Both ends are always written: `0…` alone is refused by name, and `∞` /
    /// `-∞` write "no bound on this side".
    fn parse_range_arg(&mut self) -> Result<RangeArgExpr, ParseError> {
        let start = self.current_span();
        let lo = self.parse_range_type_bound()?;
        let kind = if self.eat(&TokenKind::Ellipsis) {
            RangeKind::Inclusive
        } else {
            self.expect_issue(&TokenKind::DotDot, "range_operator_expected")?;
            RangeKind::Exclusive
        };
        let hi = self.parse_range_type_bound()?;
        Ok(RangeArgExpr {
            lo,
            hi,
            kind,
            sized: false,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_range_type_bound(&mut self) -> Result<RangeBoundExpr, ParseError> {
        let start = self.current_span();
        let negative = self.eat(&TokenKind::Minus);
        match self.peek().kind.clone() {
            TokenKind::Integer(value) => {
                self.advance();
                let value = i128::from(value);
                Ok(RangeBoundExpr::Literal {
                    value: if negative { -value } else { value },
                    span: start.merge(self.previous_span()),
                })
            }
            TokenKind::Float(value) if value.is_infinite() => {
                self.advance();
                Ok(RangeBoundExpr::Unbounded {
                    negative,
                    span: start.merge(self.previous_span()),
                })
            }
            TokenKind::Ident(_) if !negative => {
                let lifted = self.eat_keyword_ident(TokenKind::Magnitudo);
                let name = self.parse_ident()?;
                Ok(RangeBoundExpr::Name {
                    name,
                    lifted,
                    span: start.merge(self.previous_span()),
                })
            }
            _ => Err(
                ParseError::structured(ParseErrorKind::TypeInvalid, self.current_span())
                    .with_arg("issue", "range_bound_required"),
            ),
        }
    }

    /// Parse one applied argument in generic-head position.
    ///
    /// Natural literals and shape tuples are compile-time size expressions.
    /// Explicit `_` remains a type hole here; a checked magnitudo slot
    /// reclassifies it as an index hole later.
    pub(super) fn parse_type_argument(&mut self) -> Result<GenericArgExpr, ParseError> {
        if let TokenKind::Integer(value) = self.peek().kind {
            if matches!(
                self.peek_at(1).kind,
                TokenKind::Plus | TokenKind::Minus | TokenKind::Slash
            ) {
                return Ok(GenericArgExpr::Size(self.parse_figura()?));
            }
            self.advance();
            return Ok(GenericArgExpr::Size(FiguraExpr::Literal(value)));
        }
        if self.check(&TokenKind::LBracket) {
            return Ok(GenericArgExpr::Size(self.parse_figura()?));
        }
        if matches!(self.peek().kind, TokenKind::Ident(_))
            && matches!(
                self.peek_at(1).kind,
                TokenKind::Plus | TokenKind::Minus | TokenKind::Slash
            )
        {
            return Ok(GenericArgExpr::Size(self.parse_figura()?));
        }
        if self.check_keyword_ident(TokenKind::Magnitudo) {
            return Ok(GenericArgExpr::Size(self.parse_figura()?));
        }
        Ok(GenericArgExpr::Type(self.parse_type()?))
    }

    /// Parse one explicit type argument at a call site or bounded-container
    /// application.
    ///
    /// These positions are unambiguously `typeArguments` in the grammar, so a
    /// bare name stays a `Type` and NATURAL interns as a named type — the
    /// kinded reclassification of generic heads must not swallow either.
    pub(super) fn parse_explicit_type_argument(&mut self) -> Result<GenericArgExpr, ParseError> {
        if let TokenKind::Integer(value) = self.peek().kind {
            if matches!(
                self.peek_at(1).kind,
                TokenKind::Plus | TokenKind::Minus | TokenKind::Slash
            ) {
                return Ok(GenericArgExpr::Size(self.parse_figura()?));
            }
            let span = self.current_span();
            self.advance();
            return Ok(GenericArgExpr::Type(TypeExpr {
                nullable: false,
                mode: None,
                kind: TypeExprKind::Named(self.keyword_ident(&value.to_string(), span), Vec::new()),
                span,
                form: TypeFormLevel::LONG,
                iuncta_label: None,
            }));
        }
        if self.check(&TokenKind::LBracket) {
            return Ok(GenericArgExpr::Size(self.parse_figura()?));
        }
        if matches!(self.peek().kind, TokenKind::Ident(_))
            && matches!(
                self.peek_at(1).kind,
                TokenKind::Plus | TokenKind::Minus | TokenKind::Slash
            )
        {
            return Ok(GenericArgExpr::Size(self.parse_figura()?));
        }
        if self.check_keyword_ident(TokenKind::Magnitudo) {
            return Ok(GenericArgExpr::Size(self.parse_figura()?));
        }
        Ok(GenericArgExpr::Type(self.parse_type()?))
    }

    /// Parse a tensor `Figura` in type position.
    ///
    /// GRAMMAR: figura := `_` | natural | ident | figura `/` natural
    ///                       | figura (`+` | `-`) natural
    ///                       | [ figura (, figura)* ]
    pub(super) fn parse_figura(&mut self) -> Result<FiguraExpr, ParseError> {
        if self.eat_keyword_ident(TokenKind::Magnitudo) {
            // Spelled lift: `size <primary>` (la `magnitudo <primary>`) in an
            // extent position; the value is checked at that store.
            let extent = self.parse_figura_lift_extent()?;
            return Ok(FiguraExpr::Lift(Box::new(extent)));
        }

        if matches!(self.peek().kind, TokenKind::Underscore(_)) {
            self.advance();
            return Ok(FiguraExpr::Infer);
        }

        if self.check(&TokenKind::LBracket) {
            return Ok(FiguraExpr::Tuple(self.parse_figura_dimensions()?));
        }

        self.parse_figura_additive()
    }

    /// Parse the bracketed dimension list `[ figura (, figura)* ]` (empty
    /// allowed); the caller has checked the opening `[`.
    fn parse_figura_dimensions(&mut self) -> Result<Vec<FiguraExpr>, ParseError> {
        self.advance();
        let mut dims = Vec::new();
        if !self.check(&TokenKind::RBracket) {
            loop {
                let span = self.current_span();
                dims.push(self.nested(ParseErrorKind::TypeInvalid, span, |parser| {
                    parser.parse_figura()
                })?);
                if !self.consume_item_separator(&TokenKind::RBracket, "figura_dimensions")? {
                    break;
                }
            }
        }
        self.expect_issue(&TokenKind::RBracket, "shape_dimensions_end")?;
        Ok(dims)
    }

    fn parse_figura_additive(&mut self) -> Result<FiguraExpr, ParseError> {
        let mut value = self.parse_figura_product()?;
        loop {
            let add = if self.eat(&TokenKind::Plus) {
                Some(true)
            } else if self.eat(&TokenKind::Minus) {
                Some(false)
            } else {
                None
            };
            let Some(add) = add else {
                break;
            };
            let amount = self.parse_figura_natural_operand()?;
            value = if add {
                FiguraExpr::Add(Box::new(value), amount)
            } else {
                FiguraExpr::Subtract(Box::new(value), amount)
            };
        }
        Ok(value)
    }

    fn parse_figura_product(&mut self) -> Result<FiguraExpr, ParseError> {
        let mut value = self.parse_figura_atom()?;
        while self.eat(&TokenKind::Slash) {
            let divisor = self.parse_figura_natural_operand()?;
            value = FiguraExpr::Divide(Box::new(value), divisor);
        }
        Ok(value)
    }

    /// Parse the primary payload of a spelled lift: an integer literal
    /// (optionally negated), a name, a parenthesized figura term, or a
    /// bracketed shape.
    fn parse_figura_lift_extent(&mut self) -> Result<LiftExtent, ParseError> {
        if self.check(&TokenKind::LBracket) {
            return Ok(LiftExtent::Shape(self.parse_figura_dimensions()?));
        }
        if self.eat(&TokenKind::Minus) {
            let TokenKind::Integer(value) = self.peek().kind else {
                return Err(self.invalid_type_error("figura_arithmetic_requires_natural_constant"));
            };
            let span = self.current_span();
            self.advance();
            return Ok(LiftExtent::Negative { value, span });
        }
        if let TokenKind::Integer(value) = self.peek().kind {
            self.advance();
            return Ok(LiftExtent::Literal(value));
        }
        if self.eat(&TokenKind::LParen) {
            let span = self.current_span();
            let term = self.nested(ParseErrorKind::TypeInvalid, span, |parser| {
                parser.parse_figura()
            })?;
            self.expect_issue(&TokenKind::RParen, "shape_parenthesis_end")?;
            return Ok(LiftExtent::Paren(Box::new(term)));
        }
        Ok(LiftExtent::Name(self.parse_ident()?))
    }

    fn parse_figura_atom(&mut self) -> Result<FiguraExpr, ParseError> {
        if self.eat(&TokenKind::LParen) {
            let span = self.current_span();
            let value = self.nested(ParseErrorKind::TypeInvalid, span, |parser| {
                parser.parse_figura()
            })?;
            self.expect_issue(&TokenKind::RParen, "shape_parenthesis_end")?;
            return Ok(value);
        }

        if let TokenKind::Integer(value) = self.peek().kind {
            self.advance();
            return Ok(FiguraExpr::Literal(value));
        }

        Ok(FiguraExpr::Var(self.parse_ident()?))
    }

    fn parse_figura_natural_operand(&mut self) -> Result<u64, ParseError> {
        if let TokenKind::Integer(value) = self.peek().kind {
            self.advance();
            return Ok(value);
        }
        Err(self.invalid_type_error("figura_arithmetic_requires_natural_constant"))
    }
}

fn flatten_union_member(member: TypeExpr, members: &mut Vec<TypeExpr>) {
    flatten_members(member, members, true);
}

fn flatten_intersection_member(member: TypeExpr, members: &mut Vec<TypeExpr>) {
    flatten_members(member, members, false);
}

/// Flatten one nested union or intersection member in place.
///
/// `is_union` selects which associative kind is spliced; the other kind stays
/// an ordinary member because `∪` and `∩` do not nest through each other.
fn flatten_members(member: TypeExpr, members: &mut Vec<TypeExpr>, is_union: bool) {
    if member.nullable || member.mode.is_some() {
        members.push(member);
        return;
    }

    let TypeExpr {
        nullable,
        mode,
        kind,
        span,
        form,
        iuncta_label,
    } = member;
    let nested = match kind {
        TypeExprKind::Union(nested) if is_union => nested,
        TypeExprKind::Intersection(nested) if !is_union => nested,
        kind => {
            members.push(TypeExpr {
                nullable,
                mode,
                kind,
                span,
                form,
                iuncta_label,
            });
            return;
        }
    };
    for nested_member in nested {
        flatten_members(nested_member, members, is_union);
    }
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;

#[cfg(test)]
#[path = "types_diagnostic_test.rs"]
mod diagnostic_tests;
