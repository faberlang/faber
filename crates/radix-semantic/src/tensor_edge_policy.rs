//! Compatibility exports for tensor edge-policy semantic facts.
//!
//! The canonical table and identity types live in `radix-hir` because HIR
//! backends consume them directly.

pub use radix_hir::{
    ResolvedTensorEdgePolicy, TensorEdgePolicyProviderImpl, TensorEdgePolicyRequirement,
    TensorEdgePolicyTable, TensorShiftContext,
};
