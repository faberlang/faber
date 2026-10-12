//! Residual kernel dispatch for leftover `MirProviderKind::Kernel` MIR.
//!
//! N2 deleted the runner-only I/O kernels and the json/toml kernels. Norma
//! runs through `ad` / `faber-leaf` (I/O) or as ordinary Faber (json). A
//! leftover Kernel call fails closed. HTTP is absent until N5.

pub(crate) mod args;

use super::{Host, RunnerError, Value};
use radix_lexer::Span;
use radix_mir::KernelModule;

/// Reset process-global kernel and leaf state that must not cross executions.
pub fn reset_execution_state() {
    faber_leaf::reset_execution_state();
}

/// Dispatch a leftover kernel verb. Every module is gone.
///
/// # Errors
///
/// Always returns unsupported: runner kernels were deleted.
pub fn dispatch_kernel<H: Host + ?Sized>(
    _host: &mut H,
    module: KernelModule,
    verb: &str,
    _args: &[Value],
    span: Span,
) -> Result<Value, RunnerError> {
    Err(RunnerError::unsupported(
        span,
        &format!(
            "kernel {} was deleted; {verb} is not a runner kernel",
            module.name()
        ),
    ))
}
