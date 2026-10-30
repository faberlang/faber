//! Recursive-descent front end for Faber source.
//!
//! This module is the boundary between lexer output and the syntax tree consumed
//! by collection, resolution, lowering, typechecking, and backend analysis. It
//! owns token navigation, node-id allocation, parse-error accumulation, and the
//! recovery policy shared by the declaration, statement, expression, type, and
//! pattern subparsers.
//!
//! The parser is hand-written and predictive because the language grammar has
//! several contextual forms where good diagnostics and controlled recovery matter
//! more than minimizing parser code. Submodules may use bounded lookahead or
//! rollback for local ambiguities, but the public phase contract is still one
//! forward pass from token stream to `AST` plus diagnostics.
//!
//! - The token stream is expected to end in EOF and parser position never moves
//!   beyond the stream.
//! - Comments and newline trivia are syntactically transparent; navigation
//!   helpers skip them before grammar code sees a current token, except at
//!   statement/declaration boundaries where `parse_leading_trivia()` collects them.
//! - Faber declaration syntax remains type-first (`textus nomen`), with `_`
//!   used where inference is explicitly requested.
//! - This phase only accepts syntax represented by the lexer and grammar. Legacy
//!   or tempting surface forms should become diagnostics, not hidden aliases.
//! - Node IDs are allocated monotonically in parse order for later compiler
//!   phases; they are not stable source identifiers.
//!
//! Lexer errors are terminal because there is no trustworthy token stream to
//! consume. Parser errors are collected and recovery resumes at statement or
//! block boundaries so one malformed construct does not erase the rest of the
//! file from diagnostics. Recovery is deliberately coarse: it preserves useful
//! following structure rather than pretending malformed subtrees are valid.

mod claim;
mod decl;
mod error;
mod expr;
mod pattern;
mod stmt;
mod types;

pub use error::{ParseError, ParseErrorKind};

use radix_diagnostics::PhaseErrorKind;
use radix_lexer::{
    Interner, KeywordOwner, KeywordScope, KeywordSurfaceMode, LexResult, Span, Symbol, Token,
    TokenKind, keyword_allowed_as_ident, keyword_spelling_for_token, lookup_keyword_spec,
};
use radix_syntax::{CommentLine, Ident, Newline, NodeId, Program, RegionDecl, Trivia};
use std::collections::{HashMap, HashSet};

/// Result of the parsing phase for one compilation unit.
///
/// The interner is returned with the `AST` because every identifier and string
/// literal in syntax refers to interned symbols. Callers should treat `program`
/// as usable only when [`ParseResult::success`] is true; an `AST` may still be
/// present when recoverable syntax errors were collected.
pub struct ParseResult {
    /// Syntax tree for the file when lexing produced a token stream.
    pub program: Option<Program>,

    /// Lexing or parsing diagnostics accumulated for this phase.
    pub errors: Vec<ParseError>,

    /// String table shared by syntax nodes and later compiler phases.
    pub interner: Interner,
}

impl ParseResult {
    /// True when an `AST` was produced and no diagnostics were collected.
    #[must_use]
    pub fn success(&self) -> bool {
        self.program.is_some() && self.errors.is_empty()
    }
}

/// Maximum grammar nesting depth before the parser fails closed.
///
/// Source nesting is attacker-controlled, and recursive descent spends a whole
/// precedence chain of Rust frames per level, so an unbounded descent aborts the
/// process on the stack long before any later phase can report anything. Depth
/// is checked at the recursive entry points (`parse_expression`, `parse_type`,
/// `parse_block`, `parse_json_value`) and at the recursions that sidestep those
/// entries (`? :`, `←`, the unary prefix chain, the nested array binding
/// pattern); a rejected subtree becomes an ordinary parse diagnostic through the
/// usual recovery path.
///
/// Sized from measurement, not taste. With the guard removed, a parenthesized
/// nest overflows a 2 MiB test-thread stack between 32 and 48 levels and an
/// 8 MiB stack between 128 and 144 (tens of KB of parser stack per level in a
/// debug build), so a budget above 32 could not be reached before that abort on
/// the smallest stack this front end runs on. It is ~3x the deepest bracket
/// nesting in the corpus (`corpus-product/format/long-lines.fab`, 11 levels),
/// and it leaves the shared 8 MiB compile stacks (`faber` main thread, package
/// analysis) room to lower and walk the admitted tree.
pub const MAX_PARSE_DEPTH: u32 = 32;

/// Maximum folded infix operators in one expression root before the parser
/// fails closed.
///
/// A depth counter alone does not bound this shape: `parse_additive_inner` and
/// its sibling tiers fold left-associatively in a loop, so `1+1+1+…` costs the
/// parser no extra stack frame while building a left spine one node deep per
/// operator. That spine is walked node by node, so the count is charged in
/// `binary_expr` — the fold constructor every tier shares — and accumulates
/// across the sub-expressions of one root, keeping the admitted tree inside the
/// AST walker's `MAX_VISIT_DEPTH` (1024).
pub const MAX_PARSE_OPERATORS: usize = 512;

/// Mutable state for one parser invocation.
///
/// Subparser modules extend this type instead of each owning token state. That
/// keeps recovery, comment skipping, spans, identifiers, and node allocation
/// consistent across the grammar. Some state predicates are independent facts
/// that can coexist across parsing phases; keeping them as booleans avoids
/// artificial enum combinations and preserves the direct checks used by the
/// subparsers.
#[allow(clippy::struct_excessive_bools)] // WHY: parser context carries independent state predicates across subparsers.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Indices of non-trivia tokens in `tokens`, ascending.
    ///
    /// Built once at construction so [`Parser::peek_at`] is O(log N + offset)
    /// via binary search instead of re-walking trivia from `pos` on every call.
    non_trivia_indices: Vec<usize>,
    /// Cursor into `non_trivia_indices` for the first entry with index ≥ `pos`.
    /// [`Parser::restore`] resyncs it; [`Parser::peek_at`] binary-searches if stale.
    non_trivia_cursor: usize,
    errors: Vec<ParseError>,
    next_node_id: NodeId,
    interner: Interner,
    /// `ᵀ` postfix method marker, interned once at construction so
    /// `parse_transpose_postfix` does no per-occurrence interning. The
    /// canonical intrinsic spellings are interned alongside it for the
    /// semantic marker rewrite but only this handle is read back.
    transpose_marker_glyph_method: Symbol,
    options: ParseOptions,
    keyword_surface: KeywordSurfaceMode,
    reader_keyword_latin_keys: HashMap<String, String>,
    reader_keyword_surfaces: HashMap<String, String>,
    reader_type_latin_keys: HashMap<String, String>,
    reader_type_surfaces: HashMap<String, String>,
    has_latin_fallback: bool,
    /// Source spellings that declare a nominal type in this file (U1R).
    ///
    /// Populated by one pre-parse token scan so `est <name>` classification is
    /// order-independent: a body referencing a type declared later in the file
    /// still routes to `TypeCheck`. Cross-file imported types are not visible
    /// here; those keep today's `Binary(Is)` value-equality classification.
    declared_type_names: HashSet<String>,
    /// Local names this file's selective imports bind (BUG-92EE4566).
    ///
    /// Selective members are the only import form whose binding resolves as a
    /// bare type (`bind_selective_type_import`); named and wildcard imports
    /// bind module namespaces whose members are used qualified. Populated by
    /// one pre-parse token scan alongside `declared_type_names`, so a `casu`
    /// arm classifies the same before or after the import line. Names here
    /// are candidates only: the scrutinee's closed-set rule decides.
    imported_type_names: HashSet<String>,
    /// Grammar nesting levels currently open, bounded by [`MAX_PARSE_DEPTH`].
    ///
    /// Raised and lowered around each recursive entry point, so a rejected
    /// subtree leaves the count as it found it.
    nesting_depth: u32,
    /// Folded infix operators charged against the current expression root.
    ///
    /// Reset when an expression root opens, bounded by [`MAX_PARSE_OPERATORS`].
    folded_operators: usize,
    /// Number of `parse_expression` contexts currently open.
    ///
    /// A count of zero marks the next `parse_expression` call as a new root.
    expression_contexts: u32,
    /// Whether postfix `↦` participates in the current expression precedence tier.
    /// Range-bound and construction-source entries suppress it until their nested
    /// `parse_expression` calls begin a new expression context.
    conversio_postfix_enabled: bool,
    /// Expression-context depth at which a sort expression may not claim a
    /// `(fixum | varia) IDENT {` key body.
    ///
    /// Set while an `itera` source parses: the loop's own binder has the same
    /// shape as a sort key body, so the binder belongs to the loop. A
    /// parenthesized source opens a deeper context and may carry a body.
    sort_body_barred_at: Option<u32>,
    /// Set while the annotated top-level `functio` or genus method that follows
    /// a `@ nondum` annotation is about to parse, so its signature may omit the
    /// body.
    ///
    /// Consumed (cleared) by `parse_func_decl` on entry, so it never reaches a
    /// nested declaration.
    nondum_function_pending: bool,
    /// Set once either budget is exhausted; parsing then stops descending.
    ///
    /// A budget diagnostic is raised where the budget was spent, which can be a
    /// position the statement loops would otherwise re-enter and fail on again
    /// without consuming a token. Stopping the enclosing statement/program
    /// loops keeps the failed parse linear and one-diagnostic-per-run.
    budget_exhausted: bool,
    /// The diagnostic that spent the budget.
    ///
    /// Kept so every later refusal reports the construct that actually crossed
    /// the budget: a speculative sub-parse may discard the crossing error and
    /// continue, and a synthetic "budget spent" report would hide the cause.
    budget_diagnostic: Option<ParseError>,
    /// `✓ ✗` conditionals built so far, as `(node id, span)` in build order.
    ///
    /// Node ids grow monotonically, so a conditional whose id is at least the
    /// id counter at the start of an enclosing conditional's condition lies
    /// inside it (D4.6 one-level rule). [`Parser::restore`] drops entries a
    /// discarded speculative parse built.
    conditionals: Vec<(NodeId, Span)>,
    /// A value binding or parameter named `nihil` has been parsed. The bare
    /// `nihil` retired-null spelling (`PARSE030`) is then an ordinary read of
    /// that binding: keywords are contextual and a binding may shadow them.
    /// Monotone over the file; `nihil` in an earlier, unrelated scope is not
    /// distinguished from the declared name.
    nihil_value_bound: bool,
}

/// Contextual parser policy supplied by compiler frontends.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseOptions {
    /// Permit signature-only top-level functions in analyzed library interfaces.
    pub allow_bodyless_functions: bool,
}

/// Parse lexer output into a syntax tree and parser diagnostics.
///
/// Lexing failure is represented as parser-phase output so the driver can report
/// one diagnostic stream. Unlike ordinary parse errors, lexer errors stop `AST`
/// construction because token recovery has already failed upstream.
#[must_use]
pub fn parse(lex_result: LexResult) -> ParseResult {
    parse_with_options(lex_result, ParseOptions::default())
}

/// Parse with explicit frontend policy for contextual declaration forms.
#[must_use]
pub fn parse_with_options(lex_result: LexResult, options: ParseOptions) -> ParseResult {
    let LexResult {
        tokens,
        errors,
        reader_keyword_latin_keys,
        reader_keyword_surfaces,
        reader_type_latin_keys,
        reader_type_surfaces,
        has_latin_fallback,
        keyword_surface,
        interner,
        ..
    } = lex_result;
    if !errors.is_empty() {
        return ParseResult {
            program: None,
            errors: lex_errors_to_parse_errors(errors),
            interner,
        };
    }

    let mut parser = Parser::new_with_reader_identities_and_fallback(
        tokens,
        interner,
        options,
        reader_keyword_latin_keys,
        reader_keyword_surfaces,
        reader_type_latin_keys,
        reader_type_surfaces,
        keyword_surface,
        has_latin_fallback,
    );
    parser.parse_program()
}

fn lex_errors_to_parse_errors(errors: Vec<radix_lexer::LexError>) -> Vec<ParseError> {
    errors
        .into_iter()
        .map(|error| {
            let kind = error.kind;
            let mut parse_error = ParseError::structured(ParseErrorKind::LexError, error.span);
            parse_error.error_kind = Some(PhaseErrorKind::Lex(kind));
            parse_error.args.extend(error.args);
            parse_error
        })
        .collect()
}

impl Parser {
    /// Create a new parser from tokens and string interner.
    #[must_use]
    pub fn new(tokens: Vec<Token>, interner: Interner, options: ParseOptions) -> Self {
        Self::new_with_reader_identities(
            tokens,
            interner,
            options,
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            KeywordSurfaceMode::RegistryDirect,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_reader_identities(
        tokens: Vec<Token>,
        interner: Interner,
        options: ParseOptions,
        reader_keyword_latin_keys: HashMap<String, String>,
        reader_keyword_surfaces: HashMap<String, String>,
        reader_type_latin_keys: HashMap<String, String>,
        reader_type_surfaces: HashMap<String, String>,
        keyword_surface: KeywordSurfaceMode,
    ) -> Self {
        Self::new_with_reader_identities_and_fallback(
            tokens,
            interner,
            options,
            reader_keyword_latin_keys,
            reader_keyword_surfaces,
            reader_type_latin_keys,
            reader_type_surfaces,
            keyword_surface,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_reader_identities_and_fallback(
        tokens: Vec<Token>,
        interner: Interner,
        options: ParseOptions,
        reader_keyword_latin_keys: HashMap<String, String>,
        reader_keyword_surfaces: HashMap<String, String>,
        reader_type_latin_keys: HashMap<String, String>,
        reader_type_surfaces: HashMap<String, String>,
        keyword_surface: KeywordSurfaceMode,
        has_latin_fallback: bool,
    ) -> Self {
        let non_trivia_indices: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter_map(|(i, tok)| (!tok.kind.is_trivia()).then_some(i))
            .collect();
        let mut interner = interner;
        // The glyph marker names the parsed Ident; the canonical intrinsic
        // spellings exist for the semantic rewrite and are never read back.
        interner.intern("transpone");
        interner.intern("transpose");
        let transpose_marker_glyph_method = interner.intern(expr::GLYPH_TRANSPOSE_METHOD);
        let mut parser = Self {
            tokens,
            pos: 0,
            non_trivia_indices,
            non_trivia_cursor: 0,
            errors: Vec::new(),
            next_node_id: 0,
            interner,
            transpose_marker_glyph_method,
            options,
            keyword_surface,
            reader_keyword_latin_keys,
            reader_keyword_surfaces,
            reader_type_latin_keys,
            reader_type_surfaces,
            has_latin_fallback,
            declared_type_names: HashSet::new(),
            imported_type_names: HashSet::new(),
            nesting_depth: 0,
            folded_operators: 0,
            expression_contexts: 0,
            conversio_postfix_enabled: true,
            sort_body_barred_at: None,
            nondum_function_pending: false,
            budget_exhausted: false,
            budget_diagnostic: None,
            conditionals: Vec::new(),
            nihil_value_bound: false,
        };
        parser.declared_type_names = parser.scan_declared_type_names();
        parser.imported_type_names = parser.scan_imported_type_names();
        parser
    }

    /// Collect the names this file declares as types (U1R est-RHS input).
    ///
    /// One linear token walk: after a token whose keyword identity introduces a
    /// type declaration (`genus`, `implendum`, `ordo`, `discretio`, `typus` —
    /// pack spellings included), the next identifier is the declared name.
    /// Annotation-position `typus` rows (`@ radix typus T`) contribute type
    /// parameter names, which is harmless for the est classifier.
    fn scan_declared_type_names(&self) -> HashSet<String> {
        let mut names = HashSet::new();
        let mut expect_name = false;
        for token in &self.tokens {
            if token.kind.is_trivia() {
                continue;
            }
            if expect_name {
                if let TokenKind::Ident(sym) = token.kind {
                    names.insert(self.interner.resolve(sym).to_owned());
                }
                expect_name = false;
                continue;
            }
            let introduces_type_decl = match token.kind {
                TokenKind::Genus
                | TokenKind::Implendum
                | TokenKind::Ordo
                | TokenKind::Discretio
                | TokenKind::Typus => true,
                TokenKind::Ident(sym) => {
                    let identity = self
                        .keyword_claim_key(sym)
                        .and_then(lookup_keyword_spec)
                        .and_then(|spec| spec.token_kind.clone());
                    matches!(
                        identity,
                        Some(
                            TokenKind::Genus
                                | TokenKind::Implendum
                                | TokenKind::Ordo
                                | TokenKind::Discretio
                                | TokenKind::Typus
                        )
                    )
                }
                _ => false,
            };
            if introduces_type_decl {
                expect_name = true;
            }
        }
        names
    }

    /// Collect the local names this file's selective imports bind (BUG-92EE4566).
    ///
    /// One linear token walk mirroring the sugar form's selective list
    /// (`importa ex "path" fixum A, B ut C` — no braces), including its
    /// selective-vs-following-statement lookahead, so the scan and the parser
    /// never disagree about which names an import binds. Record imports
    /// (`importa { … }`) carry no selective shape and are skipped.
    fn scan_imported_type_names(&self) -> HashSet<String> {
        let tokens: Vec<&radix_lexer::Token> = self
            .tokens
            .iter()
            .filter(|token| !token.kind.is_trivia())
            .collect();
        let keyword_at = |index: usize, identity: TokenKind| -> bool {
            tokens
                .get(index)
                .is_some_and(|token| self.token_keyword_identity(&token.kind) == Some(identity))
        };
        let mut names = HashSet::new();
        let mut index = 0;
        while index < tokens.len() {
            if !keyword_at(index, TokenKind::Importa) {
                index += 1;
                continue;
            }
            index += 1;
            if keyword_at(index, TokenKind::LBrace) || !keyword_at(index, TokenKind::Ex) {
                continue;
            }
            index += 1;
            if !matches!(
                tokens.get(index).map(|token| &token.kind),
                Some(TokenKind::String(_))
            ) {
                continue;
            }
            index += 1;
            if keyword_at(index, TokenKind::Publica) {
                index += 1;
            }
            if !keyword_at(index, TokenKind::Fixum)
                || !self.fixum_opens_selective_list(&tokens, index + 1)
            {
                continue;
            }
            index += 1;
            // Members: `name [ut alias]` separated by commas; a name that
            // cannot start a member after a comma is a trailing comma.
            while let Some((name, next)) = self.import_member_name(&tokens, index) {
                let mut cursor = next;
                if keyword_at(cursor, TokenKind::Ut) {
                    let Some((alias, alias_next)) = self.import_member_name(&tokens, cursor + 1)
                    else {
                        break;
                    };
                    names.insert(alias);
                    cursor = alias_next;
                } else {
                    names.insert(name);
                }
                if !keyword_at(cursor, TokenKind::Comma) {
                    break;
                }
                index = cursor + 1;
                if !self.import_member_starts_here(&tokens, index) {
                    break;
                }
            }
        }
        names
    }

    /// Keyword identity of a token, whether the lexer emitted the dedicated
    /// kind or a contextual identifier claiming it.
    fn token_keyword_identity(&self, kind: &TokenKind) -> Option<TokenKind> {
        match kind {
            TokenKind::Importa
            | TokenKind::Ex
            | TokenKind::Publica
            | TokenKind::Fixum
            | TokenKind::Ut
            | TokenKind::LBrace
            | TokenKind::Comma => Some(kind.clone()),
            TokenKind::Ident(sym) => self
                .keyword_claim_key(*sym)
                .and_then(lookup_keyword_spec)
                .and_then(|spec| spec.token_kind.clone()),
            _ => None,
        }
    }

    /// Token-index twin of [`Parser::import_fixum_starts_selective_list`]:
    /// whether the `fixum` word at `index` opens the import's selective list
    /// instead of starting a following `fixum` statement.
    fn fixum_opens_selective_list(&self, tokens: &[&radix_lexer::Token], index: usize) -> bool {
        let Some(kind) = tokens.get(index).map(|token| &token.kind) else {
            return true;
        };
        match kind {
            TokenKind::Eof | TokenKind::Comma | TokenKind::Star => true,
            kind if self.token_keyword_identity(kind) == Some(TokenKind::Ut) => true,
            kind if Self::is_import_binding_name_kind(kind) => {
                match tokens.get(index + 1).map(|token| &token.kind) {
                    None | Some(TokenKind::Eof | TokenKind::Comma | TokenKind::Star) => true,
                    Some(kind) if self.token_keyword_identity(kind) == Some(TokenKind::Ut) => true,
                    Some(kind) if matches!(kind, TokenKind::Ident(_)) => {
                        self.token_keyword_identity(kind).is_some()
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Token-index twin of [`Parser::is_import_binding_name`].
    fn is_import_binding_name_kind(kind: &TokenKind) -> bool {
        match kind {
            TokenKind::Ident(_) | TokenKind::Underscore(_) | TokenKind::Tag => true,
            kind => keyword_allowed_as_ident(KeywordOwner::DeclarationName, kind).is_some(),
        }
    }

    /// The member (or alias) name at `index`, with the index just past it.
    ///
    /// Mirrors the parser's declaration-name slot: a keyword-spelled `Ident`
    /// is accepted whenever `keyword_allowed_as_ident` admits it, keeping the
    /// surface spelling. The `ut` alias marker is the one keyword that names
    /// nothing.
    fn import_member_name(
        &self,
        tokens: &[&radix_lexer::Token],
        index: usize,
    ) -> Option<(String, usize)> {
        let kind = &tokens.get(index)?.kind;
        if self.token_keyword_identity(kind) == Some(TokenKind::Ut) {
            return None;
        }
        match kind {
            TokenKind::Ident(sym) => {
                if let Some(identity) = self.token_keyword_identity(kind)
                    && keyword_allowed_as_ident(KeywordOwner::DeclarationName, &identity).is_none()
                {
                    return None;
                }
                Some((self.interner.resolve(*sym).to_owned(), index + 1))
            }
            TokenKind::Underscore(sym) => Some((self.interner.resolve(*sym).to_owned(), index + 1)),
            _ => None,
        }
    }

    /// Token-index twin of [`Parser::import_value_member_starts_here`].
    fn import_member_starts_here(&self, tokens: &[&radix_lexer::Token], index: usize) -> bool {
        match tokens.get(index).map(|token| &token.kind) {
            Some(TokenKind::Ident(sym)) => self
                .keyword_claim_key(*sym)
                .and_then(lookup_keyword_spec)
                .is_none(),
            Some(TokenKind::Underscore(_) | TokenKind::Star) => true,
            _ => false,
        }
    }

    /// Parse a complete Faber source file body.
    ///
    /// GRAMMAR:
    ///   program := statement*
    ///
    /// File metadata is peeled from optional `+++` frontmatter before lexing.
    pub fn parse_program(&mut self) -> ParseResult {
        let start = self.current_span();
        let mut stmts = Vec::new();
        let region = self.try_parse_region_decl();

        while !self.is_at_end() && !self.budget_exhausted {
            if self.reject_misplaced_region_decl(region.is_some()) {
                continue;
            }
            match self.parse_stmt() {
                Ok(stmt) => stmts.push(stmt),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize(false);
                }
            }
        }

        // Trailing trivia (after the last declaration) has no following
        // statement; retain it on the program-end boundary so the post-parse
        // completeness check can claim it instead of discarding it.
        let end_trivia = self.parse_block_end_trivia();
        let end = self.previous_span();
        let span = start.merge(end);

        let program = Program {
            statements: stmts,
            span,
            end_trivia,
            region,
        };

        // Claimed-comment accounting: every `LineComment` token is either
        // present in a legal syntax trivia carrier or reported here. A spent
        // parse budget leaves the rest of the source unparsed, so comment
        // placement there is not a finding: the budget diagnostic is the report.
        if !self.budget_exhausted {
            claim::check(self, &program);
        }

        ParseResult {
            program: Some(program),
            errors: std::mem::take(&mut self.errors),
            interner: std::mem::take(&mut self.interner),
        }
    }

    // FILE NAME DECLARATION (D7.7)

    /// The declared name when the current top-level word starts `regio NAME`.
    ///
    /// `regio` (en `module`) is contextual (F8): it is claimed only in
    /// statement-initial position at file top level and only when a name
    /// follows, so the spelling stays an ordinary identifier everywhere else.
    fn region_decl_name(&self) -> Option<radix_lexer::Symbol> {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return None;
        };
        if self.keyword_claim_key(sym) != Some("regio") {
            return None;
        }
        match self.peek_at(1).kind {
            TokenKind::Ident(name) => Some(name),
            _ => None,
        }
    }

    /// Parse `regio NAME` with its leading trivia, or rewind when absent.
    ///
    /// Leading comments stay with the first statement when the file has no
    /// `regio` declaration, so the checkpoint rewinds past them. The
    /// misplaced-declaration check reads the same shape speculatively.
    fn try_parse_region_decl(&mut self) -> Option<RegionDecl> {
        let checkpoint = self.checkpoint();
        let leading_trivia = self.parse_leading_trivia();
        let Some(name) = self.region_decl_name() else {
            self.restore(checkpoint);
            return None;
        };
        Some(self.parse_region_decl(leading_trivia, name))
    }

    fn parse_region_decl(
        &mut self,
        leading_trivia: Vec<Trivia>,
        name: radix_lexer::Symbol,
    ) -> RegionDecl {
        let start = self.current_span();
        self.advance();
        self.advance();
        let name_span = self.previous_span();
        RegionDecl {
            name: Ident {
                name,
                span: name_span,
            },
            span: start.merge(name_span),
            leading_trivia,
        }
    }

    /// Report a `regio` declaration that is not the file's first declaration.
    ///
    /// Returns whether one was consumed. A second declaration is a duplicate;
    /// one after any import or declaration is misplaced.
    fn reject_misplaced_region_decl(&mut self, has_region: bool) -> bool {
        let Some(decl) = self.try_parse_region_decl() else {
            return false;
        };
        let issue = if has_region {
            "module_declaration_duplicate"
        } else {
            "module_declaration_misplaced"
        };
        self.errors.push(
            ParseError::structured(ParseErrorKind::DeclarationInvalid, decl.span)
                .with_arg("issue", issue)
                .with_arg("name", self.interner.resolve(decl.name.name).to_owned()),
        );
        true
    }

    // TOKEN NAVIGATION
    //
    // Comments and newlines are syntactically insignificant in expressions, so
    // these helpers skip trivia before grammar code sees a current token.

    /// Allocate a fresh node ID for `AST` construction.
    ///
    /// Later compiler phases use node IDs to attach semantic facts to syntax.
    /// Allocation is parse-order-local, so callers must not persist these IDs as
    /// cross-run source identity.
    fn next_id(&mut self) -> NodeId {
        let id = self.next_node_id;
        self.next_node_id += 1;
        id
    }

    /// Save parser position for bounded lookahead rollback.
    fn checkpoint(&self) -> (usize, NodeId, usize) {
        (self.pos, self.next_node_id, self.errors.len())
    }

    /// Restore parser position after failed lookahead.
    ///
    /// WHY: diagnostics pushed while speculating (a type word read by a type
    /// lookahead that did not commit, such as `ratio` in expression position)
    /// belong to a parse that was abandoned, so they roll back with it.
    fn restore(&mut self, checkpoint: (usize, NodeId, usize)) {
        self.pos = checkpoint.0;
        self.next_node_id = checkpoint.1;
        self.errors.truncate(checkpoint.2);
        self.conditionals.retain(|&(id, _)| id < checkpoint.1);
        // pos may have moved backward; re-sync the non-trivia cursor.
        self.sync_non_trivia_cursor();
    }

    /// Align `non_trivia_cursor` to the first non-trivia index ≥ `pos`.
    fn sync_non_trivia_cursor(&mut self) {
        self.non_trivia_cursor = self.non_trivia_indices.partition_point(|&i| i < self.pos);
    }

    // RESOURCE BUDGETS
    //
    // Both budgets fail closed with a parse diagnostic: the rejected subtree
    // travels the ordinary error path (statement recovery), so deep or long
    // input becomes a reportable diagnostic instead of a stack abort.

    /// Run `parse` one grammar nesting level deeper, failing closed past
    /// [`MAX_PARSE_DEPTH`].
    ///
    /// The level is released on every exit, including the failing one, so a
    /// rejected subtree never leaves the count inflated for its siblings. The
    /// first failure stops the enclosing statement loops (see
    /// [`Parser::budget_exhausted`]) and no later call descends again, so an
    /// over-budget source costs one pass, not one pass per nesting level.
    fn nested<T>(
        &mut self,
        kind: ParseErrorKind,
        span: Span,
        parse: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        if self.budget_exhausted {
            return Err(self.spent_budget_error(kind, span));
        }
        self.nesting_depth = self.nesting_depth.saturating_add(1);
        let result = if self.nesting_depth > MAX_PARSE_DEPTH {
            Err(self.spend_budget(Self::nesting_too_deep_error(kind, span)))
        } else {
            parse(self)
        };
        self.nesting_depth = self.nesting_depth.saturating_sub(1);
        result
    }

    /// Charge one folded infix operator against the current expression root,
    /// failing closed past [`MAX_PARSE_OPERATORS`].
    ///
    /// Charged where the fold builds its node (see [`MAX_PARSE_OPERATORS`]) so
    /// every left-associative tier is bounded by the same counter.
    fn charge_fold(&mut self, span: Span) -> Result<(), ParseError> {
        if self.budget_exhausted {
            return Err(self.spent_budget_error(ParseErrorKind::ExpressionInvalid, span));
        }
        self.folded_operators = self.folded_operators.saturating_add(1);
        if self.folded_operators > MAX_PARSE_OPERATORS {
            let error = ParseError::structured(ParseErrorKind::ExpressionInvalid, span)
                .with_arg("issue", "expression_too_long")
                .with_arg("limit", MAX_PARSE_OPERATORS.to_string());
            return Err(self.spend_budget(error));
        }
        Ok(())
    }

    /// Mark the budget spent and hand back the diagnostic that spent it.
    fn spend_budget(&mut self, error: ParseError) -> ParseError {
        self.budget_exhausted = true;
        self.budget_diagnostic = Some(error.clone());
        error
    }

    /// Diagnostic for any construct reached after a budget was already spent.
    fn spent_budget_error(&self, kind: ParseErrorKind, span: Span) -> ParseError {
        self.budget_diagnostic
            .clone()
            .unwrap_or_else(|| ParseError::structured(kind, span))
    }

    /// Diagnostic for source nesting past [`MAX_PARSE_DEPTH`].
    ///
    /// `kind` is the grammar area the nesting was found in, so the report keeps
    /// the expression/type/statement classification the catalog already uses.
    fn nesting_too_deep_error(kind: ParseErrorKind, span: Span) -> ParseError {
        ParseError::structured(kind, span)
            .with_arg("issue", "nesting_too_deep")
            .with_arg("limit", MAX_PARSE_DEPTH.to_string())
    }

    fn peek(&self) -> &Token {
        self.peek_at(0)
    }

    /// Peek at the raw current token without skipping trivia.
    fn peek_raw(&self) -> &Token {
        debug_assert!(
            self.pos < self.tokens.len(),
            "parser position must be in bounds"
        );
        &self.tokens[self.pos]
    }

    /// Peek at the non-trivia token `offset` steps ahead without consuming.
    ///
    /// WHY: Distinguishes forms such as `sit name ←` from `sit type name ←`
    /// without consuming tokens.
    ///
    /// Uses the precomputed non-trivia index. When `non_trivia_cursor` is stale,
    /// falls back to binary search from `pos` because this method cannot mutate.
    fn peek_at(&self, offset: usize) -> &Token {
        let start = if self
            .non_trivia_indices
            .get(self.non_trivia_cursor)
            .is_some_and(|&i| i >= self.pos)
            && (self.non_trivia_cursor == 0
                || self.non_trivia_indices[self.non_trivia_cursor - 1] < self.pos)
        {
            self.non_trivia_cursor
        } else {
            self.non_trivia_indices.partition_point(|&i| i < self.pos)
        };
        if let Some(&idx) = self.non_trivia_indices.get(start + offset) {
            return &self.tokens[idx];
        }
        debug_assert!(!self.tokens.is_empty(), "token stream always ends with EOF");
        &self.tokens[self.tokens.len() - 1]
    }

    /// Check if current token matches kind (discriminant comparison only).
    ///
    /// WHY: Uses discriminant matching to check token type without comparing
    /// associated data, allowing patterns like `check(&TokenKind::Ident(_))`.
    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }

    /// Check if current token is a specific keyword (exact match).
    fn check_keyword(&self, kw: TokenKind) -> bool {
        self.peek().kind == kw
    }

    /// Resolve a symbol to the latin key the parser may claim, gated by the
    /// effective keyword surface.
    ///
    /// Registry-direct (`la`/no-pack): the recorded pack key wins, and an
    /// unrecorded spelling falls back to its raw source so tokenless keywords
    /// the lexer deliberately leaves unrecorded still claim through the
    /// registry. Pack-authoritative (non-`la` pack): only pack-declared
    /// spellings — those the lexer recorded a latin key for — may claim; an
    /// undeclared spelling stays an ordinary identifier (`None`).
    fn keyword_claim_key(&self, sym: Symbol) -> Option<&str> {
        self.keyword_claim_key_for_text(self.interner.resolve(sym))
    }

    /// [`Self::keyword_claim_key`] over a resolved spelling, without the
    /// interner hop. Tokenless/contextual keyword claims that hold a source
    /// string (annotation-name dispatch) must route through this same gate so
    /// an undeclared Latin spelling cannot claim under a non-`la` pack.
    ///
    /// A spelling the active surface declares as a *type* is a type and never
    /// falls back to the registry. The fallback below exists for keywords the
    /// active pack leaves unrecorded, and it must not shadow the pack's own
    /// type rows: D23.2 makes `nihil` the null *type* and `nulla` the null
    /// *value*, so the Latin `nihil` type row must not fall back to the
    /// retired `nihil` value alias (registry-direct `la`).
    fn keyword_claim_key_for_text<'a>(&'a self, text: &'a str) -> Option<&'a str> {
        if let Some(latin_key) = self.reader_keyword_latin_keys.get(text) {
            return Some(latin_key);
        }
        if self.reader_type_latin_keys.contains_key(text) {
            return None;
        }
        match self.keyword_surface {
            KeywordSurfaceMode::RegistryDirect => Some(text),
            KeywordSurfaceMode::PackAuthoritative => None,
        }
    }

    /// Resolve the current word's keyword identity through the registry.
    ///
    /// Returns the locale-independent identity token for a declared keyword in
    /// any source surface (latin, or a pack spelling resolved to its latin
    /// key), or `None` when the current token is not a keyword.
    fn current_keyword_ident(&self) -> Option<TokenKind> {
        self.keyword_ident_at(0)
    }

    /// Resolve a lookahead word to its locale-independent keyword identity.
    fn keyword_ident_at(&self, offset: usize) -> Option<TokenKind> {
        let TokenKind::Ident(sym) = self.peek_at(offset).kind else {
            return None;
        };
        let latin = self.keyword_claim_key(sym)?;
        let kind = lookup_keyword_spec(latin).and_then(|spec| spec.token_kind.clone());
        // `varia` (the writable local) and `in` (the writable parameter) are one
        // binding cell: a pack may spell both with one word, so the identity
        // is shared and every position accepts either key.
        match kind {
            Some(TokenKind::In) => Some(TokenKind::Varia),
            other => other,
        }
    }

    /// Check whether the current word is the given keyword, by identity.
    ///
    /// Keywords are locale items: this matches the registry identity, not a
    /// surface spelling, so it holds for any reader pack.
    fn check_keyword_ident(&self, identity: TokenKind) -> bool {
        self.current_keyword_ident() == Some(identity)
    }

    /// Check whether the current word is a keyword claimed for the given owner.
    ///
    /// Owner queries (param mode, declaration introducer, predicate operator,
    /// …) are registry-driven and locale-independent.
    fn check_keyword_in_owner(&self, owner: KeywordOwner) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        let Some(latin) = self.keyword_claim_key(sym) else {
            return false;
        };
        matches!(
            lookup_keyword_spec(latin),
            Some(spec)
                if matches!(
                    spec.scope,
                    KeywordScope::Contextual(owners) if owners.contains(&owner)
                )
        )
    }

    /// Same as [`Self::check_keyword_ident`] but on the raw token stream —
    /// no trivia skipping. Used for same-line guards (`iace … si cond`,
    /// `mori … si cond`) where an intervening newline means the word starts a
    /// new statement instead of this statement's guard.
    fn peek_raw_keyword_is(&self, latin_key: &str) -> bool {
        match self.peek_raw().kind {
            TokenKind::Ident(sym) => {
                let source = self.interner.resolve(sym);
                if let Some(recorded) = self.reader_keyword_latin_keys.get(source) {
                    return recorded == latin_key;
                }
                match self.keyword_surface {
                    KeywordSurfaceMode::RegistryDirect => source == latin_key,
                    KeywordSurfaceMode::PackAuthoritative => false,
                }
            }
            ref kind => keyword_spelling_for_token(kind) == Some(latin_key),
        }
    }

    /// Whether a raw `Newline` token occurs between the current raw position
    /// and the next non-trivia token.
    ///
    /// WHY: Same-line guards that pair a trivia-skipping keyword peek
    /// ([`Self::check_keyword_ident`]) with a raw-stream check need to know
    /// whether the peeked word is still on the current logical line. A
    /// trailing `#` comment sits between the consumed token and the newline
    /// it terminates at, so a one-token raw peek sees the comment, not the
    /// newline; scanning the raw span to the next word sees through it.
    fn raw_newline_before_next_word(&self) -> bool {
        let next_word = self.non_trivia_indices.partition_point(|&i| i < self.pos);
        let end = self
            .non_trivia_indices
            .get(next_word)
            .copied()
            .unwrap_or(self.tokens.len());
        self.tokens[self.pos..end]
            .iter()
            .any(|token| token.kind == TokenKind::Newline)
    }

    fn advance(&mut self) -> &Token {
        if self.non_trivia_cursor < self.non_trivia_indices.len() {
            let idx = self.non_trivia_indices[self.non_trivia_cursor];
            if idx >= self.pos {
                self.pos = idx;
            } else {
                self.sync_non_trivia_cursor();
                if self.non_trivia_cursor < self.non_trivia_indices.len() {
                    self.pos = self.non_trivia_indices[self.non_trivia_cursor];
                }
            }
        } else {
            while self.pos < self.tokens.len() && self.tokens[self.pos].kind.is_trivia() {
                self.pos += 1;
            }
        }
        if !self.is_at_end() {
            self.pos += 1;
            if self.non_trivia_cursor < self.non_trivia_indices.len()
                && self.non_trivia_indices[self.non_trivia_cursor] < self.pos
            {
                self.non_trivia_cursor += 1;
            }
        }
        &self.tokens[self.pos - 1]
    }

    /// Advance one token without skipping trivia.
    ///
    /// EDGE: The guard is raw-bounds (`pos < tokens.len()`) rather than
    /// [`Parser::is_at_end`], which skips trivia. At a position where only
    /// trivia followed by EOF remains, `is_at_end` is already true but the
    /// trivia tokens must still be consumable (e.g. block/program end-trivia
    /// capture).
    fn advance_raw(&mut self) -> &Token {
        if self.pos < self.tokens.len() {
            self.pos += 1;
            while self.non_trivia_cursor < self.non_trivia_indices.len()
                && self.non_trivia_indices[self.non_trivia_cursor] < self.pos
            {
                self.non_trivia_cursor += 1;
            }
        }
        &self.tokens[self.pos - 1]
    }

    /// Collect leading trivia at a statement or declaration boundary.
    ///
    /// Order: comments and newlines in source order, stopping at the first
    /// non-trivia token. One lexer `Newline` token becomes one `Trivia::Newline`.
    pub(crate) fn parse_leading_trivia(&mut self) -> Vec<Trivia> {
        let mut trivia = Vec::new();

        while self.peek_raw().kind.is_trivia() {
            match self.advance_raw().kind {
                TokenKind::LineComment(text) => {
                    let span = self.previous_span();
                    trivia.push(Trivia::CommentLine(CommentLine { text, span }));
                }
                TokenKind::Newline => {
                    let span = self.previous_span();
                    trivia.push(Trivia::Newline(Newline { span }));
                }
                _ => break,
            }
        }

        trivia
    }

    /// Collect trivia immediately before a structural end boundary.
    ///
    /// WHY: A `LineComment`/`Newline` run before a consuming `}` or before EOF
    /// is backward-attached to the block/program end rather than forward to a
    /// following statement. The token shape is identical to
    /// [`Parser::parse_leading_trivia`], but the semantic owner differs.
    pub(crate) fn parse_block_end_trivia(&mut self) -> Vec<Trivia> {
        self.parse_leading_trivia()
    }

    fn starts_item_list_entry(&self) -> bool {
        matches!(
            self.peek().kind,
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
                | TokenKind::LBrace
                | TokenKind::LBracket
                | TokenKind::Minus
                | TokenKind::At
        )
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_keyword(&mut self, kw: TokenKind) -> bool {
        if self.check_keyword(kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consume the current word when it is the given keyword identity.
    fn eat_keyword_ident(&mut self, identity: TokenKind) -> bool {
        if self.check_keyword_ident(identity) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Peek whether the next token is an identifier naming a type.
    ///
    /// WHY: pattern position (`case <type>`) distinguishes a type pattern from
    /// a value pattern at parse time. The primitive type names (`numerus`,
    /// `textus`, `bivalens`, `fractus`, `instans`, `json`, `valor`, `lista`,
    /// `tabula`, `regex`, `octeti`, `ascii`, `vacuum`) are not reserved
    /// keywords — they are contextual identifiers that resolve to types.
    /// Localized pack spellings of those types (via `reader_type_latin_keys`)
    /// route the same way. This peek lets the pattern parser route
    /// `case numerus` to a type pattern without consuming the token.
    ///
    /// U1R extends the same classification to identifiers this file declares
    /// as nominal types (`casu Errata`), and selective-import member bindings
    /// join them (BUG-92EE4566): resolution types those bindings from the
    /// imported file's interface, so the parser routes them as type-pattern
    /// candidates and the scrutinee's closed-set rule owns type-vs-value. A
    /// name that is neither declared nor selectively imported stays a value
    /// pattern. `est` no longer consults this: D23.5 made the right-hand side
    /// of `est`/`non est` always a type.
    fn peek_starts_type_name(&self) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        let source = self.interner.resolve(sym);
        if self.reader_type_latin_keys.contains_key(source) {
            return true;
        }
        if self.declared_type_names.contains(source) {
            return true;
        }
        if self.imported_type_names.contains(source) {
            return true;
        }
        // Bare width markers (`u8`, `f32`, `lu8`, ...) are type names in every
        // type position; the retired `numerus<W>` spelling re-emits as them (N7c).
        if radix_types::numeric_width::is_numeric_width_type_sugar(source) {
            return true;
        }
        self.is_latin_type_word(source)
    }

    /// Consume an identifier with exact source text.
    ///
    /// WHY: Some source words are contextual rather than globally reserved. This
    /// keeps spellings like `ab` available as ordinary identifiers except where
    /// a parser-owned grammar slot claims them.
    fn eat_identifier_text(&mut self, text: &str) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        let source = self.interner.resolve(sym);
        let matches_text = if let Some(latin_key) = self.reader_keyword_latin_keys.get(source) {
            latin_key == text
        } else {
            match self.keyword_surface {
                KeywordSurfaceMode::RegistryDirect => source == text,
                KeywordSurfaceMode::PackAuthoritative => false,
            }
        };
        if !matches_text {
            return false;
        }
        self.advance();
        true
    }

    fn canonical_type_ident(&mut self, ident: Ident) -> Ident {
        let source = self.interner.resolve(ident.name);
        let Some(latin_key) = self.reader_type_latin_keys.get(source).cloned() else {
            let source = source.to_owned();
            self.reject_latin_type_word(&source, ident.span);
            return ident;
        };
        self.keyword_ident(&latin_key, ident.span)
    }

    /// Seal a canonical Latin type word on a non-Latin reader surface.
    ///
    /// Latin is a locale pack like every other, so a pack-authoritative file
    /// without the explicit `la` fallback (`has_latin_fallback`) may not spell
    /// a type with its Latin word. A word the file declares or selectively
    /// imports is an ordinary user name and stays accepted. The help argument
    /// `expected` carries the file locale's own spelling when the pack has one.
    fn reject_latin_type_word(&mut self, source: &str, span: Span) {
        if self.keyword_surface != KeywordSurfaceMode::PackAuthoritative
            || self.has_latin_fallback
            || !self.is_latin_type_word(source)
            || self.declared_type_names.contains(source)
            || self.imported_type_names.contains(source)
        {
            return;
        }
        let mut error = ParseError::structured(ParseErrorKind::TypeInvalid, span)
            .with_arg("issue", "wrong_locale_type")
            .with_arg("type", source);
        if let Some(expected) = self.reader_type_surfaces.get(source) {
            error = error.with_arg("expected", expected.as_str());
        }
        self.errors.push(error);
    }

    /// Authored source spelling of the current word, when it is an identifier.
    ///
    /// Keyword token *kinds* carry no spelling, but pack-aware lexing emits
    /// every declared spelling as an `Ident` with its identity recorded beside
    /// it, so the authored word is recoverable where a diagnostic must name it.
    fn current_ident_source(&self) -> String {
        match self.peek().kind {
            TokenKind::Ident(sym) => self.interner.resolve(sym).to_owned(),
            _ => "nulla".to_owned(),
        }
    }

    /// Whether the current word is the null *type* in the active surface
    /// (D23.2): the pack's `nihil` type row, or the Latin `nihil` itself (a
    /// sealed file then reports it as the wrong locale's type word). The null *value* word is a keyword
    /// literal and never matches here.
    fn check_null_type_word(&self) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        let text = self.interner.resolve(sym);
        match self.reader_type_latin_keys.get(text) {
            Some(latin) => latin == "nihil",
            // The Latin word in a surface whose pack names the null type
            // otherwise still reads as the null type here, so a sealed file's
            // `casu nihil` reaches `reject_latin_type_word` instead of
            // binding the word as a pattern name; a declared or imported
            // `nihil` stays an ordinary name.
            None => {
                text == "nihil"
                    && !self.declared_type_names.contains(text)
                    && !self.imported_type_names.contains(text)
            }
        }
    }

    /// Active surface spelling of the null *type* (D23.2).
    ///
    /// Falls back to the canonical Latin `nihil` when the active surface
    /// declares no type rows (registry-direct Latin).
    fn null_type_surface(&self) -> String {
        self.reader_type_surfaces
            .get("nihil")
            .cloned()
            .unwrap_or_else(|| "nihil".to_owned())
    }

    /// Active surface spelling of a canonical Latin keyword (D23.2).
    ///
    /// Falls back to the canonical word when the active surface declares no
    /// keyword rows (registry-direct Latin).
    fn keyword_surface(&self, latin: &str) -> String {
        self.reader_keyword_surfaces
            .get(latin)
            .cloned()
            .unwrap_or_else(|| latin.to_owned())
    }

    /// Active surface spelling of the null *value* literal (D23.2).
    ///
    /// The pack word whose identity is `nulla`; the canonical Latin `nulla`
    /// when the active surface declares no keyword rows (registry-direct Latin).
    fn null_value_surface(&self) -> String {
        self.reader_keyword_surfaces
            .get("nulla")
            .cloned()
            .unwrap_or_else(|| "nulla".to_owned())
    }

    /// Whether `source` is a canonical Latin type word.
    ///
    /// Derived from data, never a second hand-typed list: the static Latin
    /// primitive, collection and tensor-family rows, every Latin type identity
    /// the active pack declares (`reader_type_surfaces`, which carries the
    /// width-policy, relational and conversation rows), and the keyword-registry
    /// words that fill type slots (`nihil`, `modulus`, `iuncta`).
    fn is_latin_type_word(&self, source: &str) -> bool {
        radix_types::lookup_primitive_type_spec(source).is_some()
            || radix_types::lookup_collection_type_spec(source).is_some()
            || radix_types::lookup_tensor_family_type_spec(source).is_some()
            || self.reader_type_surfaces.contains_key(source)
            || matches!(
                lookup_keyword_spec(source).and_then(|spec| spec.token_kind.clone()),
                Some(TokenKind::Nihil | TokenKind::Modulus | TokenKind::Iuncta)
            )
    }

    /// Require a token while preserving the grammar contract as a locale issue.
    fn expect_issue(
        &mut self,
        kind: &TokenKind,
        issue: &'static str,
    ) -> Result<&Token, ParseError> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", issue),
            )
        }
    }

    /// Require the current word to be the given keyword identity.
    ///
    /// The diagnostic names the stable `expected_keyword` issue; the expected
    /// word travels as a `keyword` arg in the active locale's spelling (the
    /// canonical Latin word when the active surface declares no keyword rows),
    /// so the renderer owns the prose.
    fn expect_keyword_ident(&mut self, identity: TokenKind) -> Result<(), ParseError> {
        // The static registry gives every keyword identity a canonical
        // spelling; the fallback only guards non-keyword misuse.
        let spelling = keyword_spelling_for_token(&identity);
        if self.check_keyword_ident(identity) {
            self.advance();
            Ok(())
        } else {
            let keyword =
                spelling.map_or_else(|| "keyword".to_owned(), |latin| self.keyword_surface(latin));
            Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", "expected_keyword")
                    .with_arg("keyword", keyword),
            )
        }
    }

    /// Require a keyword identity under a site-specific issue name (sites
    /// whose prose says more than the bare expected keyword).
    fn expect_keyword_issue(
        &mut self,
        identity: TokenKind,
        issue: &'static str,
    ) -> Result<(), ParseError> {
        if self.check_keyword_ident(identity) {
            self.advance();
            Ok(())
        } else {
            Err(
                ParseError::structured(ParseErrorKind::Expected, self.current_span())
                    .with_arg("issue", issue),
            )
        }
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn current_span(&self) -> Span {
        self.peek().span
    }

    fn previous_span(&self) -> Span {
        if self.pos > 0 {
            self.tokens[self.pos - 1].span
        } else {
            Span::default()
        }
    }

    /// Expectation failure that names its locale `issue` instead of prose.
    fn expected_issue_error(issue: &'static str, span: Span) -> ParseError {
        ParseError::structured(ParseErrorKind::Expected, span).with_arg("issue", issue)
    }

    /// Expectation failure whose prose names candidate keywords in the active
    /// reader surface.
    ///
    /// Each Latin keyword becomes a named arg carrying
    /// [`Parser::keyword_surface`]'s spelling, so a pack row can render
    /// `{fixum}`/`{varia}` (and similar) instead of hard-coding Latin words
    /// into a non-Latin reader locale.
    fn expected_issue_with_keywords(
        &self,
        issue: &'static str,
        span: Span,
        keywords: &[&'static str],
    ) -> ParseError {
        let mut error = Self::expected_issue_error(issue, span);
        for latin in keywords {
            error = error.with_arg(latin, self.keyword_surface(latin));
        }
        error
    }

    /// Trailing-comma diagnostic shared by every item-list separator.
    fn item_list_trailing_comma_error(span: Span, position: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::Expected, span)
            .with_arg("issue", "item_list_trailing_comma")
            .with_arg("family", "item_list")
            .with_arg("position", position)
    }

    /// Missing-separator diagnostic shared by every item-list separator.
    fn item_list_missing_separator_error(span: Span, position: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::Expected, span)
            .with_arg("issue", "item_list_missing_separator")
            .with_arg("family", "item_list")
            .with_arg("position", position)
            .with_arg("expected", "comma between items")
    }

    /// Consume the separator after an item-list entry.
    ///
    /// Item lists are strict: a comma is required before another entry and is
    /// forbidden immediately before the closing delimiter. The caller keeps
    /// ownership of the loop so lists with contextual boundaries can use the
    /// same diagnostic contract.
    fn consume_item_separator(
        &mut self,
        end: &TokenKind,
        position: &'static str,
    ) -> Result<bool, ParseError> {
        self.consume_item_separator_before(std::slice::from_ref(end), position)
    }

    /// Variant of [`Self::consume_item_separator`] for contextual lists with
    /// more than one legal closing token.
    fn consume_item_separator_before(
        &mut self,
        ends: &[TokenKind],
        position: &'static str,
    ) -> Result<bool, ParseError> {
        if self.eat(&TokenKind::Comma) {
            let comma_span = self.previous_span();
            if ends.iter().any(|end| self.check(end)) {
                let error = Self::item_list_trailing_comma_error(comma_span, position);
                let error = if position.starts_with("json_") {
                    error.with_arg(
                        "migration_help",
                        "remove the trailing comma; JSON follows RFC 8259",
                    )
                } else {
                    error
                };
                return Err(error);
            }
            return Ok(true);
        }

        if !ends.iter().any(|end| self.check(end))
            && !self.is_at_end()
            && self.starts_item_list_entry()
        {
            return Err(Self::item_list_missing_separator_error(
                self.current_span(),
                position,
            ));
        }

        Ok(false)
    }

    /// Reject a separator after a list's final rest item.
    fn reject_item_list_comma_after_last(
        &mut self,
        position: &'static str,
    ) -> Result<(), ParseError> {
        if self.eat(&TokenKind::Comma) {
            let comma_span = self.previous_span();
            return Err(Self::item_list_trailing_comma_error(comma_span, position));
        }
        Ok(())
    }

    /// Whether the parser sits at a boundary that terminates an expression
    /// list (`}`/EOF, a same-line break, a `cape` clause, or a statement-start
    /// keyword). `cape` is explicit here because, unlike a statement start,
    /// it is deliberately absent from [`Self::is_recovery_boundary`].
    fn at_expression_list_boundary(&self) -> bool {
        self.is_at_end()
            || self.check(&TokenKind::RBrace)
            || self.raw_newline_before_next_word()
            || self.check_keyword_ident(TokenKind::Cape)
            || self.is_recovery_boundary(&self.peek().kind)
    }

    /// Consume a separator for an expression list terminated by a statement
    /// boundary rather than a punctuation delimiter.
    fn consume_item_separator_to_boundary(
        &mut self,
        position: &'static str,
    ) -> Result<bool, ParseError> {
        if self.eat(&TokenKind::Comma) {
            let comma_span = self.previous_span();
            if self.at_expression_list_boundary() {
                return Err(Self::item_list_trailing_comma_error(comma_span, position));
            }
            return Ok(true);
        }

        if self.at_expression_list_boundary() {
            return Ok(false);
        }

        Err(Self::item_list_missing_separator_error(
            self.current_span(),
            position,
        ))
    }

    /// Reject commas in a declaration-block field position.
    fn reject_declaration_block_comma(&mut self, position: &'static str) -> Result<(), ParseError> {
        if self.eat(&TokenKind::Comma) {
            let comma_span = self.previous_span();
            return Err(
                ParseError::structured(ParseErrorKind::Unexpected, comma_span)
                    .with_arg("issue", "declaration_block_comma")
                    .with_arg("family", "declaration_block")
                    .with_arg("position", position),
            );
        }
        Ok(())
    }

    fn invalid_expression_error(&self, issue: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::ExpressionInvalid, self.current_span())
            .with_arg("issue", issue)
    }

    // ERROR RECOVERY

    /// Check whether a token can safely restart statement parsing after an error.
    ///
    /// This list is intentionally coupled to `parse_stmt`: recovery should
    /// stop at the same keyword-led forms that can legally begin a statement,
    /// plus block closure and `EOF`. Drift here changes diagnostic reach.
    ///
    /// Tokenless declaration introducers arrive as identifiers, so
    /// the identifier arm resolves the spelling to its Latin key through the
    /// surface gate and matches the same statement-start set. The other
    /// tokenless statement keywords (`figendum`/`variandum`/`reddet`/`tacebit`,
    /// `nota`/`mone`/`vide`) predate this and are intentionally not yet
    /// recovery boundaries.
    fn is_recovery_boundary(&self, kind: &TokenKind) -> bool {
        if let TokenKind::Ident(sym) = kind {
            let Some(latin) = self.keyword_claim_key(*sym) else {
                // Undeclared under a pack-authoritative surface: an ordinary
                // identifier is never a statement-start boundary.
                return false;
            };
            let ident = lookup_keyword_spec(latin).and_then(|spec| spec.token_kind.clone());
            return matches!(
                ident,
                Some(
                    TokenKind::Discretio
                        | TokenKind::Fixum
                        | TokenKind::Varia
                        | TokenKind::Generis
                        | TokenKind::Functio
                        | TokenKind::Genus
                        | TokenKind::Implendum
                        | TokenKind::Ordo
                        | TokenKind::Sit
                        | TokenKind::Typus
                        | TokenKind::Schema
                        | TokenKind::Discerne
                        | TokenKind::Dum
                        | TokenKind::Elige
                        | TokenKind::Fac
                        | TokenKind::Si
                        | TokenKind::Perge
                        | TokenKind::Redde
                        | TokenKind::Rumpe
                        | TokenKind::Tacet
                        | TokenKind::Ad
                        | TokenKind::Ex
                        | TokenKind::Importa
                        | TokenKind::Proba
                        | TokenKind::Probandum
                        | TokenKind::Custodi
                        | TokenKind::Itera
                        | TokenKind::Iace
                        | TokenKind::Mori
                        | TokenKind::Cede
                        | TokenKind::Adfirma
                        | TokenKind::Requirit
                        | TokenKind::Reice
                        | TokenKind::Scribe
                        | TokenKind::Incipiet
                        | TokenKind::Incipit
                )
            );
        }
        matches!(
            kind,
            // Structural boundaries
            TokenKind::At | TokenKind::Eof | TokenKind::LBrace | TokenKind::RBrace
        )
    }

    /// Whether the current token is a restart point for this recovery mode.
    fn at_recovery_stop(&self, stop_at_rbrace: bool) -> bool {
        self.is_recovery_boundary(&self.peek().kind)
            && (stop_at_rbrace || !matches!(self.peek().kind, TokenKind::RBrace))
    }

    /// Synchronize after an error by skipping to the next statement boundary.
    ///
    /// Top-level recovery may consume `}` because there is no enclosing block to
    /// protect. Block recovery can stop before `}` so the caller can close the
    /// block normally and preserve the surrounding `AST` shape.
    fn synchronize(&mut self, stop_at_rbrace: bool) {
        if self.at_recovery_stop(stop_at_rbrace) {
            return;
        }

        self.advance();

        while !self.is_at_end() {
            if self.at_recovery_stop(stop_at_rbrace) {
                return;
            }

            self.advance();
        }
    }

    // IDENTIFIER PARSING

    /// Parse an identifier in expression or binding positions.
    ///
    /// WHY: Reserved keywords stay reserved here (`fixum solum x` must fail).
    ///
    /// GRAMMAR:
    ///   ident := IDENT | UNDERSCORE | `tag`
    fn parse_ident(&mut self) -> Result<Ident, ParseError> {
        let token = self.advance();
        let span = token.span;
        match token.kind {
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => Ok(Ident { name: sym, span }),
            TokenKind::Tag => Ok(self.keyword_ident("tag", span)),
            _ => Err(Self::expected_issue_error("expected_identifier", span)),
        }
    }

    /// Parse the required name after `functio`.
    fn parse_function_name(&mut self) -> Result<Ident, ParseError> {
        self.parse_decl_name_after_keyword(ParseErrorKind::FunctionNameAbsent)
    }

    /// Parse the required name after `genus`.
    fn parse_class_name(&mut self) -> Result<Ident, ParseError> {
        self.parse_decl_name_after_keyword(ParseErrorKind::ClassNameAbsent)
    }

    fn parse_decl_name_after_keyword(
        &mut self,
        missing_kind: ParseErrorKind,
    ) -> Result<Ident, ParseError> {
        let token = self.advance();
        let span = token.span;
        match &token.kind {
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => Ok(Ident { name: *sym, span }),
            TokenKind::Tag => Ok(self.keyword_ident("tag", span)),
            kind => {
                if let Some(text) = keyword_allowed_as_ident(KeywordOwner::DeclarationName, kind) {
                    Ok(self.keyword_ident(text, span))
                } else {
                    Err(ParseError::structured(missing_kind, span))
                }
            }
        }
    }

    /// Parse a top-level or nested declaration name.
    fn parse_decl_name(&mut self) -> Result<Ident, ParseError> {
        self.parse_contextual_ident(KeywordOwner::DeclarationName)
    }

    /// Parse a parameter, field, or `ut` alias name after the type.
    fn parse_param_name(&mut self) -> Result<Ident, ParseError> {
        self.parse_contextual_ident(KeywordOwner::ParameterName)
    }

    /// Parse a member access or method name.
    fn parse_member_ident(&mut self) -> Result<Ident, ParseError> {
        self.parse_contextual_ident(KeywordOwner::MemberIdentifier)
    }

    /// Parse an identifier allowed for the given keyword-ownership context.
    ///
    /// Keyword spellings lex as `Ident`. The owner gate decides whether that
    /// spelling may occupy this name slot; allowed names keep their surface
    /// spelling.
    fn parse_contextual_ident(&mut self, owner: KeywordOwner) -> Result<Ident, ParseError> {
        if let Some(kind) = self.current_keyword_ident()
            && keyword_allowed_as_ident(owner, &kind).is_none()
        {
            return Err(Self::expected_issue_error(
                "expected_identifier",
                self.current_span(),
            ));
        }
        let token = self.advance();
        let span = token.span;
        match &token.kind {
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => Ok(Ident { name: *sym, span }),
            TokenKind::Tag if owner == KeywordOwner::DeclarationName => {
                Ok(self.keyword_ident("tag", span))
            }
            kind => {
                if let Some(text) = keyword_allowed_as_ident(owner, kind) {
                    Ok(self.keyword_ident(text, span))
                } else {
                    Err(Self::expected_issue_error("expected_identifier", span))
                }
            }
        }
    }

    /// Record a value binding or parameter name so a later bare `nihil` read
    /// resolves to it instead of the retired null spelling.
    fn note_value_binding(&mut self, ident: &Ident) {
        if self.interner.resolve(ident.name) == "nihil" {
            self.nihil_value_bound = true;
        }
    }

    fn keyword_ident(&mut self, name: &str, span: Span) -> Ident {
        let sym = self.interner.intern(name);
        Ident { name: sym, span }
    }

    fn parse_string(&mut self) -> Result<Symbol, ParseError> {
        let token = self.advance();
        match &token.kind {
            TokenKind::String(sym) => Ok(*sym),
            _ => Err(Self::expected_issue_error(
                "expected_string_literal",
                token.span,
            )),
        }
    }
}

#[cfg(test)]
#[path = "test_support.rs"]
pub mod test_support;

#[cfg(test)]
#[path = "comment_preservation_test.rs"]
mod comment_preservation_test;

#[cfg(test)]
#[path = "fac_omnia_test.rs"]
mod fac_omnia_test;

#[cfg(test)]
#[path = "line_start_comment_test.rs"]
mod line_start_comment_test;

#[cfg(test)]
#[path = "enum_union_trivia_test.rs"]
mod enum_union_trivia_test;

#[cfg(test)]
#[path = "claim_test.rs"]
mod claim_test;

#[cfg(test)]
#[path = "call_marker_test.rs"]
mod call_marker_test;
#[cfg(test)]
#[path = "value_marker_test.rs"]
mod value_marker_test;

#[cfg(test)]
#[path = "generic_call_test.rs"]
mod generic_call_test;

#[cfg(test)]
#[path = "est_type_classification_test.rs"]
mod est_type_classification_test;

#[cfg(test)]
#[path = "slot_helpers_test.rs"]
mod slot_helpers_test;

#[cfg(test)]
#[path = "allocator_surface_removed_test.rs"]
mod allocator_surface_removed_test;

#[cfg(test)]
#[path = "inheritance_removed_test.rs"]
mod inheritance_removed_test;

#[cfg(test)]
#[path = "contract_bound_test.rs"]
mod contract_bound_test;

#[cfg(test)]
#[path = "nexum_removed_test.rs"]
mod nexum_removed_test;

#[cfg(test)]
#[path = "nihil_binding_test.rs"]
mod nihil_binding_test;

#[cfg(test)]
#[path = "region_decl_test.rs"]
mod region_decl_test;

#[cfg(test)]
#[path = "static_decl_test.rs"]
mod static_decl_test;

#[cfg(test)]
#[path = "binding_words_test.rs"]
mod binding_words_test;

#[cfg(test)]
#[path = "import_target_test.rs"]
mod import_target_test;

#[cfg(test)]
#[path = "nondum_function_test.rs"]
mod nondum_function_test;

#[cfg(test)]
#[path = "discerne_separator_test.rs"]
mod discerne_separator_test;

#[cfg(test)]
#[path = "module_constant_test.rs"]
mod module_constant_test;

#[cfg(test)]
#[path = "compile_time_local_test.rs"]
mod compile_time_local_test;

#[cfg(test)]
#[path = "field_modifier_test.rs"]
mod field_modifier_test;

#[cfg(test)]
#[path = "construction_spread_removed_test.rs"]
mod construction_spread_removed_test;

#[cfg(test)]
#[path = "construction_source_test.rs"]
mod construction_source_test;

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

#[cfg(test)]
#[path = "size_extent_test.rs"]
mod size_extent_test;

#[cfg(test)]
#[path = "comparison_ascii_retired_test.rs"]
mod comparison_ascii_retired_test;

#[cfg(test)]
#[path = "neq_glyph_retired_test.rs"]
mod neq_glyph_retired_test;

#[cfg(test)]
#[path = "regex_literal_test.rs"]
mod regex_literal_test;

#[cfg(test)]
#[path = "rounding_glyph_test.rs"]
mod rounding_glyph_test;

#[cfg(test)]
#[path = "radical_glyph_test.rs"]
mod radical_glyph_test;

#[cfg(test)]
#[path = "typed_zero_test.rs"]
mod typed_zero_test;

#[cfg(test)]
#[path = "itera_binder_regression_test.rs"]
mod itera_binder_regression_test;

#[cfg(test)]
#[path = "schema_test.rs"]
mod schema_test;

#[cfg(test)]
#[path = "nucleum_annotation_test.rs"]
mod nucleum_annotation_test;

#[cfg(test)]
#[path = "nucleum_head_test.rs"]
mod nucleum_head_test;

#[cfg(test)]
#[path = "annotation_locale_test.rs"]
mod annotation_locale_test;

#[cfg(test)]
#[path = "annotation_line_test.rs"]
mod annotation_line_test;

#[cfg(test)]
#[path = "import_field_locale_test.rs"]
mod import_field_locale_test;

#[cfg(test)]
#[path = "comma_law_test.rs"]
mod comma_law_test;

#[cfg(test)]
#[path = "comma_law_matrix_test.rs"]
mod comma_law_matrix_test;

#[cfg(test)]
#[path = "summa_disambiguation_test.rs"]
mod summa_disambiguation_test;

#[cfg(test)]
#[path = "reducta_test.rs"]
mod reducta_test;

#[cfg(test)]
#[path = "return_thread_test.rs"]
mod return_thread_test;
#[cfg(test)]
#[path = "scan_test.rs"]
mod scan_test;
#[cfg(test)]
#[path = "sort_test.rs"]
mod sort_test;

#[cfg(test)]
#[path = "itera_source_sort_test.rs"]
mod itera_source_sort_test;

#[cfg(test)]
#[path = "max_min_disambiguation_test.rs"]
mod max_min_disambiguation_test;

#[cfg(test)]
#[path = "infinity_literal_test.rs"]
mod infinity_literal_test;

#[cfg(test)]
#[path = "lt_divert_regression_test.rs"]
mod lt_divert_regression_test;

#[cfg(test)]
#[path = "membership_glyph_test.rs"]
mod membership_glyph_test;

#[cfg(test)]
#[path = "membership_retired_test.rs"]
mod membership_retired_test;

#[cfg(test)]
#[path = "union_commune_test.rs"]
mod union_commune_test;

#[cfg(test)]
#[path = "union_conversio_test.rs"]
mod union_conversio_test;

#[cfg(test)]
#[path = "ad_annotation_test.rs"]
mod ad_annotation_test;

#[cfg(test)]
#[path = "non_trivia_index_test.rs"]
mod non_trivia_index_test;

#[cfg(test)]
#[path = "nesting_budget_test.rs"]
mod nesting_budget_test;

#[cfg(test)]
#[path = "nested_declaration_test.rs"]
mod nested_declaration_test;

#[cfg(test)]
#[path = "conditional_glyph_test.rs"]
mod conditional_glyph_test;

#[cfg(test)]
#[path = "ternary_removed_test.rs"]
mod ternary_removed_test;

#[cfg(test)]
#[path = "sum_type_data_only_test.rs"]
mod sum_type_data_only_test;

#[cfg(test)]
#[path = "format_operator_test.rs"]
mod format_operator_test;

#[cfg(test)]
#[path = "insere_test.rs"]
mod insere_test;
#[cfg(test)]
#[path = "praefixum_test.rs"]
mod praefixum_test;

#[cfg(test)]
#[path = "numeric_wrapper_retired_test.rs"]
mod numeric_wrapper_retired_test;

#[cfg(test)]
#[path = "via_hint_test.rs"]
mod via_hint_test;

#[cfg(test)]
#[path = "big_integer_literal_test.rs"]
mod big_integer_literal_test;

#[cfg(test)]
#[path = "selective_import_type_pattern_test.rs"]
mod selective_import_type_pattern_test;

#[cfg(test)]
#[path = "latin_type_seal_test.rs"]
mod latin_type_seal_test;

#[cfg(test)]
#[path = "expected_issue_test.rs"]
mod expected_issue_test;

#[cfg(test)]
#[path = "emitted_issue_rows_test.rs"]
mod emitted_issue_rows_test;

#[cfg(test)]
#[path = "range_argument_test.rs"]
mod range_argument_test;
