//! Derived wire-shape hash of the FHIR artifact.
//!
//! The artifact is positional `postcard`, so any change to a serialized
//! struct or enum (a field added, removed or reordered, a variant added or
//! reordered, a field type changed) silently shifts every later byte. The
//! exact [`SCHEMA_VERSION`](crate::artifact::SCHEMA_VERSION) gate only helps
//! when someone remembers to bump it. This module closes that gap without a
//! hand-maintained value: the hash is *derived* from a structural description
//! of [`HirArtifact`] obtained by tracing its `Deserialize` implementation
//! with [`serde_reflection`], so the hash moves exactly when the serialized
//! shape moves, and nobody edits a constant.
//!
//! The artifact header carries the hash beside the version; decode rejects an
//! artifact whose hash differs from the reader's
//! ([`HirArtifactError::UnsupportedWireShape`](crate::HirArtifactError)).
//! In practice a shape change then fails closed in any reader built from the
//! same sources, and the `SCHEMA_VERSION` bump remains the human-readable
//! record of *why* the shape moved.
//!
//! The description covers type names, field names and order, and enum variant
//! indices and payloads, for every type reachable from the root. It is
//! conservative: a rename that is wire-neutral for `postcard` still changes
//! the hash, because the named-field `MessagePack` package wire depends on
//! names.

use crate::artifact::HirArtifact;
use serde::de::DeserializeOwned;
use serde_reflection::{Error as ReflectionError, Registry, Samples, Tracer, TracerConfig};
use std::sync::OnceLock;

/// Upper bound on completion rounds. A round resolves one layer of enums that
/// sit inside other incomplete enums; real shapes converge in a handful of
/// rounds, so this ceiling only stops a pathological type from looping.
const MAX_ROUNDS: usize = 64;

/// Structural description of `T`'s serialized shape: every reachable
/// container with its fields in order and every enum variant.
///
/// `serde_reflection` completes an enum's variant list only when the enum is
/// itself the traced root (`Tracer::trace_type`); an enum met inside a struct
/// is left incomplete and the tracer revisits only variant 0 until its mark is
/// cleared. This function applies the same clearing to every nested enum
/// without a hand-kept list of enum types: each round traces `T` again on a
/// fresh tracer after replaying the earlier rounds, reads the names of the
/// enums that are still incomplete from the registry check, and records them
/// so the next round clears their marks after the pass that flagged them.
fn trace_registry<T: DeserializeOwned>() -> Result<Registry, ReflectionError> {
    let samples = Samples::new();
    // `cleared[r]` holds the enum names whose incomplete mark is cleared after
    // pass `r`, exactly as `Tracer::trace_type` does for a root enum.
    let mut cleared: Vec<Vec<String>> = Vec::new();
    for _ in 0..MAX_ROUNDS {
        let mut tracer = Tracer::new(TracerConfig::default());
        for names in &cleared {
            tracer.trace_type_once::<T>(&samples)?;
            for name in names {
                tracer.check_incomplete_enum(name);
            }
        }
        tracer.trace_type_once::<T>(&samples)?;
        match tracer.registry() {
            Ok(registry) => return Ok(registry),
            Err(ReflectionError::MissingVariants(names)) => cleared.push(names),
            Err(error) => return Err(error),
        }
    }
    Err(ReflectionError::Custom(format!(
        "wire shape did not converge in {MAX_ROUNDS} tracing rounds"
    )))
}

/// First eight bytes (big-endian) of the SHA-256 of the traced description.
///
/// # Errors
///
/// Returns the tracing error when `T`'s shape cannot be described, or a
/// description-encoding error rendered as a custom tracing error.
pub(crate) fn shape_hash_of<T: DeserializeOwned>() -> Result<u64, ReflectionError> {
    let registry = trace_registry::<T>()?;
    // The registry is a `BTreeMap`, and every nested map inside it is ordered
    // too, so this encoding is deterministic across processes and platforms.
    let description = postcard::to_allocvec(&registry)
        .map_err(|error| ReflectionError::Custom(format!("registry encoding failed: {error}")))?;
    let digest = handroll::sha256::sha256(&description);
    let mut head = [0u8; 8];
    head.copy_from_slice(&digest[..8]);
    Ok(u64::from_be_bytes(head))
}

/// Hash of the wire shape this build reads and writes, derived once per
/// process from the live type definitions of [`HirArtifact`].
///
/// If the shape cannot be traced (a property of the type definitions, never
/// of an input; the crate's tests assert the real artifact traces) this
/// returns `0`, a value no successful derivation produces in practice and
/// which the `artifact_shape_is_traceable` test turns into a failure.
#[must_use]
pub fn wire_shape_hash() -> u64 {
    static HASH: OnceLock<u64> = OnceLock::new();
    *HASH.get_or_init(|| shape_hash_of::<HirArtifact>().unwrap_or(0))
}

#[cfg(test)]
#[path = "shape_test.rs"]
mod tests;
