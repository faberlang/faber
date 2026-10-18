//! `@ radix contract "<role>"`: norma contracts the compiler recognizes.
//!
//! Norma declares its contracts in ordinary Faber (`implendum Orderable<T>`).
//! A contract the compiler must understand — because built-in types conform to
//! it and operators derive from it (DECISIONS D1.4) — carries one statement
//! annotation naming its role:
//!
//! ```text
//! @ radix contract "ordering"
//! implendum Orderable<T> { functio compare(T other) → numerus }
//! ```
//!
//! The mark reuses the `@ radix <directive> STRING` shape of `lane` and
//! `backward`. It is valid only on an `implendum`, and the compiler finds the
//! contract by the mark, never by its name. `@ radix` stays reserved for
//! compiler-owned metadata: applications must not write it.

use crate::{SemanticError, SemanticErrorKind};
use radix_hir::HirAnnotation;
use radix_lexer::{Interner, Span, TokenKind};
use radix_syntax::{Annotation, AnnotationKind, AnnotationStmt};

/// The directive word after `@ radix`.
pub const CONTRACT_DIRECTIVE: &str = "contract";

pub use radix_types::ordering::ORDERING_METHOD;

/// The closing contract's one method (D8.3).
pub const CLOSING_METHOD: &str = "close";

/// The sinking contract's item method and flush.
pub const SINKING_METHOD: &str = "sink";
pub const SINKING_FLUSH_METHOD: &str = "flush";

/// Compiler roles a norma contract can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadixContractRole {
    /// Three-way `compare(T other) → int`; drives `≺ ≻ ≤ ≥` (D1.4).
    Ordering,
    /// `close() → bool ⇥ string`; `sermo` conforms, and a closable type
    /// that can sit in a reference cycle is warned about (D8.3).
    Closing,
    /// `sink(T item) → void ⇥ E` plus `flush() → void ⇥ E`;
    /// `s ⇜ x` is the operator form of `sink`.
    Sinking,
}

impl RadixContractRole {
    #[must_use]
    pub fn parse(role: &str) -> Option<Self> {
        match role {
            "ordering" => Some(Self::Ordering),
            "closing" => Some(Self::Closing),
            "sinking" => Some(Self::Sinking),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ordering => "ordering",
            Self::Closing => "closing",
            Self::Sinking => "sinking",
        }
    }
}

/// Parse one AST annotation as `@ radix contract "<role>"`.
///
/// Returns `None` when the annotation is not a contract directive, and an
/// error when it is one with a missing or unknown role.
#[must_use]
pub fn role_from_ast(
    annotation: &Annotation,
    interner: &Interner,
) -> Option<Result<RadixContractRole, SemanticError>> {
    let AnnotationKind::Statement(stmt) = &annotation.kind else {
        return None;
    };
    role_from_stmt(stmt, annotation.span, interner)
}

fn role_from_stmt(
    stmt: &AnnotationStmt,
    span: Span,
    interner: &Interner,
) -> Option<Result<RadixContractRole, SemanticError>> {
    if interner.resolve(stmt.name.name) != "radix" {
        return None;
    }
    let directive = stmt.args.first()?;
    let TokenKind::Ident(word) = directive.kind else {
        return None;
    };
    if interner.resolve(word) != CONTRACT_DIRECTIVE {
        return None;
    }
    let (Some(value), 2) = (stmt.args.get(1), stmt.args.len()) else {
        return Some(Err(contract_error(span, "radix_contract_role_missing")));
    };
    let TokenKind::String(role) = value.kind else {
        return Some(Err(contract_error(
            value.span,
            "radix_contract_role_missing",
        )));
    };
    let role_text = interner.resolve(role);
    Some(RadixContractRole::parse(role_text).ok_or_else(|| {
        contract_error(value.span, "radix_contract_unknown_role").with_arg("role", role_text)
    }))
}

/// The role on a lowered item's annotations, if it carries a valid one.
#[must_use]
pub fn role_from_hir(
    annotations: &[HirAnnotation],
    interner: &Interner,
) -> Option<RadixContractRole> {
    annotations.iter().find_map(|annotation| {
        if interner.resolve(annotation.family) != "radix" {
            return None;
        }
        let [directive, value] = annotation.raw_args.as_slice() else {
            return None;
        };
        let (TokenKind::Ident(word), TokenKind::String(role)) = (&directive.kind, &value.kind)
        else {
            return None;
        };
        (interner.resolve(*word) == CONTRACT_DIRECTIVE)
            .then(|| RadixContractRole::parse(interner.resolve(*role)))
            .flatten()
    })
}

/// Validate every `@ radix contract` on one declaration statement.
///
/// `on_implendum` says whether the declaration can carry a role at all.
pub(crate) fn validate_declaration(
    annotations: &[Annotation],
    on_implendum: bool,
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) {
    for annotation in annotations {
        match role_from_ast(annotation, interner) {
            None => {}
            Some(Err(error)) => errors.push(error),
            Some(Ok(_)) if on_implendum => {}
            Some(Ok(_)) => errors.push(contract_error(annotation.span, "radix_contract_placement")),
        }
    }
}

pub(crate) fn contract_error(span: Span, issue: &'static str) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::AnnotationInvalid, span)
        .with_arg("issue", issue)
        .with_arg("annotation", "@ radix contract")
}
