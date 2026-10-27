//! Structured error type for FHIR artifact decode and validation failures.

use std::error::Error;
use std::fmt;

// ---------------------------------------------------------------------------
// HirArtifactError
// ---------------------------------------------------------------------------

/// Structured error for HIR artifact loading failures (F7: fail-closed).
///
/// Every variant carries enough information for a caller to render a
/// user-facing message without inspecting the raw artifact bytes.
#[derive(Debug)]
pub enum HirArtifactError {
    /// The binary payload is not valid `postcard`-encoded data.
    Malformed(postcard::Error),

    /// The payload ends before the deserializer could consume a complete
    /// [`crate::artifact::HirArtifact`].
    TruncatedArtifact,

    /// The artifact was produced by a schema version the current loader
    /// does not support (F3: ratchet — refuse forward and backward).
    UnsupportedSchemaVersion {
        /// Version encoded in the artifact.
        found: u32,
        /// Version this loader accepts.
        expected: u32,
    },

    /// The artifact's wire shape (types, field order, enum variants) differs
    /// from the shape this loader reads, although the schema version matched:
    /// a serialized type changed without a `SCHEMA_VERSION` bump. Positional
    /// `postcard` cannot decode such a payload, so it fails closed.
    UnsupportedWireShape {
        /// Shape hash encoded in the artifact.
        found: u64,
        /// Shape hash of this loader's own type definitions.
        expected: u64,
    },

    /// A serialized index references a slot that does not exist in the
    /// target arena (constructed during arena reconstruction in U2.3).
    DanglingIndex {
        /// Human-readable arena name (e.g. "interner", "types").
        kind: &'static str,
        /// Index value from the serialized payload.
        index: u32,
        /// Current size of the target arena.
        arena_size: usize,
    },

    /// A snapshot payload is structurally invalid (e.g. missing required
    /// entries, out-of-bounds references in arena contents).
    ///
    /// This is distinct from [`Self::Malformed`], which covers postcard-level
    /// deserialization failures, and from [`Self::DanglingIndex`], which covers
    /// a single out-of-bounds index read from an otherwise-valid arena.
    MalformedSnapshot {
        /// Human-readable arena name (e.g. "types", "interner").
        kind: &'static str,
        /// Description of the structural problem.
        detail: String,
    },

    /// The presentation sidecar (schema v2) is structurally invalid: a
    /// duplicate owner anchor or a duplicate attachment owner.
    ///
    /// Distinct from [`DanglingIndex`], which reports a single anchor or
    /// symbol that does not resolve; this variant reports an ownership
    /// uniqueness violation inside the presentation.
    InvalidPresentation {
        /// Stable machine-readable issue code.
        issue: &'static str,
        /// Anchor id involved in the violation.
        anchor: u32,
    },
}

impl fmt::Display for HirArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(inner) => write!(f, "malformed HIR artifact: {inner}"),
            Self::TruncatedArtifact => write!(f, "truncated HIR artifact: unexpected end of data"),
            Self::UnsupportedSchemaVersion { found, expected } => {
                write!(
                    f,
                    "unsupported HIR artifact schema version {found} (expected {expected})"
                )
            }
            Self::UnsupportedWireShape { found, expected } => {
                write!(
                    f,
                    "HIR artifact wire shape {found:#018x} differs from this build's {expected:#018x} \
                     (a serialized type changed without a schema version bump); rebuild the artifact"
                )
            }
            Self::DanglingIndex {
                kind,
                index,
                arena_size,
            } => {
                write!(f, "dangling {kind} index {index} (arena size {arena_size})")
            }
            Self::MalformedSnapshot { kind, detail } => {
                write!(f, "malformed {kind} snapshot: {detail}")
            }
            Self::InvalidPresentation { issue, anchor } => {
                write!(f, "invalid presentation: {issue} (anchor {anchor})")
            }
        }
    }
}

impl Error for HirArtifactError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Malformed(inner) => Some(inner),
            _ => None,
        }
    }
}
