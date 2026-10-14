use super::*;

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
