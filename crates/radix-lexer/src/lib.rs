//! Lexical front end for Faber source text (foundation crate).
//!
//! Package `radix` re-exports this crate as `radix::lexer` so embedding paths
//! stay stable while the crate DAG grows.
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
//! - Language keywords lex as identifiers. The scanner records latin identity
//!   keys for registered spellings; the parser claims them by identity.
//!   `§` is valid only inside string templates.
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

/// Literal payload contracts: the lexer reports raw `Ascii`/`Octeti`/`Textus`
/// tokens with shallow delimiter matching; these modules own the source-level
/// decode/escape/validation contracts shared by semantic analysis, forma, and
/// MIR lowering.
pub mod ascii_literal;
pub mod octeti_literal;
pub mod textus_literal;

#[cfg(test)]
#[path = "ascii_literal_test.rs"]
mod ascii_literal_tests;

#[cfg(test)]
#[path = "octeti_literal_test.rs"]
mod octeti_literal_tests;

#[cfg(test)]
#[path = "regex_literal_census_test.rs"]
mod regex_literal_census_tests;

use std::collections::HashMap;

pub use cursor::Cursor;
pub use keywords::{
    KeywordGroupSpec, KeywordGroupValidationError, KeywordKind, KeywordOwner, KeywordScope,
    KeywordSpec, keyword_allowed_as_ident, keyword_group_specs, keyword_specs,
    keyword_spelling_for_token, keyword_token_kind, lookup_keyword_spec, validate_keyword_groups,
};
pub use scan::{HOMOGLYPH_ISSUE, INLINE_COMMENT_ISSUE};
pub use scan::{Interner, Lexer};
pub use token::{Span, Symbol, Token, TokenKind};

/// Keyword-claim surface for one lexer run.
///
/// The parser gates raw-source keyword claiming on this mode. Under
/// [`KeywordSurfaceMode::PackAuthoritative`] — a non-`la` reader pack — a word
/// claims a grammar/owner keyword slot only when the lexer recorded its
/// pack-declared latin key; an undeclared spelling stays an ordinary
/// identifier. [`KeywordSurfaceMode::RegistryDirect`] — the `la` pack or no
/// pack — keeps the historical registry-direct claim with a raw-source
/// fallback, so tokenless keywords the lexer deliberately does not record
/// still claim through the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordSurfaceMode {
    /// `la` pack or no pack: parser claims by registry identity, with a
    /// raw-source fallback for spellings the lexer did not record.
    RegistryDirect,
    /// Non-`la` reader pack: only pack-declared spellings (recorded latin
    /// keys) claim grammar/owner keyword slots.
    PackAuthoritative,
}

impl KeywordSurfaceMode {
    /// Derive the effective surface from an active pack id.
    ///
    /// `la` is the identity surface (registry-direct); any other pack id is
    /// pack-authoritative. `None` (no pack) is the plain Latin surface.
    #[must_use]
    pub fn from_pack_id(pack_id: Option<&str>) -> Self {
        match pack_id {
            None | Some("la") => KeywordSurfaceMode::RegistryDirect,
            Some(_) => KeywordSurfaceMode::PackAuthoritative,
        }
    }
}

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

    /// Latin keyword registry identity → the pack's declared source spelling.
    ///
    /// Parser diagnostics use this reverse view when a registered keyword is
    /// written in an undeclared surface. Implementations may omit a row only
    /// for intentionally partial test packs; the parser then falls back to the
    /// canonical Latin spelling.
    fn keyword_spelling(&self, _latin_key: &str) -> Option<&str> {
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

    /// Localized type-surface rows as `(Latin type identity, declared spelling)`.
    ///
    /// Mirrors [`Self::keyword_spelling`] for type names. It returns the whole
    /// row set rather than taking one identity at a time because this crate
    /// owns the keyword registry (enumerable in [`crate::keyword_specs`]) while
    /// the type registry lives downstream, so the pack is the only side that
    /// can enumerate its own type rows. Packs that declare none return the
    /// default empty list.
    fn type_surfaces(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Whether canonical Latin type spellings are an explicit pack fallback.
    ///
    /// This is separate from keyword claiming: a fallback pack keeps its
    /// declared keyword surface authoritative while allowing missing type rows
    /// to retain their canonical Latin spelling. The parser uses this only to
    /// suppress the wrong-locale type seal for an explicitly configured `la`
    /// fallback.
    fn has_latin_fallback(&self) -> bool {
        false
    }

    /// Suggested canonical spelling for near-miss identifiers.
    fn keyword_suggestion(&self, text: &str) -> Option<String>;
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

    /// Non-fatal reader-locale spelling suggestions collected while scanning.
    pub locale_suggestions: Vec<LocaleSuggestion>,

    /// Non-fatal mixed-script identifier advisories collected while scanning.
    pub mixed_script_advisories: Vec<MixedScriptAdvisory>,

    /// Exact source spelling → Latin contextual keyword identity, for parser-owned slots.
    pub reader_keyword_latin_keys: HashMap<String, String>,

    /// Latin keyword identity → declared source spelling, for locale diagnostics.
    pub reader_keyword_surfaces: HashMap<String, String>,

    /// Exact source spelling → Latin type identity, for parser-owned type slots.
    pub reader_type_latin_keys: HashMap<String, String>,

    /// Latin type identity → declared source spelling, for locale diagnostics.
    ///
    /// The reverse view of [`LexResult::reader_type_latin_keys`]: a parser
    /// diagnostic names a type by its reader-locale surface rather than the
    /// locale-independent Latin identity it canonicalized to.
    pub reader_type_surfaces: HashMap<String, String>,

    /// Whether the active pack explicitly permits canonical Latin type fallback.
    pub has_latin_fallback: bool,

    /// Keyword-claim surface for this run, derived from the active pack.
    pub keyword_surface: KeywordSurfaceMode,

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

/// One identifier whose visible characters span two or more Unicode scripts.
///
/// The screen only observes the NFKC-normalized text (S-A5): the token still
/// lexes as a normal identifier, so no compile that succeeds today can start
/// failing. The defect addressed is silence — a Cyrillic `а` beside Latin
/// `dd` now produces this advisory instead of passing unnoticed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixedScriptAdvisory {
    /// NFKC-normalized identifier text that triggered the advisory.
    pub spelling: String,

    /// Distinct visible script names, sorted; at least two. Common/Inherited
    /// characters (digits, `_`, combining marks) are excluded.
    pub scripts: Vec<String>,

    /// Byte span of the identifier in the original source.
    pub span: Span,
}
