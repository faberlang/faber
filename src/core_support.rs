//! Build-time embedded support payload for installed Faber builds.

/// Deterministic tar.zst bytes assembled exclusively from the checked-in
/// core-support manifest. Materialization is deliberately owned by Node B.
pub const ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/core-support.tar.zst"));

/// SHA-256 of [`ARCHIVE`] emitted by `build.rs`.
pub const SHA256: &str = env!("FABER_CORE_SUPPORT_SHA256");

/// Canonical per-file SHA-256 manifest emitted beside [`ARCHIVE`] by `build.rs`.
pub const FILE_MANIFEST: &str =
    include_str!(concat!(env!("OUT_DIR"), "/core-support.files.sha256"));

pub mod materialize;
