//! Placement execution contract trait for multi-host placements.
//!
//! The [`PlacementHost`] trait is the canonical Rust definition of the
//! placement execution contract. Every host runtime must implement these four
//! operations with identical ordering guarantees and error semantics.
//!
//! The trait is host-agnostic: it names the operations every host provides
//! without referencing any specific backend, runtime, or hardware target.

/// Placement execution contract: every host must implement these four
/// operations with identical ordering guarantees and error semantics.
///
/// The contract is host-agnostic — it does not reference any specific
/// backend, runtime, or hardware target. Hosts implement the trait; the
/// trait does not know about host details.
///
/// # Associated types
///
/// Each associated type models a concept that every host must provide:
///
/// | Type               | Concept                                            |
/// |--------------------|----------------------------------------------------|
/// | `HostBuffer`       | Buffer whose data lives in host-accessible memory   |
/// | `DeviceBuffer`     | Buffer whose data lives in device-accessible memory |
/// | `CompiledKernel`   | A kernel that has been compiled and is ready to run |
/// | `BufferBinding`    | A binding from a buffer slot to a concrete buffer   |
pub trait PlacementHost {
    /// The type modeling host-accessible buffers.
    type HostBuffer;

    /// The type modeling device-accessible buffers.
    type DeviceBuffer;

    /// The type modeling a compiled, dispatchable kernel.
    type CompiledKernel;

    /// The type modeling a buffer-to-slot binding for kernel dispatch.
    type BufferBinding;

    /// Copy data from a host buffer to a device buffer.
    ///
    /// # Preconditions
    ///
    /// * `buffer` is materialized in host-accessible memory.
    /// * `device_buffer` exists on the device with a size and element type
    ///   matching `buffer`.
    ///
    /// # Postconditions
    ///
    /// * The device buffer holds a copy of the host data.
    /// * The host may mutate or drop the source buffer without affecting the
    ///   device buffer.
    ///
    /// # Ordering
    ///
    /// Must complete before any [`dispatch`](Self::dispatch) that uses the
    /// target `device_buffer`.
    ///
    /// # Errors
    ///
    /// * [`PlacementError::BufferSizeMismatch`] if the host and device
    ///   buffer sizes do not match.
    /// * [`PlacementError::DeviceLost`] if the device is no longer available.
    fn copy_in(
        &mut self,
        buffer: &Self::HostBuffer,
        device_buffer: &mut Self::DeviceBuffer,
    ) -> Result<(), PlacementError>;

    /// Dispatch a compiled kernel with the given buffer bindings.
    ///
    /// The host may mutate buffer contents in place during dispatch (e.g.,
    /// the kernel writes results directly into output buffers). Callers must
    /// pass a mutable binding slice so the host can obtain valid mutable
    /// pointers to buffer memory without forming `*mut` from a shared `&`.
    ///
    /// # Preconditions
    ///
    /// * All input buffers referenced by `bindings` have been copied in via
    ///   [`copy_in`](Self::copy_in).
    /// * `kernel` has been compiled and is ready to execute.
    ///
    /// # Postconditions
    ///
    /// * The kernel has executed on the device.
    /// * Any output buffers referenced by `bindings` contain the kernel's
    ///   results.
    ///
    /// # Ordering
    ///
    /// Must complete before any [`readback`](Self::readback) on the output
    /// buffers.
    ///
    /// # Errors
    ///
    /// * [`PlacementError::KernelNotFound`] if `kernel` has not been
    ///   compiled.
    /// * [`PlacementError::BindingMismatch`] if `bindings` do not match the
    ///   compiled kernel's signature.
    /// * [`PlacementError::RuntimeFailure`] if the kernel reports a
    ///   runtime execution failure.
    /// * [`PlacementError::DeviceLost`] if the device is no longer available.
    fn dispatch(
        &mut self,
        kernel: &Self::CompiledKernel,
        bindings: &mut [Self::BufferBinding],
    ) -> Result<(), PlacementError>;

    /// Copy data from a device buffer back to a host buffer.
    ///
    /// # Preconditions
    ///
    /// * `device_buffer` holds post-kernel data (or data written by another
    ///   device-side writer).
    /// * `host_buffer` has capacity at least equal to the device buffer's
    ///   size.
    ///
    /// # Postconditions
    ///
    /// * `host_buffer` holds a copy of the device data.
    /// * The device buffer is unchanged by the read.
    ///
    /// # Ordering
    ///
    /// Implicitly waits for device-side completion before making data
    /// visible to the host. This is a data-moving operation, not merely an
    /// ordering barrier.
    ///
    /// # Errors
    ///
    /// * [`PlacementError::BufferSizeMismatch`] if the host and device
    ///   buffer sizes do not match.
    /// * [`PlacementError::DeviceLost`] if the device is no longer available.
    fn readback(
        &mut self,
        device_buffer: &Self::DeviceBuffer,
        host_buffer: &mut Self::HostBuffer,
    ) -> Result<(), PlacementError>;

    /// Insert a device-side ordering barrier.
    ///
    /// # Preconditions
    ///
    /// * All prior queue submissions for the device's work are complete.
    ///
    /// # Postconditions
    ///
    /// * Subsequent operations on the device are ordered after all prior
    ///   operations.
    ///
    /// # Ordering
    ///
    /// Returns immediately after submitting the barrier. `sync` is a
    /// device-local ordering assertion; it does not block the host and does
    /// not move data. Host-visible waiting is a separate concern.
    ///
    /// # Errors
    ///
    /// * [`PlacementError::SyncTimeout`] if the barrier submission does not
    ///   complete within a host-defined timeout.
    /// * [`PlacementError::DeviceLost`] if the device is no longer available.
    fn sync(&mut self) -> Result<(), PlacementError>;
}

/// Errors that any [`PlacementHost`] implementation may return.
///
/// Placement is failable, not fatal: every placement operation returns a
/// `Result<(), PlacementError>`. The caller decides whether an error is
/// recoverable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementError {
    /// The device was lost or disconnected. No placement operation is valid
    /// on a lost device; the host must recreate the device and all resources
    /// before retrying.
    DeviceLost,
    /// The host and device buffer sizes do not match for a copy-in or
    /// readback operation. This is a caller error — fix the buffer
    /// descriptor and retry.
    BufferSizeMismatch,
    /// The kernel referenced by a dispatch operation has not been compiled.
    /// Compile the kernel before dispatching.
    KernelNotFound,
    /// The buffer bindings supplied to a dispatch operation do not match the
    /// compiled kernel's signature (wrong count, wrong types, or wrong
    /// access modes).
    BindingMismatch,
    /// The kernel reported a runtime execution failure via non-zero status
    /// code. This is distinct from [`BindingMismatch`], which is detected
    /// before dispatch, and from [`KernelNotFound`], which means the kernel
    /// was never compiled. The exact failure reason is host-specific.
    RuntimeFailure,
    /// A sync operation timed out before the barrier was acknowledged.
    /// Retry with a longer timeout or check device health.
    SyncTimeout,
}
