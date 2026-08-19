//! Tests for semantic error and warning diagnostics.
//!
//! These tests prove that SemanticError and SemanticErrorKind produce
//! correctly structured diagnostics, that warnings are non-fatal, and
//! that diagnostic arguments serialize correctly.

use super::{SemanticError, SemanticErrorKind, WarningKind};
use radix_diagnostics::DiagnosticArg;
use radix_lexer::Span;

#[test]
fn warnings_are_non_fatal_semantic_diagnostics() {
    let warning = SemanticError::structured(
        SemanticErrorKind::Warning(WarningKind::UnusedVariable),
        Span::default(),
    );

    assert!(!warning.is_error());
    assert!(warning.args.is_empty());
}

#[test]
fn hard_semantic_diagnostics_are_fatal() {
    let error = SemanticError::structured(SemanticErrorKind::TypeMismatch, Span::default());

    assert!(error.is_error());
    assert!(error.args.is_empty());
}

#[test]
fn structured_semantic_diagnostics_do_not_choose_prose_at_emit_site() {
    let error = SemanticError::structured(SemanticErrorKind::UndefinedVariable, Span::default())
        .with_arg("name", "x");

    assert_eq!(error.args, vec![DiagnosticArg::new("name", "x")]);
}

#[test]
fn permissive_check_downgrades_only_missing_symbol_errors() {
    assert!(SemanticErrorKind::UndefinedVariable.is_permissive_check_downgrade());
    assert!(SemanticErrorKind::UndefinedType.is_permissive_check_downgrade());
    assert!(SemanticErrorKind::UndefinedFunction.is_permissive_check_downgrade());
    assert!(SemanticErrorKind::UndefinedMember.is_permissive_check_downgrade());
    assert!(SemanticErrorKind::ImportNotFound.is_permissive_check_downgrade());

    assert!(!SemanticErrorKind::TypeMismatch.is_permissive_check_downgrade());
    assert!(!SemanticErrorKind::LoweringError.is_permissive_check_downgrade());
    assert!(
        !SemanticErrorKind::Warning(WarningKind::UnusedVariable).is_permissive_check_downgrade()
    );
}
