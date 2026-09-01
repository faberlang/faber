//! Structured HIR emission needs.
//!
//! Target-owned collectors record imports, helpers, and runtime dependencies
//! from HIR, semantic types, and emission decisions. Rendered target text must
//! never be scanned to decide compiler policy — needs are structural facts
//! gathered before emission, not substrings recovered from generated code.

#[cfg(feature = "hir-rust")]
pub use crate::codegen::rust::used_import_defs;
#[cfg(feature = "hir-go")]
pub use radix_hir_go::{GoNeeds, collect_go_needs};
#[cfg(feature = "hir-rust")]
pub use radix_hir_rust::needs::{RustNeeds, collect_rust_needs};
#[cfg(feature = "hir-ts")]
pub use radix_hir_ts::needs::{TsNeeds, TsNeedsKind, collect_ts_needs};

/// Guard helper: production policy must not use body-text `code.contains` for import decisions.
#[cfg(test)]
#[path = "../needs_test.rs"]
mod tests;
