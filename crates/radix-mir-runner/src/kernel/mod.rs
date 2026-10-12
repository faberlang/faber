//! Kernel module dispatch for the MIR runner.
//!
//! Routes `MirProviderKind::Kernel` verbs to their submodule handlers after
//! operands are evaluated to Values. Owns the match dispatch; submodules own
//! their verb contracts.

pub(crate) mod args;
pub(crate) mod http;
mod json;
mod toml;

use super::{Host, RunnerError, Value};
use radix_lexer::Span;
use radix_mir::KernelModule;

/// Reset process-global kernel and leaf state that must not cross executions.
pub fn reset_execution_state() {
    faber_leaf::reset_execution_state();
    http::reset();
}

/// Dispatch a kernel verb after operands are evaluated to [`Value`]s.
///
/// # Errors
///
/// Returns an error when the kernel verb is unsupported or execution fails.
pub fn dispatch_kernel<H: Host + ?Sized>(
    _host: &mut H,
    module: KernelModule,
    verb: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, RunnerError> {
    match module {
        KernelModule::Processus => Err(RunnerError::unsupported(
            span,
            "kernel processus was deleted; norma:process runs through ad / faber-leaf",
        )),
        KernelModule::Json => json::dispatch(verb, args, span),
        KernelModule::Solum => Err(RunnerError::unsupported(
            span,
            "kernel solum was deleted; norma:fs runs through ad / faber-leaf",
        )),
        KernelModule::Aleator => Err(RunnerError::unsupported(
            span,
            "kernel aleator was deleted; norma:random runs through ad / faber-leaf",
        )),
        KernelModule::Consolum => Err(RunnerError::unsupported(
            span,
            "kernel consolum was deleted; norma:console runs through ad / faber-leaf",
        )),
        KernelModule::Toml => toml::dispatch(verb, args, span),
    }
}
