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

use handroll::{FxHashMap, FxHashSet};
pub use nodes::*;
pub use radix_syntax::KernelTier;
pub use radix_types::{DefId, TypeId};
pub use tensor_edge_policy::{
    ResolvedTensorEdgePolicy, TensorEdgePolicyProviderImpl, TensorEdgePolicyRequirement,
    TensorEdgePolicyTable, TensorShiftContext,
};

/// Resolved `ordo` order rows for enum ordering comparisons.
///
/// Operator ruling 2026-10-15: `≺ ≻ ≤ ≥` order two values of
/// one `ordo` by their declared discriminant, not by declaration position.
/// Typecheck resolves each member's discriminant and records the ordered rows
/// keyed by the comparison's left-operand HIR id; every backend lowers the same
/// decision (an integer comparison over the declared values).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrdoOrderTable {
    /// Ordered `(variant, declared discriminant)` rows per comparison site.
    pub comparisons: FxHashMap<HirId, Vec<(DefId, i64)>>,
}

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
