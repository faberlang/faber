//! Faber exempla harness crate.
//!
//! Public `.fab` corpora live in sibling repos after the org split:
//! - keyword / language reference → `examples/corpus/`
//! - GPU / AIR / script tracks → `examples/{gpu-workload,air,script-kernel}/`
//! - Norma stdlib tours → `norma/exempla/`
//!
//! Slow end-to-end harnesses live in `exempla_e2e` and the `e2e_harness`
//! integration test binary (`tests/e2e_harness.rs`).

pub mod paths;

/// Path to the language keyword corpus root (resolved at runtime).
///
/// Prefer [`paths::corpus_dir`] for new code. This constant remains only as a
/// compile-time placeholder for the local pointer directory; harnesses must
/// call [`paths::corpus_dir`].
#[deprecated(note = "use exempla::paths::corpus_dir() — corpus lives in examples/corpus")]
pub const CORPUS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/corpus");

/// Shared exempla harness helpers and fast regression tests (parity, matrix).
#[cfg(test)]
pub mod exempla_e2e;
