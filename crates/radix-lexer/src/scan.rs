//! Stateful scanner for Faber tokens.
//!
//! This module turns the cursor and token definitions into the lexer. It owns
//! the tokenization state machine, line-start comment policy, literal parsing,
//! identifier NFKC interning, and the lexical recovery boundary. Parser recovery
//! starts after this module emits tokens; scanner recovery is limited to
//! consuming enough source to produce the next plausible token.
//!
//! TOKENIZATION POLICY
//! ===================
//! - Language keywords lex as identifiers. The scanner records latin identity
//!   keys for registered spellings; the parser claims them by identity.
//!   Annotation attachment boundaries are enforced in the parser, not via a
//!   lexer line mode.
//! - Identifier and keyword-lookup payloads are interned after Unicode NFKC
//!   normalization so visually equivalent identifier spellings share a symbol.
//!   Literal and comment payloads are interned as source bytes without NFKC
//!   (PRP-013). Spans always point at the original, unnormalized source.
//! - Numeric literals are parsed during lexing so later phases receive typed
//!   values, but invalid numbers remain lexical diagnostics instead of panics.
//! - Newlines reset line-scoped modes, update line state, and are emitted as
//!   explicit `TokenKind::Newline` trivia for leading-trivia attachment.
//!
//! ERROR STRATEGY
//! ==============
//! A malformed token emits a structured [`LexError`](super::LexError), often
//! paired with `TokenKind::Error`, and scanning continues. The scanner does not
//! synthesize missing syntax for the parser or reinterpret later bytes to hide
//! an earlier lexical failure.

use super::cursor::Cursor;
use super::keywords::{keyword_specs, keyword_token_kind};
use super::token::{Span, Symbol, Token, TokenKind};
use super::{LexError, LexErrorKind, LexResult, LocaleSuggestion, MixedScriptAdvisory};
use crate::KeywordPack;
use crate::textus_literal::{TextDelimiter, first_invalid_escape};
use handroll::FxHashMap;
use radix_diagnostics::DiagnosticArg;
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;
use unicode_script::UnicodeScript;

/// Lexer issue when `#` follows other tokens on the same logical line.
/// Stable `issue` fact for inline `#` comments after code (tests + catalogs).
pub const INLINE_COMMENT_ISSUE: &str = "inline_hash_after_code";

/// Lexer issue for a rejected glyph with a canonical operator/method hint.
/// Stable `issue` fact shared by the lexer tests and diagnostic catalog.
pub const HOMOGLYPH_ISSUE: &str = "homoglyph_did_you_mean";

/// String interner for lexer-produced symbols.
///
/// The interner is part of the lexer contract, not a global table. Symbol ids
/// are stable only inside one [`LexResult`](super::LexResult).
///
/// Two intern paths exist (PRP-013):
/// - [`Interner::intern`] — NFKC-normalizes for **identifier** / keyword-lookup
///   identity so visually equivalent Faber identifiers share a symbol. NFKC
///   subsumes NFC and applies compatibility decompositions (full-width Latin,
///   superscripts/subscripts, ligatures).
/// - [`Interner::intern_raw`] — stores **literal and comment** payloads as
///   source bytes with no Unicode normalization, so string values and comment
///   text are not silently rewritten.
///
/// TRADE-OFF: identifier normalization adds scanner cost, but it prevents later
/// semantic phases from having to reason about canonically equivalent spellings.
/// Literal source preservation keeps program data and diagnostics faithful.
#[derive(Clone)]
pub struct Interner {
    map: FxHashMap<String, Symbol>,
    strings: Vec<String>,
}

impl Interner {
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
            strings: Vec::new(),
        }
    }

    /// Intern identifier text under NFKC and return its symbol handle.
    ///
    /// Use for identifiers and keyword-lookup keys only. The returned handle
    /// resolves to the normalized spelling, not necessarily the byte-exact
    /// spelling in source. Use token spans when diagnostics need the original
    /// bytes. For string, ascii, backtick, octeti, and comment payloads, call
    /// [`Interner::intern_raw`] instead.
    pub fn intern(&mut self, s: &str) -> Symbol {
        let normalized: String = s.nfkc().collect();
        self.intern_exact(normalized)
    }

    /// Intern text the caller has already NFKC-normalized.
    ///
    /// # Contract
    /// `s` must already be NFKC-normalized (for example by `scan_identifier`).
    /// Skipping a second NFKC pass removes a redundant allocation on the
    /// identifier hot path. Callers that hold unnormalized source must use
    /// [`Interner::intern`] instead.
    pub(crate) fn intern_normalized(&mut self, s: &str) -> Symbol {
        self.intern_exact(s.to_owned())
    }

    /// Intern payload text without Unicode normalization.
    ///
    /// Use for delimited literal contents and comment text so source bytes are
    /// preserved in the resolved symbol.
    pub fn intern_raw(&mut self, s: &str) -> Symbol {
        self.intern_exact(s.to_owned())
    }

    fn intern_exact(&mut self, key: String) -> Symbol {
        if let Some(&sym) = self.map.get(&key) {
            return sym;
        }
        #[allow(clippy::cast_possible_truncation)]
        let sym = Symbol(self.strings.len() as u32);
        self.strings.push(key.clone());
        self.map.insert(key, sym);
        sym
    }

    /// Resolve a symbol back to its interned string content.
    ///
    /// Identifier symbols resolve to the NFKC form; literal and comment symbols
    /// resolve to the source payload as stored.
    #[must_use]
    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.strings[sym.0 as usize]
    }

    /// Look up the symbol for a byte-exact interned string.
    ///
    /// Read-only reverse lookup over the intern table: no normalization, no
    /// insertion. Returns `None` for text that was never interned.
    #[must_use]
    pub fn lookup(&self, s: &str) -> Option<Symbol> {
        self.map.get(s).copied()
    }

    /// Return interned strings in symbol-id order.
    #[must_use]
    pub fn strings(&self) -> &[String] {
        &self.strings
    }

    /// Return interned strings in symbol-id order as an owned table.
    ///
    /// # Wire contract
    ///
    /// - `strings: Vec<String>` is the serialized wire shape.
    /// - `Symbol(u32)` is the index into this vec (`strings[idx]`).
    /// - The lookup `map: FxHashMap<String, Symbol>` is **not** serialized.
    ///   It is rebuilt on load by calling `Interner::from_strings()` (or
    ///   equivalent) which re-interns each entry without re-normalizing.
    /// - NFKC normalization for identifiers is applied at intern time, so
    ///   the stored strings are already in canonical form. Rebuilding the
    ///   map on load is `O(n)` and produces the same `String → Symbol`
    ///   mapping as the original interner.
    #[must_use]
    pub fn to_string_table(&self) -> Vec<String> {
        self.strings.clone()
    }

    /// Rebuild an interner from strings stored in symbol-id order.
    ///
    /// Entries are already-resolved payloads (NFKC for identifiers, source
    /// bytes for literals/comments). Re-intern without re-normalizing so raw
    /// literal forms survive round-trips.
    #[must_use]
    pub fn from_strings(strings: Vec<String>) -> Self {
        let mut interner = Self::new();
        for string in strings {
            interner.intern_raw(&string);
        }
        interner
    }

    /// Rebuild an interner from the serialized string table.
    ///
    /// Alias for [`Self::from_strings`] that matches the wire-contract
    /// terminology used by the HIR artifact loader.
    #[must_use]
    pub fn from_string_table(strings: Vec<String>) -> Self {
        Self::from_strings(strings)
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// LEXER STATE
// =============================================================================

/// Scanner state for one Faber source buffer.
///
/// A lexer instance owns its cursor, token buffer, diagnostics, and interner so
/// the output cannot accidentally mix symbols from another source. Construct a
/// fresh lexer per source buffer.
pub struct Lexer<'a> {
    cursor: Cursor<'a>,
    locale_pack: Option<&'a dyn KeywordPack>,
    keyword_surface: crate::KeywordSurfaceMode,
    has_latin_fallback: bool,
    interner: Interner,
    tokens: Vec<Token>,
    errors: Vec<LexError>,
    locale_suggestions: Vec<LocaleSuggestion>,
    mixed_script_advisories: Vec<MixedScriptAdvisory>,
    reader_keyword_latin_keys: HashMap<String, String>,
    reader_keyword_surfaces: HashMap<String, String>,
    reader_type_latin_keys: HashMap<String, String>,
    reader_type_surfaces: HashMap<String, String>,
    /// True once any non-trivia token has been emitted on the current line.
    line_has_code: bool,
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub fn new(source: &'a str) -> Self {
        Self {
            cursor: Cursor::new(source),
            locale_pack: None,
            keyword_surface: crate::KeywordSurfaceMode::RegistryDirect,
            has_latin_fallback: false,
            interner: Interner::new(),
            tokens: Vec::new(),
            errors: Vec::new(),
            locale_suggestions: Vec::new(),
            mixed_script_advisories: Vec::new(),
            reader_keyword_latin_keys: HashMap::new(),
            reader_keyword_surfaces: HashMap::new(),
            reader_type_latin_keys: HashMap::new(),
            reader_type_surfaces: HashMap::new(),
            line_has_code: false,
        }
    }

    /// Activate reader-pack keyword recognition for this lexer instance.
    #[must_use]
    pub fn with_locale_pack(mut self, pack: &'a dyn KeywordPack) -> Self {
        self.locale_pack = Some(pack);
        self.keyword_surface = crate::KeywordSurfaceMode::from_pack_id(Some(pack.pack_id()));
        self.has_latin_fallback = pack.has_latin_fallback();
        for spec in keyword_specs() {
            if let Some(spelling) = pack.keyword_spelling(spec.text) {
                self.reader_keyword_surfaces
                    .insert(spec.text.to_owned(), spelling.to_owned());
            }
        }
        for (latin_key, surface) in pack.type_surfaces() {
            self.reader_type_surfaces.insert(latin_key, surface);
        }
        self
    }

    /// Consume the source and return all lexer products together.
    ///
    /// The method always emits EOF after scanning the source, even if earlier
    /// tokens produced diagnostics. This keeps parser entrypoints simple and
    /// makes lexical failure non-fatal to syntax recovery.
    #[must_use]
    pub fn lex(mut self) -> LexResult {
        while !self.cursor.is_eof() {
            self.scan_token();
        }
        self.emit_token(TokenKind::Eof, self.cursor.pos());
        LexResult {
            tokens: self.tokens,
            errors: self.errors,
            locale_suggestions: self.locale_suggestions,
            mixed_script_advisories: self.mixed_script_advisories,
            reader_keyword_latin_keys: self.reader_keyword_latin_keys,
            reader_keyword_surfaces: self.reader_keyword_surfaces,
            reader_type_latin_keys: self.reader_type_latin_keys,
            reader_type_surfaces: self.reader_type_surfaces,
            has_latin_fallback: self.has_latin_fallback,
            keyword_surface: self.keyword_surface,
            interner: self.interner,
        }
    }

    fn emit_token(&mut self, kind: TokenKind, start: u32) {
        if Self::token_counts_as_line_code(&kind) {
            self.line_has_code = true;
        }
        self.tokens
            .push(Token::new(kind, Span::new(start, self.cursor.pos())));
    }

    fn token_counts_as_line_code(kind: &TokenKind) -> bool {
        *kind != TokenKind::Newline && !matches!(kind, TokenKind::LineComment(_))
    }

    fn emit_error(&mut self, kind: LexErrorKind, start: u32, args: Vec<DiagnosticArg>) {
        self.errors.push(LexError {
            kind,
            span: Span::new(start, self.cursor.pos()),
            args,
        });
    }

    fn emit_issue_error(&mut self, kind: LexErrorKind, start: u32, issue: &'static str) {
        self.emit_error(kind, start, vec![DiagnosticArg::new("issue", issue)]);
    }

    fn scan_operator(&mut self, c: char) -> Option<TokenKind> {
        let kind = match c {
            '.' => TokenKind::Dot,
            '‥' => TokenKind::DotDot,
            '…' => TokenKind::Ellipsis,
            '∧' => TokenKind::BitwiseAnd,
            '∨' => TokenKind::BitwiseOr,
            '⊻' => TokenKind::BitwiseXor,
            '¬' => TokenKind::BitwiseNot,
            '⇐' => TokenKind::BitwiseShl,
            '⇒' => TokenKind::BitwiseShr,
            '⇇' => TokenKind::Transfer,
            '⇜' => TokenKind::SinkInto,
            '↦' => TokenKind::Conversio,
            '↤' => TokenKind::ConversioAssign,
            '↢' => TokenKind::AwaitAssign,
            '↑' => TokenKind::PostInc,
            // Rounding delimiters: `⌊x⌋` floor, `⌈x⌉` ceiling, `⌊x⌉` nearest.
            '⌊' => TokenKind::FloorOpen,
            '⌋' => TokenKind::FloorClose,
            '⌈' => TokenKind::CeilOpen,
            '⌉' => TokenKind::CeilClose,
            '↓' => TokenKind::PostDec,
            '∷' => TokenKind::Verte,

            // Glyph products (tensor-glyphs family); single-codepoint scans.
            '·' => TokenKind::MiddleDot,
            '×' => TokenKind::Cross,
            '⊗' => TokenKind::CircledTimes,
            '⊙' => TokenKind::CircledDot,
            '⊘' => TokenKind::CircledDivide,
            '÷' => TokenKind::Obelus,
            // Prefix radicals (radical-glyphs family); single-codepoint scans.
            '√' => TokenKind::RadicalSqrt,
            '∛' => TokenKind::RadicalCbrt,
            '∜' => TokenKind::RadicalFourth,

            // Infix extrema (minmax-infix-glyphs family).
            '⤒' => TokenKind::Maximum,
            '⤓' => TokenKind::Minimum,

            '∞' => TokenKind::Float(f64::INFINITY),

            // Postfix gradient selection (tensor-glyphs family).
            '∇' => TokenKind::Nabla,

            '∴' => TokenKind::Therefore,
            '∪' => TokenKind::Cup,
            '∩' => TokenKind::Cap,
            // Membership `x ∈ xs` / `x ∉ xs`. One spelling in every locale, no ASCII alias.
            '∈' => TokenKind::ElementOf,
            '∉' => TokenKind::NotElementOf,
            '+' => TokenKind::Plus,
            '*' => TokenKind::Star,
            '%' => TokenKind::Percent,
            '-' => TokenKind::Minus,
            '=' => TokenKind::Eq,
            '!' => {
                if self.cursor.eat('.') {
                    TokenKind::BangDot
                } else if self.cursor.eat('[') {
                    TokenKind::BangBracket
                } else if self.cursor.eat('(') {
                    TokenKind::BangParen
                } else {
                    TokenKind::Bang
                }
            }
            '<' => TokenKind::LtAscii,
            '>' => TokenKind::GtAscii,
            '≺' => TokenKind::Lt,
            '≻' => TokenKind::Gt,
            '←' => TokenKind::Assign,
            '≡' => TokenKind::EqEq,
            '≢' => TokenKind::BangEq,
            // FABER-2-SUNSET: U+2260 is retired (ruling 6b5c1c615); `≢` is the
            // one negation of `≡`. It lexes to its own token so the parser can
            // record `PARSE030 neq_glyph_retired` and keep parsing.
            '≠' => TokenKind::BangEqRetired,
            '≅' => TokenKind::ApproxEq,
            '≇' => TokenKind::ApproxNotEq,
            '≈' => TokenKind::FuzzyEq,
            '≉' => TokenKind::FuzzyNotEq,
            '≤' => TokenKind::LtEq,
            '≥' => TokenKind::GtEq,
            '→' => TokenKind::Arrow,
            '⇥' => TokenKind::ExitArrow,
            // D17.7 default channel: `↦ T ⊥ value`. Same glyph in every locale.
            '⊥' => TokenKind::UpTack,
            // D4.5 value conditional `c ✓ a ✗ b`. Same glyphs in every locale.
            '✓' => TokenKind::Check,
            '✗' => TokenKind::Ballot,
            // D2.1 format operator `value ¶ "spec"`. Same glyph in every locale.
            '¶' => TokenKind::Pilcrow,
            '?' => {
                if self.cursor.eat('.') {
                    TokenKind::QuestionDot
                } else if self.cursor.eat('[') {
                    TokenKind::QuestionBracket
                } else if self.cursor.eat('(') {
                    TokenKind::QuestionParen
                } else {
                    TokenKind::Question
                }
            }
            _ => return None,
        };

        Some(kind)
    }

    fn emit_line_break(&mut self, start: u32) {
        self.line_has_code = false;
        self.emit_token(TokenKind::Newline, start);
    }

    fn scan_line_break(&mut self, start: u32, c: char) -> bool {
        match c {
            '\n' => {
                self.emit_line_break(start);
                true
            }
            '\r' => {
                if self.cursor.peek() == Some('\n') {
                    self.cursor.advance();
                }
                self.emit_line_break(start);
                true
            }
            _ => false,
        }
    }

    fn clean_numeric_text(text: &str) -> String {
        text.chars().filter(|&c| c != '_').collect()
    }

    /// Return `true` when `text` contains at least one non-zero digit in the
    /// significand (the part before any `e`/`E` exponent separator). Used to
    /// distinguish true zero (`0e-400`) from underflow (`1e-400`).
    fn has_non_zero_significand(text: &str) -> bool {
        for c in text.chars() {
            match c {
                '1'..='9' => return true,
                'e' | 'E' => break,
                _ => {}
            }
        }
        false
    }

    fn scan_radix_int<P>(&mut self, start: u32, radix: u32, predicate: P, issue: &'static str)
    where
        P: Fn(char) -> bool,
    {
        self.cursor.eat_while(|c| predicate(c) || c == '_');

        // Trailing digits (or letters that look numeric-adjacent) that fail
        // the radix predicate must not leak as separate tokens. Consume them
        // and emit an error instead.
        if self
            .cursor
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() && !predicate(c))
        {
            self.cursor
                .eat_while(|c| c.is_ascii_alphanumeric() || c == '_');
            self.emit_issue_error(LexErrorKind::NumberInvalid, start, issue);
            self.emit_token(TokenKind::Error, start);
            return;
        }

        let text = self.cursor.slice(start + 2, self.cursor.pos());
        let clean = Self::clean_numeric_text(text);

        if let Ok(n) = u64::from_str_radix(&clean, radix) {
            self.emit_token(TokenKind::Integer(n), start);
        } else if !clean.is_empty() {
            // inf track (ruling 34): a digit string beyond `u64` is a big
            // literal; the digits were validated by the radix predicate. The
            // spelling keeps its radix prefix so consumers re-parse exactly.
            let prefix = self.cursor.slice(start, start + 2).to_ascii_lowercase();
            self.emit_big_integer(&format!("{prefix}{clean}"), start);
        } else {
            self.emit_issue_error(LexErrorKind::NumberInvalid, start, issue);
            self.emit_token(TokenKind::Error, start);
        }
    }

    /// Emit an integer literal too large for `u64` as [`TokenKind::BigInteger`]
    /// carrying its cleaned digit text (decimal, or `0x`/`0o`/`0b` prefixed).
    fn emit_big_integer(&mut self, text: &str, start: u32) {
        let text = self.interner.intern_raw(text);
        self.emit_token(TokenKind::BigInteger(text), start);
    }

    /// Intern a delimited literal payload after the closing delimiter is found or recovery ends.
    fn finish_delimited_literal(
        &mut self,
        start: u32,
        open_len: u32,
        close_len: u32,
        terminated: bool,
    ) -> Symbol {
        let suffix_len = if terminated { close_len } else { 0 };
        let content_start = start + open_len;
        let content_end = self.cursor.pos().saturating_sub(suffix_len);
        let text = self.cursor.slice(content_start, content_end);
        // Literal payloads preserve source bytes (PRP-013); do not NFKC.
        self.interner.intern_raw(text)
    }

    /// Emit one `LEX003 unknown_escape` diagnostic for the first escape in the
    /// literal's payload the delimiter does not allow.
    ///
    /// Only terminated literals are checked: recovery text has no reliable
    /// payload boundary, and the unterminated literal already has its own
    /// diagnostic. The token is still emitted either way.
    fn check_literal_escapes(
        &mut self,
        start: u32,
        sym: Symbol,
        delimiter: TextDelimiter,
        terminated: bool,
    ) {
        if !terminated {
            return;
        }
        let Some((_, escape)) = first_invalid_escape(self.interner.resolve(sym), delimiter) else {
            return;
        };
        self.emit_error(
            LexErrorKind::EscapeInvalid,
            start,
            vec![
                DiagnosticArg::new("issue", "unknown_escape"),
                DiagnosticArg::new("escape", escape.to_string()),
            ],
        );
    }

    /// Skip one shallow escape sequence (`\` plus the next source character).
    fn skip_escaped_char(&mut self) {
        self.cursor.advance();
        self.cursor.advance();
    }

    /// Skip one escape inside a single-line literal.
    ///
    /// WHY (PRP-014): `\<newline>` must not consume the newline inside the
    /// escape skip. Advance past just the backslash so the next loop iteration
    /// hits the newline rejection branch.
    fn skip_single_line_escape(&mut self) {
        if matches!(self.cursor.peek_next(), Some('\n' | '\r')) {
            self.cursor.advance();
        } else {
            self.skip_escaped_char();
        }
    }

    /// Scan one token or recoverable lexical error from the current position.
    ///
    /// This is the scanner's dispatch boundary. Specialized scanners own
    /// token-family policy, while this method owns mode reset, whitespace, and
    /// fallback behavior for characters with no token meaning.
    fn scan_token(&mut self) {
        let start = self.cursor.pos();

        let Some(c) = self.cursor.advance() else {
            return;
        };

        if self.scan_line_break(start, c) {
            return;
        }

        if matches!(c, ' ' | '\t') {
            return;
        }

        let kind = match c {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            ':' => TokenKind::Colon,
            ';' => TokenKind::Semicolon,
            '@' => TokenKind::At,
            '#' => return self.scan_hash_comment(start),
            '"' => return self.scan_string(start),
            '\'' => return self.scan_ascii_string(start),
            '`' => return self.scan_backtick_string(start),
            '|' => return self.scan_octeti_string(start),
            '«' => return self.scan_guillemet_block_string(start),
            '/' => return self.scan_slash(start),
            '0'..='9' => return self.scan_number(start, c),
            // U+1D40 is XID-compatible after NFKC, so claim it before the
            // identifier arm can normalize it to plain `T`.
            'ᵀ' => return self.emit_token(TokenKind::Transpose, start),
            c if is_ident_start(c) => return self.scan_identifier(start),
            c => {
                if let Some(kind) = self.scan_operator(c) {
                    kind
                } else if let Some(suggestion) = homoglyph_suggestion(c) {
                    self.emit_error(
                        LexErrorKind::CharacterUnexpected,
                        start,
                        vec![
                            DiagnosticArg::new("issue", HOMOGLYPH_ISSUE),
                            DiagnosticArg::new("char", c.to_string()),
                            DiagnosticArg::new("suggestion", suggestion),
                        ],
                    );
                    TokenKind::Error
                } else {
                    self.emit_error(
                        LexErrorKind::CharacterUnexpected,
                        start,
                        vec![
                            DiagnosticArg::new("issue", "unexpected_character"),
                            DiagnosticArg::new("char", diagnostic_char(c)),
                        ],
                    );
                    TokenKind::Error
                }
            }
        };

        self.emit_token(kind, start);
    }

    // =========================================================================
    // COMMENT SCANNERS
    // =========================================================================

    /// Scan a hash comment, the only source comment form currently accepted.
    ///
    /// `#` must be the first non-whitespace token on the logical line. Mid-line
    /// `#` after code is a lexical error so comments stay line-start trivia.
    fn scan_hash_comment(&mut self, start: u32) {
        if self.line_has_code {
            self.reject_inline_hash_comment(start);
            return;
        }

        self.cursor.eat_while(|c| !matches!(c, '\n' | '\r'));

        let text = self.cursor.slice(start + 1, self.cursor.pos());
        // Comment payloads preserve source bytes (PRP-013); do not NFKC.
        let sym = self.interner.intern_raw(text.trim_end());

        self.emit_token(TokenKind::LineComment(sym), start);
    }

    fn reject_inline_hash_comment(&mut self, start: u32) {
        self.cursor.eat_while(|c| !matches!(c, '\n' | '\r'));
        self.emit_issue_error(LexErrorKind::InlineComment, start, INLINE_COMMENT_ISSUE);
        self.emit_token(TokenKind::Error, start);
    }

    /// Scan `/` as division, a regex body opener, or a rejected C-style comment.
    ///
    /// WHY: `//` and `/* */` are ubiquitous in other languages but are not Faber
    /// trivia. Emitting two `Slash` tokens leaves the parser reporting a useless
    /// "expected expression" at statement boundaries.
    ///
    /// Ruling d381ae56: when the previous emitted token is the regex type word
    /// or `_`, the slash opens a raw regex body instead of dividing. The empty
    /// body check runs before the C-style comment branch so `regex //` reports
    /// `empty_regex_literal`, not a line comment.
    fn scan_slash(&mut self, start: u32) {
        if self.previous_token_is_regex_type_word() {
            self.scan_regex_body(start);
            return;
        }

        match self.cursor.peek() {
            Some('/') => {
                self.cursor.advance();
                self.cursor.eat_while(|c| !matches!(c, '\n' | '\r'));
                self.emit_issue_error(
                    LexErrorKind::CommentSyntaxInvalid,
                    start,
                    "c_style_line_comment",
                );
                self.emit_token(TokenKind::Error, start);
            }
            Some('*') => {
                self.cursor.advance();
                let mut terminated = false;
                while let Some(c) = self.cursor.advance() {
                    if c == '*' && self.cursor.eat('/') {
                        terminated = true;
                        break;
                    }
                }
                let issue = if terminated {
                    "c_style_block_comment"
                } else {
                    "unterminated_c_style_block_comment"
                };
                self.emit_issue_error(LexErrorKind::CommentSyntaxInvalid, start, issue);
                self.emit_token(TokenKind::Error, start);
            }
            _ => self.emit_token(TokenKind::Slash, start),
        }
    }

    /// Whether the token immediately before the current `/` opens a regex body.
    ///
    /// Only the regex type word or `_` qualifies. The type word is resolved
    /// through the active pack: with a pack, its Latin identity must be
    /// `regex`; without one, the spelling itself must be `regex`. An identifier
    /// preceded by `.` is a member access, not the type word.
    fn previous_token_is_regex_type_word(&self) -> bool {
        let Some(previous) = self.tokens.last() else {
            return false;
        };
        let sym = match previous.kind {
            TokenKind::Underscore(_) => return true,
            TokenKind::Ident(sym) => sym,
            _ => return false,
        };

        if self.tokens.len() >= 2
            && matches!(self.tokens[self.tokens.len() - 2].kind, TokenKind::Dot)
        {
            return false;
        }

        let text = self.interner.resolve(sym);
        match self.locale_pack {
            Some(pack) => pack.type_latin_key(text) == Some("regex"),
            None => text == "regex",
        }
    }

    /// Scan a raw `/.../` regex body opened by the type word or `_`.
    ///
    /// The body runs to the next unescaped `/` on the same line. A backslash
    /// consumes the following character so `\/` does not terminate and stays
    /// in the payload as authored. Newline or EOF before the closing slash
    /// reports `unterminated_regex_literal`; an immediate closing slash reports
    /// `empty_regex_literal` and emits [`TokenKind::Error`]. A slash inside a
    /// character class still ends the body because the scanner does not parse
    /// classes, so `regex /[\/]/` is the escaped form and `regex /[/]/` stops
    /// the body at the bracket.
    fn scan_regex_body(&mut self, start: u32) {
        if self.cursor.peek() == Some('/') {
            self.cursor.advance();
            let issue = "empty_regex_literal";
            self.emit_issue_error(LexErrorKind::UnterminatedString, start, issue);
            self.emit_token(TokenKind::Error, start);
            return;
        }

        let mut terminated = false;
        loop {
            match self.cursor.peek() {
                None | Some('\n' | '\r') => {
                    self.emit_issue_error(
                        LexErrorKind::UnterminatedString,
                        start,
                        "unterminated_regex_literal",
                    );
                    break;
                }
                Some('\\') => self.skip_single_line_escape(),
                Some('/') => {
                    self.cursor.advance();
                    terminated = true;
                    break;
                }
                _ => {
                    self.cursor.advance();
                }
            }
        }

        let sym = self.finish_delimited_literal(start, 1, 1, terminated);
        self.emit_token(TokenKind::RegexString(sym), start);
    }

    // =========================================================================
    // STRING SCANNERS
    // =========================================================================

    /// Scan a single-line `ascii` literal delimited by single quotes.
    fn scan_ascii_string(&mut self, start: u32) {
        let mut terminated = false;
        loop {
            match self.cursor.peek() {
                None => {
                    self.emit_issue_error(
                        LexErrorKind::UnterminatedString,
                        start,
                        "unterminated_ascii_literal",
                    );
                    break;
                }
                Some('\n' | '\r') => {
                    self.emit_issue_error(
                        LexErrorKind::UnterminatedString,
                        start,
                        "newline_in_ascii_literal",
                    );
                    break;
                }
                Some('\\') => self.skip_single_line_escape(),
                Some('\'') => {
                    self.cursor.advance();
                    terminated = true;
                    break;
                }
                _ => {
                    self.cursor.advance();
                }
            }
        }

        let sym = self.finish_delimited_literal(start, 1, 1, terminated);
        self.emit_token(TokenKind::AsciiString(sym), start);
    }

    /// Scan a captured `forma` template literal delimited by backticks.
    ///
    /// Backtick templates may span multiple lines. Escape handling follows the
    /// same shallow skip policy as [`scan_string`]: the scanner skips a
    /// backslash plus the next character so delimiter search stays correct.
    fn scan_backtick_string(&mut self, start: u32) {
        let mut terminated = false;
        loop {
            match self.cursor.peek() {
                None => {
                    self.emit_issue_error(
                        LexErrorKind::UnterminatedString,
                        start,
                        "unterminated_backtick_template_literal",
                    );
                    break;
                }
                Some('\\') => self.skip_escaped_char(),
                Some('`') => {
                    self.cursor.advance();
                    terminated = true;
                    break;
                }
                _ => {
                    self.cursor.advance();
                }
            }
        }

        let sym = self.finish_delimited_literal(start, 1, 1, terminated);
        self.check_literal_escapes(start, sym, TextDelimiter::Backtick, terminated);
        self.emit_token(TokenKind::BacktickString(sym), start);
    }

    /// Scan a compile-time `octeti` literal delimited by ASCII pipes.
    ///
    /// Interior content is raw hex text plus whitespace. `||` is the empty
    /// literal. Validation of hex pairs happens during semantic analysis.
    fn scan_octeti_string(&mut self, start: u32) {
        let mut terminated = false;
        loop {
            match self.cursor.peek() {
                None => {
                    self.emit_issue_error(
                        LexErrorKind::UnterminatedString,
                        start,
                        "unterminated_octeti_literal",
                    );
                    break;
                }
                Some('|') => {
                    self.cursor.advance();
                    terminated = true;
                    break;
                }
                _ => {
                    self.cursor.advance();
                }
            }
        }

        let sym = self.finish_delimited_literal(start, 1, 1, terminated);
        self.emit_token(TokenKind::OctetiString(sym), start);
    }

    /// Scan a single-line string literal delimited by double quotes.
    ///
    /// Single-line strings recover at newline or EOF. Escape handling is
    /// intentionally shallow here: the scanner skips a backslash plus the next
    /// character so delimiter search stays correct, but it does not validate an
    /// escape vocabulary.
    fn scan_string(&mut self, start: u32) {
        let mut terminated = false;
        loop {
            match self.cursor.peek() {
                None => {
                    self.emit_issue_error(
                        LexErrorKind::UnterminatedString,
                        start,
                        "unterminated_string_literal",
                    );
                    break;
                }
                Some('\n' | '\r') => {
                    self.emit_issue_error(
                        LexErrorKind::UnterminatedString,
                        start,
                        "newline_in_string_literal",
                    );
                    break;
                }
                Some('\\') => self.skip_single_line_escape(),
                Some('"') => {
                    self.cursor.advance();
                    terminated = true;
                    break;
                }
                _ => {
                    self.cursor.advance();
                }
            }
        }

        let sym = self.finish_delimited_literal(start, 1, 1, terminated);
        self.check_literal_escapes(start, sym, TextDelimiter::Quote, terminated);
        self.emit_token(TokenKind::String(sym), start);
    }

    /// Scan a block `textus` literal delimited by guillemets `«` and `»`.
    ///
    /// Block strings may contain newlines and unescaped `"` characters. Backslash
    /// escapes follow the same shallow skip policy as [`scan_string`].
    fn scan_guillemet_block_string(&mut self, start: u32) {
        let mut terminated = false;

        loop {
            match self.cursor.peek() {
                None => {
                    self.emit_issue_error(
                        LexErrorKind::UnterminatedString,
                        start,
                        "unterminated_guillemet_block_string_literal",
                    );
                    break;
                }
                Some('\\') => self.skip_escaped_char(),
                Some('»') => {
                    self.cursor.advance();
                    terminated = true;
                    break;
                }
                _ => {
                    self.cursor.advance();
                }
            }
        }

        #[allow(clippy::cast_possible_truncation)]
        let open_len = '«'.len_utf8() as u32;
        #[allow(clippy::cast_possible_truncation)]
        let close_len = '»'.len_utf8() as u32;
        let sym = self.finish_delimited_literal(start, open_len, close_len, terminated);
        self.check_literal_escapes(start, sym, TextDelimiter::Guillemet, terminated);
        self.emit_token(TokenKind::String(sym), start);
    }

    // =========================================================================
    // NUMBER SCANNER
    // =========================================================================

    /// Scan a numeric literal and store its parsed value in the token.
    ///
    /// Faber accepts decimal integers/floats, exponent forms, radix-prefixed
    /// integers, and `_` separators. A `.` starts a float only when followed by
    /// a digit, preserving member access and range tokens after integers.
    fn scan_number(&mut self, start: u32, first: char) {
        if first == '0' {
            match self.cursor.peek() {
                Some('x' | 'X') => {
                    self.cursor.advance();
                    self.scan_radix_int(
                        start,
                        16,
                        |c| c.is_ascii_hexdigit(),
                        "invalid_hexadecimal_number",
                    );
                    return;
                }
                Some('b' | 'B') => {
                    self.cursor.advance();
                    self.scan_radix_int(
                        start,
                        2,
                        |c| c == '0' || c == '1',
                        "invalid_binary_number",
                    );
                    return;
                }
                Some('o' | 'O') => {
                    self.cursor.advance();
                    self.scan_radix_int(
                        start,
                        8,
                        |c| ('0'..='7').contains(&c),
                        "invalid_octal_number",
                    );
                    return;
                }
                _ => {}
            }
        }

        self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');

        let is_float = if self.cursor.peek() == Some('.')
            && self.cursor.peek_next().is_some_and(|c| c.is_ascii_digit())
        {
            self.cursor.advance();
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
            true
        } else {
            false
        };

        let has_exp = if self.cursor.peek() == Some('e') || self.cursor.peek() == Some('E') {
            self.cursor.advance();
            if !self.cursor.eat('+') {
                self.cursor.eat('-');
            }
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
            true
        } else {
            false
        };

        let text = self.cursor.slice(start, self.cursor.pos());
        let clean = Self::clean_numeric_text(text);

        if is_float || has_exp {
            match clean.parse::<f64>() {
                Ok(n) if n.is_infinite() => {
                    self.emit_issue_error(LexErrorKind::NumberInvalid, start, "float_overflow");
                    self.emit_token(TokenKind::Error, start);
                }
                Ok(n) if n == 0.0 && Self::has_non_zero_significand(&clean) => {
                    self.emit_issue_error(LexErrorKind::NumberInvalid, start, "float_underflow");
                    self.emit_token(TokenKind::Error, start);
                }
                Ok(n) => {
                    // decimal-widths D1: carry the digit text alongside the f64
                    // re-derivation so decimal widths never lose exactness (R0).
                    let text = self.interner.intern_raw(&clean);
                    self.emit_token(TokenKind::FloatText { value: n, text }, start);
                }
                Err(_) => {
                    self.emit_issue_error(
                        LexErrorKind::NumberInvalid,
                        start,
                        "invalid_floating_point_number",
                    );
                    self.emit_token(TokenKind::Error, start);
                }
            }
        } else if let Ok(n) = clean.parse::<u64>() {
            self.emit_token(TokenKind::Integer(n), start);
        } else if !clean.is_empty() && clean.bytes().all(|b| b.is_ascii_digit()) {
            // A decimal digit string beyond `u64` (inf track, ruling 34).
            self.emit_big_integer(&clean, start);
        } else {
            self.emit_issue_error(LexErrorKind::NumberInvalid, start, "invalid_integer");
            self.emit_token(TokenKind::Error, start);
        }
    }

    // =========================================================================
    // IDENTIFIER AND KEYWORD SCANNER
    // =========================================================================

    /// Scan a Unicode identifier and record keyword identity when applicable.
    ///
    /// Identifier boundaries use Unicode XID rules plus `_`. Registered
    /// keyword spellings still lex as identifiers; their latin keys are
    /// recorded for parser claim.
    fn scan_identifier(&mut self, start: u32) {
        // U+1D40 is XID-compatible under NFKC, but it is a postfix operator
        // rather than identifier text. Make it a lexical boundary so `xᵀ`
        // produces `Ident("x")`, `Transpose` instead of one normalized name.
        self.cursor.eat_while(|c| c != 'ᵀ' && is_ident_continue(c));

        let text = self.cursor.slice(start, self.cursor.pos());
        if let Some(suggestion) = homoglyph_identifier_suggestion(text) {
            self.emit_error(
                LexErrorKind::CharacterUnexpected,
                start,
                vec![
                    DiagnosticArg::new("issue", HOMOGLYPH_ISSUE),
                    DiagnosticArg::new("char", text),
                    DiagnosticArg::new("suggestion", suggestion),
                ],
            );
            self.emit_token(TokenKind::Error, start);
            return;
        }
        let normalized: String = text.nfkc().collect();
        let span = Span::new(start, self.cursor.pos());
        // Non-fatal advisory (S-A5): the identifier still lexes normally.
        if let Some(scripts) = mixed_script_names(&normalized) {
            self.mixed_script_advisories.push(MixedScriptAdvisory {
                spelling: normalized.clone(),
                scripts,
                span,
            });
        }
        let kind = self.keyword_or_ident(&normalized, span);
        self.emit_token(kind, start);
    }

    /// Resolve keyword spellings under the active source surface, or intern identifier text.
    ///
    /// `text` is already NFKC-normalized by [`Lexer::scan_identifier`]. Identifier
    /// paths use [`Interner::intern_normalized`] so NFKC runs once per token.
    ///
    /// Keyword tokens are never emitted: every keyword lexes as an identifier
    /// with its latin identity recorded in [`Lexer::reader_keyword_latin_keys`],
    /// and the parser claims keywords by identity through the registry. This is
    /// locale-independent — pack spellings resolve to the same latin identity.
    fn keyword_or_ident(&mut self, text: &str, span: Span) -> TokenKind {
        if text == "_" {
            return TokenKind::Underscore(self.interner.intern_normalized(text));
        }

        let Some(pack) = self.locale_pack else {
            // Normal (Latin) surface: every registered keyword lexes as an
            // identifier; the parser claims it by identity. Recording the
            // latin key (== source text here) keeps parser-side resolution
            // uniform with pack modes.
            if keyword_token_kind(text).is_some() {
                self.reader_keyword_latin_keys
                    .insert(text.to_string(), text.to_string());
            }
            return TokenKind::Ident(self.interner.intern_normalized(text));
        };

        // Pack surface. Declared keyword/type spellings lex as identifiers
        // with their latin key recorded; the parser claims them by identity.
        let mut declared = false;
        if let Some(latin_key) = pack.keyword_latin_key(text) {
            self.reader_keyword_latin_keys
                .insert(text.to_string(), latin_key.to_string());
            declared = true;
        }

        if let Some(latin_key) = pack.type_latin_key(text) {
            self.reader_type_latin_keys
                .insert(text.to_string(), latin_key.to_string());
            declared = true;
        }

        // An exact declared keyword/type spelling is not a near-miss. It lexes
        // as an identifier and is claimed by the parser (keywords in grammar
        // slots) or canonicalized through `reader_type_latin_keys` (types), so
        // it must skip the LOCALE002 suggestion and fallback passes.
        if declared {
            return TokenKind::Ident(self.interner.intern_normalized(text));
        }

        if let Some(suggestion) = pack.keyword_suggestion(text) {
            self.locale_suggestions.push(LocaleSuggestion {
                pack_id: pack.pack_id().to_owned(),
                spelling: text.to_string(),
                suggestion,
                span,
            });
        }

        TokenKind::Ident(self.interner.intern_normalized(text))
    }
}

fn homoglyph_identifier_suggestion(text: &str) -> Option<&'static str> {
    // U+1D57 normalizes to plain `t`, but is not the admitted modifier-capital
    // T operator. Reject the source token with the same locale-invariant
    // did-you-mean fact as other rejected glyphs.
    (text == "ᵗ").then_some("ᵀ")
}

/// Classify the distinct visible Unicode scripts of `text` (already
/// NFKC-normalized by [`Lexer::scan_identifier`]).
///
/// Common and Inherited characters — digits, `_`, combining marks — carry no
/// script of their own and never count toward the mix, so `α_1` stays a
/// single-script name. Unassigned codepoints (`Script::Unknown`) are skipped
/// for the same reason. Returns `Some` only when the name spans two or more
/// scripts: the mixed-script confusable shape.
fn mixed_script_names(text: &str) -> Option<Vec<String>> {
    let mut scripts: Vec<&'static str> = Vec::new();
    for c in text.chars() {
        let script = c.script();
        if matches!(
            script,
            unicode_script::Script::Common
                | unicode_script::Script::Inherited
                | unicode_script::Script::Unknown
        ) {
            continue;
        }
        let name = script.full_name();
        if !scripts.contains(&name) {
            scripts.push(name);
        }
    }
    if scripts.len() < 2 {
        return None;
    }
    scripts.sort_unstable();
    Some(scripts.into_iter().map(str::to_owned).collect())
}

fn homoglyph_suggestion(c: char) -> Option<&'static str> {
    match c {
        // `∘` is reserved for a future fusion surface; `⊙` is the live
        // Hadamard product glyph.
        '∘' => Some("⊙"),
        // Norms remain method-shaped. Keep the hint independent of any
        // reader-locale pack so lexer behavior is stable across locales.
        '‖' | '∥' => Some(".rms_norm(...)"),

        // Vertical arrow neighbors outside the live ↑/↓ postfix pair. The
        // Supplemental Arrows-A set is deliberately explicit: U+2908/U+2909
        // carry a horizontal stroke and U+290A/U+290B carry three heads.
        // Other vertical arrows with the same directional reading and a bar,
        // dash, or double head are rejected rather than normalized. Both
        // forms are close enough to the to-bar glyphs to merit a hint. ↑ and ↓
        // are intentionally absent because they are already live tokens.
        '⤈' | '⤋' | '⇣' | '⇓' | '↧' => Some("⤓"),
        '⤉' | '⤊' | '⇡' | '⇑' | '↥' => Some("⤒"),
        // U+27C2 PERPENDICULAR looks like the `⊥` (U+22A5) default glyph.
        '⟂' => Some("⊥"),
        // Heavy dingbat variants of the D4.5 conditional glyphs.
        '✔' => Some("✓"),
        '✘' => Some("✗"),
        _ => None,
    }
}

fn diagnostic_char(c: char) -> String {
    if c.is_control() || is_bidi_control(c) {
        format!("U+{:04X}", c as u32)
    } else {
        format!("'{}'", c.escape_default())
    }
}

fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
    )
}

// =============================================================================
// IDENTIFIER PREDICATES
// =============================================================================

/// Return whether a character can start a Faber identifier.
///
/// Faber follows Unicode XID start rules and additionally permits `_` for
/// conventional unused bindings and internal-style names.
fn is_ident_start(c: char) -> bool {
    unicode_ident::is_xid_start(c) || c == '_'
}

/// Return whether a character can continue a Faber identifier.
fn is_ident_continue(c: char) -> bool {
    unicode_ident::is_xid_continue(c) || c == '_'
}

#[cfg(test)]
#[path = "scan_test.rs"]
mod scan_test;

#[cfg(test)]
#[path = "unsigned_integer_test.rs"]
mod unsigned_integer_test;
