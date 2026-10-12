//! Kernel module dispatch for the MIR runner.
//!
//! Routes `MirProviderKind::Kernel` verbs to their submodule handlers after
//! operands are evaluated to Values. Owns the match dispatch; submodules own
//! their verb contracts.

mod aleator;
pub(crate) mod args;
mod consolum;
pub(crate) mod http;
mod json;
mod processus;
mod toml;

use super::{Host, RunnerError, Value};
use radix_lexer::Span;
use radix_mir::KernelModule;

/// Reset process-global kernel state that must not cross program executions.
pub fn reset_execution_state() {
    aleator::reset_seed();
    http::reset();
}

/// Dispatch a kernel verb after operands are evaluated to [`Value`]s.
///
/// # Errors
///
/// Returns an error when the kernel verb is unsupported or execution fails.
pub fn dispatch_kernel<H: Host + ?Sized>(
    host: &mut H,
    module: KernelModule,
    verb: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, RunnerError> {
    match module {
        KernelModule::Processus => processus::dispatch(host, verb, args, span),
        KernelModule::Json => json::dispatch(verb, args, span),
        KernelModule::Solum => Err(RunnerError::unsupported(
            span,
            "kernel solum was deleted; norma:fs runs through ad / faber-leaf",
        )),
        KernelModule::Aleator => aleator::dispatch(verb, args, span),
        KernelModule::Consolum => consolum::dispatch(host, verb, args, span),
        KernelModule::Toml => toml::dispatch(verb, args, span),
    }
}
