//! Verifier trait and result types — backend-neutral interface for
//! discharging aspect obligations.

use super::{HirAspectBackendKind, HirAspectReport};

/// Normalized evidence string produced by a verifier run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirAspectEvidence(pub String);

/// Outcome of checking a single verification obligation.
///
/// Carries the status, human-readable evidence, the counter-example trace
/// when one was produced, and the backend tag so that callers can aggregate
/// or display results without coupling to any specific verifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirAspectCheckResult {
    pub status: HirAspectCheckStatus,
    pub evidence: HirAspectEvidence,
    pub counterexample: Vec<String>,
    pub backend: HirAspectBackendKind,
}

/// Verification status for one obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirAspectCheckStatus {
    /// All checks passed.
    Proven,
    /// A violation was found.
    Failed,
    /// Internal checker failure.
    Error,
    /// Check exceeded the allowed deadline.
    Timeout,
    /// External tooling is not available.
    ToolchainMissing,
}

/// Backend-agnostic verifier over a complete aspect report.
///
/// Implementations map an [`HirAspectReport`] to a set of per-obligation
/// results via some verification strategy (e.g. Lean proof
/// checking).
pub trait HirAspectVerifier {
    /// Verify all obligations in `report` and return the check results.
    fn verify(&self, report: &HirAspectReport) -> Vec<HirAspectCheckResult>;
}
