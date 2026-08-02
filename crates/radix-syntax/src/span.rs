//! Source-location access for syntax nodes.
//!
//! The lexer owns the concrete [`Span`] representation; syntax nodes only need
//! a small shared contract for phases that report diagnostics or map compiler
//! facts back to source. `Spanned` keeps that contract narrow so AST shape can
//! evolve without forcing every caller to pattern match just to name a location.
//!
//! INVARIANT
//! =========
//! A span is diagnostic provenance. It should identify the source construct that
//! produced a node, not encode semantic ownership, lifetime, or backend extent.

use super::ast::{Annotation, Expr, Ident, Program, Stmt, TypeExpr};
use radix_lexer::Span;

/// Trait for syntax values that can report their source location.
///
/// Diagnostics use this trait when they only need provenance. More precise
/// phase-specific ranges should be stored separately instead of overloading the
/// AST's original parse span.
pub trait Spanned {
    /// Return the parse span associated with this value.
    fn span(&self) -> Span;
}

impl Spanned for Program {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for Stmt {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for Expr {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for Ident {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for TypeExpr {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for Annotation {
    fn span(&self) -> Span {
        self.span
    }
}

#[cfg(test)]
#[path = "span_test.rs"]
mod tests;
