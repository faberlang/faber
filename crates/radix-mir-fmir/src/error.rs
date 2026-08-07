//! Structured fail-closed error for FMIR image decode and validation.

use std::fmt;

/// Structured error for FMIR image decode and validation failures.
///
/// Every variant carries enough information for a caller (the `faber` package
/// layer) to render its own diagnostic with the established issue codes.
#[derive(Debug)]
pub enum FmirImageError {
    /// The payload is not valid TOML (text image) or postcard (binary image)
    /// for the image schema.
    Parse(String),

    /// The image carries an artifact version the loader does not support.
    /// Version ratchets are clean breaks; there is no compatibility decoder.
    UnsupportedVersion {
        /// Version encoded in the image.
        actual: u32,
        /// Version this loader accepts.
        expected: u32,
    },

    /// The image `target` tag does not match the format it was decoded as.
    WrongTarget {
        /// Target tag encoded in the image.
        found: String,
        /// Target tag the format requires (`fmir-text` or `fmir`).
        expected: &'static str,
    },

    /// The toolchain version recorded in the image does not match the
    /// expected compiler version (faber's crate version). A device payload is
    /// admitted only from the exact producing toolchain version (N1.7).
    UnsupportedToolchain {
        /// Toolchain version recorded in the image.
        found: String,
        /// Toolchain version the loader expects.
        expected: String,
    },

    /// A device runtime requirement is not a known `device:` capability
    /// (`device:metal`, `device:cuda`). Unknown `device:` requirements fail
    /// admission at package load (N1.7).
    UnsupportedRuntimeRequirement {
        /// The rejected requirement string.
        requirement: String,
    },

    /// A backend artifact's stored FNV-1a provenance hash does not match its
    /// blob. Provenance is verified on admission (A9); a tampered or
    /// miscomputed hash fails closed.
    ArtifactHashMismatch {
        /// Backend the mismatched artifact declares.
        backend: crate::schema::FmirDeviceBackend,
        /// Hash stored in the image.
        expected: String,
        /// Hash recomputed from the blob.
        actual: String,
    },

    /// The nested device-program wire version is unsupported. The nested
    /// version is admitted independently of the outer FMIR image version
    /// (Stage 3R S8/F7), before any field-level interpretation of the device
    /// section.
    UnsupportedDeviceProgramVersion {
        /// Version encoded in the device section.
        actual: u32,
        /// Version this loader accepts.
        expected: u32,
    },

    /// The device program's carried semantic facts violate a fail-closed
    /// rule (Stage 3R F1–F7): identity aliasing, generation errors, graph
    /// cycles, illegal roots, missing dependencies, or duplicate/
    /// contradictory relation rows. The detail carries the focused
    /// diagnostic.
    WireProgramInvalid {
        /// The focused rule violation.
        detail: String,
    },

    /// The nested distributed-section version is unsupported (MD2-W1 A7).
    /// The distributed section carries its OWN version ratchet, admitted
    /// fail-closed before any field-level interpretation — independently of
    /// the nested device-program wire version and the outer image version.
    UnsupportedDistributedSectionVersion {
        /// Version encoded in the distributed section.
        actual: u32,
        /// Version this loader accepts.
        expected: u32,
    },

    /// A distributed section violates a fail-closed rule (MD2-W1): a
    /// wire-level section check (facts that do not resolve against the
    /// single-device program, an unknown element-type spelling) or the
    /// fail-before-launch validation gate (MD2-V1, run on the reconstructed
    /// logical plan). The detail carries the focused diagnostic.
    DistributedSectionInvalid {
        /// The focused rule violation.
        detail: String,
    },

    /// The equivalence gate failed (MD2-W1, exit-gate bullet 4): admission
    /// re-derived `(program, declared placement)` and the canonical bytes do
    /// NOT match the actually delivered logical plan (reconstructed from the
    /// section). The delivered plan is stale or tampered — the gate inspects
    /// the delivered bytes, never a self-declared hash.
    DistributedSectionEquivalenceMismatch {
        /// The focused stale/tampered diagnostic.
        detail: String,
    },

    /// The section's declared `logical_distributed_plan_hash` does not match
    /// the hash recomputed over the delivered logical plan (MD2-W1
    /// cross-checks MD2-H1's hash domain). A stale self-declared hash fails
    /// closed.
    DistributedSectionHashMismatch {
        /// The hash stored in the section.
        expected: String,
        /// The hash recomputed from the delivered logical plan.
        recomputed: String,
    },

    /// The nested session-section version is unsupported (GI4-2 A7). The
    /// cadence/session section carries its OWN version ratchet, admitted
    /// fail-closed before any field-level interpretation — independently of
    /// the nested device-program wire version and the outer image version.
    UnsupportedSessionSectionVersion {
        /// Version encoded in the session section.
        actual: u32,
        /// Version this loader accepts.
        expected: u32,
    },

    /// A cadence/session section violates a fail-closed rule (GI4-2): a
    /// wire-level section check — a zero KV-layout dimension, a dtype outside
    /// the closed surface, a missing or duplicate workload mode, an
    /// unresolvable or duplicate session-input slot, a cadence that
    /// contradicts the carried program's buffer facts, or an observation
    /// cadence that contradicts a decode invocation mode. The detail carries
    /// the focused diagnostic naming the violated class and the exact
    /// failing fact.
    SessionSectionInvalid {
        /// The focused rule violation.
        detail: String,
    },
}

impl fmt::Display for FmirImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(detail) => write!(f, "{detail}"),
            Self::UnsupportedVersion { actual, expected } => {
                write!(
                    f,
                    "unsupported FMIR image version {actual}; expected {expected}"
                )
            }
            Self::WrongTarget { found, expected } => {
                write!(
                    f,
                    "FMIR image target must be `{expected}` (found `{found}`)"
                )
            }
            Self::UnsupportedToolchain { found, expected } => {
                write!(
                    f,
                    "unsupported FMIR image toolchain {found}; expected {expected}"
                )
            }
            Self::UnsupportedRuntimeRequirement { requirement } => write!(
                f,
                "unsupported FMIR device runtime requirement `{requirement}` (expected `device:metal` or `device:cuda`)"
            ),
            Self::ArtifactHashMismatch {
                backend,
                expected,
                actual,
            } => {
                let spelling = match backend {
                    crate::schema::FmirDeviceBackend::Metal => "metal",
                    crate::schema::FmirDeviceBackend::Cuda => "cuda",
                };
                write!(
                    f,
                    "FMIR device artifact hash mismatch for backend `{spelling}` (stored {expected}, recomputed {actual})"
                )
            }
            Self::UnsupportedDeviceProgramVersion { actual, expected } => {
                write!(
                    f,
                    "unsupported device-program wire version {actual}; expected {expected}"
                )
            }
            Self::WireProgramInvalid { detail } => {
                write!(f, "invalid device program: {detail}")
            }
            Self::UnsupportedDistributedSectionVersion { actual, expected } => {
                write!(
                    f,
                    "unsupported distributed-section version {actual}; expected {expected}"
                )
            }
            Self::DistributedSectionInvalid { detail } => {
                write!(f, "invalid distributed section: {detail}")
            }
            Self::DistributedSectionEquivalenceMismatch { detail } => {
                write!(f, "distributed plan mismatch: {detail}")
            }
            Self::DistributedSectionHashMismatch { expected, recomputed } => {
                write!(
                    f,
                    "distributed-section logical-plan hash {expected} does not match the delivered plan (recomputed {recomputed})"
                )
            }
            Self::UnsupportedSessionSectionVersion { actual, expected } => {
                write!(
                    f,
                    "unsupported session-section version {actual}; expected {expected}"
                )
            }
            Self::SessionSectionInvalid { detail } => {
                write!(f, "invalid session section: {detail}")
            }
        }
    }
}
