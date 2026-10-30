//! Declaration grammar for Faber's crate-facing syntax tree.
//!
//! This module owns the parser surface for declarations and declaration-adjacent
//! blocks: bindings, functions, class/interface shapes, algebraic data types,
//! imports, tests, annotations, and ordinary block bodies. It is the
//! main grammar boundary between source-level declarations and `AST` contracts
//! consumed by the collector and package CLI.
//!
//! Declarations are intentionally explicit: types precede names, inferred locals
//! require `_`, voluntary slots use post-name `sponte`, and canonical modifier
//! ordering is enforced in the parser when later phases would otherwise receive
//! ambiguous ASTs. Where legacy or transitional syntax is still recognized, the
//! parser either normalizes it into the current `AST` contract or emits a targeted
//! diagnostic instead of expanding the language silently.
//!
//! - `parse_stmt` is the dispatcher for both top-level and nested statement
//!   contexts; recovery boundaries in `Parser::is_recovery_boundary` must stay
//!   aligned with it.
//! - Function, parameter, field, and closure signatures delegate to `types.rs`
//!   for type syntax; this file decides where type positions are legal.
//! - Annotation parsers define the source contract for CLI/package metadata,
//!   while unknown annotations remain statement annotations for later phases.

use super::{ParseError, ParseErrorKind, Parser};
use radix_lexer::{
    KeywordKind, KeywordOwner, KeywordScope, Span, TokenKind, keyword_allowed_as_ident,
    keyword_spelling_for_token, lookup_keyword_spec,
};
#[allow(clippy::wildcard_imports)]
use radix_syntax::*;

/// Retired abstract-genus modifier (DECISIONS D8.1).
pub(super) const RETIRED_ABSTRACTUS: &str = "abstractus";
/// Retired genus `extends` clause keyword (DECISIONS D8.1).
const RETIRED_SUB: &str = "sub";

/// Keyword identities of the five braced import record field names, in
/// grammar order: source, visibility, binding name, alias, wildcard alias.
const IMPORT_FIELD_KEYS: &[&str] = &["ex", "visibilitas", "nomen", "ut", "omnia"];

/// Whether a parsed statement is a declaration that may appear only at top
/// level (DECISIONS D22.1): a `functio` or a type declaration. Methods live in
/// `genus` bodies, which parse through their own member loop, not a block.
fn is_top_level_only_declaration(kind: &StmtKind) -> bool {
    matches!(
        kind,
        StmtKind::Func(_)
            | StmtKind::Class(_)
            | StmtKind::Interface(_)
            | StmtKind::TypeAlias(_)
            | StmtKind::Schema(_)
            | StmtKind::Enum(_)
            | StmtKind::Union(_)
    )
}

// STATEMENT DISPATCH

/// The kernel mode parsed from a kernel head's signature slot (operator
/// ruling 2026-10-30): the fragment flag and tier the `@ nucleum` form
/// writes as annotation words, plus the span of those words (none when the
/// default mode is unwritten — the default tier has no word).
#[derive(Default)]
struct ParsedKernelMode {
    is_fragment: bool,
    tier: KernelTier,
    mode_span: Option<Span>,
}

/// Former spellings of the writable-local word, now ordinary identifiers: en
/// `var` and the six translated packs' old `varia` rows.
const RETIRED_VAR_WORDS: &[&str] = &["var", "متغير", "चर", "แปร", "biến", "变量", "變值"];

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
        // The kernel declaration head synthesizes its nucleum annotation
        // during the dispatch below; the statement carries it where the
        // annotation form's marker rides.
        let mut head_annotation: Option<Annotation> = None;

        let kind = match self.peek().kind {
            TokenKind::LBrace => StmtKind::Block(self.parse_block()?),

            // Keyword identity is locale-independent and claimed only in
            // statement-initial position.
            TokenKind::Ident(_) => match self.current_keyword_ident() {
                Some(TokenKind::Discretio) => self.parse_union_decl()?,
                Some(TokenKind::Fixum | TokenKind::Varia) => self.parse_var_decl()?,
                // The retired spelling of the writable local (en `var`, and each
                // translated pack's former `varia` word): diagnosed, then parsed
                // as the `mut` declaration it was.
                None if self.at_retired_var_decl() => self.parse_retired_var_decl()?,
                // `ref T name ← place`: a read-only-through, re-pointable
                // local. Claimed only when a type word follows; a lone `ref`
                // stays an ordinary identifier.
                Some(TokenKind::De) if self.at_ref_local_decl() => self.parse_ref_decl()?,
                Some(TokenKind::Magnitudo) => {
                    let suggestion = match self.peek_at(1).kind {
                        TokenKind::Ident(symbol) => {
                            format!("const size {}", self.interner.resolve(symbol))
                        }
                        _ => "const size".to_owned(),
                    };
                    return Err(
                        ParseError::structured(ParseErrorKind::DeclarationInvalid, start)
                            .with_arg("issue", "size_requires_const")
                            .with_arg("suggestion", suggestion),
                    );
                }
                // `generis` is a genus-member modifier. At statement start the
                // retired module-level static shape (modifier + type + name)
                // is diagnosed; any other use stays an ordinary identifier.
                Some(TokenKind::Generis) if self.at_retired_static_decl() => {
                    self.parse_retired_static_decl()?
                }
                Some(TokenKind::Functio) => self.parse_func_decl()?,
                // The kernel declaration head (operator ruling 2026-10-30):
                // `nucleum name<T>(params) [mode] …`. Claimed only when a
                // declaration name follows, so an ordinary `kernel(…)` call
                // or identifier use stays expression material.
                Some(TokenKind::Nucleum) if self.at_kernel_decl_head() => {
                    let (func, head) = self.parse_kernel_decl()?;
                    head_annotation = Some(head);
                    StmtKind::Func(func)
                }
                Some(TokenKind::Genus) => self.parse_class_decl()?,
                Some(TokenKind::Implendum) => self.parse_interface_decl()?,
                Some(TokenKind::Ordo) => self.parse_enum_decl()?,
                Some(TokenKind::Sit) => self.parse_sit_decl()?,
                Some(TokenKind::Typus) => self.parse_type_alias_decl()?,
                Some(TokenKind::Schema) => self.parse_schema_decl()?,
                Some(TokenKind::Importa) => self.parse_import_decl()?,
                Some(TokenKind::Proba) => StmtKind::Proba(self.parse_proba_case()?),
                Some(TokenKind::Probandum) => self.parse_probandum_decl()?,
                Some(TokenKind::Custodi) => self.parse_custodi_stmt()?,
                Some(TokenKind::Itera) => self.parse_itera_stmt()?,
                Some(TokenKind::Iace) => self.parse_iace_stmt()?,
                Some(TokenKind::Mori) => self.parse_mori_stmt()?,
                Some(TokenKind::Cede) => self.parse_cede_stmt()?,
                Some(TokenKind::Adfirma) => self.parse_adfirma_stmt()?,
                Some(TokenKind::Requirit) => self.parse_require_stmt()?,
                Some(TokenKind::Reice) => self.parse_reject_stmt()?,
                // Scribe-family keywords claim statement-initial position only
                // when not immediately followed by `(`. `nota 5` is the output
                // statement; `nota (5)` falls through as a user call.
                Some(TokenKind::Scribe | TokenKind::Nota | TokenKind::Mone | TokenKind::Vide)
                    if self.peek_at(1).kind != TokenKind::LParen =>
                {
                    self.parse_scribe_stmt()?
                }
                Some(TokenKind::Incipit | TokenKind::Incipiet) => self.parse_incipit_stmt()?,
                Some(TokenKind::Ex) => self.parse_extract_stmt()?,
                Some(TokenKind::Discerne) => self.parse_discerne_stmt()?,
                Some(TokenKind::Dum) => self.parse_dum_stmt()?,
                Some(TokenKind::Elige) => self.parse_elige_stmt()?,
                Some(TokenKind::Fac) => self.parse_fac_stmt()?,
                Some(TokenKind::Si) => self.parse_si_stmt()?,
                Some(TokenKind::Perge) => self.parse_perge_stmt()?,
                Some(TokenKind::Redde) => self.parse_redde_stmt()?,
                Some(TokenKind::Rumpe) => self.parse_rumpe_stmt()?,
                Some(TokenKind::Tacet) => self.parse_tacet_stmt()?,
                Some(TokenKind::Ad) => self.parse_ad_stmt()?,
                Some(TokenKind::Figendum | TokenKind::Variandum) => {
                    let is_immutable = self.current_keyword_ident() == Some(TokenKind::Figendum);
                    self.parse_await_var_decl(if is_immutable {
                        AstMutability::Immutable
                    } else {
                        AstMutability::Mutable
                    })?
                }
                Some(TokenKind::Reddet) => self.parse_reddet_stmt(start)?,
                Some(TokenKind::Tacebit) => self.parse_tacebit_stmt(start)?,
                None if self.at_retired_abstractus_decl() => {
                    return Err(self.inheritance_removed_error(RETIRED_ABSTRACTUS));
                }
                None if self.at_retired_cura_stmt() => {
                    return Err(self.allocator_surface_removed_error(super::stmt::RETIRED_CURA));
                }
                _ => self.parse_expr_stmt()?,
            },

            TokenKind::At => return self.parse_annotated_stmt(leading_trivia),

            _ => self.parse_expr_stmt()?,
        };

        let span = start.merge(self.previous_span());
        Ok(Stmt {
            id,
            kind,
            span,
            annotations: head_annotation.into_iter().collect(),
            leading_trivia,
        })
    }

    fn parse_annotated_stmt(&mut self, leading_trivia: Vec<Trivia>) -> Result<Stmt, ParseError> {
        let annotations = self.parse_annotations()?;
        self.nondum_function_pending =
            (self.check_keyword_ident(TokenKind::Functio) || self.at_kernel_decl_head())
                && annotations
                    .iter()
                    .any(|annotation| self.is_nondum_annotation(annotation));
        let parsed = self.parse_stmt();
        self.nondum_function_pending = false;
        let mut stmt = parsed?;
        if !matches!(stmt.kind, StmtKind::Func(_)) {
            self.check_nucleum_tier_target(&annotations);
        }
        // A kernel head under a stack of authored annotations reconciles
        // with them here: both heads on one declaration is refused by name,
        // and the head synthesizes its marker at the front of the stack so
        // both spellings lower to the same annotation sequence.
        let head_annotation = std::mem::take(&mut stmt.annotations);
        stmt.annotations = match head_annotation.into_iter().next() {
            Some(head) => self.merge_kernel_head_annotation(annotations, head),
            None => annotations,
        };
        let mut combined = leading_trivia;
        combined.append(&mut stmt.leading_trivia);
        stmt.leading_trivia = combined;
        Ok(stmt)
    }

    // VARIABLE DECLARATIONS

    /// Parse `fixum` and `varia` binding declarations.
    ///
    /// GRAMMAR:
    ///   var-decl := (`fixum` | `varia`) type pattern ['←' expr | '↤' expr | '↢' expr]
    ///             | `fixum` type ident '=' expr
    ///
    /// The `=` form is a local compile-time constant (DECISIONS D5.10).
    ///
    /// Simple bindings carry an explicit type slot, including `_` for inference.
    /// Destructuring keeps its historical untyped shape because the binding
    /// pattern itself owns the names and any rest element.
    ///
    /// `↤` initializers (conversion-directed assignment) are accepted only when
    /// the written type is concrete: the destination type must name a
    /// conversion target before the right side is converted. `fixum _`,
    /// `sit`, and untyped destructuring have no concrete destination and are
    /// rejected with a structured diagnostic.
    pub(super) fn parse_var_decl(&mut self) -> Result<StmtKind, ParseError> {
        let keyword = self.current_word();
        let keyword_span = self.current_span();
        let mutability = if self.eat_keyword_ident(TokenKind::Fixum) {
            AstMutability::Immutable
        } else if self.eat_keyword_ident(TokenKind::Varia) {
            AstMutability::Mutable
        } else {
            return Err(self.expected_issue_with_keywords(
                "expected_mutability_keyword",
                self.current_span(),
                &["fixum", "varia"],
            ));
        };

        if self.check_keyword_ident(TokenKind::Magnitudo) {
            return self.parse_size_decl(mutability);
        }

        let stmt = self.parse_var_decl_rest(mutability, keyword.clone(), true)?;
        // A run-time `fixum` of a simple binding is the retired spelling of
        // typed `sit` (`PARSE010 runtime_const_retired`): `fixum` states a
        // compile-time fact and goes with `=`. The declaration is parsed whole
        // so recovery resumes after it. Destructuring has no `sit` form yet and
        // keeps `fixum`.
        if mutability == AstMutability::Immutable
            && let StmtKind::Var(decl) = &stmt
            && !decl.compile_time
            && decl
                .await_form
                .is_none_or(|form| form == AwaitBindingForm::Assignment)
            && matches!(
                decl.binding,
                BindingPattern::Ident(_) | BindingPattern::Wildcard(_)
            )
        {
            let name = match &decl.binding {
                BindingPattern::Ident(ident) => self.interner.resolve(ident.name).to_owned(),
                _ => "_".to_owned(),
            };
            self.errors.push(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, keyword_span)
                    .with_arg("issue", "runtime_const_retired")
                    .with_arg("keyword", keyword)
                    .with_arg("name", name),
            );
        } else if mutability == AstMutability::Immutable
            && let StmtKind::Var(decl) = &stmt
            && !decl.compile_time
            && matches!(
                decl.binding,
                BindingPattern::Array { .. } | BindingPattern::Object { .. }
            )
        {
            // A destructuring head is a binder: it takes `sit`/`varia`.
            self.errors.push(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, keyword_span)
                    .with_arg("issue", "binder_fixum_retired")
                    .with_arg("keyword", keyword),
            );
        }
        Ok(stmt)
    }

    /// Everything after the declaration word of a typed binding: the optional
    /// type, the binding pattern and the initializer. `allow_compile_time`
    /// admits the `=` form (`fixum`/`varia` only; `sit` is run-time).
    fn parse_var_decl_rest(
        &mut self,
        mutability: AstMutability,
        keyword: String,
        allow_compile_time: bool,
    ) -> Result<StmtKind, ParseError> {
        let (mut ty, binding) = self.parse_typed_binding(keyword)?;

        let mut compile_time = false;
        let (init, await_form) = if self.eat(&TokenKind::Assign) {
            // Value-side marker (`mut T b ← copy a`, `mut T b ← own a`): the
            // marker names what happens to the initializer, and the binding
            // gets the exclusive-value type mode. The binding-side spelling
            // (`mut copy T b ← a`) is retired (PARSE010).
            if let Some(slot) = ty.as_mut().filter(|slot| slot.mode.is_none())
                && let Some(mode) = self.parse_value_marker(None)
            {
                slot.mode = Some(mode);
            }
            (Some(Box::new(self.parse_expression()?)), None)
        } else if allow_compile_time && self.check(&TokenKind::Eq) {
            compile_time = true;
            self.parse_compile_time_initializer(mutability, &binding)?
        } else if self.check(&TokenKind::ConversioAssign) {
            (
                Some(Box::new(
                    self.parse_conversio_assign_initializer(ty.as_ref(), &binding)?,
                )),
                None,
            )
        } else if self.eat(&TokenKind::AwaitAssign) {
            if !matches!(binding, BindingPattern::Ident(_)) {
                return Err(Self::expected_issue_error(
                    "await_binding_requires_name",
                    self.previous_span(),
                ));
            }
            (
                Some(Box::new(self.parse_expression()?)),
                Some(AwaitBindingForm::Assignment),
            )
        } else {
            (None, None)
        };

        Ok(StmtKind::Var(VarDecl {
            mutability,
            await_form,
            ty,
            binding,
            init,
            compile_time,
        }))
    }

    /// Whether the current word is a retired writable-local word opening a
    /// declaration: the word is not a keyword of the active pack, a type word
    /// follows, and a name or a type tail follows that.
    fn at_retired_var_decl(&self) -> bool {
        if self.keyword_ident_at(0).is_some() {
            return false;
        }
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        if !RETIRED_VAR_WORDS.contains(&self.interner.resolve(sym)) {
            return false;
        }
        matches!(
            self.peek_at(1).kind,
            TokenKind::Ident(_) | TokenKind::Underscore(_)
        ) && matches!(
            self.peek_at(2).kind,
            TokenKind::Ident(_)
                | TokenKind::Underscore(_)
                | TokenKind::LtAscii
                | TokenKind::LBracket
                | TokenKind::Cup
        )
    }

    /// Diagnose the retired writable-local word (`PARSE010 var_retired`, args
    /// `keyword` as written and `name`) and parse the declaration whole as the
    /// `mut` binding it was.
    ///
    /// FABER-2-SUNSET: delete with the other retired-word diagnostics.
    fn parse_retired_var_decl(&mut self) -> Result<StmtKind, ParseError> {
        let keyword = self.current_word();
        let keyword_span = self.current_span();
        self.advance();
        let stmt = self.parse_var_decl_rest(AstMutability::Mutable, keyword.clone(), true)?;
        let name = match &stmt {
            StmtKind::Var(decl) => match &decl.binding {
                BindingPattern::Ident(ident) => self.interner.resolve(ident.name).to_owned(),
                _ => "_".to_owned(),
            },
            _ => "_".to_owned(),
        };
        self.errors.push(
            ParseError::structured(ParseErrorKind::DeclarationInvalid, keyword_span)
                .with_arg("issue", "var_retired")
                .with_arg("keyword", keyword)
                .with_arg("name", name),
        );
        Ok(stmt)
    }

    /// Whether the `ref` word at statement start opens a local declaration:
    /// a type word follows (`ref Node cur ← head`).
    fn at_ref_local_decl(&self) -> bool {
        matches!(
            self.peek_at(1).kind,
            TokenKind::Ident(_) | TokenKind::Underscore(_)
        )
    }

    /// Parse `ref T name ← place`: a run-time name that can be re-pointed and
    /// is read-only through. It is the existing `ref` access view of a
    /// writable-name cell, so the checker's `ref` rules apply unchanged.
    ///
    /// `ref` takes a place to read, never a new owned value: `← copy e` and
    /// `← own e` are `PARSE010 ref_local_value_marker`; a missing `←` is
    /// `PARSE010 ref_local_requires_initializer`; destructuring is
    /// `PARSE010 ref_local_requires_name`. The declaration is parsed whole so
    /// recovery resumes after it.
    fn parse_ref_decl(&mut self) -> Result<StmtKind, ParseError> {
        let keyword = self.current_word();
        let keyword_span = self.current_span();
        let (ty, binding) = self.parse_typed_binding(keyword.clone())?;
        if !matches!(binding, BindingPattern::Ident(_)) {
            return Err(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, keyword_span)
                    .with_arg("issue", "ref_local_requires_name")
                    .with_arg("keyword", keyword),
            );
        }
        if !self.eat(&TokenKind::Assign) {
            return Err(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, keyword_span)
                    .with_arg("issue", "ref_local_requires_initializer")
                    .with_arg("keyword", keyword),
            );
        }
        let marker_span = self.current_span();
        let marker = self.current_word();
        if self.parse_value_marker(None).is_some() {
            self.errors.push(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, marker_span)
                    .with_arg("issue", "ref_local_value_marker")
                    .with_arg("keyword", keyword)
                    .with_arg("marker", marker),
            );
        }
        let init = Box::new(self.parse_expression()?);
        Ok(StmtKind::Var(VarDecl {
            mutability: AstMutability::Mutable,
            await_form: None,
            ty,
            binding,
            init: Some(init),
            compile_time: false,
        }))
    }

    /// Spelling of the identifier word at the cursor (empty for any other
    /// token), as written in the source.
    pub(super) fn current_word(&self) -> String {
        match self.peek().kind {
            TokenKind::Ident(sym) => self.interner.resolve(sym).to_owned(),
            _ => String::new(),
        }
    }

    /// Record the retired binding-side marker (`mut copy T b ← a`,
    /// `mut own T b ← a`): `PARSE010 binding_marker_moved_to_value`, args
    /// `keyword` (the declaration word as written), `marker` (the marker word
    /// as written) and `name`.
    ///
    /// FABER-2-SUNSET: delete with the retired-word diagnostics; the marker
    /// goes on the value (`mut T b ← copy a`). The declaration is still
    /// parsed whole so recovery resumes after it. Parameter markers
    /// (`fn f(copy T p)`) are not affected: only the binding position.
    fn record_binding_marker_retired(
        &mut self,
        span: Span,
        keyword: String,
        marker: String,
        binding: &BindingPattern,
    ) {
        let name = match binding {
            BindingPattern::Ident(ident) => self.interner.resolve(ident.name).to_owned(),
            _ => "name".to_owned(),
        };
        self.errors.push(
            ParseError::structured(ParseErrorKind::DeclarationInvalid, span)
                .with_arg("issue", "binding_marker_moved_to_value")
                .with_arg("keyword", keyword)
                .with_arg("marker", marker)
                .with_arg("name", name),
        );
    }

    /// Parse the optional written type and the binding pattern of a binding
    /// declaration. A retired binding-side `copy`/`own` marker leaves the
    /// declaration parsing whole but is recorded through
    /// [`Self::record_binding_marker_retired`].
    fn parse_typed_binding(
        &mut self,
        keyword: String,
    ) -> Result<(Option<TypeExpr>, BindingPattern), ParseError> {
        let marker_span = self.current_span();
        let marker = self.current_word();
        let ty = if self.check(&TokenKind::LBracket) || self.check(&TokenKind::LBrace) {
            None
        } else {
            let first = self.current_span();
            let ty = self.parse_type()?;
            // A lone word read as a type and followed by an initializer
            // operator was the name: the type slot is missing.
            if let TypeExprKind::Named(ident, args) = &ty.kind
                && args.is_empty()
                && matches!(
                    self.peek().kind,
                    TokenKind::Assign
                        | TokenKind::Eq
                        | TokenKind::ConversioAssign
                        | TokenKind::AwaitAssign
                )
            {
                let name = self.interner.resolve(ident.name).to_owned();
                return Err(
                    ParseError::structured(ParseErrorKind::DeclarationInvalid, first)
                        .with_arg("issue", "binding_type_slot_required")
                        .with_arg("keyword", keyword)
                        .with_arg("name", name),
                );
            }
            Some(ty)
        };

        let binding = self.parse_binding_pattern()?;
        if ty
            .as_ref()
            .is_some_and(|slot| matches!(slot.mode, Some(TypeMode::Copy | TypeMode::Own)))
        {
            self.record_binding_marker_retired(marker_span, keyword, marker, &binding);
        }

        Ok((ty, binding))
    }

    /// Parse the `= expr` initializer of a local compile-time constant
    /// (DECISIONS D5.10). The current token is `=`.
    ///
    /// `=` states a compile-time fact, so only an immutable named binding may
    /// use it: `varia` is a runtime store and destructuring has no single
    /// constant to name. Both are named errors that point at `←`.
    fn parse_compile_time_initializer(
        &mut self,
        mutability: AstMutability,
        binding: &BindingPattern,
    ) -> Result<(Option<Box<Expr>>, Option<AwaitBindingForm>), ParseError> {
        let span = self.current_span();
        if mutability == AstMutability::Mutable {
            return Err(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, span)
                    .with_arg("issue", "varia_compile_time_initializer"),
            );
        }
        if !matches!(binding, BindingPattern::Ident(_)) {
            return Err(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, span)
                    .with_arg("issue", "compile_time_requires_name"),
            );
        }
        self.advance();
        Ok((Some(Box::new(self.parse_expression()?)), None))
    }

    /// Whether the current `generis` word opens the retired module-level
    /// static: the next token must begin a type (an identifier or a
    /// parenthesized type). Anything else leaves `generis` an identifier.
    fn at_retired_static_decl(&self) -> bool {
        matches!(
            self.peek_at(1).kind,
            TokenKind::Ident(_) | TokenKind::LParen
        )
    }

    /// Diagnose the retired module-level static (st1 R1): `PARSE010
    /// static_decl_retired`, args `keyword` (the spelling as written, i.e.
    /// the pack's `generis` row) and `name`.
    ///
    /// The old declaration (the genus static-field production) is parsed
    /// whole so recovery resumes after it, then answered as the constant it
    /// became (`fixum T NAME = e`); the error is already recorded, so nothing
    /// downstream runs on it. The keyword stays retired permanently: there is
    /// no alias period.
    fn parse_retired_static_decl(&mut self) -> Result<StmtKind, ParseError> {
        let keyword_span = self.current_span();
        let keyword = match self.peek().kind {
            TokenKind::Ident(sym) => self.interner.resolve(sym).to_owned(),
            _ => String::new(),
        };
        // The keyword is consumed here, so the member-level retirement
        // diagnostic does not stack on the module-level one.
        self.advance();
        let mut field = self.parse_field_decl(false)?;
        // The old surface also took `←` here (a named error then); consume the
        // initializer so the whole declaration is skipped, not half of it.
        if field.init.is_none() && self.eat(&TokenKind::Assign) {
            field.init = Some(Box::new(self.parse_expression()?));
        }
        let name = self.interner.resolve(field.name.name).to_owned();
        self.errors.push(
            ParseError::structured(ParseErrorKind::DeclarationInvalid, keyword_span)
                .with_arg("issue", "static_decl_retired")
                .with_arg("keyword", keyword)
                .with_arg("name", name),
        );
        let compile_time = field.init.is_some();
        Ok(StmtKind::Var(VarDecl {
            mutability: AstMutability::Immutable,
            await_form: None,
            ty: Some(field.ty),
            binding: BindingPattern::Ident(field.name),
            init: field.init,
            compile_time,
        }))
    }

    /// Parse a `↤` conversion-directed initializer whose binding is already
    /// parsed. The current token is `↤`.
    ///
    /// The binding pattern becomes the assignment target; the operator is
    /// preserved as `AssignOp::ConversioAssign` through the syntax tree (no
    /// erasure into `←`). The `⊥` default is parsed through the shared assignment
    /// tail so it attaches exactly as it does in expression position.
    ///
    /// Rejects every binding form without a concrete conversion destination:
    /// `fixum _` (inferred type), `sit` (inferred), untyped destructuring, and
    /// wildcard bindings — all with the stable
    /// `conversio_assign_requires_concrete_type` issue identity.
    fn parse_conversio_assign_initializer(
        &mut self,
        ty: Option<&TypeExpr>,
        binding: &BindingPattern,
    ) -> Result<Expr, ParseError> {
        let has_concrete_type = ty.is_some_and(|t| !matches!(t.kind, TypeExprKind::Infer));
        let BindingPattern::Ident(ident) = binding else {
            return Err(self.conversio_assign_requires_concrete_type_error());
        };
        if !has_concrete_type {
            return Err(self.conversio_assign_requires_concrete_type_error());
        }
        // `Ident` is not `Clone`; rebuild the binding name for the target
        // expression from its Copy fields.
        let ident_name = ident.name;
        let ident_span = ident.span;
        let id = self.next_id();
        let target = Expr {
            id,
            kind: ExprKind::Ident(Ident {
                name: ident_name,
                span: ident_span,
            }),
            span: ident_span,
        };
        self.parse_assignment_tail(target, ident_span)
    }

    /// The `↤` destination error shared by declarations whose written type
    /// cannot name a conversion target (`fixum _`, `sit`, destructuring).
    fn conversio_assign_requires_concrete_type_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::Unexpected, self.current_span())
            .with_arg("issue", "conversio_assign_requires_concrete_type")
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
        let keyword = self.current_word();
        self.advance();

        let (ty, binding) = self.parse_typed_binding(keyword)?;

        // `figendum`/`variandum` retain `←`: combining implicit conversion and
        // await ordering needs a separate language decision, so `↤` here is a
        // structured rejection.
        if self.check(&TokenKind::ConversioAssign) {
            return Err(
                ParseError::structured(ParseErrorKind::Unexpected, self.current_span())
                    .with_arg("issue", "conversio_assign_await_unsupported"),
            );
        }
        self.expect_issue(&TokenKind::Assign, "awaited_binding_assignment")?;
        let init = Some(Box::new(self.parse_expression()?));

        Ok(StmtKind::Var(VarDecl {
            mutability,
            await_form: Some(AwaitBindingForm::Morphology),
            ty,
            binding,
            init,
            compile_time: false,
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
        self.advance();
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
        self.advance();
        let future = Box::new(self.parse_expression()?);
        let span = start.merge(self.previous_span());
        Ok(StmtKind::Tacebit(TacebitStmt { future, span }))
    }

    /// Parse `const size H ← expr` or `const size N = natural`.
    fn parse_size_decl(&mut self, mutability: AstMutability) -> Result<StmtKind, ParseError> {
        let size_span = self.current_span();
        self.expect_keyword_ident(TokenKind::Magnitudo)?;
        if mutability == AstMutability::Mutable {
            return Err(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, size_span)
                    .with_arg("issue", "size_is_write_once"),
            );
        }
        let name = self.parse_ident()?;
        let is_static = if self.eat(&TokenKind::Eq) {
            if !matches!(self.peek().kind, TokenKind::Integer(_)) {
                return Err(ParseError::structured(
                    ParseErrorKind::DeclarationInvalid,
                    self.current_span(),
                )
                .with_arg("issue", "size_static_requires_literal"));
            }
            true
        } else {
            self.expect_issue(&TokenKind::Assign, "size_declaration_assignment")?;
            false
        };
        let init = Box::new(self.parse_expression()?);
        Ok(StmtKind::SizeDecl(radix_syntax::SizeDecl {
            name,
            init,
            is_static,
        }))
    }

    /// Whether the cursor (just after `sit`) starts a written type slot
    /// (`T`, or `_` to infer) before the name. A lone name followed by an
    /// initializer operator, a line break or a block end is the type-omitted
    /// form, which does not exist (`binding_type_slot_required`).
    fn at_typed_sit_decl(&self) -> bool {
        if !matches!(self.peek().kind, TokenKind::Ident(_)) {
            return !matches!(self.peek().kind, TokenKind::LBracket | TokenKind::LBrace);
        }
        // A mode word opens a type only when a type word follows it; a
        // binding may still be named like a mode word (`sit copy ← 1`).
        if (self.check_keyword_ident(TokenKind::De)
            || self.check_keyword_ident(TokenKind::Varia)
            || self.check_keyword_ident(TokenKind::Own)
            || self.check_keyword_ident(TokenKind::Copy))
            && matches!(
                self.peek_at(1).kind,
                TokenKind::Ident(_) | TokenKind::Underscore(_) | TokenKind::LParen
            )
        {
            return true;
        }
        match self.peek_at(1).kind {
            TokenKind::Assign
            | TokenKind::ConversioAssign
            | TokenKind::AwaitAssign
            | TokenKind::Eq
            | TokenKind::RBrace
            | TokenKind::Eof => false,
            _ => !self.raw_newline_between_words(0, 1),
        }
    }

    /// Whether a raw `Newline` token separates the non-trivia words at
    /// `first` and `second` (lookahead offsets from the cursor).
    fn raw_newline_between_words(&self, first: usize, second: usize) -> bool {
        let base = self.non_trivia_indices.partition_point(|&i| i < self.pos);
        let (Some(&a), Some(&b)) = (
            self.non_trivia_indices.get(base + first),
            self.non_trivia_indices.get(base + second),
        ) else {
            return true;
        };
        self.tokens[a..b]
            .iter()
            .any(|token| token.kind == TokenKind::Newline)
    }

    /// Parse `sit` (en `let`) run-time write-once declarations.
    ///
    /// Every declaration writes its type slot: `sit T name ← e`, or `sit _
    /// name ← e` to ask for inference. The type-omitted shorthand (`sit name ←
    /// e`) does not exist: it is `PARSE010 binding_type_slot_required`
    /// (args `keyword`, `name`).
    fn parse_sit_decl(&mut self) -> Result<StmtKind, ParseError> {
        let keyword = self.current_word();
        self.expect_keyword_ident(TokenKind::Sit)?;

        // Destructuring head: `sit [a, b] ← e` / `sit { a, b } ← e`.
        if self.check(&TokenKind::LBracket) || self.check(&TokenKind::LBrace) {
            return self.parse_var_decl_rest(AstMutability::Immutable, keyword, false);
        }

        if self.at_typed_sit_decl() {
            return self.parse_var_decl_rest(AstMutability::Immutable, keyword, false);
        }
        Err(self.type_slot_required_error(keyword))
    }

    /// `PARSE010 binding_type_slot_required`: a declaration word followed
    /// directly by a name and no type slot. The word at the cursor is the
    /// would-be name; `_` is the explicit request to infer.
    fn type_slot_required_error(&self, keyword: String) -> ParseError {
        ParseError::structured(ParseErrorKind::DeclarationInvalid, self.current_span())
            .with_arg("issue", "binding_type_slot_required")
            .with_arg("keyword", keyword)
            .with_arg("name", self.current_word())
    }

    pub(super) fn parse_binding_pattern(&mut self) -> Result<BindingPattern, ParseError> {
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

        let ident = self.parse_contextual_ident(KeywordOwner::BindingTarget)?;
        self.note_value_binding(&ident);
        Ok(BindingPattern::Ident(ident))
    }

    fn parse_array_binding_pattern(&mut self) -> Result<BindingPattern, ParseError> {
        let start = self.current_span();
        self.expect_issue(&TokenKind::LBracket, "array_binding_pattern_start")?;

        let mut elements = Vec::new();
        let mut rest = None;

        while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
            if self.eat_keyword_ident(TokenKind::Ceteri) {
                rest = Some(self.parse_ident()?);
                if self.check(&TokenKind::Comma) {
                    return Err(self.rest_item_comma_error("array_binding_pattern"));
                }
                break;
            }

            let element = if let TokenKind::Underscore(_) = self.peek().kind {
                let span = self.peek().span;
                self.advance();
                BindingPattern::Wildcard(span)
            } else if let TokenKind::LBracket = self.peek().kind {
                // A nested pattern nests one `array_binding_pattern` level and
                // never re-enters the expression grammar, so the depth budget is
                // charged at this recursion.
                let span = self.current_span();
                self.nested(ParseErrorKind::StatementInvalid, span, |parser| {
                    parser.parse_array_binding_pattern()
                })?
            } else {
                BindingPattern::Ident(self.parse_ident()?)
            };

            elements.push(element);

            if !self.consume_item_separator(&TokenKind::RBracket, "array_binding_pattern")? {
                break;
            }
        }

        self.expect_issue(&TokenKind::RBracket, "array_binding_pattern_end")?;
        let span = start.merge(self.previous_span());
        Ok(BindingPattern::Array {
            elements,
            is_rest: rest,
            span,
        })
    }

    fn parse_object_binding_pattern(&mut self) -> Result<BindingPattern, ParseError> {
        let start = self.current_span();
        self.expect_issue(&TokenKind::LBrace, "object_binding_pattern_start")?;

        let mut fields = Vec::new();
        let mut rest = None;

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.eat_keyword_ident(TokenKind::Ceteri) {
                rest = Some(self.parse_ident()?);
                if self.check(&TokenKind::Comma) {
                    return Err(self.rest_item_comma_error("object_binding_pattern"));
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

            if !self.consume_item_separator(&TokenKind::RBrace, "object_binding_pattern")? {
                break;
            }
        }

        self.expect_issue(&TokenKind::RBrace, "object_binding_pattern_end")?;
        let span = start.merge(self.previous_span());
        Ok(BindingPattern::Object {
            fields,
            is_rest: rest,
            span,
        })
    }

    /// Diagnose a comma after a rest element: the rest element is always last,
    /// so a following `ceteri` is a duplicate and anything else is a trailing
    /// comma. `position` names the enclosing list for the trailing-comma
    /// diagnostic.
    fn rest_item_comma_error(&mut self, position: &'static str) -> ParseError {
        let comma_span = self.current_span();
        self.advance();
        if self.check_keyword_ident(TokenKind::Ceteri) {
            return ParseError::structured(ParseErrorKind::Expected, self.current_span())
                .with_arg("issue", "duplicate_rest_pattern")
                .with_arg("keyword", "ceteri");
        }
        ParseError::structured(ParseErrorKind::Expected, comma_span)
            .with_arg("issue", "item_list_trailing_comma")
            .with_arg("family", "item_list")
            .with_arg("position", position)
    }

    // FUNCTION DECLARATIONS

    /// Parse function declaration.
    ///
    /// GRAMMAR:
    ///   func-decl := `functio` ident `(` params `)` modifiers ['→' type] block
    ///   params    := type-param* regular-param*
    ///   type-param := `prae` `typus` ident
    ///
    /// A top-level function or a genus method annotated `@ nondum` may omit the
    /// block: it is present in the interface but unavailable, and any use is
    /// refused by semantic analysis. Library analysis
    /// (`allow_bodyless_functions`) keeps accepting signature-only binding
    /// declarations.
    fn parse_func_decl(&mut self) -> Result<StmtKind, ParseError> {
        let nondum_declared = std::mem::take(&mut self.nondum_function_pending);
        self.expect_keyword_ident(TokenKind::Functio)?;

        let (decl, _) = self.parse_func_decl_after_head(nondum_declared, false)?;
        Ok(StmtKind::Func(decl))
    }

    /// Parse a kernel declaration head (operator ruling 2026-10-30).
    ///
    /// GRAMMAR:
    ///   kernel-decl := `nucleum` ident generic-params? `(` params `)`
    ///                  func-modifier* kernel-mode? return-clause?
    ///                  alternate-exit-clause? block
    ///   kernel-mode := `laxus` | `intutus` | `fragment`
    ///
    /// The head reuses the kernel keyword identity, so every reader pack
    /// spells it with its own `nucleum` row (`kernel` in English). It lowers
    /// to the same function declaration the `@ nucleum` annotation form
    /// marks: the parsed mode synthesizes the nucleum annotation, and the
    /// caller attaches it where the authored marker would ride.
    fn parse_kernel_decl(&mut self) -> Result<(FuncDecl, Annotation), ParseError> {
        let nondum_declared = std::mem::take(&mut self.nondum_function_pending);
        let head_span = self.current_span();
        self.expect_keyword_ident(TokenKind::Nucleum)?;

        let (decl, mode) = self.parse_func_decl_after_head(nondum_declared, true)?;
        // The synthesized marker covers the head word and, when written, the
        // mode words — the same span shape `@ nucleum laxus` owns.
        let span = mode
            .mode_span
            .map_or(head_span, |mode_span| head_span.merge(mode_span));
        let annotation = Annotation {
            kind: AnnotationKind::Nucleum(NucleumAnnotation {
                is_fragment: mode.is_fragment,
                tier: mode.tier,
                braced: false,
            }),
            span,
        };
        Ok((decl, annotation))
    }

    /// Parse the declaration body shared by both heads (`functio` and the
    /// kernel head): name, generic parameters, parameter list, modifiers,
    /// [kernel mode], posture, return clause, alternate exit, block.
    ///
    /// `kernel_head` selects the kernel mode slot between the modifier run and
    /// the posture; the parsed mode is returned so the head can synthesize the
    /// nucleum annotation. A `functio` head leaves the slot unparsed and gets
    /// the default (no fragment, default tier, no written words) back, unused
    /// by its caller.
    fn parse_func_decl_after_head(
        &mut self,
        nondum_declared: bool,
        kernel_head: bool,
    ) -> Result<(FuncDecl, ParsedKernelMode), ParseError> {
        let name = self.parse_function_name()?;
        let type_params = self.parse_generic_params()?;

        self.expect_issue(&TokenKind::LParen, "function_declaration_parameters_start")?;
        let params = self.parse_param_list()?;
        self.expect_issue(&TokenKind::RParen, "function_declaration_parameters_end")?;

        let modifiers = self.parse_func_modifiers()?;

        let kernel_mode = if kernel_head {
            self.parse_kernel_mode()?
        } else {
            ParsedKernelMode::default()
        };

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
        } else if self.options.allow_bodyless_functions || nondum_declared {
            None
        } else {
            return Err(ParseError::structured(
                ParseErrorKind::FunctionBodyAbsent,
                self.current_span(),
            )
            .with_arg("block", "{ ... }"));
        };

        Ok((
            FuncDecl {
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
            },
            kernel_mode,
        ))
    }

    /// GRAMMAR (operator ruling 2026-10-30):
    ///   kernel_mode := `laxus` | `intutus` | `fragment`
    ///
    /// One optional mode word in the kernel head's signature slot, after the
    /// modifier run. Mirrors the annotation form's refusal set so both heads
    /// teach one vocabulary: a second tier word (`duplicate_nucleum_tier`),
    /// a tier beside `fragment` (`nucleum_tier_on_fragment`), and a word that
    /// is no tier (`unknown_nucleum_tier`) are all refused by name.
    fn parse_kernel_mode(&mut self) -> Result<ParsedKernelMode, ParseError> {
        let start = self.current_span();
        let is_fragment = self.eat_keyword_ident(TokenKind::Fragment);
        let tier = self.parse_kernel_mode_tier(is_fragment)?;
        let mode_span = (is_fragment || tier != KernelTier::Default)
            .then(|| start.merge(self.previous_span()));
        Ok(ParsedKernelMode {
            is_fragment,
            tier,
            mode_span,
        })
    }

    /// The tier word of the kernel mode slot, mirroring the annotation
    /// form's `parse_nucleum_tier` refusals over the signature slot's words.
    ///
    /// A callable-posture word is grammar structure of the shared signature
    /// tail, not a mode: it ends the slot and reaches the posture parse (and
    /// the semantic `nucleum_async` refusal) exactly as the annotation form
    /// does. Any other word in the slot names an unknown tier, because after
    /// the modifier run only a mode, a posture word, or the return and block
    /// clauses may follow.
    fn parse_kernel_mode_tier(&mut self, fragment: bool) -> Result<KernelTier, ParseError> {
        let mut tier = KernelTier::Default;
        let mut written = false;
        loop {
            let TokenKind::Ident(symbol) = self.peek().kind else {
                break;
            };
            let next = match self.current_keyword_ident() {
                Some(TokenKind::Laxus) => KernelTier::Relaxed,
                Some(TokenKind::Intutus) => KernelTier::Unsafe,
                Some(TokenKind::Fragment) if written => {
                    return Err(self.invalid_annotation_error("nucleum_tier_on_fragment"));
                }
                Some(TokenKind::Fragment) => break,
                Some(TokenKind::Fiet | TokenKind::Fiunt | TokenKind::Fient) => break,
                _ => {
                    return Err(self
                        .invalid_annotation_error("unknown_nucleum_tier")
                        .with_arg("name", self.interner.resolve(symbol).to_owned()));
                }
            };
            if written {
                return Err(self.invalid_annotation_error("duplicate_nucleum_tier"));
            }
            if fragment {
                return Err(self.invalid_annotation_error("nucleum_tier_on_fragment"));
            }
            self.advance();
            tier = next;
            written = true;
        }
        Ok(tier)
    }

    /// Whether the current `nucleum` word is claimed as a kernel declaration
    /// head: a declaration-name shape must follow, so an ordinary identifier
    /// use, a `kernel(…)` call, or a member access stays expression material.
    fn at_kernel_decl_head(&self) -> bool {
        if self.current_keyword_ident() != Some(TokenKind::Nucleum) {
            return false;
        }
        matches!(
            self.peek_at(1).kind,
            TokenKind::Ident(_) | TokenKind::Underscore(_) | TokenKind::Tag
        )
    }

    /// Fold a kernel declaration head's synthesized marker into the
    /// annotation stack of the same declaration (operator ruling 2026-10-30).
    ///
    /// Both heads on one declaration are refused by name — no silent merge —
    /// and a fragment mode conflicts with a graphics fragment annotation
    /// exactly as the annotation form's modifier does. The synthesized marker
    /// takes the front of the stack, the canonical position the annotation
    /// form writes it in, so both spellings lower to one annotation sequence.
    fn merge_kernel_head_annotation(
        &mut self,
        mut annotations: Vec<Annotation>,
        head: Annotation,
    ) -> Vec<Annotation> {
        if annotations
            .iter()
            .any(|annotation| matches!(annotation.kind, AnnotationKind::Nucleum(_)))
        {
            self.errors.push(
                ParseError::structured(ParseErrorKind::AnnotationInvalid, head.span)
                    .with_arg("issue", "nucleum_double_head"),
            );
        } else if matches!(&head.kind, AnnotationKind::Nucleum(nucleum) if nucleum.is_fragment)
            && let Some(graphics) = annotations
                .iter()
                .find(|annotation| self.is_graphics_fragment_annotation(&annotation.kind))
        {
            self.errors.push(
                ParseError::structured(ParseErrorKind::AnnotationInvalid, graphics.span)
                    .with_arg("issue", "nucleum_fragment_conflicts_with_fragment"),
            );
        }
        annotations.insert(0, head);
        annotations
    }

    /// Whether `annotation` is the `@ nondum` availability marker, in either
    /// its sugar (`@ nondum [target] ["reason"]`) or braced spelling.
    fn is_nondum_annotation(&self, annotation: &Annotation) -> bool {
        match &annotation.kind {
            AnnotationKind::Statement(stmt) => {
                radix_syntax::nondum_from_stmt(stmt, &self.interner).is_some()
            }
            AnnotationKind::Braced(braced) => {
                radix_syntax::braced_nondum_view(braced, &self.interner).is_some()
            }
            _ => false,
        }
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
    ///   regular-param := [`ceteri`] type ident [`sponte`] [`ut` ident] [`vel` expr]
    ///
    /// Generic parameters live in the angle-bracket list before `(`.
    fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();

        while !self.check(&TokenKind::RParen) && !self.is_at_end() {
            let start = self.current_span();

            let rest = self.eat_keyword_ident(TokenKind::Ceteri);
            let axis = self.parse_axis_marker();
            let ty = self.parse_type()?;

            let name = self.parse_param_name()?;
            self.note_value_binding(&name);

            let sponte = self.parse_post_name_marker();

            let alias = if self.eat_keyword_ident(TokenKind::Ut) {
                let alias = self.parse_param_name()?;
                self.note_value_binding(&alias);
                Some(alias)
            } else {
                None
            };

            let default = if self.eat_keyword_ident(TokenKind::Vel) {
                Some(Box::new(self.parse_expression()?))
            } else {
                None
            };

            let span = start.merge(self.previous_span());
            params.push(Param {
                sponte,
                mode: if axis {
                    AstParamMode::Axis
                } else {
                    AstParamMode::Owned
                },
                is_rest: rest,
                ty,
                name,
                alias,
                default,
                span,
            });

            if !self.consume_item_separator(&TokenKind::RParen, "function_parameters")? {
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
            } else if self.at_retired_curata_modifier() {
                return Err(self.allocator_surface_removed_error(super::stmt::RETIRED_CURATA));
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
                            .with_arg("issue", "exitus_value_out_of_range"));
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
    fn parse_post_name_marker(&mut self) -> bool {
        self.eat_keyword_ident(TokenKind::Sponte)
    }

    // CLASS DECLARATIONS

    /// Parse a class declaration.
    ///
    /// GRAMMAR:
    ///   class-decl := genus ident [type-params] [implet contract-list] { member* }
    ///   contract-list := contract-ref ((',' | '∩') contract-ref)*
    ///
    /// Class headers are intentionally narrow: a comma-separated `implet` list
    /// and nothing else. Class inheritance (`sub`, `abstractus`) was removed
    /// (DECISIONS D8.1); its old shapes report `inheritance_removed`. Field and
    /// method parsing stays inside this module because member syntax shares
    /// declaration markers with top-level bindings and functions.
    fn parse_class_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Genus)?;

        let name = self.parse_class_name()?;
        let type_params = self.parse_generic_params()?;

        if self.at_retired_sub_clause() {
            return Err(self.inheritance_removed_error(RETIRED_SUB));
        }

        let mut implements = Vec::new();
        if self.eat_keyword_ident(TokenKind::Implet) {
            loop {
                // R1: an implements slot is a named contract; holes are not
                // conformance declarations and fail closed here.
                if matches!(self.peek().kind, TokenKind::Underscore(_)) {
                    return Err(ParseError::structured(
                        ParseErrorKind::Expected,
                        self.current_span(),
                    )
                    .with_arg("issue", "implements_slot_requires_name"));
                }
                implements.push(self.parse_contract_ref()?);
                // `∩` is the same separator as the comma: the implements
                // clause is a conjunctive (intersection) list. `∪` and holes
                // are not item separators, so they fail closed here (R1).
                if self.eat(&TokenKind::Cap) {
                    continue;
                }
                if !self.consume_item_separator(&TokenKind::LBrace, "class_implements")? {
                    break;
                }
            }
        }

        if !self.check(&TokenKind::LBrace) {
            return Err(ParseError::structured(
                ParseErrorKind::ClassBodyAbsent,
                self.current_span(),
            ));
        }
        self.advance();

        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let member = self.parse_class_member()?;
            members.push(member);
        }

        // Trivia immediately before the consuming `}` has no following member
        // to attach to; retain it on the class-end boundary so the post-parse
        // completeness check can claim it (mirrors `parse_block`).
        let end_trivia = self.parse_block_end_trivia();

        self.expect_issue(&TokenKind::RBrace, "class_body_end")?;

        Ok(StmtKind::Class(ClassDecl {
            name,
            type_params,
            implements,
            members,
            end_trivia,
        }))
    }

    /// Whether the current word opens a retired `abstractus genus` header.
    ///
    /// `abstractus` was removed (DECISIONS D8.1). Only the old declaration
    /// shape `abstractus genus …` is diagnosed, so `abstractus` stays an
    /// ordinary identifier everywhere else. Matching the canonical Latin
    /// spelling is deliberate: the keyword no longer has pack rows.
    pub(super) fn at_retired_abstractus_decl(&self) -> bool {
        self.current_ident_is(RETIRED_ABSTRACTUS)
            && self.keyword_ident_at(1) == Some(TokenKind::Genus)
    }

    /// Whether the current word is the retired `sub Parent` genus clause.
    ///
    /// The old shape was `sub IDENT` directly after the genus name and type
    /// parameters, where no other identifier is legal.
    fn at_retired_sub_clause(&self) -> bool {
        self.current_ident_is(RETIRED_SUB) && matches!(self.peek_at(1).kind, TokenKind::Ident(_))
    }

    /// Migration diagnostic for removed class inheritance (`abstractus`/`sub`).
    pub(super) fn inheritance_removed_error(&self, keyword: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::Unexpected, self.current_span())
            .with_arg("issue", "inheritance_removed")
            .with_arg("keyword", keyword)
    }

    /// Parse the field portion shared by genus and union declaration members.
    ///
    /// `genus_member` is true for a `genus` field, where a leading `de` is the
    /// refused `ref` modifier; a `discretio` shared field is unmarked by
    /// design, so there a leading `de` is the access marker of its type.
    ///
    /// A `const` member is the type-level compile-time constant: it takes the
    /// `=` initializer, and only that. A missing initializer is `PARSE010
    /// const_member_requires_initializer`; the run-time arrow is `PARSE010
    /// const_member_assign_initializer`. Both are recoverable.
    fn parse_field_decl(&mut self, genus_member: bool) -> Result<FieldDecl, ParseError> {
        let (modifier, retired) = match self.eat_field_modifier(genus_member) {
            Some((modifier, retired)) => (Some(modifier), retired),
            None => (None, false),
        };
        if modifier.is_some() && self.current_field_modifier().is_some() {
            return Err(ParseError::structured(
                ParseErrorKind::DeclarationInvalid,
                self.current_span(),
            )
            .with_arg("issue", "field_modifier_repeated"));
        }
        if self.at_removed_nexum_modifier() {
            return Err(
                ParseError::structured(ParseErrorKind::Unexpected, self.current_span())
                    .with_arg("issue", "nexum_removed"),
            );
        }
        let ty = self.parse_type()?;
        let name = self.parse_param_name()?;
        let sponte = self.parse_post_name_marker();
        let is_const = modifier == Some(FieldModifier::Const) && !retired;
        let init = if self.eat(&TokenKind::Eq) {
            Some(Box::new(self.parse_expression()?))
        } else if is_const && self.check(&TokenKind::Assign) {
            // `const T X ← e`: a constant is a compile-time fact, so the
            // run-time arrow is named. The initializer is parsed whole so
            // recovery resumes after the member.
            self.errors.push(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, self.current_span())
                    .with_arg("issue", "const_member_assign_initializer"),
            );
            self.advance();
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };
        if is_const && init.is_none() {
            self.errors.push(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, name.span)
                    .with_arg("issue", "const_member_requires_initializer"),
            );
        }

        Ok(FieldDecl {
            modifier,
            sponte,
            ty,
            name,
            init,
        })
    }

    /// The field modifier the current word spells, if any (DECISIONS D16.1).
    ///
    /// `sit` (en `let`) is the read-only instance field, `varia` (en `mut`) the
    /// writable one, `fixum` (en `const`) the type-level compile-time
    /// constant. The retired `generis` (en `static`) and the refused `de` (en
    /// `ref`) are not modifiers; they are recognised by
    /// `retired_field_modifier_issue` so the parser can name them.
    fn current_field_modifier(&self) -> Option<FieldModifier> {
        match self.current_keyword_ident()? {
            TokenKind::Sit => Some(FieldModifier::Fixum),
            TokenKind::Varia => Some(FieldModifier::Varia),
            TokenKind::Fixum => Some(FieldModifier::Const),
            _ => None,
        }
    }

    /// The named diagnostic for a leading `generis` (retired) or `de`
    /// (refused) field word, with the modifier it is read as. A `de` after a
    /// modifier is not a modifier at all: it is the access marker of the
    /// field's type (`varia de Node next`).
    fn retired_field_modifier_issue(
        &self,
        genus_member: bool,
    ) -> Option<(&'static str, FieldModifier)> {
        match self.current_keyword_ident()? {
            TokenKind::Generis => Some(("static_member_retired", FieldModifier::Const)),
            TokenKind::De if genus_member => Some(("ref_field_refused", FieldModifier::Fixum)),
            _ => None,
        }
    }

    /// Consume one leading field modifier, if present; the flag is true when
    /// the word was retired or refused rather than a live modifier.
    ///
    /// GRAMMAR:
    ///   field-decl := ('sit' | 'varia' | 'fixum') type ident 'sponte'? ('=' expr)?
    ///
    /// Exactly one modifier: a second one (`fixum sit`, `fixum varia`,
    /// `sit varia`) is rejected here; a missing one on a `genus` field is
    /// rejected by `parse_class_member`. `discretio` shared fields stay
    /// unmarked (fork F7 held), so the modifier is optional at this level.
    ///
    /// A retired `generis` or refused `de` word is recorded as `PARSE010
    /// static_member_retired` / `ref_field_refused` and read as the modifier it
    /// became (the constant, the read-only field), so recovery resumes after
    /// the field.
    fn eat_field_modifier(&mut self, genus_member: bool) -> Option<(FieldModifier, bool)> {
        if let Some((issue, read_as)) = self.retired_field_modifier_issue(genus_member) {
            let span = self.current_span();
            let keyword = self.current_word();
            self.advance();
            self.errors.push(
                ParseError::structured(ParseErrorKind::DeclarationInvalid, span)
                    .with_arg("issue", issue)
                    .with_arg("keyword", keyword),
            );
            return Some((read_as, true));
        }
        let modifier = self.current_field_modifier()?;
        self.advance();
        Some((modifier, false))
    }

    /// Whether the current word is the removed `nexum` field modifier.
    ///
    /// `nexum` was removed (DECISIONS D16.2). The old shape was
    /// `nexum <type> <name>`: two further words on the same line, or a
    /// generic type head. A field of a user type spelled `nexum`
    /// (`nexum name`) stays legal. Matching the canonical Latin spelling is
    /// deliberate: the keyword no longer has pack rows.
    fn at_removed_nexum_modifier(&self) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        if self.interner.resolve(sym) != "nexum"
            || !matches!(self.peek_at(1).kind, TokenKind::Ident(_))
        {
            return false;
        }
        match self.peek_at(2).kind {
            TokenKind::LtAscii => true,
            TokenKind::Ident(_) => {
                let slot = self.non_trivia_indices.partition_point(|&i| i < self.pos);
                let (Some(&ty_idx), Some(&name_idx)) = (
                    self.non_trivia_indices.get(slot + 1),
                    self.non_trivia_indices.get(slot + 2),
                ) else {
                    return false;
                };
                self.keyword_ident_at(2) != Some(TokenKind::Sponte)
                    && !self.tokens[ty_idx + 1..name_idx]
                        .iter()
                        .any(|token| token.kind == TokenKind::Newline)
            }
            _ => false,
        }
    }

    /// Parse a class member, preserving annotations at the member boundary.
    fn parse_class_member(&mut self) -> Result<ClassMember, ParseError> {
        let leading_trivia = self.parse_leading_trivia();
        let start = self.current_span();

        let mut annotations = self.parse_annotations()?;

        let kind = if self.check_keyword_ident(TokenKind::Functio) {
            // An `@ nondum` method may omit its block, like a free function.
            self.nondum_function_pending = annotations
                .iter()
                .any(|annotation| self.is_nondum_annotation(annotation));
            let parsed = self.parse_func_decl();
            let StmtKind::Func(func) = parsed? else {
                return Err(self.expected_issue_with_keywords(
                    "expected_method_declaration",
                    self.current_span(),
                    &["functio"],
                ));
            };
            ClassMemberKind::Method(func)
        } else if self.at_kernel_decl_head() {
            // A kernel method head: same rule as a `functio` method, with the
            // head's marker folded into the member's annotation stack.
            self.nondum_function_pending = annotations
                .iter()
                .any(|annotation| self.is_nondum_annotation(annotation));
            let (func, head) = self.parse_kernel_decl()?;
            annotations = self.merge_kernel_head_annotation(annotations, head);
            ClassMemberKind::Method(func)
        } else {
            self.check_nucleum_tier_target(&annotations);
            let field_start = self.current_span();
            let field = self.parse_field_decl(true)?;
            if field.modifier.is_none() {
                // D16.1: every genus field declares exactly one modifier.
                // Recoverable, so one run reports every unmarked field; the
                // migration is `faber field-modifier-codemod`.
                self.errors.push(
                    ParseError::structured(
                        ParseErrorKind::DeclarationInvalid,
                        field_start.merge(self.previous_span()),
                    )
                    .with_arg("issue", "field_modifier_missing"),
                );
            }
            ClassMemberKind::Field(field)
        };

        let span = start.merge(self.previous_span());
        Ok(ClassMember {
            leading_trivia,
            annotations,
            kind,
            span,
        })
    }

    // INTERFACE DECLARATIONS

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
        self.expect_keyword_ident(TokenKind::Implendum)?;

        let name = self.parse_decl_name()?;
        let type_params = self.parse_generic_params()?;

        self.expect_issue(&TokenKind::LBrace, "interface_body_start")?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let leading_trivia = self.parse_leading_trivia();
            let annotations = self.parse_annotations()?;
            // An interface signature declares a contract, not a kernel.
            self.check_nucleum_tier_target(&annotations);
            let start = self.current_span();

            self.expect_keyword_ident(TokenKind::Functio)?;
            let method_name = self.parse_member_ident()?;

            self.expect_issue(&TokenKind::LParen, "interface_method_parameters_start")?;
            let params = self.parse_param_list()?;
            self.expect_issue(&TokenKind::RParen, "interface_method_parameters_end")?;

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

        self.expect_issue(&TokenKind::RBrace, "interface_body_end")?;

        Ok(StmtKind::Interface(InterfaceDecl {
            name,
            type_params,
            methods,
        }))
    }

    /// Parse a type alias declaration.
    fn parse_type_alias_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Typus)?;
        let name = self.parse_decl_name()?;
        let type_params = self.parse_generic_params()?;
        self.expect_issue(&TokenKind::Eq, "type_alias_value_separator")?;
        let ty = self.parse_type()?;

        Ok(StmtKind::TypeAlias(TypeAliasDecl {
            name,
            type_params,
            ty,
            annotations: Vec::new(),
        }))
    }

    // SCHEMA DECLARATIONS

    /// Parse a relational-view schema declaration.
    ///
    /// GRAMMAR:
    ///   schema-decl := `schema` ident '{' schema-column* '}'
    ///   schema-column := `columna` type ident [':' ident]
    ///
    /// A schema is a partial application-owned relational heading
    /// (docs/design/relational-views.md §2): it names only the columns the
    /// application needs, and extra source columns stay invisible. Column
    /// types reuse the ordinary type parser, so `T ∪ nihil` spells nullable
    /// columns through the type position. The optional
    /// `publicName : sourceName` alias row maps a public column to a source
    /// column; an absent alias is the identity mapping.
    ///
    /// Grammar-level rejections use the same issue keys as later semantic
    /// checks: method-like members (`schema_method`), `sub`/`implet`
    /// inheritance (`schema_inheritance`), and nested column declarations
    /// (`schema_nested_column`) fail here at parse time.
    fn parse_schema_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Schema)?;
        let name = self.parse_decl_name()?;

        // Grammar-level rejection: `sub`/`implet` inheritance syntax on a
        // schema is not a header surface (schemas have no inheritance).
        if self.current_ident_is(RETIRED_SUB) || self.check_keyword_ident(TokenKind::Implet) {
            return Err(ParseError::structured(
                ParseErrorKind::DeclarationInvalid,
                self.current_span(),
            )
            .with_arg("issue", "schema_inheritance"));
        }

        self.expect_issue(&TokenKind::LBrace, "schema_body_start")?;

        let mut columns = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let leading_trivia = self.parse_leading_trivia();
            let start = self.current_span();

            // Grammar-level rejections, checked before the `columna` slot so a
            // malformed member fails with the pinned issue key instead of a
            // generic "expected 'columna'".
            if self.check_keyword_ident(TokenKind::Functio) {
                return Err(ParseError::structured(
                    ParseErrorKind::DeclarationInvalid,
                    self.current_span(),
                )
                .with_arg("issue", "schema_method"));
            }
            if self.current_ident_is(RETIRED_SUB) || self.check_keyword_ident(TokenKind::Implet) {
                return Err(ParseError::structured(
                    ParseErrorKind::DeclarationInvalid,
                    self.current_span(),
                )
                .with_arg("issue", "schema_inheritance"));
            }

            self.expect_keyword_ident(TokenKind::Columna)?;

            // Nested column declaration: a `columna` keyword in the type slot
            // (`columna columna …`) means a column nested inside a column.
            if self.check_keyword_ident(TokenKind::Columna) {
                return Err(ParseError::structured(
                    ParseErrorKind::DeclarationInvalid,
                    self.current_span(),
                )
                .with_arg("issue", "schema_nested_column"));
            }

            let ty = self.parse_type()?;
            let column_name = self.parse_ident()?;

            // Nested column declaration: a second `columna` on the same logical
            // line where the alias row (or the next column) belongs — no column
            // carries a nested column list. A `columna` on a fresh line is the
            // next column, not a nested declaration (newlines are trivia, so
            // the raw-token scan distinguishes the two). The scan runs from
            // the raw position through any trailing `#` comment to the next
            // word, so a comment ending the column's row cannot masquerade as
            // a same-line `columna`.
            if self.check_keyword_ident(TokenKind::Columna) && !self.raw_newline_before_next_word()
            {
                return Err(ParseError::structured(
                    ParseErrorKind::DeclarationInvalid,
                    self.current_span(),
                )
                .with_arg("issue", "schema_nested_column"));
            }

            let source_alias = if self.eat(&TokenKind::Colon) {
                Some(self.parse_ident()?)
            } else {
                None
            };

            let span = start.merge(self.previous_span());
            columns.push(SchemaColumn {
                leading_trivia,
                ty,
                name: column_name,
                source_alias,
                span,
            });
        }

        // Trivia immediately before the consuming `}` has no following column
        // to attach to; retain it on the schema-end boundary so the post-parse
        // completeness check can claim it (mirrors `parse_class_decl`).
        let end_trivia = self.parse_block_end_trivia();

        self.expect_issue(&TokenKind::RBrace, "schema_body_end")?;

        Ok(StmtKind::Schema(SchemaDecl {
            name,
            columns,
            end_trivia,
        }))
    }

    // ENUM AND UNION DECLARATIONS

    /// Parse enum declaration.
    ///
    /// GRAMMAR:
    ///   enum-decl := `ordo` ident '{' enum-member* '}'
    ///   enum-member := ident ['=' (integer | string)]
    fn parse_enum_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Ordo)?;
        let name = self.parse_decl_name()?;
        self.reject_sum_type_implet()?;

        self.expect_issue(&TokenKind::LBrace, "enum_body_start")?;

        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let leading_trivia = self.parse_leading_trivia();
            if self.skip_sum_type_function(None)? {
                continue;
            }
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
                                .with_arg("issue", "enum_value_out_of_range"));
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
                                    .with_arg("issue", "enum_value_out_of_range"));
                                }
                            }
                        } else {
                            return Err(ParseError::structured(
                                ParseErrorKind::Expected,
                                self.current_span(),
                            )
                            .with_arg("expected", "number")
                            .with_arg("issue", "enum_member_value_number"));
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
                        .with_arg("expected", "value")
                        .with_arg("issue", "enum_member_value"));
                    }
                }
            } else {
                None
            };

            let span = start.merge(self.previous_span());
            members.push(EnumMember {
                leading_trivia,
                name: member_name,
                value,
                span,
            });

            // A misplaced `functio` member reports its own D9.1 diagnostic
            // rather than a missing separator before it.
            if !self.check_keyword_ident(TokenKind::Functio)
                && !self.consume_item_separator(&TokenKind::RBrace, "enum_members")?
            {
                break;
            }
        }

        self.expect_issue(&TokenKind::RBrace, "enum_body_end")?;

        Ok(StmtKind::Enum(EnumDecl { name, members }))
    }

    /// Parse tagged union declaration.
    ///
    /// GRAMMAR:
    ///   union-decl := discretio ident [type-params] { union-member* variant* }
    ///   union-member := annotation* `field_decl`
    ///   variant := ident [ { field* } ]
    ///   field := type ident
    fn parse_union_decl(&mut self) -> Result<StmtKind, ParseError> {
        self.expect_keyword_ident(TokenKind::Discretio)?;
        let name = self.parse_decl_name()?;
        let type_params = self.parse_generic_params()?;
        self.reject_sum_type_implet()?;

        self.expect_issue(&TokenKind::LBrace, "union_body_start")?;

        let mut shared_fields = Vec::new();
        let mut variants = Vec::new();
        let mut declaration_region_started = false;
        let mut parsing_variants = false;
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let leading_trivia = self.parse_leading_trivia();
            if self.skip_sum_type_function(None)? {
                continue;
            }

            if !parsing_variants {
                let start = self.current_span();
                if self.check(&TokenKind::At) {
                    declaration_region_started = true;
                    let annotations = self.parse_annotations()?;
                    if self.skip_sum_type_function(Some(start))? {
                        continue;
                    }
                    self.check_nucleum_tier_target(&annotations);
                    let field = self.parse_field_decl(false)?;
                    shared_fields.push(UnionField {
                        leading_trivia,
                        annotations,
                        field,
                        span: start.merge(self.previous_span()),
                    });
                    continue;
                }

                if declaration_region_started {
                    let checkpoint = self.checkpoint();
                    if let Ok(field) = self.parse_field_decl(false) {
                        shared_fields.push(UnionField {
                            leading_trivia,
                            annotations: Vec::new(),
                            field,
                            span: start.merge(self.previous_span()),
                        });
                        continue;
                    }
                    self.restore(checkpoint);
                }
            }

            parsing_variants = true;
            let start = self.current_span();
            let variant_name = self.parse_ident()?;

            let fields = if self.eat(&TokenKind::LBrace) {
                let mut fields = Vec::new();
                while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                    let field_start = self.current_span();
                    let ty = self.parse_type()?;
                    let field_name = self.parse_ident()?;
                    if self.variant_redeclares_commune_field(field_name.name, &shared_fields) {
                        return Err(ParseError::structured(
                            ParseErrorKind::Expected,
                            field_name.span,
                        )
                        .with_arg("issue", "union_variant_redeclares_shared_field"));
                    }
                    let field_span = field_start.merge(self.previous_span());
                    fields.push(VariantField {
                        ty,
                        name: field_name,
                        span: field_span,
                    });
                    self.reject_declaration_block_comma("discretio_payload")?;
                }
                self.expect_issue(&TokenKind::RBrace, "union_variant_fields_end")?;
                fields
            } else {
                Vec::new()
            };

            let span = start.merge(self.previous_span());
            variants.push(Variant {
                leading_trivia,
                name: variant_name,
                fields,
                span,
            });

            // A misplaced `functio` member reports its own D9.1 diagnostic
            // rather than a missing separator before it.
            if !self.check_keyword_ident(TokenKind::Functio)
                && !self.consume_item_separator(&TokenKind::RBrace, "discretio_variants")?
            {
                break;
            }
        }

        self.expect_issue(&TokenKind::RBrace, "union_body_end")?;

        Ok(StmtKind::Union(UnionDecl {
            name,
            type_params,
            shared_fields,
            variants,
        }))
    }

    /// D9.1: `ordo` and `discretio` are data only, so an `implet` clause on
    /// the header is rejected with a pointed diagnostic before the body.
    fn reject_sum_type_implet(&self) -> Result<(), ParseError> {
        if self.check_keyword_ident(TokenKind::Implet) {
            return Err(ParseError::structured(
                ParseErrorKind::DeclarationInvalid,
                self.current_span(),
            )
            .with_arg("issue", "sum_type_implements"));
        }
        Ok(())
    }

    /// D9.1: a `functio` inside a sum-type body is parsed whole so recovery
    /// resumes at the next member, then dropped with a pointed diagnostic.
    /// Returns `true` when a function member was consumed.
    /// `annotated_from` is the span where preceding member annotations began.
    fn skip_sum_type_function(&mut self, annotated_from: Option<Span>) -> Result<bool, ParseError> {
        if !self.check_keyword_ident(TokenKind::Functio) {
            return Ok(false);
        }
        let start = annotated_from.unwrap_or_else(|| self.current_span());
        self.parse_func_decl()?;
        self.errors.push(
            ParseError::structured(
                ParseErrorKind::DeclarationInvalid,
                start.merge(self.previous_span()),
            )
            .with_arg("issue", "sum_type_function"),
        );
        self.eat(&TokenKind::Comma);
        Ok(true)
    }

    fn variant_redeclares_commune_field(
        &self,
        field_name: radix_lexer::Symbol,
        shared_fields: &[UnionField],
    ) -> bool {
        shared_fields.iter().any(|shared| {
            shared.field.name.name == field_name
                && shared
                    .annotations
                    .iter()
                    .any(|annotation| self.annotation_is_commune(annotation))
        })
    }

    fn annotation_is_commune(&self, annotation: &Annotation) -> bool {
        matches!(
            &annotation.kind,
            AnnotationKind::Statement(statement)
                if statement.args.is_empty()
                    && self.interner.resolve(statement.name.name) == "commune"
        )
    }

    // IMPORT DECLARATIONS

    /// Parse a source import declaration.
    ///
    /// GRAMMAR:
    ///   import-decl := `importa` `ex` string [`publica`] import-kind
    ///   import-kind := ident [`ut` ident] | '*' `ut` ident
    ///                | `fixum` import-value-binding (`,` import-value-binding)* `,`?
    ///   import-value-binding := ident [`ut` ident]
    ///
    /// The parser only records the import specifier, visibility, and binding
    /// shape. Package and library resolution decide later whether the string
    /// names a local module, stdlib interface, or invalid provider path.
    fn parse_import_decl(&mut self) -> Result<StmtKind, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Importa)?;

        let target = self.parse_import_target()?;
        let (path, visibility, kind) = if target.is_none() && self.check(&TokenKind::LBrace) {
            self.parse_import_record()?
        } else {
            self.parse_import_sugar(target.is_some())?
        };

        let span = start.merge(self.previous_span());
        Ok(StmtKind::Import(ImportDecl {
            path,
            visibility,
            kind,
            target,
            span,
        }))
    }

    /// Whether the cursor sits on a target-import slot: a plain word (never a
    /// keyword of the reader pack) directly followed by the specifier string,
    /// where `from` / `{` would otherwise stand (`import python "math" m`).
    fn at_import_target_slot(&self, offset: usize) -> bool {
        matches!(self.peek_at(offset).kind, TokenKind::Ident(_))
            && self.keyword_ident_at(offset).is_none()
            && matches!(self.peek_at(offset + 1).kind, TokenKind::String(_))
    }

    /// Parse the optional target word of a target-native import.
    ///
    /// GRAMMAR:
    ///   target-import := `importa` target string [`publica`] import-kind
    ///
    /// The target word is a target CLI name, contextual in this slot only and
    /// never translated. A word that names no target is `PARSE050
    /// import_target_unknown` with a did-you-mean over the target names.
    fn parse_import_target(&mut self) -> Result<Option<Ident>, ParseError> {
        if !self.at_import_target_slot(0) {
            return Ok(None);
        }
        let word = self.parse_ident()?;
        let spelled = self.interner.resolve(word.name).to_owned();
        if radix_target::Target::from_import_word(&spelled).is_none() {
            let mut error = Self::invalid_import_error_at("import_target_unknown", word.span)
                .with_arg("target", spelled.clone());
            if let Some(near) = radix_target::Target::nearest_import_word(&spelled) {
                error = error.with_arg("suggestion", near);
            }
            return Err(error);
        }
        Ok(Some(word))
    }

    fn parse_import_sugar(
        &mut self,
        targeted: bool,
    ) -> Result<(radix_lexer::Symbol, Visibility, ImportKind), ParseError> {
        if !targeted {
            self.expect_keyword_ident(TokenKind::Ex)?;
        }

        let path = self.parse_string()?;
        let path_span = self.previous_span();
        if self.interner.resolve(path).is_empty() {
            return Err(Self::invalid_import_error_at(
                "import_name_inference_failed",
                path_span,
            ));
        }

        if self.check_keyword_ident(TokenKind::Privata) {
            return Err(Self::removed_import_visibility_error(self.current_span()));
        }
        let explicit_visibility = self.check_keyword_ident(TokenKind::Publica);
        let visibility = if self.eat_keyword_ident(TokenKind::Publica) {
            Visibility::Publica
        } else {
            Visibility::Privata
        };

        let kind = if self.eat(&TokenKind::Star) {
            self.expect_keyword_ident(TokenKind::Ut)?;
            let alias = self.parse_ident()?;
            ImportKind::Wildcard { alias }
        } else if self.check_keyword_ident(TokenKind::Fixum)
            && self.import_fixum_starts_selective_list()
        {
            self.eat_keyword_ident(TokenKind::Fixum);
            ImportKind::Selective {
                members: self.parse_selective_import_members()?,
            }
        } else if Self::is_import_binding_name(&self.peek().kind)
            && !self.at_following_import_decl()
            && (explicit_visibility || !self.import_binding_continues_expression())
            && (explicit_visibility || !matches!(self.peek().kind, TokenKind::Tag))
        {
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
        } else if targeted {
            // A target specifier is the target's own module name; Faber never
            // infers a binding from it.
            return Err(Self::invalid_import_error_at(
                "import_target_binding_required",
                self.current_span(),
            ));
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
        self.expect_issue(&TokenKind::LBrace, "import_record_start")?;

        let mut path = None;
        let mut path_span = None;
        let mut visibility = Visibility::Privata;
        let mut name = None;
        let mut alias = None;
        let mut wildcard_alias = None;
        let mut seen_fields = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let field = self.parse_import_field_name()?;
            let field_name = self.interner.resolve(field.name).to_owned();
            self.expect_issue(&TokenKind::Eq, "import_field_value_separator")?;

            // WHY: unknown names error below on first sight, so a repeat here
            // is always a known field; last-wins would silently drop the first.
            if seen_fields.contains(&field.name) {
                return Err(
                    Self::invalid_import_error_at("duplicate_import_field", field.span)
                        .with_arg("field", field_name),
                );
            }
            seen_fields.push(field.name);

            // Field names are keyword identities resolved through the reader
            // pack, so only the active locale's own words are accepted.
            match self.import_field_identity(&field_name) {
                Some("ex") => {
                    path = Some(self.parse_string()?);
                    path_span = Some(self.previous_span());
                }
                Some("visibilitas") => {
                    visibility = self.parse_import_visibility_value()?;
                }
                Some("nomen") => {
                    name = Some(self.parse_decl_name()?);
                }
                Some("ut") => {
                    alias = Some(self.parse_ident()?);
                }
                Some("omnia") => {
                    wildcard_alias = Some(self.parse_ident()?);
                }
                _ => return Err(self.unknown_import_field_error(&field_name, field.span)),
            }

            if !self.consume_item_separator(&TokenKind::RBrace, "import_fields")? {
                break;
            }
        }

        self.expect_issue(&TokenKind::RBrace, "import_record_end")?;

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
            TokenKind::Ex => Ok(self.keyword_ident(&self.keyword_surface("ex"), span)),
            TokenKind::Ut => Ok(self.keyword_ident(&self.keyword_surface("ut"), span)),
            _ => Err(ParseError::structured(ParseErrorKind::Expected, span)
                .with_arg("expected", "identifier")
                .with_arg("issue", "import_field_name")),
        }
    }

    /// The keyword identity an import record field word claims on the active
    /// reader surface, if any.
    ///
    /// Resolves through the pack (`from` is `ex` in en), so a word the pack does
    /// not declare for one of the five field identities claims nothing.
    fn import_field_identity(&self, word: &str) -> Option<&'static str> {
        let key = self.keyword_claim_key_for_text(word)?;
        IMPORT_FIELD_KEYS
            .iter()
            .copied()
            .find(|field| *field == key)
    }

    /// `unknown_import_field`, or `wrong_locale_import_field` when the word is
    /// a canonical field spelling the active pack re-spells.
    fn unknown_import_field_error(&self, word: &str, span: radix_lexer::Span) -> ParseError {
        if let Some(latin) = IMPORT_FIELD_KEYS
            .iter()
            .copied()
            .find(|field| *field == word)
        {
            let expected = self.keyword_surface(latin);
            if expected != word {
                return Self::invalid_import_error_at("wrong_locale_import_field", span)
                    .with_arg("field", word)
                    .with_arg("expected", expected);
            }
        }
        Self::invalid_import_error_at("unknown_import_field", span)
    }

    fn parse_import_visibility_value(&mut self) -> Result<Visibility, ParseError> {
        if self.eat_keyword_ident(TokenKind::Publica) {
            Ok(Visibility::Publica)
        } else {
            Err(self.invalid_import_error("invalid_import_visibility"))
        }
    }

    /// Whether the word after an import source opens the next import
    /// (`importa ex …` / `importa {`) rather than naming this one's binding.
    ///
    /// Without this, a default-named import followed by another import
    /// swallowed the next line's `importa` as its binding (D7.7 makes
    /// back-to-back default imports the ordinary shape).
    fn at_following_import_decl(&self) -> bool {
        self.check_keyword_ident(TokenKind::Importa)
            && (self.keyword_ident_at(1) == Some(TokenKind::Ex)
                || self.peek_at(1).kind == TokenKind::LBrace
                || self.at_import_target_slot(1))
    }

    /// Whether a token may occupy an import binding name slot.
    ///
    /// Declaration name slots accept every keyword spelling, matching
    /// `parse_decl_name`; `ut` is excluded only in the selective member parser,
    /// where it is the alias marker.
    fn is_import_binding_name(kind: &TokenKind) -> bool {
        match kind {
            TokenKind::Ident(_) | TokenKind::Underscore(_) | TokenKind::Tag => true,
            kind => keyword_allowed_as_ident(KeywordOwner::DeclarationName, kind).is_some(),
        }
    }

    /// Whether a `fixum` keyword after the import source opens a selective
    /// member list instead of starting a following `fixum` statement.
    ///
    /// The following-statement reading wins when the token after `fixum` is a
    /// name followed by an expression continuation (`sit count ← …`, typed
    /// declarations, postfix access). The selective reading wins when the list
    /// is closed or continued (`fixum` at EOF, `fixum dic,`), a wildcard or
    /// orphaned `ut` member is attempted, or the first member is followed by
    /// an alias or a declaration keyword.
    fn import_fixum_starts_selective_list(&self) -> bool {
        match &self.peek_at(1).kind {
            TokenKind::Eof | TokenKind::Comma | TokenKind::Star | TokenKind::Ut => true,
            // `ut` lexes as an identifier and claims the alias marker by
            // identity; a spelled `fixum … ut …` is a member with no name.
            TokenKind::Ident(_) if self.keyword_ident_at(1) == Some(TokenKind::Ut) => true,
            kind if Self::is_import_binding_name(kind) => match &self.peek_at(2).kind {
                TokenKind::Ut | TokenKind::Eof | TokenKind::Comma | TokenKind::Star => true,
                TokenKind::Ident(_) => self.keyword_ident_at(2).is_some(),
                _ => false,
            },
            _ => false,
        }
    }

    fn parse_selective_import_members(&mut self) -> Result<Vec<ImportValueBinding>, ParseError> {
        let mut members = Vec::new();
        loop {
            if members.is_empty() && !Self::is_import_binding_name(&self.peek().kind) {
                // The first member is mandatory; an empty list is malformed.
                return Err(match self.peek().kind {
                    TokenKind::Star => {
                        self.invalid_import_error("selective_import_wildcard_mixing")
                    }
                    _ => self.invalid_import_error("selective_import_missing_member"),
                });
            }
            members.push(self.parse_import_value_binding()?);
            if !self.eat(&TokenKind::Comma) {
                if matches!(self.peek().kind, TokenKind::Star) {
                    return Err(self.invalid_import_error("selective_import_wildcard_mixing"));
                }
                if self.import_value_member_starts_here() {
                    return Err(self.missing_import_separator_error());
                }
                break;
            }
            if !self.import_value_member_starts_here() {
                break; // trailing comma
            }
        }
        Ok(members)
    }

    /// Whether the current token can start another selective member after a
    /// comma. Keywords are excluded so a following declaration is not consumed
    /// as a member after a trailing comma; `*` is admitted so a wildcard
    /// attempt inside the list reports the structured mixing diagnostic.
    fn import_value_member_starts_here(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Ident(sym) => self
                .keyword_claim_key(*sym)
                .and_then(lookup_keyword_spec)
                .is_none(),
            TokenKind::Underscore(_) | TokenKind::Star => true,
            _ => false,
        }
    }

    fn parse_import_value_binding(&mut self) -> Result<ImportValueBinding, ParseError> {
        let start = self.current_span();
        let name = self.parse_import_value_binding_name()?;
        let alias = if self.eat_keyword_ident(TokenKind::Ut) {
            Some(self.parse_import_value_binding_name()?)
        } else {
            None
        };
        let span = start.merge(self.previous_span());
        Ok(ImportValueBinding { name, alias, span })
    }

    fn parse_import_value_binding_name(&mut self) -> Result<Ident, ParseError> {
        let kind = &self.peek().kind;
        if matches!(kind, TokenKind::Star) {
            return Err(self.invalid_import_error("selective_import_wildcard_mixing"));
        }
        if self.is_import_alias_marker() || !Self::is_import_binding_name(kind) {
            return Err(self.invalid_import_error("selective_import_binding_expected"));
        }
        self.parse_decl_name()
    }

    /// Whether the current token is the `ut` alias marker — as a dedicated
    /// token or by keyword identity — which is not a legal member name.
    fn is_import_alias_marker(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ut) || self.keyword_ident_at(0) == Some(TokenKind::Ut)
    }

    /// Whether the token after a candidate import binding continues an
    /// expression, making the identifier the start of a following statement.
    ///
    /// A named binding is followed by `ut` (alias), a keyword or `@` (next
    /// declaration), or EOF. A continuation token (`.`, `(`, `[`, operators,
    /// assignment) or an ordinary identifier means the word belongs to a
    /// following statement, so the import binding is inferred from the path
    /// instead.
    fn import_binding_continues_expression(&self) -> bool {
        match &self.peek_at(1).kind {
            TokenKind::Ut | TokenKind::Eof | TokenKind::At | TokenKind::Tag => false,
            TokenKind::Ident(sym) => {
                let Some(latin) = self.keyword_claim_key(*sym) else {
                    return true;
                };
                lookup_keyword_spec(latin)
                    .and_then(|spec| spec.token_kind.as_ref())
                    .is_none()
            }
            _ => true,
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
            || Self::inferred_import_name_is_reserved(&segment)
        {
            return Err(
                Self::invalid_import_error_at("import_name_inference_failed", span)
                    .with_arg("path", path_text.clone()),
            );
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

    fn inferred_import_name_is_reserved(name: &str) -> bool {
        lookup_keyword_spec(name)
            .and_then(|spec| spec.token_kind.as_ref())
            .is_some_and(|kind| {
                keyword_allowed_as_ident(KeywordOwner::DeclarationName, kind).is_none()
            })
    }

    fn invalid_import_error(&self, issue: &'static str) -> ParseError {
        Self::invalid_import_error_at(issue, self.current_span())
    }

    fn missing_import_separator_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::Expected, self.current_span())
            .with_arg("issue", "item_list_missing_separator")
            .with_arg("family", "item_list")
            .with_arg("position", "selective_import_members")
            .with_arg("expected", "comma between items")
    }

    fn invalid_import_error_at(issue: &'static str, span: radix_lexer::Span) -> ParseError {
        ParseError::structured(ParseErrorKind::ImportInvalid, span).with_arg("issue", issue)
    }

    fn removed_import_visibility_error(span: radix_lexer::Span) -> ParseError {
        Self::invalid_import_error_at("import_privata_removed", span)
    }

    // TEST DECLARATIONS

    /// Parse test suite declaration.
    ///
    /// GRAMMAR:
    ///   probandum := `probandum` string proba-modifier* '{' probandum-body '}'
    ///   probandum-body := (setup | proba | nested-probandum)*
    fn parse_probandum_decl(&mut self) -> Result<StmtKind, ParseError> {
        let start = self.current_span();
        self.expect_keyword_ident(TokenKind::Probandum)?;

        let name = self.parse_string()?;
        let modifiers = self.parse_test_modifiers()?;

        self.expect_issue(&TokenKind::LBrace, "probandum_body_start")?;

        let body = self.parse_probandum_body()?;

        self.expect_issue(&TokenKind::RBrace, "probandum_body_end")?;

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
                    return Err(self.expected_issue_with_keywords(
                        "expected_praepara_hook",
                        self.current_span(),
                        &["praepara", "praeparabit", "postpara", "postparabit"],
                    ));
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
        self.expect_keyword_ident(TokenKind::Proba)?;

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
            if self.eat_keyword_ident(TokenKind::Erratur) {
                modifiers.push(ProbaModifier::Erratur);
            } else if self.eat_keyword_ident(TokenKind::Omitte) {
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

    // ANNOTATIONS

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
            self.advance();

            let kind = self.parse_annotation_kind()?;
            let span = start.merge(self.previous_span());

            self.require_newline_after_bare_annotation(&kind)?;
            annotations.push(Annotation { kind, span });
        }

        self.check_nucleum_annotation_conflicts(&annotations)?;
        Ok(annotations)
    }

    /// GRAMMAR: [043]/[045] end a bare `annotation_sugar` at NEWLINE, so per
    /// [004] the annotated declaration must start on the next physical line —
    /// in same-line text the only derivation swallows the declaration head
    /// into the `NON_NEWLINE_TOKEN*` payload.
    fn require_newline_after_bare_annotation(
        &mut self,
        kind: &AnnotationKind,
    ) -> Result<(), ParseError> {
        if !self.annotation_is_bare_sugar(kind)
            || self.is_at_end()
            || self.raw_newline_before_next_word()
        {
            return Ok(());
        }
        Err(self
            .invalid_annotation_error("unexpected_annotation_token")
            .with_arg("context", self.bare_annotation_context(kind)))
    }

    /// Whether the annotation is sugar with no payload and no braced body, so
    /// its production names NEWLINE as the terminator. Payload sugar ([043]
    /// `NON_NEWLINE_TOKEN*`, preserved for statement annotations whose block
    /// opens on the annotation's own line) and braced forms ([040], ending at
    /// `}`) do not.
    fn annotation_is_bare_sugar(&self, kind: &AnnotationKind) -> bool {
        match kind {
            AnnotationKind::Statement(stmt) => stmt.args.is_empty(),
            AnnotationKind::Braced(_)
            | AnnotationKind::Cli(_)
            | AnnotationKind::Imperium(_)
            | AnnotationKind::Optio(_)
            | AnnotationKind::Operandus(_)
            | AnnotationKind::RadixTypusDomain(_)
            | AnnotationKind::RadixLane(_) => false,
            // Braced `@ nucleum { … }` shares the sugar variant; a nucleum is
            // bare only when the form did not end at `}`.
            AnnotationKind::Nucleum(_) => !matches!(
                self.tokens.get(self.pos - 1).map(|t| &t.kind),
                Some(TokenKind::RBrace)
            ),
            // Markers (`@ publica`, `@ conversio`, …) carry no payload.
            _ => true,
        }
    }

    /// Canonical `@ name` context for the annotation-line diagnostic, using
    /// the same latin family identity the AST keeps. Only bare-sugar kinds
    /// reach this helper.
    fn bare_annotation_context(&self, kind: &AnnotationKind) -> String {
        let name = match kind {
            AnnotationKind::Statement(stmt) => self.interner.resolve(stmt.name.name),
            AnnotationKind::Nucleum(_) => "nucleum",
            AnnotationKind::Publica => "publica",
            AnnotationKind::Protecta => "protecta",
            AnnotationKind::Privata => "privata",
            AnnotationKind::Interna => "interna",
            AnnotationKind::Conversio => "conversio",
            AnnotationKind::Ad(_) => "ad",
            _ => "annotation",
        };
        format!("@ {name}")
    }

    /// Return the active pack's spelling when `raw` is a registered annotation
    /// family written in its undeclared Latin form.
    ///
    /// Ordinary extension annotations remain statement annotations. A spelling
    /// that the registry already identifies as annotation-owned is different:
    /// accepting its canonical Latin form under a translated pack would make a
    /// mixed-locale file look valid while bypassing the pack surface gate.
    fn untranslated_annotation_spelling(&self, raw: &str) -> Option<String> {
        // `conversion` is the English surface of the canonical `conversio`
        // annotation. In registry-direct (Latin) mode the raw fallback would
        // otherwise claim this undeclared alias before the locale gate sees it.
        if raw == "conversion"
            && self.reader_keyword_latin_keys.get(raw).map(String::as_str) != Some("conversio")
        {
            return self
                .reader_keyword_surfaces
                .get("conversio")
                .cloned()
                .or_else(|| Some("conversio".to_owned()));
        }

        if self.keyword_claim_key_for_text(raw).is_some() {
            return None;
        }

        let spec = lookup_keyword_spec(raw)?;
        let annotation_owned = matches!(spec.category, KeywordKind::Annotation)
            || matches!(
                spec.scope,
                KeywordScope::Contextual(owners)
                    if owners.contains(&KeywordOwner::AnnotationName)
            );
        if !annotation_owned {
            return None;
        }

        self.reader_keyword_surfaces
            .get(spec.text)
            .cloned()
            .or_else(|| Some(spec.text.to_owned()))
    }

    fn parse_annotation_kind(&mut self) -> Result<AnnotationKind, ParseError> {
        // Capture the annotation-name spelling as authored: the dispatch below
        // is surface-gated, so it must see the spelling before
        // `parse_annotation_name` canonicalizes declared spellings to their
        // latin key.
        let raw_annotation_name = match self.peek().kind {
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => {
                Some(self.interner.resolve(sym).to_owned())
            }
            _ => None,
        };
        if let Some(raw) = raw_annotation_name.as_deref()
            && let Some(expected) = self.untranslated_annotation_spelling(raw)
        {
            return Err(self
                .invalid_annotation_error("wrong_locale_annotation")
                .with_arg("annotation", raw)
                .with_arg("expected", expected));
        }
        let name = self.parse_annotation_name()?;

        // Dispatch on the consumed annotation name's keyword identity (any
        // reader locale), not the current token (which is already past it).
        // The claim routes through the pack-authoritative surface gate: a
        // keyword annotation dispatches only for spellings the active surface
        // declared (registry-direct surfaces resolve every spelling). Token-
        // shaped keyword tokens (hand-built streams) are declared by
        // construction.
        let annotation_identity = match raw_annotation_name.as_deref() {
            Some(raw) => self.keyword_claim_key_for_text(raw),
            None => Some(self.interner.resolve(name.name)),
        }
        .and_then(|latin| lookup_keyword_spec(latin).and_then(|spec| spec.token_kind.clone()));

        // Nucleum is structured in both sugar and braced forms so the fragment
        // modifier is typed data, not an untyped argument or a raw braced field.
        if matches!(annotation_identity, Some(TokenKind::Nucleum)) {
            return self.parse_nucleum_annotation();
        }

        // `@ conversio` is a bare marker on the function declaration prefix.
        // The ordered concrete pair is derived from the function signature by
        // semantic enrollment; the old typed union-arm payload is retracted.
        // A kernel head names a device kernel, a different contract from a
        // host conversion row, so the pair is refused by name here (operator
        // ruling 2026-10-30) rather than enrolled from a kernel signature.
        if matches!(annotation_identity, Some(TokenKind::Conversio)) {
            if self.at_kernel_decl_head() {
                return Err(self.invalid_annotation_error("conversio_kernel_head"));
            }
            if self.current_keyword_ident() != Some(TokenKind::Functio) {
                return Err(self
                    .invalid_annotation_error("unexpected_annotation_token")
                    .with_arg("context", "@ conversio"));
            }
            return Ok(AnnotationKind::Conversio);
        }

        // `@ ad '<route>'` is compiler-owned route-handler registration
        // (D6.15): a structured form, never a generic statement annotation.
        if matches!(annotation_identity, Some(TokenKind::Ad)) {
            return self.parse_ad_annotation();
        }

        if self.interner.resolve(name.name) == "commune" {
            self.expect_annotation_end("unexpected token after @ commune")?;
            return Ok(AnnotationKind::Statement(AnnotationStmt {
                name,
                args: Vec::new(),
            }));
        }

        if self.check(&TokenKind::LBrace) {
            return self.parse_braced_annotation(name);
        }

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
            Some(TokenKind::Interna) => {
                self.expect_annotation_end("unexpected token after @ interna")?;
                return Ok(AnnotationKind::Interna);
            }
            _ => {}
        }

        let mut args = Vec::new();

        while self.is_annotation_arg() {
            args.push(self.advance().clone());
        }

        Ok(AnnotationKind::Statement(AnnotationStmt { name, args }))
    }

    /// Parse the payload of `@ ad '<route>'` after the annotation name.
    ///
    /// GRAMMAR:
    ///   adAnnotation := `@` `ad` asciiLiteral NEWLINE (annotation NEWLINE)* functioDecl
    ///
    /// Only exact routes exist: `@ ad sed "…"` pattern routes are deferred
    /// (D6.2). The annotated declaration must be a `functio` (possibly after
    /// further stacked annotations); every other legality rule — top-level
    /// placement, handler shape, reserved routes — is semantic policy.
    fn parse_ad_annotation(&mut self) -> Result<AnnotationKind, ParseError> {
        if self.next_word_is("sed") {
            return Err(self.invalid_annotation_error("ad_pattern_route_deferred"));
        }
        let TokenKind::AsciiString(route) = self.peek().kind else {
            return Err(self.invalid_annotation_error("ad_annotation_route"));
        };
        let route_span = self.current_span();
        self.advance();
        self.expect_annotation_end("unexpected token after @ ad")?;
        if !self.check(&TokenKind::At) && self.current_keyword_ident() != Some(TokenKind::Functio) {
            return Err(self.invalid_annotation_error("ad_annotation_requires_functio"));
        }
        Ok(AnnotationKind::Ad(AdAnnotation { route, route_span }))
    }

    fn parse_braced_annotation(&mut self, name: Ident) -> Result<AnnotationKind, ParseError> {
        let family = self.interner.resolve(name.name).to_owned();
        self.expect_issue(&TokenKind::LBrace, "annotation_fields_start")?;

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let field_start = self.current_span();
            let field_name = self.parse_annotation_field_name()?;
            let field_name_str = self.interner.resolve(field_name.name).to_owned();
            self.expect_issue(&TokenKind::Eq, "annotation_field_value_separator")?;

            let value = if family == "annotatio" && field_name_str == "target" {
                self.parse_annotatio_target_value()?
            } else if Self::braced_annotation_field_uses_type(&family, &field_name_str) {
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

            if !self.consume_item_separator(&TokenKind::RBrace, "annotation_fields")? {
                break;
            }
        }

        self.expect_issue(&TokenKind::RBrace, "annotation_fields_end")?;
        Ok(AnnotationKind::Braced(BracedAnnotation { name, fields }))
    }

    fn braced_annotation_field_uses_type(family: &str, field_name: &str) -> bool {
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
            return Ok(Self::named_type_annotation_value(name));
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
            return Ok(Self::named_type_annotation_value(name));
        }
        Err(self.invalid_annotation_error("annotatio_unknown_target"))
    }

    fn named_type_annotation_value(name: Ident) -> AnnotationFieldValue {
        let span = name.span;
        AnnotationFieldValue::Type(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(name, Vec::new()),
            span,
            form: TypeFormLevel::LONG,
            iuncta_label: None,
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

        while self.is_annotation_arg() || self.check_keyword_ident(TokenKind::Typus) {
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
            is_flag: flag,
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
            is_rest: rest,
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

    fn parse_nucleum_annotation(&mut self) -> Result<AnnotationKind, ParseError> {
        if self.check(&TokenKind::LBrace) {
            return self.parse_braced_nucleum_annotation();
        }

        let fragment = self.eat_annotation_ident(TokenKind::Fragment);
        if self.eat_annotation_ident(TokenKind::Lane) {
            return Err(self.removed_nucleum_lane_error());
        }
        let tier = self.parse_nucleum_tier(fragment)?;
        self.expect_annotation_end("unexpected token after @ nucleum")?;
        Ok(AnnotationKind::Nucleum(NucleumAnnotation {
            is_fragment: fragment,
            tier,
            braced: false,
        }))
    }

    /// Parse the optional tier word after `@ nucleum` (`laxus` / `intutus`,
    /// en `relaxed` / `unsafe`). A second tier word, a word that is no tier,
    /// and a tier on a `fragment` each refuse by name; the default tier has no
    /// word.
    fn parse_nucleum_tier(&mut self, fragment: bool) -> Result<KernelTier, ParseError> {
        let mut tier = KernelTier::Default;
        let mut written = false;
        while self.is_annotation_arg() {
            let word = self.current_keyword_ident();
            let next = match word {
                Some(TokenKind::Laxus) => KernelTier::Relaxed,
                Some(TokenKind::Intutus) => KernelTier::Unsafe,
                Some(TokenKind::Fragment) if written => {
                    return Err(self.invalid_annotation_error("nucleum_tier_on_fragment"));
                }
                Some(TokenKind::Fragment) => break,
                _ => {
                    let TokenKind::Ident(symbol) = self.peek().kind else {
                        break;
                    };
                    return Err(self
                        .invalid_annotation_error("unknown_nucleum_tier")
                        .with_arg("name", self.interner.resolve(symbol).to_owned()));
                }
            };
            if written {
                return Err(self.invalid_annotation_error("duplicate_nucleum_tier"));
            }
            if fragment {
                return Err(self.invalid_annotation_error("nucleum_tier_on_fragment"));
            }
            self.advance();
            tier = next;
            written = true;
        }
        Ok(tier)
    }

    /// A tier modifier belongs to a kernel function. Report it by name when
    /// the annotated declaration is anything else.
    fn check_nucleum_tier_target(&mut self, annotations: &[Annotation]) {
        for annotation in annotations {
            if let AnnotationKind::Nucleum(nucleum) = &annotation.kind
                && nucleum.tier != KernelTier::Default
            {
                self.errors.push(
                    ParseError::structured(ParseErrorKind::AnnotationInvalid, annotation.span)
                        .with_arg("issue", "nucleum_tier_non_kernel"),
                );
            }
        }
    }

    fn parse_braced_nucleum_annotation(&mut self) -> Result<AnnotationKind, ParseError> {
        self.expect_issue(&TokenKind::LBrace, "nucleum_annotation_fields_start")?;

        let mut fragment = false;
        let mut saw_fragment_field = false;
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let field_name = self.parse_annotation_field_name()?;
            let field_name_str = self.interner.resolve(field_name.name).to_owned();
            self.expect_issue(&TokenKind::Eq, "nucleum_annotation_field_value_separator")?;

            match field_name_str.as_str() {
                "fragment" => {
                    if saw_fragment_field {
                        return Err(self.invalid_annotation_error("duplicate_nucleum_field"));
                    }
                    saw_fragment_field = true;
                    fragment = self.parse_nucleum_fragment_flag()?;
                }
                "lane" => return Err(self.removed_nucleum_lane_error()),
                _ => {
                    return Err(self
                        .invalid_annotation_error("invalid_nucleum_field")
                        .with_arg("field", field_name_str));
                }
            }

            if !self.consume_item_separator(&TokenKind::RBrace, "nucleum_fields")? {
                break;
            }
        }

        self.expect_issue(&TokenKind::RBrace, "nucleum_annotation_fields_end")?;
        Ok(AnnotationKind::Nucleum(NucleumAnnotation {
            is_fragment: fragment,
            tier: KernelTier::Default,
            braced: true,
        }))
    }

    /// The `lane` binding was removed (gpu-reset rule 12 point 5): a lane is
    /// never named; a `filum` loop binds its iteration coordinate with `apud`.
    /// The old spelling gets a migration diagnostic naming the removed binding
    /// instead of a parse of its former meaning.
    fn removed_nucleum_lane_error(&self) -> ParseError {
        let error = self.invalid_annotation_error("nucleum_lane_removed");
        match self.peek().kind {
            TokenKind::String(symbol) | TokenKind::AsciiString(symbol) => {
                error.with_arg("name", self.interner.resolve(symbol).to_owned())
            }
            _ => error.with_arg("name", "id"),
        }
    }

    fn parse_nucleum_fragment_flag(&mut self) -> Result<bool, ParseError> {
        if self.check_keyword_ident(TokenKind::Verum) || self.check(&TokenKind::Verum) {
            self.advance();
            return Ok(true);
        }
        if self.check_keyword_ident(TokenKind::Falsum) || self.check(&TokenKind::Falsum) {
            self.advance();
            return Ok(false);
        }
        Err(self.invalid_annotation_error("invalid_nucleum_fragment_value"))
    }

    fn check_nucleum_annotation_conflicts(
        &self,
        annotations: &[Annotation],
    ) -> Result<(), ParseError> {
        let mut nucleum_span = None;
        let mut nucleum_fragment = false;
        for annotation in annotations {
            if let AnnotationKind::Nucleum(nucleum) = &annotation.kind {
                if nucleum_span.is_some() {
                    return Err(ParseError::structured(
                        ParseErrorKind::AnnotationInvalid,
                        annotation.span,
                    )
                    .with_arg("issue", "duplicate_nucleum"));
                }
                nucleum_span = Some(annotation.span);
                nucleum_fragment = nucleum.is_fragment;
            }
        }

        if nucleum_fragment {
            for annotation in annotations {
                if self.is_graphics_fragment_annotation(&annotation.kind) {
                    return Err(ParseError::structured(
                        ParseErrorKind::AnnotationInvalid,
                        annotation.span,
                    )
                    .with_arg("issue", "nucleum_fragment_conflicts_with_fragment"));
                }
            }
        }

        Ok(())
    }

    fn is_graphics_fragment_annotation(&self, kind: &AnnotationKind) -> bool {
        let name = match kind {
            AnnotationKind::Statement(stmt) => stmt.name.name,
            AnnotationKind::Braced(braced) => braced.name.name,
            _ => return false,
        };
        self.interner.resolve(name) == "fragment"
    }

    /// Accept a string literal or a bare identifier as the lane value.
    /// Identifiers are resolved through the interner so that known lane
    /// names (`air`, `mir`, `hir-direct`) work unquoted in statement
    /// annotations.
    fn parse_lane_value(&mut self) -> Result<radix_lexer::Symbol, ParseError> {
        let token = self.advance();
        match &token.kind {
            TokenKind::String(sym) | TokenKind::Ident(sym) => Ok(*sym),
            _ => Err(Self::expected_issue_error(
                "expected_lane_value",
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
                // Declared rows canonicalize; undeclared spellings (including
                // Latin under a non-`la` pack) stay as authored.
                let source = self.interner.resolve(sym);
                let latin = self.keyword_claim_key(sym).unwrap_or(source).to_owned();
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
                // Surface → Latin family identity (`kernel` → `nucleum`). The
                // shared-field annotation is deliberately kept out of the
                // contextual keyword table: both locale spellings are raw
                // annotation names, normalized here to one AST marker.
                let source = self.interner.resolve(sym);
                let latin = match self.keyword_claim_key(sym) {
                    Some("shared" | "commune") => "commune".to_owned(),
                    Some(latin) => latin.to_owned(),
                    None => source.to_owned(),
                };
                Ok(self.keyword_ident(&latin, span))
            }
            TokenKind::Publica => Ok(self.keyword_ident("publica", span)),
            TokenKind::Protecta => Ok(self.keyword_ident("protecta", span)),
            TokenKind::Privata => Ok(self.keyword_ident("privata", span)),
            TokenKind::Interna => Ok(self.keyword_ident("interna", span)),
            TokenKind::Futura => Ok(self.keyword_ident("futura", span)),
            TokenKind::Cursor => Ok(self.keyword_ident("cursor", span)),
            TokenKind::Tag => Ok(self.keyword_ident("tag", span)),
            TokenKind::Solum => Ok(self.keyword_ident("solum", span)),
            TokenKind::Omitte => Ok(self.keyword_ident("omitte", span)),
            TokenKind::Metior => Ok(self.keyword_ident("metior", span)),
            TokenKind::Nucleum => Ok(self.keyword_ident("nucleum", span)),
            TokenKind::Fragment => Ok(self.keyword_ident("fragment", span)),
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
        // The `in` word is the writable-binding cell's parameter spelling;
        // identity resolution canonicalizes it to `varia`.
        let expected = if expected == TokenKind::In {
            TokenKind::Varia
        } else {
            expected
        };
        self.current_keyword_ident() == Some(expected)
    }

    /// Check the current word against a literal grammar word (not a registry
    /// keyword), e.g. the `lane` marker in `@ radix lane "…"`. Matches the
    /// source spelling or its resolved latin key. Unlike a keyword claim, a
    /// literal marker keeps the raw-source fallback: no pack declares `lane`,
    /// so gating it would break `@ radix lane "…"` under every non-`la` pack.
    fn next_word_is(&self, text: &str) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        let source = self.interner.resolve(sym);
        self.keyword_claim_key(sym).unwrap_or(source) == text
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
        ParseError::structured(ParseErrorKind::AnnotationInvalid, self.current_span())
            .with_arg("issue", issue)
    }

    fn parse_annotation_default(&mut self) -> Result<Expr, ParseError> {
        if let TokenKind::Ident(sym) = self.peek().kind {
            let literal = match self.interner.resolve(sym) {
                "verum" => Some(Literal::Bool(true)),
                "falsum" => Some(Literal::Bool(false)),
                // D23.2: the null value literal is `nulla`. The transitional
                // `nihil` alias still parses through the ordinary literal path
                // in `parse_expression`.
                "nulla" => Some(Literal::Nil),
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
        // `typus` ends an annotation and starts a declaration. The annotation
        // modifier key `typus` (`@optio … typus <type>`) is consumed inside
        // `parse_optio_annotation`'s own loop, which extends the generic
        // argument guard with the keyword.
        matches!(self.peek().kind, TokenKind::LBrace)
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
            || self.check_keyword_ident(TokenKind::Typus)
            || self.check_keyword_ident(TokenKind::Varia)
    }

    fn is_annotation_arg(&self) -> bool {
        if self.check(&TokenKind::At)
            || self.is_declaration_start_token()
            || self.raw_newline_before_next_word()
        {
            return false;
        }

        matches!(
            self.peek().kind,
            TokenKind::Ident(_)
                | TokenKind::Underscore(_)
                | TokenKind::String(_)
                | TokenKind::Integer(_)
                | TokenKind::BigInteger(_)
                | TokenKind::Float(_)
                | TokenKind::FloatText { .. }
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
                | TokenKind::Vel
                | TokenKind::Verum
        )
    }

    fn is_annotation_type_start(&self) -> bool {
        match self.peek().kind {
            TokenKind::Ident(sym) => {
                match self.keyword_claim_key(sym).and_then(lookup_keyword_spec) {
                    // A keyword ends the annotation type list unless it is a
                    // type-position word (`modulus`, `iuncta`). A spelling not
                    // declared in the active surface is an ordinary identifier
                    // and stays a type start.
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

    // BLOCKS AND GENERIC PARAMETERS

    /// Parse a statement block and recover within its brace boundary.
    ///
    /// GRAMMAR:
    ///   block := '{' statement* '}'
    ///
    /// Block recovery passes `stop_at_rbrace = true` so a bad inner statement can
    /// be skipped without consuming the `}` that returns control to the caller.
    ///
    /// Nested blocks re-enter here through `parse_stmt`, so the depth budget is
    /// charged at this entry point.
    pub(super) fn parse_block(&mut self) -> Result<BlockStmt, ParseError> {
        let start = self.current_span();
        self.nested(ParseErrorKind::StatementInvalid, start, |parser| {
            parser.parse_block_body(start)
        })
    }

    /// Block body beneath the depth budget; see [`Parser::parse_block`].
    fn parse_block_body(&mut self, start: Span) -> Result<BlockStmt, ParseError> {
        self.expect_issue(&TokenKind::LBrace, "block_start")?;

        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() && !self.budget_exhausted {
            match self.parse_stmt() {
                // D22.1: declarations are top-level. The declaration is parsed
                // whole so recovery resumes after it, then dropped with a
                // named diagnostic instead of reaching lowering.
                Ok(stmt) if is_top_level_only_declaration(&stmt.kind) => {
                    self.errors.push(
                        ParseError::structured(ParseErrorKind::DeclarationInvalid, stmt.span)
                            .with_arg("issue", "declaration_not_top_level"),
                    );
                }
                Ok(stmt) => stmts.push(stmt),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize(true);
                }
            }
        }

        // A spent budget leaves the rest of this block unparsed: close the block
        // positionally instead of adding one "expected '}'" per enclosing level
        // to the single budget diagnostic that already stops the parse.
        if self.budget_exhausted {
            let span = start.merge(self.previous_span());
            return Ok(BlockStmt {
                statements: stmts,
                end_trivia: Vec::new(),
                span,
            });
        }

        // Trivia immediately before the consuming `}` has no following
        // statement; retain it on the block-end boundary so the post-parse
        // completeness check can claim it.
        let end_trivia = self.parse_block_end_trivia();

        self.expect_issue(&TokenKind::RBrace, "block_end")?;

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
    ///   generic-params       := '<' generic-param (',' generic-param)* '>'
    ///   generic-param        := ident generic-bound? generic-type-default?
    ///                          | `magnitudo` ident generic-size-default?
    ///   generic-bound        := `implet` contract-ref ('∩' contract-ref)*
    ///   contract-ref         := ident ('<' type (',' type)* '>')?
    ///   generic-type-default := '=' type
    ///   generic-size-default := '=' NATURAL
    ///
    /// Type parameters (`T`) precede size parameters (`magnitudo N`). A
    /// default is a declaration-time fact spelled with `=`; once one parameter
    /// defaults, every later parameter must default so positional generic
    /// application stays unambiguous. The runtime optional-value spelling
    /// `vel` (localized `coalesce`) is rejected in this position.
    fn parse_generic_params(&mut self) -> Result<Vec<TypeParam>, ParseError> {
        if !self.eat(&TokenKind::LtAscii) {
            return Ok(Vec::new());
        }

        let mut params = Vec::new();
        let mut seen_magnitudo = false;
        let mut seen_default = false;
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

            // D1.1: a type parameter may name contracts its witnesses must
            // declare. Inside `<…>` the comma separates parameters, so
            // several bounds join with `∩` only. Size parameters take no
            // bound; `implet` after one fails at the separator below.
            let mut bounds = Vec::new();
            if kind == GenericParamKind::Typus && self.eat_keyword_ident(TokenKind::Implet) {
                loop {
                    bounds.push(self.parse_contract_ref()?);
                    if !self.eat(&TokenKind::Cap) {
                        break;
                    }
                }
            }

            if self.check_keyword_ident(TokenKind::Vel) {
                return Err(self.vel_generic_default_error());
            }

            let default = if self.eat(&TokenKind::Eq) {
                seen_default = true;
                Some(self.parse_generic_default(kind, &name)?)
            } else {
                if seen_default {
                    return Err(self.generic_default_not_trailing_error(&name));
                }
                None
            };
            let span = bounds
                .last()
                .map_or(start.merge(name.span), |bound| start.merge(bound.span));
            params.push(TypeParam {
                kind,
                name,
                default,
                bounds,
                span,
            });

            if !self.consume_item_separator(&TokenKind::GtAscii, "generic_parameters")? {
                break;
            }
        }

        self.expect_issue(&TokenKind::GtAscii, "generic_parameters_end")?;
        Ok(params)
    }

    /// Parse one contract reference in an `implet` clause or bound.
    ///
    /// GRAMMAR:
    ///   contract-ref := ident ('<' type (',' type)* '>')?
    ///
    /// A generic contract is written with its arguments in full
    /// (`Orderable<Persona>`, `Orderable<T>`); arity is a semantic check.
    fn parse_contract_ref(&mut self) -> Result<ContractRefExpr, ParseError> {
        let name = self.parse_decl_name()?;
        let mut args = Vec::new();
        if self.eat(&TokenKind::LtAscii) {
            loop {
                args.push(self.parse_type()?);
                if !self.consume_item_separator(&TokenKind::GtAscii, "type_parameters")? {
                    break;
                }
            }
            self.expect_issue(&TokenKind::GtAscii, "contract_type_arguments_end")?;
        }
        let span = name.span.merge(self.previous_span());
        Ok(ContractRefExpr { name, args, span })
    }

    /// Parse the `=` default of one generic parameter, by declared sort.
    ///
    /// The two sorts own closed default domains: a type annotation for `T`,
    /// a natural literal for `magnitudo`. Cross-sort payloads are rejected
    /// here instead of being parsed and reclassified later.
    fn parse_generic_default(
        &mut self,
        kind: GenericParamKind,
        name: &Ident,
    ) -> Result<GenericParamDefault, ParseError> {
        let missing = matches!(self.peek().kind, TokenKind::Comma | TokenKind::GtAscii);
        if missing {
            return Err(self.generic_default_missing_value_error(kind, name));
        }
        match kind {
            GenericParamKind::Typus => {
                if let TokenKind::Integer(_) = self.peek().kind {
                    return Err(self.generic_default_wrong_class_error(kind, name));
                }
                Ok(GenericParamDefault::Typus(Box::new(self.parse_type()?)))
            }
            GenericParamKind::Magnitudo => {
                if let TokenKind::Integer(value) = self.peek().kind {
                    let span = self.current_span();
                    self.advance();
                    return Ok(GenericParamDefault::Magnitudo { value, span });
                }
                Err(self.generic_default_wrong_class_error(kind, name))
            }
        }
    }

    fn generic_default_sort(kind: GenericParamKind) -> &'static str {
        match kind {
            GenericParamKind::Typus => "typus",
            GenericParamKind::Magnitudo => "magnitudo",
        }
    }

    fn generic_default_expectation(kind: GenericParamKind) -> &'static str {
        match kind {
            GenericParamKind::Typus => "type_annotation",
            GenericParamKind::Magnitudo => "natural_literal",
        }
    }

    fn vel_generic_default_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::TypeParameterInvalid, self.current_span())
            .with_arg("issue", "removed_vel_generic_default")
            .with_arg("keyword", "vel")
            .with_arg("replacement", "=")
    }

    fn generic_default_not_trailing_error(&self, name: &Ident) -> ParseError {
        ParseError::structured(ParseErrorKind::TypeParameterInvalid, name.span)
            .with_arg("issue", "generic_default_not_trailing")
            .with_arg("parameter", self.interner.resolve(name.name).to_owned())
    }

    fn generic_default_missing_value_error(
        &self,
        kind: GenericParamKind,
        name: &Ident,
    ) -> ParseError {
        ParseError::structured(ParseErrorKind::TypeParameterInvalid, self.current_span())
            .with_arg("issue", "generic_default_missing_value")
            .with_arg("parameter", self.interner.resolve(name.name).to_owned())
            .with_arg("expected", Self::generic_default_expectation(kind))
    }

    fn generic_default_wrong_class_error(
        &self,
        kind: GenericParamKind,
        name: &Ident,
    ) -> ParseError {
        ParseError::structured(ParseErrorKind::TypeParameterInvalid, self.current_span())
            .with_arg("issue", "generic_default_wrong_class")
            .with_arg("sort", Self::generic_default_sort(kind))
            .with_arg("parameter", self.interner.resolve(name.name).to_owned())
            .with_arg("expected", Self::generic_default_expectation(kind))
    }

    fn type_param_after_magnitudo_error(&self) -> ParseError {
        ParseError::structured(ParseErrorKind::TypeParameterInvalid, self.current_span())
            .with_arg("issue", "type_param_after_magnitudo")
            .with_arg("size_param", "magnitudo")
    }
}
