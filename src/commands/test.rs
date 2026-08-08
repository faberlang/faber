//! `faber test` — interpret proba cases on the MIR stepper (not a Cargo harness).

use crate::cli::TestArgs;
use crate::input_shape::locale_without_package_error;
use crate::package::{self, TestSourceFilter};
use radix::driver::{Config, Session};
use radix::proba::{
    format_summary, inventory_cases, run_proba_on_analyzed, run_proba_source, CaseOutcome,
    ProbaReport, TestSelection,
};
use std::path::{Path, PathBuf};

/// Discover, select, and run proba cases exclusively on the MIR stepper.
pub(super) fn cmd_test(args: &TestArgs) {
    crate::commands::validate_deny_codes(&args.deny);
    let input_path = PathBuf::from(&args.path);
    if let Some(message) = locale_without_package_error(
        args.locale.as_deref(),
        &[args.path.display().to_string()],
        false,
    ) {
        eprintln!("error: {message}");
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
            "error: --ignored is not supported on the MIR stepper runner; use omitte/futurum cases with default run (they skip) or --include-ignored to list them"
        );
        std::process::exit(2);
    }

    let warn_policy = radix::driver::WarnPolicy {
        deny_all_warnings: args.deny_warnings,
        deny_codes: args.deny.clone(),
    };

    // Target-neutral analysis config. The stepper never emits product targets;
    // any Target variant is only used for session/policy construction.
    let config = match package::config_with_locale(
        radix::Target::HirTypeScript,
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
    let mut package = match package::analyze_package_for_tests(config, input, proba_filter) {
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
        .any(radix::diagnostics::Diagnostic::is_error)
    {
        super::eprint_compile_diagnostics(&package.diagnostics);
        eprintln!("error: package analysis failed");
        std::process::exit(1);
    }

    let mut combined = ProbaReport::default();
    // Per-unit lower + run for units that inventory at least one proba case.
    // Product-only units are skipped (no stepper lower) so packages with
    // unsupported product MIR still run self-contained test units. Cross-unit
    // imports that need package MIR linking fail closed — never Rust fallback.
    for unit in &mut package.units {
        let unit_path = unit.path.display().to_string();
        let cases = inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
        if cases.is_empty() {
            continue;
        }
        match run_proba_on_analyzed(
            &mut unit.analysis,
            selection,
            harness_filter,
            exact,
            include_ignored,
        ) {
            Ok(report) => {
                combined.results.extend(report.results);
            }
            Err(error) => {
                eprintln!("error: failed to lower/run tests in {unit_path}");
                print_run_error(&error);
                // Fail closed for units that own proba cases.
                for case in cases {
                    combined.results.push(radix::proba::CaseResult {
                        name: case.name,
                        suite_path: case.suite_path,
                        outcome: CaseOutcome::Failed {
                            message: format!(
                                "unit failed to lower for stepper proba execution ({unit_path})"
                            ),
                        },
                    });
                }
            }
        }
    }

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

fn print_run_error(error: &radix::mir::RunSourceError) {
    match error {
        radix::mir::RunSourceError::Frontend(diagnostics) => {
            for diag in diagnostics {
                if diag.is_error() {
                    eprintln!("error: {}", diag.message);
                } else {
                    eprintln!("warning: {}", diag.message);
                }
            }
        }
        radix::mir::RunSourceError::Mir(errors) => {
            for error in errors {
                eprintln!("error: {}", error.message);
            }
        }
        radix::mir::RunSourceError::Stepper(errors) => {
            for error in errors {
                eprintln!("error: {}", error.message);
            }
        }
    }
}
