//! Shared ABI metadata for MIR-backed systems targets.
//!
//! TARGET: low-level emitters that need callable, runtime-helper, or kernel
//! resource facts before choosing target syntax. This module records the shape
//! and purpose of ABI surfaces; target-specific spelling still belongs to the
//! target emitter.

mod broadcast;
mod contract;
mod error;
mod launch;
mod reflection;
mod resource;
mod signature;
#[cfg(test)]
mod tests;
mod types;

pub use broadcast::*;
pub use contract::*;
pub use error::*;
pub use launch::*;
pub use resource::*;
pub(crate) use signature::{grid_reduce_node, grid_reduce_partial_layout};
// Vocabulary types moved to radix-types (re-exported at the crate root).
pub use crate::{MirKernelAxis, MirKernelBuiltin};
pub use types::*;
