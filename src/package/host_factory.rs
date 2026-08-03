//! The faber host factory (differentiable-GPU campaign S1-5; N1.5).
//!
//! Host ownership (architecture record §5 / delivery spec N1.5) is split:
//! **hosts** owns the native Metal/CUDA sessions and the composite host
//! (`faber-host-macos-arm64`), and **faber** owns the host factory — one
//! deliberate host-construction policy across the FHIR / FMIR / `fmir-bin` /
//! image-runner routes. This module is that factory: every product route
//! resolves its backend through [`resolve_backend_selection`] (the composite
//! host's single decision), constructs its host through
//! [`construct_composite_host`], and reports an A9-style discovery receipt
//! (selected device + artifact hash) when a device session is selected.
//!
//! # The policy (N1.1 / N1.4)
//!
//! - `auto` + no device program → CPU-only route, unchanged;
//! - `auto` + device program → exactly one admitted backend is selected; zero
//!   or more than one fails closed (`E_BACKEND_UNAVAILABLE`) with the
//!   candidates named and the explicit flag required;
//! - explicit `metal`/`cuda` + no device program → `E_NO_DEVICE_PROGRAM`
//!   ("package has no device program");
//! - explicit backend not admitted on the machine → `E_BACKEND_UNAVAILABLE`
//!   before any launch; an explicit GPU request never silently falls back.
//!
//! Every failure is a structured diagnostic (code + issue + named args),
//! never a panic and never a silent CPU fallback. The resolution decision is
//! pure over the injected `admitted` list so every branch is testable without
//! hardware; the product path probes the machine via
//! [`admitted_backends`].

use faber::device::{DeviceBackend, DeviceSelection};
use faber_host_macos_arm64::composite_host::{
    resolve_device_selection, CompositeHost, CompositeHostConfig, DeviceExecutionReceipt,
};
use faber_host_macos_arm64::device_descriptor::DeviceDescriptor;
use faber_host_macos_arm64::HostError;
use radix::diagnostics::Diagnostic;
use std::collections::BTreeMap;

/// Stable host error surface re-exported for the CLI (N1.4). The codes live
/// in `faber-host-macos-arm64`; faber re-exports them so route diagnostics
/// and tests share one spelling.
#[allow(unused_imports)]
pub use faber_host_macos_arm64::device_descriptor::{
    E_BACKEND_UNAVAILABLE, E_DEVICE_ABI_MISMATCH, E_DEVICE_DESCRIPTOR, E_DEVICE_DTYPE_MISMATCH,
    E_DEVICE_ENTRY_MISMATCH, E_DEVICE_SHAPE_MISMATCH, E_NO_DEVICE_PROGRAM,
};

/// Map a host-side failure to a faber structured diagnostic (N1.4): the
/// stable host code becomes the structured `issue` and a named `code` arg so
/// programmatic consumers see the same stable surface the composite host
/// reports. Never a panic and never a silent CPU fallback.
#[allow(dead_code)] // S1-6 consumption seam + descriptor-row tests.
#[must_use]
pub fn host_error_diagnostic(error: &HostError) -> Diagnostic {
    Diagnostic::error(&error.message)
        .with_arg("issue", error.code.clone())
        .with_arg("code", error.code.clone())
}

/// **The one host-construction decision** (N1.1 auto rule + N1.4 table).
///
/// Pure over the injected `admitted` list so every branch is testable without
/// hardware. Returns `None` for the CPU-only route and `Some(backend)` when a
/// device session must be constructed; every failure is a structured
/// diagnostic and never a CPU fallback.
///
/// # Errors
/// - `E_BACKEND_UNAVAILABLE` — `auto` cannot resolve (zero or more than one
///   admitted backend) or an explicit backend is not admitted;
/// - `E_NO_DEVICE_PROGRAM` — an explicit backend was requested on a route
///   whose package carries no device program.
pub fn resolve_backend_selection(
    selection: DeviceSelection,
    requires_device: bool,
    admitted: &[DeviceBackend],
) -> Result<Option<DeviceBackend>, Diagnostic> {
    resolve_device_selection(selection, requires_device, admitted).map_err(|error| {
        Diagnostic::error(&error.message)
            .with_arg("issue", error.code.clone())
            .with_arg("code", error.code.clone())
            .with_arg("selection", selection.spelling().to_owned())
            .with_arg("requires_device", requires_device.to_string())
    })
}

/// Probe the machine for admitted native backends (discovery receipts).
#[must_use]
pub fn admitted_backends() -> Vec<DeviceBackend> {
    faber_host_macos_arm64::composite_host::admitted_backends()
}

/// Resolve the effective backend selection (N1.1): CLI `--backend` >
/// manifest `[device] backend` > default `auto`. Both inputs arrive already
/// parsed (the CLI flag from `RunArgs`, the manifest value through
/// [`crate::package::manifest_backend_selection`]); this function is the
/// frozen precedence fold every route applies.
#[allow(dead_code)] // consumed by the bin `commands/run` route + lib tests.
#[must_use]
pub fn effective_backend_selection(
    cli_backend: Option<DeviceSelection>,
    manifest_backend: Option<DeviceSelection>,
) -> DeviceSelection {
    cli_backend
        .or(manifest_backend)
        .unwrap_or(DeviceSelection::Auto)
}

/// Construct the composite host under the one host-construction policy
/// (product path; live admission probes). Opens the device session
/// (fail-closed) or returns the CPU-only host.
///
/// # Errors
/// - `E_BACKEND_UNAVAILABLE` — the resolved backend cannot be opened;
/// - `E_NO_DEVICE_PROGRAM` — explicit backend on a payload-less route.
#[allow(dead_code)] // S1-6 launches the device route through this seam.
pub fn construct_composite_host(
    selection: DeviceSelection,
    requires_device: bool,
) -> Result<CompositeHost, Diagnostic> {
    CompositeHost::new(CompositeHostConfig {
        selection,
        requires_device,
    })
    .map_err(|error| host_error_diagnostic(&error))
}

/// Execute a typed device descriptor through the composite host's device
/// session.
///
/// Fail-before-launch: a CPU-only host, a wrong-backend or structurally bad
/// descriptor, an ABI/dtype/shape conflict, or an unknown kernel entry all
/// fail with typed diagnostics **before any launch** (N1.4). Returns an A9
/// receipt when the lifecycle completes.
///
/// # Errors
/// - `E_NO_DEVICE_PROGRAM` — no device session on this host;
/// - `E_DEVICE_DESCRIPTOR` — wrong-backend or structurally bad descriptor;
/// - `E_DEVICE_ABI_MISMATCH` / `E_DEVICE_DTYPE_MISMATCH` /
///   `E_DEVICE_SHAPE_MISMATCH` / `E_DEVICE_ENTRY_MISMATCH` — typed conflicts;
/// - session-level failures bubble through unchanged.
#[allow(dead_code)] // S1-6 launches the device route through this seam.
pub fn execute_device_descriptor(
    host: &mut CompositeHost,
    descriptor: &DeviceDescriptor,
    inputs: &BTreeMap<u32, Vec<f32>>,
    outputs: &[u32],
) -> Result<DeviceExecutionReceipt, Diagnostic> {
    host.execute_descriptor(descriptor, inputs, outputs)
        .map_err(|error| host_error_diagnostic(&error))
}

/// A9-style discovery receipt: the selected device + artifact hash the
/// ordinary `faber run` command reports when it selects a device backend
/// (Stage 1 gate wording).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDiscoveryReceipt {
    /// Selected backend.
    pub backend: DeviceBackend,
    /// Selected-hardware name from the admission probe.
    pub device_name: String,
    /// FNV-1a provenance hash of the selected backend's declared artifact
    /// (verified against its blob at image admission).
    pub artifact_hash: String,
}

impl BackendDiscoveryReceipt {
    /// Render the discovery receipt line (selected device + artifact hash).
    pub fn print(&self) {
        println!(
            "device: selected backend `{}` on {} (artifact {})",
            self.backend.spelling(),
            self.device_name,
            self.artifact_hash
        );
    }
}

/// Build the discovery receipt for a resolved backend from the live
/// admission probes and the image's declared device artifacts.
///
/// `None` when the resolved backend declares no artifact in the image — the
/// caller fails closed (a device program without a declared artifact for its
/// selected backend is a missing descriptor, N1.4).
#[must_use]
pub fn discovery_receipt(
    backend: DeviceBackend,
    artifacts: &[radix_mir_fmir::FmirDeviceArtifact],
) -> Option<BackendDiscoveryReceipt> {
    let artifact = artifacts.iter().find(|artifact| {
        matches!(
            (&artifact.backend, backend),
            (radix_mir_fmir::FmirDeviceBackend::Metal, DeviceBackend::Metal)
                | (radix_mir_fmir::FmirDeviceBackend::Cuda, DeviceBackend::Cuda)
        )
    })?;
    Some(BackendDiscoveryReceipt {
        backend,
        device_name: backend_device_name(backend),
        artifact_hash: artifact.hash.clone(),
    })
}

/// Selected-hardware name for A9 receipts from the admission probe.
#[must_use]
pub fn backend_device_name(backend: DeviceBackend) -> String {
    match backend {
        DeviceBackend::Metal => {
            faber_host_macos_arm64::probe_metal_environment()
                .mtl_device
                .unwrap_or_else(|| "metal".to_owned())
        }
        DeviceBackend::Cuda => faber_host_macos_arm64::probe_cuda_environment()
            .nvidia_smi
            .unwrap_or_else(|| "cuda".to_owned()),
    }
}

/// Fail-closed diagnostic for a device program with no executable descriptor
/// (N1.4 "bad/missing device descriptor" row). Never a silent CPU fallback.
///
/// The S1-3 artifact/descriptor pipeline is not wired into this CLI yet; a
/// device-bearing image resolves its backend and reports its discovery
/// receipt, then fails closed here instead of running the CPU program.
#[must_use]
pub fn missing_device_descriptor(backend: DeviceBackend) -> Diagnostic {
    Diagnostic::error(format!(
        "device program for backend `{}` has no executable descriptor; device execution is not wired yet",
        backend.spelling()
    ))
    .with_arg("issue", E_DEVICE_DESCRIPTOR.to_owned())
    .with_arg("code", E_DEVICE_DESCRIPTOR.to_owned())
    .with_arg("backend", backend.spelling().to_owned())
}

/// Fail-closed diagnostic for a device program that declares no artifact for
/// the selected backend (N1.4 "bad/missing device descriptor" row).
#[must_use]
pub fn missing_backend_artifact(backend: DeviceBackend) -> Diagnostic {
    Diagnostic::error(format!(
        "device program declares no artifact for selected backend `{}`",
        backend.spelling()
    ))
    .with_arg("issue", E_DEVICE_DESCRIPTOR.to_owned())
    .with_arg("code", E_DEVICE_DESCRIPTOR.to_owned())
    .with_arg("backend", backend.spelling().to_owned())
}

#[cfg(test)]
#[path = "host_factory_test.rs"]
mod tests;
