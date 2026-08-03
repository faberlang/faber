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
    /// expected compiler version (faber's crate version).
    UnsupportedToolchain {
        /// Toolchain version recorded in the image.
        found: String,
        /// Toolchain version the loader expects.
        expected: String,
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
        }
    }
}
