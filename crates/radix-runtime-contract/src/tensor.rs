//! Radix-owned tensor operation contract: shared error-message constants and
//! shape arithmetic.
//!
//! Mirrors `faber-runtime::tensor`. Radix is the authority for this contract;
//! the runtime copy is expected to become a generated/committed artifact in a
//! later phase. Copied faithfully so compile-time (runner) and runtime emit
//! byte-identical error text and agree on shape math.
//!
//! KEEP IN SYNC: `radix-hir-rust/src/runtime_contract_tensor.rs`
//! (verified by `#[cfg(test)] mod tests` in this file).

pub const ERR_NEGATIVE_DIM: &str = "tensor shape dimension must be non-negative";
pub const ERR_NEGATIVE_INDEX: &str = "tensor index must be non-negative";
pub const ERR_NEGATIVE_SLICE: &str = "tensor slice bounds must be non-negative";
pub const ERR_INVALID_SLICE_RANGE: &str = "tensor slice end must be at least start";
pub const ERR_INDEX_OUT_OF_BOUNDS: &str = "tensor index out of bounds";
pub const ERR_ELEMENT_COUNT_OVERFLOW: &str = "tensor element count overflow";

pub const ERR_FORMA_RESHAPE_COUNT: &str = "tensor forma (reshape) element count mismatch";
pub const ERR_FORMA_ELEMENT_COUNT: &str = "tensor forma element count mismatch";
pub const ERR_ACCIPE_INVALID_INDEX: &str = "tensor accipe invalid index";
pub const ERR_PONDE_INVALID_INDEX: &str = "tensor ponde invalid index";
pub const ERR_CREA_INVALID_SHAPE: &str = "tensor crea invalid shape";
pub const ERR_SECTIO_INVALID_SLICE_BOUNDS: &str = "tensor sectio invalid slice bounds";
pub const ERR_BROADCAST_SHAPE: &str = "tensor broadcast shape mismatch";
pub const ERR_MATMUL_RECEIVER_RANK: &str = "tensor matmul requires rank-2 tensor receiver";
pub const ERR_MATMUL_ARGUMENT_RANK: &str = "tensor matmul requires rank-2 tensor argument";
pub const ERR_MATMUL_INNER_DIMENSION: &str = "tensor matmul inner dimension mismatch";
pub const ERR_TRANSPOSE_RANK: &str = "tensor transpose requires rank-2 tensor";
pub const ERR_PERMUTE_RANK: &str = "tensor permute axis count must equal tensor rank";
pub const ERR_PERMUTE_NEGATIVE_AXIS: &str = "tensor permute axis must be non-negative";
pub const ERR_PERMUTE_AXIS_OUT_OF_RANGE: &str = "tensor permute axis out of range";
pub const ERR_PERMUTE_DUPLICATE_AXIS: &str = "tensor permute axis must appear exactly once";
pub const ERR_MEDIA_EMPTY: &str = "tensor media (mean) requires at least one element";
pub const ERR_DIVIDE_NON_FINITE_INPUT: &str = "tensor division input must be finite";
pub const ERR_DIVIDE_ZERO_DENOMINATOR: &str = "tensor division denominator must be non-zero";
pub const ERR_DIVIDE_NON_FINITE_RESULT: &str = "tensor division result must be finite";

#[must_use]
pub fn tensor_dim_non_negative(value: i64) -> bool {
    value >= 0
}

/// Total element count of a shape, or `None` on overflow/negative dims.
#[must_use]
pub fn tensor_shape_element_count(shape: &[i64]) -> Option<usize> {
    shape.iter().try_fold(1_usize, |acc, dim| {
        let dim = usize::try_from(*dim).ok()?;
        acc.checked_mul(dim)
    })
}

/// Row-major flat offset for `index` into a tensor of `shape`, or `None` if the
/// ranks disagree, a dimension/index is negative, or an index is out of bounds.
#[must_use]
pub fn tensor_flat_offset(shape: &[i64], index: &[i64]) -> Option<usize> {
    if shape.len() != index.len() {
        return None;
    }
    let mut offset = 0_usize;
    let mut stride = 1_usize;
    for (dim, idx) in shape.iter().zip(index.iter()).rev() {
        let dim = usize::try_from(*dim).ok()?;
        let idx = usize::try_from(*idx).ok()?;
        if idx >= dim {
            return None;
        }
        offset = offset.checked_add(idx.checked_mul(stride)?)?;
        stride = stride.checked_mul(dim)?;
    }
    Some(offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All `ERR_*` constants match the `radix-hir-rust` third copy.
    #[test]
    fn err_shape_constants_match_codegen_copy() {
        assert_eq!(
            ERR_NEGATIVE_DIM,
            radix_hir_rust::runtime_contract_tensor::ERR_NEGATIVE_DIM
        );
        assert_eq!(
            ERR_NEGATIVE_INDEX,
            radix_hir_rust::runtime_contract_tensor::ERR_NEGATIVE_INDEX
        );
        assert_eq!(
            ERR_NEGATIVE_SLICE,
            radix_hir_rust::runtime_contract_tensor::ERR_NEGATIVE_SLICE
        );
        assert_eq!(
            ERR_INVALID_SLICE_RANGE,
            radix_hir_rust::runtime_contract_tensor::ERR_INVALID_SLICE_RANGE
        );
        assert_eq!(
            ERR_INDEX_OUT_OF_BOUNDS,
            radix_hir_rust::runtime_contract_tensor::ERR_INDEX_OUT_OF_BOUNDS
        );
        assert_eq!(
            ERR_ELEMENT_COUNT_OVERFLOW,
            radix_hir_rust::runtime_contract_tensor::ERR_ELEMENT_COUNT_OVERFLOW
        );
    }

    #[test]
    fn err_forma_constants_match_codegen_copy() {
        assert_eq!(
            ERR_FORMA_RESHAPE_COUNT,
            radix_hir_rust::runtime_contract_tensor::ERR_FORMA_RESHAPE_COUNT
        );
        assert_eq!(
            ERR_FORMA_ELEMENT_COUNT,
            radix_hir_rust::runtime_contract_tensor::ERR_FORMA_ELEMENT_COUNT
        );
        assert_eq!(
            ERR_ACCIPE_INVALID_INDEX,
            radix_hir_rust::runtime_contract_tensor::ERR_ACCIPE_INVALID_INDEX
        );
        assert_eq!(
            ERR_PONDE_INVALID_INDEX,
            radix_hir_rust::runtime_contract_tensor::ERR_PONDE_INVALID_INDEX
        );
        assert_eq!(
            ERR_CREA_INVALID_SHAPE,
            radix_hir_rust::runtime_contract_tensor::ERR_CREA_INVALID_SHAPE
        );
        assert_eq!(
            ERR_SECTIO_INVALID_SLICE_BOUNDS,
            radix_hir_rust::runtime_contract_tensor::ERR_SECTIO_INVALID_SLICE_BOUNDS
        );
    }

    #[test]
    fn err_broadcast_and_matmul_constants_match_codegen_copy() {
        assert_eq!(
            ERR_BROADCAST_SHAPE,
            radix_hir_rust::runtime_contract_tensor::ERR_BROADCAST_SHAPE
        );
        assert_eq!(
            ERR_MATMUL_RECEIVER_RANK,
            radix_hir_rust::runtime_contract_tensor::ERR_MATMUL_RECEIVER_RANK
        );
        assert_eq!(
            ERR_MATMUL_ARGUMENT_RANK,
            radix_hir_rust::runtime_contract_tensor::ERR_MATMUL_ARGUMENT_RANK
        );
        assert_eq!(
            ERR_MATMUL_INNER_DIMENSION,
            radix_hir_rust::runtime_contract_tensor::ERR_MATMUL_INNER_DIMENSION
        );
        assert_eq!(
            ERR_TRANSPOSE_RANK,
            radix_hir_rust::runtime_contract_tensor::ERR_TRANSPOSE_RANK
        );
    }

    #[test]
    fn err_permute_constants_match_codegen_copy() {
        assert_eq!(
            ERR_PERMUTE_RANK,
            radix_hir_rust::runtime_contract_tensor::ERR_PERMUTE_RANK
        );
        assert_eq!(
            ERR_PERMUTE_NEGATIVE_AXIS,
            radix_hir_rust::runtime_contract_tensor::ERR_PERMUTE_NEGATIVE_AXIS
        );
        assert_eq!(
            ERR_PERMUTE_AXIS_OUT_OF_RANGE,
            radix_hir_rust::runtime_contract_tensor::ERR_PERMUTE_AXIS_OUT_OF_RANGE
        );
        assert_eq!(
            ERR_PERMUTE_DUPLICATE_AXIS,
            radix_hir_rust::runtime_contract_tensor::ERR_PERMUTE_DUPLICATE_AXIS
        );
        assert_eq!(
            ERR_MEDIA_EMPTY,
            radix_hir_rust::runtime_contract_tensor::ERR_MEDIA_EMPTY
        );
    }

    #[test]
    fn err_divide_constants_match_codegen_copy() {
        assert_eq!(
            ERR_DIVIDE_NON_FINITE_INPUT,
            radix_hir_rust::runtime_contract_tensor::ERR_DIVIDE_NON_FINITE_INPUT
        );
        assert_eq!(
            ERR_DIVIDE_ZERO_DENOMINATOR,
            radix_hir_rust::runtime_contract_tensor::ERR_DIVIDE_ZERO_DENOMINATOR
        );
        assert_eq!(
            ERR_DIVIDE_NON_FINITE_RESULT,
            radix_hir_rust::runtime_contract_tensor::ERR_DIVIDE_NON_FINITE_RESULT
        );
    }

    /// Shape-math functions produce identical results across copies.
    #[test]
    fn shape_math_functions_match_codegen_copy() {
        let shapes: &[&[i64]] = &[&[2, 3], &[1], &[5, 4, 3], &[0], &[1, 0, 1]];
        for shape in shapes {
            let a = tensor_shape_element_count(shape);
            let b = radix_hir_rust::runtime_contract_tensor::tensor_shape_element_count(shape);
            assert_eq!(
                a, b,
                "tensor_shape_element_count mismatch for shape {shape:?}"
            );
        }
    }

    #[test]
    fn flat_offset_matches_codegen_copy() {
        let cases: &[(&[i64], &[i64])] = &[
            (&[2, 3], &[0, 0]),
            (&[2, 3], &[1, 2]),
            (&[5], &[3]),
            (&[2, 3, 4], &[1, 1, 1]),
        ];
        for (shape, index) in cases {
            let a = tensor_flat_offset(shape, index);
            let b = radix_hir_rust::runtime_contract_tensor::tensor_flat_offset(shape, index);
            assert_eq!(
                a, b,
                "tensor_flat_offset mismatch for shape {shape:?}, index {index:?}"
            );
        }
    }

    #[test]
    fn flat_offset_none_cases_match_codegen_copy() {
        assert_eq!(
            tensor_flat_offset(&[2, 3], &[0]),
            radix_hir_rust::runtime_contract_tensor::tensor_flat_offset(&[2, 3], &[0])
        );
        assert_eq!(
            tensor_flat_offset(&[-1, 3], &[0, 0]),
            radix_hir_rust::runtime_contract_tensor::tensor_flat_offset(&[-1, 3], &[0, 0])
        );
        assert_eq!(
            tensor_flat_offset(&[2, 3], &[2, 0]),
            radix_hir_rust::runtime_contract_tensor::tensor_flat_offset(&[2, 3], &[2, 0])
        );
    }

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
        assert_eq!(
            ERR_MEDIA_EMPTY,
            "tensor media (mean) requires at least one element"
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

    /// Identity helper, pinned so the hir-rust copy cannot drift.
    #[test]
    fn dim_non_negative_matches_codegen_copy() {
        for value in [-1, 0, 1, 42] {
            assert_eq!(
                tensor_dim_non_negative(value),
                radix_hir_rust::runtime_contract_tensor::tensor_dim_non_negative(value),
                "tensor_dim_non_negative mismatch for {value}"
            );
        }
    }
}
