//! In-process MIR runner for Faber scripting.
//!
//! Core interpret path lives in leaf `radix-mir-runner`. This package module
//! re-exports that leaf and owns analyze/lower orchestration for `run_source`
//! / `run_analyzed` (needs driver `AnalyzedModule`).

pub use radix_mir_runner::{
    BufferHost, Host, RUNNER_KERNEL_REFUSED_ISSUE, RunnerDispatchStatus, RunnerError, RunnerKernelPolicy,
    RunnerRunOutcome, RunnerStats, StdioHost, Value, classify_run_tier, classify_runner_lowerability,
    configure_shuffle_seed, reset_execution_state, run_entry, run_entry_with_kernel_policy,
    run_entry_with_process_exit, run_entry_with_stats, run_entry_with_stats_and_kernel_policy, run_function,
    run_function_with_kernel_policy, run_function_with_limits, run_function_with_limits_and_kernel_policy,
    run_function_with_stats_and_limits, run_function_with_stats_and_limits_and_kernel_policy,
    run_function_with_step_budget, runner_dispatch_status, serve_route,
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
#[path = "entry_exit_test.rs"]
mod entry_exit_tests;

#[cfg(test)]
#[path = "route_serve_test.rs"]
mod route_serve_tests;

#[cfg(test)]
#[path = "sermo_fault_test.rs"]
mod sermo_fault_tests;

#[cfg(test)]
#[path = "http_route_test.rs"]
mod http_route_tests;

#[cfg(test)]
#[path = "range_step_test.rs"]
mod range_step_tests;

#[cfg(test)]
#[path = "sink_into_test.rs"]
mod sink_into_tests;

#[cfg(test)]
#[path = "creo_hook_test.rs"]
mod creo_hook_tests;

#[cfg(test)]
#[path = "call_depth_test.rs"]
mod call_depth_tests;

#[cfg(test)]
#[path = "display_tokens_run_test.rs"]
mod display_tokens_run_tests;

#[cfg(test)]
#[path = "kernel_ban_test.rs"]
mod kernel_ban_tests;

#[cfg(test)]
#[path = "lazy_cursor_test.rs"]
mod lazy_cursor_tests;

#[cfg(test)]
#[path = "lege_read_test.rs"]
mod lege_read_tests;

#[cfg(test)]
#[path = "vector_matrix_test.rs"]
mod vector_matrix_tests;

#[cfg(test)]
#[path = "option_chain_call_test.rs"]
mod option_chain_call_tests;

#[cfg(test)]
#[path = "rounding_conversio_run_test.rs"]
mod rounding_conversio_run_tests;

#[cfg(test)]
#[path = "contract_environment_test.rs"]
mod contract_environment_tests;

use crate::diagnostics::Diagnostic;
use crate::driver::{AnalyzedModule, Session};
use crate::mir::{KernelLowering, lower_analyzed_unit};
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
/// Kernels follow the session's [`crate::driver::Config::kernel_lowering`]:
/// the default refuses them, a host-kernel session runs them on the CPU (see
/// [`run_analyzed_with_kernel_lowering`]).
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
    crate::driver::require_contract_execution_target(&analysis, crate::codegen::Target::MirStepper).map_err(
        |diagnostic| {
            let diagnostic = *diagnostic;
            let diagnostic = match diagnostic.span {
                Some(span) => diagnostic.with_source_context(name, source, span),
                None => diagnostic.with_file(name),
            };
            RunSourceError::Frontend(vec![diagnostic])
        },
    )?;
    run_analyzed_with_kernel_lowering(&mut analysis, session.config.kernel_lowering, host)
        .map_err(RunSourceError::Stepper)
}

/// Lower and interpret an already-analyzed unit. `@ kernel` functions are
/// refused ([`RunnerKernelPolicy::Refuse`]).
///
/// # Errors
/// Returns [`RunnerError`] values for MIR lowering failures and
/// interpretation-time failures.
pub fn run_analyzed<H: Host + ?Sized>(unit: &mut AnalyzedModule, host: &mut H) -> Result<(), Vec<RunnerError>> {
    run_analyzed_with_kernel_lowering(unit, KernelLowering::Device, host)
}

/// [`run_analyzed`] with the kernel route named. `Device` lowers kernels in
/// the device form and refuses to run them; `Host` lowers them as ordinary
/// functions and runs them single-threaded on the runner
/// ([`RunnerKernelPolicy::Execute`]).
///
/// # Errors
/// Returns [`RunnerError`] values for MIR lowering failures and
/// interpretation-time failures.
pub fn run_analyzed_with_kernel_lowering<H: Host + ?Sized>(
    unit: &mut AnalyzedModule,
    kernel_lowering: KernelLowering,
    host: &mut H,
) -> Result<(), Vec<RunnerError>> {
    crate::driver::require_contract_execution_target(unit, crate::codegen::Target::MirStepper).map_err(
        |diagnostic| {
            let diagnostic = *diagnostic;
            let issue = diagnostic
                .issue()
                .unwrap_or("contract_target_mismatch")
                .to_owned();
            vec![RunnerError {
                code: diagnostic.code.unwrap_or("PKG001"),
                issue,
                span: diagnostic.span.unwrap_or_default(),
                message: diagnostic.message,
                args: diagnostic
                    .args
                    .into_iter()
                    .map(|arg| (arg.name.to_owned(), arg.value))
                    .collect(),
            }]
        },
    )?;
    let options = crate::mir::MirLoweringOptions { kernel_lowering, ..Default::default() };
    #[cfg(feature = "device")]
    let bundle = crate::driver::prepare_air_backward_bundle(unit)
        .map_err(|error| vec![RunnerError::internal(error.span.unwrap_or_default(), error.message)])?;
    let lowered = lower_analyzed_unit(unit.application_view(), options).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| RunnerError::internal(error.span, error.message))
            .collect::<Vec<_>>()
    })?;
    #[cfg(feature = "device")]
    let lowered = if let Some(bundle) = bundle {
        crate::driver::apply_air_backward_bundle(lowered, bundle, unit, false)
            .map_err(|error| vec![RunnerError::internal(error.span.unwrap_or_default(), error.message)])?
    } else {
        lowered
    };
    let policy = match kernel_lowering {
        KernelLowering::Device => RunnerKernelPolicy::Refuse,
        KernelLowering::Host => RunnerKernelPolicy::Execute,
    };
    run_entry_with_process_exit(&lowered.validated, host, policy)
}
