//! Semantic `check` command handler and normalized diagnostic collection.
//!
//! Single-file check for the radix CLI: lex, parse, and analyze one source
//! input, then print diagnostics or return a normalized collection. Package
//! inputs are rejected with a message pointing at the `faber` CLI. Fatal
//! errors that permissive mode does not downgrade exit with code 1.

use super::super::cli::{CheckCommand, DiagnosticMode};
use super::diagnostics_locale_pack::load_diagnostics_locale_pack_or_exit;
use super::emit::print_diagnostics;
use super::package::should_treat_as_package_from_input;
use super::source::{
    format_location, lex_error_message, parse_error_message, read_source, semantic_error_message,
    source_file_from_input,
};
use crate::diagnostics::{DiagnosticConvert, Severity};
use crate::driver::WarnPolicy;

const MISSING_PARSE_PROGRAM_MESSAGE: &str = "successful parse result missing program";
const PERMISSIVE_DOWNGRADE_MESSAGE: &str = "permissive mode downgraded unresolved/import-driven semantic errors";

pub fn cmd_check(command: CheckCommand) {
    if command.package || should_treat_as_package_from_input(&command.input) {
        eprintln!("error: package checking is owned by the `faber` tool; rerun with `faber check --package`");
        std::process::exit(1);
    }

    let warn_policy = WarnPolicy { deny_all_warnings: command.deny_warnings, deny_codes: command.deny_codes.clone() };

    let (name, source) = read_source(&command.input);
    let source_file = source_file_from_input(name, source);

    // Code pack: an explicit `--locale` wins; otherwise honor the source's
    // `+++ locale = "…" +++` frontmatter (en-surface corpus) exactly like the
    // analysis driver and the diagnostic-check path. The `SourceFile` content
    // is already frontmatter-stripped, so frontmatter resolution goes through
    // the peeled metadata.
    let mut code_pack = super::compile::explicit_locale_pack(command.locale.as_deref())
        .or_else(|| super::compile::frontmatter_locale_pack_from_frontmatter(source_file.frontmatter.as_ref()));
    let diagnostics_locale_pack = load_diagnostics_locale_pack_or_exit(command.diagnostics_locale_pack.as_ref());

    if command.diagnostic_mode == DiagnosticMode::Diagnostics || command.diagnostics_locale_pack.is_some() {
        run_diagnostic_check(
            &command,
            &source_file,
            &warn_policy,
            code_pack.as_ref(),
            diagnostics_locale_pack.as_ref(),
        );
        return;
    }

    let (program, mut interner) = parse_check_program(&source_file, code_pack.as_ref());
    let cli_analysis = crate::cli::analyze(&program, &interner);
    if print_cli_errors(&source_file, &cli_analysis.errors) > 0 {
        std::process::exit(1);
    }

    let mut pass_config = crate::semantic::PassConfig::for_target(crate::codegen::Target::HirRust);
    // WHY: the code/reader pack must reach the semantic passes (intrinsic
    // surface→canonical resolution, type spellings), not just lexing and
    // import contracts. Without it the CLI check treats every localized
    // intrinsic surface (en `contains` → `continet`) as an unknown method
    // miss and leaves a fresh inference hole that surfaces as
    // SEM014.cannot_infer_expression_type for non-literal args — while the
    // driver path (`faber`/`emit`) rewrites the same call and checks green.
    // The driver wires the pack at the same seam (`driver::mod.rs`).
    if let Some(path) = source_path_for_import_contract(&source_file.name) {
        let contract = super::compile::import_contract_for_file(&path, &source_file.content, code_pack.as_ref(), None);
        if let Some(pack) = code_pack.as_mut() {
            if let Err(err) = crate::locale::merge_library_locale_packs(pack, &contract.library_package_roots) {
                eprintln!(
                    "error: failed to load library locale pack: {}",
                    crate::locale::format_locale_pack_load_error(&err)
                );
                std::process::exit(1);
            }
        }
        pass_config.namespace_exports = contract.namespace_exports;
        pass_config.namespace_seams = contract.namespace_seams;
        pass_config.namespace_declarations = contract.namespace_declarations;
        pass_config.file_interfaces = contract.file_interfaces;
    }
    pass_config.locale_pack.clone_from(&code_pack);
    let semantic_result =
        crate::semantic::analyze_with_cli(&program, &pass_config, &mut interner, cli_analysis.program.as_ref());

    let report =
        print_semantic_diagnostics(&source_file, &semantic_result.diagnostics, command.permissive, &warn_policy);
    if command.permissive && report.downgraded > 0 {
        eprintln!(
            "warning:{}: downgraded {} unresolved/import-driven semantic error(s) in permissive mode",
            source_file.name.as_str(),
            report.downgraded
        );
    }

    if report.denied_warnings > 0 {
        eprintln!(
            "error:{}: {} warning(s) promoted to error(s) by --deny policy",
            source_file.name.as_str(),
            report.denied_warnings
        );
    }

    let ok =
        (semantic_result.success() || (command.permissive && report.fatal_errors == 0)) && report.denied_warnings == 0;
    if ok {
        eprintln!("ok: {}", source_file.name.as_str());
    } else {
        std::process::exit(1);
    }
}

fn run_diagnostic_check(
    command: &CheckCommand,
    source_file: &crate::driver::SourceFile,
    warn_policy: &WarnPolicy,
    code_pack: Option<&crate::locale::LocalePack>,
    diagnostics_locale_pack: Option<&crate::locale::LocalePack>,
) {
    // `code_pack` (explicit `--locale` or source frontmatter) drives lexing;
    // `diagnostics_locale_pack` (from `--diagnostics-locale`) drives message
    // rendering only. When no diagnostics pack is given, the catalog English
    // rendering applies.
    let mut result = check_diagnostics_for_source_with_locale_pack(
        &source_file.name,
        &source_file.content,
        command.permissive,
        code_pack,
        None,
    );
    crate::driver::apply_warn_policy(&mut result.diagnostics, warn_policy);
    let success = result.success
        && !result
            .diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error);
    if !result.diagnostics.is_empty() {
        print_diagnostics(&result.diagnostics, command.diagnostic_mode, diagnostics_locale_pack);
    }
    if success {
        eprintln!("ok: {}", source_file.name.as_str());
    } else {
        std::process::exit(1);
    }
}

/// Use a filesystem path when available so provider imports can be resolved.
fn source_path_for_import_contract(name: &str) -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(name);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

fn parse_check_program(
    source_file: &crate::driver::SourceFile,
    code_pack: Option<&crate::locale::LocalePack>,
) -> (crate::syntax::Program, crate::lexer::Interner) {
    let lex_result = match code_pack {
        Some(pack) => crate::lexer::lex_with_locale_pack(source_file.content.as_str(), pack),
        None => crate::lexer::lex(source_file.content.as_str()),
    };
    if !lex_result.success() {
        for err in &lex_result.errors {
            eprintln!(
                "{}: {}",
                format_location(source_file, err.span.start),
                lex_error_message(source_file, err)
            );
        }
        std::process::exit(1);
    }

    let parse_result = crate::parser::parse(lex_result);
    if !parse_result.success() {
        for err in &parse_result.errors {
            eprintln!(
                "{}: {}",
                format_location(source_file, err.span.start),
                parse_error_message(source_file, err)
            );
        }
        std::process::exit(1);
    }

    let crate::parser::ParseResult { program, interner, .. } = parse_result;
    let Some(program) = program else {
        eprintln!("internal error: successful parse result missing program");
        std::process::exit(1);
    };

    (program, interner)
}

fn print_cli_errors(source_file: &crate::driver::SourceFile, errors: &[crate::semantic::SemanticError]) -> usize {
    for err in errors {
        eprintln!(
            "error: {}: {}",
            format_location(source_file, err.span.start),
            semantic_error_message(source_file, err)
        );
    }
    errors.len()
}

struct CheckErrorTally {
    fatal_errors: usize,
    downgraded: usize,
    denied_warnings: usize,
}

fn print_semantic_diagnostics(
    source_file: &crate::driver::SourceFile,
    diagnostics: &[crate::semantic::SemanticError],
    permissive: bool,
    warn_policy: &WarnPolicy,
) -> CheckErrorTally {
    let mut fatal_errors = 0usize;
    let mut downgraded = 0usize;
    let mut denied_warnings = 0usize;
    for err in diagnostics {
        let downgraded_error = permissive && err.kind.is_permissive_check_downgrade();
        let code = crate::diagnostics::semantic_spec(err.kind).code;
        let denied_warning =
            (!err.is_error() || downgraded_error) && warn_policy.promotes(Severity::Warning, Some(code));
        let prefix = if (err.is_error() && !downgraded_error) || denied_warning {
            "error"
        } else {
            "warning"
        };
        eprintln!(
            "{}: {}: {}",
            prefix,
            format_location(source_file, err.span.start),
            semantic_error_message(source_file, err)
        );
        if err.is_error() {
            if downgraded_error {
                downgraded += 1;
            } else {
                fatal_errors += 1;
            }
        }
        if denied_warning {
            denied_warnings += 1;
        }
    }
    CheckErrorTally { fatal_errors, downgraded, denied_warnings }
}

/// Result of running check through normalized diagnostics.
#[derive(Debug)]
pub struct CheckDiagnosticsResult {
    /// Normalized diagnostics collected before check completion.
    pub diagnostics: Vec<crate::diagnostics::Diagnostic>,

    /// Whether check policy accepts the input after diagnostics are considered.
    pub success: bool,
}

/// Run single-file check and collect normalized diagnostics without exiting.
#[must_use]
pub fn check_diagnostics_for_source(name: &str, source: &str, permissive: bool) -> CheckDiagnosticsResult {
    check_diagnostics_for_source_with_locale_pack(name, source, permissive, None, None)
}

/// Run single-file check with an optional reader pack and collect normalized diagnostics without exiting.
pub fn check_diagnostics_for_source_with_locale_pack(
    name: &str,
    source: &str,
    permissive: bool,
    code_pack: Option<&crate::locale::LocalePack>,
    templates: Option<&std::collections::BTreeMap<String, String>>,
) -> CheckDiagnosticsResult {
    let mut diagnostics = Vec::new();

    // Honor the source's `+++ locale = "…" +++` frontmatter when the caller
    // supplied no explicit reader pack, mirroring the analysis driver. The
    // en-surface corpus depends on it: without the pack, `const`, `fn`, and
    // `import from …` spellings do not lex as keywords and check fails at
    // parse with no diagnostics of substance.
    let mut resolved_pack = code_pack
        .map(ToOwned::to_owned)
        .or_else(|| super::compile::frontmatter_locale_pack_from_source(name, source));

    let lex_result = lex_check_source(source, resolved_pack.as_ref());
    collect_locale_diagnostics(&mut diagnostics, name, source, &lex_result);
    if !lex_result.success() {
        collect_lex_diagnostics(&mut diagnostics, name, source, &lex_result.errors);
        return CheckDiagnosticsResult { diagnostics, success: false };
    }

    let parse_result = crate::parser::parse(lex_result);
    let had_parse_errors = !parse_result.success();
    if had_parse_errors {
        collect_parse_diagnostics(&mut diagnostics, name, source, &parse_result.errors);
    }
    let parse_error_spans: Vec<crate::lexer::Span> = parse_result.errors.iter().map(|err| err.span).collect();
    let Some((mut program, mut interner)) = parsed_check_program(parse_result, &mut diagnostics, name) else {
        return CheckDiagnosticsResult { diagnostics, success: false };
    };
    // When the parser recovered a partial AST, drop cascade-corrupted items
    // and continue semantic analysis on the remainder so parse diagnostics
    // and the recovered items' semantic diagnostics co-report in one pass.
    if had_parse_errors {
        program = crate::driver::recovered_program_items(program, &parse_error_spans);
    }

    let cli_analysis = crate::cli::analyze(&program, &interner);
    collect_cli_diagnostics(&mut diagnostics, name, source, &cli_analysis.errors);
    // Parse-phase diagnostics already collected must not stop analysis here;
    // CLI-analysis errors still do.
    if diagnostics
        .iter()
        .any(|diag| diag.is_error() && diag.phase != crate::diagnostics::DiagnosticPhase::Parse)
    {
        return CheckDiagnosticsResult { diagnostics, success: false };
    }

    let mut pass_config = crate::semantic::PassConfig::for_target(crate::codegen::Target::HirRust);
    // WHY: same pack seam as `cmd_check` — the resolved reader pack must reach
    // the semantic passes so localized intrinsic surfaces resolve to their
    // canonical registry names instead of falling through the unknown-method
    // miss path (SEM014.cannot_infer_expression_type on non-literal args).
    pass_config.import_path_templates = templates.cloned();
    if let Some(path) = source_path_for_import_contract(name) {
        let contract = super::compile::import_contract_for_file(&path, source, resolved_pack.as_ref(), templates);
        if let Some(pack) = resolved_pack.as_mut() {
            if let Err(err) = crate::locale::merge_library_locale_packs(pack, &contract.library_package_roots) {
                diagnostics.push(
                    crate::diagnostics::Diagnostic::error(format!(
                        "failed to load library locale pack: {}",
                        crate::locale::format_locale_pack_load_error(&err)
                    ))
                    .with_file(name.to_owned()),
                );
                return CheckDiagnosticsResult { diagnostics, success: false };
            }
        }
        pass_config.namespace_exports = contract.namespace_exports;
        pass_config.namespace_seams = contract.namespace_seams;
        pass_config.namespace_declarations = contract.namespace_declarations;
        pass_config.file_interfaces = contract.file_interfaces;
    }
    pass_config.locale_pack.clone_from(&resolved_pack);
    let semantic_result =
        crate::semantic::analyze_with_cli(&program, &pass_config, &mut interner, cli_analysis.program.as_ref());

    let report =
        collect_semantic_check_diagnostics(&mut diagnostics, name, source, &semantic_result.diagnostics, permissive);

    if permissive && report.downgraded > 0 {
        push_permissive_downgrade_diagnostic(&mut diagnostics, name, report.downgraded);
    }

    CheckDiagnosticsResult {
        diagnostics,
        // Parse errors are not downgradable, so a recovered parse can never
        // check clean even when recovered items' semantic analysis succeeds.
        success: !had_parse_errors && (semantic_result.success() || (permissive && report.fatal_errors == 0)),
    }
}

fn lex_check_source(source: &str, code_pack: Option<&crate::locale::LocalePack>) -> crate::lexer::LexResult {
    match code_pack {
        Some(pack) => crate::lexer::lex_with_locale_pack(source, pack),
        None => crate::lexer::lex(source),
    }
}

fn collect_locale_diagnostics(
    diagnostics: &mut Vec<crate::diagnostics::Diagnostic>,
    name: &str,
    source: &str,
    lex_result: &crate::lexer::LexResult,
) {
    diagnostics.extend(
        lex_result
            .locale_suggestions
            .iter()
            .map(|suggestion| crate::diagnostics::Diagnostic::from_locale_suggestion(name, source, suggestion)),
    );
}

fn collect_lex_diagnostics(
    diagnostics: &mut Vec<crate::diagnostics::Diagnostic>,
    name: &str,
    source: &str,
    errors: &[crate::lexer::LexError],
) {
    diagnostics.extend(
        errors
            .iter()
            .map(|err| crate::diagnostics::Diagnostic::from_lex_error(name, source, err)),
    );
}

fn collect_parse_diagnostics(
    diagnostics: &mut Vec<crate::diagnostics::Diagnostic>,
    name: &str,
    source: &str,
    errors: &[crate::parser::ParseError],
) {
    diagnostics.extend(
        errors
            .iter()
            .map(|err| crate::diagnostics::Diagnostic::from_parse_error(name, source, err)),
    );
}

fn parsed_check_program(
    parse_result: crate::parser::ParseResult,
    diagnostics: &mut Vec<crate::diagnostics::Diagnostic>,
    name: &str,
) -> Option<(crate::syntax::Program, crate::lexer::Interner)> {
    let crate::parser::ParseResult { program, interner, .. } = parse_result;
    let Some(program) = program else {
        diagnostics.push(
            crate::diagnostics::Diagnostic::error(MISSING_PARSE_PROGRAM_MESSAGE)
                .with_phase(crate::diagnostics::DiagnosticPhase::Parse)
                .with_code("INTERNAL001")
                .with_args(vec![crate::diagnostics::DiagnosticArg::new(
                    "issue",
                    "parse_result_missing_program",
                )])
                .with_file(name.to_owned()),
        );
        return None;
    };

    Some((program, interner))
}

fn collect_cli_diagnostics(
    diagnostics: &mut Vec<crate::diagnostics::Diagnostic>,
    name: &str,
    source: &str,
    errors: &[crate::semantic::SemanticError],
) {
    diagnostics.extend(
        errors
            .iter()
            .map(|err| crate::diagnostics::Diagnostic::from_semantic_error(name, source, err)),
    );
}

fn collect_semantic_check_diagnostics(
    diagnostics: &mut Vec<crate::diagnostics::Diagnostic>,
    name: &str,
    source: &str,
    errors: &[crate::semantic::SemanticError],
    permissive: bool,
) -> CheckErrorTally {
    let mut fatal_errors = 0usize;
    let mut downgraded = 0usize;
    for err in errors {
        let downgraded_error = permissive && err.kind.is_permissive_check_downgrade();
        let mut diagnostic = crate::diagnostics::Diagnostic::from_semantic_error(name, source, err);
        if downgraded_error {
            diagnostic = diagnostic.with_severity(crate::diagnostics::Severity::Warning);
            downgraded += 1;
        } else if err.is_error() {
            fatal_errors += 1;
        }
        diagnostics.push(diagnostic);
    }

    CheckErrorTally { fatal_errors, downgraded, denied_warnings: 0 }
}

fn push_permissive_downgrade_diagnostic(
    diagnostics: &mut Vec<crate::diagnostics::Diagnostic>,
    name: &str,
    downgraded: usize,
) {
    diagnostics.push(
        crate::diagnostics::Diagnostic::warning(PERMISSIVE_DOWNGRADE_MESSAGE)
            .with_phase(crate::diagnostics::DiagnosticPhase::Tool)
            .with_code("TOOL001")
            .with_args(vec![
                crate::diagnostics::DiagnosticArg::new("issue", "permissive_semantic_errors_downgraded"),
                crate::diagnostics::DiagnosticArg::new("count", downgraded.to_string()),
            ])
            .with_file(name.to_owned()),
    );
}
