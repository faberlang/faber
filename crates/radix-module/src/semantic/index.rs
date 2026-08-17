//! Re-exports index expression types for the semantic module's public API.
//!
//! This file is the re-export boundary for IndexExpr, IndexId, and IndexVar.
//! The canonical source is the `radix-types` foundation crate.

pub use radix_types::{IndexExpr, IndexId, IndexVar};

#[cfg(test)]
#[path = "index_test.rs"]
mod tests;
