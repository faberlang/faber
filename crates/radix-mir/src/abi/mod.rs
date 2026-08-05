//! Shared ABI metadata for MIR-backed systems targets.
//!
//! TARGET: low-level emitters that need callable, runtime-helper, or kernel
//! resource facts before choosing target syntax. This module records the shape
//! and purpose of ABI surfaces; target-specific spelling still belongs to the
//! target emitter.

mod contract;
mod error;
mod launch;
mod reflection;
mod resource;
mod signature;
#[cfg(test)]
mod tests;
mod types;

pub use contract::*;
pub use error::*;
pub use launch::*;
pub use resource::*;
pub use types::*;
