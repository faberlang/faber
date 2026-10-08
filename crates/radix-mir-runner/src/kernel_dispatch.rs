//! Kernel-dispatch seam for [`RunnerKernelPolicy::Dispatch`](crate::RunnerKernelPolicy).
//!
//! A call to a compute-stage (`@ kernel`) function is a pure call with
//! synchronous semantics. Under `Dispatch` the runner does not refuse the
//! call and does not execute the body: it evaluates the arguments and hands
//! the call to a [`KernelDispatcher`], then continues with the dispatcher's
//! result. This module is the seam only; it holds no device code.

use crate::{RunnerError, Value};
use radix_lexer::Span;
use radix_mir::MirFunctionId;

/// Executes a kernel call on behalf of the runner.
pub trait KernelDispatcher {
    /// Run `function` with the evaluated `args`.
    ///
    /// A `mut` parameter's argument is written back to the caller's place
    /// after the call returns, so the dispatcher mutates `args` in place
    /// (a tensor handle, or a replaced scalar). The returned value is the
    /// call's value; `None` is the unit value. A returned error stops the run
    /// and carries the dispatcher's issue code.
    ///
    /// # Errors
    ///
    /// Returns a [`RunnerError`] when the dispatch fails.
    fn dispatch(
        &mut self,
        function: MirFunctionId,
        args: &mut [Value],
        span: Span,
    ) -> Result<Option<Value>, RunnerError>;
}
