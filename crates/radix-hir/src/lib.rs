//! High-level intermediate representation nodes (foundation crate).
//!
//! Owns HIR data structures and visitors. HIR *lowering* remains in package
//! `radix` (`radix::hir::lower`) until the resolver / semantic pass surface is
//! extracted. Package `radix` re-exports this crate as `radix::hir` for path
//! stability.
//!
//! [`DefId`] is defined in `radix-types` and re-exported here so existing
//! `hir::DefId` paths keep working.

mod nodes;
#[cfg(test)]
mod presentation_test;
pub mod visit;

pub use nodes::*;
pub use radix_types::DefId;
