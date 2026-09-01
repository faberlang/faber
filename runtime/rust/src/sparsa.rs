//! Sparse numeric tensor runtime for generated Rust code.
//!
//! `Sparsa<T>` stores only non-default entries in a coordinate-map (COO-style)
//! `HashMap`. Reads return `T::default()` for any in-bounds coordinate that has
//! not been explicitly stored, implementing the sparsa zero-identity semantics.

use std::collections::HashMap;

use crate::tensor::{Tensor, tensor_dim_non_negative, tensor_shape_element_count};

// ── Contract-authority re-exports ───────────────────────────────────────────
// Single canonical definition lives at
// radix-runtime-contract/src/sparsa.rs (the compiler-side authority).
pub use crate::contract::sparsa::{
    ERR_ACCIPE_INVALID_INDEX, ERR_CONVERSIO_RANK_MISMATCH, ERR_ELEMENT_COUNT_OVERFLOW,
    ERR_INDEX_OUT_OF_BOUNDS, ERR_NEGATIVE_DIM, ERR_NEGATIVE_INDEX, ERR_NONNIHIL_COUNT_OVERFLOW,
    ERR_PONDE_INVALID_INDEX, ERR_RANK_MISMATCH,
};

/// Homogeneous sparse numeric buffer with runtime shape metadata.
///
/// Only non-default entries are stored in `entries`. In-bounds reads for absent
/// coordinates return `T::default()` (the zero identity for numeric types).
#[derive(Clone, Debug)]
pub struct Sparsa<T> {
    /// Shape of the logical dense tensor.
    shape: Vec<i64>,
    /// Stored non-default entries keyed by coordinate tuple.
    entries: HashMap<Vec<i64>, T>,
}

// ── Internal helpers ────────────────────────────────────────────────────────

fn shape_dims(shape: &[i64]) -> Result<Vec<i64>, &'static str> {
    if shape.iter().all(|&d| tensor_dim_non_negative(d)) {
        Ok(shape.to_vec())
    } else {
        Err(ERR_NEGATIVE_DIM)
    }
}

fn validate_indices(shape: &[i64], indices: &[i64]) -> Result<(), &'static str> {
    if indices.len() != shape.len() {
        return Err(ERR_RANK_MISMATCH);
    }
    for (idx, dim) in indices.iter().zip(shape.iter()) {
        if *idx < 0 {
            return Err(ERR_NEGATIVE_INDEX);
        }
        if *idx >= *dim {
            return Err(ERR_INDEX_OUT_OF_BOUNDS);
        }
    }
    Ok(())
}

// ── Public API ──────────────────────────────────────────────────────────────

impl<T: Clone + Default + PartialEq> Sparsa<T> {
    /// Construct an all-zero (empty) sparse tensor with the given shape.
    ///
    /// No entries are stored; every in-bounds read returns `T::default()`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any dimension is negative.
    pub fn vacua(shape: &[i64]) -> Result<Self, &'static str> {
        let shape = shape_dims(shape)?;
        Ok(Self {
            shape,
            entries: HashMap::new(),
        })
    }

    /// Build sparse storage from a dense tensor, dropping exact default values.
    #[must_use]
    pub fn from_tensor(dense: &Tensor<T>) -> Self {
        let shape = dense.magnitudines();
        let entries = entries_from_dense_values(&shape, dense.planata());
        Self { shape, entries }
    }

    /// Rank (number of dimensions).
    #[must_use]
    pub fn longitudo(&self) -> i64 {
        // SAFETY: shape length fits in i64 for practical use.
        #[allow(clippy::cast_possible_wrap)]
        let len = self.shape.len() as i64;
        len
    }

    /// Shape dimensions.
    #[must_use]
    pub fn magnitudines(&self) -> Vec<i64> {
        self.shape.clone()
    }

    /// Total logical element count (`prod(dims)`).
    ///
    /// Returns `None` on overflow.
    #[must_use]
    pub fn element_count(&self) -> Option<usize> {
        tensor_shape_element_count(&self.shape)
    }

    /// Number of stored (non-default) entries.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the entry count overflows `i64`.
    pub fn nonnihil(&self) -> Result<i64, &'static str> {
        i64::try_from(self.entries.len()).map_err(|_| ERR_NONNIHIL_COUNT_OVERFLOW)
    }

    /// Read the value at the given index.
    ///
    /// Returns `T::default()` for in-bounds coordinates that have no stored
    /// entry.
    ///
    /// # Errors
    ///
    /// Returns `Err` if `indices` length does not match the shape rank, any
    /// index is negative, or any index is out of bounds.
    pub fn accipe(&self, indices: &[i64]) -> Result<T, &'static str> {
        validate_indices(&self.shape, indices)?;
        Ok(self.entries.get(indices).cloned().unwrap_or_default())
    }

    /// Write a value at the given index.
    ///
    /// If the value equals `T::default()`, the entry is removed to preserve
    /// sparsity (absent entries are implicitly zero).
    ///
    /// # Errors
    ///
    /// Returns `Err` if `indices` length does not match the shape rank, any
    /// index is negative, or any index is out of bounds.
    pub fn ponde(&mut self, indices: &[i64], value: T) -> Result<(), &'static str> {
        validate_indices(&self.shape, indices)?;
        if value == T::default() {
            self.entries.remove(indices);
        } else {
            self.entries.insert(indices.to_vec(), value);
        }
        Ok(())
    }

    /// Materialize to a dense `Tensor<T>`, filling absent entries with default.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the element count overflows `usize` or the tensor
    /// construction fails due to shape mismatch.
    pub fn densata(&self) -> Result<Tensor<T>, &'static str> {
        let count = self.element_count().ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?;

        let mut data = Vec::with_capacity(count);
        push_dense_values(&mut data, &self.shape, &self.entries);
        Tensor::structa(data, &self.shape)
    }
}

fn entries_from_dense_values<T: Clone + Default + PartialEq>(
    shape: &[i64],
    values: Vec<T>,
) -> HashMap<Vec<i64>, T> {
    let mut entries = HashMap::new();
    if values.is_empty() {
        return entries;
    }

    let mut current = vec![0i64; shape.len()];
    for value in values {
        if value != T::default() {
            entries.insert(current.clone(), value);
        }
        if shape.is_empty() || !advance_coordinate(&mut current, shape) {
            break;
        }
    }
    entries
}

/// Append row-major dense values without allocating a separate coordinate list.
fn push_dense_values<T: Clone + Default>(
    data: &mut Vec<T>,
    shape: &[i64],
    entries: &HashMap<Vec<i64>, T>,
) {
    if shape.contains(&0) {
        return;
    }

    let mut current = vec![0i64; shape.len()];

    loop {
        data.push(entries.get(&current).cloned().unwrap_or_default());

        if shape.is_empty() || !advance_coordinate(&mut current, shape) {
            break;
        }
    }
}

/// Advance a row-major coordinate in-place. Returns `false` after the last item.
fn advance_coordinate(current: &mut [i64], shape: &[i64]) -> bool {
    for axis in (0..shape.len()).rev() {
        current[axis] += 1;
        if current[axis] < shape[axis] {
            return true;
        }
        current[axis] = 0;
    }
    false
}

#[cfg(test)]
#[path = "sparsa_test.rs"]
mod tests;
