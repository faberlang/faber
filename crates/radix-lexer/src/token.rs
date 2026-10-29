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
    /// An integer literal beyond `u64` (inf track, ruling 34): the cleaned
    /// digit text, decimal or `0x`/`0o`/`0b` prefixed. Its value is judged
    /// where it lands — any `inf` slot holds it, every bounded slot rejects it.
    BigInteger(Symbol),
    Float(f64),
    /// Decimal literal preserving exact digit text (decimal-widths D1, R0 fix).
    /// `value` is the f64 re-derivation used by the fractus path — identical to
    /// the old `Float(f64)` collapse. `text` is the cleaned decimal spelling.
    FloatText {
        value: f64,
        text: Symbol,
    },
    String(Symbol),
    /// Single-quoted `'...'` payload interned without delimiters.
    AsciiString(Symbol),
    /// Backtick `` `...` `` `forma` template payload interned without delimiters.
    BacktickString(Symbol),
    /// Pipe-delimited `|...|` hex payload interned without delimiters.
    OctetiString(Symbol),
    /// Raw `/.../` regex body interned without the slashes.
    ///
    /// Emitted only directly after the regex type word or `_` (ruling
    /// d381ae56): a bare `/` in operand position stays division. Backslash
    /// escapes are kept as authored (`\/` stays `\/`) because the backend
    /// receives the body source bytes.
    RegexString(Symbol),

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
    Ceteri,
    Errata,
    Exitus,
    Generis,
    Iacit,
    Immutata,
    Interna,
    Optiones,
    Privata,
    Protecta,
    Publica,
    // === Keywords: Control flow ===
    Apud,
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
    Sin,
    // === Keywords: Transfer ===
    Perge,
    Redde,
    Rumpe,
    Tacet,
    // === Keywords: Error handling ===
    Adfirma,
    Cape,
    Trap,
    Iace,
    Mori,
    Requirit,
    Reice,
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
    /// `intutus` (en `unsafe`): the `@ nucleum` tier that also drops unproven index checks.
    Intutus,
    /// `laxus` (en `relaxed`): the `@ nucleum` tier that allows relaxed float math.
    Laxus,
    Longum,
    Lane,
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
    // === Keywords: Type operations ===
    Conversio, // ↦
    Verte,     // ∷ postfix ascription
    // === Keywords: Output ===
    Mone,
    Nota,
    Scribe,
    Vide,
    // === Keywords: Entry points ===
    Ad,
    Argumenta,
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
    /// `axis`: marks a number as an axis index at a call and in a signature.
    Axis,
    Lege,
    Lineam,
    Omnia,
    /// `machina` (en `device`): the device-region modifier directly after `fac`.
    Machina,
    Praefixum,
    /// Build-time file embed in a constant initializer (en `embed`, D8.10).
    Insere,
    Scriptum,
    Sparge,
    Ut,
    /// Convert-hint clause opener after a `↦` target (`x ↦ u32 via Hex`, D11.9).
    Via,
    // === Keywords: Ranges ===
    Ab,
    Ante,
    Per,
    Usque,
    // === Keywords: Testing ===
    Erratur,
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
    // === Keywords: Null literal ===
    Nulla,
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
    UpTack,    // ⊥ default channel (D17.7)
    Check,     // ✓ value conditional then-marker (D4.5)
    Ballot,    // ✗ value conditional else-marker (D4.5)
    Pilcrow,   // ¶ format operator (D2.1)
    Cup,       // ∪ inline union
    Cap,       // ∩ inline intersection
    At,        // @

    // === Membership glyphs (no ASCII or word spelling) ===
    ElementOf,    // ∈ membership over a range or collection
    NotElementOf, // ∉ non-membership over a range or collection

    // === Operators ===
    Plus,   // +
    Minus,  // -
    Star,   // *
    Slash,  // /
    Obelus, // ÷ U+00F7 true division

    // === Prefix radicals (radical-glyphs family) ===
    RadicalSqrt,   // √ U+221A prefix square root (never a keyword)
    RadicalCbrt,   // ∛ U+221B prefix real cube root (never a keyword)
    RadicalFourth, // ∜ U+221C prefix fourth root (never a keyword)

    Percent,    // %
    BitwiseAnd, // ∧
    BitwiseOr,  // ∨
    BitwiseXor, // ⊻
    BitwiseNot, // ¬
    BitwiseShl, // ⇐
    BitwiseShr, // ⇒
    Transfer,   // ⇇ U+21C7 copy-into operator (never a keyword)
    SinkInto,   // ⇜ U+21DC sink-into operator (never a keyword)
    Bang,       // !
    Question,   // ?

    // === Glyph products (tensor-glyphs family) ===
    MiddleDot,     // · U+00B7 inner product (dot / matvec / matmul)
    Cross,         // × U+00D7 cross product (3-vectors)
    CircledTimes,  // ⊗ U+2297 outer / Kronecker product
    CircledDot,    // ⊙ U+2299 Hadamard / elementwise product
    CircledDivide, // ⊘ U+2298 Hadamard / elementwise divide

    // === Infix extrema (minmax-infix-glyphs family) ===
    Maximum, // ⤒ U+2912 max / upper bound
    Minimum, // ⤓ U+2913 min / lower bound

    // === Postfix tensor transpose (tensor-glyphs family) ===
    Transpose, // ᵀ U+1D40 rank-2 postfix transpose sugar

    // === Postfix gradient selection (tensor-glyphs family) ===
    Nabla, // ∇ U+2207 postfix gradient operator: `call(...) ∇ [places]`

    // === Comparison ===
    Eq,              // =
    Assign,          // ←
    AwaitAssign,     // ↢ U+21A2
    ConversioAssign, // ↤ U+21A4
    EqEq,            // ≡
    EqEqEq,          // reserved; no source form
    ApproxEq,        // ≅ promoted-exact
    ApproxNotEq,     // ≇ promoted-exact negation
    FuzzyEq,         // ≈ tolerance compare
    FuzzyNotEq,      // ≉ tolerance compare negation
    BangEq,          // ≢
    BangEqRetired,   // ≠ (U+2260, retired; parser records neq_glyph_retired)
    BangEqEq,        // reserved; no source form
    Lt,              // ≺
    Gt,              // ≻
    LtAscii,         // < generic delimiter (legacy comparison during migration)
    GtAscii,         // > generic delimiter (legacy comparison during migration)
    LtEq,            // ≤
    GtEq,            // ≥

    // === Postfix increment/decrement (statement-only) ===
    PostInc,    // ↑
    FloorOpen,  // ⌊ (U+230A)
    FloorClose, // ⌋ (U+230B)
    CeilOpen,   // ⌈ (U+2308)
    CeilClose,  // ⌉ (U+2309)
    PostDec,    // ↓

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
