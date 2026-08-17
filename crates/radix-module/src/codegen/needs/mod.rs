//! Structured HIR emission needs.
//!
//! Target-owned collectors record imports, helpers, and runtime dependencies
//! from HIR, semantic types, and emission decisions. Rendered target text must
//! never be scanned to decide compiler policy — needs are structural facts
//! gathered before emission, not substrings recovered from generated code.

#[cfg(feature = "hir-rust")]
mod rust;
#[cfg(feature = "hir-ts")]
mod ts;

#[cfg(feature = "hir-rust")]
pub use crate::codegen::rust::used_import_defs;
#[cfg(feature = "hir-go")]
pub use radix_hir_go::{collect_go_needs, GoNeeds};
#[cfg(feature = "hir-rust")]
pub use rust::{collect_rust_needs, RustNeeds};
#[cfg(feature = "hir-ts")]
pub use ts::{collect_ts_needs, TsNeeds, TsNeedsKind};

/// Guard helper: production policy must not use body-text `code.contains` for import decisions.
#[cfg(test)]
#[path = "../needs_test.rs"]
mod tests;
