//! Statically dispatched edge-policy identities for generic tensor code.

use crate::tensor::TensorEdgePolicy;

/// Supplies a tensor edge policy for a generic policy type and tensor rank.
///
/// Rust tensors erase rank at runtime, while generated generic functions keep
/// the source rank as a const parameter for typechecking and forwarding.
pub trait TensorEdgePolicyProvider<T, const R: usize> {
    /// Construct the zero-state policy adapter used by the tensor carrier.
    fn edge_policy() -> TensorEdgePolicy<T>;
}

/// Zero-sized marker for clamp edge resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClampEdgePolicy;

impl<T, const R: usize> TensorEdgePolicyProvider<T, R> for ClampEdgePolicy {
    fn edge_policy() -> TensorEdgePolicy<T> {
        TensorEdgePolicy::Clamp
    }
}

/// Zero-sized marker for reflect edge resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReflectEdgePolicy;

impl<T, const R: usize> TensorEdgePolicyProvider<T, R> for ReflectEdgePolicy {
    fn edge_policy() -> TensorEdgePolicy<T> {
        TensorEdgePolicy::Reflect
    }
}

/// Zero-sized marker for wrap edge resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct WrapEdgePolicy;

impl<T, const R: usize> TensorEdgePolicyProvider<T, R> for WrapEdgePolicy {
    fn edge_policy() -> TensorEdgePolicy<T> {
        TensorEdgePolicy::Wrap
    }
}
