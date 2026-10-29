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

/// Retired allocator-scope statement keyword (DECISIONS D8.2).
pub(super) const RETIRED_CURA: &str = "cura";
/// Retired allocator-requirement function modifier (DECISIONS D8.2).
pub(super) const RETIRED_CURATA: &str = "curata";

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
        self.expect_keyword_ident(TokenKind::Si)?;
        Ok(StmtKind::Si(self.parse_si_tail()?))
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
                self.expect_keyword_ident(TokenKind::Sin)?;
                Some(SecusClause::Sin {
                    stmt: Box::new(self.parse_si_tail()?),
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
        self.expect_keyword_ident(TokenKind::Dum)?;

        let cond = Box::new(self.parse_expression()?);
        let body = self.parse_ergo_body()?;
        let catch = self.try_parse_cape_stmt()?;

        Ok(StmtKind::Dum(DumStmt { cond, body, catch }))
    }

    /// Parse an `itera` loop and its ownership mode.
    ///
    /// GRAMMAR:
    ///   itera-stmt := `itera` ((`ex`|`ab`) expr (`,` expr)* | `de` expr) [apud-clause] [filum-clause] (`fixum`|`varia`) itera-binding if-body [`cape` ident block]
    ///   apud-clause := `apud` `[` IDENTIFIER (`,` IDENTIFIER)* `]`
    ///   filum-clause := `filum` IDENTIFIER
    ///   itera-binding := array-pattern | object-pattern | IDENTIFIER (`,` IDENTIFIER)*
    ///
    /// The parser records the source preposition mode; later phases validate
    /// the semantic source shape for that mode. `ex` and `ab` admit the
    /// comma-separated source series (`ex` lockstep zip, `ab` Cartesian range
    /// product); `de` stays unary. Source/binder arity and range membership
    /// are semantic laws, not parse decisions.
    pub(super) fn parse_itera_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Itera)?;

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

        // The loop binder `fixum name {` is not a sort key body: bar the sort
        // expression's body at the source's own expression depth.
        let previous_sort_bar = self.sort_body_barred_at;
        self.sort_body_barred_at = Some(self.expression_contexts.saturating_add(1));
        let sources = self.parse_itera_sources(mode);
        self.sort_body_barred_at = previous_sort_bar;
        let sources = sources?;
        // A bare sort or reduce expression is never a loop source: bind it
        // to a name first (operator ruling 2026-10-10, need).
        // Parenthesised expressions stay loop sources; only the direct
        // expression kind is refused. The loop still parses so one file
        // reports every site.
        for source in &sources {
            if matches!(source.kind, ExprKind::Sort(_) | ExprKind::Reducta(_)) {
                self.errors.push(
                    ParseError::structured(ParseErrorKind::ExpressionInvalid, source.span)
                        .with_arg("issue", "itera_source_sort_reduce_unbound"),
                );
            }
        }
        let apud = if self.eat_keyword_ident(TokenKind::Apud) {
            self.parse_apud_clause()?
        } else {
            Vec::new()
        };
        // The lane-binder clause rides the shared itera head, in the same
        // position as `summa`'s (`filum` is claimed by registry identity
        // only here). The binder is declared metadata; lowering owns what
        // it binds and the runner lowers the plain loop.
        let filum = if matches!(
            self.peek().kind,
            TokenKind::Ident(sym) if self.keyword_claim_key(sym) == Some("filum")
        ) {
            self.advance();
            Some(self.parse_ident()?)
        } else {
            None
        };

        let mutability_span = self.current_span();
        let mutability = self.parse_binder_mutability()?;
        // HIR's itera node does not retain the source mutability marker, so
        // reject this mode conflict while the parser still has that fact.
        if matches!(mode, IteraMode::De) && matches!(mutability, AstMutability::Mutable) {
            return Err(
                ParseError::structured(ParseErrorKind::Expected, mutability_span)
                    .with_arg("issue", "itera_de_requires_fixum")
                    .with_arg("expected", "the fixed form `sit`"),
            );
        }
        // A name binder may continue through commas (`itera_binding`'s
        // IDENTIFIER series); pattern binders and the legacy `_` loop
        // variable close the head. The token decides before parsing.
        let binder_is_name = matches!(self.peek().kind, TokenKind::Ident(_));
        // A top-level `_` in an itera binder is the legacy unused loop
        // variable, not a destructuring wildcard. Keep it as an identifier;
        // wildcard patterns remain available inside array/object binders.
        let binding = if matches!(self.peek().kind, TokenKind::Underscore(_)) {
            BindingPattern::Ident(self.parse_ident()?)
        } else {
            self.parse_binding_pattern()?
        };
        let mut bindings = vec![binding];
        if binder_is_name {
            while self.eat(&TokenKind::Comma) {
                bindings.push(BindingPattern::Ident(self.parse_ident()?));
            }
        }
        let body = self.parse_ergo_body()?;
        let catch = self.try_parse_cape_stmt()?;

        Ok(StmtKind::Itera(IteraStmt {
            mode,
            sources,
            apud,
            filum,
            mutability,
            bindings,
            body,
            catch,
        }))
    }

    /// Parse the comma-separated source list of an `itera` head.
    fn parse_itera_sources(&mut self, mode: IteraMode) -> Result<Vec<Expr>, ParseError> {
        let mut sources = vec![self.parse_expression()?];
        if matches!(mode, IteraMode::Ex | IteraMode::Ab) {
            while self.eat(&TokenKind::Comma) {
                sources.push(self.parse_expression()?);
            }
        }
        Ok(sources)
    }

    /// Parse the bracketed coordinate names claimed by an `apud` clause.
    pub(super) fn parse_apud_clause(&mut self) -> Result<Vec<Ident>, ParseError> {
        self.expect_issue(&TokenKind::LBracket, "apud_coordinates_start")?;

        let mut coordinates = vec![self.parse_ident()?];
        while self.eat(&TokenKind::Comma) {
            coordinates.push(self.parse_ident()?);
        }
        self.expect_issue(&TokenKind::RBracket, "apud_coordinates_end")?;
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
        self.expect_keyword_ident(TokenKind::Elige)?;

        let expr = Box::new(self.parse_expression()?);

        self.expect_issue(&TokenKind::LBrace, "elige_body_start")?;

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

        self.expect_issue(&TokenKind::RBrace, "elige_body_end")?;

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
    ///   discerne-stmt := `discerne` [`omnia`] subject-path (`et` subject-path)* '{' arm* [`default`] '}'
    ///   subject-path := ident ('.' ident)*
    ///   arm := `casu` pattern (`et` pattern)* if-body
    ///
    /// `omnia` is the exhaustive marker only where a subject path follows it
    /// (`discerne omnia a et b`); before `{`, `.` or the separator it is the
    /// first subject, because keywords are contextual and a binding may be
    /// named like one. `omnia` is only recorded here; later phases own the
    /// exhaustive-coverage check because they know the subject types and
    /// variant sets. A comma between subjects, and a subject that is not a
    /// path, are named migration diagnostics.
    pub(super) fn parse_discerne_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Discerne)?;

        let exhaustive = self.eat_discerne_exhaustive_marker();
        let subjects = self.parse_discerne_subjects()?;

        self.expect_issue(&TokenKind::LBrace, "discerne_body_start")?;

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

        self.expect_issue(&TokenKind::RBrace, "discerne_body_end")?;

        Ok(StmtKind::Discerne(DiscerneStmt {
            exhaustive,
            subjects,
            arms,
            default,
        }))
    }

    /// Eat the exhaustive marker only when a subject path follows it (D-10).
    fn eat_discerne_exhaustive_marker(&mut self) -> bool {
        if !self.check_keyword_ident(TokenKind::Omnia) {
            return false;
        }
        let subject_follows = matches!(self.peek_at(1).kind, TokenKind::Ident(_) | TokenKind::Tag)
            && self.keyword_ident_at(1) != Some(TokenKind::Et);
        if subject_follows {
            self.advance();
        }
        subject_follows
    }

    /// Parse the `et`-separated subject list of a `discerne` header.
    ///
    /// A comma is the retired separator: it is reported once for the header
    /// and read as `et`, so the rest of the header parses without a cascade.
    fn parse_discerne_subjects(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut subjects = Vec::new();
        let mut comma_reported = false;
        loop {
            subjects.push(self.parse_discerne_subject()?);
            if self.eat_keyword_ident(TokenKind::Et) {
                continue;
            }
            if self.eat(&TokenKind::Comma) {
                if !comma_reported {
                    comma_reported = true;
                    self.errors.push(
                        ParseError::structured(
                            ParseErrorKind::StatementInvalid,
                            self.previous_span(),
                        )
                        .with_arg("issue", "discerne_subject_comma_retired"),
                    );
                }
                if self.check(&TokenKind::LBrace) {
                    break;
                }
                continue;
            }
            break;
        }
        Ok(subjects)
    }

    /// Parse one subject: a name or member path (`ego` included), never an
    /// expression. Anything else is parsed as an expression for recovery and
    /// reported as `discerne_subject_not_path`.
    fn parse_discerne_subject(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let checkpoint = self.checkpoint();
        if self.at_subject_path_start() {
            let path = self.parse_subject_path()?;
            if self.check(&TokenKind::LBrace)
                || self.check(&TokenKind::Comma)
                || self.check_keyword_ident(TokenKind::Et)
            {
                return Ok(path);
            }
            self.restore(checkpoint);
        }
        let expr = self.parse_expression()?;
        self.errors.push(
            ParseError::structured(ParseErrorKind::StatementInvalid, start.merge(expr.span))
                .with_arg("issue", "discerne_subject_not_path"),
        );
        Ok(expr)
    }

    /// Whether the current word can begin a subject path. The null and bool
    /// keyword literals are values, not names.
    fn at_subject_path_start(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::Tag)
            && !self.check_keyword_ident(TokenKind::Verum)
            && !self.check_keyword_ident(TokenKind::Falsum)
            && !self.check_keyword_ident(TokenKind::Nihil)
    }

    /// Parse `ident ('.' ident)*` into the expression kinds the expression
    /// parser builds for the same text.
    fn parse_subject_path(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let mut path = if self.check_keyword_ident(TokenKind::Ego) {
            let span = self.peek().span;
            self.advance();
            Expr {
                id: self.next_id(),
                kind: ExprKind::Ego(span),
                span,
            }
        } else {
            let ident = self.parse_ident()?;
            Expr {
                id: self.next_id(),
                kind: ExprKind::Ident(ident),
                span: start.merge(self.previous_span()),
            }
        };
        while self.check(&TokenKind::Dot) {
            self.advance();
            let member = self.parse_member_ident()?;
            path = Expr {
                id: self.next_id(),
                kind: ExprKind::Member(MemberExpr {
                    object: Box::new(path),
                    member,
                }),
                span: start.merge(self.previous_span()),
            };
        }
        Ok(path)
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
        self.expect_keyword_ident(TokenKind::Custodi)?;
        self.expect_issue(&TokenKind::LBrace, "custodi_body_start")?;

        let mut clauses = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            self.expect_keyword_ident(TokenKind::Si)?;
            let start = self.current_span();
            let cond = Box::new(self.parse_expression()?);
            let body = self.parse_ergo_body()?;
            let span = start.merge(self.previous_span());
            clauses.push(CustodiClause { cond, body, span });
        }

        self.expect_issue(&TokenKind::RBrace, "custodi_body_end")?;

        Ok(StmtKind::Custodi(CustodiStmt { clauses }))
    }

    /// Parse `fac`, the canonical block form for do/try-style execution.
    ///
    /// GRAMMAR:
    ///   fac-stmt := `fac` [`omnia` | `machina`] block [`cape` ident block] [`dum` expr]
    ///
    /// At statement level, a trailing `dum` turns `fac` into a do-while form.
    /// Closure bodies call the shared construct with `allow_while = false` so
    /// expression syntax cannot smuggle in a statement loop.
    pub(super) fn parse_fac_stmt(&mut self) -> Result<StmtKind, ParseError> {
        Ok(StmtKind::Fac(self.parse_fac_construct(true)?))
    }

    pub(super) fn parse_fac_construct(&mut self, allow_while: bool) -> Result<FacStmt, ParseError> {
        self.expect_keyword_ident(TokenKind::Fac)?;

        let modifier = if self.eat_keyword_ident(TokenKind::Omnia) {
            FacModifier::Omnia
        } else if self.eat_keyword_ident(TokenKind::Machina) {
            FacModifier::Machina
        } else {
            FacModifier::None
        };
        if modifier != FacModifier::None
            && (self.check_keyword_ident(TokenKind::Omnia)
                || self.check_keyword_ident(TokenKind::Machina))
        {
            // `do all device` and `do device all` (and a repeat): `device`
            // already delivers every fault once at the block's exit.
            return Err(ParseError::structured(
                ParseErrorKind::StatementInvalid,
                self.current_span(),
            )
            .with_arg("issue", "fac_modifier_redundant"));
        }
        let body = self.parse_block()?;
        let catch = self.try_parse_cape_stmt()?;

        let while_ = if self.eat_keyword_ident(TokenKind::Dum) {
            if !allow_while {
                return Err(self.invalid_expression_error("closure_fac_body_dum"));
            }
            let cond = self.parse_expression()?;
            // Grammar order is `fac block cape_clause? (dum expression)?`: a
            // handler after the condition would otherwise re-enter statement
            // parsing as bare identifiers (`cape`, `err`) and surface as
            // misleading unknown-name errors.
            if self.check_keyword_ident(TokenKind::Cape) {
                return Err(ParseError::structured(
                    ParseErrorKind::StatementInvalid,
                    self.current_span(),
                )
                .with_arg("issue", "fac_cape_after_dum"));
            }
            Some(Box::new(cond))
        } else {
            None
        };

        Ok(FacStmt {
            modifier,
            body,
            catch,
            while_,
        })
    }

    // TRANSFER STATEMENTS

    /// Parse `redde` or `redde filum`.
    ///
    /// GRAMMAR:
    ///   redde-stmt := `redde` [`expr`]
    ///   redde-filum-stmt := `redde` `filum` [`expr`]
    ///
    /// The value must start on the statement's line. A newline ends a bare
    /// return (or a bare thread return), so a following statement cannot be
    /// swallowed as its payload.
    pub(super) fn parse_redde_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Redde)?;

        if !self.raw_newline_before_next_word()
            && matches!(
                self.peek().kind,
                TokenKind::Ident(sym) if self.keyword_claim_key(sym) == Some("filum")
            )
        {
            self.advance();
            let value = if !self.is_at_end()
                && !self.check(&TokenKind::RBrace)
                && !self.check(&TokenKind::Semicolon)
                && !self.raw_newline_before_next_word()
            {
                Some(Box::new(self.parse_expression()?))
            } else {
                None
            };
            return Ok(StmtKind::ReddeFilum(ReddeFilumStmt { value }));
        }

        let value = if !self.is_at_end()
            && !self.check(&TokenKind::RBrace)
            && !self.check(&TokenKind::Semicolon)
            && !self.raw_newline_before_next_word()
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
        self.expect_keyword_ident(TokenKind::Rumpe)?;
        Ok(StmtKind::Rumpe(RumpeStmt { span }))
    }

    /// Parse `perge` (loop continue).
    ///
    /// GRAMMAR:
    ///   perge-stmt := `perge`
    pub(super) fn parse_perge_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let span = self.current_span();
        self.expect_keyword_ident(TokenKind::Perge)?;
        Ok(StmtKind::Perge(PergeStmt { span }))
    }

    /// Parse `cede value` as a yield statement.
    ///
    /// The AST keeps the historical `ExprKind::Cede` carrier so HIR lowering can
    /// produce the canonical `Yield` operation, but expression parsing no longer
    /// accepts `cede` in initializer, argument, or return-value positions.
    pub(super) fn parse_cede_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Cede)?;
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
        self.expect_keyword_ident(TokenKind::Iace)?;
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
        self.expect_keyword_ident(TokenKind::Mori)?;
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
        self.expect_keyword_ident(TokenKind::Tacet)?;
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
    /// list separator. A string literal on the same line right after the
    /// condition is a message missing its `mori` particle; it is rejected
    /// rather than parsed as a separate statement that silently drops it.
    pub(super) fn parse_adfirma_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Adfirma)?;

        let cond = Box::new(self.parse_expression()?);

        if self.check_keyword_ident(TokenKind::Secus) {
            return Err(self.expected_statement_error("adfirma_else_rejected"));
        }

        if !self.raw_newline_before_next_word()
            && matches!(
                self.peek().kind,
                TokenKind::String(_) | TokenKind::AsciiString(_) | TokenKind::BacktickString(_)
            )
        {
            return Err(self.expected_statement_error("adfirma_message_particle"));
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
        self.expect_keyword_ident(TokenKind::Requirit)?;

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

    /// Parse `reice` / `reject`.
    ///
    /// GRAMMAR:
    ///   reice-stmt := `reice` expr 'iace' expr
    ///
    /// The boolean opposite of `requirit`: fail when the condition holds.
    /// Same `iace` particle and mandatory error payload as `requirit`.
    /// Lowering desugars the node to `si (cond) { iace err }`.
    pub(super) fn parse_reject_stmt(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Reice)?;

        let cond = Box::new(self.parse_expression()?);

        if self.check_keyword_ident(TokenKind::Secus) {
            return Err(self.expected_statement_error("reice_else_rejected"));
        }

        if !self.eat_keyword_ident(TokenKind::Iace) {
            return Err(self.expected_statement_error("reice_iace"));
        }

        let error = Box::new(self.parse_expression()?);

        Ok(StmtKind::Reject(RejectStmt { cond, error }))
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
            return Err(self.expected_issue_with_keywords(
                "expected_scribe_keyword",
                self.current_span(),
                &["scribe", "vide", "mone", "nota"],
            ));
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
            return Err(self.expected_issue_with_keywords(
                "expected_incipit_keyword",
                self.current_span(),
                &["incipit", "incipiet"],
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

    /// Whether the current word opens the retired `cura` allocator scope.
    ///
    /// `cura` was removed (DECISIONS D8.2). The old statement shape was
    /// `cura STRING …`; only that shape is diagnosed, so `cura` stays an
    /// ordinary identifier everywhere else. Matching the canonical Latin
    /// spelling is deliberate: the keyword no longer has pack rows.
    pub(super) fn at_retired_cura_stmt(&self) -> bool {
        self.current_ident_is(RETIRED_CURA) && matches!(self.peek_at(1).kind, TokenKind::String(_))
    }

    /// Whether the current word is the retired `curata` function modifier.
    ///
    /// The old shape was `curata IDENT [ut IDENT]` after the parameter list,
    /// where no other identifier is legal.
    pub(super) fn at_retired_curata_modifier(&self) -> bool {
        self.current_ident_is(RETIRED_CURATA) && matches!(self.peek_at(1).kind, TokenKind::Ident(_))
    }

    pub(super) fn current_ident_is(&self, text: &str) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(sym) if self.interner.resolve(sym) == text)
    }

    /// Migration diagnostic for the removed allocator surface (`cura`/`curata`).
    pub(super) fn allocator_surface_removed_error(&self, keyword: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::Unexpected, self.current_span())
            .with_arg("issue", "allocator_surface_removed")
            .with_arg("keyword", keyword)
    }

    /// Statement-level `ad` is not a valid statement form; use expression-form `ad`.
    pub(super) fn parse_ad_stmt(&mut self) -> Result<StmtKind, ParseError> {
        let keyword_span = self.current_span();
        self.expect_keyword_ident(TokenKind::Ad)?;
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
        self.expect_keyword_ident(TokenKind::Ex)?;

        let source = Box::new(self.parse_expression()?);

        let mutability = self.parse_binder_mutability()?;

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

    /// Whether the cursor holds a binder-introducing word: `sit`/`varia`
    /// (en `let`/`mut`), or one of the two refused spellings (`fixum`, `de`).
    pub(super) fn binder_head_word_at(&self, offset: usize) -> bool {
        matches!(
            self.keyword_ident_at(offset),
            Some(TokenKind::Sit | TokenKind::Varia | TokenKind::Fixum | TokenKind::De)
        )
    }

    /// Parse the required binder word shared by iteration binders, `ex`
    /// destructuring, pattern bindings, and sort/reduce bodies.
    ///
    /// GRAMMAR:
    ///   binder-word := `sit` | `varia`          (en `let` | `mut`)
    ///
    /// A binder names a value per iteration or match, so it is a write-once
    /// (`sit`) or re-assignable (`varia`) name. The retired `fixum` binder is
    /// `PARSE010 binder_fixum_retired` and `de` (`ref`) is `PARSE010
    /// ref_binder_refused`; both are recorded and read as an immutable binder
    /// so recovery resumes after the head.
    pub(super) fn parse_binder_mutability(&mut self) -> Result<AstMutability, ParseError> {
        if self.eat_keyword_ident(TokenKind::Sit) {
            return Ok(AstMutability::Immutable);
        }
        if self.eat_keyword_ident(TokenKind::Varia) {
            return Ok(AstMutability::Mutable);
        }
        let span = self.current_span();
        let issue = match self.current_keyword_ident() {
            Some(TokenKind::Fixum) => "binder_fixum_retired",
            Some(TokenKind::De) => "ref_binder_refused",
            _ => {
                let expected = format!(
                    "'{}' or '{}'",
                    self.keyword_surface("sit"),
                    self.keyword_surface("varia")
                );
                return Err(self
                    .expected_statement_error("mutability_marker")
                    .with_arg("expected", expected));
            }
        };
        let keyword = self.current_word();
        self.advance();
        self.errors.push(
            ParseError::structured(ParseErrorKind::DeclarationInvalid, span)
                .with_arg("issue", issue)
                .with_arg("keyword", keyword),
        );
        Ok(AstMutability::Immutable)
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
