//! High-level intermediate representation nodes (foundation crate).
//!
//! Owns HIR data structures and visitors. HIR *lowering* remains in package
//! `radix` (`radix::hir::lower`) until the resolver / semantic pass surface is
//! extracted. Package `radix` re-exports this crate as `radix::hir` for path
//! stability.
//!
//! [`DefId`] is defined in `radix-types` and re-exported here so existing
//! `hir::DefId` paths keep working.

pub mod capture;
pub mod failable;
mod nodes;
#[cfg(test)]
mod nodes_test;
#[cfg(test)]
mod presentation_test;
pub mod tails;
pub mod tensor_edge_policy;
mod unavailable;
pub mod visit;

pub use nodes::*;
pub use radix_types::{DefId, TypeId};
use rustc_hash::{FxHashMap, FxHashSet};
pub use tensor_edge_policy::{
    ResolvedTensorEdgePolicy, TensorEdgePolicyProviderImpl, TensorEdgePolicyRequirement,
    TensorEdgePolicyTable, TensorShiftContext,
};

/// Function facts installed by analyzed HIR consumers at the codegen boundary.
///
/// Every non-Rust HIR backend consumes the same semantic fact surface. Keeping
/// this carrier in the neutral HIR crate prevents each backend from inventing a
/// nominally different wrapper for the same analyzed table.
#[derive(Default, Clone)]
pub struct FailableFacts {
    /// Definitions whose return value carries an alternate exit.
    pub failable: FxHashSet<DefId>,
    /// Per-definition error type overrides.
    pub err_tys: FxHashMap<DefId, TypeId>,
    /// Definitions whose body is asynchronous or generator-shaped.
    pub async_defs: FxHashSet<DefId>,
}
