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
    keyword_allowed_as_ident, keyword_spelling_for_token, lookup_keyword_spec, Interner,
    KeywordOwner, KeywordScope, KeywordSurfaceMode, LexResult, Span, Symbol, Token, TokenKind,
};
use radix_syntax::{CommentLine, Ident, Newline, NodeId, Program, Trivia};
use std::collections::HashMap;

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

/// Mutable state for one parser invocation.
///
/// Subparser modules extend this type instead of each owning token state. That
/// keeps recovery, comment skipping, spans, identifiers, and node allocation
/// consistent across the grammar.
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
    options: ParseOptions,
    keyword_surface: KeywordSurfaceMode,
    reader_keyword_latin_keys: HashMap<String, String>,
    reader_type_latin_keys: HashMap<String, String>,
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
        reader_type_latin_keys,
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

    let mut parser = Parser::new_with_reader_identities(
        tokens,
        interner,
        options,
        reader_keyword_latin_keys,
        reader_type_latin_keys,
        keyword_surface,
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
            KeywordSurfaceMode::RegistryDirect,
        )
    }

    fn new_with_reader_identities(
        tokens: Vec<Token>,
        interner: Interner,
        options: ParseOptions,
        reader_keyword_latin_keys: HashMap<String, String>,
        reader_type_latin_keys: HashMap<String, String>,
        keyword_surface: KeywordSurfaceMode,
    ) -> Self {
        let non_trivia_indices: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter_map(|(i, tok)| (!tok.kind.is_trivia()).then_some(i))
            .collect();
        Self {
            tokens,
            pos: 0,
            non_trivia_indices,
            non_trivia_cursor: 0,
            errors: Vec::new(),
            next_node_id: 0,
            interner,
            options,
            keyword_surface,
            reader_keyword_latin_keys,
            reader_type_latin_keys,
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

        while !self.is_at_end() {
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
        };

        // Claimed-comment accounting: every `LineComment` token is either
        // present in a legal syntax trivia carrier or reported here.
        claim::check(self, &program);

        ParseResult {
            program: Some(program),
            errors: std::mem::take(&mut self.errors),
            interner: std::mem::take(&mut self.interner),
        }
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
    fn checkpoint(&self) -> (usize, NodeId) {
        (self.pos, self.next_node_id)
    }

    /// Restore parser position after failed lookahead.
    fn restore(&mut self, checkpoint: (usize, NodeId)) {
        self.pos = checkpoint.0;
        self.next_node_id = checkpoint.1;
        // pos may have moved backward; re-sync the non-trivia cursor.
        self.sync_non_trivia_cursor();
    }

    /// Align `non_trivia_cursor` to the first non-trivia index ≥ `pos`.
    fn sync_non_trivia_cursor(&mut self) {
        self.non_trivia_cursor = self.non_trivia_indices.partition_point(|&i| i < self.pos);
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
    /// WHY: Distinguishes forms such as `fixum _ name ←` from `fixum type name ←`
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
    fn keyword_claim_key_for_text<'a>(&'a self, text: &'a str) -> Option<&'a str> {
        if let Some(latin_key) = self.reader_keyword_latin_keys.get(text) {
            return Some(latin_key);
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
        let TokenKind::Ident(sym) = self.peek().kind else {
            return None;
        };
        let latin = self.keyword_claim_key(sym)?;
        lookup_keyword_spec(latin).and_then(|spec| spec.token_kind.clone())
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
            // Keep non-trivia cursor consistent with raw advances past trivia or real tokens.
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

    /// Peek whether the next token is an identifier naming a primitive type.
    ///
    /// WHY: `est <type>` needs to disambiguate a type-name `RHS` from a value
    /// expression at parse time. The primitive type names (`numerus`, `textus`,
    /// `bivalens`, `fractus`, `instans`, `json`, `valor`, `lista`, `tabula`, `regex`,
    /// `octeti`, `ascii`, `vacuum`) are not reserved keywords — they are
    /// contextual identifiers that resolve to types. Localized pack spellings
    /// of those types (via `reader_type_latin_keys`) route the same way. This
    /// peek lets the parser route `est numerus` to `TypeCheck` without
    /// consuming the token.
    fn peek_starts_type_name(&self) -> bool {
        let TokenKind::Ident(sym) = self.peek().kind else {
            return false;
        };
        let source = self.interner.resolve(sym);
        if self.reader_type_latin_keys.contains_key(source) {
            return true;
        }
        matches!(
            source,
            "numerus"
                | "textus"
                | "bivalens"
                | "fractus"
                | "instans"
                | "json"
                | "valor"
                | "lista"
                | "tabula"
                | "regex"
                | "octeti"
                | "ascii"
                | "vacuum"
        )
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
            return ident;
        };
        self.keyword_ident(&latin_key, ident.span)
    }

    fn expect(&mut self, kind: &TokenKind, msg: &str) -> Result<&Token, ParseError> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(self.expected_message_error(msg))
        }
    }

    /// Require the current word to be the given keyword identity.
    fn expect_keyword_ident(&mut self, identity: TokenKind, msg: &str) -> Result<(), ParseError> {
        if self.check_keyword_ident(identity) {
            self.advance();
            Ok(())
        } else {
            Err(self.expected_message_error(msg))
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

    fn expected_error(expected: &'static str, span: Span) -> ParseError {
        ParseError::structured(ParseErrorKind::Expected, span).with_arg("expected", expected)
    }

    fn expected_message_error(&self, message: &str) -> ParseError {
        let expected = message.strip_prefix("expected ").unwrap_or(message);
        ParseError::structured(ParseErrorKind::Expected, self.current_span())
            .with_arg("expected", expected)
    }

    fn invalid_expression_error(&self, issue: &'static str) -> ParseError {
        ParseError::structured(ParseErrorKind::InvalidExpression, self.current_span())
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
                        | TokenKind::Cura
                        | TokenKind::Tacet
                        | TokenKind::Ad
                        | TokenKind::Abstractus
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

    /// Synchronize after an error by skipping to the next statement boundary.
    ///
    /// Top-level recovery may consume `}` because there is no enclosing block to
    /// protect. Block recovery can stop before `}` so the caller can close the
    /// block normally and preserve the surrounding `AST` shape.
    fn synchronize(&mut self, stop_at_rbrace: bool) {
        if self.is_recovery_boundary(&self.peek().kind)
            && (stop_at_rbrace || !matches!(self.peek().kind, TokenKind::RBrace))
        {
            return;
        }

        self.advance();

        while !self.is_at_end() {
            if self.is_recovery_boundary(&self.peek().kind)
                && (stop_at_rbrace || !matches!(self.peek().kind, TokenKind::RBrace))
            {
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
            _ => Err(Self::expected_error("identifier", span)),
        }
    }

    /// Parse the required name after `functio`.
    fn parse_function_name(&mut self) -> Result<Ident, ParseError> {
        self.parse_decl_name_after_keyword(
            ParseErrorKind::MissingFunctionName,
            "functio",
            "functio",
        )
    }

    /// Parse the required name after `genus`.
    fn parse_class_name(&mut self) -> Result<Ident, ParseError> {
        self.parse_decl_name_after_keyword(ParseErrorKind::MissingClassName, "genus", "genus")
    }

    fn parse_decl_name_after_keyword(
        &mut self,
        missing_kind: ParseErrorKind,
        keyword_arg: &'static str,
        keyword: &'static str,
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
                    Err(ParseError::structured(missing_kind, span).with_arg(keyword_arg, keyword))
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
    fn parse_contextual_ident(&mut self, owner: KeywordOwner) -> Result<Ident, ParseError> {
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
                    Err(Self::expected_error("identifier", span))
                }
            }
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
            _ => Err(Self::expected_error("string literal", token.span)),
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
#[path = "line_start_comment_test.rs"]
mod line_start_comment_test;

#[cfg(test)]
#[path = "enum_union_trivia_test.rs"]
mod enum_union_trivia_test;

#[cfg(test)]
#[path = "claim_test.rs"]
mod claim_test;

#[cfg(test)]
#[path = "generic_call_test.rs"]
mod generic_call_test;

#[cfg(test)]
#[path = "slot_helpers_test.rs"]
mod slot_helpers_test;

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

#[cfg(test)]
#[path = "schema_test.rs"]
mod schema_test;

#[cfg(test)]
mod non_trivia_index_tests {
    use super::*;
    use radix_lexer::lex;

    fn linear_peek_at(tokens: &[Token], start_pos: usize, offset: usize) -> &Token {
        let mut pos = start_pos;
        let mut skipped = 0;
        while pos < tokens.len() {
            if !tokens[pos].kind.is_trivia() {
                if skipped == offset {
                    return &tokens[pos];
                }
                skipped += 1;
            }
            pos += 1;
        }
        &tokens[tokens.len() - 1]
    }

    #[test]
    fn peek_at_matches_linear_trivia_skip() {
        let sources = [
            "a b c",
            "fixum _ x ← 1\n# c\nvaria y ← 2",
            "incipit {\n    fixum numerus x ← a ⇐ 2\n}",
        ];
        for src in sources {
            let result = lex(src);
            let tokens = result.tokens;
            let mut parser = Parser::new(tokens.clone(), result.interner, ParseOptions::default());
            loop {
                for offset in 0..3 {
                    let indexed = &parser.peek_at(offset).kind;
                    let linear = &linear_peek_at(&tokens, parser.pos, offset).kind;
                    assert_eq!(
                        std::mem::discriminant(indexed),
                        std::mem::discriminant(linear),
                        "src={src:?} pos={} offset={offset}",
                        parser.pos,
                        offset = offset
                    );
                }
                if parser.is_at_end() {
                    break;
                }
                let _ = parser.advance();
            }
        }
    }
}
