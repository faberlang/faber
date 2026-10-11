//! `faber test` package-route input filter and bounded analysis.
//!
//! An explicit `.proba` path is a source selection: the package route must
//! not load sibling test files. Analysis itself is wall-clock bounded so a
//! loader hang becomes a diagnostic that names the descent, not silence.

use radix::module::diagnostics::Diagnostic;
use radix::module::driver::Config;
use radix::program::analyze::AnalyzedProgram;
use radix::program::source_files::is_proba_source_path;
use radix::program::test_source_filter::TestSourceFilter;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Outer wall-clock budget for package-route test analysis.
///
/// Radix-program also budgets between units; this bound covers a unit that
/// never returns. The CLI sanity window is 120s; this is the inner cap.
pub(crate) const PACKAGE_TEST_ANALYZE_BUDGET: Duration = Duration::from_secs(30);

const ANALYZE_THREAD_STACK_SIZE: usize = 8 * 1024 * 1024;
const MAX_DESCENT_NAMES: usize = 24;

/// Build the `*.proba` load filter for a `faber test` input.
///
/// A named `.proba` file replaces `--include` so the package route cannot
/// walk sibling suites. `--exclude` is preserved. Directory / package-root
/// inputs keep the caller's include/exclude as given.
pub fn test_source_filter_for_input(
    input: &Path,
    include: Vec<String>,
    exclude: Vec<String>,
) -> Option<TestSourceFilter> {
    let input = super::absolutize_path(input);
    let existing = if include.is_empty() && exclude.is_empty() {
        None
    } else {
        Some(TestSourceFilter { include, exclude })
    };
    if !is_proba_source_path(&input) {
        return existing;
    }
    let source_root = source_root_for_input(&input);
    Some(TestSourceFilter::restricting_explicit_proba(
        existing.as_ref(),
        &source_root,
        &input,
    ))
}

/// Analyze a package for `faber test` with auto-filter and a wall-clock budget.
///
/// # Errors
///
/// Returns the package analysis diagnostics when the package cannot be
/// analyzed, or a single budget diagnostic when the wall-clock budget elapses
/// before analysis completes.
pub fn analyze_package_for_tests(
    config: &Config,
    input: &Path,
    proba_filter: Option<&TestSourceFilter>,
) -> Result<AnalyzedProgram, Vec<Diagnostic>> {
    analyze_package_for_tests_with_budget(config, input, proba_filter, PACKAGE_TEST_ANALYZE_BUDGET)
}

/// Same as [`analyze_package_for_tests`] with an explicit budget (tests).
pub(crate) fn analyze_package_for_tests_with_budget(
    config: &Config,
    input: &Path,
    proba_filter: Option<&TestSourceFilter>,
    budget: Duration,
) -> Result<AnalyzedProgram, Vec<Diagnostic>> {
    let input = super::absolutize_path(input);
    let owned_filter = merge_explicit_proba_filter(&input, proba_filter);
    let filter = owned_filter.as_ref().or(proba_filter);
    let descent = test_route_descent(&input, filter);
    run_with_analyze_budget(budget, {
        let config = config.clone();
        let input = input.clone();
        let filter = filter.cloned();
        move || {
            radix::program::analyze::analyze_package_for_tests(
                &config,
                &input,
                filter.as_ref(),
                &super::faber_analysis_seams(),
            )
        }
    })
    .unwrap_or_else(|()| Err(vec![bounded_analyze_diagnostic(budget, &descent)]))
}

/// Run `work` on a large-stack thread; `Err(())` means the budget elapsed.
pub(crate) fn run_with_analyze_budget<T, F>(budget: Duration, work: F) -> Result<T, ()>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let spawn = thread::Builder::new()
        .name("faber-test-analyze".to_owned())
        .stack_size(ANALYZE_THREAD_STACK_SIZE)
        .spawn(move || {
            tx.send(work()).ok();
        });
    let Ok(handle) = spawn else {
        return Err(());
    };
    match rx.recv_timeout(budget) {
        Ok(value) => match handle.join() {
            Ok(()) => Ok(value),
            Err(payload) => std::panic::resume_unwind(payload),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => Err(()),
        Err(mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
            Ok(()) => Err(()),
            Err(payload) => std::panic::resume_unwind(payload),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestRouteDescent {
    pub input: String,
    pub include: Vec<String>,
    pub selected: Vec<String>,
    pub discovered: Vec<String>,
}

pub(crate) fn test_route_descent(
    input: &Path,
    filter: Option<&TestSourceFilter>,
) -> TestRouteDescent {
    let input = super::absolutize_path(input);
    let source_root = source_root_for_input(&input);
    let discovered_paths =
        radix::program::source_files::package_source_files(&source_root, true).unwrap_or_default();
    let mut discovered = Vec::new();
    let mut selected = Vec::new();
    for path in discovered_paths {
        if !is_proba_source_path(&path) {
            continue;
        }
        let relative = relative_display(&source_root, &path);
        discovered.push(relative.clone());
        let allowed = match filter {
            None => true,
            Some(filter) => filter.allows_path(&source_root, &path),
        };
        if allowed {
            selected.push(relative);
        }
    }
    TestRouteDescent {
        input: input.display().to_string(),
        include: filter
            .map(|filter| filter.include.clone())
            .unwrap_or_default(),
        selected,
        discovered,
    }
}

pub(crate) fn bounded_analyze_diagnostic(
    budget: Duration,
    descent: &TestRouteDescent,
) -> Diagnostic {
    let include = if descent.include.is_empty() {
        "(all)".to_owned()
    } else {
        descent.include.join(", ")
    };
    crate::package_diagnostic_error(format!(
        "package test analysis exceeded the {}ms budget while descending from `{}` (include: {include}); selected test sources: {}; discovered test sources: {}",
        budget.as_millis(),
        descent.input,
        join_capped(&descent.selected),
        join_capped(&descent.discovered),
    ))
    .with_file(descent.input.clone())
    .with_arg("issue", "package_test_analyze_budget_exceeded")
    .with_arg("budget_ms", budget.as_millis().to_string())
    .with_arg("phase", "analyze")
    .with_arg("include", include)
    .with_arg("selected", join_capped(&descent.selected))
    .with_arg("discovered", join_capped(&descent.discovered))
}

fn merge_explicit_proba_filter(
    input: &Path,
    existing: Option<&TestSourceFilter>,
) -> Option<TestSourceFilter> {
    if !is_proba_source_path(input) {
        return None;
    }
    Some(TestSourceFilter::restricting_explicit_proba(
        existing,
        &source_root_for_input(input),
        input,
    ))
}

fn source_root_for_input(input: &Path) -> PathBuf {
    match super::discover_package(input) {
        Ok(spec) if spec.source_root.as_os_str().is_empty() => input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
        Ok(spec) => spec.source_root,
        Err(_) => input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
    }
}

fn relative_display(source_root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(source_root).unwrap_or(path);
    let mut parts = Vec::new();
    for component in relative.components() {
        if let Component::Normal(part) = component {
            parts.push(part.to_string_lossy());
        }
    }
    parts.join("/")
}

fn join_capped(names: &[String]) -> String {
    if names.is_empty() {
        return "(none)".to_owned();
    }
    if names.len() <= MAX_DESCENT_NAMES {
        return names.join(", ");
    }
    let shown = names[..MAX_DESCENT_NAMES].join(", ");
    format!("{shown}, +{} more", names.len() - MAX_DESCENT_NAMES)
}

#[cfg(test)]
#[path = "test_route_test.rs"]
mod test_route_test;
