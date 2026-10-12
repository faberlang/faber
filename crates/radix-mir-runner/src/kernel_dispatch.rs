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
use std::cell::RefCell;
use std::fmt;
use std::ops::Range;
use std::rc::Rc;

/// Opaque handle to a device buffer; the dispatcher owns its meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceToken(pub u64);

/// A half-open range of physical backing-element indices.
pub type ElementRange = Range<usize>;

/// Shared handle through which a tensor's host accessors reach the dispatcher
/// that owns its device buffer.
pub type ResidencyHook = Rc<RefCell<dyn KernelDispatcher>>;

/// A recoverable kernel error a delivery point surfaced: the constant text a
/// `⇥` kernel declared, and the open block the faulting launch was tagged with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceFault {
    /// The text delivered to the owner block's `cape` binding.
    pub text: String,
    /// The block id given to [`KernelDispatcher::set_launch_owner`] for the
    /// faulting launch (`None`: no block was open).
    pub owner: Option<u64>,
}

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

    /// Tag the launches of the next [`Self::dispatch`] with the innermost open
    /// handled block (`None`: no block is open). Called by the runner right
    /// before each dispatch; the tag comes back on a [`DeviceFault`].
    fn set_launch_owner(&mut self, _owner: Option<u64>) {}

    /// Take the first recoverable kernel error that a delivery point
    /// ([`Self::make_host_current`], [`Self::flush`], [`Self::finish`]) already
    /// surfaced, if any. This does no device work, so the runner may call it
    /// after every statement of a `fac omnia` body; the submit count does not
    /// change. A fatal fault never reaches here: it fails the delivery point.
    ///
    /// # Errors
    ///
    /// Returns a [`RunnerError`] when taking the fault fails.
    fn deliver_faults(&mut self) -> Result<Option<DeviceFault>, RunnerError> {
        Ok(None)
    }

    /// Make the host copy of `range` current for `token` (download only that
    /// range). Called before a host read of device-resident elements.
    ///
    /// # Errors
    ///
    /// Returns a [`RunnerError`] when the download fails.
    fn make_host_current(
        &mut self,
        _token: DeviceToken,
        _range: ElementRange,
    ) -> Result<(), RunnerError> {
        Ok(())
    }

    /// The host wrote to `token`'s tensor: the device copy is now stale and
    /// must be re-uploaded on its next device use.
    fn host_wrote(&mut self, _token: DeviceToken) {}

    /// Make every launched kernel complete and surface the first kernel fault
    /// in program order. Called at the delivery points that are not a host
    /// read: the `ad` effect boundary and program end. Never called per
    /// launch.
    ///
    /// # Errors
    ///
    /// Returns the first deferred device fault as a [`RunnerError`].
    fn flush(&mut self) -> Result<(), RunnerError> {
        Ok(())
    }

    /// Program end: [`Self::flush`], then release the device. Defaults to a
    /// plain flush.
    ///
    /// # Errors
    ///
    /// Returns the flush error, else the release error.
    fn finish(&mut self) -> Result<(), RunnerError> {
        self.flush()
    }
}

/// Device residency of one tensor storage, shared by every view of it.
struct Residency {
    token: DeviceToken,
    device_current: bool,
    /// Sorted, coalesced backing ranges whose host copy is current.
    host_current: Vec<ElementRange>,
    hook: ResidencyHook,
    error: Option<RunnerError>,
}

/// Residency marker stored beside a tensor's backing data. Absent (`None`)
/// unless a dispatcher marked the storage; absent means host-only behavior.
#[derive(Clone, Default)]
pub(crate) struct ResidencySlot(Rc<RefCell<Option<Residency>>>);

impl fmt::Debug for ResidencySlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResidencySlot")
    }
}

impl PartialEq for ResidencySlot {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

fn covered(ranges: &[ElementRange], want: &ElementRange) -> bool {
    ranges
        .iter()
        .any(|range| range.start <= want.start && want.end <= range.end)
}

fn insert_range(ranges: &mut Vec<ElementRange>, add: ElementRange) {
    ranges.push(add);
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<ElementRange> = Vec::with_capacity(ranges.len());
    for range in ranges.drain(..) {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    *ranges = merged;
}

impl ResidencySlot {
    pub(crate) fn mark(&self, token: DeviceToken, hook: ResidencyHook, host_current: bool) {
        *self.0.borrow_mut() = Some(Residency {
            token,
            device_current: true,
            #[allow(clippy::single_range_in_vec_init)] // one whole-buffer range
            host_current: if host_current {
                vec![0..usize::MAX]
            } else {
                Vec::new()
            },
            hook,
            error: None,
        });
    }

    pub(crate) fn state(&self) -> Option<(DeviceToken, bool)> {
        self.0
            .borrow()
            .as_ref()
            .map(|residency| (residency.token, residency.device_current))
    }

    pub(crate) fn take_error(&self) -> Option<RunnerError> {
        self.0
            .borrow_mut()
            .as_mut()
            .and_then(|residency| residency.error.take())
    }

    /// Host read of `range`: download it once if the device copy is the
    /// current one. Returns `false` when the download failed (the error is
    /// retained for [`Self::take_error`]).
    pub(crate) fn ensure_host(&self, range: ElementRange) -> bool {
        let (token, hook) = {
            let guard = self.0.borrow();
            let Some(residency) = guard.as_ref() else {
                return true;
            };
            if !residency.device_current || covered(&residency.host_current, &range) {
                return true;
            }
            (residency.token, Rc::clone(&residency.hook))
        };
        let result = match hook.try_borrow_mut() {
            Ok(mut dispatcher) => dispatcher.make_host_current(token, range.clone()),
            Err(_) => Err(RunnerError::internal(
                Span::default(),
                "device residency hook is busy during a host read",
            )),
        };
        let mut guard = self.0.borrow_mut();
        let Some(residency) = guard.as_mut() else {
            return true;
        };
        match result {
            Ok(()) => {
                insert_range(&mut residency.host_current, range);
                true
            }
            Err(error) => {
                residency.error = Some(error);
                false
            }
        }
    }

    /// Host write to a backing of `len` elements: the rest of the device data
    /// is downloaded first (the device copy is about to be abandoned), then
    /// the device copy is marked stale with one `host_wrote`.
    pub(crate) fn before_write(&self, len: usize) -> bool {
        if self.0.borrow().is_none() {
            return true;
        }
        if !self.ensure_host(0..len) {
            return false;
        }
        let (token, hook) = {
            let mut guard = self.0.borrow_mut();
            let Some(residency) = guard.as_mut() else {
                return true;
            };
            if !residency.device_current {
                return true;
            }
            residency.device_current = false;
            (residency.token, Rc::clone(&residency.hook))
        };
        if let Ok(mut dispatcher) = hook.try_borrow_mut() {
            dispatcher.host_wrote(token);
        }
        true
    }
}
