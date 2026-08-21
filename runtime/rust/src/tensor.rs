//! Dense numeric tensor runtime for generated Rust code.

use std::fmt::Debug;
use std::sync::{Arc, Mutex, MutexGuard};

/// Homogeneous numeric buffer with runtime shape metadata.
#[derive(Clone, Debug)]
pub struct Tensor<T> {
    data: Arc<Mutex<Vec<T>>>,
    shape: Vec<usize>,
    strides: Vec<usize>,
    offset: usize,
}

// ── Error messages ─────────────────────────────────────────────────────────
// Shared error strings are the contract authority's; ops implemented in this
// file own their local domain messages. Both surfaces resolve through
// `faber::tensor::*`.

// Contract-authority re-exports: the single canonical definition lives at
// radix-runtime-contract/src/tensor.rs (the compiler-side authority).
pub use crate::contract::tensor::{
    tensor_dim_non_negative, tensor_flat_offset, tensor_shape_element_count,
    ERR_ACCIPE_INVALID_INDEX, ERR_BROADCAST_SHAPE, ERR_CREA_INVALID_SHAPE,
    ERR_DIVIDE_NON_FINITE_INPUT, ERR_DIVIDE_NON_FINITE_RESULT, ERR_DIVIDE_ZERO_DENOMINATOR,
    ERR_ELEMENT_COUNT_OVERFLOW, ERR_FORMA_ELEMENT_COUNT, ERR_FORMA_RESHAPE_COUNT,
    ERR_INDEX_OUT_OF_BOUNDS, ERR_INVALID_SLICE_RANGE, ERR_MATMUL_ARGUMENT_RANK,
    ERR_MATMUL_INNER_DIMENSION, ERR_MATMUL_RECEIVER_RANK, ERR_MEDIA_EMPTY, ERR_NEGATIVE_DIM,
    ERR_NEGATIVE_INDEX, ERR_NEGATIVE_SLICE, ERR_PERMUTE_AXIS_OUT_OF_RANGE,
    ERR_PERMUTE_DUPLICATE_AXIS, ERR_PERMUTE_NEGATIVE_AXIS, ERR_PERMUTE_RANK,
    ERR_PONDE_INVALID_INDEX, ERR_SECTIO_INVALID_SLICE_BOUNDS, ERR_TRANSPOSE_RANK,
};

// Local domain messages, grouped by op so each kernel's error surface is
// visible at a glance. `pub(crate)` = internal to this crate; the `pub`
// layernorm messages are part of the generated-code error surface.
// ── relu / sqrt / gelu ──
pub(crate) const ERR_RELU_NON_FINITE_INPUT: &str =
    "ReLU requires finite input; NaN or inf was given.";
pub(crate) const ERR_SQRT_NON_FINITE_INPUT: &str =
    "Sqrt requires finite input; NaN or inf was given.";
pub(crate) const ERR_SQRT_NEGATIVE_INPUT: &str = "Sqrt requires non-negative input.";
pub(crate) const ERR_GELU_NON_FINITE_INPUT: &str =
    "Gelu input must be finite; NaN or inf was given.";
// ── exp / log ──
pub(crate) const ERR_EXP_NON_FINITE_INPUT: &str = "Exp input must be finite; NaN or inf was given.";
pub(crate) const ERR_EXP_OVERFLOW: &str = "Exp overflow: output is non-finite.";
pub(crate) const ERR_LOG_NON_FINITE_INPUT: &str = "Log input must be finite; NaN or inf was given.";
pub(crate) const ERR_LOG_NON_POSITIVE_INPUT: &str = "Log requires positive input.";
pub(crate) const ERR_LOG_NON_FINITE_RESULT: &str = "Log produced non-finite result.";
// ── softmax ──
pub(crate) const ERR_SOFTMAX_NON_FINITE_INPUT: &str =
    "Softmax input must be finite; NaN or inf was given.";
pub(crate) const ERR_SOFTMAX_EMPTY_TENSOR: &str = "Softmax requires non-empty tensor.";
// ── crux_entropia ──
pub(crate) const ERR_CRUX_ENTROPIA_NON_FINITE_INPUT: &str =
    "Cross-entropy logits must be finite; NaN or inf was given.";
pub(crate) const ERR_CRUX_ENTROPIA_EMPTY_TENSOR: &str = "Cross-entropy requires non-empty tensor.";
pub(crate) const ERR_CRUX_ENTROPIA_TARGET_NON_FINITE: &str =
    "Cross-entropy targets must be finite; NaN or inf was given.";
pub(crate) const ERR_CRUX_ENTROPIA_TARGET_RANGE: &str = "Cross-entropy targets must be in [0, 1].";
pub(crate) const ERR_CRUX_ENTROPIA_SHAPE_MISMATCH: &str =
    "Cross-entropy logits and targets must have the same shape.";
pub(crate) const ERR_CRUX_ENTROPIA_RANK: &str = "Cross-entropy requires rank-1 or rank-2 tensor.";
// ── layernorm (pub) ──
pub const ERR_LAYERNORM_NON_FINITE_INPUT: &str =
    "layernorm requires finite input; NaN or inf was given.";
pub const ERR_LAYERNORM_EMPTY_TENSOR: &str = "layernorm requires non-empty tensor.";
pub const ERR_LAYERNORM_RANK_TOO_HIGH: &str = "layernorm requires rank-1 or rank-2 tensor.";
pub const ERR_LAYERNORM_AXIS_OUT_OF_RANGE: &str = "layernorm axis out of range.";
pub const ERR_LAYERNORM_GAMMA_SHAPE_MISMATCH: &str =
    "layernorm gamma shape does not match input shape at normalization axis.";
pub const ERR_LAYERNORM_BETA_SHAPE_MISMATCH: &str =
    "layernorm beta shape does not match input shape at normalization axis.";
pub const ERR_LAYERNORM_GAMMA_NON_FINITE: &str =
    "layernorm gamma must be finite; NaN or inf was given.";
pub const ERR_LAYERNORM_BETA_NON_FINITE: &str =
    "layernorm beta must be finite; NaN or inf was given.";
pub const ERR_LAYERNORM_EPSILON_INVALID: &str = "layernorm epsilon must be > 0 and finite.";

#[must_use]
pub fn tensor_shape_has_element_count(shape: &[i64], actual: usize) -> bool {
    tensor_shape_element_count(shape) == Some(actual)
}

fn shape_dims(shape: &[i64]) -> Result<Vec<usize>, &'static str> {
    shape
        .iter()
        .map(|&dim| parse_non_negative(dim, ERR_NEGATIVE_DIM))
        .collect()
}

fn shape_dims_and_count<T>(shape: &[i64]) -> Result<(Vec<usize>, usize), &'static str> {
    let dims = shape_dims(shape)?;
    let count = checked_allocation_count::<T>(&dims)?;
    Ok((dims, count))
}

fn index_dims(indices: &[i64]) -> Result<Vec<usize>, &'static str> {
    indices
        .iter()
        .map(|&index| parse_non_negative(index, ERR_NEGATIVE_INDEX))
        .collect()
}

fn parse_non_negative(value: i64, message: &'static str) -> Result<usize, &'static str> {
    if value < 0 {
        Err(message)
    } else {
        // SAFETY: guarded by non-negative check above.
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        Ok(value as usize)
    }
}

fn slice_bounds(start: i64, end: i64) -> Result<(usize, usize), &'static str> {
    let start = parse_non_negative(start, ERR_NEGATIVE_SLICE)?;
    let end = parse_non_negative(end, ERR_NEGATIVE_SLICE)?;
    if end < start {
        return Err(ERR_INVALID_SLICE_RANGE);
    }
    Ok((start, end))
}

fn tensor_data<T>(data: &Arc<Mutex<Vec<T>>>) -> MutexGuard<'_, Vec<T>> {
    match data.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl<T: Clone + Default> Tensor<T> {
    pub(crate) fn linea(data: Vec<T>) -> Self {
        let shape = vec![data.len()];
        Self::from_contiguous(data, shape)
    }

    /// Rank-0 tensor: one default-initialized element slot.
    #[must_use]
    pub fn vacua() -> Self {
        Self::from_contiguous(vec![T::default()], Vec::new())
    }

    #[must_use]
    pub fn longitudo(&self) -> i64 {
        // SAFETY: shape length fits in i64 for practical use.
        #[allow(clippy::cast_possible_wrap)]
        let len = self.shape.len() as i64;
        len
    }

    #[must_use]
    pub fn magnitudines(&self) -> Vec<i64> {
        // SAFETY: each dimension fits in i64 for practical tensor shapes.
        self.shape
            .iter()
            .map(|&d| {
                #[allow(clippy::cast_possible_wrap)]
                let d = d as i64;
                d
            })
            .collect()
    }

    #[must_use]
    pub fn element_count(&self) -> usize {
        element_count_usize(&self.shape)
    }

    /// Create a new tensor filled with the given value.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any dimension is negative or the element count overflows.
    pub fn crea(shape: &[i64], fill: T) -> Result<Self, &'static str> {
        let (dims, count) = shape_dims_and_count::<T>(shape)?;
        Ok(Self::from_contiguous(vec![fill; count], dims))
    }

    /// Create a tensor from data and shape.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any dimension is negative or if the data length does
    /// not match the shape element count.
    pub fn structa(data: Vec<T>, shape: &[i64]) -> Result<Self, &'static str> {
        let dims = shape_dims(shape)?;
        if !tensor_shape_has_element_count(shape, data.len()) {
            return Err("tensor element count does not match shape");
        }
        Ok(Self::from_contiguous(data, dims))
    }

    #[must_use]
    pub fn planata(&self) -> Vec<T> {
        let data = tensor_data(&self.data);
        self.logical_offsets()
            .into_iter()
            .map(|offset| data[offset].clone())
            .collect()
    }

    /// Reshape the tensor.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any dimension is negative or if the element count does
    /// not match the new shape.
    pub fn forma(&self, shape: &[i64]) -> Result<Self, &'static str> {
        let dims = shape_dims(shape)?;
        if !tensor_shape_has_element_count(shape, self.element_count()) {
            return Err(ERR_FORMA_RESHAPE_COUNT);
        }
        Ok(Self::from_contiguous(self.planata(), dims))
    }

    /// Read the value at the given indices.
    ///
    /// Returns `Ok(None)` for in-bounds indices that fall within a view gap.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any index is negative.
    pub fn accipe(&self, indices: &[i64]) -> Result<Option<T>, &'static str> {
        let index = index_dims(indices)?;
        let Some(offset) = self.offset_for_index(&index) else {
            return Ok(None);
        };
        Ok(tensor_data(&self.data).get(offset).cloned())
    }

    /// Write a value at the given indices.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any index is negative or out of bounds.
    pub fn ponde(&mut self, indices: &[i64], value: T) -> Result<(), &'static str> {
        let index = index_dims(indices)?;
        let Some(offset) = self.offset_for_index(&index) else {
            return Err(ERR_INDEX_OUT_OF_BOUNDS);
        };
        tensor_data(&self.data)[offset] = value;
        Ok(())
    }

    pub fn reple(&mut self, value: T) {
        let offsets = self.logical_offsets();
        let mut data = tensor_data(&self.data);
        for offset in offsets {
            data[offset] = value.clone();
        }
    }

    /// Element-wise conversion preserving shape metadata.
    ///
    /// Codegen supplies the per-element map so tensor `↦` mirrors scalar conversio
    /// rules (widening casts, fractus→numerus truncation, and so on).
    pub fn convert_elements<B, F>(&self, map: F) -> Tensor<B>
    where
        B: Clone + Default,
        F: Fn(T) -> B,
    {
        let elems: Vec<B> = self.planata().into_iter().map(map).collect();
        Tensor::from_contiguous(elems, self.shape.clone())
    }

    /// View a contiguous slice along axis 0 from `start` (inclusive) to `end` (exclusive).
    ///
    /// # Errors
    ///
    /// Returns `Err` if bounds are negative, `end < start`, or `end` exceeds
    /// the first dimension.
    pub fn sectio(&self, start: i64, end: i64) -> Result<Self, &'static str> {
        let (start, end) = slice_bounds(start, end)?;
        if self.shape.is_empty() || end > self.shape[0] {
            return Err(ERR_INDEX_OUT_OF_BOUNDS);
        }
        let mut shape = self.shape.clone();
        shape[0] = end - start;
        Ok(Self {
            data: Arc::clone(&self.data),
            shape,
            strides: self.strides.clone(),
            offset: self.offset + start * self.strides[0],
        })
    }

    #[must_use]
    pub fn materialize(&self) -> Self {
        Self::from_contiguous(self.planata(), self.shape.clone())
    }

    /// Materialized rank-2 transpose.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the tensor is not rank-2 or the element count overflows.
    pub fn transpose_rank2(&self) -> Result<Self, &'static str> {
        if self.shape.len() != 2 {
            return Err(ERR_TRANSPOSE_RANK);
        }
        let rows = self.shape[0];
        let cols = self.shape[1];
        let count = checked_allocation_count::<T>(&[cols, rows])?;
        let mut data = Vec::with_capacity(count);
        for col in 0..cols {
            for row in 0..rows {
                data.push(self.value_at_logical(&[row, col]));
            }
        }
        Ok(Self::from_contiguous(data, vec![cols, rows]))
    }

    /// Materialized axis permutation. The result is a copy with row-major strides.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the axis count does not match the tensor rank, any axis
    /// is negative or out of range, an axis is duplicated, or the element count
    /// overflows.
    pub fn permute(&self, axes: &[i64]) -> Result<Self, &'static str> {
        let axes = permute_axes(axes, self.shape.len())?;
        let shape: Vec<usize> = axes.iter().map(|&axis| self.shape[axis]).collect();
        let count = checked_allocation_count::<T>(&shape)?;
        let mut data = Vec::with_capacity(count);
        for ordinal in 0..count {
            let output_index = unravel_index(ordinal, &shape);
            let mut input_index = vec![0; self.shape.len()];
            for (output_axis, &input_axis) in axes.iter().enumerate() {
                input_index[input_axis] = output_index[output_axis];
            }
            data.push(self.value_at_logical(&input_index));
        }
        Ok(Self::from_contiguous(data, shape))
    }

    fn from_contiguous(data: Vec<T>, shape: Vec<usize>) -> Self {
        Self {
            data: Arc::new(Mutex::new(data)),
            strides: row_major_strides(&shape),
            shape,
            offset: 0,
        }
    }

    fn offset_for_index(&self, index: &[usize]) -> Option<usize> {
        if index.len() != self.shape.len() {
            return None;
        }
        let mut offset = self.offset;
        for ((idx, dim), stride) in index.iter().zip(self.shape.iter()).zip(self.strides.iter()) {
            if idx >= dim {
                return None;
            }
            offset = offset.checked_add(idx.checked_mul(*stride)?)?;
        }
        Some(offset)
    }

    fn logical_offsets(&self) -> Vec<usize> {
        let count = self.element_count();
        (0..count)
            .map(|ordinal| {
                let index = unravel_index(ordinal, &self.shape);
                self.logical_offset_for_index(&index)
            })
            .collect()
    }

    fn value_at_logical(&self, index: &[usize]) -> T {
        let offset = self.logical_offset_for_index(index);
        tensor_data(&self.data)[offset].clone()
    }

    fn logical_offset_for_index(&self, index: &[usize]) -> usize {
        self.offset
            + index
                .iter()
                .zip(self.strides.iter())
                .map(|(idx, stride)| idx * stride)
                .sum::<usize>()
    }
}

fn element_count_usize(shape: &[usize]) -> usize {
    checked_element_count_usize(shape).expect("tensor shape has checked element count")
}

fn checked_element_count_usize(shape: &[usize]) -> Option<usize> {
    shape
        .iter()
        .try_fold(1_usize, |acc, dim| acc.checked_mul(*dim))
}

fn checked_allocation_count<T>(shape: &[usize]) -> Result<usize, &'static str> {
    let count = checked_element_count_usize(shape).ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?;
    let element_size = std::mem::size_of::<T>();
    if element_size != 0 && count > (isize::MAX as usize) / element_size {
        return Err(ERR_ELEMENT_COUNT_OVERFLOW);
    }
    Ok(count)
}

fn row_major_strides(shape: &[usize]) -> Vec<usize> {
    let mut strides = vec![1; shape.len()];
    let mut next = 1_usize;
    for (idx, dim) in shape.iter().enumerate().rev() {
        strides[idx] = next;
        next = next.saturating_mul(*dim);
    }
    strides
}

fn unravel_index(mut ordinal: usize, shape: &[usize]) -> Vec<usize> {
    if shape.is_empty() {
        return Vec::new();
    }
    let mut index = vec![0; shape.len()];
    for (axis, dim) in shape.iter().enumerate().rev() {
        index[axis] = ordinal % dim;
        ordinal /= dim;
    }
    index
}

fn permute_axes(axes: &[i64], rank: usize) -> Result<Vec<usize>, &'static str> {
    if axes.len() != rank {
        return Err(ERR_PERMUTE_RANK);
    }
    let mut parsed = Vec::with_capacity(rank);
    let mut seen = vec![false; rank];
    for &axis in axes {
        let axis = parse_non_negative(axis, ERR_PERMUTE_NEGATIVE_AXIS)?;
        if axis >= rank {
            return Err(ERR_PERMUTE_AXIS_OUT_OF_RANGE);
        }
        if seen[axis] {
            return Err(ERR_PERMUTE_DUPLICATE_AXIS);
        }
        seen[axis] = true;
        parsed.push(axis);
    }
    Ok(parsed)
}

fn broadcast_shape(lhs: &[usize], rhs: &[usize]) -> Result<Vec<usize>, &'static str> {
    let rank = lhs.len().max(rhs.len());
    let mut shape = Vec::with_capacity(rank);
    for axis in 0..rank {
        let lhs_dim = broadcast_dim(lhs, rank, axis);
        let rhs_dim = broadcast_dim(rhs, rank, axis);
        let dim = if lhs_dim == rhs_dim {
            lhs_dim
        } else if lhs_dim == 1 {
            rhs_dim
        } else if rhs_dim == 1 {
            lhs_dim
        } else {
            return Err(ERR_BROADCAST_SHAPE);
        };
        shape.push(dim);
    }
    Ok(shape)
}

fn broadcast_dim(shape: &[usize], rank: usize, axis: usize) -> usize {
    let pad = rank - shape.len();
    if axis < pad {
        1
    } else {
        shape[axis - pad]
    }
}

fn broadcast_index(index: &[usize], shape: &[usize]) -> Vec<usize> {
    let pad = index.len() - shape.len();
    (0..shape.len())
        .map(|axis| {
            if shape[axis] == 1 {
                0
            } else {
                index[axis + pad]
            }
        })
        .collect()
}

fn tensor_elementwise<T, F>(
    lhs: &Tensor<T>,
    rhs: &Tensor<T>,
    op: F,
) -> Result<Tensor<T>, &'static str>
where
    T: Clone + Default,
    F: Fn(T, T) -> T,
{
    let shape = broadcast_shape(&lhs.shape, &rhs.shape)?;
    let count = checked_allocation_count::<T>(&shape)?;
    let mut data = Vec::with_capacity(count);
    for ordinal in 0..count {
        let index = unravel_index(ordinal, &shape);
        let lhs_index = broadcast_index(&index, &lhs.shape);
        let rhs_index = broadcast_index(&index, &rhs.shape);
        data.push(op(
            lhs.value_at_logical(&lhs_index),
            rhs.value_at_logical(&rhs_index),
        ));
    }
    Ok(Tensor::from_contiguous(data, shape))
}

/// Elementwise broadcast arithmetic. Each op is its own `impl` block so the
/// `std::ops` bound is only required where the kernel actually needs it.
impl<T> Tensor<T>
where
    T: Clone + Default + std::ops::Add<Output = T>,
{
    /// Elementwise `self + other` after NumPy-style broadcast unification.
    ///
    /// # Errors
    ///
    /// Returns `Err` if shapes are not broadcast-compatible or the element
    /// count overflows.
    pub fn addita(&self, other: &Tensor<T>) -> Result<Tensor<T>, &'static str> {
        tensor_elementwise(self, other, |lhs, rhs| lhs + rhs)
    }

    /// Sum of all elements. Integer overflow is the author's responsibility
    /// (per the tensor arithmetic goal non-goals); widen with `↦` first if needed.
    #[must_use]
    pub fn summa(&self) -> T {
        self.planata()
            .into_iter()
            .fold(T::default(), |acc, value| acc + value)
    }
}

impl<T> Tensor<T>
where
    T: Clone + Default + std::ops::Sub<Output = T>,
{
    /// Elementwise `self - other` after NumPy-style broadcast unification.
    ///
    /// # Errors
    ///
    /// Returns `Err` if shapes are not broadcast-compatible or the element
    /// count overflows.
    pub fn subtrahe(&self, other: &Tensor<T>) -> Result<Tensor<T>, &'static str> {
        tensor_elementwise(self, other, |lhs, rhs| lhs - rhs)
    }
}

impl<T> Tensor<T>
where
    T: Clone + Default + std::ops::Mul<Output = T>,
{
    /// Elementwise `self * other` after NumPy-style broadcast unification.
    ///
    /// # Errors
    ///
    /// Returns `Err` if shapes are not broadcast-compatible or the element
    /// count overflows.
    pub fn multiplica(&self, other: &Tensor<T>) -> Result<Tensor<T>, &'static str> {
        tensor_elementwise(self, other, |lhs, rhs| lhs * rhs)
    }
}

impl Tensor<f32> {
    /// Elementwise negation preserving tensor shape.
    #[must_use]
    pub fn neg(&self) -> Tensor<f32> {
        Tensor::from_contiguous(
            self.planata().into_iter().map(|value| -value).collect(),
            self.shape.clone(),
        )
    }

    /// Elementwise rectified linear unit: max(0, x).
    ///
    /// Rejects non-finite inputs (NaN, inf) per the domain-sensitive primitive
    /// policy.  No other domain constraints.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any element is NaN or infinite.
    pub fn relu(&self) -> Result<Tensor<f32>, &'static str> {
        for &value in self.planata().iter() {
            if !value.is_finite() {
                return Err(ERR_RELU_NON_FINITE_INPUT);
            }
        }
        Ok(Tensor::from_contiguous(
            self.planata()
                .into_iter()
                .map(|value| value.max(0.0))
                .collect(),
            self.shape.clone(),
        ))
    }

    /// Elementwise square root with domain validation.
    ///
    /// Rejects non-finite inputs (NaN, inf) and negative inputs per the
    /// domain-sensitive primitive policy.  Returns `sqrt(x)` for all valid
    /// finite non-negative inputs.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any element is NaN, infinite, or negative.
    pub fn sqrt(&self) -> Result<Tensor<f32>, &'static str> {
        for &value in self.planata().iter() {
            if !value.is_finite() {
                return Err(ERR_SQRT_NON_FINITE_INPUT);
            }
            if value < 0.0 {
                return Err(ERR_SQRT_NEGATIVE_INPUT);
            }
        }
        Ok(Tensor::from_contiguous(
            self.planata()
                .into_iter()
                .map(|value| value.sqrt())
                .collect(),
            self.shape.clone(),
        ))
    }

    /// Elementwise Gaussian Error Linear Unit using the tanh approximation.
    ///
    /// Computes `0.5 * x * (1 + tanh(α * (x + β * x³)))` where
    /// α = √(2/π) and β = 0.044715. Error < 1e-6 vs exact Gelu.
    /// Rejects non-finite inputs (NaN, inf) per the domain-sensitive primitive
    /// policy. All finite f32 inputs are valid — no other domain constraints.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any element is NaN or infinite.
    pub fn gelu(&self) -> Result<Tensor<f32>, &'static str> {
        for &value in self.planata().iter() {
            if !value.is_finite() {
                return Err(ERR_GELU_NON_FINITE_INPUT);
            }
        }
        let alpha = (2.0 / std::f32::consts::PI).sqrt();
        let beta = 0.044_715;
        Ok(Tensor::from_contiguous(
            self.planata()
                .into_iter()
                .map(|x| {
                    let cube = x * x * x;
                    0.5 * x * (1.0 + (alpha * (x + beta * cube)).tanh())
                })
                .collect(),
            self.shape.clone(),
        ))
    }

    /// Elementwise natural exponentiation: e^x.
    ///
    /// Rejects non-finite inputs (NaN, inf) and rejects results that overflow
    /// to infinity per the domain-sensitive primitive policy.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any input element is NaN or infinite, or if any
    /// result overflows to a non-finite value.
    pub fn exp(&self) -> Result<Tensor<f32>, &'static str> {
        for &value in self.planata().iter() {
            if !value.is_finite() {
                return Err(ERR_EXP_NON_FINITE_INPUT);
            }
        }
        let mut data = Vec::with_capacity(self.element_count());
        for &value in self.planata().iter() {
            let result = value.exp();
            if !result.is_finite() {
                return Err(ERR_EXP_OVERFLOW);
            }
            data.push(result);
        }
        Ok(Tensor::from_contiguous(data, self.shape.clone()))
    }

    /// Elementwise natural logarithm: ln(x).
    ///
    /// Rejects non-finite inputs (NaN, inf), non-positive inputs (x ≤ 0),
    /// and non-finite results per the domain-sensitive primitive policy.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any input element is NaN, infinite, or zero/negative,
    /// or if any result is non-finite.
    pub fn log(&self) -> Result<Tensor<f32>, &'static str> {
        for &value in self.planata().iter() {
            if !value.is_finite() {
                return Err(ERR_LOG_NON_FINITE_INPUT);
            }
            if value <= 0.0 {
                return Err(ERR_LOG_NON_POSITIVE_INPUT);
            }
        }
        let mut data = Vec::with_capacity(self.element_count());
        for &value in self.planata().iter() {
            let result = value.ln();
            if !result.is_finite() {
                return Err(ERR_LOG_NON_FINITE_RESULT);
            }
            data.push(result);
        }
        Ok(Tensor::from_contiguous(data, self.shape.clone()))
    }

    /// Softmax: exp(x_i - max(x)) / sum(exp(x_j - max(x))) with numerical
    /// stability. Operates on rank-1 (vector) or rank-2 (batched row-wise,
    /// axis 1 — the last axis).
    ///
    /// Rejects non-finite inputs (NaN, inf) and empty tensors per the
    /// domain-sensitive primitive policy. No VJP — Softmax backward is
    /// deferred to a follow-on goal.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any element is NaN or infinite, or if the tensor
    /// is empty.
    pub fn softmax(&self) -> Result<Tensor<f32>, &'static str> {
        if self.element_count() == 0 {
            return Err(ERR_SOFTMAX_EMPTY_TENSOR);
        }
        // Materialize once; the flat buffer feeds both the domain check and
        // every batch slice below.
        let flat = self.planata();
        for &value in &flat {
            if !value.is_finite() {
                return Err(ERR_SOFTMAX_NON_FINITE_INPUT);
            }
        }
        let rank = self.shape.len();
        // v1: rank-1 (single axis) or rank-2 (axis 1 — the last axis).
        let last_dim = self.shape[rank - 1];
        let batch = self.element_count() / last_dim;

        let mut out_data = Vec::with_capacity(self.element_count());
        for b in 0..batch {
            let base = b * last_dim;
            // Find max for numerical stability.
            let mut max_val = f32::NEG_INFINITY;
            for i in 0..last_dim {
                max_val = max_val.max(flat[base + i]);
            }
            // Compute exp(x_i - max) and sum.
            let mut exps = Vec::with_capacity(last_dim);
            let mut exp_sum = 0.0_f32;
            for i in 0..last_dim {
                let exp_val = (flat[base + i] - max_val).exp();
                exps.push(exp_val);
                exp_sum += exp_val;
            }
            // Normalize.
            for exp_val in exps {
                out_data.push(exp_val / exp_sum);
            }
        }
        Ok(Tensor::from_contiguous(out_data, self.shape.clone()))
    }

    /// Cross-entropy loss with internal softmax over the last axis.
    ///
    /// Computes `-sum(targets * log(softmax(logits) + ε)) / N` where
    /// ε = 1e-7 for numerical stability and N = last_dim (number of classes).
    /// Operates on rank-1 (single example) or rank-2 (batched, row-wise).
    ///
    /// Cross-entropy loss: `-sum(targets * log(softmax(logits) + ε)) / N`.
    /// Loss is normalized by N = last dimension (number of classes).
    ///
    /// Domain validation: non-empty, finite logits, finite targets,
    /// targets in [0, 1], matching shapes, rank 1 or 2.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any domain constraint is violated.
    pub fn crux_entropia(&self, targets: &Tensor<f32>) -> Result<f32, &'static str> {
        if self.element_count() == 0 {
            return Err(ERR_CRUX_ENTROPIA_EMPTY_TENSOR);
        }
        for &value in self.planata().iter() {
            if !value.is_finite() {
                return Err(ERR_CRUX_ENTROPIA_NON_FINITE_INPUT);
            }
        }
        for &value in targets.planata().iter() {
            if !value.is_finite() {
                return Err(ERR_CRUX_ENTROPIA_TARGET_NON_FINITE);
            }
            if !(0.0..=1.0).contains(&value) {
                return Err(ERR_CRUX_ENTROPIA_TARGET_RANGE);
            }
        }
        if self.magnitudines() != targets.magnitudines() {
            return Err(ERR_CRUX_ENTROPIA_SHAPE_MISMATCH);
        }
        let rank = self.shape.len();
        if !(1..=2).contains(&rank) {
            return Err(ERR_CRUX_ENTROPIA_RANK);
        }

        let softmax = self.softmax()?;
        let eps = 1e-7_f32;
        let last_dim = self.shape[rank - 1] as f32;

        let mut sum = 0.0_f32;
        let softmax_data = softmax.planata();
        let targets_data = targets.planata();
        for (s, &t) in softmax_data.iter().zip(targets_data.iter()) {
            sum -= t * (s + eps).ln();
        }
        // Normalize by N = last_dim (class count), not by batch size.
        // For rank-1 shape [C]: N = C, batch = 1.
        // For rank-2 shape [B, C]: N = C, batch = B.
        Ok(sum / last_dim)
    }

    /// Elementwise scalar multiplication preserving tensor shape.
    #[must_use]
    pub fn scala(&self, factor: f32) -> Tensor<f32> {
        Tensor::from_contiguous(
            self.planata()
                .into_iter()
                .map(|value| value * factor)
                .collect(),
            self.shape.clone(),
        )
    }

    /// Elementwise checked division after NumPy-style broadcast unification.
    ///
    /// # Errors
    ///
    /// Returns `Err` if shapes are not broadcast-compatible, the element count
    /// overflows, any input is non-finite, the denominator is zero, or the
    /// result is non-finite.
    pub fn divide(&self, other: &Tensor<f32>) -> Result<Tensor<f32>, &'static str> {
        let shape = broadcast_shape(&self.shape, &other.shape)?;
        let count = checked_allocation_count::<f32>(&shape)?;
        let mut data = Vec::with_capacity(count);
        for ordinal in 0..count {
            let index = unravel_index(ordinal, &shape);
            let lhs_index = broadcast_index(&index, &self.shape);
            let rhs_index = broadcast_index(&index, &other.shape);
            data.push(checked_divide_f32(
                self.value_at_logical(&lhs_index),
                other.value_at_logical(&rhs_index),
            )?);
        }
        Ok(Tensor::from_contiguous(data, shape))
    }

    /// Elementwise checked reciprocal preserving tensor shape.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any input is non-finite, zero, or the division
    /// produces a non-finite result.
    pub fn reciproca(&self) -> Result<Tensor<f32>, &'static str> {
        let data = self
            .planata()
            .into_iter()
            .map(|value| checked_divide_f32(1.0, value))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Tensor::from_contiguous(data, self.shape.clone()))
    }

    /// Mean of all elements as an f32 scalar.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the tensor has zero elements.
    pub fn media(&self) -> Result<f32, &'static str> {
        let count = self.element_count();
        if count == 0 {
            return Err(ERR_MEDIA_EMPTY);
        }
        // SAFETY: intentional f32 mean; precision loss acceptable for large element counts.
        #[allow(clippy::cast_precision_loss)]
        Ok(self.summa() / count as f32)
    }

    /// Layer normalization over a specified axis.
    ///
    /// Computes `(x - mean) / sqrt(var + eps)` followed by optional affine
    /// transform `result * gamma + beta`. Mean and variance are computed
    /// over the normalization axis independently for each slice.
    ///
    /// Domain validation rejects non-finite input, empty tensors, rank > 2,
    /// out-of-range axis, shape-mismatched gamma/beta, non-finite
    /// gamma/beta, and invalid epsilon.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any domain constraint is violated.
    pub fn layernorm(
        &self,
        axis: i64,
        epsilon: f32,
        gamma: Option<&Tensor<f32>>,
        beta: Option<&Tensor<f32>>,
    ) -> Result<Tensor<f32>, &'static str> {
        // Domain validation
        if !epsilon.is_finite() || epsilon <= 0.0 {
            return Err(ERR_LAYERNORM_EPSILON_INVALID);
        }
        let rank = self.shape.len();
        if self.element_count() == 0 {
            return Err(ERR_LAYERNORM_EMPTY_TENSOR);
        }
        if rank > 2 {
            return Err(ERR_LAYERNORM_RANK_TOO_HIGH);
        }
        if rank == 0 {
            return Err(ERR_LAYERNORM_RANK_TOO_HIGH);
        }
        let axis_usize = parse_non_negative(axis, ERR_LAYERNORM_AXIS_OUT_OF_RANGE)?;
        if axis_usize >= rank {
            return Err(ERR_LAYERNORM_AXIS_OUT_OF_RANGE);
        }

        let input_data = self.planata();
        for &value in &input_data {
            if !value.is_finite() {
                return Err(ERR_LAYERNORM_NON_FINITE_INPUT);
            }
        }

        // Validate gamma
        if let Some(g) = gamma {
            let g_data = g.planata();
            if g.shape.len() != 1 || g.shape[0] != self.shape[axis_usize] {
                return Err(ERR_LAYERNORM_GAMMA_SHAPE_MISMATCH);
            }
            for &value in &g_data {
                if !value.is_finite() {
                    return Err(ERR_LAYERNORM_GAMMA_NON_FINITE);
                }
            }
        }

        // Validate beta
        if let Some(b) = beta {
            let b_data = b.planata();
            if b.shape.len() != 1 || b.shape[0] != self.shape[axis_usize] {
                return Err(ERR_LAYERNORM_BETA_SHAPE_MISMATCH);
            }
            for &value in &b_data {
                if !value.is_finite() {
                    return Err(ERR_LAYERNORM_BETA_NON_FINITE);
                }
            }
        }

        // Forward computation: manual shape-loops
        if rank == 1 {
            // Normalize over the entire vector
            let cols = self.shape[0];

            // Compute mean
            let mean: f64 = input_data.iter().map(|&v| v as f64).sum::<f64>() / cols as f64;
            let mean = mean as f32;

            // Compute variance
            let var: f64 = input_data
                .iter()
                .map(|&v| {
                    let d = v as f64 - mean as f64;
                    d * d
                })
                .sum::<f64>()
                / cols as f64;
            let var = var as f32;

            let inv_std = 1.0 / (var + epsilon).sqrt();

            // Compute normalized and optionally affine
            let gamma_data = gamma.map(|g| g.planata());
            let beta_data = beta.map(|b| b.planata());
            let result: Vec<f32> = input_data
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let centered = v - mean;
                    let norm = centered * inv_std;
                    match (&gamma_data, &beta_data) {
                        (Some(g), Some(b)) => norm * g[i] + b[i],
                        (Some(g), None) => norm * g[i],
                        (None, Some(b)) => norm + b[i],
                        (None, None) => norm,
                    }
                })
                .collect();
            Ok(Tensor::from_contiguous(result, vec![cols]))
        } else {
            // rank == 2, axis 0 or 1
            let rows = self.shape[0];
            let cols = self.shape[1];

            // For axis=1: normalize each row independently
            // For axis=0: normalize each column independently
            let normalize_along_cols = axis_usize == 1;

            // Hoisted once: gamma/beta are fixed for the call, so their flat
            // data feeds every row/column slice instead of being re-materialized
            // per element (mirrors the rank-1 branch above).
            let gamma_data = gamma.map(|g| g.planata());
            let beta_data = beta.map(|b| b.planata());

            let result: Vec<f32> = if normalize_along_cols {
                let mut result = vec![0.0_f32; rows * cols];

                for r in 0..rows {
                    let row_start = r * cols;
                    let row_end = row_start + cols;
                    let row_data = &input_data[row_start..row_end];

                    // Mean
                    let mean: f64 = row_data.iter().map(|&v| v as f64).sum::<f64>() / cols as f64;
                    let mean = mean as f32;

                    // Variance
                    let var: f64 = row_data
                        .iter()
                        .map(|&v| {
                            let d = v as f64 - mean as f64;
                            d * d
                        })
                        .sum::<f64>()
                        / cols as f64;
                    let var = var as f32;

                    let inv_std = 1.0 / (var + epsilon).sqrt();

                    for c in 0..cols {
                        let idx = row_start + c;
                        let centered = input_data[idx] - mean;
                        let norm = centered * inv_std;

                        result[idx] = match (&gamma_data, &beta_data) {
                            (Some(g), Some(b)) => norm * g[c] + b[c],
                            (Some(g), None) => norm * g[c],
                            (None, Some(b)) => norm + b[c],
                            (None, None) => norm,
                        };
                    }
                }
                result
            } else {
                // axis=0: normalize each column independently
                let mut result = vec![0.0_f32; rows * cols];

                for c in 0..cols {
                    // Collect column data
                    let mut col_sum: f64 = 0.0;
                    for r in 0..rows {
                        col_sum += input_data[r * cols + c] as f64;
                    }
                    let mean = (col_sum / rows as f64) as f32;

                    let mut col_var: f64 = 0.0;
                    for r in 0..rows {
                        let v = input_data[r * cols + c];
                        let d = v as f64 - mean as f64;
                        col_var += d * d;
                    }
                    let var = (col_var / rows as f64) as f32;
                    let inv_std = 1.0 / (var + epsilon).sqrt();

                    for r in 0..rows {
                        let idx = r * cols + c;
                        let centered = input_data[idx] - mean;
                        let norm = centered * inv_std;

                        result[idx] = match (&gamma_data, &beta_data) {
                            (Some(g), Some(b)) => norm * g[r] + b[r],
                            // Gamma/beta for axis=0: shape matches rows, apply per row.
                            (Some(g), None) => norm * g[r],
                            (None, Some(b)) => norm + b[r],
                            (None, None) => norm,
                        };
                    }
                }
                result
            };

            Ok(Tensor::from_contiguous(result, vec![rows, cols]))
        }
    }
}

fn checked_divide_f32(numerator: f32, denominator: f32) -> Result<f32, &'static str> {
    if !numerator.is_finite() || !denominator.is_finite() {
        return Err(ERR_DIVIDE_NON_FINITE_INPUT);
    }
    if denominator == 0.0 {
        return Err(ERR_DIVIDE_ZERO_DENOMINATOR);
    }
    let result = numerator / denominator;
    if !result.is_finite() {
        return Err(ERR_DIVIDE_NON_FINITE_RESULT);
    }
    Ok(result)
}

// WHY: matmul needs both `Add` and `Mul` trait bounds since the contraction
// sums products. Placing it in its own impl block keeps the `Add` bound
// scoped to matmul without polluting the elementwise `Mul` block.
impl<T> Tensor<T>
where
    T: Clone + Default + std::ops::Add<Output = T> + std::ops::Mul<Output = T>,
{
    /// Rank-2 matrix multiply `self × other`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if either tensor is not rank-2, the inner dimensions do
    /// not match, or the result element count overflows.
    pub fn matmul(&self, other: &Tensor<T>) -> Result<Tensor<T>, &'static str> {
        let dims = &self.shape;
        if dims.len() != 2 {
            return Err(ERR_MATMUL_RECEIVER_RANK);
        }
        let other_dims = &other.shape;
        if other_dims.len() != 2 {
            return Err(ERR_MATMUL_ARGUMENT_RANK);
        }
        let m = dims[0];
        let k1 = dims[1];
        let k2 = other_dims[0];
        let n = other_dims[1];
        if k1 != k2 {
            return Err(ERR_MATMUL_INNER_DIMENSION);
        }
        // WHY: explicit O(M*K*N) contraction loop keeps the kernel readable and
        // works for materialized tensors and views through descriptor offsets.
        let result_count = checked_allocation_count::<T>(&[m, n])?;
        let mut result = Vec::with_capacity(result_count);
        for i in 0..m {
            for j in 0..n {
                let mut acc = T::default();
                for k in 0..k1 {
                    let prod = self.value_at_logical(&[i, k]) * other.value_at_logical(&[k, j]);
                    acc = acc + prod;
                }
                result.push(acc);
            }
        }
        Ok(Tensor::from_contiguous(result, vec![m, n]))
    }
}

#[cfg(test)]
#[path = "tensor_test.rs"]
mod tests;
