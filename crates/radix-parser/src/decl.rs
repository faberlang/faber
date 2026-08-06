//! Declaration grammar for Faber's crate-facing syntax tree.
//!
//! This module owns the parser surface for declarations and declaration-adjacent
//! blocks: bindings, functions, class/interface shapes, algebraic data types,
//! imports, tests, annotations, and ordinary block bodies. It is the
//! main grammar boundary between source-level declarations and `AST` contracts
//! consumed by the collector and package CLI.
//!
//! DESIGN PHILOSOPHY
//! =================
//! Declarations are intentionally explicit: types precede names, inferred locals
//! require `_`, voluntary slots use post-name `sponte`, and canonical modifier
//! ordering is enforced in the parser when later phases would otherwise receive
//! ambiguous ASTs. Where legacy or transitional syntax is still recognized, the
//! parser either normalizes it into the current `AST` contract or emits a targeted
//! diagnostic instead of expanding the language silently.
//!
//! INVARIANTS
//! ==========
//! - `parse_stmt` is the dispatcher for both top-level and nested statements
//!   contexts; recovery boundaries in `mod.rs` must stay aligned with it.
//! - Function, parameter, field, and closure signatures delegate to `types.rs`
//!   for type syntax; this file decides where type positions are legal.
//! - Annotation parsers define the source contract for CLI/package metadata,
//!   while unknown annotations remain statement annotations for later phases.

use super::{ParseError, ParseErrorKind, Parser};
use radix_lexer::{
    keyword_allowed_as_ident, keyword_spelling_for_token, lookup_keyword_spec, KeywordOwner,
    KeywordScope, TokenKind,
};
#[allow(clippy::wildcard_imports)]
// syntax is a pure re-export barrel from radix-syntax
use radix_syntax::*;

// =============================================================================
// STATEMENT DISPATCH
// =============================================================================

impl Parser {
    /// Dispatch a top-level or nested statement by its leading token.
    ///
    /// GRAMMAR:
    ///   statement := declaration | control-flow | transfer | block | expr-stmt
    ///
    /// This function is part of the parser's recovery contract. Any keyword added
    /// here as a statement start should also be considered for
    /// `Parser::is_recovery_boundary` so malformed input can resume at the new
    /// construct instead of skipping it.
    pub(super) fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        let leading_trivia = self.parse_leading_trivia();
        let start = self.current_span();
        let id = self.next_id();

        let kind = match self.peek().kind {
            // Block
            TokenKind::LBrace => StmtKind::Block(self.parse_block()?),

            // Keyword-led statements: dispatch on the registry keyword
            // identity (locale-independent), claimed only in statement-initial
            // position.
            TokenKind::Ident(_) => {
                match self.current_keyword_ident() {
                    // Declaration introducers.
                    Some(TokenKind::Discretio) => self.parse_union_decl()?,
                    Some(TokenKind::Fixum) | Some(TokenKind::Varia) => self.parse_var_decl()?,
                    Some(TokenKind::Functio) => self.parse_func_decl()?,
                    Some(TokenKind::Genus) => self.parse_class_decl()?,
                    Some(TokenKind::Implendum) => self.parse_interface_decl()?,
                    Some(TokenKind::Ordo) => self.parse_enum_decl()?,
                    Some(TokenKind::Sit) => self.parse_sit_decl()?,
                    Some(TokenKind::Typus) => self.parse_type_alias_decl()?,
                    // Statement introducers (previously token-shaped).
                    Some(TokenKind::Abstractus) => self.parse_abstract_class_decl()?,
                    Some(TokenKind::Importa) => self.parse_import_decl()?,
                    Some(TokenKind::Proba) => StmtKind::Proba(self.parse_proba_case()?),
                    Some(TokenKind::Probandum) => self.parse_probandum_decl()?,
                    Some(TokenKind::Custodi) => self.parse_custodi_stmt()?,
                    Some(TokenKind::Itera) => self.parse_itera_stmt()?,
                    Some(TokenKind::Iace) => self.parse_iace_stmt()?,
                    Some(TokenKind::Mori) => self.parse_mori_stmt()?,
                    Some(TokenKind::Cede) => self.parse_cede_stmt()?,
                    Some(TokenKind::Adfirma) => self.parse_adfirma_stmt()?,
                    Some(TokenKind::Scribe) => self.parse_scribe_stmt()?,
                    Some(TokenKind::Incipit) | Some(TokenKind::Incipiet) => {
                        self.parse_incipit_stmt()?
                    }
                    Some(TokenKind::Ex) => self.parse_extract_stmt()?,
                    // Control flow.
                    Some(TokenKind::Discerne) => self.parse_discerne_stmt()?,
                    Some(TokenKind::Dum) => self.parse_dum_stmt()?,
                    Some(TokenKind::Elige) => self.parse_elige_stmt()?,
                    Some(TokenKind::Fac) => self.parse_fac_stmt()?,
                    Some(TokenKind::Si) => self.parse_si_stmt()?,
                    Some(TokenKind::Perge) => self.parse_perge_stmt()?,
                    Some(TokenKind::Redde) => self.parse_redde_stmt()?,
                    Some(TokenKind::Rumpe) => self.parse_rumpe_stmt()?,
                    // Resource/transfer and route statements.
                    Some(TokenKind::Cura) => self.parse_cura_stmt()?,
                    Some(TokenKind::Tacet) => self.parse_tacet_stmt()?,
                    Some(TokenKind::Ad) => self.parse_ad_stmt()?,
                    Some(TokenKind::Figendum) | Some(TokenKind::Variandum) => {
                        let is_immutable =
                            self.current_keyword_ident() == Some(TokenKind::Figendum);
                        self.parse_await_var_decl(if is_immutable {
                            AstMutability::Immutable
                        } else {
                            AstMutability::Mutable
                        })?
                    }
                    Some(TokenKind::Reddet) => self.parse_reddet_stmt(start)?,
                    Some(TokenKind::Tacebit) => self.parse_tacebit_stmt(start)?,
                    // Output scribe builtins.
                    Some(TokenKind::Nota) | Some(TokenKind::Mone) | Some(TokenKind::Vide) => {
                        self.parse_scribe_stmt()?
                    }
                    _ => self.parse_expr_stmt()?,
                }
            }

            // Annotations (for following statement)
            TokenKind::At => {
                let annotations = self.parse_annotations()?;
                let mut stmt = self.parse_stmt()?;
                let mut combined = leading_trivia;
                combined.append(&mut stmt.leading_trivia);
                stmt.leading_trivia = combined;
                stmt.annotations = annotations;
                return Ok(stmt);
            }

            // Expression statement
            _ => self.parse_expr_stmt()?,
        };

        let span = start.merge(self.previous_span());
        Ok(Stmt {
            id,
            kind,
            span,
            annotations: Vec::new(),
            leading_trivia,
        })
    }

    // =============================================================================
    // VARIABLE DECLARATIONS
    // =============================================================================

    /// Parse `fixum` and `varia` binding declarations.
    ///
    /// GRAMMAR:
    ///   var-decl := (`fixum` | `varia`) type pattern ['←' expr]
    ///
    /// Simple bindings carry an explicit type slot, including `_` for inference.
    /// Destructuring keeps its historical untyped shape because the binding
    /// pattern itself owns the names and any rest element.
    fn parse_var_decl(&mut self) -> Result<StmtKind, ParseError> {
        let (mutability, is_await) = if self.eat_keyword_ident(TokenKind::Fixum) {
            (AstMutability::Immutable, false)
        } else if self.eat_keyword_ident(TokenKind::Varia) {
            (AstMutability::Mutable, false)
        } else {
            return Err(Self::expected_error("fixum or varia", self.current_span()));
        };

        // Destructuring keeps the historical untyped pattern form; simple bindings
        // use an explicit type slot, with `_` meaning "infer from the initializer".
        let ty = if self.check(&TokenKind::LBracket) || self.check(&TokenKind::LBrace) {
            None
        } else {
            Some(self.parse_type()?)
        };

        // Binding pattern
        let binding = self.parse_binding_pattern()?;

        // Optional initializer
        let init = if self.eat(&TokenKind::Assign) {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        Ok(StmtKind::Var(VarDecl {
            mutability,
            is_await,
            ty,
            binding,
            init,
        }))
    }

    /// Parse `figendum` / `variandum` awaited binding declarations.
    ///
    /// GRAMMAR:
    ///   await-var-decl := (`figendum` | `variandum`) type pattern '←' expr
    ///
    /// These spellings await a `promissum<T ⇥ E>`, bind the resolved success `T`,
    /// and propagate a compatible alternate `E`. They reuse the ordinary
    /// `VarDecl` shape with `is_await = true` and the specified mutability.
    fn parse_await_var_decl(&mut self, mutability: AstMutability) -> Result<StmtKind, ParseError> {
        self.advance(); // consume the figendum/variandum identifier

        let ty = if self.check(&TokenKind::LBracket) || self.check(&TokenKind::LBrace) {
            None
        } else {
            Some(self.parse_type()?)
        };

        let binding = self.parse_binding_pattern()?;

        self.expect(&TokenKind::Assign, "expected '←' after awaited binding")?;
        let init = Some(Box::new(self.parse_expression()?));

        Ok(StmtKind::Var(VarDecl {
            mutability,
            is_await: true,
            ty,
            binding,
            init,
        }))
    }

    /// Parse `reddet future` — await-return.
    ///
    /// GRAMMAR:
    ///   reddet-stmt := `reddet` expr
    ///
    /// Awaits a `promissum<T>` or `promissum<T ⇥ E>`, returns the resolved `T`
    /// through the normal `→ T` channel, and adopts a compatible `E`. Valid only
    /// inside a `fiet` body.
    fn parse_reddet_stmt(&mut self, start: radix_lexer::Span) -> Result<StmtKind, ParseError> {
        self.advance(); // consume `reddet`
        let future = Box::new(self.parse_expression()?);
        let span = start.merge(self.previous_span());
        Ok(StmtKind::Reddet(ReddetStmt { future, span }))
    }

    /// Parse `tacebit future` — await-discard.
    ///
    /// GRAMMAR:
    ///   tacebit-stmt := `tacebit` expr
    ///
    /// Awaits a `promissum<T>` or `promissum<T ⇥ E>` to completion for any success
    /// type `T`, discards the successful `T`, and propagates a compatible `E`.
    fn parse_tacebit_stmt(&mut self, start: radix_lexer::Span) -> Result<StmtKind, ParseError> {
        self.advance(); // consume `tacebit`
        let future = Box::new(self.parse_expression()?);
        let span = start.merge(self.previous_span());
        Ok(StmtKind::Tacebit(TacebitStmt { future, span }))
    }

    /// Parse `sit` inferred immutable binding declarations.
    /// kind.
    fn parse_sit_decl(&mut self) -> Result<StmtKind, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Sit, "expected 'sit'")?;

        if self.check(&TokenKind::LBracket) || self.check(&TokenKind::LBrace) {
            return Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "sit_no_destructuring")
                    .with_arg("keyword", "sit"),
            );
        }

        if matches!(self.peek().kind, TokenKind::Underscore(_)) {
            return Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "sit_identifier_binding")
                    .with_arg("keyword", "sit"),
            );
        }

        let ident = self.parse_ident()?;
        let binding = BindingPattern::Ident(ident);

        let init = if self.eat(&TokenKind::Assign) {
            Some(Box::new(self.parse_expression()?))
        } else {
            // WHY: `sit x` is the inferred deferred immutable (≡ `fixum _ x`).
            // We do not peek for a trailing identifier here: newlines are not
            // token-level separators, so the next statement's leading identifier
            // would be misread as an explicit-type attempt. Stray tokens surface
            // as natural parse errors from the block parser.
            None
        };

        let ty = Some(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Infer,
            span: start,
        });

        Ok(StmtKind::Var(VarDecl {
            mutability: AstMutability::Immutable,
            is_await: false,
            ty,
            binding,
            init,
        }))
    }

    fn parse_binding_pattern(&mut self) -> Result<BindingPattern, ParseError> {
        if let TokenKind::LBracket = self.peek().kind {
            return self.parse_array_binding_pattern();
        }

        if let TokenKind::LBrace = self.peek().kind {
            return self.parse_object_binding_pattern();
        }

        if let TokenKind::Underscore(_) = self.peek().kind {
            let span = self.peek().span;
            self.advance();
            return Ok(BindingPattern::Wildcard(span));
        }

        let ident = self.parse_ident()?;
        Ok(BindingPattern::Ident(ident))
    }

    fn parse_array_binding_pattern(&mut self) -> Result<BindingPattern, ParseError> {
        let start = self.current_span();
        self.expect(&TokenKind::LBracket, "expected '['")?;

        let mut elements = Vec::new();
        let mut rest = None;

        while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
            if self.eat_keyword_ident(TokenKind::Ceteri) {
                if rest.is_some() {
                    return Err(ParseError::structured(
                        ParseErrorKind::Expected,
                        self.current_span(),
                    )
                    .with_arg("issue", "duplicate_rest_pattern")
                    .with_arg("keyword", "ceteri"));
                }
                let name = self.parse_ident()?;
                rest = Some(name);
                if self.eat(&TokenKind::Comma) && self.check_keyword_ident(TokenKind::Ceteri) {
                    return Err(ParseError::structured(
                        ParseErrorKind::Expected,
                        self.current_span(),
                    )
                    .with_arg("issue", "duplicate_rest_pattern")
                    .with_arg("keyword", "ceteri"));
                }
                break;
            }

            let element = if let TokenKind::Underscore(_) = self.peek().kind {
                let span = self.peek().span;
                self.advance();
                BindingPattern::Wildcard(span)
            } else if let TokenKind::LBracket = self.peek().kind {
                self.parse_array_binding_pattern()?
            } else {
                BindingPattern::Ident(self.parse_ident()?)
            };

            elements.push(element);

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        self.expect(&TokenKind::RBracket, "expected ']' after pattern")?;
        let span = start.merge(self.previous_span());
        Ok(BindingPattern::Array {
            elements,
            rest,
            span,
        })
    }

    fn parse_object_binding_pattern(&mut self) -> Result<BindingPattern, ParseError> {
        let start = self.current_span();
        self.expect(&TokenKind::LBrace, "expected '{'")?;

        let mut fields = Vec::new();
        let mut rest = None;

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.eat_keyword_ident(TokenKind::Ceteri) {
                if rest.is_some() {
                    return Err(ParseError::structured(
                        ParseErrorKind::Expected,
                        self.current_span(),
                    )
                    .with_arg("issue", "duplicate_rest_pattern")
                    .with_arg("keyword", "ceteri"));
                }
                rest = Some(self.parse_ident()?);
                if self.eat(&TokenKind::Comma) && self.check_keyword_ident(TokenKind::Ceteri) {
                    return Err(ParseError::structured(
                        ParseErrorKind::Expected,
                        self.current_span(),
                    )
                    .with_arg("issue", "duplicate_rest_pattern")
                    .with_arg("keyword", "ceteri"));
                }
                break;
            }

            let name = self.parse_ident()?;
            let alias = if self.eat_keyword_ident(TokenKind::Ut) {
                Some(self.parse_ident()?)
            } else {
                None
            };
            fields.push(ExField { name, alias });

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        self.expect(&TokenKind::RBrace, "expected '}' after pattern")?;
        let span = start.merge(self.previous_span());
        Ok(BindingPattern::Object { fields, rest, span })
    }

    // =============================================================================
    // FUNCTION DECLARATIONS
    // =============================================================================

    /// Parse function declaration.
    ///
    /// GRAMMAR:
    ///   func-decl := `functio` ident `(` params `)` modifiers ['→' type] block
    ///   params    := type-param* regular-param*
    ///   type-param := `prae` `typus` ident
    ///
    /// WHY: Functions can have type parameters (for generics), ownership mode
    /// parameters (de/in/ex), and trailing modifiers (curata, errata, etc.).
    fn parse_func_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Functio, "expected 'functio'")?;

        let name = self.parse_function_name()?;
        let type_params = self.parse_generic_params()?;

        self.expect(&TokenKind::LParen, "expected '(' after function name")?;
        let params = self.parse_param_list()?;
        self.expect(&TokenKind::RParen, "expected ')' after parameters")?;

        let modifiers = self.parse_func_modifiers()?;

        let posture = self.parse_callable_posture();

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

        let body = if self.check(&TokenKind::LBrace) {
            Some(self.parse_block()?)
        } else if self.options.allow_bodyless_functions {
            None
        } else {
            return Err(ParseError::structured(
                ParseErrorKind::MissingFunctionBody,
                self.current_span(),
            )
            .with_arg("functio", "functio"));
        };

        Ok(StmtKind::Func(FuncDecl {
            name,
            type_params,
            params,
            modifiers,
            posture,
            ret,
            err,
            body,
            annotations: Vec::new(),
            leading_trivia: Vec::new(),
        }))
    }

    /// Parse the optional callable-posture word after modifiers.
    ///
    /// Bare signatures remain synchronous finite. The morphology words are
    /// contextual identifiers, so they are consumed only in this signature slot.
    fn parse_callable_posture(&mut self) -> CallablePosture {
        if self.eat_identifier_text("fiet") {
            CallablePosture::AsyncFinite
        } else if self.eat_identifier_text("fiunt") {
            CallablePosture::SyncStream
        } else if self.eat_identifier_text("fient") {
            CallablePosture::AsyncStream
        } else {
            CallablePosture::SyncFinite
        }
    }

    /// Parse value parameters inside `(...)`.
    ///
    /// GRAMMAR:
    ///   param-list := regular-param* (',' regular-param)*
    ///   regular-param := [`de`|`in`|`ex`] [`ceteri`] type ident [`sponte`] [`ut` ident] [`vel` expr]
    ///
    /// Generic parameters live in the angle-bracket list before `(`.
    fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        if self.check_keyword_ident(TokenKind::Prae) {
            return Err(self.removed_prae_typus_error());
        }

        let mut params = Vec::new();

        while !self.check(&TokenKind::RParen) && !self.is_at_end() {
            let start = self.current_span();

            // Mode: de/in/ex (prefix on type)
            let mode = if self.eat_keyword_ident(TokenKind::De) {
                AstParamMode::De
            } else if self.eat_keyword_ident(TokenKind::In) {
                AstParamMode::In
            } else if self.eat_keyword_ident(TokenKind::Ex) {
                AstParamMode::Ex
            } else {
                AstParamMode::Owned
            };

            // Rest: ceteri
            let rest = self.eat_keyword_ident(TokenKind::Ceteri);

            // Type
            let ty = self.parse_type()?;

            // Name
            let name = self.parse_param_name()?;

            let sponte = self.parse_post_name_marker()?;

            // Alias: ut NAME
            let alias = if self.eat_keyword_ident(TokenKind::Ut) {
                Some(self.parse_param_name()?)
            } else {
                None
            };

            // Default: vel EXPR
            let default = if self.eat_keyword_ident(TokenKind::Vel) {
                Some(Box::new(self.parse_expression()?))
            } else {
                None
            };

            let span = start.merge(self.previous_span());
            params.push(Param {
                sponte,
                mode,
                rest,
                ty,
                name,
                alias,
                default,
                span,
            });

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        Ok(params)
    }

    /// Parse trailing function modifiers that affect call/runtime contracts.
    fn parse_func_modifiers(&mut self) -> Result<Vec<FuncModifier>, ParseError> {
        let mut modifiers = Vec::new();

        loop {
            if self.eat_keyword_ident(TokenKind::Argumenta) {
                let name = self.parse_ident()?;
                modifiers.push(FuncModifier::Argumenta(name));
            } else if self.eat_keyword_ident(TokenKind::Curata) {
                let required = self.parse_ident()?;
                let alias = if self.eat_keyword_ident(TokenKind::Ut) {
                    Some(self.parse_ident()?)
                } else {
                    None
                };
                modifiers.push(FuncModifier::Curata { required, alias });
            } else if self.eat_keyword_ident(TokenKind::Errata) {
                let name = self.parse_ident()?;
                modifiers.push(FuncModifier::Errata(name));
            } else if self.eat_keyword_ident(TokenKind::Exitus) {
                let value = if let TokenKind::Integer(n) = self.peek().kind {
                    self.advance();
                    match i64::try_from(n) {
                        Ok(n) => ExitusValue::Number(n),
                        Err(_) => {
                            return Err(ParseError::structured(
                                ParseErrorKind::Unexpected,
                                self.previous_span(),
                            )
                            .with_arg("issue", "exitus_value_out_of_range"))
                        }
                    }
                } else {
                    ExitusValue::Name(self.parse_ident()?)
                };
                modifiers.push(FuncModifier::Exitus(value));
            } else if self.eat_keyword_ident(TokenKind::Immutata) {
                modifiers.push(FuncModifier::Immutata);
            } else if self.eat_keyword_ident(TokenKind::Iacit) {
                modifiers.push(FuncModifier::Iacit);
            } else if self.eat_keyword_ident(TokenKind::Optiones) {
                let name = self.parse_ident()?;
                modifiers.push(FuncModifier::Optiones(name));
            } else {
                break;
            }
        }

        Ok(modifiers)
    }

    /// Parse the optional post-name `sponte` marker shared by parameters and fields.
    fn parse_post_name_marker(&mut self) -> Result<bool, ParseError> {
        if self.eat_identifier_text("fixus") {
            return Err(self.removed_fixus_error());
        }
        Ok(self.eat_keyword_ident(TokenKind::Sponte))
    }

    // =============================================================================
    // CLASS DECLARATIONS
    // =============================================================================

    /// Parse a concrete class declaration.
    ///
    /// GRAMMAR:
    ///   class-decl := genus ident [type-params] [sub ident] [implet ident-list] { member* }
    ///
    /// Class headers are intentionally narrow: one optional `sub` parent and a
    /// comma-separated `implet` list. Field and method parsing stays inside this
    /// module because member syntax shares declaration markers with top-level
    /// bindings and functions.
    fn parse_class_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.parse_class_decl_inner(false)
    }

    /// Parse `abstractus genus ...` without introducing a second class grammar.
    fn parse_abstract_class_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Abstractus, "expected 'abstractus'")?;
        self.parse_class_decl_inner(true)
    }

    fn parse_class_decl_inner(&mut self, is_abstract: bool) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Genus, "expected 'genus'")?;

        let name = self.parse_class_name()?;
        let type_params = self.parse_generic_params()?;

        // Extends
        let extends = if self.eat_keyword_ident(TokenKind::Sub) {
            Some(self.parse_decl_name()?)
        } else {
            None
        };

        // Implements
        let mut implements = Vec::new();
        if self.eat_keyword_ident(TokenKind::Implet) {
            loop {
                implements.push(self.parse_decl_name()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }

        if !self.check(&TokenKind::LBrace) {
            return Err(ParseError::structured(
                ParseErrorKind::MissingClassBody,
                self.current_span(),
            )
            .with_arg("genus", "genus"));
        }
        self.advance();

        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let member = self.parse_class_member()?;
            members.push(member);
        }

        self.expect(&TokenKind::RBrace, "expected '}' after class body")?;

        Ok(StmtKind::Class(ClassDecl {
            is_abstract,
            name,
            type_params,
            extends,
            implements,
            members,
        }))
    }

    /// Parse a class member, preserving annotations at the member boundary.
    fn parse_class_member(&mut self) -> Result<ClassMember, ParseError> {
        let leading_trivia = self.parse_leading_trivia();
        let start = self.current_span();

        let annotations = self.parse_annotations()?;

        let kind = if self.check_keyword_ident(TokenKind::Functio) {
            // Method
            let StmtKind::Func(func) = self.parse_func_decl()? else {
                return Err(Self::expected_error("functio", self.current_span()));
            };
            ClassMemberKind::Method(func)
        } else {
            // Field
            let is_static = self.eat_keyword_ident(TokenKind::Generis);
            let is_bound = self.eat_keyword_ident(TokenKind::Nexum);

            let ty = self.parse_type()?;
            let name = self.parse_param_name()?;

            let sponte = self.parse_post_name_marker()?;

            let init = if self.eat(&TokenKind::Eq) {
                Some(Box::new(self.parse_expression()?))
            } else {
                None
            };

            ClassMemberKind::Field(FieldDecl {
                is_static,
                is_bound,
                sponte,
                ty,
                name,
                init,
            })
        };

        let span = start.merge(self.previous_span());
        Ok(ClassMember {
            leading_trivia,
            annotations,
            kind,
            span,
        })
    }

    // =============================================================================
    // INTERFACE DECLARATIONS
    // =============================================================================

    /// Parse an interface declaration and its method signatures.
    ///
    /// GRAMMAR:
    ///   interface-decl := implendum ident [type-params] { method-sig* }
    ///   method-sig := annotation* functio ident ( params ) modifiers [posture] [arrow type]
    ///
    /// Interface methods deliberately reuse function parameter and modifier
    /// parsing, but they stop before bodies so the `AST` carries a contract rather
    /// than an implementation placeholder.
    fn parse_interface_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Implendum, "expected 'implendum'")?;

        let name = self.parse_decl_name()?;
        let type_params = self.parse_generic_params()?;

        self.expect(&TokenKind::LBrace, "expected '{'")?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let leading_trivia = self.parse_leading_trivia();
            let annotations = self.parse_annotations()?;
            let start = self.current_span();

            self.expect_keyword_ident(TokenKind::Functio, "expected 'functio'")?;
            let method_name = self.parse_member_ident()?;

            self.expect(&TokenKind::LParen, "expected '('")?;
            let params = self.parse_param_list()?;
            self.expect(&TokenKind::RParen, "expected ')'")?;

            let modifiers = self.parse_func_modifiers()?;
            let posture = self.parse_callable_posture();

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

            let span = start.merge(self.previous_span());
            methods.push(InterfaceMethod {
                leading_trivia,
                annotations,
                name: method_name,
                params,
                modifiers,
                posture,
                ret,
                err,
                span,
            });
        }

        self.expect(&TokenKind::RBrace, "expected '}'")?;

        Ok(StmtKind::Interface(InterfaceDecl {
            name,
            type_params,
            methods,
        }))
    }

    /// Parse a type alias declaration.
    fn parse_type_alias_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Typus, "expected 'typus'")?;
        let name = self.parse_decl_name()?;
        let type_params = self.parse_generic_params()?;
        self.expect(&TokenKind::Eq, "expected '='")?;
        let ty = self.parse_type()?;

        Ok(StmtKind::TypeAlias(TypeAliasDecl {
            name,
            type_params,
            ty,
        }))
    }

    // =============================================================================
    // ENUM AND UNION DECLARATIONS
    // =============================================================================

    /// Parse enum declaration.
    ///
    /// GRAMMAR:
    ///   enum-decl := `ordo` ident '{' enum-member* '}'
    ///   enum-member := ident ['=' (integer | string)]
    ///
    /// WHY: Enums can have explicit integer or string values for FFI compatibility.
    fn parse_enum_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Ordo, "expected 'ordo'")?;
        let name = self.parse_decl_name()?;

        self.expect(&TokenKind::LBrace, "expected '{'")?;

        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let start = self.current_span();
            let member_name = self.parse_ident()?;

            let value = if self.eat(&TokenKind::Eq) {
                match self.peek().kind {
                    TokenKind::Integer(n) => {
                        self.advance();
                        // Enum payloads keep a checked signed contract: the u64
                        // magnitude must fit the i64 domain explicitly.
                        match i64::try_from(n) {
                            Ok(n) => Some(EnumValue::Integer(n)),
                            Err(_) => {
                                return Err(ParseError::structured(
                                    ParseErrorKind::Unexpected,
                                    self.previous_span(),
                                )
                                .with_arg("issue", "enum_value_out_of_range"))
                            }
                        }
                    }
                    TokenKind::Minus => {
                        self.advance();
                        if let TokenKind::Integer(n) = self.peek().kind {
                            self.advance();
                            // The i64::MIN magnitude (2^63) is the only value
                            // whose negation cannot round-trip through i64.
                            let negated = if n == (i64::MAX as u64) + 1 {
                                Some(i64::MIN)
                            } else {
                                i64::try_from(n).ok().map(|n| -n)
                            };
                            match negated {
                                Some(n) => Some(EnumValue::Integer(n)),
                                None => {
                                    return Err(ParseError::structured(
                                        ParseErrorKind::Unexpected,
                                        self.previous_span(),
                                    )
                                    .with_arg("issue", "enum_value_out_of_range"))
                                }
                            }
                        } else {
                            return Err(ParseError::structured(
                                ParseErrorKind::Expected,
                                self.current_span(),
                            )
                            .with_arg("expected", "number"));
                        }
                    }
                    TokenKind::String(s) => {
                        self.advance();
                        Some(EnumValue::String(s))
                    }
                    _ => {
                        return Err(ParseError::structured(
                            ParseErrorKind::Expected,
                            self.current_span(),
                        )
                        .with_arg("expected", "value"));
                    }
                }
            } else {
                None
            };

            let span = start.merge(self.previous_span());
            members.push(EnumMember {
                name: member_name,
                value,
                span,
            });

            // Optional comma
            self.eat(&TokenKind::Comma);
        }

        self.expect(&TokenKind::RBrace, "expected '}'")?;

        Ok(StmtKind::Enum(EnumDecl { name, members }))
    }

    /// Parse tagged union declaration.
    ///
    /// GRAMMAR:
    ///   union-decl := discretio ident [type-params] { variant* }
    ///   variant := ident [ { field* } ]
    ///   field := type ident
    ///
    /// WHY: Tagged unions (discriminated unions) enable sum types with optional
    /// variant-specific payload fields.
    fn parse_union_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Discretio, "expected 'discretio'")?;
        let name = self.parse_decl_name()?;
        let type_params = self.parse_generic_params()?;

        self.expect(&TokenKind::LBrace, "expected '{'")?;

        let mut variants = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let start = self.current_span();
            let variant_name = self.parse_ident()?;

            let fields = if self.eat(&TokenKind::LBrace) {
                let mut fields = Vec::new();
                while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                    let field_start = self.current_span();
                    let ty = self.parse_type()?;
                    let field_name = self.parse_ident()?;
                    let field_span = field_start.merge(self.previous_span());
                    fields.push(VariantField {
                        ty,
                        name: field_name,
                        span: field_span,
                    });
                    self.eat(&TokenKind::Comma);
                }
                self.expect(&TokenKind::RBrace, "expected '}'")?;
                fields
            } else {
                Vec::new()
            };

            let span = start.merge(self.previous_span());
            variants.push(Variant {
                name: variant_name,
                fields,
                span,
            });

            self.eat(&TokenKind::Comma);
        }

        self.expect(&TokenKind::RBrace, "expected '}'")?;

        Ok(StmtKind::Union(UnionDecl {
            name,
            type_params,
            variants,
        }))
    }

    // =============================================================================
    // IMPORT AND DIRECTIVE DECLARATIONS
    // =============================================================================

    /// Parse a source import declaration.
    ///
    /// GRAMMAR:
    ///   import-decl := `importa` `ex` string [`privata`|`publica`] import-kind
    ///   import-kind := ident [`ut` ident] | '*' `ut` ident
    ///
    /// The parser only records the import specifier, visibility, and binding
    /// shape. Package and library resolution decide later whether the string
    /// names a local module, stdlib interface, or invalid provider path.
    fn parse_import_decl(&mut self) -> Result<StmtKind, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Importa, "expected 'importa'")?;

        let (path, visibility, kind) = if self.check(&TokenKind::LBrace) {
            self.parse_import_record()?
        } else {
            self.parse_import_sugar()?
        };

        let span = start.merge(self.previous_span());
        Ok(StmtKind::Import(ImportDecl {
            path,
            visibility,
            kind,
            span,
        }))
    }

    fn parse_import_sugar(
        &mut self,
    ) -> Result<(radix_lexer::Symbol, Visibility, ImportKind), ParseError> {
        self.expect_keyword_ident(TokenKind::Ex, "expected 'ex'")?;

        let path = self.parse_string()?;
        let path_span = self.previous_span();
        if self.interner.resolve(path).is_empty() {
            return Err(self.invalid_import_error_at("import_name_inference_failed", path_span));
        }

        let explicit_visibility =
            self.check_keyword_ident(TokenKind::Privata) || self.check_keyword_ident(TokenKind::Publica);
        let visibility = if self.eat_keyword_ident(TokenKind::Privata) {
            Visibility::Privata
        } else if self.eat_keyword_ident(TokenKind::Publica) {
            Visibility::Publica
        } else {
            Visibility::Privata
        };

        let kind = if self.eat(&TokenKind::Star) {
            self.expect_keyword_ident(TokenKind::Ut, "expected 'ut'")?;
            let alias = self.parse_ident()?;
            ImportKind::Wildcard { alias }
        } else if explicit_visibility && self.is_import_named_binding_start() {
            let name = self.parse_decl_name()?;
            let alias = if self.eat_keyword_ident(TokenKind::Ut) {
                Some(self.parse_ident()?)
            } else {
                None
            };
            ImportKind::Named {
                name,
                alias,
                inferred: false,
            }
        } else {
            let name = self.infer_import_name(path, path_span)?;
            ImportKind::Named {
                name,
                alias: None,
                inferred: true,
            }
        };

        Ok((path, visibility, kind))
    }

    fn parse_import_record(
        &mut self,
    ) -> Result<(radix_lexer::Symbol, Visibility, ImportKind), ParseError> {
        self.expect(&TokenKind::LBrace, "expected '{' after importa")?;

        let mut path = None;
        let mut path_span = None;
        let mut visibility = Visibility::Privata;
        let mut name = None;
        let mut alias = None;
        let mut wildcard_alias = None;

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let field = self.parse_import_field_name()?;
            let field_name = self.interner.resolve(field.name).to_owned();
            self.expect(&TokenKind::Eq, "expected '=' in import field")?;

            match field_name.as_str() {
                "ex" => {
                    path = Some(self.parse_string()?);
                    path_span = Some(self.previous_span());
                }
                "visibilitas" => {
                    visibility = self.parse_import_visibility_value()?;
                }
                "nomen" => {
                    name = Some(self.parse_decl_name()?);
                }
                "ut" => {
                    alias = Some(self.parse_ident()?);
                }
                "omnia" => {
                    wildcard_alias = Some(self.parse_ident()?);
                }
                _ => {
                    return Err(self.invalid_import_error("unknown_import_field"));
                }
            }

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        self.expect(&TokenKind::RBrace, "expected '}' after import fields")?;

        let Some(path) = path else {
            return Err(self.invalid_import_error("missing_import_source"));
        };
        let path_span = path_span.unwrap_or_else(|| self.previous_span());

        let kind = if let Some(wildcard_alias) = wildcard_alias {
            if name.is_some() || alias.is_some() {
                return Err(self.invalid_import_error("mixed_wildcard_and_named_import"));
            }
            ImportKind::Wildcard {
                alias: wildcard_alias,
            }
        } else {
            let inferred = name.is_none();
            let name = match name {
                Some(name) => name,
                None => self.infer_import_name(path, path_span)?,
            };
            ImportKind::Named {
                name,
                alias,
                inferred,
            }
        };

        Ok((path, visibility, kind))
    }

    fn parse_import_field_name(&mut self) -> Result<Ident, ParseError> {
        let token = self.advance();
        let span = token.span;
        match token.kind {
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => Ok(Ident { name: sym, span }),
            TokenKind::Ex => Ok(self.keyword_ident("ex", span)),
            TokenKind::Ut => Ok(self.keyword_ident("ut", span)),
            _ => Err(ParseError::structured(ParseErrorKind::Expected, span)
                .with_arg("expected", "identifier")
                .with_arg("issue", "import_field_name")),
        }
    }

    fn parse_import_visibility_value(&mut self) -> Result<Visibility, ParseError> {
        if self.eat_keyword_ident(TokenKind::Privata) {
            Ok(Visibility::Privata)
        } else if self.eat_keyword_ident(TokenKind::Publica) {
            Ok(Visibility::Publica)
        } else {
            Err(self.invalid_import_error("invalid_import_visibility"))
        }
    }

    fn is_import_named_binding_start(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Ident(_) | TokenKind::Underscore(_) | TokenKind::Tag => true,
            kind => keyword_allowed_as_ident(KeywordOwner::DeclarationName, kind).is_some(),
        }
    }

    fn infer_import_name(
        &mut self,
        path: radix_lexer::Symbol,
        span: radix_lexer::Span,
    ) -> Result<Ident, ParseError> {
        let path_text = self.interner.resolve(path).to_owned();
        let segment = path_text
            .rsplit('/')
            .next()
            .and_then(|segment| segment.rsplit(':').next())
            .unwrap_or(&path_text)
            .trim_end_matches(".fab")
            .to_owned();

        if !Self::valid_inferred_import_name(&segment)
            || self.inferred_import_name_is_reserved(&segment)
        {
            return Err(self
                .invalid_import_error_at("import_name_inference_failed", span)
                .with_arg("path", path_text.clone()));
        }

        Ok(Ident {
            name: self.interner.intern(&segment),
            span,
        })
    }

    fn valid_inferred_import_name(name: &str) -> bool {
        let mut chars = name.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        (first == '_' || first.is_alphabetic()) && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
    }

    fn inferred_import_name_is_reserved(&self, name: &str) -> bool {
        lookup_keyword_spec(name)
            .and_then(|spec| spec.token_kind.as_ref())
            .is_some_and(|kind| {
                keyword_allowed_as_ident(KeywordOwner::DeclarationName, kind).is_none()
            })
    }

    fn invalid_import_error(&self, issue: &'static str) -> ParseError {
        self.invalid_import_error_at(issue, self.current_span())
    }

    fn invalid_import_error_at(&self, issue: &'static str, span: radix_lexer::Span) -> ParseError {
        ParseError::structured(ParseErrorKind::InvalidImport, span).with_arg("issue", issue)
    }

    // =============================================================================
    // TEST DECLARATIONS
    // =============================================================================

    /// Parse test suite declaration.
    ///
    /// GRAMMAR:
    ///   probandum := `probandum` string proba-modifier* '{' probandum-body '}'
    ///   probandum-body := (setup | proba | nested-probandum)*
    ///
    /// WHY: Test suites support setup/teardown hooks (praepara/postpara), individual
    /// test cases (proba), and nesting for organization.
    fn parse_probandum_decl(&mut self) -> Result<StmtKind, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Probandum, "expected 'probandum'")?;

        let name = self.parse_string()?;
        let modifiers = self.parse_test_modifiers()?;

        self.expect(&TokenKind::LBrace, "expected '{'")?;

        let body = self.parse_probandum_body()?;

        self.expect(&TokenKind::RBrace, "expected '}'")?;

        let span = start.merge(self.previous_span());
        Ok(StmtKind::Probandum(ProbandumDecl {
            name,
            modifiers,
            body,
            span,
        }))
    }

    fn parse_probandum_body(&mut self) -> Result<ProbandumBody, ParseError> {
        let mut setup = Vec::new();
        let mut tests = Vec::new();
        let mut nested = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.check_keyword_ident(TokenKind::Praepara)
                || self.check_keyword_ident(TokenKind::Praeparabit)
                || self.check_keyword_ident(TokenKind::Postpara)
                || self.check_keyword_ident(TokenKind::Postparabit)
            {
                let start = self.current_span();
                let kind = if self.eat_keyword_ident(TokenKind::Praepara) {
                    PraeparaKind::Praepara
                } else if self.eat_keyword_ident(TokenKind::Praeparabit) {
                    PraeparaKind::Praeparabit
                } else if self.eat_keyword_ident(TokenKind::Postpara) {
                    PraeparaKind::Postpara
                } else if self.eat_keyword_ident(TokenKind::Postparabit) {
                    PraeparaKind::Postparabit
                } else {
                    return Err(Self::expected_error("praepara hook", self.current_span()));
                };
                let all = self.eat_keyword_ident(TokenKind::Omnia);
                let body = self.parse_block()?;
                let span = start.merge(self.previous_span());
                setup.push(PraeparaBlock {
                    kind,
                    all,
                    body,
                    span,
                });
            } else if self.check_keyword_ident(TokenKind::Probandum) {
                if let StmtKind::Probandum(test) = self.parse_probandum_decl()? {
                    nested.push(test);
                }
            } else if self.check_keyword_ident(TokenKind::Proba) {
                let case = self.parse_proba_case()?;
                tests.push(case);
            } else {
                return Err(
                    ParseError::structured(ParseErrorKind::Expected, self.current_span())
                        .with_arg("issue", "test_case_or_setup")
                        .with_arg("proba", "proba")
                        .with_arg("probandum", "probandum")
                        .with_arg("setup", "praepara"),
                );
            }
        }

        Ok(ProbandumBody {
            setup,
            tests,
            nested,
        })
    }

    fn parse_proba_case(&mut self) -> Result<ProbaCase, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Proba, "expected 'proba'")?;

        let name = self.parse_string()?;

        let modifiers = self.parse_test_modifiers()?;

        let body = self.parse_block()?;

        let span = start.merge(self.previous_span());
        Ok(ProbaCase {
            modifiers,
            name,
            body,
            span,
        })
    }

    fn parse_test_modifiers(&mut self) -> Result<Vec<ProbaModifier>, ParseError> {
        let mut modifiers = Vec::new();
        loop {
            if self.eat_keyword_ident(TokenKind::Omitte) {
                let reason = self.parse_string()?;
                modifiers.push(ProbaModifier::Omitte(reason));
            } else if self.eat_keyword_ident(TokenKind::Futurum) {
                let reason = self.parse_string()?;
                modifiers.push(ProbaModifier::Futurum(reason));
            } else if self.eat_keyword_ident(TokenKind::Solum) {
                modifiers.push(ProbaModifier::Solum);
            } else if self.eat_keyword_ident(TokenKind::Tag) {
                let tag = self.parse_string()?;
                modifiers.push(ProbaModifier::Tag(tag));
            } else if self.eat_keyword_ident(TokenKind::Temporis) {
                let n = self.parse_test_modifier_integer("temporis")?;
                modifiers.push(ProbaModifier::Temporis(n));
            } else if self.eat_keyword_ident(TokenKind::Metior) {
                modifiers.push(ProbaModifier::Metior);
            } else if self.eat_keyword_ident(TokenKind::Repete) {
                let n = self.parse_test_modifier_integer("repete")?;
                modifiers.push(ProbaModifier::Repete(n));
            } else if self.eat_keyword_ident(TokenKind::Fragilis) {
                let n = self.parse_test_modifier_integer("fragilis")?;
                modifiers.push(ProbaModifier::Fragilis(n));
            } else if self.eat_keyword_ident(TokenKind::Requirit) {
                let req = self.parse_string()?;
                modifiers.push(ProbaModifier::Requirit(req));
            } else if self.eat_keyword_ident(TokenKind::SolumIn) {
                let env = self.parse_string()?;
                modifiers.push(ProbaModifier::SolumIn(env));
            } else {
                break;
            }
        }
        Ok(modifiers)
    }

    fn parse_test_modifier_integer(&mut self, modifier: &'static str) -> Result<i64, ParseError> {
        match self.peek().kind {
            TokenKind::Integer(n) => {
                self.advance();
                i64::try_from(n).map_err(|_| {
                    ParseError::structured(ParseErrorKind::Unexpected, self.previous_span())
                        .with_arg("issue", "test_modifier_integer_out_of_range")
                        .with_arg("modifier", modifier)
                })
            }
            _ => Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "test_modifier_integer")
                    .with_arg("modifier", modifier),
            ),
        }
    }

    /// Parse one or more declaration annotations.
    ///
    /// Known CLI/package annotations are parsed into structured `AST` variants so
    /// `faber` can build command metadata without reparsing token payloads.
    /// Unknown annotations remain token-backed statement annotations for later
    /// target- or stdlib-specific consumers.
    fn parse_annotations(&mut self) -> Result<Vec<Annotation>, ParseError> {
        let mut annotations = Vec::new();

        while self.check(&TokenKind::At) {
            let start = self.current_span();
            self.advance(); // @

            let kind = self.parse_annotation_kind()?;
            let span = start.merge(self.previous_span());

            annotations.push(Annotation { kind, span });
        }

        Ok(annotations)
    }

    fn parse_annotation_kind(&mut self) -> Result<AnnotationKind, ParseError> {
        let name = self.parse_annotation_name()?;

        if self.check(&TokenKind::LBrace) {
            return self.parse_braced_annotation(name);
        }

        // Dispatch on the consumed annotation name's keyword identity (any
        // reader locale), not the current token (which is already past it).
        let annotation_identity = {
            let latin = self.identifier_text_or_keyword_latin_key(name.name);
            lookup_keyword_spec(latin).and_then(|spec| spec.token_kind.clone())
        };
        match annotation_identity {
            Some(TokenKind::Cli) => return self.parse_cli_annotation(),
            Some(TokenKind::Imperium) => return self.parse_imperium_annotation(),
            Some(TokenKind::Optio) => return self.parse_optio_annotation(),
            Some(TokenKind::Operandus) => return self.parse_operandus_annotation(),
            Some(TokenKind::Radix) if self.next_annotation_ident_is(TokenKind::Typus) => {
                return self.parse_radix_typus_domain_annotation();
            }
            Some(TokenKind::Radix) if self.next_word_is("lane") => {
                return self.parse_radix_lane_annotation();
            }
            Some(TokenKind::Publica) => {
                self.expect_annotation_end("unexpected token after @ publica")?;
                return Ok(AnnotationKind::Publica);
            }
            Some(TokenKind::Protecta) => {
                self.expect_annotation_end("unexpected token after @ protecta")?;
                return Ok(AnnotationKind::Protecta);
            }
            Some(TokenKind::Privata) => {
                self.expect_annotation_end("unexpected token after @ privata")?;
                return Ok(AnnotationKind::Privata);
            }
            _ => {}
        }

        let mut args = Vec::new();

        while self.is_annotation_arg() {
            args.push(self.advance().clone());
        }

        Ok(AnnotationKind::Statement(AnnotationStmt { name, args }))
    }

    fn parse_braced_annotation(&mut self, name: Ident) -> Result<AnnotationKind, ParseError> {
        let family = self.interner.resolve(name.name).to_owned();
        self.expect(&TokenKind::LBrace, "expected '{' after annotation name")?;

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let field_start = self.current_span();
            let field_name = self.parse_annotation_field_name()?;
            let field_name_str = self.interner.resolve(field_name.name).to_owned();
            self.expect(&TokenKind::Eq, "expected '=' in annotation field")?;

            let value = if family == "annotatio" && field_name_str == "target" {
                self.parse_annotatio_target_value()?
            } else if self.braced_annotation_field_uses_type(&family, &field_name_str) {
                AnnotationFieldValue::Type(self.parse_type()?)
            } else {
                AnnotationFieldValue::Expr(Box::new(self.parse_expression()?))
            };

            let span = field_start.merge(self.previous_span());
            fields.push(AnnotationField {
                name: field_name,
                value,
                span,
            });

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        self.expect(&TokenKind::RBrace, "expected '}' after annotation fields")?;
        Ok(AnnotationKind::Braced(BracedAnnotation { name, fields }))
    }

    fn braced_annotation_field_uses_type(&self, family: &str, field_name: &str) -> bool {
        matches!(
            (family, field_name),
            ("optio" | "operandus", "typus") | ("radix", "allowed")
        )
    }

    /// Parse `@ annotatio { target = … }` values.
    ///
    /// `functio` is a keyword, so ordinary expression parsing rejects it. Accept the
    /// attachment-target vocabulary as keyword idents, string literals, or bare idents.
    fn parse_annotatio_target_value(&mut self) -> Result<AnnotationFieldValue, ParseError> {
        if self.check_keyword_ident(TokenKind::Functio) {
            let span = self.peek().span;
            self.advance();
            let name = self.keyword_ident("functio", span);
            return Ok(self.named_type_annotation_value(name));
        }
        if matches!(
            self.peek().kind,
            TokenKind::String(_) | TokenKind::AsciiString(_)
        ) {
            return Ok(AnnotationFieldValue::Expr(Box::new(
                self.parse_expression()?,
            )));
        }
        if matches!(
            self.peek().kind,
            TokenKind::Ident(_) | TokenKind::Underscore(_)
        ) {
            let name = self.parse_ident()?;
            return Ok(self.named_type_annotation_value(name));
        }
        Err(self.invalid_annotation_error("annotatio_unknown_target"))
    }

    fn named_type_annotation_value(&self, name: Ident) -> AnnotationFieldValue {
        let span = name.span;
        AnnotationFieldValue::Type(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(name, Vec::new()),
            span,
        })
    }

    fn parse_cli_annotation(&mut self) -> Result<AnnotationKind, ParseError> {
        let name = self.parse_string()?;
        self.expect_annotation_end("unexpected token after @ cli name")?;
        Ok(AnnotationKind::Cli(CliAnnotation { name }))
    }

    fn parse_imperium_annotation(&mut self) -> Result<AnnotationKind, ParseError> {
        let name = self.parse_string()?;
        self.expect_annotation_end("unexpected token after @ imperium name")?;
        Ok(AnnotationKind::Imperium(ImperiumAnnotation { name }))
    }

    fn parse_optio_annotation(&mut self) -> Result<AnnotationKind, ParseError> {
        let binding = self.parse_ident()?;
        let mut ty = None;
        let mut short = None;
        let mut long = None;
        let mut description = None;
        let mut global = false;
        let mut default = None;

        while self.is_annotation_arg() {
            if self.eat_annotation_ident(TokenKind::Brevis) {
                short = Some(self.parse_string()?);
            } else if self.eat_annotation_ident(TokenKind::Longum) {
                long = Some(self.parse_string()?);
            } else if self.eat_annotation_ident(TokenKind::Typus) {
                ty = Some(self.parse_type()?);
            } else if self.eat_annotation_ident(TokenKind::Descriptio) {
                description = Some(self.parse_string()?);
            } else if self.eat_annotation_ident(TokenKind::Ubique) {
                global = true;
            } else if self.eat_annotation_ident(TokenKind::Vel) {
                default = Some(Box::new(self.parse_annotation_default()?));
            } else {
                return Err(self
                    .invalid_annotation_error("invalid_modifier")
                    .with_arg("annotation", "optio"));
            }
        }

        let flag = ty
            .as_ref()
            .is_some_and(|ty| self.is_annotation_bivalens_type(ty));

        Ok(AnnotationKind::Optio(OptioAnnotation {
            binding,
            ty,
            short,
            long,
            flag,
            description,
            global,
            default,
        }))
    }

    fn parse_operandus_annotation(&mut self) -> Result<AnnotationKind, ParseError> {
        let rest = self.eat_annotation_ident(TokenKind::Ceteri);
        let ty = self.parse_type()?;
        let binding = self.parse_ident()?;
        let mut description = None;
        let mut global = false;
        let mut default = None;

        while self.is_annotation_arg() {
            if self.eat_annotation_ident(TokenKind::Descriptio) {
                description = Some(self.parse_string()?);
            } else if self.eat_annotation_ident(TokenKind::Ubique) {
                global = true;
            } else if self.eat_annotation_ident(TokenKind::Vel) {
                default = Some(Box::new(self.parse_annotation_default()?));
            } else {
                return Err(self
                    .invalid_annotation_error("invalid_modifier")
                    .with_arg("annotation", "operandus"));
            }
        }

        Ok(AnnotationKind::Operandus(OperandusAnnotation {
            rest,
            ty,
            binding,
            description,
            global,
            default,
        }))
    }

    fn parse_radix_typus_domain_annotation(&mut self) -> Result<AnnotationKind, ParseError> {
        self.expect_annotation_ident(TokenKind::Typus, "expected 'typus' after @ radix")?;
        let param = self.parse_ident()?;
        self.expect_annotation_ident(TokenKind::In, "expected 'in' in @ radix typus domain")?;

        let mut allowed = Vec::new();
        while self.is_annotation_type_start() {
            allowed.push(self.parse_type()?);
        }

        if allowed.is_empty() {
            return Err(self.invalid_annotation_error("empty_radix_typus_domain"));
        }
        self.expect_annotation_end("unexpected token after @ radix typus domain")?;
        Ok(AnnotationKind::RadixTypusDomain(
            RadixTypusDomainAnnotation { param, allowed },
        ))
    }

    fn parse_radix_lane_annotation(&mut self) -> Result<AnnotationKind, ParseError> {
        self.expect_annotation_word("lane", "expected 'lane' after @ radix")?;
        let lane = self.parse_lane_value()?;
        self.expect_annotation_end("unexpected token after @ radix lane value")?;
        Ok(AnnotationKind::RadixLane(RadixLaneAnnotation { lane }))
    }

    /// Accept a string literal or a bare identifier as the lane value.
    /// Identifiers are resolved through the interner so that known lane
    /// names (`air`, `mir`, `hir-direct`) work unquoted in statement
    /// annotations.
    fn parse_lane_value(&mut self) -> Result<radix_lexer::Symbol, ParseError> {
        let token = self.advance();
        match &token.kind {
            TokenKind::String(sym) => Ok(*sym),
            TokenKind::Ident(sym) => Ok(*sym),
            _ => Err(Self::expected_error(
                "string literal or identifier",
                token.span,
            )),
        }
    }

    fn parse_annotation_field_name(&mut self) -> Result<Ident, ParseError> {
        let token = self.advance();
        let span = token.span;
        match token.kind {
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => {
                // Surface → Latin so braced field keys accept pack spellings
                // (`short` → `brevis`) while HIR keeps Latin field names.
                let latin = self.identifier_text_or_keyword_latin_key(sym).to_owned();
                Ok(self.keyword_ident(&latin, span))
            }
            TokenKind::Ceteri => Ok(self.keyword_ident("ceteri", span)),
            TokenKind::In => Ok(self.keyword_ident("in", span)),
            _ => Err(ParseError::structured(ParseErrorKind::Expected, span)
                .with_arg("expected", "identifier")
                .with_arg("issue", "annotation_field_name")),
        }
    }

    fn parse_annotation_name(&mut self) -> Result<Ident, ParseError> {
        let token = self.advance();
        let span = token.span;
        match token.kind {
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => {
                // Surface → Latin family identity (`kernel` → `nucleum`). Unknown
                // / user contract names stay as authored spellings.
                let latin = self.identifier_text_or_keyword_latin_key(sym).to_owned();
                Ok(self.keyword_ident(&latin, span))
            }
            TokenKind::Publica => Ok(self.keyword_ident("publica", span)),
            TokenKind::Protecta => Ok(self.keyword_ident("protecta", span)),
            TokenKind::Privata => Ok(self.keyword_ident("privata", span)),
            TokenKind::Futura => Ok(self.keyword_ident("futura", span)),
            TokenKind::Cursor => Ok(self.keyword_ident("cursor", span)),
            TokenKind::Tag => Ok(self.keyword_ident("tag", span)),
            TokenKind::Solum => Ok(self.keyword_ident("solum", span)),
            TokenKind::Omitte => Ok(self.keyword_ident("omitte", span)),
            TokenKind::Metior => Ok(self.keyword_ident("metior", span)),
            _ => Err(ParseError::structured(ParseErrorKind::Expected, span)
                .with_arg("issue", "annotation_name")),
        }
    }

    fn eat_annotation_ident(&mut self, expected: TokenKind) -> bool {
        if self.next_annotation_ident_is(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn next_annotation_ident_is(&self, expected: TokenKind) -> bool {
        // `expected` is a keyword identity; the current word resolves to its
        // identity through the registry in any reader locale.
        self.current_keyword_ident() == Some(expected)
    }

    /// Check the current word against a literal grammar word (not a registry
    /// keyword), e.g. the `lane` marker in `@ radix lane "…"`. Matches the
    /// source spelling or its resolved latin key.
    fn next_word_is(&self, text: &str) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        self.identifier_text_or_keyword_latin_key(sym) == text
    }

    fn expect_annotation_ident(
        &mut self,
        expected: TokenKind,
        message: &str,
    ) -> Result<(), ParseError> {
        if self.next_annotation_ident_is(expected.clone()) {
            self.advance();
            Ok(())
        } else {
            let expected_spelling = keyword_spelling_for_token(&expected).unwrap_or("");
            Err(self
                .invalid_annotation_error("expected_annotation_ident")
                .with_arg("expected", expected_spelling)
                .with_arg(
                    "context",
                    message.strip_prefix("expected ").unwrap_or(message),
                ))
        }
    }

    fn expect_annotation_word(&mut self, text: &str, message: &str) -> Result<(), ParseError> {
        if self.next_word_is(text) {
            self.advance();
            Ok(())
        } else {
            Err(self
                .invalid_annotation_error("expected_annotation_ident")
                .with_arg("expected", text)
                .with_arg(
                    "context",
                    message.strip_prefix("expected ").unwrap_or(message),
                ))
        }
    }

    fn expect_annotation_end(&mut self, message: &str) -> Result<(), ParseError> {
        if self.is_annotation_arg() {
            Err(self
                .invalid_annotation_error("unexpected_annotation_token")
                .with_arg(
                    "context",
                    message
                        .strip_prefix("unexpected token after ")
                        .unwrap_or(message),
                ))
        } else {
            Ok(())
        }
    }

    fn invalid_annotation_error(&self, issue: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::InvalidAnnotation, self.current_span())
            .with_arg("issue", issue)
    }

    fn parse_annotation_default(&mut self) -> Result<Expr, ParseError> {
        if let TokenKind::Ident(sym) = self.peek().kind {
            let literal = match self.interner.resolve(sym) {
                "verum" => Some(Literal::Bool(true)),
                "falsum" => Some(Literal::Bool(false)),
                "nihil" => Some(Literal::Nil),
                _ => None,
            };

            if let Some(literal) = literal {
                let span = self.peek().span;
                self.advance();
                let id = self.next_id();
                return Ok(Expr {
                    id,
                    kind: ExprKind::Literal(literal),
                    span,
                });
            }
        }

        self.parse_expression()
    }

    fn is_annotation_bivalens_type(&self, ty: &TypeExpr) -> bool {
        matches!(
            &ty.kind,
            TypeExprKind::Named(name, params)
                if params.is_empty()
                    && !ty.nullable
                    && ty.mode.is_none()
                    && self.interner.resolve(name.name) == "bivalens"
        )
    }

    fn is_declaration_start_token(&self) -> bool {
        // Token-shaped introducers plus the tokenless Batch E words, which
        // arrive as identifiers and are claimed by spelling in
        // statement-initial position. `typus` is deliberately absent: it is
        // an annotation modifier key (`@optio … typus <type>`), so in
        // annotation-arg position it stays an argument, matching the
        // pre-migration token behavior (is_declaration_start_token never
        // listed Typus; is_annotation_arg listed it as an arg).
        matches!(
            self.peek().kind,
            TokenKind::LBrace
        ) || self.check_keyword_ident(TokenKind::Abstractus)
            || self.check_keyword_ident(TokenKind::Importa)
            || self.check_keyword_ident(TokenKind::Incipit)
            || self.check_keyword_ident(TokenKind::Proba)
            || self.check_keyword_ident(TokenKind::Probandum)
            || self.check_keyword_ident(TokenKind::Discretio)
            || self.check_keyword_ident(TokenKind::Fixum)
            || self.check_keyword_ident(TokenKind::Functio)
            || self.check_keyword_ident(TokenKind::Genus)
            || self.check_keyword_ident(TokenKind::Implendum)
            || self.check_keyword_ident(TokenKind::Ordo)
            || self.check_keyword_ident(TokenKind::Sit)
            || self.check_keyword_ident(TokenKind::Varia)
    }

    fn is_annotation_arg(&self) -> bool {
        if self.check(&TokenKind::At) || self.is_declaration_start_token() {
            return false;
        }

        matches!(
            self.peek().kind,
            TokenKind::Ident(_)
                | TokenKind::Underscore(_)
                | TokenKind::String(_)
                | TokenKind::Integer(_)
                | TokenKind::Float(_)
                | TokenKind::Comma
                | TokenKind::Ceteri
                | TokenKind::De
                | TokenKind::Ex
                | TokenKind::Falsum
                | TokenKind::In
                | TokenKind::LParen
                | TokenKind::RParen
                | TokenKind::Arrow
                | TokenKind::ExitArrow
                | TokenKind::LtAscii
                | TokenKind::GtAscii
                | TokenKind::Colon
                | TokenKind::Dot
                | TokenKind::Eq
                | TokenKind::Assign
                | TokenKind::Nihil
                | TokenKind::Si
                | TokenKind::Typus
                | TokenKind::Vel
                | TokenKind::Verum
        )
    }

    fn is_annotation_type_start(&self) -> bool {
        match self.peek().kind {
            TokenKind::Ident(sym) => {
                let latin = self.identifier_text_or_keyword_latin_key(sym);
                match lookup_keyword_spec(latin) {
                    // A keyword ends the annotation type list unless it is a
                    // type-position word (`modulus`, `iuncta`).
                    Some(spec) => matches!(
                        spec.scope,
                        KeywordScope::Contextual(owners)
                            if owners.iter().any(|o| matches!(
                                o,
                                KeywordOwner::TypeName | KeywordOwner::Constructor
                            ))
                    ),
                    _ => true,
                }
            }
            _ => matches!(
                self.peek().kind,
                TokenKind::Underscore(_) | TokenKind::LParen
            ),
        }
    }

    // =============================================================================
    // BLOCK AND ANNOTATION PARSING
    // =============================================================================

    /// Parse a statement block and recover within its brace boundary.
    ///
    /// GRAMMAR:
    ///   block := '{' statement* '}'
    ///
    /// Block recovery passes `stop_at_rbrace = true` so a bad inner statement can
    /// be skipped without consuming the `}` that returns control to the caller.
    pub(super) fn parse_block(&mut self) -> Result<BlockStmt, ParseError> {
        let start = self.current_span();
        self.expect(&TokenKind::LBrace, "expected '{'")?;

        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            match self.parse_stmt() {
                Ok(stmt) => stmts.push(stmt),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize(true);
                }
            }
        }

        // Trivia immediately before the consuming `}` has no following
        // statement; retain it on the block-end boundary so the post-parse
        // completeness check can claim it.
        let end_trivia = self.parse_block_end_trivia();

        self.expect(&TokenKind::RBrace, "expected '}'")?;

        let span = start.merge(self.previous_span());
        Ok(BlockStmt {
            statements: stmts,
            end_trivia,
            span,
        })
    }

    /// Parse an optional angle-bracket generic parameter list.
    ///
    /// GRAMMAR:
    ///   generic-params := '<' generic-param (',' generic-param)* '>'
    ///   generic-param  := ident | `magnitudo` ident
    ///
    /// Type parameters (`T`) precede size parameters (`magnitudo N`).
    fn parse_generic_params(&mut self) -> Result<Vec<TypeParam>, ParseError> {
        if !self.eat(&TokenKind::LtAscii) {
            return Ok(Vec::new());
        }

        let mut params = Vec::new();
        let mut seen_magnitudo = false;
        loop {
            let start = self.current_span();
            let (kind, name) = if self.eat_keyword_ident(TokenKind::Magnitudo) {
                seen_magnitudo = true;
                (GenericParamKind::Magnitudo, self.parse_ident()?)
            } else {
                if seen_magnitudo {
                    return Err(self.type_param_after_magnitudo_error());
                }
                (GenericParamKind::Typus, self.parse_ident()?)
            };
            let span = start.merge(name.span);
            params.push(TypeParam { kind, name, span });

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        self.expect(&TokenKind::GtAscii, "expected '>' after generic parameters")?;
        Ok(params)
    }

    fn removed_prae_typus_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::InvalidTypeParameter, self.current_span())
            .with_arg("issue", "removed_prae_typus")
            .with_arg("keyword", "prae typus")
            .with_arg("replacement", "functio name<T>(...)")
    }

    fn removed_fixus_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::InvalidParameter, self.current_span())
            .with_arg("issue", "removed_fixus")
            .with_arg("keyword", "fixus")
            .with_arg("optional_marker", "sponte")
            .with_arg("immutable_marker", "fixum")
    }

    fn type_param_after_magnitudo_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::InvalidTypeParameter, self.current_span())
            .with_arg("issue", "type_param_after_magnitudo")
            .with_arg("size_param", "magnitudo")
    }
}
