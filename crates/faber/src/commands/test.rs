//! `faber test` — interpret proba cases on the MIR runner (not a Cargo harness).

use crate::cli::TestArgs;
use crate::input_shape::locale_without_package_error;
use crate::package;
use proba::{
    format_summary, harness_filter_matches, has_solum_cases, inventory_cases,
    run_proba_on_validated_with_function_ids, run_proba_source, selection_skip_reason,
    source_skip_reason, CaseOutcome, CaseResult, ProbaCase, ProbaReport, TestSelection,
};
use radix_module::driver::{Config, Session};
use radix_program::test_source_filter::TestSourceFilter;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Discover, select, and run proba cases exclusively on the MIR runner.
pub(super) fn cmd_test(args: &TestArgs) {
    crate::commands::validate_deny_codes(&args.deny);
    // Package discovery requires absolute inputs; absolutizing here (same seam
    // as the package build/check/run routes) keeps `.`, relative, and absolute
    // paths resolving to one package root (rdx-s05-1).
    let input_path = crate::package::absolutize_path(&args.path);
    if let Some(message) = locale_without_package_error(
        args.locale.as_deref(),
        &[args.path.display().to_string()],
        false,
    ) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
    // Keep an explicit reader-locale missing path on the stable package
    // diagnostic identity instead of leaking the raw discovery I/O error.
    if args
        .locale
        .as_deref()
        .is_some_and(|locale| !locale.trim().is_empty())
        && !args.path.exists()
    {
        let diagnostic = package::missing_package_path_diagnostic(&args.path);
        eprintln!("{}", radix_module::diagnostics::render_plain(&diagnostic));
        std::process::exit(1);
    }

    let selection = TestSelection {
        name: args.name.clone(),
        suite: args.suite.clone(),
        tag: args.tag.clone(),
    };

    let proba_filter = if args.include.is_empty() && args.exclude.is_empty() {
        None
    } else {
        Some(TestSourceFilter {
            include: args.include.clone(),
            exclude: args.exclude.clone(),
        })
    };

    // Prefer explicit `--filter`; fall back to positional FILTER.
    let harness_filter = args.filter_flag.as_deref().or(args.filter.as_deref());

    // `--ignored` alone means “only source-skipped (omitte/futurum) cases” —
    // not yet supported as a positive suite; document via fail-closed message.
    if args.ignored {
        eprintln!(
            "error: --ignored is not supported on the MIR runner; use omitte/futurum cases with default run (they skip) or --include-ignored to list them"
        );
        std::process::exit(2);
    }

    let warn_policy = radix_module::driver::WarnPolicy {
        deny_all_warnings: args.deny_warnings,
        deny_codes: args.deny.clone(),
    };

    // ONE-EFFECTIVE-TARGET SEAM (TARGETLANE001 executed-lane routing;
    // authoritative decision record `radix/docs/factory/
    // executed-lane-target-decision.md`): the runner analysis target resolves
    // to a MIR-backed target — the manifest `[build] target` when present,
    // else the executed-lane default `fmir` — so air-lane packages (e.g.
    // gradus) pass the lane policy at analysis instead of tripping
    // `lane_requires_mir_backed_target` under the old HIR-direct placeholder.
    // The runner never emits product targets; the target only selects the
    // analysis lane-policy posture.
    let analysis_target = package::executed_lane_analysis_target(&Config::default(), &input_path);
    let config = match package::config_with_locale(
        analysis_target,
        &input_path,
        args.locale.as_deref(),
        args.diagnostics_locale.as_deref(),
    ) {
        Ok((config, _locale_pack)) => config.with_warn_policy(warn_policy),
        Err(diag) => {
            eprintln!("error: {}", diag.message);
            std::process::exit(1);
        }
    };

    let (report, unmatched_include) = match package::test_input_route(&input_path, args.no_package)
    {
        package::TestInputRoute::SingleFile => (
            run_single_file(
                &config,
                &input_path,
                &selection,
                harness_filter,
                args.exact,
                args.include_ignored,
            ),
            false,
        ),
        package::TestInputRoute::Package => run_package(
            &config,
            &input_path,
            &selection,
            proba_filter.as_ref(),
            harness_filter,
            args.exact,
            args.include_ignored,
        ),
        package::TestInputRoute::ManifestLessDirectory => {
            eprintln!("error: {}", package::MANIFEST_LESS_TEST_MESSAGE);
            std::process::exit(1);
        }
    };

    if report.results.is_empty() {
        let detail = empty_run_detail(unmatched_include);
        let code = empty_run_exit_code(args.allow_no_tests);
        if code == 0 {
            eprintln!("warning: {detail}");
            print_report(&report, args.nocapture);
        } else {
            eprintln!("error: {detail}");
        }
        std::process::exit(code);
    }

    print_report(&report, args.nocapture);
    if report.success() {
        std::process::exit(0);
    }
    std::process::exit(1);
}

/// Zero selected cases after discovery/filtering. `--include` that matched no
/// `*.proba` sources is a distinct error so inline `.fab` tests cannot silently
/// widen the run (E7).
fn empty_run_detail(unmatched_include: bool) -> &'static str {
    if unmatched_include {
        "--include matched no *.proba sources"
    } else {
        "no proba cases discovered"
    }
}

/// Exit code for an empty report: nonzero unless `--allow-no-tests`.
fn empty_run_exit_code(allow_no_tests: bool) -> i32 {
    if allow_no_tests {
        0
    } else {
        1
    }
}

/// True when `--include` was given and no analyzed unit is a `*.proba` file.
///
/// Include filters only `*.proba` at load; product `.fab` units stay in the
/// graph. A no-match include must not run those leftover inline tests.
fn unmatched_include_filter<I, P>(include: &[String], unit_paths: I) -> bool
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    !include.is_empty()
        && !unit_paths
            .into_iter()
            .any(|path| radix_program::source_files::is_proba_source_path(path.as_ref()))
}

fn run_single_file(
    config: &Config,
    path: &Path,
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
) -> ProbaReport {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error: could not read {}: {err}", path.display());
            std::process::exit(1);
        }
    };
    let session = Session::new(config.clone());
    let name = path.display().to_string();
    match run_proba_source(
        &session,
        &name,
        &source,
        selection,
        harness_filter,
        exact,
        include_ignored,
    ) {
        Ok(report) => report,
        Err(error) => {
            print_run_error(&error);
            std::process::exit(1);
        }
    }
}

fn run_package(
    config: &Config,
    input: &Path,
    selection: &TestSelection,
    proba_filter: Option<&TestSourceFilter>,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
) -> (ProbaReport, bool) {
    let package = match radix_program::analyze::analyze_package_for_tests(
        config,
        input,
        proba_filter,
        &package::faber_analysis_seams(),
    ) {
        Ok(package) => package,
        Err(diagnostics) => fail_package_analysis(&diagnostics),
    };

    if package
        .diagnostics
        .iter()
        .any(radix_module::diagnostics::Diagnostic::is_error)
    {
        fail_package_analysis(&package.diagnostics);
    }

    let unmatched_include = proba_filter.is_some_and(|filter| {
        unmatched_include_filter(
            &filter.include,
            package.nodes.values().map(|unit| &unit.path),
        )
    });
    if unmatched_include {
        return (ProbaReport::default(), true);
    }

    let mut test_units = Vec::new();
    for unit in package.nodes.values() {
        let cases = inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
        if cases.is_empty() {
            continue;
        }
        test_units.push((unit.path.clone(), cases));
    }

    // Link and lower each test unit independently. A package-level link would
    // let one unsupported unit poison every otherwise runnable case (E10).
    // Re-analysis gives each unit a fresh owned package graph; the helper then
    // narrows that graph to the unit's local import closure before linking.
    let combined = if test_units.is_empty() {
        ProbaReport::default()
    } else {
        let mut combined = ProbaReport::default();
        for (unit_path, cases) in test_units {
            match run_package_test_unit(
                config,
                input,
                proba_filter,
                &unit_path,
                selection,
                harness_filter,
                exact,
                include_ignored,
            ) {
                Ok(report) => combined.results.extend(report.results),
                Err(diagnostics) => {
                    super::eprint_compile_diagnostics(&diagnostics);
                    eprintln!(
                        "error: package MIR test link failed for {}",
                        unit_path.display()
                    );
                    combined.results.extend(
                        blocked_unit_report(
                            &unit_path,
                            cases,
                            selection,
                            harness_filter,
                            exact,
                            include_ignored,
                            "unit failed to link/lower for runner proba execution",
                        )
                        .results,
                    );
                }
            }
        }
        combined
    };

    (combined, false)
}

/// Analyze, isolate, link, and run one test unit.
///
/// The package analyzer gives us the complete graph so local imports still
/// resolve. The MIR linker only receives this unit's local import closure, so
/// an unrelated unit's unsupported lowering cannot fail this run.
fn run_package_test_unit(
    config: &Config,
    input: &Path,
    proba_filter: Option<&TestSourceFilter>,
    unit_path: &Path,
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
) -> Result<ProbaReport, Vec<radix_module::diagnostics::Diagnostic>> {
    let mut package = radix_program::analyze::analyze_package_for_tests(
        config,
        input,
        proba_filter,
        &package::faber_analysis_seams(),
    )?;
    if package
        .diagnostics
        .iter()
        .any(radix_module::diagnostics::Diagnostic::is_error)
    {
        return Err(package.diagnostics);
    }

    let Some((root_id, _)) = package.node_by_path(unit_path) else {
        return Err(vec![radix_module::diagnostics::Diagnostic::error(format!(
            "test unit disappeared during package re-analysis: {}",
            unit_path.display()
        ))
        .with_file(unit_path.display().to_string())]);
    };
    let root_id = root_id.clone();
    let included = package.imports.local_closure(std::iter::once(&root_id));
    package.nodes.retain(|id, _| included.contains(id));
    package.entry = Some(root_id.clone());
    package.roots = vec![root_id.clone()];

    let Some(unit) = package.nodes.get(&root_id) else {
        return Err(vec![radix_module::diagnostics::Diagnostic::error(format!(
            "test unit was not retained in its local package closure: {}",
            unit_path.display()
        ))
        .with_file(unit_path.display().to_string())]);
    };
    let cases = inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
    if cases.is_empty() {
        return Ok(ProbaReport::default());
    }
    let functions = cases
        .iter()
        .map(|case| (unit.path.clone(), case.def_id))
        .collect();
    let scope = radix_program::mir::TestPackageScope::for_program(&package, functions);

    package::mir::with_linked_test_package_mir(
        package,
        &scope,
        config,
        |validated, function_ids| {
            run_proba_on_validated_with_function_ids(
                validated,
                unit_path,
                &cases,
                function_ids,
                selection,
                harness_filter,
                exact,
                include_ignored,
            )
        },
    )
}

fn blocked_unit_report(
    unit_path: &Path,
    cases: Vec<ProbaCase>,
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    reason: &str,
) -> ProbaReport {
    let has_solum = has_solum_cases(&cases);
    let mut report = ProbaReport::default();
    for case in cases {
        if !harness_filter_matches(&case, harness_filter, exact) {
            continue;
        }
        if let Some(skip_reason) = selection_skip_reason(&case, selection, has_solum) {
            if include_ignored {
                report.results.push(CaseResult {
                    unit_path: unit_path.to_path_buf(),
                    name: case.name,
                    suite_path: case.suite_path,
                    outcome: CaseOutcome::Skipped {
                        reason: skip_reason,
                    },
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                });
            }
            continue;
        }
        if let Some(skip_reason) = source_skip_reason(&case) {
            report.results.push(CaseResult {
                unit_path: unit_path.to_path_buf(),
                name: case.name,
                suite_path: case.suite_path,
                outcome: CaseOutcome::Skipped {
                    reason: skip_reason,
                },
                stdout: Vec::new(),
                stderr: Vec::new(),
            });
            continue;
        }
        report.results.push(CaseResult {
            unit_path: unit_path.to_path_buf(),
            name: case.name,
            suite_path: case.suite_path,
            outcome: CaseOutcome::Blocked {
                reason: format!("{reason} ({})", unit_path.display()),
            },
            stdout: Vec::new(),
            stderr: Vec::new(),
        });
    }
    report
}

fn print_report(report: &ProbaReport, nocapture: bool) {
    for result in &report.results {
        let path = format!("{}::{}", result.unit_path.display(), result.display_path());
        match &result.outcome {
            CaseOutcome::Passed => {
                println!("ok   {path}");
                if nocapture {
                    replay_output(result);
                }
            }
            CaseOutcome::Failed { message } => {
                // Always show failure detail so CI can name the case.
                println!("FAIL {path}");
                replay_output(result);
                eprintln!("  {message}");
            }
            CaseOutcome::Blocked { reason } => {
                println!("block {path}");
                eprintln!("  {reason}");
            }
            CaseOutcome::Skipped { reason } => {
                println!("skip {path} ({reason})");
            }
        }
    }
    println!("{}", format_summary(report));
}

fn replay_output(result: &CaseResult) {
    for line in &result.stdout {
        println!("{line}");
    }
    for line in &result.stderr {
        eprintln!("{line}");
    }
}

fn fail_package_analysis(diagnostics: &[radix_module::diagnostics::Diagnostic]) -> ! {
    let rendered = render_package_analysis_diagnostics(diagnostics);
    if !rendered.is_empty() {
        eprintln!("{rendered}");
    }
    eprintln!("error: package analysis failed");
    std::process::exit(1);
}

/// Package-route analysis diagnostics: errors use the same plain renderer as
/// `faber check`; repeated warning codes collapse to one counted line (E4).
fn render_package_analysis_diagnostics(
    diagnostics: &[radix_module::diagnostics::Diagnostic],
) -> String {
    let mut warning_counts = HashMap::<&str, usize>::new();
    for diag in diagnostics {
        if diag.severity == radix_module::diagnostics::Severity::Warning {
            if let Some(code) = diag.code {
                *warning_counts.entry(code).or_insert(0) += 1;
            }
        }
    }

    let mut emitted_warning_codes = HashSet::<&str>::new();
    let mut lines = Vec::new();
    for diag in diagnostics {
        if diag.severity == radix_module::diagnostics::Severity::Warning {
            if let Some(code) = diag.code {
                if warning_counts.get(code).copied().unwrap_or(0) > 1 {
                    if emitted_warning_codes.insert(code) {
                        let count = warning_counts[code];
                        lines.push(format!("warning[{code}]: {count} occurrences"));
                    }
                    continue;
                }
            }
        }
        lines.push(radix_module::diagnostics::render_plain(diag));
    }
    lines.join("\n")
}

fn print_run_error(error: &radix_module::mir::RunSourceError) {
    match error {
        radix_module::mir::RunSourceError::Frontend(diagnostics) => {
            for diag in diagnostics {
                if diag.is_error() {
                    eprintln!("error: {}", diag.message);
                } else {
                    eprintln!("warning: {}", diag.message);
                }
            }
        }
        radix_module::mir::RunSourceError::Mir(errors) => {
            for error in errors {
                eprintln!("error: {}", error.message);
            }
        }
        radix_module::mir::RunSourceError::Stepper(errors) => {
            for error in errors {
                eprintln!("error: {}", error.message);
            }
        }
    }
}

#[cfg(test)]
#[path = "test_test.rs"]
mod tests;
