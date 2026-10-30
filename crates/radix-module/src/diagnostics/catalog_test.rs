//! Tests for diagnostic catalog code stability and helper functions.
//! Proves: codes are unique per lex/parse/semantic phase; representative
//! codes remain stable; argument helpers preserve quoting.

use crate::diagnostics::is_known_diagnostic_code;
use crate::lexer::{HOMOGLYPH_ISSUE, LexErrorKind};
use crate::parser::ParseErrorKind;
use crate::semantic::{SemanticErrorKind, WarningKind};
use radix_diagnostics::{
    DiagnosticArg, DiagnosticSpec, locale_suggestion_spec, locale_unknown_spec, serialized_by_backend_spec,
};
use radix_semantic::catalog::{arg_value, quote_arg};
use radix_semantic::catalog_semantic::{semantic_help, semantic_spec};
use radix_semantic::catalog_syntax::{lex_message, lex_spec, parse_message, parse_spec};
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
    assert_eq!(parse_spec(ParseErrorKind::FunctionNameAbsent).code, "PARSE011");
    assert_eq!(semantic_spec(SemanticErrorKind::TypeMismatch).code, "SEM010");
    assert_eq!(
        semantic_spec(SemanticErrorKind::Warning(WarningKind::UnusedImport)).code,
        "WARN002"
    );
    assert_eq!(locale_suggestion_spec().code, "LOCALE002");
}

#[test]
fn legacy_message_arg_is_not_a_catalog_rendering_fallback() {
    let args = vec![DiagnosticArg::new("legacy_message", "do not render me")];

    assert_eq!(parse_message(ParseErrorKind::Expected, &args), None);
}

#[test]
fn lexer_homoglyph_catalog_rows_preserve_canonical_suggestions() {
    let circled = vec![
        DiagnosticArg::new("issue", HOMOGLYPH_ISSUE),
        DiagnosticArg::new("char", "∘"),
        DiagnosticArg::new("suggestion", "⊙"),
    ];
    assert_eq!(lex_spec(LexErrorKind::CharacterUnexpected).code, "LEX004");
    assert_eq!(
        lex_message(LexErrorKind::CharacterUnexpected, &circled),
        Some("unexpected character: '∘'; did you mean '⊙'?".to_owned())
    );

    let norm = vec![
        DiagnosticArg::new("issue", HOMOGLYPH_ISSUE),
        DiagnosticArg::new("char", "‖"),
        DiagnosticArg::new("suggestion", ".rms_norm(...)"),
    ];
    assert_eq!(
        lex_message(LexErrorKind::CharacterUnexpected, &norm),
        Some("unexpected character: '‖'; did you mean '.rms_norm(...)'?".to_owned())
    );
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
        (LexErrorKind::NumberInvalid, "LEX002"),
        (LexErrorKind::EscapeInvalid, "LEX003"),
        (LexErrorKind::CharacterUnexpected, "LEX004"),
        (LexErrorKind::CommentSyntaxInvalid, "LEX006"),
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
        (ParseErrorKind::DeclarationInvalid, "PARSE010"),
        (ParseErrorKind::FunctionNameAbsent, "PARSE011"),
        (ParseErrorKind::FunctionBodyAbsent, "PARSE012"),
        (ParseErrorKind::ClassNameAbsent, "PARSE013"),
        (ParseErrorKind::ClassBodyAbsent, "PARSE014"),
        (ParseErrorKind::ParameterInvalid, "PARSE015"),
        (ParseErrorKind::TypeParameterInvalid, "PARSE016"),
        (ParseErrorKind::ModifierInvalid, "PARSE017"),
        (ParseErrorKind::AnnotationInvalid, "PARSE018"),
        (ParseErrorKind::ModifierDuplicate, "PARSE019"),
        (ParseErrorKind::StatementInvalid, "PARSE020"),
        (ParseErrorKind::ConditionAbsent, "PARSE021"),
        (ParseErrorKind::BlockAbsent, "PARSE022"),
        (ParseErrorKind::ThereforeWrongContext, "PARSE026"),
        (ParseErrorKind::ErgoWrongContext, "PARSE027"),
        (ParseErrorKind::PatternInvalid, "PARSE023"),
        (ParseErrorKind::CasuArmInvalid, "PARSE024"),
        (ParseErrorKind::CasuValueInvalid, "PARSE025"),
        (ParseErrorKind::ExpressionInvalid, "PARSE030"),
        (ParseErrorKind::LiteralInvalid, "PARSE031"),
        (ParseErrorKind::OperatorInvalid, "PARSE032"),
        (ParseErrorKind::UnterminatedGroup, "PARSE033"),
        (ParseErrorKind::CallArgumentInvalid, "PARSE034"),
        (ParseErrorKind::MemberAccessInvalid, "PARSE035"),
        (ParseErrorKind::AssignmentTargetInvalid, "PARSE036"),
        (ParseErrorKind::TypeInvalid, "PARSE040"),
        (ParseErrorKind::TypeAnnotationInvalid, "PARSE041"),
        (ParseErrorKind::UnterminatedTypeParams, "PARSE042"),
        (ParseErrorKind::ImportInvalid, "PARSE050"),
        (ParseErrorKind::CommentPlacementInvalid, "PARSE060"),
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

/// The kind-to-code stability table for semantic errors, shared by the
/// stability test and the catalog-coverage test so a new catalog row cannot
/// be added without an entry here (and vice versa).
fn semantic_code_table() -> Vec<(SemanticErrorKind, &'static str)> {
    vec![
        (SemanticErrorKind::VariableUndefined, "SEM001"),
        (SemanticErrorKind::TypeUndefined, "SEM002"),
        (SemanticErrorKind::FunctionUndefined, "SEM003"),
        (SemanticErrorKind::MemberUndefined, "SEM004"),
        (SemanticErrorKind::DefinitionDuplicate, "SEM005"),
        (SemanticErrorKind::ImportAbsent, "SEM006"),
        (SemanticErrorKind::CircularDependency, "SEM007"),
        (SemanticErrorKind::LoweringError, "SEM008"),
        (SemanticErrorKind::CliValidation, "SEM009"),
        (SemanticErrorKind::TypeMismatch, "SEM010"),
        (SemanticErrorKind::OperandTypesInvalid, "SEM011"),
        (SemanticErrorKind::NonCallable, "SEM012"),
        (SemanticErrorKind::ArityMismatch, "SEM013"),
        (SemanticErrorKind::TypeAnnotationAbsent, "SEM014"),
        (SemanticErrorKind::CastInvalid, "SEM015"),
        (SemanticErrorKind::ConversionInvalid, "SEM016"),
        (SemanticErrorKind::UnavailableDeclaration, "SEM017"),
        (SemanticErrorKind::AnnotationInvalid, "SEM018"),
        (SemanticErrorKind::ConflictingVisibility, "SEM019"),
        (SemanticErrorKind::ImmutableAssignment, "SEM020"),
        (SemanticErrorKind::AssignmentTargetInvalid, "SEM021"),
        (SemanticErrorKind::BreakOutsideBreakable, "SEM030"),
        (SemanticErrorKind::ContinueOutsideLoop, "SEM031"),
        (SemanticErrorKind::ReturnOutsideFunction, "SEM032"),
        (SemanticErrorKind::ReturnAbsent, "SEM033"),
        (SemanticErrorKind::NonExhaustiveMatch, "SEM040"),
        (SemanticErrorKind::UnreachablePattern, "SEM041"),
        (SemanticErrorKind::PatternDuplicate, "SEM042"),
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
        (SemanticErrorKind::StaticInitializerInvalid, "SEM060"),
        (SemanticErrorKind::InsereInvalid, "SEM061"),
        (SemanticErrorKind::TopLevelBindingForbidden, "SEM062"),
        (SemanticErrorKind::MemberInaccessible, "SEM063"),
        (SemanticErrorKind::PraefixumInvalid, "SEM064"),
        (SemanticErrorKind::ConstSharedIntoVar, "SEM065"),
        (SemanticErrorKind::CopyNonduplicable, "SEM066"),
    ]
}

#[test]
fn semantic_error_codes_are_stable() {
    for (kind, expected_code) in semantic_code_table() {
        let spec = semantic_spec(kind);
        assert_eq!(
            spec.code, expected_code,
            "semantic_spec({kind:?}) code mismatch: expected {expected_code}, got {}",
            spec.code
        );
    }
}

#[test]
fn semantic_stability_table_covers_every_catalog_code() {
    let table: HashSet<&str> = semantic_code_table()
        .iter()
        .map(|(_, code)| *code)
        .collect();
    let catalog: HashSet<&str> = all_specs()
        .iter()
        .map(|spec| spec.code)
        .filter(|code| code.starts_with("SEM"))
        .collect();
    let missing_from_table: Vec<&&str> = catalog.difference(&table).collect();
    let absent_from_catalog: Vec<&&str> = table.difference(&catalog).collect();
    assert!(
        missing_from_table.is_empty() && absent_from_catalog.is_empty(),
        "semantic stability table and catalog SEM codes diverge: \
         in catalog, missing from table {missing_from_table:?}; \
         in table, absent from catalog {absent_from_catalog:?}"
    );
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
        (WarningKind::MonomorphicHoleSolvedToUnion, "WARN016"),
        (WarningKind::UnionHoleSolvedMonomorphically, "WARN017"),
        (WarningKind::PreferStringTemplate, "WARN018"),
        (WarningKind::DiscouragedMutLocal, "WARN019"),
        (WarningKind::PreferGuillemetQuote, "WARN034"),
        (WarningKind::MixedTemplateHoles, "WARN035"),
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
    assert_eq!(locale_suggestion_spec().code, "LOCALE002");
    assert_eq!(locale_unknown_spec().code, "LOCALE003");
}

#[test]
fn reader_codes_are_distinct() {
    // Each reader locale spec must have a unique code.
    let codes = [locale_suggestion_spec().code, locale_unknown_spec().code];
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

fn all_specs() -> Vec<DiagnosticSpec> {
    crate::diagnostics::all_diagnostic_specs()
}

#[test]
fn is_known_diagnostic_code_accepts_catalog_codes() {
    assert!(is_known_diagnostic_code("WARN016"));
    assert!(is_known_diagnostic_code("WARN017"));
    assert!(is_known_diagnostic_code("SEM004"));
    assert!(is_known_diagnostic_code("LEX001"));
    assert!(is_known_diagnostic_code("PARSE011"));
    assert!(!is_known_diagnostic_code("UNRES001"));
    assert!(!is_known_diagnostic_code("UNRES002"));
    assert!(!is_known_diagnostic_code("UNRES003"));
}

/// The advice code (`Severity::Info`) is a catalog code, so `--deny INFO001`
/// is accepted; info is never promoted (`driver::apply_warn_policy`).
#[test]
fn serialized_by_backend_advice_code_is_known_and_stable() {
    assert_eq!(serialized_by_backend_spec().code, "INFO001");
    assert!(is_known_diagnostic_code("INFO001"));
}

#[test]
fn is_known_diagnostic_code_rejects_unknown_codes() {
    assert!(!is_known_diagnostic_code("WARN066"));
    assert!(!is_known_diagnostic_code("FOO"));
    assert!(!is_known_diagnostic_code(""));
    assert!(!is_known_diagnostic_code("CODEGEN001"));
}

#[test]
fn via_bits_retirement_help_names_the_bn_replacement() {
    let args = vec![DiagnosticArg::new("issue", "conversio_via_bits_retired")];
    let help = semantic_help(SemanticErrorKind::ConversionInvalid, &args)
        .unwrap_or_else(|| panic!("catalog help missing for conversio_via_bits_retired"));
    assert!(help.contains("b32"), "retirement help should name bN: {help}");
}
