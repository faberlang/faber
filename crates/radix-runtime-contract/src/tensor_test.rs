use super::*;

#[test]
fn err_shape_constants_have_correct_text() {
    assert_eq!(
        ERR_NEGATIVE_DIM,
        "tensor shape dimension must be non-negative"
    );
    assert_eq!(ERR_NEGATIVE_INDEX, "tensor index must be non-negative");
    assert_eq!(
        ERR_NEGATIVE_SLICE,
        "tensor slice bounds must be non-negative"
    );
    assert_eq!(
        ERR_INVALID_SLICE_RANGE,
        "tensor slice end must be at least start"
    );
    assert_eq!(ERR_INDEX_OUT_OF_BOUNDS, "tensor index out of bounds");
    assert_eq!(ERR_ELEMENT_COUNT_OVERFLOW, "tensor element count overflow");
}

#[test]
fn err_forma_constants_have_correct_text() {
    assert_eq!(
        ERR_FORMA_RESHAPE_COUNT,
        "tensor forma (reshape) element count mismatch"
    );
    assert_eq!(
        ERR_FORMA_ELEMENT_COUNT,
        "tensor forma element count mismatch"
    );
    assert_eq!(ERR_ACCIPE_INVALID_INDEX, "tensor accipe invalid index");
    assert_eq!(ERR_PONDE_INVALID_INDEX, "tensor ponde invalid index");
    assert_eq!(ERR_CREA_INVALID_SHAPE, "tensor crea invalid shape");
    assert_eq!(
        ERR_SECTIO_INVALID_SLICE_BOUNDS,
        "tensor sectio invalid slice bounds"
    );
}

#[test]
fn err_broadcast_and_matmul_constants_have_correct_text() {
    assert_eq!(ERR_BROADCAST_SHAPE, "tensor broadcast shape mismatch");
    assert_eq!(
        ERR_MATMUL_RECEIVER_RANK,
        "tensor matmul requires rank-2 tensor receiver"
    );
    assert_eq!(
        ERR_MATMUL_ARGUMENT_RANK,
        "tensor matmul requires rank-2 tensor argument"
    );
    assert_eq!(
        ERR_MATMUL_INNER_DIMENSION,
        "tensor matmul inner dimension mismatch"
    );
    assert_eq!(
        ERR_TRANSPOSE_RANK,
        "tensor transpose requires rank-2 tensor"
    );
}

#[test]
fn err_permute_constants_have_correct_text() {
    assert_eq!(
        ERR_PERMUTE_RANK,
        "tensor permute axis count must equal tensor rank"
    );
    assert_eq!(
        ERR_PERMUTE_NEGATIVE_AXIS,
        "tensor permute axis must be non-negative"
    );
    assert_eq!(
        ERR_PERMUTE_AXIS_OUT_OF_RANGE,
        "tensor permute axis out of range"
    );
    assert_eq!(
        ERR_PERMUTE_DUPLICATE_AXIS,
        "tensor permute axis must appear exactly once"
    );
}

#[test]
fn err_divide_constants_have_correct_text() {
    assert_eq!(
        ERR_DIVIDE_NON_FINITE_INPUT,
        "tensor division input must be finite"
    );
    assert_eq!(
        ERR_DIVIDE_ZERO_DENOMINATOR,
        "tensor division denominator must be non-zero"
    );
    assert_eq!(
        ERR_DIVIDE_NON_FINITE_RESULT,
        "tensor division result must be finite"
    );
}
