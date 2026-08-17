//! Semantic diagnostic taxonomy.
//!
//! Semantic passes share one diagnostic shape so the driver can collect,
//! downgrade, render, and report errors without losing the phase that produced
//! them. The taxonomy is deliberately broad: name binding, HIR lowering,
//! typechecking, ownership analysis, control-flow checks, and lints all report
//! through [`SemanticError`].
//!
//! WARNING POLICY
//! ==============
//! Warnings are represented as [`SemanticErrorKind::Warning`] so they can keep
//! the same source span and named diagnostic facts as hard diagnostics. They are
//! not fatal by default: [`SemanticError::is_error`] is the boundary used by
//! `SemanticResult::success` to distinguish warnings from compilation blockers.
//!
//! COMPATIBILITY
//! =============
//! Some front-end check modes intentionally downgrade unresolved-name failures
//! for permissive exploratory flows. [`SemanticErrorKind::is_permissive_check_downgrade`]
//! names that policy in one place instead of scattering ad hoc kind checks
//! through command surfaces.

use crate::diagnostics::DiagnosticArg;
use crate::lexer::Span;

/// One semantic diagnostic with source position and optional corrective help.
///
/// The type is used for both hard errors and warnings; call [`Self::is_error`]
/// when a caller needs the fatal/non-fatal distinction.
#[derive(Debug, Clone)]
pub struct SemanticError {
    pub kind: SemanticErrorKind,
    pub args: Vec<DiagnosticArg>,
    pub span: Span,
}

impl SemanticError {
    /// Build a structured semantic diagnostic whose prose should come from the
    /// diagnostics catalog.
    #[must_use]
    pub fn structured(kind: SemanticErrorKind, span: Span) -> Self {
        Self { kind, args: Vec::new(), span }
    }

    /// Attach a named template argument.
    #[must_use]
    pub fn with_arg(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.args.push(DiagnosticArg::new(name, value));
        self
    }

    /// Return whether this diagnostic should block successful compilation.
    #[must_use]
    pub fn is_error(&self) -> bool {
        !matches!(self.kind, SemanticErrorKind::Warning(_))
    }
}

/// Semantic diagnostic categories used by passes and command policy — owned
/// by `radix-diagnostics` so `Diagnostic` can carry the typed kind without a
/// crate cycle. Re-exported here for the historical
/// `radix_module::semantic::SemanticErrorKind` / `radix_module::semantic::WarningKind` path.
pub use radix_diagnostics::{SemanticErrorKind, WarningKind};

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.kind)
    }
}

impl std::error::Error for SemanticError {}

#[cfg(test)]
#[path = "error_test.rs"]
mod tests;
