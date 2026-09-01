//! Hand-rolled primitives replacing small external crates in the default build
//! (removal-delivery-spec U5).
//!
//! std-only, zero dependencies — enforced by `manifest_is_std_only` (`src/lib_test.rs`).
//! Consumers: `entropy`/`uuid_v4` (U5b, replaces rand/getrandom in the runner),
//! `sha256` (U5c, replaces sha2), error `impl`s live at their call sites (U5d).

pub mod entropy;
pub use entropy::uuid_v4;
pub mod sha256;
pub use sha256::{Sha256, sha256, sha256_hex};

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
