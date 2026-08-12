//! Faber runtime package copy of the Radix-owned tensor operation contract:
//! shared error-message constants and shape arithmetic.
//!
//! Authority: `radix-runtime-contract/src/tensor.rs` (the compiler-side
//! authority). The runtime package is a standalone public crate, so the
//! contract material is committed here as a faithful copy. Copied faithfully
//! so compile-time (stepper) and runtime emit byte-identical error text and
//! agree on shape math.
//!
//! KEEP IN SYNC: `radix-runtime-contract/src/tensor.rs` and
//! `radix-hir-rust/src/runtime_contract_tensor.rs`.

/// Dimension must be non-negative.
pub const ERR_NEGATIVE_DIM: &str = "tensor shape dimension must be non-negative";
/// Index must be non-negative.
pub const ERR_NEGATIVE_INDEX: &str = "tensor index must be non-negative";
/// Slice bounds must be non-negative.
pub const ERR_NEGATIVE_SLICE: &str = "tensor slice bounds must be non-negative";
/// Slice end must be at least start.
pub const ERR_INVALID_SLICE_RANGE: &str = "tensor slice end must be at least start";
/// Index outside the allocated shape.
pub const ERR_INDEX_OUT_OF_BOUNDS: &str = "tensor index out of bounds";
/// Element count exceeds representable range.
pub const ERR_ELEMENT_COUNT_OVERFLOW: &str = "tensor element count overflow";

/// `forma` (reshape) element count mismatch.
pub const ERR_FORMA_RESHAPE_COUNT: &str = "tensor forma (reshape) element count mismatch";
/// `forma` element count mismatch.
pub const ERR_FORMA_ELEMENT_COUNT: &str = "tensor forma element count mismatch";
/// `accipe` (get) invalid index.
pub const ERR_ACCIPE_INVALID_INDEX: &str = "tensor accipe invalid index";
/// `ponde` (set) invalid index.
pub const ERR_PONDE_INVALID_INDEX: &str = "tensor ponde invalid index";
/// `crea` invalid shape.
pub const ERR_CREA_INVALID_SHAPE: &str = "tensor crea invalid shape";
/// `sectio` (slice) invalid bounds.
pub const ERR_SECTIO_INVALID_SLICE_BOUNDS: &str = "tensor sectio invalid slice bounds";
/// Broadcast shape mismatch.
pub const ERR_BROADCAST_SHAPE: &str = "tensor broadcast shape mismatch";
/// `matmul` receiver must be rank-2.
pub const ERR_MATMUL_RECEIVER_RANK: &str = "tensor matmul requires rank-2 tensor receiver";
/// `matmul` argument must be rank-2.
pub const ERR_MATMUL_ARGUMENT_RANK: &str = "tensor matmul requires rank-2 tensor argument";
/// `matmul` inner dimension mismatch.
pub const ERR_MATMUL_INNER_DIMENSION: &str = "tensor matmul inner dimension mismatch";
/// `transpose` requires rank-2.
pub const ERR_TRANSPOSE_RANK: &str = "tensor transpose requires rank-2 tensor";
/// `permute` axis count must equal rank.
pub const ERR_PERMUTE_RANK: &str = "tensor permute axis count must equal tensor rank";
/// `permute` axis must be non-negative.
pub const ERR_PERMUTE_NEGATIVE_AXIS: &str = "tensor permute axis must be non-negative";
/// `permute` axis out of range.
pub const ERR_PERMUTE_AXIS_OUT_OF_RANGE: &str = "tensor permute axis out of range";
/// `permute` axis must appear exactly once.
pub const ERR_PERMUTE_DUPLICATE_AXIS: &str = "tensor permute axis must appear exactly once";
/// `media` (mean) requires at least one element.
pub const ERR_MEDIA_EMPTY: &str = "tensor media (mean) requires at least one element";
/// Division input must be finite.
pub const ERR_DIVIDE_NON_FINITE_INPUT: &str = "tensor division input must be finite";
/// Division denominator must be non-zero.
pub const ERR_DIVIDE_ZERO_DENOMINATOR: &str = "tensor division denominator must be non-zero";
/// Division result must be finite.
pub const ERR_DIVIDE_NON_FINITE_RESULT: &str = "tensor division result must be finite";

/// A tensor shape dimension is valid only when non-negative.
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
