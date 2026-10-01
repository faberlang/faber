//! Integration tests for the semantic analysis pipeline.
//!
//! These tests prove that the semantic orchestrator (collection, resolution,
//! lowering, typecheck) and pass configuration produce expected diagnostics
//! and structured issue arguments across error, warning, and success paths.

use super::{
    PassConfig, SemanticError, SemanticErrorKind, WarningKind, analyze, is_use_site_only_error,
};
use radix_target::Target;

use crate::Primitive;
use crate::file_interface::{FileExport, FileExportKind, FileInterface, InterfaceTypeSnapshot};
use crate::test_support::args_have_no_legacy_message;

#[test]
fn hir_lowering_bridge_uses_structured_issue_arg() {
    let lex = radix_lexer::lex("fixum numerus x\n");
    let mut parsed = radix_parser::parse(lex);
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("program");
    let config = PassConfig::for_target(Target::HirRust);

    let result = analyze(&program, &config, &mut parsed.interner);
    let err = result
        .diagnostics
        .iter()
        .find(|err| err.kind == SemanticErrorKind::LoweringError)
        .expect("HIR lowering diagnostic");

    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "top_level_initializer")
    );
    assert!(args_have_no_legacy_message(&err.args));
}

#[test]
fn pass_config_file_interfaces_hook_defaults_empty_and_accepts_contracts() {
    let mut config = PassConfig::for_target(Target::HirRust);
    assert!(config.file_interfaces.is_empty());

    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "nomen".to_owned(),
        kind: FileExportKind::Const(InterfaceTypeSnapshot::Primitive(Primitive::Textus), None),
    });
    config.file_interfaces.insert("path".to_owned(), interface);

    assert_eq!(config.file_interfaces["path"].exports.len(), 1);
}

// ---------------------------------------------------------------------------
// em-u2 — pre-lowering gate classification (use-site vs coherence-class).
// ---------------------------------------------------------------------------

#[test]
fn gate_classification_is_conservative() {
    // Direct classification regression for the `:389` gate predicate. The
    // missing-symbol-on-use family (`UndefinedVariable` /
    // `UndefinedFunction` / `UndefinedMember`) is the ONLY set allowed past
    // the pre-lowering gate; every coherence-class / definition-identity /
    // shape / context kind keeps the gate firing. Classification must be
    // conservative: when in doubt, the gate stops before lowering.
    for kind in [
        SemanticErrorKind::VariableUndefined,
        SemanticErrorKind::FunctionUndefined,
        SemanticErrorKind::MemberUndefined,
    ] {
        assert!(
            is_use_site_only_error(&SemanticError::structured(
                kind,
                radix_lexer::Span::default()
            )),
            "{kind:?} is a use-site name-resolution miss and must reach lowering"
        );
    }
    for kind in [
        // Definition-identity / coherence class: breaks the table lowering
        // assumes, so the gate must keep firing.
        SemanticErrorKind::DefinitionDuplicate,
        SemanticErrorKind::ImportAbsent,
        SemanticErrorKind::CircularDependency,
        SemanticErrorKind::TypeUndefined,
        SemanticErrorKind::LoweringError,
        SemanticErrorKind::AnnotationInvalid,
        SemanticErrorKind::CliValidation,
        // Pre-lowering shape / context / type kinds are also not use-site-only.
        SemanticErrorKind::TypeMismatch,
        SemanticErrorKind::ReturnOutsideFunction,
        SemanticErrorKind::BreakOutsideBreakable,
        SemanticErrorKind::ContinueOutsideLoop,
        SemanticErrorKind::UseBeforeInit,
        SemanticErrorKind::NonExhaustiveMatch,
        SemanticErrorKind::ReturnAbsent,
        SemanticErrorKind::Warning(WarningKind::FileInterfaceExportSkipped),
    ] {
        assert!(
            !is_use_site_only_error(&SemanticError::structured(
                kind,
                radix_lexer::Span::default()
            )),
            "{kind:?} must keep the pre-lowering gate firing"
        );
    }
}

#[test]
fn use_site_only_errors_reach_lowering() {
    // em-u2 gate classification, integration: a program whose ONLY
    // pre-lowering errors are use-site name-resolution misses must reach
    // lowering. `unknown_thing` is a use-site miss (resolve reports
    // `UndefinedVariable` / `unknown_identifier`); the lowerer then folds in
    // its own `undefined_name` diagnostic — which is absent when the
    // pre-lowering gate stops the pipeline.
    let lex = radix_lexer::lex("functio f() → numerus {\n    redde unknown_thing\n}\n");
    let mut parsed = radix_parser::parse(lex);
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("program");
    let config = PassConfig::for_target(Target::HirRust);

    let result = analyze(&program, &config, &mut parsed.interner);

    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.kind == SemanticErrorKind::VariableUndefined),
        "resolve must report the use-site miss, got: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|err| {
            err.kind == SemanticErrorKind::LoweringError
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "undefined_name")
        }),
        "lowering must have run and folded its undefined_name in before the complete-set gate, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn coherence_class_errors_stop_before_lowering() {
    // em-u2 gate classification regression: definition-identity errors
    // (duplicate definitions) still stop the pipeline BEFORE lowering — no
    // lowering-own diagnostic may be folded in.
    let lex = radix_lexer::lex("functio dup() {}\nfunctio dup() {}\n");
    let mut parsed = radix_parser::parse(lex);
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("program");
    let config = PassConfig::for_target(Target::HirRust);

    let result = analyze(&program, &config, &mut parsed.interner);

    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.kind == SemanticErrorKind::DefinitionDuplicate),
        "duplicate definitions must be reported, got: {:?}",
        result.diagnostics
    );
    assert!(
        result.hir.is_none(),
        "coherence-class errors must stop before lowering (hir must be None)"
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|err| err.kind == SemanticErrorKind::LoweringError),
        "lowering must not have run for a coherence-class error, got: {:?}",
        result.diagnostics
    );
}
