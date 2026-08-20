//! Token and span data contracts shared after lexing.
//!
//! This module is the stable data boundary between the lexer and parser. It
//! owns compact byte spans, interned symbol handles, and the parser-facing
//! token vocabulary. Scanner policy lives in `scan.rs`; keyword taxonomy lives
//! in `keywords.rs`. Keyword variants here are identity tokens the parser
//! claims by spelling; the scanner does not emit them as reserved words.
//!
//! INVARIANTS
//! ==========
//! - Spans are byte offsets into the original UTF-8 source, not character
//!   counts and not offsets into normalized strings.
//! - `Symbol` values are indexes into the [`Interner`](super::Interner) that
//!   came from the same lexer run.
//! - Literal tokens carry parsed numeric values; source spelling is recovered
//!   through spans when diagnostics or formatters need it.
//!
//! COMPATIBILITY
//! =============
//! Some variants are reserved for parser or internal syntax surfaces even when
//! no current source spelling produces them. Removing such variants is a
//! language representation change, not just lexer cleanup.

use serde::{Deserialize, Serialize};

// Source spans live in `radix-diagnostics` (shared currency for lex + diag).
pub use radix_diagnostics::Span;

// =============================================================================
// TOKENS
// =============================================================================

/// One lexical item paired with its original source location.
///
/// Tokens never borrow source text directly. Payload text moves through
/// [`Symbol`] handles, and exact spelling remains available through the span
/// and original source buffer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Token {
    /// Classified token payload emitted by the scanner.
    pub kind: TokenKind,

    /// Source bytes consumed to produce this token.
    pub span: Span,
}

impl Token {
    #[must_use]
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

// =============================================================================
// SYMBOLS
// =============================================================================

/// Interned string handle local to one lexer result.
///
/// Symbols are cheap to copy and compare, but they are not globally meaningful.
/// Resolve them only with the [`Interner`](super::Interner) that produced the
/// token stream containing the symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol(pub u32);

// =============================================================================
// TOKEN KINDS
// =============================================================================

/// Parser-facing token vocabulary.
///
/// Includes source tokens the scanner emits, keyword identity variants the
/// parser claims, metadata and recovery sentinels, and a few reserved internal
/// variants with no current source spelling. Spelling policy lives in the
/// scanner and keyword tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TokenKind {
    // === Identifiers and literals ===
    Ident(Symbol),
    /// Pattern-matching wildcard; not an ordinary identifier.
    Underscore(Symbol),
    /// Ordinary expression integers carry an unsigned `u64` magnitude so
    /// source values through `u64::MAX` survive lexing. Negative expressions
    /// remain unary negation over this magnitude; signed consumers (JSON,
    /// enum values, CLI, annotations) convert with explicit checked bounds.
    Integer(u64),
    Float(f64),
    String(Symbol),
    /// Single-quoted `'...'` payload interned without delimiters.
    AsciiString(Symbol),
    /// Backtick `` `...` `` `forma` template payload interned without delimiters.
    BacktickString(Symbol),
    /// Pipe-delimited `|...|` hex payload interned without delimiters.
    OctetiString(Symbol),

    // === Keywords: Declarations ===
    Columna,
    Discretio,
    Fixum,
    Functio,
    Genus,
    Iuncta,
    Implendum,
    Importa,
    Magnitudo,
    Modulus,
    Ordo,
    Proba,
    Probandum,
    Sit, // inferred immutable local; sugar for `fixum _`
    Schema,
    Typus,
    Varia,
    // === Keywords: Modifiers ===
    Abstractus,
    Ceteri,
    Curata,
    Errata,
    Exitus,
    Generis,
    Iacit,
    Immutata,
    Interna,
    Nexum,
    Optiones,
    Prae,
    Privata,
    Protecta,
    Publica,
    // === Keywords: Control flow ===
    Casu,
    Ceterum,
    Custodi,
    Discerne,
    Dum,
    Elige,
    Ergo, // compact statement-body joint only
    Fac,
    Itera,
    Secus,
    Si,
    Sic,
    Sin,
    // === Keywords: Transfer ===
    Perge,
    Redde,
    Rumpe,
    Tacet,
    // === Keywords: Error handling ===
    Adfirma,
    Cape,
    Iace,
    Mori,
    Requirit,
    // === Keywords: Async ===
    Cede,
    Clausura,
    Cursor,
    Futura,
    Fiet,
    Fiunt,
    Fient,
    Figendum,
    Variandum,
    Reddet,
    Tacebit,
    // === Keywords: Annotation families ===
    Brevis,
    Cli,
    Descriptio,
    Fragment,
    Imperium,
    Longum,
    Nomen,
    Nondum,
    Nucleum,
    Operandus,
    Optio,
    Radix,
    Ubique,
    Vertex,
    // === Keywords: Boolean/null ===
    Falsum,
    Nihil,
    Verum,
    // === Keywords: Logical operators ===
    Aut,
    Est,
    Et,
    Non,
    Vel,
    // === Keywords: Objects ===
    Ego,
    Finge,
    Implet,
    Sub,
    // === Keywords: Type operations ===
    Conversio, // ↦
    Verte,     // ∷ postfix ascription; qua/innatum/novum removed
    // === Keywords: Output ===
    Mone,
    Nota,
    Scribe,
    Vide,
    // === Keywords: Entry points ===
    Ad,
    Argumenta,
    Cura,
    Incipiet,
    Incipit,
    Meus, // `ad` outgoing arm (caller → gateway)
    Tuus, // `ad` incoming arm (gateway → caller)
    // === Keywords: Misc ===
    De,
    Ex,
    In,
    Own,
    Copy,
    Lege,
    Lineam,
    Omnia,
    Praefixum,
    Scriptum,
    Sparge,
    Ut,
    // === Keywords: Ranges ===
    Ab,
    Ante,
    Inter,
    Intra,
    Per,
    Usque,
    // === Keywords: Testing ===
    Fragilis,
    Futurum,
    Metior,
    Omitte,
    Postpara,
    Postparabit,
    Praepara,
    Praeparabit,
    Repete,
    Solum,
    SolumIn,
    Tag,
    Temporis,
    // === Keywords: Nullability ===
    Negativum,
    Nonnihil,
    Nonnulla,
    Nulla,
    Positivum,
    // === Declaration markers (post-name) ===
    Sponte,

    // === Punctuation ===
    LParen,    // (
    RParen,    // )
    LBrace,    // {
    RBrace,    // }
    LBracket,  // [
    RBracket,  // ]
    Comma,     // ,
    Colon,     // :
    Semicolon, // ;
    Dot,       // .
    Arrow,     // →
    ExitArrow, // ⇥
    Cup,       // ∪ inline union
    At,        // @

    // === Operators ===
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Percent,    // %
    BitwiseAnd, // ∧
    BitwiseOr,  // ∨
    BitwiseXor, // ⊻
    BitwiseNot, // ¬
    BitwiseShl, // ⇐
    BitwiseShr, // ⇒
    Bang,       // !
    Question,   // ?

    // === Glyph products (tensor-glyphs family) ===
    MiddleDot,    // · U+00B7 inner product (dot / matvec / matmul)
    Cross,        // × U+00D7 cross product (3-vectors)
    CircledTimes, // ⊗ U+2297 outer / Kronecker product
    CircledDot,   // ⊙ U+2299 Hadamard / elementwise product

    // === Postfix gradient selection (tensor-glyphs family) ===
    Nabla, // ∇ U+2207 postfix gradient operator: `call(...) ∇ [places]`

    // === Comparison ===
    Eq,              // =
    Assign,          // ←
    ConversioAssign, // ↤ U+21A4
    EqEq,            // ≡
    EqEqEq,          // reserved; no source form
    ApproxEq,        // ≈
    ApproxNotEq,     // ≉
    BangEq,          // ≠
    BangEqEq,        // reserved; no source form
    Lt,              // ≺
    Gt,              // ≻
    LtAscii,         // < generic delimiter (legacy comparison during migration)
    GtAscii,         // > generic delimiter (legacy comparison during migration)
    LtEq,            // ≤
    GtEq,            // ≥

    // === Postfix increment/decrement (statement-only) ===
    PostInc, // ↑
    PostDec, // ↓

    // === Optional chaining ===
    QuestionDot,     // ?.
    QuestionBracket, // ?[
    QuestionParen,   // ?(
    BangDot,         // !.
    BangBracket,     // ![
    BangParen,       // !(

    // === Range ===
    DotDot,   // ‥ exclusive
    Ellipsis, // … inclusive

    // === Compact clausura joint (glyph; never a keyword / pack row) ===
    Therefore, // ∴ compact clausura joint only (not statement bodies)

    // === Comments and trivia ===
    LineComment(Symbol), // #
    Newline,             // \n (or \r\n normalized to one token)

    // === Special ===
    Eof,
    Error,
}

impl TokenKind {
    /// Return whether this token kind is a keyword identity variant.
    ///
    /// The scanner does not emit reserved words. This predicate classifies
    /// parser-claimed identities and a few glyph-owned or annotation-owned
    /// tokens that diagnostics and formatters treat as keyword-like.
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn is_keyword(&self) -> bool {
        #[allow(clippy::enum_glob_use)]
        use TokenKind::*;
        matches!(
            self,
            Columna
                | Discretio
                | Fixum
                | Functio
                | Genus
                | Iuncta
                | Implendum
                | Importa
                | Magnitudo
                | Modulus
                | Ordo
                | Proba
                | Probandum
                | Sit
                | Schema
                | Typus
                | Varia
                | Abstractus
                | Ceteri
                | Curata
                | Errata
                | Exitus
                | Generis
                | Iacit
                | Immutata
                | Interna
                | Nexum
                | Optiones
                | Prae
                | Privata
                | Protecta
                | Publica
                | Casu
                | Ceterum
                | Custodi
                | Discerne
                | Dum
                | Elige
                | Ergo
                | Fac
                | Itera
                | Secus
                | Si
                | Sic
                | Sin
                | Perge
                | Redde
                | Rumpe
                | Tacet
                | Adfirma
                | Cape
                | Iace
                | Mori
                | Requirit
                | Cede
                | Clausura
                | Cursor
                | Futura
                | Falsum
                | Nihil
                | Verum
                | Aut
                | Est
                | Et
                | Non
                | Vel
                | Ego
                | Finge
                | Implet
                | Sub
                | Conversio
                | Verte
                | Mone
                | Nota
                | Scribe
                | Vide
                | Ad
                | Argumenta
                | Cura
                | Incipiet
                | Incipit
                | Meus
                | Tuus
                | De
                | Ex
                | In
                | Own
                | Copy
                | Lege
                | Lineam
                | Omnia
                | Praefixum
                | Scriptum
                | Sparge
                | Ut
                | Ante
                | Inter
                | Intra
                | Per
                | Usque
                | Fragilis
                | Futurum
                | Metior
                | Omitte
                | Postpara
                | Postparabit
                | Praepara
                | Praeparabit
                | Repete
                | Solum
                | SolumIn
                | Tag
                | Temporis
                | Negativum
                | Nonnihil
                | Nonnulla
                | Nulla
                | Positivum
                | Sponte
                | Ab
                | Fiet
                | Fiunt
                | Fient
                | Figendum
                | Variandum
                | Reddet
                | Tacebit
                | Brevis
                | Cli
                | Descriptio
                | Fragment
                | Imperium
                | Longum
                | Nomen
                | Nondum
                | Nucleum
                | Operandus
                | Optio
                | Radix
                | Ubique
                | Vertex
        )
    }

    /// Return whether this token carries source comment text.
    ///
    /// Parsers may skip comments while tools such as formatters preserve them.
    #[must_use]
    pub fn is_comment(&self) -> bool {
        matches!(self, TokenKind::LineComment(_))
    }

    /// Return whether this token is leading/trailing trivia skipped during infix parsing.
    #[must_use]
    pub fn is_trivia(&self) -> bool {
        matches!(self, TokenKind::LineComment(_) | TokenKind::Newline)
    }
}

#[cfg(test)]
#[path = "token_test.rs"]
mod tests;
