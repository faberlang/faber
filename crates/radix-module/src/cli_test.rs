//! Tests for CLI argument parsing and dispatch.
//! Proves: CLI modes (`CliMode`, `CliType`) parse correctly; mounted command
//! modules prefix their routes; diagnostic conversion from lex/parse results
//! works through the CLI entry point.

use super::{CliMode, CliType, analyze};
use crate::diagnostics::DiagnosticConvert;
use crate::diagnostics::{Diagnostic, DiagnosticArg};
use crate::lexer::lex_with_locale_pack;
use crate::parser::parse;
use crate::semantic::SemanticErrorKind;
use crate::test_support::args_have_no_legacy_message;

/// The built-in Latin (`la`) reader pack: the canonical surface the parser
/// tests below are written in.
fn la_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("installed la pack should load")
}

/// The built-in English (`en`) reader pack: the default user-facing surface.
fn en_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("installed en pack should load")
}

fn parse_program(source: &str) -> crate::parser::ParseResult {
    let result = parse(lex_with_locale_pack(source, &la_pack()));
    assert!(
        result.errors.is_empty(),
        "expected parse success, got {:?}",
        result
            .errors
            .iter()
            .map(|err| format!("{err:?}"))
            .collect::<Vec<_>>()
    );
    result
}

fn analyze_source(source: &str) -> super::CliAnalysis {
    let result = parse_program(source);
    analyze(result.program.as_ref().expect("program"), &result.interner)
}

fn analyze_en_source(source: &str) -> super::CliAnalysis {
    let result = parse(lex_with_locale_pack(source, &en_pack()));
    assert!(
        result.errors.is_empty(),
        "expected parse success, got {:?}",
        result
            .errors
            .iter()
            .map(|err| format!("{err:?}"))
            .collect::<Vec<_>>()
    );
    analyze(result.program.as_ref().expect("program"), &result.interner)
}

fn analyze_mounted_source(source: &str, prefix: &[&str]) -> super::CliAnalysis {
    let result = parse_program(source);
    let prefix = prefix
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>();
    super::analyze_mounted_module(result.program.as_ref().expect("program"), &result.interner, &prefix)
}

fn messages(analysis: &super::CliAnalysis) -> Vec<String> {
    analysis
        .errors
        .iter()
        .map(|err| {
            let diagnostic = Diagnostic::from_semantic_error("cli.fab", "", err);
            format!("{:?}:{:?}", diagnostic.code, diagnostic.issue())
        })
        .collect()
}

fn expect_cli_issue<'a>(analysis: &'a super::CliAnalysis, issue: &str) -> &'a crate::semantic::SemanticError {
    let err = analysis
        .errors
        .iter()
        .find(|err| {
            err.kind == SemanticErrorKind::CliValidation
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == issue)
        })
        .expect("structured CLI validation diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated CLI diagnostic should not choose prose at emit site"
    );
    err
}

fn expect_cli_issue_with_args<'a>(
    analysis: &'a super::CliAnalysis,
    issue: &str,
    expected_args: &[(&str, &str)],
) -> &'a crate::semantic::SemanticError {
    let err = expect_cli_issue(analysis, issue);
    for (name, value) in expected_args {
        assert!(
            err.args
                .iter()
                .any(|arg| arg.name == *name && arg.value == *value),
            "missing CLI diagnostic arg {name}={value}; got {:?}",
            err.args
        );
    }
    err
}

#[test]
fn detects_not_cli_programs() {
    let analysis = analyze_source("incipit {}");

    assert_eq!(analysis.mode, CliMode::NotCli);
    assert!(analysis.program.is_none());
    assert!(analysis.errors.is_empty());
}

#[test]
fn builds_single_command_cli_ir() {
    let analysis = analyze_source(
        r#"
@ cli "salve"
@ versio "1.0.0"
@ descriptio "Greets people"
@ optio name longum "name" typus textus vel "Roma"
@ optio loud brevis "l" longum "loud" typus bivalens
@ operandus ceteri textus words
incipit argumenta args {}
"#,
    );

    assert!(analysis.errors.is_empty(), "{:?}", messages(&analysis));
    let program = analysis.program.expect("cli program");
    assert_eq!(program.mode, CliMode::SingleCommand);
    assert_eq!(program.name, "salve");
    assert_eq!(program.entry_args, "args");
    assert_eq!(program.version.as_deref(), Some("1.0.0"));
    assert_eq!(program.options.len(), 2);
    assert_eq!(program.options[0].ty, CliType::Textus);
    assert_eq!(program.options[1].ty, CliType::Bivalens);
    assert!(program.options[1].flag);
    assert!(matches!(program.options[1].default, Some(super::CliDefault::Bool(false))));
    assert_eq!(program.operands.len(), 1);
    assert!(program.operands[0].rest);
}

#[test]
fn braced_operand_rest_key_is_accepted_on_the_en_surface() {
    let analysis = analyze_en_source(
        r#"
@ cli { name = "tool" }
@ operand { binding = files, type = string, rest = true, description = "Extra files" }
main args args {
}
"#,
    );

    assert!(analysis.errors.is_empty(), "{:?}", messages(&analysis));
    let program = analysis.program.expect("cli program");
    assert_eq!(program.mode, CliMode::SingleCommand);
    assert_eq!(program.operands.len(), 1);
    assert!(program.operands[0].rest);
    assert_eq!(program.operands[0].binding, "files");
    assert_eq!(program.operands[0].ty, CliType::Textus);
}

#[test]
fn builds_subcommand_cli_ir_with_globals() {
    let analysis = analyze_source(
        r#"
@ cli "tool"
@ optio verbose brevis "v" longum "verbose" typus bivalens ubique
incipit argumenta args {}

@ imperium "jobs/list"
@ alias "ls"
@ descriptio "List jobs"
@ optio limit longum "limit" typus numerus vel 20
functio list() argumenta args {}
"#,
    );

    assert!(analysis.errors.is_empty(), "{:?}", messages(&analysis));
    let program = analysis.program.expect("cli program");
    assert_eq!(program.mode, CliMode::Subcommand);
    assert_eq!(program.global_options.len(), 1);
    assert_eq!(program.commands.len(), 1);
    assert_eq!(program.commands[0].path, vec!["jobs", "list"]);
    assert_eq!(program.commands[0].aliases, vec!["ls"]);
    assert_eq!(program.commands[0].args_binding.as_deref(), Some("args"));
}

#[test]
fn requires_cli_entry_argument_binding() {
    let analysis = analyze_source(
        r#"
@ cli "tool"
incipit {}
"#,
    );

    expect_cli_issue(&analysis, "missing_entry_argument");
}

#[test]
fn cli_validation_uses_structured_issue_args() {
    let missing_entry = analyze_source(
        r#"
@ cli "bad"
incipit {}
"#,
    );
    let missing_entry = expect_cli_issue(&missing_entry, "missing_entry_argument");
    assert_eq!(missing_entry.args, vec![DiagnosticArg::new("issue", "missing_entry_argument")]);

    let unsupported = analyze_source(
        r#"
@ cli "bad"
@ optio output longum "output" typus tabula
incipit argumenta args {}
"#,
    );
    let unsupported_type = expect_cli_issue(&unsupported, "unsupported_type");
    assert_eq!(
        unsupported_type.args,
        vec![
            DiagnosticArg::new("issue", "unsupported_type"),
            DiagnosticArg::new("type", "tabula")
        ]
    );
}

#[test]
fn rejects_cli_annotation_on_non_incipit_without_panicking() {
    let analysis = analyze_source(
        r#"
@ cli "tool"
functio tool() {}
"#,
    );

    assert_eq!(analysis.mode, CliMode::NotCli);
    assert!(analysis.program.is_none());
    expect_cli_issue(&analysis, "cli_on_non_incipit");
}

#[test]
fn validates_duplicate_flags_bindings_and_rest_ordering() {
    let analysis = analyze_source(
        r#"
@ cli "bad"
@ optio left brevis "x"
@ optio right brevis "x"
@ operandus ceteri textus files
@ operandus textus trailing
incipit argumenta args {}
"#,
    );
    expect_cli_issue_with_args(
        &analysis,
        "duplicate_short_flag",
        &[("short", "x"), ("label", "single-command")],
    );
    expect_cli_issue(&analysis, "rest_not_final");
    expect_cli_issue_with_args(&analysis, "operand_after_rest", &[("binding", "trailing")]);
}

#[test]
fn rejects_global_local_collisions() {
    let analysis = analyze_source(
        r#"
@ cli "bad"
@ optio config longum "config" ubique
incipit argumenta args {}

@ imperium "run"
@ optio config longum "local-config"
functio run() {}
"#,
    );

    expect_cli_issue_with_args(
        &analysis,
        "local_option_global_collision",
        &[("label", "command 'run'"), ("binding", "config")],
    );
}

#[test]
fn rejects_single_command_global_local_collisions() {
    let analysis = analyze_source(
        r#"
@ cli "bad"
@ optio config longum "config" ubique
@ optio config longum "local-config"
incipit argumenta args {}
"#,
    );

    expect_cli_issue_with_args(
        &analysis,
        "local_option_global_collision",
        &[("label", "single-command"), ("binding", "config")],
    );
}

#[test]
fn rejects_non_empty_root_body_for_subcommand_cli() {
    let analysis = analyze_source(
        r#"
@ cli "bad"
incipit argumenta args {
  nota "setup"
}

@ imperium "run"
functio run() {}
"#,
    );

    expect_cli_issue(&analysis, "subcommand_root_body");
}

#[test]
fn root_cli_analysis_keeps_module_mounts_for_package_compilation() {
    let analysis = analyze_source(
        r#"
@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    );

    assert!(analysis.errors.is_empty(), "{:?}", messages(&analysis));
    assert_eq!(analysis.program.expect("cli").commands.len(), 0);
}

#[test]
fn rejects_alias_collision_with_later_command_path() {
    let analysis = analyze_source(
        r#"
@ cli "bad"
incipit argumenta args {}

@ imperium "one"
@ alias "two"
functio one() {}

@ imperium "two"
functio two() {}
"#,
    );

    expect_cli_issue_with_args(&analysis, "alias_collides_with_path", &[("alias", "two")]);
}

#[test]
fn rejects_root_command_alias_paths() {
    let analysis = analyze_source(
        r#"
@ cli "bad"
incipit argumenta args {}

@ imperium "one"
@ alias "nested/path"
functio one() {}
"#,
    );

    expect_cli_issue_with_args(&analysis, "invalid_command_alias", &[("alias", "nested/path")]);
}

#[test]
fn rejects_unsupported_cli_types() {
    let analysis = analyze_source(
        r#"
@ cli "bad"
@ optio output longum "output" typus tabula<textus, textus>
incipit argumenta args {}
"#,
    );

    expect_cli_issue(&analysis, "unsupported_type_shape");
}

#[test]
fn malformed_braced_cli_annotation_reports_structured_error() {
    let analysis = analyze_source(
        r#"
@ cli { name = "tool" }
incipit argumenta args {}
"#,
    );

    assert_eq!(analysis.mode, CliMode::NotCli);
    assert!(analysis.program.is_none());
    expect_cli_issue_with_args(
        &analysis,
        "malformed_annotation",
        &[
            ("annotation", "@ cli"),
            ("field", "nomen"),
            ("expected", "string literal"),
        ],
    );
}

#[test]
fn malformed_braced_optio_annotation_reports_structured_error() {
    let analysis = analyze_source(
        r#"
@ cli "tool"
@ optio { binding = "verbose", longum = "verbose" }
incipit argumenta args {}
"#,
    );

    expect_cli_issue_with_args(
        &analysis,
        "malformed_annotation",
        &[
            ("annotation", "@ optio"),
            ("field", "binding"),
            ("expected", "identifier"),
        ],
    );
}

#[test]
fn analyzes_mounted_module_with_prefixed_commands() {
    let analysis = analyze_mounted_source(
        r#"
@ imperium "jobs"
@ alias "ls"
functio jobs() argumenta args {}
"#,
        &["tool"],
    );

    assert!(analysis.errors.is_empty(), "{:?}", messages(&analysis));
    assert_eq!(analysis.mode, CliMode::Subcommand);
    let program = analysis.program.expect("mounted program");
    assert!(program.name.is_empty());
    assert_eq!(program.commands.len(), 1);
    assert_eq!(program.commands[0].path, vec!["tool", "jobs"]);
    assert_eq!(program.commands[0].aliases, vec!["tool/ls"]);
    assert_eq!(program.commands[0].args_binding.as_deref(), Some("args"));
}

#[test]
fn rejects_root_cli_entry_in_mounted_module() {
    let analysis = analyze_mounted_source(
        r#"
@ cli "tool"
incipit argumenta args {}
"#,
        &["host"],
    );

    expect_cli_issue(&analysis, "mounted_module_root_cli");
    assert_eq!(analysis.mode, CliMode::NotCli);
    assert!(analysis.program.is_none());
}

#[test]
fn rejects_nested_module_mounts_in_mounted_module() {
    let analysis = analyze_mounted_source(
        r#"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
        &["tool"],
    );

    expect_cli_issue(&analysis, "nested_module_mount");
    assert_eq!(analysis.mode, CliMode::NotCli);
    assert!(analysis.program.is_none());
}
