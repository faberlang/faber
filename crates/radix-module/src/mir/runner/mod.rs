//! In-process MIR runner for Faber scripting.
//!
//! Core interpret path lives in leaf `radix-mir-runner`. This package module
//! re-exports that leaf and owns analyze/lower orchestration for `run_source`
//! / `run_analyzed` (needs driver `AnalyzedModule`).

pub use radix_mir_runner::{
    classify_runner_lowerability, run_entry, run_function, runner_dispatch_status, BufferHost, Host,
    RunnerDispatchStatus, RunnerError, StdioHost, Value,
};

use crate::diagnostics::Diagnostic;
use crate::driver::{analyze_source, AnalyzedModule, Session};
use crate::mir::lower_analyzed_unit_with_context;

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
    let mut analysis = analyze_source(session, name, source).map_err(RunSourceError::Frontend)?;
    run_analyzed(&mut analysis, host).map_err(RunSourceError::Stepper)
}

/// Lower and interpret an already-analyzed unit.
///
/// # Errors
/// Returns [`RunnerError`] values for MIR lowering failures and
/// interpretation-time failures.
pub fn run_analyzed<H: Host + ?Sized>(unit: &mut AnalyzedModule, host: &mut H) -> Result<(), Vec<RunnerError>> {
    let lowered = lower_analyzed_unit_with_context(unit).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| RunnerError::internal(error.span, error.message))
            .collect::<Vec<_>>()
    })?;
    run_entry(&lowered.validated, host)
}

#[cfg(test)]
#[path = "capability_test.rs"]
mod capability_tests;

#[cfg(test)]
mod value {
    pub use radix_lexer::Span;
    pub use radix_mir::MirBinOp;
    pub use radix_mir_runner::value::*;
}

#[cfg(test)]
#[path = "value_test.rs"]
mod value_tests;

#[cfg(test)]
mod kernel {
    pub use radix_mir_runner::kernel::*;
    pub use radix_mir_runner::value::MapKey;
    pub use radix_mir_runner::{BufferHost, Value};
}

#[cfg(test)]
#[path = "kernel_test.rs"]
mod kernel_tests;

#[cfg(test)]
#[path = "conversio_test.rs"]
mod conversio_tests;

#[cfg(test)]
#[path = "cede_cursor_test.rs"]
mod cede_cursor_tests;
