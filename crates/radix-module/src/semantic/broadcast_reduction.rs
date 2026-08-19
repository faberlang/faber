//! Broadcast cotangent reduction query.
//!
//! When an elementwise tensor op broadcasts (e.g. `tensor<f32, [N]> +
//! tensor<f32, [1]>`), the forward pass stretches size-1 dimensions. The
//! backward pass must reduce the cotangent from the broadcast result shape
//! back to each operand's original shape. This module provides the shared
//! read-only query that computes which axes need sum-reduction.
//!
//! The algorithm mirrors [`super::IndexUnifier::unify_broadcast`] trailing
//! alignment: left-pad the shorter shape with literal `1` dims, then mark
//! every axis where the operand has literal `1` and the result does not.

use super::{IndexId, TypeTable};

/// Compute which axes need sum-reduction when reducing a broadcast result
/// cotangent back to an operand's original shape.
///
/// Re-exported from `radix-types` — see [`radix_types::compute_cotangent_reduction_axes`].
///
/// Returns `None` if the shapes are not broadcast-compatible (defense-in-
/// depth — the forward typecheck already guarantees compatibility).
/// Returns `Some(axes)` where `axes` is a (possibly empty) list of axis
/// indices in **operand-local** space (0-indexed from operand's own rank,
/// not the padded rank). An empty list means no reduction is needed
/// (same-shape or operand shape equals result shape after alignment).
#[must_use]
pub fn compute_cotangent_reduction_axes(
    operand_shape: IndexId,
    result_shape: IndexId,
    types: &TypeTable,
) -> Option<Vec<usize>> {
    radix_types::compute_cotangent_reduction_axes(operand_shape, result_shape, types)
}

#[cfg(test)]
#[path = "broadcast_reduction_test.rs"]
mod tests;
