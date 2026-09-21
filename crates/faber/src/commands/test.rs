//! `faber test` — interpret proba cases on the MIR runner (not a Cargo harness).

use super::run_stats;
use super::test_stats::{self, TestStatsArtifact};
use crate::cli::{TestArgs, TestOutputFormat};
use faber_cli::input_shape::locale_without_package_error;
use faber_cli::package;
use proba::{
    CaseOutcome, CaseResult, ProbaCase, ProbaReport, TestSelection, format_summary,
    has_solum_cases, inventory_cases, is_harness_filter_match, run_proba_on_validated,
    run_proba_on_validated_with_function_ids, selection_skip_reason, source_skip_reason,
};
use radix::module::driver::{Config, Session};
use radix::program::test_source_filter::TestSourceFilter;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Default)]
struct TestRunOptions {
    filters: Vec<String>,
    skips: Vec<String>,
    suites: Vec<String>,
    tags: Vec<String>,
    name: Option<String>,
    exact: bool,
    list: bool,
    fail_fast: bool,
    /// CLI `--test-timeout` wall-clock default (seconds) for cases without a
    /// per-test `timeout` / `temporis` modifier.
    default_timeout_secs: Option<u64>,
    /// Arm the interpreter diagnostic meter for each executed case.
    collect_stats: bool,
}

#[derive(Debug, Default)]
struct RunMetadata {
    durations_ms: HashMap<String, u128>,
}

/// Discover, select, and run proba cases exclusively on the MIR runner.
pub(super) fn cmd_test(args: &TestArgs) {
    crate::commands::validate_deny_codes(&args.deny);
    // Package discovery requires absolute inputs; absolutizing here (same seam
    // as the package build/check/run routes) keeps `.`, relative, and absolute
    // paths resolving to one package root (rdx-s05-1).
    let input_path = faber_cli::package::absolutize_path(&args.path);
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
        eprintln!("{}", radix::module::diagnostics::render_plain(&diagnostic));
        std::process::exit(1);
    }

    let mut filters = args.filter_flag.clone();
    if let Some(filter) = args.filter.clone() {
        filters.push(filter);
    }
    // --test-timeout 0 is rejected fail-closed at parse time (cli parser);
    // cmd_test never sees a zero default here.
    let options = TestRunOptions {
        filters,
        skips: args.skip.clone(),
        suites: args.suite.clone(),
        tags: args.tag.clone(),
        name: args.name.clone(),
        exact: args.exact,
        list: args.list,
        fail_fast: args.fail_fast,
        default_timeout_secs: args.test_timeout,
        collect_stats: args.stats || args.stats_file.is_some(),
    };

    // A named `.proba` argument is the source selection. `--include` on
    // the package root still selects by pattern; a file path does not
    // pull the rest of the package.
    let proba_filter = package::test_source_filter_for_input(
        &input_path,
        args.include.clone(),
        args.exclude.clone(),
    );

    // `--ignored` alone means “only source-skipped (omitte/futurum) cases” —
    // not yet supported as a positive suite; document via fail-closed message.
    if args.ignored {
        eprintln!(
            "error: --ignored is not supported on the MIR runner; use omitte/futurum cases with default run (they skip) or --include-ignored to list them"
        );
        std::process::exit(2);
    }

    let stats_requested = args.stats || args.stats_file.is_some();
    if stats_requested && args.list {
        eprintln!(
            "error: test statistics require executing cases; --list does not run the interpreter"
        );
        std::process::exit(1);
    }

    let warn_policy = radix::module::driver::WarnPolicy {
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

    let started = Instant::now();
    let (report, unmatched_include, metadata) =
        match package::test_input_route(&input_path, args.no_package) {
            package::TestInputRoute::SingleFile => {
                let (report, metadata) = run_single_file_with_options(
                    &config,
                    &input_path,
                    &options,
                    args.include_ignored,
                    args.focused,
                );
                (report, false, metadata)
            }
            package::TestInputRoute::Package => run_package_with_options(
                &config,
                &input_path,
                &options,
                proba_filter.as_ref(),
                args.include_ignored,
                args.focused,
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
            print_report(&report, args.nocapture, args.format, &metadata, args.list);
            emit_test_stats_if_requested(args, &input_path, &report, &metadata, started, code);
        } else {
            eprintln!("error: {detail}");
        }
        std::process::exit(code);
    }

    print_report(&report, args.nocapture, args.format, &metadata, args.list);
    let code = i32::from(!report.success());
    emit_test_stats_if_requested(args, &input_path, &report, &metadata, started, code);
    std::process::exit(code);
}

fn emit_test_stats_if_requested(
    args: &TestArgs,
    input: &Path,
    report: &ProbaReport,
    metadata: &RunMetadata,
    started: Instant,
    exit: i32,
) {
    if !args.stats && args.stats_file.is_none() {
        return;
    }
    let package = match run_stats::package_metadata(input) {
        Ok(package) => package,
        Err(_) => run_stats::PackageMetadata {
            name: input
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("package")
                .to_owned(),
            entry: input
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("test")
                .to_owned(),
        },
    };
    let artifact = TestStatsArtifact::new(
        package,
        report,
        &metadata.durations_ms,
        elapsed_wall_ms(started),
        exit,
        result_key,
    );
    if let Err(error) = test_stats::emit(&artifact, args.stats, args.stats_file.as_ref()) {
        eprintln!("error: could not write test statistics: {error}");
        std::process::exit(1);
    }
}

fn elapsed_wall_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
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
    if allow_no_tests { 0 } else { 1 }
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
            .any(|path| radix::program::source_files::is_proba_source_path(path.as_ref()))
}

fn run_single_file_with_options(
    config: &Config,
    path: &Path,
    options: &TestRunOptions,
    include_ignored: bool,
    focused: bool,
) -> (ProbaReport, RunMetadata) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error: could not read {}: {err}", path.display());
            std::process::exit(1);
        }
    };
    let session = Session::new(config.clone());
    let name = path.display().to_string();
    match run_source_with_options(&session, &name, &source, options, include_ignored, focused) {
        Ok(result) => result,
        Err(error) => {
            print_run_error(&error);
            std::process::exit(1);
        }
    }
}

fn run_source_with_options(
    session: &Session,
    name: &str,
    source: &str,
    options: &TestRunOptions,
    include_ignored: bool,
    focused: bool,
) -> Result<(ProbaReport, RunMetadata), radix::module::mir::RunSourceError> {
    let mut analysis = radix::program::analyze_prepared_program_name(session, name, source)
        .map_err(radix::module::mir::RunSourceError::Frontend)?;
    let all_cases = inventory_cases(&analysis.hir, &analysis.interner);
    let focused = focused || has_solum_cases(&all_cases);
    let available = if focused {
        focused_cases(&all_cases)
    } else {
        all_cases
    };
    let cases: Vec<_> = available
        .iter()
        .filter(|case| case_matches_options(Path::new(name), case, options))
        .cloned()
        .collect();
    if options.list {
        return Ok((list_report(Path::new(name), &cases), RunMetadata::default()));
    }
    if cases.is_empty() {
        return Ok((ProbaReport::default(), RunMetadata::default()));
    }

    let lowered = radix::module::mir::lower_analyzed_unit(&mut analysis, Default::default())
        .map_err(radix::module::mir::RunSourceError::Mir)?;
    let mut report = ProbaReport::default();
    let mut metadata = RunMetadata::default();
    for case in cases {
        let started = Instant::now();
        let mut one = run_proba_on_validated(
            &lowered.validated,
            std::slice::from_ref(&case),
            &TestSelection::default(),
            None,
            false,
            include_ignored,
            options.default_timeout_secs,
            options.collect_stats,
        );
        let elapsed = started.elapsed().as_millis();
        for result in &mut one.results {
            result.unit_path = Path::new(name).to_path_buf();
            metadata.durations_ms.insert(result_key(result), elapsed);
        }
        let stop = one.results.iter().any(result_is_failure);
        report.results.extend(one.results);
        if options.fail_fast && stop {
            break;
        }
    }
    Ok((report, metadata))
}

fn is_solum_case(case: &ProbaCase) -> bool {
    case.modifiers
        .iter()
        .any(|modifier| matches!(modifier, proba::ProbaModifierView::Solum))
}

fn focused_cases(cases: &[ProbaCase]) -> Vec<ProbaCase> {
    cases
        .iter()
        .filter(|case| is_solum_case(case))
        .cloned()
        .collect()
}

fn append_focus_skips(
    report: &mut ProbaReport,
    unit_path: &Path,
    cases: &[ProbaCase],
    focused: bool,
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
) {
    if !focused || !include_ignored {
        return;
    }
    for case in cases {
        if is_solum_case(case) || !is_harness_filter_match(case, harness_filter, exact) {
            continue;
        }
        let Some(reason) = selection_skip_reason(case, selection, true) else {
            continue;
        };
        report.results.push(CaseResult {
            unit_path: unit_path.to_path_buf(),
            name: case.name.clone(),
            suite_path: case.suite_path.clone(),
            outcome: CaseOutcome::Skipped { reason },
            stdout: Vec::new(),
            stderr: Vec::new(),
            stats: None,
        });
    }
}

fn run_package_with_options(
    config: &Config,
    input: &Path,
    options: &TestRunOptions,
    proba_filter: Option<&TestSourceFilter>,
    include_ignored: bool,
    focused: bool,
) -> (ProbaReport, bool, RunMetadata) {
    let package = match package::analyze_package_for_tests(config, input, proba_filter) {
        Ok(package) => package,
        Err(diagnostics) => fail_package_analysis(&diagnostics),
    };
    if package
        .diagnostics
        .iter()
        .any(radix::module::diagnostics::Diagnostic::is_error)
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
        return (ProbaReport::default(), true, RunMetadata::default());
    }

    let mut test_units = Vec::new();
    for unit in package.nodes.values() {
        let cases = inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
        if !cases.is_empty() {
            test_units.push((unit.path.clone(), cases));
        }
    }
    let focused = focused || test_units.iter().any(|(_, cases)| has_solum_cases(cases));
    let metadata = RunMetadata::default();
    if options.list {
        let mut report = ProbaReport::default();
        for (unit_path, all_cases) in test_units {
            let available = if focused {
                focused_cases(&all_cases)
            } else {
                all_cases
            };
            let selected: Vec<_> = available
                .iter()
                .filter(|case| case_matches_options(&unit_path, case, options))
                .cloned()
                .collect();
            report
                .results
                .extend(list_report(&unit_path, &selected).results);
        }
        return (report, false, metadata);
    }

    let mut metadata = metadata;
    let mut combined = ProbaReport::default();
    for (unit_path, cases) in test_units {
        match run_package_test_unit_with_options(
            config,
            input,
            proba_filter,
            &unit_path,
            options,
            include_ignored,
            focused,
            &mut metadata,
        ) {
            Ok(report) => {
                let stop = report.results.iter().any(result_is_failure);
                combined.results.extend(report.results);
                if options.fail_fast && stop {
                    break;
                }
            }
            Err(diagnostics) => {
                super::eprint_compile_diagnostics(&diagnostics);
                eprintln!(
                    "error: package MIR test link failed for {}",
                    unit_path.display()
                );
                let report = blocked_unit_report_with_options(
                    &unit_path,
                    &cases,
                    options,
                    focused,
                    &unit_link_block_reason(&diagnostics),
                );
                let stop = report.results.iter().any(result_is_failure);
                combined.results.extend(report.results);
                if options.fail_fast && stop {
                    break;
                }
            }
        }
    }
    (combined, false, metadata)
}

fn run_package(
    config: &Config,
    input: &Path,
    selection: &TestSelection,
    proba_filter: Option<&TestSourceFilter>,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    focused: bool,
) -> (ProbaReport, bool) {
    let package = match package::analyze_package_for_tests(config, input, proba_filter) {
        Ok(package) => package,
        Err(diagnostics) => fail_package_analysis(&diagnostics),
    };

    if package
        .diagnostics
        .iter()
        .any(radix::module::diagnostics::Diagnostic::is_error)
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

    // A `solum` marker anywhere in the package makes focus a package-wide
    // decision. The per-unit runner still owns linking, but every unit receives
    // the same focused case set instead of deciding focus from local inventory.
    let focused = focused || test_units.iter().any(|(_, cases)| has_solum_cases(cases));

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
                focused,
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
                            focused,
                            &unit_link_block_reason(&diagnostics),
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
fn run_package_test_unit_with_options(
    config: &Config,
    input: &Path,
    proba_filter: Option<&TestSourceFilter>,
    unit_path: &Path,
    options: &TestRunOptions,
    include_ignored: bool,
    focused: bool,
    metadata: &mut RunMetadata,
) -> Result<ProbaReport, Vec<radix::module::diagnostics::Diagnostic>> {
    let mut package = package::analyze_package_for_tests(config, input, proba_filter)?;
    if package
        .diagnostics
        .iter()
        .any(radix::module::diagnostics::Diagnostic::is_error)
    {
        return Err(package.diagnostics);
    }

    let Some((root_id, _)) = package.node_by_path(unit_path) else {
        return Err(vec![
            radix::module::diagnostics::Diagnostic::error(format!(
                "test unit disappeared during package re-analysis: {}",
                unit_path.display()
            ))
            .with_file(unit_path.display().to_string()),
        ]);
    };
    let root_id = root_id.clone();
    let included = package.imports.local_closure(std::iter::once(&root_id));
    package.nodes.retain(|id, _| included.contains(id));
    package.entry = Some(root_id.clone());
    package.roots = vec![root_id.clone()];

    let Some(unit) = package.nodes.get(&root_id) else {
        return Err(vec![
            radix::module::diagnostics::Diagnostic::error(format!(
                "test unit was not retained in its local package closure: {}",
                unit_path.display()
            ))
            .with_file(unit_path.display().to_string()),
        ]);
    };
    let all_cases = inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
    let available = if focused {
        focused_cases(&all_cases)
    } else {
        all_cases.clone()
    };
    let cases: Vec<_> = available
        .iter()
        .filter(|case| case_matches_options(unit_path, case, options))
        .cloned()
        .collect();
    if cases.is_empty() {
        return Ok(ProbaReport::default());
    }

    let functions = cases
        .iter()
        .map(|case| (unit.path.clone(), case.def_id))
        .collect();
    let scope = radix::program::mir::TestPackageScope::for_program(&package, functions);

    package::mir::with_linked_test_package_mir(
        package,
        &scope,
        config,
        |validated, function_ids| {
            let mut report = ProbaReport::default();
            for case in &cases {
                let started = Instant::now();
                let mut one = run_proba_on_validated_with_function_ids(
                    validated,
                    unit_path,
                    std::slice::from_ref(case),
                    function_ids,
                    &TestSelection::default(),
                    None,
                    false,
                    include_ignored,
                    options.default_timeout_secs,
                    options.collect_stats,
                );
                let elapsed = started.elapsed().as_millis();
                for result in &mut one.results {
                    result.unit_path = unit_path.to_path_buf();
                    metadata.durations_ms.insert(result_key(result), elapsed);
                }
                let stop = one.results.iter().any(result_is_failure);
                report.results.extend(one.results);
                if options.fail_fast && stop {
                    break;
                }
            }
            report
        },
    )
}

fn run_package_test_unit(
    config: &Config,
    input: &Path,
    proba_filter: Option<&TestSourceFilter>,
    unit_path: &Path,
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    focused: bool,
) -> Result<ProbaReport, Vec<radix::module::diagnostics::Diagnostic>> {
    let mut package = package::analyze_package_for_tests(config, input, proba_filter)?;
    if package
        .diagnostics
        .iter()
        .any(radix::module::diagnostics::Diagnostic::is_error)
    {
        return Err(package.diagnostics);
    }

    let Some((root_id, _)) = package.node_by_path(unit_path) else {
        return Err(vec![
            radix::module::diagnostics::Diagnostic::error(format!(
                "test unit disappeared during package re-analysis: {}",
                unit_path.display()
            ))
            .with_file(unit_path.display().to_string()),
        ]);
    };
    let root_id = root_id.clone();
    let included = package.imports.local_closure(std::iter::once(&root_id));
    package.nodes.retain(|id, _| included.contains(id));
    package.entry = Some(root_id.clone());
    package.roots = vec![root_id.clone()];

    let Some(unit) = package.nodes.get(&root_id) else {
        return Err(vec![
            radix::module::diagnostics::Diagnostic::error(format!(
                "test unit was not retained in its local package closure: {}",
                unit_path.display()
            ))
            .with_file(unit_path.display().to_string()),
        ]);
    };
    let all_cases = inventory_cases(&unit.analysis.hir, &unit.analysis.interner);
    if all_cases.is_empty() {
        return Ok(ProbaReport::default());
    }
    let cases = if focused {
        focused_cases(&all_cases)
    } else {
        all_cases.clone()
    };
    if cases.is_empty() {
        let mut report = ProbaReport::default();
        append_focus_skips(
            &mut report,
            unit_path,
            &all_cases,
            focused,
            selection,
            harness_filter,
            exact,
            include_ignored,
        );
        return Ok(report);
    }
    let functions = cases
        .iter()
        .map(|case| (unit.path.clone(), case.def_id))
        .collect();
    let scope = radix::program::mir::TestPackageScope::for_program(&package, functions);

    package::mir::with_linked_test_package_mir(
        package,
        &scope,
        config,
        |validated, function_ids| {
            let mut report = run_proba_on_validated_with_function_ids(
                validated,
                unit_path,
                &cases,
                function_ids,
                selection,
                harness_filter,
                exact,
                include_ignored,
                None,
                false,
            );
            append_focus_skips(
                &mut report,
                unit_path,
                &all_cases,
                focused,
                selection,
                harness_filter,
                exact,
                include_ignored,
            );
            report
        },
    )
}

fn blocked_unit_report_with_options(
    unit_path: &Path,
    cases: &[ProbaCase],
    options: &TestRunOptions,
    focused: bool,
    reason: &str,
) -> ProbaReport {
    let available = if focused {
        focused_cases(cases)
    } else {
        cases.to_vec()
    };
    let mut report = ProbaReport::default();
    for case in available {
        if !case_matches_options(unit_path, &case, options) {
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
                stats: None,
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
            stats: None,
        });
    }
    report
}

fn blocked_unit_report(
    unit_path: &Path,
    cases: Vec<ProbaCase>,
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    focused: bool,
    reason: &str,
) -> ProbaReport {
    let has_solum = focused || has_solum_cases(&cases);
    let mut report = ProbaReport::default();
    for case in cases {
        if !is_harness_filter_match(&case, harness_filter, exact) {
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
                    stats: None,
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
                stats: None,
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
            stats: None,
        });
    }
    report
}

fn case_matches_options(unit_path: &Path, case: &ProbaCase, options: &TestRunOptions) -> bool {
    if let Some(name) = options.name.as_deref() {
        if case.name != name {
            return false;
        }
    }
    if !options.suites.is_empty() && !options.suites.iter().any(|suite| suite == &case.suite_path) {
        return false;
    }
    if !options.tags.is_empty()
        && !options.tags.iter().any(|tag| {
            case.modifiers.iter().any(
                |modifier| matches!(modifier, proba::ProbaModifierView::Tag(value) if value == tag),
            )
        })
    {
        return false;
    }

    let filters_match = options.filters.is_empty()
        || options
            .filters
            .iter()
            .any(|filter| case_query_matches(filter, unit_path, case, options.exact));
    if !filters_match {
        return false;
    }
    !options
        .skips
        .iter()
        .any(|skip| case_query_matches(skip, unit_path, case, options.exact))
}

fn case_query_matches(query: &str, unit_path: &Path, case: &ProbaCase, exact: bool) -> bool {
    if let Some((file, case_query)) = query.split_once("::") {
        if file.is_empty() || case_query.is_empty() {
            return false;
        }
        return path_query_matches(file, unit_path)
            && (case_display_path(case) == case_query || case.name == case_query);
    }
    let display = case_display_path(case);
    if exact {
        display == query || case.name == query
    } else {
        display.contains(query) || case.name.contains(query)
    }
}

fn case_display_path(case: &ProbaCase) -> String {
    if case.suite_path.is_empty() {
        case.name.clone()
    } else {
        format!("{}/{}", case.suite_path, case.name)
    }
}

fn path_query_matches(query: &str, path: &Path) -> bool {
    let query = query.replace('\\', "/");
    let path = path.to_string_lossy().replace('\\', "/");
    path == query || path.ends_with(&format!("/{query}"))
}

fn result_key(result: &CaseResult) -> String {
    format!("{}::{}", result.unit_path.display(), result.display_path())
}

fn result_is_failure(result: &CaseResult) -> bool {
    matches!(
        result.outcome,
        CaseOutcome::Failed { .. } | CaseOutcome::Blocked { .. }
    )
}

fn list_report(unit_path: &Path, cases: &[ProbaCase]) -> ProbaReport {
    ProbaReport {
        results: cases
            .iter()
            .map(|case| CaseResult {
                unit_path: unit_path.to_path_buf(),
                name: case.name.clone(),
                suite_path: case.suite_path.clone(),
                outcome: CaseOutcome::Skipped {
                    reason: "listed; not executed".to_owned(),
                },
                stdout: Vec::new(),
                stderr: Vec::new(),
                stats: None,
            })
            .collect(),
    }
}

fn print_report(
    report: &ProbaReport,
    nocapture: bool,
    format: TestOutputFormat,
    metadata: &RunMetadata,
    list: bool,
) {
    if format == TestOutputFormat::Json {
        print_json_report(report, metadata, list);
        return;
    }
    for result in &report.results {
        let path = result_key(result);
        let duration = metadata.durations_ms.get(&path).copied().unwrap_or(0);
        if list {
            println!("{path}");
            continue;
        }
        match &result.outcome {
            CaseOutcome::Passed => {
                println!("ok   {path} ({duration} ms)");
                if nocapture {
                    replay_output(result);
                }
            }
            CaseOutcome::Failed { message } => {
                // Always show failure detail so CI can name the case.
                println!("FAIL {path} ({duration} ms)");
                replay_output(result);
                eprintln!("  {message}");
            }
            CaseOutcome::Blocked { reason } => {
                println!("block {path} ({duration} ms)");
                eprintln!("  {reason}");
            }
            CaseOutcome::Skipped { reason } => {
                println!("skip {path} ({reason}; {duration} ms)");
            }
        }
    }
    if !list {
        println!("{}", format_summary(report));
    }
}

/// Unit-link failure reason: name the blocking MIR gap from diagnostics when
/// present, otherwise the generic link/lower fallback.
fn unit_link_block_reason(diagnostics: &[radix::module::diagnostics::Diagnostic]) -> String {
    blocking_gap_from_diagnostics(diagnostics)
        .unwrap_or_else(|| "unit failed to link/lower for runner proba execution".to_owned())
}

fn blocking_gap_from_diagnostics(
    diagnostics: &[radix::module::diagnostics::Diagnostic],
) -> Option<String> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.message.as_str())
        .find(|message| is_named_mir_gap(message))
        .map(str::to_owned)
}

fn is_named_mir_gap(message: &str) -> bool {
    message.contains("unsupported MIR lowering")
        || message.contains("faberscript unsupported:")
        || message.contains("missing type information for MIR lowering")
        || message.contains("invalid MIR:")
}

/// Stable MIR-gap identity for census grouping. Strips the unit-path suffix
/// `blocked_unit_report*` appends and known diagnostic prefixes.
fn blocking_gap(reason: &str) -> String {
    classify_blocking_gap(strip_trailing_unit_path(reason))
}

fn strip_trailing_unit_path(reason: &str) -> &str {
    if let Some(start) = reason.rfind(" (") {
        if reason.ends_with(')') {
            return reason[..start].trim();
        }
    }
    reason.trim()
}

fn classify_blocking_gap(reason: &str) -> String {
    if let Some(rest) = reason.strip_prefix("unsupported MIR lowering: ") {
        return rest.trim().to_owned();
    }
    if let Some(rest) = reason.strip_prefix("MIR lowering failed: unsupported MIR lowering: ") {
        return rest.trim().to_owned();
    }
    if let Some(rest) = reason.strip_prefix("invalid MIR: ") {
        return rest.trim().to_owned();
    }
    if reason.starts_with("faberscript unsupported:") {
        return reason.trim().to_owned();
    }
    if reason.contains("missing type information for MIR lowering") {
        return "missing type information for MIR lowering".to_owned();
    }
    if reason.contains("no lowered MIR function") {
        return "no lowered MIR function".to_owned();
    }
    if reason.contains("unit failed to link/lower") {
        return "unit failed to link/lower".to_owned();
    }
    reason.trim().to_owned()
}

fn json_report_value(
    report: &ProbaReport,
    metadata: &RunMetadata,
    list: bool,
) -> serde_json::Value {
    let cases: Vec<_> = report
        .results
        .iter()
        .map(|result| {
            let (status, reason, gap) = match &result.outcome {
                CaseOutcome::Passed => ("passed", None, None),
                CaseOutcome::Failed { message } => ("failed", Some(message.as_str()), None),
                CaseOutcome::Blocked { reason } => {
                    ("blocked", Some(reason.as_str()), Some(blocking_gap(reason)))
                }
                CaseOutcome::Skipped { reason } => ("skipped", Some(reason.as_str()), None),
            };
            serde_json::json!({
                "path": result_key(result),
                "unit": result.unit_path,
                "name": result.name,
                "suite": result.suite_path,
                "status": if list { "listed" } else { status },
                "duration_ms": metadata.durations_ms.get(&result_key(result)).copied().unwrap_or(0),
                "reason": reason,
                "gap": if list { None } else { gap },
            })
        })
        .collect();
    serde_json::json!({
        "success": report.success(),
        "passed": report.passed(),
        "failed": report.failed(),
        "blocked": report.blocked(),
        "skipped": report.skipped(),
        "cases": cases,
        "census": json_census_value(report, list),
    })
}

fn json_census_value(report: &ProbaReport, list: bool) -> serde_json::Value {
    if list {
        return serde_json::json!({
            "blocked": [],
            "gaps": [],
        });
    }
    let blocked: Vec<_> = report
        .results
        .iter()
        .filter_map(|result| match &result.outcome {
            CaseOutcome::Blocked { reason } => Some(serde_json::json!({
                "path": result_key(result),
                "unit": result.unit_path,
                "name": result.name,
                "suite": result.suite_path,
                "gap": blocking_gap(reason),
                "reason": reason,
            })),
            _ => None,
        })
        .collect();
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &blocked {
        let Some(gap) = item.get("gap").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(path) = item.get("path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        grouped
            .entry(gap.to_owned())
            .or_default()
            .push(path.to_owned());
    }
    let gaps: Vec<_> = grouped
        .into_iter()
        .map(|(gap, mut cases)| {
            cases.sort();
            serde_json::json!({
                "gap": gap,
                "count": cases.len(),
                "cases": cases,
            })
        })
        .collect();
    serde_json::json!({
        "blocked": blocked,
        "gaps": gaps,
    })
}

fn print_json_report(report: &ProbaReport, metadata: &RunMetadata, list: bool) {
    match serde_json::to_string_pretty(&json_report_value(report, metadata, list)) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("failed to serialize test report: {error}");
            std::process::exit(70);
        }
    }
}

fn replay_output(result: &CaseResult) {
    for line in &result.stdout {
        println!("{line}");
    }
    for line in &result.stderr {
        eprintln!("{line}");
    }
}

fn fail_package_analysis(diagnostics: &[radix::module::diagnostics::Diagnostic]) -> ! {
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
    diagnostics: &[radix::module::diagnostics::Diagnostic],
) -> String {
    let mut warning_counts = HashMap::<&str, usize>::new();
    for diag in diagnostics {
        if diag.severity == radix::module::diagnostics::Severity::Warning {
            if let Some(code) = diag.code {
                *warning_counts.entry(code).or_insert(0) += 1;
            }
        }
    }

    let mut emitted_warning_codes = HashSet::<&str>::new();
    let mut lines = Vec::new();
    for diag in diagnostics {
        if diag.severity == radix::module::diagnostics::Severity::Warning {
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
        lines.push(radix::module::diagnostics::render_plain(diag));
    }
    lines.join("\n")
}

fn print_run_error(error: &radix::module::mir::RunSourceError) {
    match error {
        radix::module::mir::RunSourceError::Frontend(diagnostics) => {
            for diag in diagnostics {
                if diag.is_error() {
                    eprintln!("error: {}", diag.message);
                } else {
                    eprintln!("warning: {}", diag.message);
                }
            }
        }
        radix::module::mir::RunSourceError::Mir(errors) => {
            for error in errors {
                eprintln!("error: {}", error.message);
            }
        }
        radix::module::mir::RunSourceError::Stepper(errors) => {
            for error in errors {
                eprintln!("error: {}", error.message);
            }
        }
    }
}

#[cfg(test)]
#[path = "test_test.rs"]
mod tests;

#[cfg(test)]
#[path = "test_selection_test.rs"]
mod test_selection_tests;
