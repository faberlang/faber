//! Target-neutral proba inventory and MIR-runner case runner.
//!
//! `faber test` executes Faber `proba` / `probandum` cases on the MIR runner.
//! Selection policy and case results live here so language tests do not depend
//! on a codegen backend or Cargo harness.

#![cfg(feature = "runner")]

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

use radix::driver::{analyze_source, AnalyzedModule, Session};
use radix::hir::{DefId, HirItemKind, HirModule, HirTestMetadata, HirTestModifier};
use radix::lexer::Interner;
use radix::mir::{
    lower_analyzed_unit_with_context, run_function, BufferHost, MirFunctionId, RunSourceError,
    RunnerError, ValidatedMir,
};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

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
    Failed { message: String },
    Skipped { reason: String },
}

/// Result for one inventoriable case in report order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    pub name: String,
    pub suite_path: String,
    pub outcome: CaseOutcome,
}

impl CaseResult {
    /// Display path used in reports: `suite/name` or just `name`.
    #[must_use]
    pub fn display_path(&self) -> String {
        if self.suite_path.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.suite_path, self.name)
        }
    }
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

    /// True when every selected runnable case passed (skips allowed).
    #[must_use]
    pub fn success(&self) -> bool {
        self.failed() == 0
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

    if let Some(name) = selection.name.as_deref() {
        if case.name != name {
            return Some(format!("faber: not selected by name {name}"));
        }
    }

    if let Some(suite) = selection.suite.as_deref() {
        if case.suite_path != suite {
            return Some(format!("faber: not selected by suite {suite}"));
        }
    }

    if let Some(tag) = selection.tag.as_deref() {
        if !case
            .modifiers
            .iter()
            .any(|m| matches!(m, ProbaModifierView::Tag(value) if value == tag))
        {
            return Some(format!("faber: not selected by tag {tag}"));
        }
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

/// Optional harness-style substring filter on case display path (`suite/name`).
#[must_use]
pub fn harness_filter_matches(case: &ProbaCase, filter: Option<&str>, exact: bool) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let path = if case.suite_path.is_empty() {
        case.name.clone()
    } else {
        format!("{}/{}", case.suite_path, case.name)
    };
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
) -> Result<ProbaReport, RunSourceError> {
    let mut analysis = analyze_source(session, name, source).map_err(RunSourceError::Frontend)?;
    run_proba_on_analyzed(
        &mut analysis,
        selection,
        harness_filter,
        exact,
        include_ignored,
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
) -> Result<ProbaReport, RunSourceError> {
    let cases = inventory_cases(&unit.hir, &unit.interner);
    let lowered = lower_analyzed_unit_with_context(unit).map_err(RunSourceError::Mir)?;
    Ok(run_proba_on_validated(
        &lowered.validated,
        &cases,
        selection,
        harness_filter,
        exact,
        include_ignored,
    ))
}

/// Execute inventoriable cases against already-validated MIR.
///
/// Used by single-file runs and by package runners that share one linked program.
#[must_use]
pub fn run_proba_on_validated(
    validated: &ValidatedMir<'_>,
    cases: &[ProbaCase],
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
) -> ProbaReport {
    run_proba_cases(
        validated,
        cases,
        selection,
        harness_filter,
        exact,
        include_ignored,
        |case| mir_function_id_for_def(validated, case.def_id),
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
) -> ProbaReport {
    let module_path = module_path.to_path_buf();
    run_proba_cases(
        validated,
        cases,
        selection,
        harness_filter,
        exact,
        include_ignored,
        |case| {
            function_ids
                .get(&(module_path.clone(), case.def_id))
                .copied()
        },
    )
}

fn run_proba_cases(
    validated: &ValidatedMir<'_>,
    cases: &[ProbaCase],
    selection: &TestSelection,
    harness_filter: Option<&str>,
    exact: bool,
    include_ignored: bool,
    function_id_for_case: impl Fn(&ProbaCase) -> Option<MirFunctionId>,
) -> ProbaReport {
    let has_solum = has_solum_cases(cases);
    let mut results = Vec::with_capacity(cases.len());

    for case in cases {
        if !harness_filter_matches(case, harness_filter, exact) {
            continue;
        }

        if let Some(reason) = selection_skip_reason(case, selection, has_solum) {
            // Selection skips are omitted from the report (not counted as skipped)
            // unless include_ignored is set — matching language-level selection,
            // not Cargo's “ignored suite”.
            if include_ignored {
                results.push(CaseResult {
                    name: case.name.clone(),
                    suite_path: case.suite_path.clone(),
                    outcome: CaseOutcome::Skipped { reason },
                });
            }
            continue;
        }

        if let Some(reason) = source_skip_reason(case) {
            results.push(CaseResult {
                name: case.name.clone(),
                suite_path: case.suite_path.clone(),
                outcome: CaseOutcome::Skipped { reason },
            });
            continue;
        }

        let outcome = match function_id_for_case(case) {
            Some(function_id) => execute_case(validated, function_id),
            None => CaseOutcome::Failed {
                message: format!(
                    "proba case {:?} has no lowered MIR function (def #{})",
                    case.name, case.def_id.0
                ),
            },
        };
        results.push(CaseResult {
            name: case.name.clone(),
            suite_path: case.suite_path.clone(),
            outcome,
        });
    }

    ProbaReport { results }
}

fn mir_function_id_for_def(validated: &ValidatedMir<'_>, def_id: DefId) -> Option<MirFunctionId> {
    validated
        .program()
        .functions
        .iter()
        .find(|function| function.source == Some(def_id))
        .map(|function| function.id)
}

fn execute_case(validated: &ValidatedMir<'_>, function_id: MirFunctionId) -> CaseOutcome {
    let mut host = BufferHost::default();
    // Assertion failures call Host::abort which panics on BufferHost; catch as fail.
    // Silence the default panic hook so failed cases do not dump a backtrace to stderr.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let trapped = catch_unwind(AssertUnwindSafe(|| {
        run_function(validated, function_id, &mut host)
    }));
    std::panic::set_hook(previous_hook);
    match trapped {
        Ok(Ok(())) => CaseOutcome::Passed,
        Ok(Err(errors)) => CaseOutcome::Failed {
            message: format_runner_errors(&errors),
        },
        Err(payload) => {
            if let Some(reason) = host.last_abort.clone() {
                return CaseOutcome::Failed { message: reason };
            }
            let message = abort_message_from_payload(payload.as_ref())
                .unwrap_or_else(|| "proba case aborted".to_owned());
            CaseOutcome::Failed { message }
        }
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
    format!(
        "test result: {}. {} passed; {} failed; {} skipped",
        if report.success() { "ok" } else { "FAILED" },
        report.passed(),
        report.failed(),
        report.skipped()
    )
}
