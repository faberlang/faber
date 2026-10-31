//! Target-neutral proba inventory and MIR-runner case runner.
//!
//! `faber test` executes Faber `proba` / `probandum` cases on the MIR runner.
//! Selection policy and case results live here so language tests do not depend
//! on a codegen backend or Cargo harness.

#![cfg(feature = "runner")]

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

use radix::hir::{DefId, HirItemKind, HirModule, HirTestMetadata, HirTestModifier};
use radix::lexer::Interner;
use radix::mir::{
    BufferHost, KernelLowering, MirFunctionId, MirLoweringOptions, RunSourceError, RunnerError,
    RunnerKernelPolicy, RunnerStats, ValidatedMir, lower_analyzed_unit, reset_execution_state,
    run_function_with_limits_and_kernel_policy,
    run_function_with_stats_and_limits_and_kernel_policy,
};
use radix::module::{AnalyzedModule, Session};
use radix::program::analyze_prepared_program_name;
use radix_mir_runner::kernel_dispatch::ResidencyHook;
use radix_mir_runner::{
    RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED, RUNNER_ISSUE_WALL_CLOCK_TIMEOUT,
    run_function_with_limits_and_kernel_dispatcher,
};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How a proba case executes its `@ kernel` calls.
#[derive(Clone, Copy)]
pub enum CaseKernels<'a> {
    /// The MIR runner's own policy: refuse kernels, or execute host-form
    /// kernels in sequence (the `reference` oracle route).
    Runner(RunnerKernelPolicy),
    /// Hand every kernel call to a device dispatcher (the `cpu` device
    /// session route). The opener is called once per execution, because a
    /// dispatcher tears its session down when the program ends; its error is
    /// the case's blocked reason.
    Dispatch(&'a dyn Fn() -> Result<ResidencyHook, String>),
}

/// User-facing test filter requested by the package or CLI layer.
///
/// Selection is language-level (proba name, suite path, tag) and independent of
/// any codegen backend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestSelection {
    /// Exact test name to keep, if one was requested.
    pub name: Option<String>,

    /// Slash-joined suite path to keep, if one was requested.
    pub suite: Option<String>,

    /// Metadata tag to keep, if one was requested.
    pub tag: Option<String>,
}

/// One inventoriable proba case from HIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbaCase {
    /// HIR definition id of the synthetic test function.
    pub def_id: DefId,
    /// Display name (`proba "…"`).
    pub name: String,
    /// Nested `probandum` path joined with `/` (empty when top-level).
    pub suite_path: String,
    /// Source span of the case.
    pub span: radix::lexer::Span,
    /// Skip / schedule modifiers preserved from source.
    pub modifiers: Vec<ProbaModifierView>,
}

/// Resolved test-modifier view for selection and skip reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbaModifierView {
    /// Strict expected-failure marker; a captured body failure satisfies it.
    Erratur,

    Omitte(String),
    Futurum(String),
    Solum,
    Tag(String),
    Temporis(i64),
    Metior,
    Repete(i64),
    Fragilis(i64),
    SolumIn(String),
}

/// Outcome of one case after selection and (optional) execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseOutcome {
    Passed,
    Failed {
        message: String,
    },
    /// Runner cannot execute the case (unsupported lowering, missing MIR
    /// function, unit link failure). Distinct from [`Self::Failed`]: the
    /// assertion never ran.
    Blocked {
        reason: String,
    },
    Skipped {
        reason: String,
    },
}

/// Result for one inventoriable case in report order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// Source unit containing the case.
    pub unit_path: PathBuf,
    pub name: String,
    pub suite_path: String,
    pub outcome: CaseOutcome,
    /// Lines written to stdout while the case ran.
    pub stdout: Vec<String>,
    /// Lines written to stderr while the case ran.
    pub stderr: Vec<String>,
    /// Interpreter meter snapshot when the caller armed diagnostic stats.
    /// Absent when the case was not executed or stats were not requested.
    pub stats: Option<RunnerStats>,
}

#[derive(Debug)]
struct CaseExecution {
    outcome: CaseOutcome,
    stdout: Vec<String>,
    stderr: Vec<String>,
    stats: Option<RunnerStats>,
}

impl CaseExecution {
    fn without_output(outcome: CaseOutcome) -> Self {
        Self {
            outcome,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stats: None,
        }
    }
}

impl CaseResult {
    /// Report row with no captured output and no meter snapshot.
    #[must_use]
    pub fn reported(
        unit_path: PathBuf,
        name: String,
        suite_path: String,
        outcome: CaseOutcome,
    ) -> Self {
        Self {
            unit_path,
            name,
            suite_path,
            outcome,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stats: None,
        }
    }

    /// Display path used in reports: `suite/name` or just `name`.
    #[must_use]
    pub fn display_path(&self) -> String {
        join_suite_path(&self.suite_path, &self.name)
    }
}

/// `suite/name`, or just `name` when there is no suite.
fn join_suite_path(suite_path: &str, name: &str) -> String {
    if suite_path.is_empty() {
        name.to_owned()
    } else {
        format!("{suite_path}/{name}")
    }
}

/// Report row for a case that never ran.
fn skipped_result(unit_path: &Path, case: &ProbaCase, reason: String) -> CaseResult {
    CaseResult::reported(
        unit_path.to_path_buf(),
        case.name.clone(),
        case.suite_path.clone(),
        CaseOutcome::Skipped { reason },
    )
}

/// Aggregate report for a proba run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProbaReport {
    pub results: Vec<CaseResult>,
}

impl ProbaReport {
    #[must_use]
    pub fn passed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.outcome, CaseOutcome::Passed))
            .count()
    }

    #[must_use]
    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.outcome, CaseOutcome::Failed { .. }))
            .count()
    }

    #[must_use]
    pub fn skipped(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.outcome, CaseOutcome::Skipped { .. }))
            .count()
    }

    #[must_use]
    pub fn blocked(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.outcome, CaseOutcome::Blocked { .. }))
            .count()
    }

    /// True when at least one case was selected and all runnable cases passed
    /// (source skips allowed). An empty selection is not a successful run.
    /// Blocked cases are fail-closed: the runner never executed them.
    #[must_use]
    pub fn success(&self) -> bool {
        !self.results.is_empty() && self.failed() == 0 && self.blocked() == 0
    }
}

/// Enumerate proba cases from a lowered HIR program (source order).
#[must_use]
pub fn inventory_cases(hir: &HirModule, interner: &Interner) -> Vec<ProbaCase> {
    let mut cases = Vec::new();
    for item in &hir.items {
        let HirItemKind::Function(func) = &item.kind else {
            continue;
        };
        let Some(test) = func.test.as_ref() else {
            continue;
        };
        cases.push(proba_case_from_metadata(item.def_id, test, interner));
    }
    cases
}

fn proba_case_from_metadata(
    def_id: DefId,
    test: &HirTestMetadata,
    interner: &Interner,
) -> ProbaCase {
    ProbaCase {
        def_id,
        name: interner.resolve(test.name).to_owned(),
        suite_path: test
            .suite_path
            .iter()
            .map(|sym| interner.resolve(*sym).to_owned())
            .collect::<Vec<_>>()
            .join("/"),
        span: test.span,
        modifiers: test
            .modifiers
            .iter()
            .map(|m| resolve_modifier(m, interner))
            .collect(),
    }
}

fn resolve_modifier(modifier: &HirTestModifier, interner: &Interner) -> ProbaModifierView {
    match modifier {
        HirTestModifier::Erratur => ProbaModifierView::Erratur,
        HirTestModifier::Omitte(reason) => {
            ProbaModifierView::Omitte(interner.resolve(*reason).to_owned())
        }
        HirTestModifier::Futurum(reason) => {
            ProbaModifierView::Futurum(interner.resolve(*reason).to_owned())
        }
        HirTestModifier::Solum => ProbaModifierView::Solum,
        HirTestModifier::Tag(tag) => ProbaModifierView::Tag(interner.resolve(*tag).to_owned()),
        HirTestModifier::Temporis(n) => ProbaModifierView::Temporis(*n),
        HirTestModifier::Metior => ProbaModifierView::Metior,
        HirTestModifier::Repete(n) => ProbaModifierView::Repete(*n),
        HirTestModifier::Fragilis(n) => ProbaModifierView::Fragilis(*n),
        HirTestModifier::SolumIn(env) => {
            ProbaModifierView::SolumIn(interner.resolve(*env).to_owned())
        }
    }
}

/// Whether any inventoried case carries `solum` (focus-only mode).
#[must_use]
pub fn has_solum_cases(cases: &[ProbaCase]) -> bool {
    cases.iter().any(|case| {
        case.modifiers
            .iter()
            .any(|m| matches!(m, ProbaModifierView::Solum))
    })
}

/// Selection-filter skip reason, if this case is out of the active query.
#[must_use]
pub fn selection_skip_reason(
    case: &ProbaCase,
    selection: &TestSelection,
    has_solum: bool,
) -> Option<String> {
    if has_solum
        && !case
            .modifiers
            .iter()
            .any(|m| matches!(m, ProbaModifierView::Solum))
    {
        return Some("faber: not selected by solum".to_owned());
    }

    if let Some(name) = selection.name.as_deref()
        && case.name != name
    {
        return Some(format!("faber: not selected by name {name}"));
    }

    if let Some(suite) = selection.suite.as_deref()
        && case.suite_path != suite
    {
        return Some(format!("faber: not selected by suite {suite}"));
    }

    if let Some(tag) = selection.tag.as_deref()
        && !case
            .modifiers
            .iter()
            .any(|m| matches!(m, ProbaModifierView::Tag(value) if value == tag))
    {
        return Some(format!("faber: not selected by tag {tag}"));
    }

    None
}

/// Source-modifier skip reason (`omitte` / `futurum`).
#[must_use]
pub fn source_skip_reason(case: &ProbaCase) -> Option<String> {
    for modifier in &case.modifiers {
        match modifier {
            ProbaModifierView::Omitte(reason) => {
                return Some(format!("faber: omitte - {reason}"));
            }
            ProbaModifierView::Futurum(reason) => {
                return Some(format!("faber: futurum - {reason}"));
            }
            _ => {}
        }
    }
    None
}

/// How a case's remaining modifiers (`temporis` / `repete` / `solum_in` /
/// `fragilis` / `metior` / `erratur`) are enforced. Every parsed modifier is
/// either consumed here or rejected; none is silently ignored.
enum ExecutionPlan {
    Reject {
        reason: String,
    },
    Skip {
        reason: String,
    },
    Run {
        /// Resolved wall-clock timeout in seconds for this case:
        /// per-test `temporis`/`timeout` wins, else the caller-supplied
        /// default (CLI `--test-timeout`), else `None` (deadline-less).
        timeout_secs: Option<u64>,
        repeats: u64,
        erratur: bool,
    },
}

fn execution_plan(
    modifiers: &[ProbaModifierView],
    default_timeout_secs: Option<u64>,
) -> ExecutionPlan {
    let mut timeout_secs = default_timeout_secs;
    let mut repeats = 1_u64;
    let mut erratur = false;
    let mut solum_in = None;
    for modifier in modifiers {
        match modifier {
            ProbaModifierView::Erratur => erratur = true,
            ProbaModifierView::Omitte(_)
            | ProbaModifierView::Futurum(_)
            | ProbaModifierView::Solum
            | ProbaModifierView::Tag(_) => {}
            ProbaModifierView::Temporis(n) => match u64::try_from(*n) {
                Ok(secs) if secs > 0 => timeout_secs = Some(secs),
                _ => {
                    return ExecutionPlan::Reject {
                        reason: format!(
                            "faber: temporis requires a positive wall-clock timeout in seconds, got {n}"
                        ),
                    };
                }
            },
            ProbaModifierView::Repete(n) => match u64::try_from(*n) {
                Ok(count) if count > 0 => repeats = count,
                _ => {
                    return ExecutionPlan::Reject {
                        reason: format!("faber: repete requires a positive count, got {n}"),
                    };
                }
            },
            ProbaModifierView::Fragilis(_) => {
                return ExecutionPlan::Reject {
                    reason: "faber: fragilis is rejected — flaky retries are not executed"
                        .to_owned(),
                };
            }
            ProbaModifierView::Metior => {
                return ExecutionPlan::Reject {
                    reason: "faber: metior is rejected — benchmarks are not executed by faber test"
                        .to_owned(),
                };
            }
            ProbaModifierView::SolumIn(env) => solum_in = Some(env.clone()),
        }
    }
    if let Some(env) = solum_in
        && !solum_in_matches(&env)
    {
        return ExecutionPlan::Skip {
            reason: format!("faber: solum_in - only in {env}"),
        };
    }
    ExecutionPlan::Run {
        timeout_secs,
        repeats,
        erratur,
    }
}

/// The environment name that `solum_in` modifiers match in addition to the
/// OS name. `faber test --solum-in` sets it once; the helper performs no
/// environment access of its own.
static SOLUM_IN_ENV: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Set the environment name that `solum_in` modifiers match in addition to
/// `std::env::consts::OS`. Called by `faber test --solum-in`.
pub fn set_solum_in_environment(name: &str) {
    let mut configured = SOLUM_IN_ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *configured = Some(name.to_owned());
}

/// `solum_in "name"` runs only in that named environment.
///
/// Match order: the name configured through [`set_solum_in_environment`],
/// then `std::env::consts::OS`.
fn solum_in_matches(requested: &str) -> bool {
    let configured = SOLUM_IN_ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    solum_in_matches_in(requested, configured.as_deref(), std::env::consts::OS)
}

fn solum_in_matches_in(requested: &str, configured: Option<&str>, os: &str) -> bool {
    requested == os || configured == Some(requested)
}

/// Optional harness-style substring filter on case display path (`suite/name`).
#[must_use]
pub fn is_harness_filter_match(case: &ProbaCase, filter: Option<&str>, exact: bool) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let path = join_suite_path(&case.suite_path, &case.name);
    if exact {
        path == filter || case.name == filter
    } else {
        path.contains(filter) || case.name.contains(filter)
    }
}

/// Analyze, lower, and run all proba cases in one Faber source unit.
///
/// # Errors
///
/// Returns frontend / MIR errors when the unit cannot be analyzed or lowered.
pub fn run_proba_source(
    session: &Session,
    name: &str,
    source: &str,
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    default_timeout_secs: Option<u64>,
    collect_stats: bool,
) -> Result<ProbaReport, RunSourceError> {
    let mut analysis =
        analyze_prepared_program_name(session, name, source).map_err(RunSourceError::Frontend)?;
    // The session's kernel lowering names the route: host-form kernels are the
    // CPU oracle route and run under `Execute`; device-form kernels stay
    // refused by the runner.
    let kernel_lowering = session.config.kernel_lowering;
    let kernel_policy = match kernel_lowering {
        KernelLowering::Host => RunnerKernelPolicy::Execute,
        KernelLowering::Device => RunnerKernelPolicy::Refuse,
    };
    run_proba_on_analyzed_with_unit_path(
        &mut analysis,
        Path::new(name),
        selection,
        harness_filter,
        exact,
        include_ignored,
        default_timeout_secs,
        collect_stats,
        kernel_lowering,
        kernel_policy,
    )
}

/// Run inventoriable proba cases on an already-analyzed unit.
///
/// # Errors
///
/// Returns MIR / runner setup errors when lowering fails.
pub fn run_proba_on_analyzed(
    unit: &mut AnalyzedModule,
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    default_timeout_secs: Option<u64>,
    collect_stats: bool,
) -> Result<ProbaReport, RunSourceError> {
    run_proba_on_analyzed_with_unit_path(
        unit,
        Path::new(""),
        selection,
        harness_filter,
        exact,
        include_ignored,
        default_timeout_secs,
        collect_stats,
        KernelLowering::Device,
        RunnerKernelPolicy::Refuse,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_proba_on_analyzed_with_unit_path(
    unit: &mut AnalyzedModule,
    unit_path: &Path,
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    default_timeout_secs: Option<u64>,
    collect_stats: bool,
    kernel_lowering: KernelLowering,
    kernel_policy: RunnerKernelPolicy,
) -> Result<ProbaReport, RunSourceError> {
    let cases = inventory_cases(&unit.hir, &unit.interner);
    let lowered = lower_analyzed_unit(
        unit.application_view(),
        MirLoweringOptions {
            kernel_lowering,
            ..Default::default()
        },
    )
    .map_err(RunSourceError::Mir)?;
    Ok(run_proba_on_validated_with_unit_path(
        &lowered.validated,
        unit_path,
        &cases,
        selection,
        harness_filter,
        exact,
        include_ignored,
        default_timeout_secs,
        collect_stats,
        kernel_policy,
    ))
}

/// Execute inventoriable cases against already-validated MIR whose function
/// IDs are unrewritten.
///
/// Used by single-file runs. Package runners pass linker-owned identities to
/// [`run_proba_on_validated_with_function_ids`] instead.
#[must_use]
pub fn run_proba_on_validated(
    validated: &ValidatedMir<'_>,
    cases: &[ProbaCase],
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    default_timeout_secs: Option<u64>,
    collect_stats: bool,
    kernel_policy: RunnerKernelPolicy,
) -> ProbaReport {
    run_proba_on_validated_with_unit_path(
        validated,
        Path::new(""),
        cases,
        selection,
        harness_filter,
        exact,
        include_ignored,
        default_timeout_secs,
        collect_stats,
        kernel_policy,
    )
}

fn run_proba_on_validated_with_unit_path(
    validated: &ValidatedMir<'_>,
    unit_path: &Path,
    cases: &[ProbaCase],
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    default_timeout_secs: Option<u64>,
    collect_stats: bool,
    kernel_policy: RunnerKernelPolicy,
) -> ProbaReport {
    run_proba_cases(
        validated,
        unit_path,
        cases,
        selection,
        harness_filter,
        exact,
        include_ignored,
        default_timeout_secs,
        collect_stats,
        CaseKernels::Runner(kernel_policy),
        |case| mir_function_id_from_def(validated, case.def_id),
    )
}

/// Execute one package module's cases against a linked MIR program.
///
/// The caller supplies the linker-owned identity map because package lowering
/// rewrites source and function IDs while merging modules. `module_path` keeps
/// the original HIR `DefId` scoped to its source module, preserving report
/// identity and the existing per-unit `solum` selection behavior.
#[must_use]
pub fn run_proba_on_validated_with_function_ids(
    validated: &ValidatedMir<'_>,
    module_path: &Path,
    cases: &[ProbaCase],
    function_ids: &HashMap<(PathBuf, DefId), MirFunctionId>,
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    default_timeout_secs: Option<u64>,
    collect_stats: bool,
    kernels: CaseKernels<'_>,
) -> ProbaReport {
    let module_path = module_path.to_path_buf();
    run_proba_cases(
        validated,
        &module_path,
        cases,
        selection,
        harness_filter,
        exact,
        include_ignored,
        default_timeout_secs,
        collect_stats,
        kernels,
        |case| {
            function_ids
                .get(&(module_path.clone(), case.def_id))
                .copied()
        },
    )
}

fn run_proba_cases(
    validated: &ValidatedMir<'_>,
    unit_path: &Path,
    cases: &[ProbaCase],
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    default_timeout_secs: Option<u64>,
    collect_stats: bool,
    kernels: CaseKernels<'_>,
    function_id_for_case: impl Fn(&ProbaCase) -> Option<MirFunctionId>,
) -> ProbaReport {
    let has_solum = has_solum_cases(cases);
    let mut results = Vec::with_capacity(cases.len());

    for case in cases {
        if !is_harness_filter_match(case, harness_filter, exact) {
            continue;
        }

        if let Some(reason) = selection_skip_reason(case, selection, has_solum) {
            // Selection skips are omitted from the report (not counted as skipped)
            // unless include_ignored is set — matching language-level selection,
            // not Cargo's “ignored suite”.
            if include_ignored {
                results.push(skipped_result(unit_path, case, reason));
            }
            continue;
        }

        if let Some(reason) = source_skip_reason(case) {
            results.push(skipped_result(unit_path, case, reason));
            continue;
        }

        let execution = match execution_plan(&case.modifiers, default_timeout_secs) {
            ExecutionPlan::Reject { reason } => {
                CaseExecution::without_output(CaseOutcome::Failed { message: reason })
            }
            ExecutionPlan::Skip { reason } => {
                CaseExecution::without_output(CaseOutcome::Skipped { reason })
            }
            ExecutionPlan::Run {
                timeout_secs,
                repeats,
                erratur,
            } => match function_id_for_case(case) {
                Some(function_id) => execute_case_repeated(
                    validated,
                    function_id,
                    timeout_secs,
                    repeats,
                    erratur,
                    collect_stats,
                    kernels,
                ),
                None => CaseExecution::without_output(CaseOutcome::Blocked {
                    reason: format!(
                        "proba case {:?} has no lowered MIR function (def #{})",
                        case.name, case.def_id.0
                    ),
                }),
            },
        };
        results.push(CaseResult {
            unit_path: unit_path.to_path_buf(),
            name: case.name.clone(),
            suite_path: case.suite_path.clone(),
            outcome: execution.outcome,
            stdout: execution.stdout,
            stderr: execution.stderr,
            stats: execution.stats,
        });
    }

    ProbaReport { results }
}

fn mir_function_id_from_def(validated: &ValidatedMir<'_>, def_id: DefId) -> Option<MirFunctionId> {
    validated
        .program()
        .functions
        .iter()
        .find(|function| function.source == Some(def_id))
        .map(|function| function.id)
}

fn execute_case_repeated(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    timeout_secs: Option<u64>,
    repeats: u64,
    erratur: bool,
    collect_stats: bool,
    kernels: CaseKernels<'_>,
) -> CaseExecution {
    execute_repeated(repeats, || {
        // Apply erratur inside each execution so a passing expected-error run
        // does not trip the repeat loop's non-Passed short-circuit.
        execute_case(
            validated,
            function_id,
            timeout_secs,
            erratur,
            collect_stats,
            kernels,
        )
    })
}

fn execute_repeated(repeats: u64, mut execute: impl FnMut() -> CaseExecution) -> CaseExecution {
    let mut last = CaseExecution::without_output(CaseOutcome::Passed);
    for _ in 0..repeats {
        last = execute();
        if !matches!(last.outcome, CaseOutcome::Passed) {
            return last;
        }
    }
    last
}

fn execute_case(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    timeout_secs: Option<u64>,
    erratur: bool,
    collect_stats: bool,
    kernels: CaseKernels<'_>,
) -> CaseExecution {
    // A seed belongs to one program execution. Clear the runner's process-wide
    // kernel state before this case can observe an earlier program's seed.
    reset_execution_state();
    // A device route opens its dispatcher before the case starts; a device
    // that cannot open blocks the case, naming why.
    let dispatcher = match kernels {
        CaseKernels::Runner(_) => None,
        CaseKernels::Dispatch(open) => match open() {
            Ok(dispatcher) => Some(dispatcher),
            Err(reason) => {
                return CaseExecution::without_output(CaseOutcome::Blocked { reason });
            }
        },
    };
    let kernel_policy = match kernels {
        CaseKernels::Runner(policy) => policy,
        CaseKernels::Dispatch(_) => RunnerKernelPolicy::Dispatch,
    };
    let mut host = BufferHost::default();
    // Wall-clock deadline from the resolved timeout (per-test temporis, else
    // the CLI default). A horizon past `Instant`'s representable range cannot
    // form a deadline; the runner's default step budget still applies as a
    // backstop via the `None` step-budget argument.
    let deadline =
        timeout_secs.and_then(|secs| Instant::now().checked_add(Duration::from_secs(secs)));
    // Assertion failures call Host::abort which panics on BufferHost; catch as a
    // body failure. Under erratur, this captured failure is the expected result.
    // Silence the default panic hook so failed cases do not dump a backtrace to stderr.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let trapped = catch_unwind(AssertUnwindSafe(|| {
        if let Some(dispatcher) = &dispatcher {
            match run_function_with_limits_and_kernel_dispatcher(
                validated,
                function_id,
                &mut host,
                None,
                deadline,
                collect_stats,
                dispatcher,
            ) {
                Ok(outcome) => (outcome.result, collect_stats.then_some(outcome.stats)),
                Err(errors) => (Err(errors), None),
            }
        } else if collect_stats {
            match run_function_with_stats_and_limits_and_kernel_policy(
                validated,
                function_id,
                &mut host,
                None,
                deadline,
                kernel_policy,
            ) {
                Ok(outcome) => (outcome.result, Some(outcome.stats)),
                Err(errors) => (Err(errors), None),
            }
        } else {
            (
                run_function_with_limits_and_kernel_policy(
                    validated,
                    function_id,
                    &mut host,
                    None,
                    deadline,
                    kernel_policy,
                ),
                None,
            )
        }
    }));
    std::panic::set_hook(previous_hook);
    let (run_result, stats) = match trapped {
        Ok(pair) => pair,
        Err(payload) => {
            let outcome = if erratur {
                CaseOutcome::Passed
            } else if let Some(reason) = host.last_abort.clone() {
                CaseOutcome::Failed { message: reason }
            } else {
                let message = abort_message_from_payload(payload.as_ref())
                    .unwrap_or_else(|| "proba case aborted".to_owned());
                CaseOutcome::Failed { message }
            };
            return CaseExecution {
                outcome,
                stdout: captured_stdout(&host),
                stderr: host.stderr_lines,
                stats: None,
            };
        }
    };
    CaseExecution {
        outcome: outcome_from_run_result(run_result, erratur),
        stdout: captured_stdout(&host),
        stderr: host.stderr_lines,
        stats,
    }
}

fn captured_stdout(host: &BufferHost) -> Vec<String> {
    let mut stdout = host.stdout_lines.clone();
    if !host.pending_stdout.is_empty() {
        stdout.push(host.pending_stdout.clone());
    }
    stdout
}

/// Runner issues emitted when execution cannot say whether the test body
/// completed: limits and capability gaps do not satisfy `erratur`.
fn outcome_from_run_result(result: Result<(), Vec<RunnerError>>, erratur: bool) -> CaseOutcome {
    match result {
        // Strict-xfail: an erratur case that completes cleanly is a suite
        // failure, not a silently accepted pass.
        Ok(()) if erratur => CaseOutcome::Failed {
            message: "faber: erratur expected the test body to fail, but it completed cleanly"
                .to_owned(),
        },
        Ok(()) => CaseOutcome::Passed,
        Err(errors) => {
            let outcome = case_outcome_from_runner_errors(&errors);
            if erratur && !errors.iter().any(is_execution_limit) {
                match outcome {
                    CaseOutcome::Failed { .. } => CaseOutcome::Passed,
                    other => other,
                }
            } else {
                outcome
            }
        }
    }
}

fn is_execution_limit(error: &RunnerError) -> bool {
    error.issue == RUNNER_ISSUE_WALL_CLOCK_TIMEOUT
        || error.issue == RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED
}

fn case_outcome_from_runner_errors(errors: &[RunnerError]) -> CaseOutcome {
    // Verdicts consume runner identity; prose is only report detail. Preserve
    // timeout/budget precedence when an execution reports several errors.
    let message = format_runner_errors(errors);
    if errors
        .iter()
        .any(|error| error.issue == RUNNER_ISSUE_WALL_CLOCK_TIMEOUT)
    {
        CaseOutcome::Failed {
            message: "faber: test exceeded its wall-clock timeout".to_owned(),
        }
    } else if errors
        .iter()
        .any(|error| error.issue == RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED)
    {
        CaseOutcome::Failed {
            message: format!("temporis: {message}"),
        }
    } else if errors
        .iter()
        .any(|error| error.code == "runner_unsupported")
    {
        CaseOutcome::Blocked { reason: message }
    } else {
        CaseOutcome::Failed { message }
    }
}

fn format_runner_errors(errors: &[RunnerError]) -> String {
    errors
        .iter()
        .map(|error| error.message.clone())
        .collect::<Vec<_>>()
        .join("; ")
}

fn abort_message_from_payload(payload: &(dyn std::any::Any + Send)) -> Option<String> {
    if let Some(message) = payload.downcast_ref::<String>() {
        return Some(strip_abort_prefix(message));
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return Some(strip_abort_prefix(message));
    }
    None
}

fn strip_abort_prefix(message: &str) -> String {
    message
        .strip_prefix("runner abort: ")
        .unwrap_or(message)
        .to_owned()
}

/// Format a human-readable summary line for CLI output.
#[must_use]
pub fn format_summary(report: &ProbaReport) -> String {
    let verdict = if report.failed() > 0 {
        "FAILED"
    } else if report.blocked() > 0 {
        "BLOCKED"
    } else {
        "ok"
    };
    format!(
        "test result: {verdict}. {} passed; {} failed; {} blocked; {} skipped",
        report.passed(),
        report.failed(),
        report.blocked(),
        report.skipped()
    )
}
