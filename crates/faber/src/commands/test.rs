//! `faber test` — interpret proba cases on the MIR runner (not a Cargo harness).

use crate::cli::TestArgs;
use crate::input_shape::locale_without_package_error;
use crate::package;
use proba::{
    format_summary, inventory_cases, run_proba_on_validated_with_function_ids, run_proba_source,
    CaseOutcome, CaseResult, ProbaReport, TestSelection,
};
use radix_module::driver::{Config, Session};
use radix_program::test_source_filter::TestSourceFilter;
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

    let report = if input_is_single_source_file(&input_path) {
        run_single_file(
            &config,
            &input_path,
            &selection,
            harness_filter,
            args.exact,
            args.include_ignored,
        )
    } else {
        run_package(
            &config,
            &input_path,
            &selection,
            proba_filter.as_ref(),
            harness_filter,
            args.exact,
            args.include_ignored,
        )
    };

    print_report(&report, args.nocapture);
    if report.success() {
        std::process::exit(0);
    }
    std::process::exit(1);
}

fn input_is_single_source_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "fab" || ext == "proba")
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
) -> ProbaReport {
    let package = match radix_program::analyze::analyze_package_for_tests(
        config,
        input,
        proba_filter,
        &package::faber_analysis_seams(),
    ) {
        Ok(package) => package,
        Err(diagnostics) => {
            super::eprint_compile_diagnostics(&diagnostics);
            eprintln!("error: package analysis failed");
            std::process::exit(1);
        }
    };

    if package
        .diagnostics
        .iter()
        .any(radix_module::diagnostics::Diagnostic::is_error)
    {
        super::eprint_compile_diagnostics(&package.diagnostics);
        eprintln!("error: package analysis failed");
        std::process::exit(1);
    }

    let mut test_units = Vec::new();
    let mut functions = Vec::new();
    for unit in package.nodes.values() {
        let cases = inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
        if cases.is_empty() {
            continue;
        }
        functions.extend(cases.iter().map(|case| (unit.path.clone(), case.def_id)));
        test_units.push((unit.path.clone(), cases));
    }

    let combined = if test_units.is_empty() {
        ProbaReport::default()
    } else {
        let scope = radix_program::mir::TestPackageScope::for_program(&package, functions);
        let failed_units = test_units.clone();
        match package::mir::with_linked_test_package_mir(
            package,
            &scope,
            config,
            |validated, function_ids| {
                let mut combined = ProbaReport::default();
                for (unit_path, cases) in &test_units {
                    let report = run_proba_on_validated_with_function_ids(
                        validated,
                        unit_path,
                        cases,
                        function_ids,
                        selection,
                        harness_filter,
                        exact,
                        include_ignored,
                    );
                    combined.results.extend(report.results);
                }
                combined
            },
        ) {
            Ok(report) => report,
            Err(diagnostics) => {
                super::eprint_compile_diagnostics(&diagnostics);
                eprintln!("error: package MIR test link failed");
                let mut failed = ProbaReport::default();
                for (unit_path, cases) in failed_units {
                    for case in cases {
                        failed.results.push(CaseResult {
                            name: case.name,
                            suite_path: case.suite_path,
                            outcome: CaseOutcome::Failed {
                                message: format!(
                                    "package failed to link for runner proba execution ({})",
                                    unit_path.display()
                                ),
                            },
                        });
                    }
                }
                failed
            }
        }
    };

    if combined.results.is_empty() {
        eprintln!("warning: no proba cases discovered");
    }

    combined
}

fn print_report(report: &ProbaReport, _nocapture: bool) {
    for result in &report.results {
        let path = result.display_path();
        match &result.outcome {
            CaseOutcome::Passed => {
                println!("ok   {path}");
            }
            CaseOutcome::Failed { message } => {
                // Always show failure detail so CI can name the case.
                println!("FAIL {path}");
                eprintln!("  {message}");
            }
            CaseOutcome::Skipped { reason } => {
                println!("skip {path} ({reason})");
            }
        }
    }
    println!("{}", format_summary(report));
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
