//! Faber runtime package copy of the Radix-owned sparsa (sparse tensor)
//! operation contract: shared error-message constants.
//!
//! Authority: `radix-runtime-contract/src/sparsa.rs` (the compiler-side
//! authority). The runtime package is a standalone public crate, so the
//! contract material is committed here as a faithful copy. Copied faithfully
//! so compile-time and runtime emit byte-identical error text.
//!
//! KEEP IN SYNC: `radix-runtime-contract/src/sparsa.rs` and
//! `radix-hir-rust/src/runtime_contract_sparsa.rs`.

/// Shape dimension must be non-negative.
pub const ERR_NEGATIVE_DIM: &str = "sparsa shape dimension must be non-negative";
/// Index must be non-negative.
pub const ERR_NEGATIVE_INDEX: &str = "sparsa index must be non-negative";
/// Index outside the allocated shape.
pub const ERR_INDEX_OUT_OF_BOUNDS: &str = "sparsa index out of bounds";
/// Index rank does not match shape rank.
pub const ERR_RANK_MISMATCH: &str = "sparsa index rank does not match shape rank";
/// Non-nihil element count exceeds representable range.
pub const ERR_NONNIHIL_COUNT_OVERFLOW: &str = "sparsa nonnihil count overflow";
/// Element count exceeds representable range.
pub const ERR_ELEMENT_COUNT_OVERFLOW: &str = "sparsa element count overflow";
/// Conversio index rank does not match shape rank.
pub const ERR_CONVERSIO_RANK_MISMATCH: &str =
    "sparsa conversio index rank does not match shape rank";
/// `accipe` (get) invalid index.
pub const ERR_ACCIPE_INVALID_INDEX: &str = "sparsa accipe invalid index";
/// `ponde` (set) invalid index.
pub const ERR_PONDE_INVALID_INDEX: &str = "sparsa ponde invalid index";
