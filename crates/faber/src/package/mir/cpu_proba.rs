//! The CPU device session for proba cases (`faber test --device cpu`,
//! gpu-reset rule 16.9).
//!
//! A proba case is one program execution, and a session dispatcher tears its
//! session down when the program ends, so each case runs on its own CPU
//! session: the same image, dispatcher and worker pool `faber run --device
//! cpu` builds, opened without reporting the discovery receipt.

use super::{
    CpuDeviceShape, cpu_device_registry, default_cpu_workers, lowered_declares_kernels,
    open_device_session, session_dispatcher,
};
use radix::module::diagnostics::Diagnostic;
use radix::module::mir::LoweredMirUnit;
use radix::program::mir::DeviceSelection;
use radix_mir_runner::kernel_dispatch::ResidencyHook;
use std::num::NonZeroU32;
use std::path::Path;

/// Whether the lowered unit declares a kernel: a unit without one has
/// nothing to dispatch, and its cases run on the reference route.
#[must_use]
pub fn declares_kernels(lowered: &LoweredMirUnit<'_>) -> bool {
    lowered_declares_kernels(lowered)
}

/// Open a fresh CPU device session over `lowered` and return the kernel
/// dispatcher of one case execution. `workers` is the resolved worker count
/// (`--threads N` or `[device] threads`); absent, half the available cores.
///
/// # Errors
/// Diagnostics when the CPU device does not open, or when no kernel of the
/// unit lowers to it (`E_DEVICE_NONE_ADMITTED`).
pub fn open_case_dispatcher(
    input: &Path,
    lowered: &LoweredMirUnit<'_>,
    workers: Option<NonZeroU32>,
) -> Result<ResidencyHook, Vec<Diagnostic>> {
    let workers = workers.or_else(default_cpu_workers);
    let mut registry = cpu_device_registry(workers);
    // `faber test` has no `--partitions`: one partition.
    let shape = CpuDeviceShape {
        workers,
        partitions: NonZeroU32::MIN,
    };
    let admitted = open_device_session(input, DeviceSelection::Cpu, &mut registry, shape)?;
    let built = session_dispatcher(input, admitted, lowered, &lowered.validated)?;
    let hook: ResidencyHook = built.dispatcher;
    Ok(hook)
}

/// [`super::with_linked_test_package_mir`] handing the whole lowered unit to
/// `run`, so a CPU session can build its device image from it.
///
/// # Errors
/// Diagnostics when the package does not link or lower.
pub fn with_linked_test_package_lowered_mir<R>(
    package: radix::program::analyze::AnalyzedProgram,
    scope: &radix::program::mir::TestPackageScope,
    config: &radix::module::driver::Config,
    run: impl for<'a> FnOnce(&LoweredMirUnit<'a>, &radix::program::mir::TestFunctionIds) -> R,
) -> Result<R, Vec<Diagnostic>> {
    radix::program::mir::with_linked_test_package_lowered_mir(
        package,
        scope,
        config,
        &super::faber_mir_seams(),
        run,
    )
}
