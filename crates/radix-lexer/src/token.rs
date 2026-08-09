//! Token and span data contracts shared after lexing.
//!
//! This module is the stable data boundary between the lexer and parser. It
//! defines compact byte spans, interned symbol handles, and the complete set of
//! token kinds the parser can observe. Scanner policy lives in `scan.rs`;
//! keyword taxonomy and alias metadata live in `keywords.rs`; this file keeps
//! the transport representation explicit and cheap to clone.
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

/// Complete token vocabulary visible to the parser.
///
/// This enum is deliberately broad: it includes current source tokens, metadata
/// tokens, error/recovery sentinels, and a small number of reserved internal
/// variants. Keep spelling policy in scanner/keyword tables; keep this type as
/// the parser-facing vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TokenKind {
    // === Identifiers and literals ===
    Ident(Symbol),
    Underscore(Symbol), // WHY: Special case for pattern matching wildcard
    /// WHY: Ordinary expression integers carry an unsigned `u64` magnitude so
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
    Discretio, // tagged union
    Fixum,     // const
    Functio,   // function
    Genus,     // class
    Iuncta,    // tuple type
    Implendum, // interface contract
    Importa,   // import
    Magnitudo, // compile-time size/index generic introducer
    Modulus,   // fixed-width modular word type family
    Ordo,      // enum
    Proba,     // test case
    Probandum, // test suite
    Sit,       // inferred immutable local (sugar for fixum _)
    Typus,     // type alias
    Varia,     // let/var
    // === Keywords: Modifiers ===
    Abstractus, // abstract
    Ceteri,     // variadic rest
    Curata,     // allocator requirement
    Errata,     // custom error type
    Exitus,     // exit code
    Generis,    // static
    Iacit,      // throws
    Immutata,   // pure function
    Nexum,      // bound method
    Optiones,   // options struct
    Prae,       // override
    Privata,    // private
    Protecta,   // protected
    Publica,    // public
    // === Keywords: Control flow ===
    Casu,     // case
    Ceterum,  // default
    Custodi,  // guard clauses
    Discerne, // pattern match
    Dum,      // while
    Elige,    // switch
    Ergo,     // 'ergo' — compact statement-body joint only
    Fac,      // do-while
    Itera,    // for
    Secus,    // else
    Si,       // if
    Sic,      // then (ternary)
    Sin,      // else if
    // === Keywords: Transfer ===
    Perge, // continue
    Redde, // return
    Rumpe, // break
    Tacet, // explicit noop
    // === Keywords: Error handling ===
    Adfirma, // assert
    Cape,    // catch
    Iace,    // throw
    Mori,    // panic
    // === Keywords: Async ===
    Cede,      // await/yield
    Clausura,  // closure
    Cursor,    // generator (annotation)
    Futura,    // async (annotation)
    Fiet,      // async finite (callable posture)
    Fiunt,     // sync stream (callable posture)
    Fient,     // async generator (callable posture)
    Figendum,  // await const binding
    Variandum, // await var binding
    Reddet,    // return_await
    Tacebit,   // await
    // === Keywords: Annotation families (wave 1) ===
    Brevis,     // short
    Cli,        // cli
    Descriptio, // description
    Fragment,   // fragment
    Imperium,   // command
    Longum,     // long
    Machina,    // device
    Nomen,      // name
    Nondum,     // unstable
    Nucleum,    // kernel
    Operandus,  // operand
    Optio,      // option
    Radix,      // radix
    Ubique,     // global
    Vertex,     // vertex
    // === Keywords: Boolean/null ===
    Falsum, // false
    Nihil,  // null/nil
    Verum,  // true
    // === Keywords: Logical operators ===
    Aut, // or
    Est, // identity check (===)
    Et,  // and
    Non, // not (!)
    Vel, // nullish coalesce (??)
    // === Keywords: Objects ===
    Ego,    // self/this
    Finge,  // construct tagged union variant
    Implet, // implements
    Sub,    // extends
    // === Keywords: Type operations ===
    Conversio, // ↦ — runtime value conversion
    Verte,     // ∷ — postfix static type ascription (qua/innatum/novum removed)
    // === Keywords: Output ===
    Mone,   // warn print
    Nota,   // neutral diagnostic note
    Scribe, // print
    Vide,   // debug print
    // === Keywords: Entry points ===
    Ad,        // HTTP endpoint
    Argumenta, // command-line args
    Cura,      // allocator scope
    Incipiet,  // main (async)
    Incipit,   // main (sync)
    Meus,      // `ad` outgoing arm (caller→gateway)
    Tuus,      // `ad` incoming arm (gateway→caller)
    // === Keywords: Misc ===
    De,        // borrow/from
    Ex,        // extract/move
    In,        // mutable borrow/in
    Lege,      // read input
    Lineam,    // read line
    Omnia,     // all (exhaustive match)
    Praefixum, // comptime
    Scriptum,  // interpolated script
    Sparge,    // spread operator
    Ut,        // as/alias
    // === Keywords: Ranges ===
    Ab,    // range/iteration mode
    Ante,  // exclusive end range
    Inter, // between
    Intra, // within
    Per,   // step
    Usque, // inclusive end range
    // === Keywords: Testing ===
    Fragilis,    // flaky test retries
    Futurum,     // todo test modifier
    Metior,      // benchmark
    Omitte,      // skip test
    Postpara,    // after each
    Postparabit, // after all
    Praepara,    // before each
    Praeparabit, // before all
    Repete,      // repeat test
    Requirit,    // test requires
    Solum,       // only this test
    SolumIn,     // only in environment
    Tag,         // test tag
    Temporis,    // test timeout
    // === Keywords: Nullability ===
    Negativum, // is negative check
    Nonnihil,  // is not nil check
    Nonnulla,  // is not null check
    Nulla,     // is null check
    Positivum, // is positive check
    // === Declaration markers (post-name) ===
    Sponte, // voluntary / optional declaration slot

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
    Cup,       // ∪ (inline union type T ∪ U, T ∪ nihil)
    At,        // @ (annotation marker)

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

    // === Comparison ===
    Eq,          // =
    Assign,      // ←
    ConversioAssign, // ↤ — conversion-directed assignment (U+21A4)
    EqEq,        // ≡
    EqEqEq,      // (reserved for future / internal; no source form)
    ApproxEq,    // ≈
    ApproxNotEq, // ≉
    BangEq,      // ≠
    BangEqEq,    // (reserved for future / internal; no source form)
    Lt,          // ≺ strict less-than comparison
    Gt,          // ≻ strict greater-than comparison
    LtAscii,     // < generic application delimiter (legacy comparison during migration)
    GtAscii,     // > generic application delimiter (legacy comparison during migration)
    LtEq,        // ≤
    GtEq,        // ≥

    // === Postfix increment/decrement (statement-only) ===
    PostInc, // ⊕
    PostDec, // ⊖

    // === Optional chaining ===
    QuestionDot,     // ?. (optional member access)
    QuestionBracket, // ?[ (optional index)
    QuestionParen,   // ?( (optional call)
    BangDot,         // !. (non-null assertion member)
    BangBracket,     // ![ (non-null assertion index)
    BangParen,       // !( (non-null assertion call)

    // === Range ===
    DotDot,   // ‥ (exclusive range)
    Ellipsis, // … (inclusive range)

    // === Compact clausura joint (glyph; never a keyword / pack row) ===
    Therefore, // ∴ — compact clausura joint only (not statement bodies)

    // === Comments and trivia ===
    LineComment(Symbol), // #
    Newline,             // \n (or \r\n normalized to one token)

    // === Special ===
    Eof,   // End of file marker
    Error, // Lexer error placeholder
}

impl TokenKind {
    /// Return whether this token kind is reserved-word syntax.
    ///
    /// The parser and diagnostics use this to distinguish "identifier-like"
    /// payloads from tokens that need keyword-specific messages or formatting.
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn is_keyword(&self) -> bool {
        #[allow(clippy::enum_glob_use)]
        use TokenKind::*;
        matches!(
            self,
            Discretio
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
                | Requirit
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
                | Machina
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
