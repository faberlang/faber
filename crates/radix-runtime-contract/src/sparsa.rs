//! Radix-owned sparsa (sparse tensor) operation contract: shared error-message
//! constants.
//!
//! Mirrors `faber::sparsa`. Radix is the authority; the runtime copy is
//! expected to become a generated/committed artifact in a later phase. Copied
//! faithfully so compile-time and runtime emit byte-identical error text.
//!
//! KEEP IN SYNC: `radix-hir-rust/src/runtime_contract_sparsa.rs`
//! (verified by `#[cfg(test)] mod tests` in this file).

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

#[cfg(test)]
mod tests {
    use super::*;

    /// All `ERR_*` constants match the `radix-hir-rust` third copy.
    #[test]
    fn err_constants_match_codegen_copy() {
        let codegen = radix_hir_rust::runtime_contract_sparsa::ERR_NEGATIVE_DIM;
        assert_eq!(ERR_NEGATIVE_DIM, codegen);

        let codegen = radix_hir_rust::runtime_contract_sparsa::ERR_NEGATIVE_INDEX;
        assert_eq!(ERR_NEGATIVE_INDEX, codegen);

        let codegen = radix_hir_rust::runtime_contract_sparsa::ERR_INDEX_OUT_OF_BOUNDS;
        assert_eq!(ERR_INDEX_OUT_OF_BOUNDS, codegen);

        let codegen = radix_hir_rust::runtime_contract_sparsa::ERR_RANK_MISMATCH;
        assert_eq!(ERR_RANK_MISMATCH, codegen);

        let codegen = radix_hir_rust::runtime_contract_sparsa::ERR_NONNIHIL_COUNT_OVERFLOW;
        assert_eq!(ERR_NONNIHIL_COUNT_OVERFLOW, codegen);

        let codegen = radix_hir_rust::runtime_contract_sparsa::ERR_ELEMENT_COUNT_OVERFLOW;
        assert_eq!(ERR_ELEMENT_COUNT_OVERFLOW, codegen);

        let codegen = radix_hir_rust::runtime_contract_sparsa::ERR_CONVERSIO_RANK_MISMATCH;
        assert_eq!(ERR_CONVERSIO_RANK_MISMATCH, codegen);

        let codegen = radix_hir_rust::runtime_contract_sparsa::ERR_ACCIPE_INVALID_INDEX;
        assert_eq!(ERR_ACCIPE_INVALID_INDEX, codegen);

        let codegen = radix_hir_rust::runtime_contract_sparsa::ERR_PONDE_INVALID_INDEX;
        assert_eq!(ERR_PONDE_INVALID_INDEX, codegen);
    }

    /// All `ERR_*` constants have the correct text (pins against runtime copy).
    #[test]
    fn err_constants_have_correct_text() {
        assert_eq!(
            ERR_NEGATIVE_DIM,
            "sparsa shape dimension must be non-negative"
        );
        assert_eq!(ERR_NEGATIVE_INDEX, "sparsa index must be non-negative");
        assert_eq!(ERR_INDEX_OUT_OF_BOUNDS, "sparsa index out of bounds");
        assert_eq!(
            ERR_RANK_MISMATCH,
            "sparsa index rank does not match shape rank"
        );
        assert_eq!(
            ERR_NONNIHIL_COUNT_OVERFLOW,
            "sparsa nonnihil count overflow"
        );
        assert_eq!(ERR_ELEMENT_COUNT_OVERFLOW, "sparsa element count overflow");
        assert_eq!(
            ERR_CONVERSIO_RANK_MISMATCH,
            "sparsa conversio index rank does not match shape rank"
        );
        assert_eq!(ERR_ACCIPE_INVALID_INDEX, "sparsa accipe invalid index");
        assert_eq!(ERR_PONDE_INVALID_INDEX, "sparsa ponde invalid index");
    }
}
