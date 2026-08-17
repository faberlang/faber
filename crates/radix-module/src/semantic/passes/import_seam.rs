//! Import-validator seams: declaration tier × package boundary.
//!
//! Same package → `@ publica` and `@ interna` names are importable. Different
//! package → `@ publica` only. Module-private names (unmarked or `@ privata`)
//! are never importable. Analysis applies caller-supplied `namespace_seams` only;
//! this module does not probe package roots. Use-site diagnostics consult
//! [`violation_issue`]; the resolver fail-closed Interna gate from VM-U2 is
//! replaced by this policy.

use crate::lexer::Span;
use crate::semantic::passes::visibility_annotation::VisibilityTier;
use crate::semantic::{SemanticError, SemanticErrorKind};

/// Cross-package `@ interna` reference.
pub const ISSUE_IMPORT_INTERNA_CROSS_PACKAGE: &str = "import_interna_cross_package";
/// Import of an unmarked or `@ privata` name from another file.
pub const ISSUE_IMPORT_MODULE_PRIVATE: &str = "import_module_private";

/// Package relationship between the importing file and the imported target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSeam {
    /// Both files share a `faber.toml` package root.
    SamePackage,
    /// Different package roots, or either file has no package root.
    CrossPackage,
}

/// Whether a named declaration may be referenced across `seam`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Importability {
    /// The name may be referenced at this seam.
    Allowed,
    /// The name is not a known declaration of the target.
    Missing,
    /// The name exists and is module-private.
    ModulePrivate,
    /// The name is `@ interna` and the consumer is a different package.
    InternaCrossPackage,
}

/// Classify a caller-supplied same-package flag as a seam.
#[must_use]
pub fn seam_from_flag(same_package: bool) -> ImportSeam {
    if same_package {
        ImportSeam::SamePackage
    } else {
        ImportSeam::CrossPackage
    }
}

/// Classify a target declaration (if known) against the consumer's package seam.
#[must_use]
pub fn importability(tier: Option<VisibilityTier>, seam: ImportSeam) -> Importability {
    match tier {
        Some(VisibilityTier::Publica) => Importability::Allowed,
        Some(VisibilityTier::Interna) => match seam {
            ImportSeam::SamePackage => Importability::Allowed,
            ImportSeam::CrossPackage => Importability::InternaCrossPackage,
        },
        Some(VisibilityTier::Privata) => Importability::ModulePrivate,
        None => Importability::Missing,
    }
}

/// Structured issue slug when `tier` is not importable at `seam`.
#[must_use]
pub fn violation_issue(tier: Option<VisibilityTier>, seam: ImportSeam) -> Option<&'static str> {
    match importability(tier, seam) {
        Importability::Allowed | Importability::Missing => None,
        Importability::ModulePrivate => Some(ISSUE_IMPORT_MODULE_PRIVATE),
        Importability::InternaCrossPackage => Some(ISSUE_IMPORT_INTERNA_CROSS_PACKAGE),
    }
}

/// Structured import-seam diagnostic (`SEM006` / [`SemanticErrorKind::ImportNotFound`]).
#[must_use]
pub fn diagnostic(span: Span, issue: &'static str, name: &str) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::ImportNotFound, span)
        .with_arg("issue", issue)
        .with_arg("name", name)
}

#[cfg(test)]
#[path = "import_seam_test.rs"]
mod tests;
