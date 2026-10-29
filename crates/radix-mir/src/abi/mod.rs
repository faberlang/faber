//! Shared ABI metadata for MIR-backed systems targets.
//!
//! TARGET: low-level emitters that need callable, runtime-helper, or kernel
//! resource facts before choosing target syntax. This module records the shape
//! and purpose of ABI surfaces; target-specific spelling still belongs to the
//! target emitter.

mod broadcast;
mod contract;
mod d64_fault;
mod error;
mod lane_index;
mod launch;
mod reflection;
mod resource;
mod signature;
#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
mod throw_sites;
mod types;

pub use broadcast::*;
pub use contract::*;
pub use d64_fault::{D64_FAULT_TEXT, d64_fault_site, is_d64_type};
pub use error::*;
pub use lane_index::{
    IndexGuard, LaneIndexProof, LaneWriteMap, LaneWriteSelector, UnprovenIndex,
    UnprovenVectorIndex, lane_write_maps, unproven_indexes, unproven_vector_indexes,
};
pub use launch::*;
pub use resource::*;
pub use signature::{ParameterAliases, grid_reduce_helper_tensors};
pub(crate) use signature::{
    append_gather_axis_extent_resources, append_matmul_runtime_extent_resources, grid_reduce_node,
    grid_reduce_partial_layout, scan_node, scan_partial_layout,
};
pub use throw_sites::{ThrowSite, throw_sites};
// Vocabulary types moved to radix-types (re-exported at the crate root).
pub use crate::{MirKernelAxis, MirKernelBuiltin};
pub use types::*;
