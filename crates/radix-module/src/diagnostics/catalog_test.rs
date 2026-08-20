//! Tests for diagnostic catalog code stability and helper functions.
//! Proves: codes are unique per lex/parse/semantic phase; representative
//! codes remain stable; argument helpers preserve quoting.

use super::super::catalog_semantic::{semantic_message, semantic_spec};
use super::super::catalog_syntax::{lex_spec, parse_message, parse_spec};
use super::super::locale_unknown_spec;
use super::super::{unresolved_intrinsic_spec, unresolved_keyword_spec, unresolved_library_member_spec};
use super::{arg_value, locale_suggestion_spec, quote_arg};
use crate::diagnostics::DiagnosticArg;
use crate::lexer::LexErrorKind;
use crate::parser::ParseErrorKind;
use crate::semantic::{SemanticErrorKind, WarningKind};
use std::collections::HashSet;

#[test]
fn catalog_codes_are_unique() {
    let mut codes = HashSet::new();

    for spec in all_specs() {
        assert!(codes.insert(spec.code), "duplicate diagnostic code {}", spec.code);
    }
}

#[test]
fn representative_codes_remain_stable() {
    assert_eq!(lex_spec(LexErrorKind::UnterminatedString).code, "LEX001");
    assert_eq!(parse_spec(ParseErrorKind::MissingFunctionName).code, "PARSE011");
    assert_eq!(semantic_spec(SemanticErrorKind::TypeMismatch).code, "SEM010");
    assert_eq!(
        semantic_spec(SemanticErrorKind::Warning(WarningKind::UnusedImport)).code,
        "WARN002"
    );
    assert_eq!(unresolved_keyword_spec().code, "UNRES001");
    assert_eq!(unresolved_intrinsic_spec().code, "UNRES002");
    assert_eq!(unresolved_library_member_spec().code, "UNRES003");
    assert_eq!(locale_suggestion_spec().code, "LOCALE002");
}

#[test]
fn legacy_message_arg_is_not_a_catalog_rendering_fallback() {
    let args = vec![DiagnosticArg::new("legacy_message", "do not render me")];

    assert_eq!(parse_message(ParseErrorKind::Expected, &args), None);
    assert_eq!(semantic_message(SemanticErrorKind::TypeMismatch, &args), None);
}

#[test]
fn argument_helpers_share_lookup_and_preserve_quoting() {
    let args = vec![DiagnosticArg::new("name", "salve")];

    assert_eq!(arg_value(&args, "name"), Some("salve"));
    assert_eq!(quote_arg(&args, "name"), Some("'salve'".to_owned()));
    assert_eq!(quote_arg(&args, "missing"), None);
}

// ── Table-driven code stability tests ────────────────────────────────────────

#[test]
fn lex_error_codes_are_stable() {
    let cases = [
        (LexErrorKind::UnterminatedString, "LEX001"),
        (LexErrorKind::InvalidNumber, "LEX002"),
        (LexErrorKind::InvalidEscape, "LEX003"),
        (LexErrorKind::UnexpectedCharacter, "LEX004"),
        (LexErrorKind::RemovedSectionDirective, "LEX005"),
        (LexErrorKind::InvalidCommentSyntax, "LEX006"),
        (LexErrorKind::InlineComment, "LEX007"),
    ];
    for (kind, expected_code) in &cases {
        let spec = lex_spec(*kind);
        assert_eq!(
            spec.code, *expected_code,
            "lex_spec({:?}) code mismatch: expected {}, got {}",
            kind, expected_code, spec.code
        );
    }
}

#[test]
fn parse_error_codes_are_stable() {
    let cases = [
        (ParseErrorKind::LexError, "PARSE000"),
        (ParseErrorKind::Expected, "PARSE001"),
        (ParseErrorKind::Unexpected, "PARSE002"),
        (ParseErrorKind::InvalidDeclaration, "PARSE010"),
        (ParseErrorKind::MissingFunctionName, "PARSE011"),
        (ParseErrorKind::MissingFunctionBody, "PARSE012"),
        (ParseErrorKind::MissingClassName, "PARSE013"),
        (ParseErrorKind::MissingClassBody, "PARSE014"),
        (ParseErrorKind::InvalidParameter, "PARSE015"),
        (ParseErrorKind::InvalidTypeParameter, "PARSE016"),
        (ParseErrorKind::InvalidModifier, "PARSE017"),
        (ParseErrorKind::InvalidAnnotation, "PARSE018"),
        (ParseErrorKind::DuplicateModifier, "PARSE019"),
        (ParseErrorKind::InvalidStatement, "PARSE020"),
        (ParseErrorKind::MissingCondition, "PARSE021"),
        (ParseErrorKind::MissingBlock, "PARSE022"),
        (ParseErrorKind::ThereforeWrongContext, "PARSE026"),
        (ParseErrorKind::ErgoWrongContext, "PARSE027"),
        (ParseErrorKind::InvalidPattern, "PARSE023"),
        (ParseErrorKind::InvalidCasuArm, "PARSE024"),
        (ParseErrorKind::InvalidCasuValue, "PARSE025"),
        (ParseErrorKind::InvalidExpression, "PARSE030"),
        (ParseErrorKind::InvalidLiteral, "PARSE031"),
        (ParseErrorKind::InvalidOperator, "PARSE032"),
        (ParseErrorKind::UnterminatedGroup, "PARSE033"),
        (ParseErrorKind::InvalidCallArgument, "PARSE034"),
        (ParseErrorKind::InvalidMemberAccess, "PARSE035"),
        (ParseErrorKind::InvalidAssignmentTarget, "PARSE036"),
        (ParseErrorKind::InvalidType, "PARSE040"),
        (ParseErrorKind::InvalidTypeAnnotation, "PARSE041"),
        (ParseErrorKind::UnterminatedTypeParams, "PARSE042"),
        (ParseErrorKind::InvalidImport, "PARSE050"),
        (ParseErrorKind::InvalidCommentPlacement, "PARSE060"),
    ];
    for (kind, expected_code) in &cases {
        let spec = parse_spec(*kind);
        assert_eq!(
            spec.code, *expected_code,
            "parse_spec({:?}) code mismatch: expected {}, got {}",
            kind, expected_code, spec.code
        );
    }
}

#[test]
fn semantic_error_codes_are_stable() {
    let cases = [
        (SemanticErrorKind::UndefinedVariable, "SEM001"),
        (SemanticErrorKind::UndefinedType, "SEM002"),
        (SemanticErrorKind::UndefinedFunction, "SEM003"),
        (SemanticErrorKind::UndefinedMember, "SEM004"),
        (SemanticErrorKind::DuplicateDefinition, "SEM005"),
        (SemanticErrorKind::ImportNotFound, "SEM006"),
        (SemanticErrorKind::CircularDependency, "SEM007"),
        (SemanticErrorKind::LoweringError, "SEM008"),
        (SemanticErrorKind::CliValidation, "SEM009"),
        (SemanticErrorKind::TypeMismatch, "SEM010"),
        (SemanticErrorKind::InvalidOperandTypes, "SEM011"),
        (SemanticErrorKind::NotCallable, "SEM012"),
        (SemanticErrorKind::WrongArity, "SEM013"),
        (SemanticErrorKind::MissingTypeAnnotation, "SEM014"),
        (SemanticErrorKind::InvalidCast, "SEM015"),
        (SemanticErrorKind::InvalidConversion, "SEM016"),
        (SemanticErrorKind::UnavailableDeclaration, "SEM017"),
        (SemanticErrorKind::InvalidAnnotation, "SEM018"),
        (SemanticErrorKind::ConflictingVisibility, "SEM019"),
        (SemanticErrorKind::ImmutableAssignment, "SEM020"),
        (SemanticErrorKind::InvalidAssignmentTarget, "SEM021"),
        (SemanticErrorKind::BreakOutsideBreakable, "SEM030"),
        (SemanticErrorKind::ContinueOutsideLoop, "SEM031"),
        (SemanticErrorKind::ReturnOutsideFunction, "SEM032"),
        (SemanticErrorKind::MissingReturn, "SEM033"),
        (SemanticErrorKind::NonExhaustiveMatch, "SEM040"),
        (SemanticErrorKind::UnreachablePattern, "SEM041"),
        (SemanticErrorKind::DuplicatePattern, "SEM042"),
        (SemanticErrorKind::OmniaRequiresEnum, "SEM043"),
        (SemanticErrorKind::OmniaForbidsCatchall, "SEM044"),
        (SemanticErrorKind::UseAfterMove, "SEM050"),
        (SemanticErrorKind::BorrowOfMoved, "SEM051"),
        (SemanticErrorKind::MutableBorrowConflict, "SEM052"),
        (SemanticErrorKind::CannotMoveOut, "SEM053"),
        (SemanticErrorKind::LifetimeMismatch, "SEM054"),
        (SemanticErrorKind::ShadowedVariable, "SEM055"),
        (SemanticErrorKind::AssignToImmutableBorrow, "SEM056"),
        (SemanticErrorKind::ModeMismatch, "SEM057"),
        (SemanticErrorKind::UseBeforeInit, "SEM058"),
        (SemanticErrorKind::AirPurityViolation, "SEM059"),
    ];
    for (kind, expected_code) in &cases {
        let spec = semantic_spec(*kind);
        assert_eq!(
            spec.code, *expected_code,
            "semantic_spec({:?}) code mismatch: expected {}, got {}",
            kind, expected_code, spec.code
        );
    }
}

#[test]
fn warning_codes_are_stable() {
    let cases = [
        (WarningKind::UnusedVariable, "WARN001"),
        (WarningKind::UnusedImport, "WARN002"),
        (WarningKind::UnusedFunction, "WARN003"),
        (WarningKind::UnreachableCode, "WARN004"),
        (WarningKind::UnnecessaryCast, "WARN005"),
        (WarningKind::TargetNoop, "WARN007"),
        (WarningKind::UnusedMutRefParam, "WARN008"),
        (WarningKind::UnusedMoveParam, "WARN009"),
        (WarningKind::ExplicitIgnotumAnnotation, "WARN010"),
        (WarningKind::WildcardAbsorbsNull, "WARN011"),
        (WarningKind::UnnecessaryVaria, "WARN013"),
        (WarningKind::FileInterfaceExportSkipped, "WARN014"),
        (WarningKind::UnrecognizedCurataAllocator, "WARN015"),
        (WarningKind::MonomorphicHoleSolvedToUnion, "WARN016"),
        (WarningKind::UnionHoleSolvedMonomorphically, "WARN017"),
        (WarningKind::PreferStringTemplate, "WARN018"),
        (WarningKind::DiscouragedMutLocal, "WARN019"),
    ];
    for (kind, expected_code) in &cases {
        let spec = semantic_spec(SemanticErrorKind::Warning(*kind));
        assert_eq!(
            spec.code, *expected_code,
            "warning_spec({:?}) code mismatch: expected {}, got {}",
            kind, expected_code, spec.code
        );
    }
}

#[test]
fn reader_codes_are_stable() {
    assert_eq!(unresolved_keyword_spec().code, "UNRES001");
    assert_eq!(unresolved_intrinsic_spec().code, "UNRES002");
    assert_eq!(unresolved_library_member_spec().code, "UNRES003");
    assert_eq!(locale_suggestion_spec().code, "LOCALE002");
    assert_eq!(locale_unknown_spec().code, "LOCALE003");
}

#[test]
fn reader_codes_are_distinct() {
    // Each reader locale spec must have a unique code.
    let codes = [
        unresolved_keyword_spec().code,
        unresolved_intrinsic_spec().code,
        unresolved_library_member_spec().code,
        locale_suggestion_spec().code,
        locale_unknown_spec().code,
    ];
    let mut seen = std::collections::HashSet::new();
    for code in codes {
        assert!(seen.insert(code), "duplicate reader locale code {code}");
    }
}

#[test]
fn arg_value_returns_none_for_missing_key() {
    let args = vec![DiagnosticArg::new("name", "salve")];
    assert_eq!(arg_value(&args, "nonexistent"), None);
}

#[test]
fn arg_value_returns_none_for_empty_args() {
    let args: Vec<DiagnosticArg> = Vec::new();
    assert_eq!(arg_value(&args, "any"), None);
    assert_eq!(quote_arg(&args, "any"), None);
}

fn all_specs() -> Vec<super::super::DiagnosticSpec> {
    super::super::all_diagnostic_specs()
}

#[test]
fn is_known_diagnostic_code_accepts_catalog_codes() {
    assert!(super::super::is_known_diagnostic_code("WARN016"));
    assert!(super::super::is_known_diagnostic_code("WARN017"));
    assert!(super::super::is_known_diagnostic_code("SEM004"));
    assert!(super::super::is_known_diagnostic_code("LEX001"));
    assert!(super::super::is_known_diagnostic_code("PARSE011"));
    assert!(super::super::is_known_diagnostic_code("UNRES001"));
    assert!(super::super::is_known_diagnostic_code("UNRES002"));
    assert!(super::super::is_known_diagnostic_code("UNRES003"));
}

#[test]
fn is_known_diagnostic_code_rejects_unknown_codes() {
    assert!(!super::super::is_known_diagnostic_code("WARN066"));
    assert!(!super::super::is_known_diagnostic_code("FOO"));
    assert!(!super::super::is_known_diagnostic_code(""));
    assert!(!super::super::is_known_diagnostic_code("CODEGEN001"));
}
