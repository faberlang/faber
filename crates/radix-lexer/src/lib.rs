//! Lexical front end for Faber source text (foundation crate).
//!
//! Package `radix` re-exports this crate as `radix::lexer` so embedding paths
//! stay stable while the crate DAG grows.
//!
//!
//! This module is the compiler boundary between raw UTF-8 source and the token
//! stream consumed by the parser. It owns source-position spans, token and
//! literal representation, keyword spelling policy, Unicode identifier
//! normalization, and lexical diagnostics. Later phases should not re-interpret
//! raw source spelling when a token, span, or interned symbol already carries
//! the contract they need.
//!
//! TOKENIZATION POLICY
//! ===================
//! - Lexing is single-pass and error-resilient: malformed input records a
//!   diagnostic and scanning continues where the cursor can recover.
//! - Spans are byte offsets into the original source; they preserve exact
//!   source locations even when identifiers are NFKC-normalized for symbol
//!   identity. Literal and comment payloads keep source bytes (no NFKC).
//! - Keyword recognition is mode-aware. Normal source keywords are reserved,
//!   while annotation names remain identifier-like after `@` so metadata can use
//!   language-looking words deliberately. Line-start `§` file directives were
//!   removed; `§` remains valid only inside string templates.
//! - The lexer does not own grammar recovery. It emits `Error` tokens and
//!   structured [`LexError`] values; parser and diagnostic layers decide how to
//!   present or recover from the surrounding syntax.
//!
//! INVARIANTS
//! ==========
//! - Every successful lex emits a terminal EOF token, even when errors were
//!   collected earlier in the stream.
//! - Tokens and symbols must travel with the [`Interner`] returned in the same
//!   [`LexResult`]; symbol ids are local to one lexer instance.
//! - Lexer errors are diagnostics, not panics. Invalid user source should not
//!   crash this phase.

mod cursor;
mod keywords;
mod scan;
mod token;

use std::collections::HashMap;

pub use cursor::Cursor;
pub use keywords::{
    keyword_allowed_as_ident, keyword_group_specs, keyword_specs, keyword_spelling_for_token,
    keyword_token_kind, lookup_keyword_spec, validate_keyword_groups, KeywordCategory,
    KeywordGroupSpec, KeywordGroupValidationError, KeywordOwner, KeywordScope, KeywordSpec,
};
pub use scan::INLINE_COMMENT_ISSUE;
pub use scan::{Interner, Lexer};
pub use token::{Span, Symbol, Token, TokenKind};

// =============================================================================
// PUBLIC API
// =============================================================================

/// Lex one Faber source buffer into parser-ready tokens.
///
/// This is the convenience boundary for callers that do not need to configure
/// scanner state directly. The returned [`LexResult`] keeps the token stream,
/// collected diagnostics, and symbol table together because interned symbols
/// are meaningful only against the interner that created them.
#[must_use]
pub fn lex(source: &str) -> LexResult {
    Lexer::new(source).lex()
}

/// Lex one source buffer with keyword spellings supplied by a [`KeywordPack`].
///
/// This is an explicit opt-in path for reader-locale activation. The default
/// [`lex`] function remains the canonical Latin source surface.
pub fn lex_with_locale_pack(source: &str, pack: &dyn KeywordPack) -> LexResult {
    Lexer::new(source).with_locale_pack(pack).lex()
}

/// Keyword surface used by pack-aware lexing.
///
/// Implemented by `LocalePack` in the `radix` package so this crate does
/// not depend on reader-locale loading or semantic type tables.
pub trait KeywordPack {
    /// Stable pack id for diagnostics.
    fn pack_id(&self) -> &str;

    /// Exact pack spelling → token kind.
    fn keyword_token_kind(&self, text: &str) -> Option<TokenKind>;

    /// Exact pack spelling → Latin keyword registry identity.
    ///
    /// Contextual keywords have no global [`TokenKind`], so pack-aware lexing
    /// records their canonical spelling for parser grammar slots such as
    /// callable posture without rewriting ordinary identifier payloads.
    fn keyword_latin_key(&self, _text: &str) -> Option<&str> {
        None
    }

    /// Exact pack type spelling → Latin type registry identity.
    ///
    /// Type names are parser-visible identifiers, so pack-aware lexing records
    /// localized primitive/collection spellings for parser-owned type slots
    /// without rewriting ordinary identifier payloads.
    fn type_latin_key(&self, _text: &str) -> Option<&str> {
        None
    }

    /// Explicit Latin fallback when the pack declares one.
    fn latin_keyword_fallback(&self, text: &str) -> Option<LatinKeywordFallback>;

    /// Suggested canonical spelling for near-miss identifiers.
    fn keyword_suggestion(&self, text: &str) -> Option<String>;
}

/// Latin keyword fallback selected by pack-aware lexing.
///
/// `token_kind` is `None` for tokenless keywords (migrated rows): the fallback
/// still fires loudly, but the word lexes as an identifier and the parser
/// claims it by spelling in its grammar slot.
#[derive(Debug, Clone, PartialEq)]
pub struct LatinKeywordFallback {
    pub token_kind: Option<TokenKind>,
    pub keyword: String,
    pub localized: Option<String>,
}

// =============================================================================
// TYPES
// =============================================================================

/// Complete output of one lexer run.
///
/// A result may contain errors and still carry a full token stream. That split
/// is intentional: downstream phases can attempt syntax recovery while the
/// diagnostics layer still reports precise lexical failures.
pub struct LexResult {
    /// Token stream in source order, always terminated by [`TokenKind::Eof`].
    pub tokens: Vec<Token>,

    /// Lexical diagnostics collected while scanning.
    pub errors: Vec<LexError>,

    /// Non-fatal reader-locale fallback events collected while scanning.
    pub locale_fallbacks: Vec<LocaleFallback>,

    /// Non-fatal reader-locale spelling suggestions collected while scanning.
    pub locale_suggestions: Vec<LocaleSuggestion>,

    /// Exact source spelling → Latin contextual keyword identity, for parser-owned slots.
    pub reader_keyword_latin_keys: HashMap<String, String>,

    /// Exact source spelling → Latin type identity, for parser-owned type slots.
    pub reader_type_latin_keys: HashMap<String, String>,

    /// Symbol table for identifiers, strings, and comments emitted above.
    pub interner: Interner,
}

impl LexResult {
    /// Return whether lexing produced no diagnostics.
    #[must_use]
    pub fn success(&self) -> bool {
        self.errors.is_empty()
    }
}

// =============================================================================
// ERRORS
// =============================================================================

use radix_diagnostics::DiagnosticArg;

/// Lexical failure classes — owned by `radix-diagnostics` so `Diagnostic` can
/// carry the typed kind without a crate cycle. Re-exported here for the
/// historical `radix_lexer::LexErrorKind` / `radix::lexer::LexErrorKind` path.
pub use radix_diagnostics::LexErrorKind;

/// One lexical diagnostic with a source span and named template facts.
///
/// The lexer records errors at the point where tokenization policy is violated,
/// but it does not format prose, multi-line reports, or decide parse recovery.
/// Those responsibilities belong to higher diagnostic layers.
#[derive(Debug, Clone)]
pub struct LexError {
    /// Machine-readable class for recovery and diagnostics.
    pub kind: LexErrorKind,

    /// Byte span in the original source that triggered the diagnostic.
    pub span: Span,

    /// Named facts for catalog templates and future locale renderers.
    pub args: Vec<DiagnosticArg>,
}

/// One visible fallback from a reader-locale source spelling to Latin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleFallback {
    /// Reader pack id active when fallback occurred.
    pub pack_id: String,

    /// Latin keyword spelling that was accepted through fallback.
    pub keyword: String,

    /// Localized canonical spelling from the pack, when the pack declares one.
    pub localized: Option<String>,

    /// Byte span of the fallback keyword spelling in the original source.
    pub span: Span,
}

/// One reader-locale spelling suggestion for an unaccepted keyword-like token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleSuggestion {
    /// Reader pack id active when the suggestion occurred.
    pub pack_id: String,

    /// Source spelling that was not accepted as a keyword.
    pub spelling: String,

    /// Canonical pack spelling that should be used instead.
    pub suggestion: String,

    /// Byte span of the misspelled source spelling in the original source.
    pub span: Span,
}
