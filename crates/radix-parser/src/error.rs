//! Parse-error data contract for the compiler front end.
//!
//! The parser reports syntax failures as lightweight diagnostics: a broad
//! category, named template facts, and source span. Human-readable prose belongs
//! to the diagnostics catalog and renderer, not parser emit sites.
//!
//! INVARIANTS
//! ==========
//! - Lexer failures are converted to `LexError` so callers can handle one parse
//!   result shape even when AST construction never begins.
//! - Every parser-produced error carries the current token span at the point the
//!   grammar contract failed.
//! - Error kinds should remain semantic enough for diagnostics, but not so
//!   granular that every grammar production needs a one-off enum variant.

use radix_diagnostics::{DiagnosticArg, PhaseErrorKind};
use radix_lexer::Span;

// =============================================================================
// ERROR TYPES
// =============================================================================

/// Syntax diagnostic emitted while building an `AST`.
///
/// `kind` is the machine-readable bucket, `args` carries named facts for
/// diagnostics, and `span` points to the token where parsing knew the construct
/// could not satisfy the grammar.
#[derive(Debug, Clone)]
pub struct ParseError {
    /// Broad diagnostic category for reporting and tests.
    pub kind: ParseErrorKind,

    /// Named facts for catalog templates and future locale renderers.
    pub args: Vec<DiagnosticArg>,

    /// Source location attached to the offending or missing construct.
    pub span: Span,

    /// Typed phase error kind preserved from upstream phases (e.g. lex).
    ///
    /// Set when this error bridges a `LexError` and the typed kind must
    /// survive the parse-phase boundary without Debug-text reconstruction.
    pub error_kind: Option<PhaseErrorKind>,
}

impl ParseError {
    /// Build a structured parser error whose primary text should come from the
    /// diagnostics catalog.
    #[must_use]
    pub fn structured(kind: ParseErrorKind, span: Span) -> Self {
        Self {
            kind,
            args: Vec::new(),
            span,
            error_kind: None,
        }
    }

    /// Attach a named template argument.
    #[must_use]
    pub fn with_arg(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.args.push(DiagnosticArg::new(name, value));
        self
    }
}

/// Parser diagnostic taxonomy — owned by `radix-diagnostics` so `Diagnostic`
/// can carry the typed kind without a crate cycle. Re-exported here for the
/// historical `radix_parser::ParseErrorKind` / `radix::parser::ParseErrorKind` path.
pub use radix_diagnostics::ParseErrorKind;

// =============================================================================
// STANDARD TRAIT IMPLEMENTATIONS
// =============================================================================

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.kind)
    }
}

impl std::error::Error for ParseError {}
