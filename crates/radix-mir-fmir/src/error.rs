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
        }
    }
}
