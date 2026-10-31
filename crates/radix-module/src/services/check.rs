//! Typed check service: normalized diagnostics, no print/exit.

use crate::diagnostics::DiagnosticConvert;
use crate::driver::MISSING_PARSE_PROGRAM_MESSAGE;

const PERMISSIVE_DOWNGRADE_MESSAGE: &str = "permissive mode downgraded unresolved/import-driven semantic errors";

struct CheckErrorTally {
    fatal_errors: usize,
    downgraded: usize,
}

/// Use a filesystem path when available so provider imports can be resolved.
fn source_path_for_import_contract(name: &str) -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(name);
    if path.is_file() { Some(path) } else { None }
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
#[must_use]
pub fn check_diagnostics_for_source_with_locale_pack(
    name: &str,
    source: &str,
    permissive: bool,
    code_pack: Option<&crate::locale::LocalePack>,
    templates: Option<&std::collections::BTreeMap<String, String>>,
) -> CheckDiagnosticsResult {
    check_diagnostics_for_source_with_config(
        name,
        source,
        permissive,
        code_pack,
        templates,
        &crate::driver::Config::default(),
    )
}

/// Run single-file check under a caller-owned locale policy.
///
/// `locale_config` carries the language-pack root and product loader a
/// declared `+++ locale = "\u{2026}" +++` frontmatter resolves through (faber
/// installs its embedded reader packs here, so an installed binary checks a
/// declared locale with no workspace `locale/` tree beside it).
#[must_use]
pub fn check_diagnostics_for_source_with_config(
    name: &str,
    source: &str,
    permissive: bool,
    code_pack: Option<&crate::locale::LocalePack>,
    templates: Option<&std::collections::BTreeMap<String, String>>,
    locale_config: &crate::driver::Config,
) -> CheckDiagnosticsResult {
    // Reader-locale suggestions wait until the check result is known: an
    // edit-distance guess is shown only where an error sits at its span.
    let mut locale_candidates = Vec::new();
    let mut result = check_source_deferring_locale(
        name,
        source,
        permissive,
        code_pack,
        templates,
        locale_config,
        &mut locale_candidates,
    );
    let kept = crate::diagnostics::Diagnostic::filter_locale_suggestions(locale_candidates, &result.diagnostics);
    result.diagnostics.extend(kept);
    // WHY: a failed check of a source with no declared locale is the case where
    // a wrong-locale read hides behind unknown-identifier errors. The note is
    // advisory only; it never changes the locale the source was read in.
    if !result.success && code_pack.is_none() && !source_declares_locale(name, source) {
        let body = crate::driver::peel_raw_source(name, source).map_or(source, |peeled| peeled.body);
        if let Some(note) = crate::driver::latin_source_reads_as_other_locale(locale_config, name, body) {
            result.diagnostics.insert(0, note);
        }
    }
    result.diagnostics = normalize_locale002_notes(result.diagnostics);
    result
}

/// Keep one LOCALE002 note per file and hoist those notes ahead of the other
/// diagnostics.
///
/// WHY: the wrong-locale reading explains the very errors it hides behind, so
/// it must be reported once, first; a second keyword-level LOCALE002 for the
/// same file repeats the reading instead of adding a fact, so it is dropped.
#[must_use]
pub fn normalize_locale002_notes(
    diagnostics: Vec<crate::diagnostics::Diagnostic>,
) -> Vec<crate::diagnostics::Diagnostic> {
    let mut notes = Vec::new();
    let mut rest = Vec::new();
    let mut noted_files = std::collections::BTreeSet::new();
    for diagnostic in diagnostics {
        if diagnostic.code == Some("LOCALE002") {
            if noted_files.insert(diagnostic.file.clone()) {
                notes.push(diagnostic);
            }
        } else {
            rest.push(diagnostic);
        }
    }
    notes.extend(rest);
    notes
}

/// Whether `source` declares its reader locale in `+++` frontmatter.
///
/// WHY: the wrong-locale advisory may only claim the source "has no declared
/// locale" when that is true. Callers that assemble diagnostics outside this
/// service use the same peel to suppress the note for declared sources.
/// Declaration is a frontmatter fact only — it does not depend on the
/// declared pack being loadable from any root or loader.
#[must_use]
pub fn source_declares_locale(name: &str, source: &str) -> bool {
    crate::driver::peel_raw_source(name, source).is_ok_and(|peeled| {
        peeled
            .frontmatter
            .as_ref()
            .is_some_and(|frontmatter| frontmatter.locale_result().is_some())
    })
}

fn check_source_deferring_locale(
    name: &str,
    source: &str,
    permissive: bool,
    code_pack: Option<&crate::locale::LocalePack>,
    templates: Option<&std::collections::BTreeMap<String, String>>,
    locale_config: &crate::driver::Config,
    locale_candidates: &mut Vec<crate::diagnostics::Diagnostic>,
) -> CheckDiagnosticsResult {
    let mut diagnostics = Vec::new();
    let peeled = match crate::driver::peel_raw_source(name, source) {
        Ok(peeled) => peeled,
        Err(error) => {
            diagnostics.push(crate::driver::source_load_diagnostic(name, &error));
            return CheckDiagnosticsResult { diagnostics, success: false };
        }
    };
    let body = peeled.body;

    // Honor the source's `+++ locale = "…" +++` frontmatter when no explicit
    // pack won, mirroring the analysis driver (`resolve_peeled_lex_pack`): a
    // caller-supplied pack or an explicit config pack wins over the
    // declaration, the declaration itself wins over the session default, and
    // the no-locale default is `en`. The en-surface corpus depends on the
    // frontmatter branch: without the pack, `const`, `fn`, and `import from …`
    // spellings do not lex as keywords and check fails at parse with no
    // diagnostics of substance. A declared locale that cannot be resolved
    // under `locale_config` fails the check with its LOCALE003 diagnostic —
    // never a silent read in another surface.
    let explicit_session_pack = locale_config
        .locale_pack_explicit
        .then(|| locale_config.locale_pack.clone())
        .flatten();
    let declared_pack = match code_pack.map(ToOwned::to_owned).or(explicit_session_pack) {
        Some(pack) => Some(pack),
        None => match crate::program::compile::frontmatter_locale_pack_from_frontmatter(
            locale_config,
            peeled.frontmatter.as_ref(),
        ) {
            Ok(pack) => pack,
            Err(diagnostic) => {
                diagnostics.push(*diagnostic);
                return CheckDiagnosticsResult { diagnostics, success: false };
            }
        },
    };
    let mut resolved_pack = match declared_pack.or_else(|| locale_config.locale_pack.clone()) {
        Some(pack) => Some(pack),
        None => match crate::driver::default_code_pack(locale_config) {
            Ok(pack) => Some(pack),
            Err(diagnostic) => {
                diagnostics.push(*diagnostic);
                return CheckDiagnosticsResult { diagnostics, success: false };
            }
        },
    };

    let lex_result = lex_check_source(body, resolved_pack.as_ref());
    collect_locale_diagnostics(&mut diagnostics, locale_candidates, name, body, &lex_result);
    if !lex_result.success() {
        collect_lex_diagnostics(&mut diagnostics, name, body, &lex_result.errors);
        return CheckDiagnosticsResult { diagnostics, success: false };
    }

    let parse_result = crate::parser::parse(lex_result);
    let had_parse_errors = !parse_result.success();
    if had_parse_errors {
        collect_parse_diagnostics(&mut diagnostics, name, body, &parse_result.errors);
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
    collect_cli_diagnostics(&mut diagnostics, name, body, &cli_analysis.errors);
    // Parse-phase diagnostics already collected must not stop analysis here;
    // CLI-analysis errors still do.
    if diagnostics
        .iter()
        .any(|diag| diag.is_error() && diag.phase != crate::diagnostics::DiagnosticPhase::Parse)
    {
        return CheckDiagnosticsResult { diagnostics, success: false };
    }

    // A program bound to a target by a target-native import is checked for
    // that target: `check` has no build target of its own to compare.
    let check_target = program
        .statements
        .iter()
        .find_map(|stmt| match &stmt.kind {
            crate::syntax::StmtKind::Import(decl) => decl.target.as_ref(),
            _ => None,
        })
        .and_then(|word| crate::codegen::Target::from_import_word(interner.resolve(word.name)))
        .unwrap_or(crate::codegen::Target::HirRust);
    let mut pass_config = crate::semantic::PassConfig::for_target(check_target);
    // WHY: same pack seam as `cmd_check` — the resolved reader pack must reach
    // the semantic passes so localized intrinsic surfaces resolve to their
    // canonical registry names instead of falling through the unknown-method
    // miss path (SEM014.cannot_infer_expression_type on non-literal args).
    pass_config.import_path_templates = templates.cloned();
    if let Some(path) = source_path_for_import_contract(name) {
        let contract = crate::program::compile::import_contract_for_file_with_config(
            &path,
            source,
            resolved_pack.as_ref(),
            templates,
            locale_config,
        );
        if let Some(pack) = resolved_pack.as_mut()
            && let Err(err) = crate::locale::merge_library_locale_packs(pack, &contract.library_package_roots)
        {
            diagnostics.push(
                crate::diagnostics::Diagnostic::error(format!(
                    "failed to load library locale pack: {}",
                    crate::locale::format_locale_pack_load_error(&err)
                ))
                .with_phase(crate::diagnostics::DiagnosticPhase::Lex)
                .with_code("LOCALE001")
                .with_args(vec![crate::diagnostics::DiagnosticArg::new(
                    "issue",
                    "library_locale_pack_load_failed",
                )])
                .with_file(name.to_owned()),
            );
            return CheckDiagnosticsResult { diagnostics, success: false };
        }
        // D7.7: default-named imports bind their targets' `regio` names.
        crate::program::apply_default_import_names(&mut program, &mut interner, &contract.default_import_names);
        pass_config.namespace_exports = contract.namespace_exports;
        pass_config.namespace_seams = contract.namespace_seams;
        pass_config.namespace_declarations = contract.namespace_declarations;
        pass_config.file_interfaces = contract.file_interfaces;
    }
    pass_config.locale_pack.clone_from(&resolved_pack);
    let semantic_result =
        crate::semantic::analyze_with_cli(&program, &pass_config, &mut interner, cli_analysis.program.as_ref());

    let report =
        collect_semantic_check_diagnostics(&mut diagnostics, name, body, &semantic_result.diagnostics, permissive);

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
    locale_candidates: &mut Vec<crate::diagnostics::Diagnostic>,
    name: &str,
    source: &str,
    lex_result: &crate::lexer::LexResult,
) {
    locale_candidates.extend(
        lex_result
            .locale_suggestions
            .iter()
            .map(|suggestion| crate::diagnostics::Diagnostic::from_locale_suggestion(name, source, suggestion)),
    );
    diagnostics.extend(
        lex_result
            .mixed_script_advisories
            .iter()
            .map(|advisory| crate::diagnostics::Diagnostic::from_mixed_script_advisory(name, source, advisory)),
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

    CheckErrorTally { fatal_errors, downgraded }
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
