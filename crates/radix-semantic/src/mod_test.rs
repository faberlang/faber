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
    let lex = radix_lexer::lex("sit numerus x\n");
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
    // Direct classification regression for the pre-lowering gate predicate. The
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

// ---------------------------------------------------------------------------
// format-data-loss TL-1 — executable top-level statements beside `incipit`.
// ---------------------------------------------------------------------------

/// Parse `source` and return the `issue` slug of every `LoweringError`, with its span line.
fn lowering_issues(source: &str) -> Vec<(String, usize)> {
    let lex = radix_lexer::lex(source);
    let mut parsed = radix_parser::parse(lex);
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("program");
    let config = PassConfig::for_target(Target::HirRust);
    let result = analyze(&program, &config, &mut parsed.interner);
    result
        .diagnostics
        .iter()
        .filter(|err| err.kind == SemanticErrorKind::LoweringError)
        .map(|err| {
            let issue = err
                .args
                .iter()
                .find(|arg| arg.name == "issue")
                .map(|arg| arg.value.to_string())
                .expect("issue arg");
            (
                issue,
                source[..err.span.start as usize].matches('\n').count() + 1,
            )
        })
        .collect()
}

#[test]
fn top_level_statements_beside_incipit_are_rejected_one_per_statement() {
    let issues = lowering_issues(
        "sit numerus a ← 25\nsi a ≻ 1 {\n    scribe \"x\"\n}\nsi a ≻ 2 {\n    scribe \"y\"\n}\nsi a ≻ 3 {\n    scribe \"z\"\n}\nincipit {\n    scribe \"m\"\n}\n",
    );
    assert_eq!(
        issues,
        vec![
            ("top_level_statement_beside_entry".to_owned(), 2),
            ("top_level_statement_beside_entry".to_owned(), 5),
            ("top_level_statement_beside_entry".to_owned(), 8),
        ]
    );
}

#[test]
fn top_level_statements_without_incipit_keep_the_implicit_entry() {
    let issues = lowering_issues("sit numerus a ← 25\nsi a ≻ 1 {\n    scribe \"x\"\n}\n");
    assert!(issues.is_empty(), "unexpected issues: {issues:?}");
}

#[test]
fn failed_top_level_declaration_beside_incipit_has_only_its_own_error() {
    let issues = lowering_issues("sit numerus x\nincipit {\n    scribe \"m\"\n}\n");
    assert_eq!(issues, vec![("top_level_initializer".to_owned(), 1)]);
}

// ---------------------------------------------------------------------------
// locale-pack type spellings — edge-policy rows stay out of type position.
// ---------------------------------------------------------------------------

#[test]
fn locale_pack_type_spellings_skip_edge_policy_rows() {
    // `register_locale_pack_type_spellings` deliberately skips `Frame` and
    // `TensorEdgePolicy` rows: those names resolve in type position through
    // the parser/lowerer and must not become type aliases a user value
    // binding could collide with. `modulus_t` is a non-skipped row from the
    // same pack, proving the registration loop actually ran.
    let lex = radix_lexer::lex("functio f() {}\n");
    let mut parsed = radix_parser::parse(lex);
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("program");
    let mut config = PassConfig::base();
    config.locale_pack = Some(crate::locale::latin_locale_pack());

    let result = analyze(&program, &config, &mut parsed.interner);

    let modulus_t = parsed.interner.intern("modulus_t");
    assert!(
        result.resolver.lookup_type(modulus_t).is_some(),
        "the pack's non-skipped rows must register"
    );
    for policy in crate::BuiltinTensorEdgePolicy::ALL {
        let symbol = parsed.interner.intern(policy.latin());
        assert!(
            result.resolver.lookup_type(symbol).is_none(),
            "edge-policy spelling {} must stay out of type position",
            policy.latin()
        );
    }
}
