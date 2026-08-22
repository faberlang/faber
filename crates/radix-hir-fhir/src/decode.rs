//! Deterministic `postcard` codec for the FHIR artifact.
//!
//! [`encode`] serializes a [`HirArtifact`]; [`decode`] deserializes it with
//! the schema-version gate (F3) and the referential-integrity validation
//! gate (U2.3). Both paths are fail-closed: no panic on the load path (F7).

use crate::artifact::{HirArtifact, SCHEMA_VERSION};
use crate::error::HirArtifactError;
use crate::validate::validate_referential_integrity;

/// Serialize a [`HirArtifact`] to deterministic `postcard` bytes.
///
/// # Errors
///
/// Returns `postcard::Error` if serialization fails (e.g. unsupported data
/// shapes); serialization of a valid artifact is expected to succeed.
pub fn encode(artifact: &HirArtifact) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(artifact)
}

/// Deserialize and validate a [`HirArtifact`] from `postcard` bytes.
///
/// # Schema versioning (F3)
///
/// `schema_version` is the first wire field, so the version prefix is read
/// before the payload is interpreted and any mismatch — forward or backward —
/// is rejected with [`HirArtifactError::UnsupportedSchemaVersion`]. There is
/// no compatibility decoder for earlier schemas.
///
/// # Fail-closed (F7)
///
/// Malformed postcard data, truncation, and referential-integrity violations
/// return a structured [`HirArtifactError`]. This function never panics.
///
/// # Errors
///
/// Returns [`HirArtifactError::Truncated`] on truncated input,
/// [`HirArtifactError::Malformed`] on postcard decoding failures,
/// [`HirArtifactError::UnsupportedSchemaVersion`] on a version mismatch,
/// and the validation errors from the referential-integrity gate (U2.3).
pub fn decode(bytes: &[u8]) -> Result<HirArtifact, HirArtifactError> {
    // F3: read the schema-version prefix (first wire field) and reject any
    // mismatch before interpreting the payload. A schema-v1 artifact is
    // rejected here with UnsupportedSchemaVersion — never a confusing
    // field-layout error.
    let (schema_version, _rest) = postcard::take_from_bytes::<u32>(bytes).map_err(|e| {
        if matches!(e, postcard::Error::DeserializeUnexpectedEnd) {
            HirArtifactError::Truncated
        } else {
            HirArtifactError::Malformed(e)
        }
    })?;
    if schema_version != SCHEMA_VERSION {
        return Err(HirArtifactError::UnsupportedSchemaVersion {
            found: schema_version,
            expected: SCHEMA_VERSION,
        });
    }

    // Payload interpretation begins only after the version prefix is accepted.
    let artifact: HirArtifact = postcard::from_bytes(bytes).map_err(|e| {
        if matches!(e, postcard::Error::DeserializeUnexpectedEnd) {
            HirArtifactError::Truncated
        } else {
            HirArtifactError::Malformed(e)
        }
    })?;

    // U2.3: Referential-integrity gate. Validate every arena index before
    // returning Ok — no codegen touch after a DanglingIndex (CSO E8).
    let type_count = artifact.types.type_count();
    let interner_len = artifact.interner.len();
    validate_referential_integrity(&artifact, type_count, interner_len)?;

    Ok(artifact)
}

#[cfg(test)]
#[path = "decode_test.rs"]
mod tests;
