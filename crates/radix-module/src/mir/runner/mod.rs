//! In-process MIR runner for Faber scripting.
//!
//! Core interpret path lives in leaf `radix-mir-runner`. This package module
//! re-exports that leaf and owns analyze/lower orchestration for `run_source`
//! / `run_analyzed` (needs driver `AnalyzedModule`).

pub use radix_mir_runner::{
    BufferHost, Host, RunnerDispatchStatus, RunnerError, RunnerRunOutcome, RunnerStats, StdioHost, Value,
    classify_runner_lowerability, reset_execution_state, run_entry, run_entry_with_stats, run_function,
    run_function_with_limits, run_function_with_stats_and_limits, run_function_with_step_budget,
    runner_dispatch_status,
};

#[cfg(test)]
#[path = "ad_close_test.rs"]
mod ad_close_tests;

#[cfg(test)]
#[path = "ad_serve_test.rs"]
mod ad_serve_tests;

#[cfg(test)]
#[path = "itera_inf_test.rs"]
mod itera_inf_tests;

#[cfg(test)]
#[path = "range_step_test.rs"]
mod range_step_tests;

#[cfg(test)]
#[path = "call_depth_test.rs"]
mod call_depth_tests;

#[cfg(test)]
#[path = "kernel_ban_test.rs"]
mod kernel_ban_tests;

#[cfg(test)]
#[path = "lazy_cursor_test.rs"]
mod lazy_cursor_tests;

use crate::diagnostics::Diagnostic;
use crate::driver::{AnalyzedModule, Session};
use crate::mir::lower_analyzed_unit;
use crate::program::analyze_prepared_program_name;

/// Failure modes for the analyzed-source run routes.
///
/// The [`RunSourceError::Mir`] variant is only produced by callers that lower
/// directly; [`run_source`] absorbs lowering failures into
/// [`RunSourceError::Stepper`] via [`run_analyzed`].
#[derive(Debug)]
pub enum RunSourceError {
    Frontend(Vec<Diagnostic>),
    Mir(Vec<crate::mir::MirError>),
    Stepper(Vec<RunnerError>),
}

/// Analyze, lower, and interpret one Faber source unit.
///
/// # Errors
/// Returns [`RunSourceError::Frontend`] for source-level analysis failures, or
/// [`RunSourceError::Stepper`] for lowering and interpretation-time failures
/// (lowering failures are absorbed into `RunnerError` by [`run_analyzed`]).
pub fn run_source<H: Host + ?Sized>(
    session: &Session,
    name: &str,
    source: &str,
    host: &mut H,
) -> Result<(), RunSourceError> {
    let mut analysis = analyze_prepared_program_name(session, name, source).map_err(RunSourceError::Frontend)?;
    run_analyzed(&mut analysis, host).map_err(RunSourceError::Stepper)
}

/// Lower and interpret an already-analyzed unit.
///
/// # Errors
/// Returns [`RunnerError`] values for MIR lowering failures and
/// interpretation-time failures.
pub fn run_analyzed<H: Host + ?Sized>(unit: &mut AnalyzedModule, host: &mut H) -> Result<(), Vec<RunnerError>> {
    let lowered = lower_analyzed_unit(unit, crate::mir::MirLoweringOptions::default()).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| RunnerError::internal(error.span, error.message))
            .collect::<Vec<_>>()
    })?;
    run_entry(&lowered.validated, host)
}
