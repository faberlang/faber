//! Radix-owned tensor operation contract: shared error-message constants and
//! shape arithmetic.
//!
//! Mirrors `faber::tensor`. Radix is the authority for this contract;
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
pub const ERR_FORMA_LAYOUT_NOT_VIEWABLE: &str =
    "tensor forma layout cannot be represented as a view";
pub const ERR_FORMA_ELEMENT_COUNT: &str = "tensor forma element count mismatch";
pub const ERR_ACCIPE_INVALID_INDEX: &str = "tensor accipe invalid index";
pub const ERR_PONDE_INVALID_INDEX: &str = "tensor ponde invalid index";
pub const ERR_CREA_INVALID_SHAPE: &str = "tensor crea invalid shape";
pub const ERR_SECTIO_INVALID_SLICE_BOUNDS: &str = "tensor sectio invalid slice bounds";
pub const ERR_BROADCAST_SHAPE: &str = "tensor broadcast shape mismatch";
pub const ERR_TENSOR_SHAPE_HOLE_UNDERDETERMINED: &str =
    "tensor shape hole cannot be inferred uniquely from the available element count";
pub const ERR_TENSOR_EDGE_NOT_SHIFTED: &str = "tensor limes requires an unresolved shifted view";
pub const ERR_TENSOR_EDGE_UNRESOLVED_READ: &str =
    "tensor flatten cannot read an unresolved shifted optional element";
pub const ERR_TENSOR_MATERIALIZE_UNRESOLVED: &str =
    "tensor materialize cannot copy unresolved shifted optional elements";
pub const ERR_TENSOR_COPY_INTO_SHAPE_MISMATCH: &str = "tensor copy into shape mismatch";
pub const ERR_TENSOR_EDGE_POLICY_INVALID: &str =
    "tensor edge policy produced an invalid coordinate";
pub const ERR_TENSOR_EDGE_RANK_MISMATCH: &str =
    "tensor edge coordinate rank does not match tensor rank";
pub const ERR_TENSOR_EDGE_READ_ONLY: &str = "tensor edge views do not support writes";
pub const ERR_TENSOR_COALESCE_REQUIRES_OPTIONAL: &str =
    "tensor coalesce requires an unresolved shifted optional view";
pub const ERR_MATMUL_RECEIVER_RANK: &str = "tensor matmul requires rank-2 tensor receiver";
pub const ERR_MATMUL_ARGUMENT_RANK: &str = "tensor matmul requires rank-2 tensor argument";
pub const ERR_MATMUL_INNER_DIMENSION: &str = "tensor matmul inner dimension mismatch";
pub const ERR_MATMUL_BATCH_DIMENSION: &str =
    "tensor matmul batch dimensions do not match receiver prefix";
pub const ERR_TRANSPOSE_RANK: &str = "tensor transpose requires rank-2 tensor";
pub const ERR_PERMUTE_RANK: &str = "tensor permute axis count must equal tensor rank";
pub const ERR_PERMUTE_NEGATIVE_AXIS: &str = "tensor permute axis must be non-negative";
pub const ERR_PERMUTE_AXIS_OUT_OF_RANGE: &str = "tensor permute axis out of range";
pub const ERR_PERMUTE_DUPLICATE_AXIS: &str = "tensor permute axis must appear exactly once";
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
#[path = "tensor_test.rs"]
mod tests;
