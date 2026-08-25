//! Executable statement grammar for Faber blocks and entrypoints.
//!
//! This module owns the non-declaration statement forms that can appear inside
//! blocks or at file scope: conditionals, loops, dispatch, transfer, diagnostics,
//! entrypoints, resource scopes, and destructuring statements. It shares
//! `StmtKind` construction with `decl.rs`, but keeps executable grammar separate
//! so declaration syntax and runtime control-flow syntax do not blur.
//! Statement-level `ad` is rejected here in favor of the expression form.
//!
//! Statement bodies deliberately use a small set of shapes: braced blocks for
//! scoped bodies, `ergo` for single-statement bodies, and `fac { ... } cape ...`
//! for structured error handling. The parser records surface intent and leaves
//! reachability, exhaustiveness, ownership, and endpoint semantics to later
//! phases.
//!
//! - `sin` is the canonical else-if spelling; `secus si` is diagnosed instead
//!   of being accepted as an alias.
//! - `elige` is value dispatch, while `discerne` is pattern dispatch. Their AST
//!   shapes stay separate for later exhaustiveness and codegen decisions.
//! - Optional `cape` parsing is centralized so catch clauses attach consistently
//!   to statements that support them.

use super::{ParseError, ParseErrorKind, Parser};
use radix_lexer::{Span, TokenKind};
#[allow(clippy::wildcard_imports)]
use radix_syntax::*;

// CONDITIONAL STATEMENTS

impl Parser {
    /// Parse `si` / `sin` / `secus` branching.
    ///
    /// GRAMMAR:
    ///   si-stmt := `si` expr if-body [`cape` ident block] [`secus` secus-clause | `sin` si-stmt]
    ///   if-body := block | `ergo` stmt   (`∴` is clausura-only; not accepted)
    ///
    /// Branch bodies accept either a block or `ergo` single statement. `cape`
    /// attaches to the branch body being parsed, which preserves error-handling
    /// scope for nested `sin` chains.
    pub(super) fn parse_si_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Si, "expected 'si'")?;
        Ok(StmtKind::Si(self.parse_si_tail()?))
    }

    /// Parse `sin` after the keyword has already been consumed.
    ///
    /// `sin` shares the same tail shape as `si`; only the leading keyword differs.
    fn parse_sin_stmt(&mut self) -> Result<SiStmt, ParseError> {
        self.parse_si_tail()
    }

    /// Parse the shared condition/body/catch/tail shape for `si` and `sin`.
    fn parse_si_tail(&mut self) -> Result<SiStmt, ParseError> {
        let cond = Box::new(self.parse_expression()?);
        let then = self.parse_ergo_body()?;
        let catch = self.try_parse_cape_stmt()?;

        // Comments and blank lines between branches of one chain sit at the
        // ordinary statement-level divider position. Capture them as
        // branch-link leading trivia so they are preserved and claimed
        // instead of skipped as anonymous trivia.
        let else_ = if self.check_keyword_ident(TokenKind::Secus)
            || self.check_keyword_ident(TokenKind::Sin)
        {
            let leading_trivia = self.parse_leading_trivia();
            if self.eat_keyword_ident(TokenKind::Secus) {
                Some(self.parse_secus_stmt(leading_trivia)?)
            } else {
                self.expect_keyword_ident(TokenKind::Sin, "expected 'sin'")?;
                Some(SecusClause::Sin {
                    stmt: Box::new(self.parse_sin_stmt()?),
                    leading_trivia,
                })
            }
        } else {
            None
        };

        Ok(SiStmt {
            cond,
            then,
            catch,
            else_,
        })
    }

    /// Parse a statement body that can be braced or introduced by `ergo`.
    ///
    /// `∴` is not accepted here (clausura joint only) — wrong-context error.
    fn parse_ergo_body(&mut self) -> Result<IfBody, ParseError> {
        if self.check(&TokenKind::LBrace) {
            Ok(IfBody::Block(self.parse_block()?))
        } else if self.check(&TokenKind::Therefore) {
            Err(self.therefore_wrong_context_error())
        } else if self.eat_keyword_ident(TokenKind::Ergo) {
            Ok(IfBody::Ergo(Box::new(self.parse_stmt()?)))
        } else {
            Err(self.missing_block_error())
        }
    }

    fn parse_secus_stmt(&mut self, leading_trivia: Vec<Trivia>) -> Result<SecusClause, ParseError> {
        if self.check_keyword_ident(TokenKind::Si) {
            return Err(self.expected_statement_error("secus_si_else_if"));
        }

        if self.check(&TokenKind::LBrace) {
            let body = self.parse_block()?;
            let catch = self.try_parse_cape_stmt()?;
            Ok(SecusClause::Block {
                body,
                catch,
                leading_trivia,
            })
        } else if self.check(&TokenKind::Therefore) {
            Err(self.therefore_wrong_context_error())
        } else if self.eat_keyword_ident(TokenKind::Ergo) {
            let stmt = Box::new(self.parse_stmt()?);
            let catch = self.try_parse_cape_stmt()?;
            Ok(SecusClause::Stmt {
                stmt,
                catch,
                leading_trivia,
            })
        } else {
            Err(self.missing_block_error())
        }
    }

    fn missing_block_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::BlockAbsent, self.current_span())
            .with_arg("block", "{ ... }")
            .with_arg("ergo", "ergo")
    }

    fn therefore_wrong_context_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::ThereforeWrongContext, self.current_span())
    }

    // LOOP STATEMENTS

    /// Parse `dum`.
    ///
    /// GRAMMAR:
    ///   dum-stmt := `dum` expr if-body [`cape` ident block]
    pub(super) fn parse_dum_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Dum, "expected 'dum'")?;

        let cond = Box::new(self.parse_expression()?);
        let body = self.parse_ergo_body()?;
        let catch = self.try_parse_cape_stmt()?;

        Ok(StmtKind::Dum(DumStmt { cond, body, catch }))
    }

    /// Parse an `itera` loop and its ownership mode.
    ///
    /// GRAMMAR:
    ///   itera-stmt := `itera` mode expr [apud-clause] (`fixum`|`varia`) binding-pattern if-body [`cape` ident block]
    ///   mode := `ex` | `de` | `ab`
    ///   apud-clause := `apud` `[` IDENTIFIER (`,` IDENTIFIER)* `]`
    ///
    /// The parser records the source preposition mode; later phases validate
    /// the semantic source shape for that mode.
    pub(super) fn parse_itera_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Itera, "expected 'itera'")?;

        let mode = if self.eat_keyword_ident(TokenKind::Ex) {
            IteraMode::Ex
        } else if self.eat_keyword_ident(TokenKind::De) {
            IteraMode::De
        } else if self.eat_identifier_text("ab") {
            IteraMode::Ab
        } else {
            return Err(self
                .expected_statement_error("itera_mode")
                .with_arg("expected", "'ex', 'de', or 'ab'"));
        };

        let iterable = Box::new(self.parse_expression()?);
        let apud = if self.eat_keyword_ident(TokenKind::Apud) {
            self.parse_apud_clause()?
        } else {
            Vec::new()
        };

        let mutability_span = self.current_span();
        let mutability = self.parse_mutability()?;
        // HIR's itera node does not retain the source mutability marker, so
        // reject this mode conflict while the parser still has that fact.
        if matches!(mode, IteraMode::De) && matches!(mutability, AstMutability::Mutable) {
            return Err(
                ParseError::structured(ParseErrorKind::Expected, mutability_span)
                    .with_arg("issue", "itera_de_requires_fixum")
                    .with_arg("expected", "the const form `fixum`"),
            );
        }
        // A top-level `_` in an itera binder is the legacy unused loop
        // variable, not a destructuring wildcard. Keep it as an identifier;
        // wildcard patterns remain available inside array/object binders.
        let binding = if matches!(self.peek().kind, TokenKind::Underscore(_)) {
            BindingPattern::Ident(self.parse_ident()?)
        } else {
            self.parse_binding_pattern()?
        };
        let body = self.parse_ergo_body()?;
        let catch = self.try_parse_cape_stmt()?;

        Ok(StmtKind::Itera(IteraStmt {
            mode,
            iterable,
            apud,
            mutability,
            binding,
            body,
            catch,
        }))
    }

    /// Parse the bracketed coordinate names claimed by an `apud` clause.
    pub(super) fn parse_apud_clause(&mut self) -> Result<Vec<Ident>, ParseError> {
        self.expect(&TokenKind::LBracket, "expected '[' after 'apud'")?;

        let mut coordinates = vec![self.parse_ident()?];
        while self.eat(&TokenKind::Comma) {
            coordinates.push(self.parse_ident()?);
        }
        self.expect(
            &TokenKind::RBracket,
            "expected ']' after `apud` coordinates",
        )?;
        Ok(coordinates)
    }

    // PATTERN MATCHING AND DISPATCH

    /// Parse value-based `elige` dispatch.
    ///
    /// GRAMMAR:
    ///   elige-stmt := `elige` expr '{' casu-case* [ceterum-default] '}' [`cape` ident block]
    ///   casu-case := `casu` expr if-body
    ///   ceterum-default := `ceterum` if-body
    ///
    /// `elige` cases are expressions, not patterns. Keeping this separate from
    /// `discerne` prevents destructuring-only syntax from leaking into value
    /// dispatch.
    pub(super) fn parse_elige_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Elige, "expected 'elige'")?;

        let expr = Box::new(self.parse_expression()?);

        self.expect(&TokenKind::LBrace, "expected '{'")?;

        let mut cases = Vec::new();
        let mut default = None;

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.eat_keyword_ident(TokenKind::Casu) {
                let start = self.current_span();
                let value = Box::new(self.parse_expression()?);
                let body = self.parse_ergo_body()?;
                let span = start.merge(self.previous_span());
                cases.push(CasuCase { value, body, span });
            } else if self.eat_keyword_ident(TokenKind::Ceterum) {
                let start = self.current_span();
                let body = self.parse_ergo_body()?;
                let span = start.merge(self.previous_span());
                default = Some(CeterumDefault { body, span });
                break;
            } else {
                return Err(self.invalid_casu_arm_error());
            }
        }

        self.expect(&TokenKind::RBrace, "expected '}'")?;

        let catch = self.try_parse_cape_stmt()?;

        Ok(StmtKind::Elige(EligeStmt {
            expr,
            cases,
            default,
            catch,
        }))
    }

    /// Parse pattern-based `discerne` dispatch.
    ///
    /// GRAMMAR:
    ///   discerne-stmt := `discerne` [`omnia`] expr (',' expr)* '{' arm* [`default`] '}'
    ///   arm := `casu` pattern (',' pattern)* if-body
    ///
    /// `omnia` is only recorded here; later phases own the exhaustive-coverage
    /// check because they know the subject types and variant sets.
    pub(super) fn parse_discerne_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Discerne, "expected 'discerne'")?;

        let exhaustive = self.eat_keyword_ident(TokenKind::Omnia);

        let mut subjects = Vec::new();
        loop {
            subjects.push(self.parse_expression()?);
            if !self.consume_item_separator(&TokenKind::LBrace, "discerne_subjects")? {
                break;
            }
        }

        self.expect(&TokenKind::LBrace, "expected '{'")?;

        let mut arms = Vec::new();
        let mut default = None;

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.eat_keyword_ident(TokenKind::Casu) {
                let start = self.current_span();
                let patterns = self.parse_patterns()?;
                let body = self.parse_ergo_body()?;
                let span = start.merge(self.previous_span());
                arms.push(CasuArm {
                    patterns,
                    body,
                    span,
                });
            } else if self.eat_keyword_ident(TokenKind::Ceterum) {
                let start = self.current_span();
                let body = self.parse_ergo_body()?;
                let span = start.merge(self.previous_span());
                default = Some(CeterumDefault { body, span });
                break;
            } else {
                return Err(self.invalid_casu_arm_error());
            }
        }

        self.expect(&TokenKind::RBrace, "expected '}'")?;

        Ok(StmtKind::Discerne(DiscerneStmt {
            exhaustive,
            subjects,
            arms,
            default,
        }))
    }

    fn invalid_casu_arm_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::CasuArmInvalid, self.current_span())
            .with_arg("casu", "casu")
            .with_arg("ceterum", "ceterum")
    }

    /// Parse `custodi`.
    ///
    /// GRAMMAR:
    ///   custodi-stmt := `custodi` '{' (`si` expr if-body)+ '}'
    pub(super) fn parse_custodi_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Custodi, "expected 'custodi'")?;
        self.expect(&TokenKind::LBrace, "expected '{'")?;

        let mut clauses = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            self.expect_keyword_ident(TokenKind::Si, "expected 'si'")?;
            let start = self.current_span();
            let cond = Box::new(self.parse_expression()?);
            let body = self.parse_ergo_body()?;
            let span = start.merge(self.previous_span());
            clauses.push(CustodiClause { cond, body, span });
        }

        self.expect(&TokenKind::RBrace, "expected '}'")?;

        Ok(StmtKind::Custodi(CustodiStmt { clauses }))
    }

    /// Parse `fac`, the canonical block form for do/try-style execution.
    ///
    /// GRAMMAR:
    ///   fac-stmt := `fac` block [`cape` ident block] [`dum` expr]
    ///
    /// At statement level, a trailing `dum` turns `fac` into a do-while form.
    /// Closure bodies call the shared construct with `allow_while = false` so
    /// expression syntax cannot smuggle in a statement loop.
    pub(super) fn parse_fac_stmt(&mut self) -> Result<StmtKind, ParseError> {
        Ok(StmtKind::Fac(self.parse_fac_construct(true)?))
    }

    pub(super) fn parse_fac_construct(&mut self, allow_while: bool) -> Result<FacStmt, ParseError> {
        self.expect_keyword_ident(TokenKind::Fac, "expected 'fac'")?;

        let body = self.parse_block()?;
        let catch = self.try_parse_cape_stmt()?;

        let while_ = if self.eat_keyword_ident(TokenKind::Dum) {
            if !allow_while {
                return Err(self.invalid_expression_error("closure_fac_body_dum"));
            }
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        Ok(FacStmt {
            body,
            catch,
            while_,
        })
    }

    // TRANSFER STATEMENTS

    /// Parse `redde`.
    ///
    /// GRAMMAR:
    ///   redde-stmt := `redde` [`expr`]
    pub(super) fn parse_redde_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Redde, "expected 'redde'")?;

        let value = if !self.is_at_end()
            && !self.check(&TokenKind::RBrace)
            && !self.check(&TokenKind::Semicolon)
        {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        Ok(StmtKind::Redde(ReddeStmt { value }))
    }

    /// Parse `rumpe` (loop break).
    ///
    /// GRAMMAR:
    ///   rumpe-stmt := `rumpe`
    pub(super) fn parse_rumpe_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let span = self.current_span();
        self.expect_keyword_ident(TokenKind::Rumpe, "expected 'rumpe'")?;
        Ok(StmtKind::Rumpe(RumpeStmt { span }))
    }

    /// Parse `perge` (loop continue).
    ///
    /// GRAMMAR:
    ///   perge-stmt := `perge`
    pub(super) fn parse_perge_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let span = self.current_span();
        self.expect_keyword_ident(TokenKind::Perge, "expected 'perge'")?;
        Ok(StmtKind::Perge(PergeStmt { span }))
    }

    /// Parse `cede value` as a yield statement.
    ///
    /// The AST keeps the historical `ExprKind::Cede` carrier so HIR lowering can
    /// produce the canonical `Yield` operation, but expression parsing no longer
    /// accepts `cede` in initializer, argument, or return-value positions.
    pub(super) fn parse_cede_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Cede, "expected 'cede'")?;
        let expr = self.parse_expression()?;
        let span = start.merge(expr.span);
        let id = self.next_id();
        Ok(StmtKind::Expr(ExprStmt {
            expr: Box::new(Expr {
                id,
                kind: ExprKind::Cede(CedeExpr {
                    expr: Box::new(expr),
                }),
                span,
            }),
        }))
    }

    /// Parse recoverable throw (`iace`).
    ///
    /// GRAMMAR:
    ///   iace-stmt := `iace` expr [`si` expr]
    ///
    /// The optional `si` guard is parser sugar. When a guard is present the
    /// parser desugars to a `si` block so downstream passes see a uniform
    /// conditional shape:
    ///
    ///   iace val          →  iace val               (unchanged)
    ///   iace val si cond  desugars to  si cond { iace val }
    pub(super) fn parse_iace_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Iace, "expected 'iace'")?;
        let value = Box::new(self.parse_expression()?);

        // Guard: only consume `si` when it directly follows the value
        // expression — no intervening newline trivia. A newline before
        // `si` means it is a new statement, not this statement's guard.
        if self.peek_raw_keyword_is("si") {
            self.advance();
            let guard = self.parse_expression()?;
            let inner_span = start.merge(value.span);
            let inner_id = self.next_id();
            let inner_stmt = Stmt {
                id: inner_id,
                kind: StmtKind::Iace(IaceStmt { value }),
                span: inner_span,
                annotations: Vec::new(),
                leading_trivia: Vec::new(),
            };
            let body = BlockStmt {
                statements: vec![inner_stmt],
                end_trivia: Vec::new(),
                span: inner_span,
            };
            Ok(StmtKind::Si(SiStmt {
                cond: Box::new(guard),
                then: IfBody::Block(body),
                catch: None,
                else_: None,
            }))
        } else {
            Ok(StmtKind::Iace(IaceStmt { value }))
        }
    }

    /// Parse unrecoverable panic (`mori`).
    ///
    /// GRAMMAR:
    ///   mori-stmt := `mori` expr [`si` expr]
    ///
    /// The optional `si` guard is parser sugar. When a guard is present the
    /// parser desugars to a `si` block:
    ///
    ///   mori `"msg"`          →  mori `"msg"`             (unchanged)
    ///   mori `"msg"` si cond  desugars to  si cond { mori `"msg"` }
    pub(super) fn parse_mori_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Mori, "expected 'mori'")?;
        let value = Box::new(self.parse_expression()?);

        // Guard: only consume `si` when it directly follows the value
        // expression — no intervening newline trivia.
        if self.peek_raw_keyword_is("si") {
            self.advance();
            let guard = self.parse_expression()?;
            let inner_span = start.merge(value.span);
            let inner_id = self.next_id();
            let inner_stmt = Stmt {
                id: inner_id,
                kind: StmtKind::Mori(MoriStmt { value }),
                span: inner_span,
                annotations: Vec::new(),
                leading_trivia: Vec::new(),
            };
            let body = BlockStmt {
                statements: vec![inner_stmt],
                end_trivia: Vec::new(),
                span: inner_span,
            };
            Ok(StmtKind::Si(SiStmt {
                cond: Box::new(guard),
                then: IfBody::Block(body),
                catch: None,
                else_: None,
            }))
        } else {
            Ok(StmtKind::Mori(MoriStmt { value }))
        }
    }

    /// Parse `tacet`.
    pub(super) fn parse_tacet_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let span = self.current_span();
        self.expect_keyword_ident(TokenKind::Tacet, "expected 'tacet'")?;
        Ok(StmtKind::Tacet(TacetStmt { span }))
    }

    // ERROR HANDLING

    /// Parse `adfirma`.
    ///
    /// GRAMMAR:
    ///   adfirma-stmt := `adfirma` expr ['mori' expr]
    ///
    /// `mori` (en `panic`) introduces the optional failure payload. Leftover
    /// `secus` / `else` on this statement is rejected — the particle names
    /// the action. A comma would collide with the grammar's homogeneous
    /// list separator.
    pub(super) fn parse_adfirma_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Adfirma, "expected 'adfirma'")?;

        let cond = Box::new(self.parse_expression()?);

        if self.check_keyword_ident(TokenKind::Secus) {
            return Err(self.expected_statement_error("adfirma_else_rejected"));
        }

        let message = if self.eat_keyword_ident(TokenKind::Mori) {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        Ok(StmtKind::Adfirma(AdfirmaStmt { cond, message }))
    }

    /// Parse `requirit` / `require`.
    ///
    /// GRAMMAR:
    ///   requirit-stmt := `requirit` expr 'iace' expr
    ///
    /// The `iace` particle (en `throw`) names the recoverable action. The
    /// error expression is mandatory — the guard always names its payload.
    /// Leftover `secus` / `else` is rejected. Lowering desugars the node to
    /// `si non (cond) { iace err }`.
    pub(super) fn parse_require_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Requirit, "expected 'requirit'")?;

        let cond = Box::new(self.parse_expression()?);

        if self.check_keyword_ident(TokenKind::Secus) {
            return Err(self.expected_statement_error("requirit_else_rejected"));
        }

        if !self.eat_keyword_ident(TokenKind::Iace) {
            return Err(self.expected_statement_error("requirit_iace"));
        }

        let error = Box::new(self.parse_expression()?);

        Ok(StmtKind::Require(RequireStmt { cond, error }))
    }

    // I/O AND DEBUGGING

    /// Parse `scribe` / `vide` / `mone` / `nota`.
    ///
    /// GRAMMAR:
    ///   scribe-stmt := (`scribe`|`vide`|`mone`|`nota`) expr (',' expr)*
    pub(super) fn parse_scribe_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let kind = if self.eat_keyword_ident(TokenKind::Scribe) {
            ScribeKind::Scribe
        } else if self.eat_keyword_ident(TokenKind::Vide) {
            ScribeKind::Vide
        } else if self.eat_keyword_ident(TokenKind::Mone) {
            ScribeKind::Mone
        } else if self.eat_keyword_ident(TokenKind::Nota) {
            ScribeKind::Nota
        } else {
            return Err(Self::expected_error("scribe", self.current_span()));
        };

        let mut args = Vec::new();
        loop {
            args.push(self.parse_expression()?);
            if !self.consume_item_separator_to_boundary("scribe_arguments")? {
                break;
            }
        }

        Ok(StmtKind::Scribe(ScribeStmt { kind, args }))
    }

    // SPECIAL STATEMENTS

    /// Parse `incipit` / `incipiet`.
    ///
    /// GRAMMAR:
    ///   incipit-stmt := (`incipit`|`incipiet`) [`argumenta` ident] [`exitus` expr] block
    ///
    /// `incipit` is the sync entry point; `incipiet` is the async form.
    pub(super) fn parse_incipit_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let is_async = if self.eat_keyword_ident(TokenKind::Incipit) {
            false
        } else if self.eat_keyword_ident(TokenKind::Incipiet) {
            true
        } else {
            return Err(Self::expected_error(
                "incipit or incipiet",
                self.current_span(),
            ));
        };

        let args = if self.eat_keyword_ident(TokenKind::Argumenta) {
            Some(self.parse_ident()?)
        } else {
            None
        };

        let exitus = if self.eat_keyword_ident(TokenKind::Exitus) {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        let body = IfBody::Block(self.parse_block()?);

        Ok(StmtKind::Incipit(IncipitStmt {
            is_async,
            body,
            args,
            exitus,
        }))
    }

    /// Parse `cura` allocator scope.
    ///
    /// GRAMMAR:
    ///   cura-stmt := `cura` STRING (`fixum`|`varia`) type ident block [`cape` ident block]
    ///
    /// The allocator kind is a validated string so strategy names do not become
    /// grammar words.
    pub(super) fn parse_cura_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Cura, "expected 'cura'")?;

        let kind_span = self.current_span();
        let kind_symbol = self.parse_string()?;
        let kind_text = self.interner.resolve(kind_symbol);
        let kind = match kind_text {
            "arena" => CuraKind::Arena,
            "page" => CuraKind::Page,
            _ => {
                return Err(
                    Self::expected_statement_error_at("allocator_kind", kind_span)
                        .with_arg("expected", "allocator kind \"arena\" or \"page\""),
                );
            }
        };

        let mutability = self.parse_mutability()?;

        let ty = self.parse_type()?;
        let binding = self.parse_ident()?;
        let body = self.parse_block()?;
        let catch = self.try_parse_cape_stmt()?;

        Ok(StmtKind::Cura(CuraStmt {
            kind,
            mutability,
            ty,
            binding,
            body,
            catch,
        }))
    }

    /// Statement-level `ad` is no longer supported; use expression-form `ad`.
    pub(super) fn parse_ad_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let keyword_span = self.current_span();
        self.expect_keyword_ident(TokenKind::Ad, "expected 'ad'")?;
        Err(Self::invalid_statement_error_at(
            keyword_span,
            "ad",
            "ad 'route' (opener) ↦ T expression form",
        ))
    }

    fn invalid_statement_error_at(
        span: Span,
        statement: &'static str,
        replacement: &'static str,
    ) -> ParseError {
        ParseError::structured(ParseErrorKind::StatementInvalid, span)
            .with_arg("issue", "ad_expression_only")
            .with_arg("statement", statement)
            .with_arg("replacement", replacement)
    }

    /// Parse `ex` destructuring.
    ///
    /// GRAMMAR:
    ///   ex-stmt := `ex` expr (`fixum`|`varia`) field (',' field)* [`ceteri` ident]
    ///   field := ident [`ut` ident]
    pub(super) fn parse_extract_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Ex, "expected 'ex'")?;

        let source = Box::new(self.parse_expression()?);

        let mutability = self.parse_mutability()?;

        let mut fields = Vec::new();
        let mut rest = None;

        loop {
            if self.eat_keyword_ident(TokenKind::Ceteri) {
                if rest.is_some() {
                    return Err(self.expected_statement_error("duplicate_rest_pattern"));
                }
                rest = Some(self.parse_ident()?);
                self.reject_item_list_comma_after_last("extract_fields")?;
                break;
            }

            let name = self.parse_ident()?;
            let alias = if self.eat_keyword_ident(TokenKind::Ut) {
                Some(self.parse_ident()?)
            } else {
                None
            };
            fields.push(ExField { name, alias });

            if !self.consume_item_separator_to_boundary("extract_fields")? {
                break;
            }
        }

        let span = start.merge(self.previous_span());
        Ok(StmtKind::Ex(ExStmt {
            source,
            mutability,
            fields,
            is_rest: rest,
            span,
        }))
    }

    /// Parse a bare expression as a statement, or postfix increment/decrement.
    ///
    /// GRAMMAR:
    ///   incDecStmt := place ('↑' | '↓')
    ///   exprStmt   := expression
    pub(super) fn parse_expr_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let expr = self.parse_expression()?;
        match self.peek().kind {
            TokenKind::PostInc => {
                self.advance();
                Ok(StmtKind::IncDec(IncDecStmt {
                    target: Box::new(expr),
                    op: IncDecOp::Inc,
                }))
            }
            TokenKind::PostDec => {
                self.advance();
                Ok(StmtKind::IncDec(IncDecStmt {
                    target: Box::new(expr),
                    op: IncDecOp::Dec,
                }))
            }
            _ => Ok(StmtKind::Expr(ExprStmt {
                expr: Box::new(expr),
            })),
        }
    }

    /// Parse the required `fixum` / `varia` mutability marker shared by binding statements.
    pub(super) fn parse_mutability(&mut self) -> Result<AstMutability, ParseError> {
        if self.eat_keyword_ident(TokenKind::Fixum) {
            Ok(AstMutability::Immutable)
        } else if self.eat_keyword_ident(TokenKind::Varia) {
            Ok(AstMutability::Mutable)
        } else {
            Err(self
                .expected_statement_error("mutability_marker")
                .with_arg("expected", "'fixum' or 'varia'"))
        }
    }

    fn expected_statement_error(&self, issue: &'static str) -> ParseError {
        Self::expected_statement_error_at(issue, self.current_span())
    }

    fn expected_statement_error_at(issue: &'static str, span: Span) -> ParseError {
        ParseError::structured(ParseErrorKind::Expected, span).with_arg("issue", issue)
    }

    /// Try to parse an optional `cape` clause.
    ///
    /// GRAMMAR:
    ///   cape-clause := `cape` ident block
    ///
    /// Centralizing this grammar keeps catch binding and body spans consistent
    /// across branches, loops, resource scopes, and `fac`.
    fn try_parse_cape_stmt(&mut self) -> Result<Option<CapeClause>, ParseError> {
        if !self.eat_keyword_ident(TokenKind::Cape) {
            return Ok(None);
        }

        let start = self.current_span();
        let binding = self.parse_ident()?;
        let body = self.parse_block()?;
        let span = start.merge(self.previous_span());

        Ok(Some(CapeClause {
            binding,
            body,
            span,
        }))
    }
}
