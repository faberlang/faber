//! Dense numeric tensor runtime for generated Rust code.

use std::fmt::Debug;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::tensor_edge::{
    BuiltinEdgePolicy, CustomEdgePolicyError, CustomEdgeValueError, TensorEdgeDescriptor,
    TensorEdgeResolution, resolve_builtin, resolve_custom_value, resolve_optional,
};

/// The edge rule used to resolve a shifted tensor view.
#[derive(Debug)]
pub enum TensorEdgePolicy<T> {
    Clamp,
    Reflect,
    Wrap,
    Custom {
        remap: fn(&[i64]) -> Vec<i64>,
        transform: Option<fn(&[i64], T) -> T>,
    },
}

impl<T> Copy for TensorEdgePolicy<T> {}

impl<T> Clone for TensorEdgePolicy<T> {
    fn clone(&self) -> Self {
        *self
    }
}

#[derive(Clone, Debug)]
struct TensorLogicalView<P> {
    base_shape: Vec<usize>,
    base_strides: Vec<usize>,
    base_offset: usize,
    layers: Vec<TensorViewLayer<P>>,
}

#[derive(Clone, Debug)]
enum TensorViewLayer<P> {
    Shift {
        shape: Vec<usize>,
        descriptor: TensorEdgeDescriptor<P>,
    },
    Reshape {
        input_shape: Vec<usize>,
        output_shape: Vec<usize>,
    },
    SliceAxisZero {
        input_shape: Vec<usize>,
        start: usize,
        step: usize,
    },
    TransposeTrailingAxes {
        input_shape: Vec<usize>,
    },
    Expanded {
        input_shape: Vec<usize>,
        target_to_source: Vec<Option<usize>>,
    },
}

/// Homogeneous numeric buffer with runtime shape metadata.
#[derive(Clone, Debug)]
pub struct Tensor<T> {
    data: Arc<Mutex<Vec<T>>>,
    shape: Vec<usize>,
    strides: Vec<usize>,
    offset: usize,
    logical_view: Option<TensorLogicalView<TensorEdgePolicy<T>>>,
}

// ── Error messages ─────────────────────────────────────────────────────────
// Shared error strings are the contract authority's; ops implemented in this
// file own their local domain messages. Both surfaces resolve through
// `faber::tensor::*`.

// Contract-authority re-exports: the single canonical definition lives at
// radix-runtime-contract/src/tensor.rs (the compiler-side authority).
pub use crate::contract::tensor::{
    ERR_ACCIPE_INVALID_INDEX, ERR_BROADCAST_SHAPE, ERR_CREA_INVALID_SHAPE,
    ERR_DIVIDE_NON_FINITE_INPUT, ERR_DIVIDE_NON_FINITE_RESULT, ERR_DIVIDE_ZERO_DENOMINATOR,
    ERR_ELEMENT_COUNT_OVERFLOW, ERR_FORMA_ELEMENT_COUNT, ERR_FORMA_LAYOUT_NOT_VIEWABLE,
    ERR_FORMA_RESHAPE_COUNT, ERR_INDEX_OUT_OF_BOUNDS, ERR_INVALID_SLICE_RANGE,
    ERR_MATMUL_ARGUMENT_RANK, ERR_MATMUL_BATCH_DIMENSION, ERR_MATMUL_INNER_DIMENSION,
    ERR_MATMUL_RECEIVER_RANK, ERR_MEDIA_EMPTY, ERR_NEGATIVE_DIM, ERR_NEGATIVE_INDEX,
    ERR_NEGATIVE_SLICE, ERR_PERMUTE_AXIS_OUT_OF_RANGE, ERR_PERMUTE_DUPLICATE_AXIS,
    ERR_PERMUTE_NEGATIVE_AXIS, ERR_PERMUTE_RANK, ERR_PONDE_INVALID_INDEX,
    ERR_SECTIO_INVALID_SLICE_BOUNDS, ERR_TENSOR_COALESCE_REQUIRES_OPTIONAL,
    ERR_TENSOR_COPY_INTO_SHAPE_MISMATCH, ERR_TENSOR_EDGE_NOT_SHIFTED,
    ERR_TENSOR_EDGE_POLICY_INVALID, ERR_TENSOR_EDGE_RANK_MISMATCH, ERR_TENSOR_EDGE_READ_ONLY,
    ERR_TENSOR_EDGE_UNRESOLVED_READ, ERR_TENSOR_MATERIALIZE_UNRESOLVED,
    ERR_TENSOR_SHAPE_HOLE_UNDERDETERMINED, ERR_TRANSPOSE_RANK, tensor_dim_non_negative,
    tensor_flat_offset, tensor_shape_element_count,
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
/// # Errors
/// Returns an error when the requested operation cannot be completed.
pub fn tensor_shape_has_element_count(shape: &[i64], actual: usize) -> bool {
    tensor_shape_element_count(shape) == Some(actual)
}

fn shape_dims(shape: &[i64]) -> Result<Vec<usize>, &'static str> {
    shape
        .iter()
        .map(|&dim| parse_non_negative(dim, ERR_NEGATIVE_DIM))
        .collect()
}

fn infer_shape_hole(
    shape: &[Option<i64>],
    element_count: usize,
    mismatch: &'static str,
) -> Result<Vec<i64>, &'static str> {
    let holes: Vec<usize> = shape
        .iter()
        .enumerate()
        .filter_map(|(axis, dimension)| dimension.is_none().then_some(axis))
        .collect();
    if shape.iter().flatten().any(|dimension| *dimension < 0) {
        return Err(ERR_NEGATIVE_DIM);
    }
    if holes.is_empty() {
        return shape
            .iter()
            .map(|dimension| dimension.ok_or(ERR_TENSOR_SHAPE_HOLE_UNDERDETERMINED))
            .collect();
    }
    if holes.len() != 1 {
        return Err(ERR_TENSOR_SHAPE_HOLE_UNDERDETERMINED);
    }

    let hole = holes[0];
    let known_product = shape
        .iter()
        .enumerate()
        .filter_map(|(axis, dimension)| (axis != hole).then_some(*dimension))
        .try_fold(1_usize, |product, dimension| {
            let dimension = usize::try_from(dimension?).ok()?;
            product.checked_mul(dimension)
        })
        .ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?;
    if known_product == 0 {
        return Err(ERR_TENSOR_SHAPE_HOLE_UNDERDETERMINED);
    }
    if !element_count.is_multiple_of(known_product) {
        return Err(mismatch);
    }
    let inferred =
        i64::try_from(element_count / known_product).map_err(|_| ERR_ELEMENT_COUNT_OVERFLOW)?;
    shape
        .iter()
        .enumerate()
        .map(|(axis, dimension)| {
            if axis == hole {
                Ok(inferred)
            } else {
                dimension.ok_or(ERR_TENSOR_SHAPE_HOLE_UNDERDETERMINED)
            }
        })
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
    /// # Errors
    /// Returns an error when the requested operation cannot be completed.
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

    /// Create a tensor from a flat buffer while resolving at most one shape hole.
    ///
    /// A hole is inferred only when the supplied element count determines an
    /// exact, unique non-negative extent. Multiple holes and zero known product
    /// remain ambiguous, including an empty buffer.
    ///
    /// # Errors
    ///
    /// Returns an error for negative dimensions, an ambiguous hole, or a data
    /// count that does not match the resolved shape.
    pub fn structa_with_holes(data: Vec<T>, shape: &[Option<i64>]) -> Result<Self, &'static str> {
        let resolved = infer_shape_hole(
            shape,
            data.len(),
            "tensor element count does not match shape",
        )?;
        Self::structa(data, &resolved)
    }

    /// Flatten logical values in row-major order.
    ///
    /// # Errors
    ///
    /// Returns an error when an unresolved shifted optional element or an
    /// invalid custom edge-policy coordinate would become a plain value.
    pub fn planata(&self) -> Result<Vec<T>, &'static str> {
        if self.has_unresolved_edge() {
            return Err(ERR_TENSOR_EDGE_UNRESOLVED_READ);
        }
        let count = self.element_count();
        let mut values = Vec::with_capacity(count);
        for ordinal in 0..count {
            let index = unravel_index(ordinal, &self.shape);
            values.push(
                self.read_logical_index(&index)?
                    .ok_or(ERR_TENSOR_EDGE_UNRESOLVED_READ)?,
            );
        }
        Ok(values)
    }

    /// Reshape the tensor without copying its backing storage.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any dimension is negative, if the element count does
    /// not match the new shape, or if the logical order cannot be represented
    /// by strides alone.
    pub fn forma(&self, shape: &[i64]) -> Result<Self, &'static str> {
        let dims = shape_dims(shape)?;
        if !tensor_shape_has_element_count(shape, self.element_count()) {
            return Err(ERR_FORMA_RESHAPE_COUNT);
        }
        let strides = reshape_view_strides(&self.shape, &self.strides, &dims)
            .ok_or(ERR_FORMA_LAYOUT_NOT_VIEWABLE)?;
        let logical_view = self.append_view_layer(TensorViewLayer::Reshape {
            input_shape: self.shape.clone(),
            output_shape: dims.clone(),
        });
        Ok(Self::view(
            Arc::clone(&self.data),
            dims,
            strides,
            self.offset,
            logical_view,
        ))
    }

    /// Reshape after inferring a single extent from this tensor's element count.
    ///
    /// # Errors
    ///
    /// Returns an error for negative dimensions, an ambiguous hole, a count
    /// mismatch, or a layout that cannot represent the requested view.
    pub fn forma_with_holes(&self, shape: &[Option<i64>]) -> Result<Self, &'static str> {
        let resolved = infer_shape_hole(shape, self.element_count(), ERR_FORMA_RESHAPE_COUNT)?;
        self.forma(&resolved)
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
        self.read_logical_index(&index)
    }

    /// Write a value at the given indices.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any index is negative or out of bounds.
    pub fn ponde(&mut self, indices: &[i64], value: T) -> Result<(), &'static str> {
        if self.logical_view.is_some() {
            return Err(ERR_TENSOR_EDGE_READ_ONLY);
        }
        let index = index_dims(indices)?;
        let Some(offset) = self.offset_for_index(&index) else {
            return Err(ERR_INDEX_OUT_OF_BOUNDS);
        };
        tensor_data(&self.data)[offset] = value;
        Ok(())
    }

    /// Replace every element in the tensor with one value.
    ///
    /// # Errors
    ///
    /// Returns an error when the logical offsets are invalid or the tensor is
    /// a shifted or resolved edge view, which is read-only.
    pub fn reple(&mut self, value: T) -> Result<(), &'static str> {
        if self.logical_view.is_some() {
            return Err(ERR_TENSOR_EDGE_READ_ONLY);
        }
        let offsets = self.logical_offsets()?;
        let Some((&last_offset, preceding_offsets)) = offsets.split_last() else {
            return Ok(());
        };
        let mut data = tensor_data(&self.data);
        for &offset in preceding_offsets {
            data[offset] = value.clone();
        }
        data[last_offset] = value;
        Ok(())
    }

    /// Copy every logical value of `source` through this tensor's strides into
    /// its existing storage (`target ⇇ source`).
    ///
    /// The destination keeps its allocation: every handle that shares its
    /// storage observes the new values. The source is read in full before the
    /// first write, so a source that shares storage with the destination (a
    /// view of it) copies the values it held when the call began.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the shapes differ, when this tensor is a shifted or
    /// resolved edge view (read-only), or when `source` holds an unresolved
    /// optional element or an invalid edge-policy coordinate. No element is
    /// written on error.
    pub fn write_values(&mut self, source: &Tensor<T>) -> Result<(), &'static str> {
        if self.logical_view.is_some() {
            return Err(ERR_TENSOR_EDGE_READ_ONLY);
        }
        if self.shape != source.shape {
            return Err(ERR_TENSOR_COPY_INTO_SHAPE_MISMATCH);
        }
        let values = source.planata()?;
        let offsets = self.logical_offsets()?;
        let mut data = tensor_data(&self.data);
        for (offset, value) in offsets.into_iter().zip(values) {
            data[offset] = value;
        }
        Ok(())
    }

    /// Element-wise conversion preserving shape metadata.
    ///
    /// Codegen supplies the per-element map so tensor `↦` mirrors scalar conversio
    /// rules (widening casts, fractus→numerus truncation, and so on).
    ///
    /// # Errors
    ///
    /// Returns an error when the source view contains an unresolved optional
    /// element or a custom edge policy produces an invalid coordinate.
    pub fn convert_elements<B, F>(&self, map: F) -> Result<Tensor<B>, &'static str>
    where
        B: Clone + Default,
        F: Fn(T) -> B,
    {
        let elems: Vec<B> = self.planata()?.into_iter().map(map).collect();
        Ok(Tensor::from_contiguous(elems, self.shape.clone()))
    }

    /// View a contiguous slice along axis 0 from `start` (inclusive) to `end` (exclusive).
    ///
    /// # Errors
    ///
    /// Returns `Err` if bounds are negative, `end < start`, or `end` exceeds
    /// the first dimension.
    pub fn sectio(&self, start: i64, end: i64) -> Result<Self, &'static str> {
        self.sectio_strided(start, end, 1)
    }

    /// View an axis-0 slice with a positive step.
    ///
    /// # Errors
    ///
    /// Returns `Err` if bounds are negative, `end < start`, the range exceeds
    /// the first dimension, or `step` is not positive.
    pub fn sectio_strided(&self, start: i64, end: i64, step: i64) -> Result<Self, &'static str> {
        if step <= 0 {
            return Err(ERR_SECTIO_INVALID_SLICE_BOUNDS);
        }
        let (start, end) = slice_bounds(start, end)?;
        if self.shape.is_empty() || end > self.shape[0] {
            return Err(ERR_INDEX_OUT_OF_BOUNDS);
        }
        let step = usize::try_from(step).map_err(|_| ERR_ELEMENT_COUNT_OVERFLOW)?;
        let range_len = end - start;
        let first_dim = range_len / step + usize::from(range_len % step != 0);
        let stride = self.strides[0]
            .checked_mul(step)
            .ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?;
        let offset = self
            .offset
            .checked_add(
                start
                    .checked_mul(self.strides[0])
                    .ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?,
            )
            .ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?;
        let mut shape = self.shape.clone();
        shape[0] = first_dim;
        let mut strides = self.strides.clone();
        strides[0] = stride;
        let logical_view = self.append_view_layer(TensorViewLayer::SliceAxisZero {
            input_shape: self.shape.clone(),
            start,
            step,
        });
        Ok(Self::view(
            Arc::clone(&self.data),
            shape,
            strides,
            offset,
            logical_view,
        ))
    }

    /// Explicitly expand or insert axes as a zero-copy broadcast view.
    ///
    /// Source axes are matched to target axes in order. Exact extents are
    /// preferred; source extents of one may stretch with a zero stride, and
    /// unmatched target axes are inserted with a zero stride. If several
    /// exact mappings are possible, the rightmost mapping is chosen.
    ///
    /// # Errors
    ///
    /// Returns `Err` if a dimension is negative, the target element count
    /// overflows, or no ordered source-axis mapping satisfies the target shape.
    pub fn expanded(&self, shape: &[i64]) -> Result<Self, &'static str> {
        let shape = shape_dims(shape)?;
        checked_element_count_usize(&shape).ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?;
        let (strides, target_to_source) = expanded_view_layout(&self.shape, &self.strides, &shape)?;
        let logical_view = self.append_view_layer(TensorViewLayer::Expanded {
            input_shape: self.shape.clone(),
            target_to_source,
        });
        Ok(Self::view(
            Arc::clone(&self.data),
            shape,
            strides,
            self.offset,
            logical_view,
        ))
    }

    /// Expand a view with holes filled from the statically resolved result shape.
    ///
    /// # Errors
    ///
    /// Returns an error when a requested extent disagrees with the witness,
    /// either shape contains a negative extent, or the expansion is invalid.
    pub fn expanded_with_witness(
        &self,
        shape: &[Option<i64>],
        expected_shape: &[i64],
    ) -> Result<Self, &'static str> {
        if shape.len() != expected_shape.len() {
            return Err(ERR_BROADCAST_SHAPE);
        }
        let resolved: Vec<i64> = shape
            .iter()
            .zip(expected_shape)
            .map(|(requested, &expected)| {
                if expected < 0 {
                    return Err(ERR_NEGATIVE_DIM);
                }
                match requested {
                    Some(requested) if *requested < 0 => Err(ERR_NEGATIVE_DIM),
                    Some(requested) if *requested != expected => Err(ERR_BROADCAST_SHAPE),
                    Some(requested) => Ok(*requested),
                    None => Ok(expected),
                }
            })
            .collect::<Result<_, _>>()?;
        self.expanded(&resolved)
    }

    /// Displace each logical read coordinate while retaining the same storage.
    ///
    /// # Errors
    ///
    /// Returns an error when the offset rank differs from the tensor rank.
    pub fn shift(&self, offsets: &[i64]) -> Result<Self, &'static str> {
        if offsets.len() != self.shape.len() {
            return Err(ERR_TENSOR_EDGE_RANK_MISMATCH);
        }
        let descriptor =
            TensorEdgeDescriptor::shifted(offsets.iter().copied().map(i128::from).collect());
        let mut logical_view = self
            .logical_view
            .clone()
            .unwrap_or_else(|| TensorLogicalView {
                base_shape: self.shape.clone(),
                base_strides: self.strides.clone(),
                base_offset: self.offset,
                layers: Vec::new(),
            });
        logical_view.layers.push(TensorViewLayer::Shift {
            shape: self.shape.clone(),
            descriptor,
        });
        Ok(Self::view(
            Arc::clone(&self.data),
            self.shape.clone(),
            self.strides.clone(),
            self.offset,
            Some(logical_view),
        ))
    }

    /// Resolve the latest shifted optional view without copying its storage.
    ///
    /// # Errors
    ///
    /// Returns an error when no unresolved shifted view is available.
    pub fn limes(&self, policy: TensorEdgePolicy<T>) -> Result<Self, &'static str> {
        let Some(mut logical_view) = self.logical_view.clone() else {
            return Err(ERR_TENSOR_EDGE_NOT_SHIFTED);
        };
        let resolution = match policy {
            TensorEdgePolicy::Clamp => TensorEdgeResolution::Builtin(BuiltinEdgePolicy::Clamp),
            TensorEdgePolicy::Reflect => TensorEdgeResolution::Builtin(BuiltinEdgePolicy::Reflect),
            TensorEdgePolicy::Wrap => TensorEdgeResolution::Builtin(BuiltinEdgePolicy::Wrap),
            custom @ TensorEdgePolicy::Custom { .. } => TensorEdgeResolution::Custom(custom),
        };
        let Some(TensorViewLayer::Shift { descriptor, .. }) =
            logical_view.layers.iter_mut().rev().find(|layer| {
                matches!(
                    layer,
                    TensorViewLayer::Shift {
                        descriptor: TensorEdgeDescriptor {
                            resolution: TensorEdgeResolution::Optional,
                            ..
                        },
                        ..
                    }
                )
            })
        else {
            return Err(ERR_TENSOR_EDGE_NOT_SHIFTED);
        };
        *descriptor = descriptor
            .clone()
            .limes(resolution)
            .ok_or(ERR_TENSOR_EDGE_NOT_SHIFTED)?;
        Ok(Self::view(
            Arc::clone(&self.data),
            self.shape.clone(),
            self.strides.clone(),
            self.offset,
            Some(logical_view),
        ))
    }

    /// # Errors
    /// Returns an error for unresolved optional elements or invalid custom
    /// edge-policy mappings.
    pub fn materialize(&self) -> Result<Self, &'static str> {
        if self.has_unresolved_edge() {
            return Err(ERR_TENSOR_MATERIALIZE_UNRESOLVED);
        }
        Ok(Self::from_contiguous(self.planata()?, self.shape.clone()))
    }

    /// Resolve optional shifted reads with a fallback in one logical pass.
    ///
    /// # Errors
    ///
    /// Returns an error when no unresolved shifted optional view exists or a
    /// resolved custom edge policy produces an invalid coordinate.
    // The owned fallback is cloned only for optional cells; one input can
    // replace any number of missing logical values.
    #[allow(clippy::needless_pass_by_value)]
    pub fn coalesce(&self, fallback: T) -> Result<Self, &'static str> {
        if !self.has_unresolved_edge() {
            return Err(ERR_TENSOR_COALESCE_REQUIRES_OPTIONAL);
        }
        let mut values = Vec::with_capacity(self.element_count());
        for ordinal in 0..self.element_count() {
            let index = unravel_index(ordinal, &self.shape);
            values.push(
                self.read_logical_index(&index)?
                    .unwrap_or_else(|| fallback.clone()),
            );
        }
        Ok(Self::from_contiguous(values, self.shape.clone()))
    }

    /// Transpose the trailing two axes as a zero-copy view.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the tensor rank is less than 2.
    pub fn transpose_rank2(&self) -> Result<Self, &'static str> {
        if self.shape.len() < 2 {
            return Err(ERR_TRANSPOSE_RANK);
        }
        let mut shape = self.shape.clone();
        let mut strides = self.strides.clone();
        let last_axis = shape.len() - 1;
        shape.swap(last_axis - 1, last_axis);
        strides.swap(last_axis - 1, last_axis);
        let logical_view = self.append_view_layer(TensorViewLayer::TransposeTrailingAxes {
            input_shape: self.shape.clone(),
        });
        Ok(Self::view(
            Arc::clone(&self.data),
            shape,
            strides,
            self.offset,
            logical_view,
        ))
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
            data.push(
                self.read_logical_index(&input_index)?
                    .ok_or(ERR_TENSOR_EDGE_UNRESOLVED_READ)?,
            );
        }
        Ok(Self::from_contiguous(data, shape))
    }

    fn from_contiguous(data: Vec<T>, shape: Vec<usize>) -> Self {
        Self {
            data: Arc::new(Mutex::new(data)),
            strides: row_major_strides(&shape),
            shape,
            offset: 0,
            logical_view: None,
        }
    }

    fn view(
        data: Arc<Mutex<Vec<T>>>,
        shape: Vec<usize>,
        strides: Vec<usize>,
        offset: usize,
        logical_view: Option<TensorLogicalView<TensorEdgePolicy<T>>>,
    ) -> Self {
        Self {
            data,
            shape,
            strides,
            offset,
            logical_view,
        }
    }

    fn append_view_layer(
        &self,
        layer: TensorViewLayer<TensorEdgePolicy<T>>,
    ) -> Option<TensorLogicalView<TensorEdgePolicy<T>>> {
        let mut logical_view = self.logical_view.clone()?;
        logical_view.layers.push(layer);
        Some(logical_view)
    }

    fn has_unresolved_edge(&self) -> bool {
        self.logical_view.as_ref().is_some_and(|view| {
            view.layers.iter().any(|layer| {
                matches!(
                    layer,
                    TensorViewLayer::Shift {
                        descriptor: TensorEdgeDescriptor {
                            resolution: TensorEdgeResolution::Optional,
                            ..
                        },
                        ..
                    }
                )
            })
        })
    }

    fn read_logical_index(&self, index: &[usize]) -> Result<Option<T>, &'static str> {
        if !index_is_in_bounds(index, &self.shape) {
            return Ok(None);
        }
        let Some(logical_view) = &self.logical_view else {
            let Some(offset) = self.offset_for_index(index) else {
                return Ok(None);
            };
            return Ok(tensor_data(&self.data).get(offset).cloned());
        };
        self.read_through_layers(logical_view, logical_view.layers.len(), index)
    }

    fn read_through_layers(
        &self,
        logical_view: &TensorLogicalView<TensorEdgePolicy<T>>,
        layer_count: usize,
        index: &[usize],
    ) -> Result<Option<T>, &'static str> {
        if layer_count == 0 {
            if !index_is_in_bounds(index, &logical_view.base_shape) {
                return Ok(None);
            }
            return self.base_value_at(logical_view, index).map(Some);
        }

        match &logical_view.layers[layer_count - 1] {
            TensorViewLayer::Shift { shape, descriptor } => {
                self.read_shift_layer(logical_view, layer_count - 1, index, shape, descriptor)
            }
            TensorViewLayer::Reshape {
                input_shape,
                output_shape,
            } => {
                if !index_is_in_bounds(index, output_shape) {
                    return Ok(None);
                }
                let ordinal =
                    row_major_ordinal(index, output_shape).ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?;
                let input_index = unravel_index(ordinal, input_shape);
                self.read_through_layers(logical_view, layer_count - 1, &input_index)
            }
            TensorViewLayer::SliceAxisZero {
                input_shape,
                start,
                step,
            } => {
                if index.len() != input_shape.len() {
                    return Ok(None);
                }
                let mut input_index = index.to_vec();
                let Some(first) = input_index.first_mut() else {
                    return Ok(None);
                };
                *first = start
                    .checked_add(first.checked_mul(*step).ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?)
                    .ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?;
                self.read_through_layers(logical_view, layer_count - 1, &input_index)
            }
            TensorViewLayer::TransposeTrailingAxes { input_shape } => {
                if index.len() != input_shape.len() || input_shape.len() < 2 {
                    return Ok(None);
                }
                let mut input_index = index.to_vec();
                let last = input_index.len() - 1;
                input_index.swap(last - 1, last);
                self.read_through_layers(logical_view, layer_count - 1, &input_index)
            }
            TensorViewLayer::Expanded {
                input_shape,
                target_to_source,
            } => {
                if index.len() != target_to_source.len() {
                    return Ok(None);
                }
                let mut input_index = vec![0; input_shape.len()];
                for (target_axis, source_axis) in target_to_source.iter().enumerate() {
                    if let Some(source_axis) = source_axis {
                        input_index[*source_axis] = if input_shape[*source_axis] == 1 {
                            0
                        } else {
                            index[target_axis]
                        };
                    }
                }
                self.read_through_layers(logical_view, layer_count - 1, &input_index)
            }
        }
    }

    fn read_shift_layer(
        &self,
        logical_view: &TensorLogicalView<TensorEdgePolicy<T>>,
        prior_layer_count: usize,
        index: &[usize],
        shape: &[usize],
        descriptor: &TensorEdgeDescriptor<TensorEdgePolicy<T>>,
    ) -> Result<Option<T>, &'static str> {
        let coordinate: Vec<i128> = index
            .iter()
            .map(|&value| i128::try_from(value).map_err(|_| ERR_ELEMENT_COUNT_OVERFLOW))
            .collect::<Result<_, _>>()?;
        match &descriptor.resolution {
            TensorEdgeResolution::Optional => {
                let Some(mapped) = resolve_optional(&coordinate, &descriptor.shift, shape) else {
                    return Ok(None);
                };
                self.read_through_layers(logical_view, prior_layer_count, &mapped)
            }
            TensorEdgeResolution::Builtin(policy) => {
                let Some(mapped) = resolve_builtin(*policy, &coordinate, &descriptor.shift, shape)
                else {
                    return Ok(None);
                };
                self.read_through_layers(logical_view, prior_layer_count, &mapped)
            }
            TensorEdgeResolution::Custom(TensorEdgePolicy::Custom { remap, transform }) => {
                resolve_custom_value(
                    &coordinate,
                    &descriptor.shift,
                    shape,
                    *remap,
                    |mapped| {
                        self.read_through_layers(logical_view, prior_layer_count, mapped)?
                            .ok_or(ERR_TENSOR_EDGE_UNRESOLVED_READ)
                    },
                    |coordinate, value| match *transform {
                        Some(apply) => apply(coordinate, value),
                        None => value,
                    },
                )
                .map(Some)
                .map_err(map_custom_edge_error)
            }
            TensorEdgeResolution::Custom(_) => Err(ERR_TENSOR_EDGE_POLICY_INVALID),
        }
    }

    fn offset_for_index(&self, index: &[usize]) -> Option<usize> {
        offset_for_layout(index, &self.shape, &self.strides, self.offset)
    }

    fn logical_offsets(&self) -> Result<Vec<usize>, &'static str> {
        let count = self.element_count();
        (0..count)
            .map(|ordinal| {
                let index = unravel_index(ordinal, &self.shape);
                self.offset_for_index(&index).ok_or(ERR_INDEX_OUT_OF_BOUNDS)
            })
            .collect()
    }

    fn value_at_logical(&self, index: &[usize]) -> Result<T, &'static str> {
        self.read_logical_index(index)?
            .ok_or(ERR_TENSOR_EDGE_UNRESOLVED_READ)
    }

    fn base_value_at(
        &self,
        logical_view: &TensorLogicalView<TensorEdgePolicy<T>>,
        index: &[usize],
    ) -> Result<T, &'static str> {
        let offset = offset_for_layout(
            index,
            &logical_view.base_shape,
            &logical_view.base_strides,
            logical_view.base_offset,
        )
        .ok_or(ERR_INDEX_OUT_OF_BOUNDS)?;
        tensor_data(&self.data)
            .get(offset)
            .cloned()
            .ok_or(ERR_INDEX_OUT_OF_BOUNDS)
    }
}

fn map_custom_edge_error(error: CustomEdgeValueError<&'static str>) -> &'static str {
    match error {
        CustomEdgeValueError::Policy(
            CustomEdgePolicyError::CoordinateRankMismatch
            | CustomEdgePolicyError::MappedCoordinateRankMismatch,
        ) => ERR_TENSOR_EDGE_RANK_MISMATCH,
        CustomEdgeValueError::Policy(_) => ERR_TENSOR_EDGE_POLICY_INVALID,
        CustomEdgeValueError::Read(error) => error,
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

fn reshape_view_strides(
    source_shape: &[usize],
    source_strides: &[usize],
    target_shape: &[usize],
) -> Option<Vec<usize>> {
    let element_count = checked_element_count_usize(source_shape)?;
    if element_count != checked_element_count_usize(target_shape)? {
        return None;
    }
    if element_count == 0 {
        return Some(row_major_strides(target_shape));
    }

    // Divide the source layout into maximal physically contiguous chunks,
    // ignoring singleton axes whose stride cannot affect logical iteration.
    let mut chunks = Vec::new();
    let mut source_axis = 0;
    while source_axis < source_shape.len() {
        if source_shape[source_axis] <= 1 {
            source_axis += 1;
            continue;
        }
        let mut last_axis = source_axis;
        let mut chunk_count = source_shape[source_axis];
        let mut next_axis = source_axis + 1;
        while next_axis < source_shape.len() {
            if source_shape[next_axis] <= 1 {
                next_axis += 1;
                continue;
            }
            let contiguous_stride =
                source_strides[next_axis].checked_mul(source_shape[next_axis])?;
            if source_strides[last_axis] != contiguous_stride {
                break;
            }
            chunk_count = chunk_count.checked_mul(source_shape[next_axis])?;
            last_axis = next_axis;
            next_axis += 1;
        }
        chunks.push((chunk_count, source_strides[last_axis]));
        source_axis = next_axis;
    }

    if chunks.is_empty() {
        return Some(row_major_strides(target_shape));
    }

    let target_axes: Vec<usize> = target_shape
        .iter()
        .enumerate()
        .filter_map(|(axis, &extent)| (extent > 1).then_some(axis))
        .collect();
    let mut target_strides = row_major_strides(target_shape);
    let mut target_cursor = 0;
    for (chunk_count, innermost_stride) in chunks {
        let first_target_cursor = target_cursor;
        let mut target_chunk_count = 1_usize;
        while target_chunk_count < chunk_count {
            let axis = *target_axes.get(target_cursor)?;
            target_chunk_count = target_chunk_count.checked_mul(target_shape[axis])?;
            if target_chunk_count > chunk_count {
                return None;
            }
            target_cursor += 1;
        }
        if target_chunk_count != chunk_count {
            return None;
        }

        let mut stride = innermost_stride;
        for target_position in (first_target_cursor..target_cursor).rev() {
            let axis = target_axes[target_position];
            target_strides[axis] = stride;
            stride = stride.checked_mul(target_shape[axis])?;
        }
    }
    if target_cursor != target_axes.len() {
        return None;
    }
    Some(target_strides)
}

fn expanded_view_layout(
    source_shape: &[usize],
    source_strides: &[usize],
    target_shape: &[usize],
) -> Result<(Vec<usize>, Vec<Option<usize>>), &'static str> {
    const IMPOSSIBLE: usize = usize::MAX;

    let source_rank = source_shape.len();
    let target_rank = target_shape.len();
    let mut costs = vec![vec![IMPOSSIBLE; target_rank + 1]; source_rank + 1];
    costs[source_rank].fill(0);
    for source_axis in (0..source_rank).rev() {
        for target_axis in (0..target_rank).rev() {
            let skip_target = costs[source_axis][target_axis + 1];
            let stretch_cost = if source_shape[source_axis] == target_shape[target_axis] {
                0
            } else if source_shape[source_axis] == 1 {
                1
            } else {
                IMPOSSIBLE
            };
            let map_target = if stretch_cost == IMPOSSIBLE
                || costs[source_axis + 1][target_axis + 1] == IMPOSSIBLE
            {
                IMPOSSIBLE
            } else {
                costs[source_axis + 1][target_axis + 1] + stretch_cost
            };
            costs[source_axis][target_axis] = skip_target.min(map_target);
        }
    }
    if costs[0][0] == IMPOSSIBLE {
        return Err(ERR_BROADCAST_SHAPE);
    }

    let mut source_to_target = vec![0; source_rank];
    let (mut source_axis, mut target_axis) = (0, 0);
    while source_axis < source_rank {
        let skip_target = costs[source_axis][target_axis + 1];
        let exact_match = source_shape[source_axis] == target_shape[target_axis];
        let stretch_cost = if exact_match {
            0
        } else if source_shape[source_axis] == 1 {
            1
        } else {
            IMPOSSIBLE
        };
        let map_target = if stretch_cost == IMPOSSIBLE
            || costs[source_axis + 1][target_axis + 1] == IMPOSSIBLE
        {
            IMPOSSIBLE
        } else {
            costs[source_axis + 1][target_axis + 1] + stretch_cost
        };
        if skip_target <= map_target {
            target_axis += 1;
        } else {
            source_to_target[source_axis] = target_axis;
            source_axis += 1;
            target_axis += 1;
        }
    }

    let mut target_strides = vec![0; target_rank];
    let mut target_to_source = vec![None; target_rank];
    for (source_axis, &target_axis) in source_to_target.iter().enumerate() {
        target_to_source[target_axis] = Some(source_axis);
        if source_shape[source_axis] == target_shape[target_axis] {
            target_strides[target_axis] = source_strides[source_axis];
        }
    }
    Ok((target_strides, target_to_source))
}

fn index_is_in_bounds(index: &[usize], shape: &[usize]) -> bool {
    index.len() == shape.len()
        && index
            .iter()
            .zip(shape)
            .all(|(index, extent)| index < extent)
}

fn offset_for_layout(
    index: &[usize],
    shape: &[usize],
    strides: &[usize],
    base_offset: usize,
) -> Option<usize> {
    if index.len() != shape.len() || shape.len() != strides.len() {
        return None;
    }
    let mut offset = base_offset;
    for ((index, extent), stride) in index.iter().zip(shape).zip(strides) {
        if index >= extent {
            return None;
        }
        offset = offset.checked_add(index.checked_mul(*stride)?)?;
    }
    Some(offset)
}

fn row_major_ordinal(index: &[usize], shape: &[usize]) -> Option<usize> {
    if index.len() != shape.len() {
        return None;
    }
    index
        .iter()
        .zip(shape)
        .try_fold(0_usize, |ordinal, (&index, &extent)| {
            if index >= extent {
                return None;
            }
            ordinal.checked_mul(extent)?.checked_add(index)
        })
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

fn permute_axes(axis_order: &[i64], rank: usize) -> Result<Vec<usize>, &'static str> {
    if axis_order.len() != rank {
        return Err(ERR_PERMUTE_RANK);
    }
    let mut parsed = Vec::with_capacity(rank);
    let mut seen = vec![false; rank];
    for &requested_axis in axis_order {
        let axis_index = parse_non_negative(requested_axis, ERR_PERMUTE_NEGATIVE_AXIS)?;
        if axis_index >= rank {
            return Err(ERR_PERMUTE_AXIS_OUT_OF_RANGE);
        }
        if seen[axis_index] {
            return Err(ERR_PERMUTE_DUPLICATE_AXIS);
        }
        seen[axis_index] = true;
        parsed.push(axis_index);
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
    if axis < pad { 1 } else { shape[axis - pad] }
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
            lhs.value_at_logical(&lhs_index)?,
            rhs.value_at_logical(&rhs_index)?,
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
    ///
    /// # Errors
    ///
    /// Returns an error when a logical read is unresolved or a custom edge
    /// policy produces an invalid coordinate.
    pub fn summa(&self) -> Result<T, &'static str> {
        Ok(self
            .planata()?
            .into_iter()
            .fold(T::default(), |acc, value| acc + value))
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
    ///
    /// # Errors
    ///
    /// Returns an error when a logical read is unresolved or a custom edge
    /// policy produces an invalid coordinate.
    pub fn neg(&self) -> Result<Tensor<f32>, &'static str> {
        Ok(Tensor::from_contiguous(
            self.planata()?.into_iter().map(|value| -value).collect(),
            self.shape.clone(),
        ))
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
        let flat = self.planata()?;
        for &value in &flat {
            if !value.is_finite() {
                return Err(ERR_RELU_NON_FINITE_INPUT);
            }
        }
        Ok(Tensor::from_contiguous(
            flat.into_iter().map(|value| value.max(0.0)).collect(),
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
        let flat = self.planata()?;
        for &value in &flat {
            if !value.is_finite() {
                return Err(ERR_SQRT_NON_FINITE_INPUT);
            }
            if value < 0.0 {
                return Err(ERR_SQRT_NEGATIVE_INPUT);
            }
        }
        Ok(Tensor::from_contiguous(
            flat.into_iter().map(f32::sqrt).collect(),
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
        let flat = self.planata()?;
        for &value in &flat {
            if !value.is_finite() {
                return Err(ERR_GELU_NON_FINITE_INPUT);
            }
        }
        let alpha = (2.0 / std::f32::consts::PI).sqrt();
        let beta = 0.044_715;
        Ok(Tensor::from_contiguous(
            flat.into_iter()
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
        let flat = self.planata()?;
        for &value in &flat {
            if !value.is_finite() {
                return Err(ERR_EXP_NON_FINITE_INPUT);
            }
        }
        let mut data = Vec::with_capacity(self.element_count());
        for value in flat {
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
        let flat = self.planata()?;
        for &value in &flat {
            if !value.is_finite() {
                return Err(ERR_LOG_NON_FINITE_INPUT);
            }
            if value <= 0.0 {
                return Err(ERR_LOG_NON_POSITIVE_INPUT);
            }
        }
        let mut data = Vec::with_capacity(self.element_count());
        for value in flat {
            let result = value.ln();
            if !result.is_finite() {
                return Err(ERR_LOG_NON_FINITE_RESULT);
            }
            data.push(result);
        }
        Ok(Tensor::from_contiguous(data, self.shape.clone()))
    }

    /// Softmax: `exp(x_i` - max(x)) / `sum(exp(x_j` - max(x))) with numerical
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
        let flat = self.planata()?;
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
    /// ε = 1e-7 for numerical stability and N = `last_dim` (number of classes).
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
        let logits_data = self.planata()?;
        let targets_data = targets.planata()?;
        for &value in &logits_data {
            if !value.is_finite() {
                return Err(ERR_CRUX_ENTROPIA_NON_FINITE_INPUT);
            }
        }
        for &value in &targets_data {
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
        // The class count must use integer-to-float rounding; repeated f32 addition
        // stops advancing once the count exceeds f32's consecutive integer range.
        #[allow(clippy::cast_precision_loss)]
        let last_dim = self.shape[rank - 1] as f32;

        let mut sum = 0.0_f32;
        let softmax_data = softmax.planata()?;
        for (s, &t) in softmax_data.iter().zip(targets_data.iter()) {
            sum -= t * (s + eps).ln();
        }
        // Normalize by N = last_dim (class count), not by batch size.
        // For rank-1 shape [C]: N = C, batch = 1.
        // For rank-2 shape [B, C]: N = C, batch = B.
        Ok(sum / last_dim)
    }

    /// Elementwise scalar multiplication preserving tensor shape.
    ///
    /// # Errors
    ///
    /// Returns an error when a logical read is unresolved or a custom edge
    /// policy produces an invalid coordinate.
    pub fn scala(&self, factor: f32) -> Result<Tensor<f32>, &'static str> {
        Ok(Tensor::from_contiguous(
            self.planata()?
                .into_iter()
                .map(|value| value * factor)
                .collect(),
            self.shape.clone(),
        ))
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
                self.value_at_logical(&lhs_index)?,
                other.value_at_logical(&rhs_index)?,
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
            .planata()?
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
        Ok(self.summa()? / count as f32)
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

        let input_data = self.planata()?;
        for &value in &input_data {
            if !value.is_finite() {
                return Err(ERR_LAYERNORM_NON_FINITE_INPUT);
            }
        }

        // Validate gamma
        if let Some(g) = gamma {
            let g_data = g.planata()?;
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
            let b_data = b.planata()?;
            if b.shape.len() != 1 || b.shape[0] != self.shape[axis_usize] {
                return Err(ERR_LAYERNORM_BETA_SHAPE_MISMATCH);
            }
            for &value in &b_data {
                if !value.is_finite() {
                    return Err(ERR_LAYERNORM_BETA_NON_FINITE);
                }
            }
        }

        let gamma_data = gamma.map(Tensor::planata).transpose()?;
        let beta_data = beta.map(Tensor::planata).transpose()?;
        if rank == 1 {
            Ok(layernorm_rank1(
                &input_data,
                epsilon,
                gamma_data.as_deref(),
                beta_data.as_deref(),
            ))
        } else {
            Ok(layernorm_rank2(
                &input_data,
                self.shape[0],
                self.shape[1],
                axis_usize,
                epsilon,
                gamma_data.as_deref(),
                beta_data.as_deref(),
            ))
        }
    }
}

fn layernorm_moments<I>(values: I) -> (f32, f32)
where
    I: Iterator<Item = f32> + Clone,
{
    // The moments accumulate in f64 so finite f32 inputs do not overflow their
    // sums. The runtime intentionally narrows each completed moment to f32.
    #[allow(clippy::cast_precision_loss)]
    let count = values.clone().count() as f64;
    let mean = values.clone().map(f64::from).sum::<f64>() / count;
    #[allow(clippy::cast_possible_truncation)]
    let mean = mean as f32;
    let variance = values
        .map(|value| {
            let distance = f64::from(value) - f64::from(mean);
            distance * distance
        })
        .sum::<f64>()
        / count;
    #[allow(clippy::cast_possible_truncation)]
    let variance = variance as f32;
    (mean, variance)
}

fn layernorm_value(
    value: f32,
    mean: f32,
    variance: f32,
    epsilon: f32,
    affine_index: usize,
    gamma: Option<&[f32]>,
    beta: Option<&[f32]>,
) -> f32 {
    let inverse_standard_deviation = 1.0 / (variance + epsilon).sqrt();
    let normalized = (value - mean) * inverse_standard_deviation;
    match (gamma, beta) {
        (Some(gamma), Some(beta)) => normalized * gamma[affine_index] + beta[affine_index],
        (Some(gamma), None) => normalized * gamma[affine_index],
        (None, Some(beta)) => normalized + beta[affine_index],
        (None, None) => normalized,
    }
}

fn layernorm_rank1(
    input: &[f32],
    epsilon: f32,
    gamma: Option<&[f32]>,
    beta: Option<&[f32]>,
) -> Tensor<f32> {
    let (mean, variance) = layernorm_moments(input.iter().copied());
    let result = input
        .iter()
        .enumerate()
        .map(|(index, &value)| layernorm_value(value, mean, variance, epsilon, index, gamma, beta))
        .collect();
    Tensor::from_contiguous(result, vec![input.len()])
}

fn layernorm_rank2(
    input: &[f32],
    rows: usize,
    columns: usize,
    axis: usize,
    epsilon: f32,
    gamma: Option<&[f32]>,
    beta: Option<&[f32]>,
) -> Tensor<f32> {
    let mut result = vec![0.0_f32; rows * columns];
    if axis == 1 {
        for row in 0..rows {
            let row_start = row * columns;
            let row_end = row_start + columns;
            let (mean, variance) = layernorm_moments(input[row_start..row_end].iter().copied());
            for column in 0..columns {
                let index = row_start + column;
                result[index] =
                    layernorm_value(input[index], mean, variance, epsilon, column, gamma, beta);
            }
        }
    } else {
        for column in 0..columns {
            let values = (0..rows).map(|row| input[row * columns + column]);
            let (mean, variance) = layernorm_moments(values);
            for row in 0..rows {
                let index = row * columns + column;
                result[index] =
                    layernorm_value(input[index], mean, variance, epsilon, row, gamma, beta);
            }
        }
    }
    Tensor::from_contiguous(result, vec![rows, columns])
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
    /// Matrix multiply `self × other`, including leading batched axes.
    ///
    /// # Errors
    ///
    /// A rank-2 receiver accepts a rank-1 vector or rank-2 matrix argument.
    /// Rank-3 and rank-4 receivers accept a rank-2-or-higher argument whose
    /// leading batch axes exactly match a prefix of the receiver's batch axes.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the receiver or argument rank is unsupported, the
    /// argument's batch prefix or contraction dimension does not match, or the
    /// result element count overflows.
    pub fn matmul(&self, other: &Tensor<T>) -> Result<Tensor<T>, &'static str> {
        let receiver_rank = self.shape.len();
        if !(2..=4).contains(&receiver_rank) {
            return Err(ERR_MATMUL_RECEIVER_RANK);
        }

        let argument_rank = other.shape.len();
        if receiver_rank == 2 && argument_rank == 1 {
            let rows = self.shape[0];
            let inner = self.shape[1];
            if inner != other.shape[0] {
                return Err(ERR_MATMUL_INNER_DIMENSION);
            }
            let result_count = checked_allocation_count::<T>(&[rows])?;
            let mut result = Vec::with_capacity(result_count);
            for row in 0..rows {
                let mut acc = T::default();
                for k in 0..inner {
                    acc = acc + self.value_at_logical(&[row, k])? * other.value_at_logical(&[k])?;
                }
                result.push(acc);
            }
            return Ok(Tensor::from_contiguous(result, vec![rows]));
        }

        if argument_rank < 2 || argument_rank > receiver_rank {
            return Err(ERR_MATMUL_ARGUMENT_RANK);
        }

        let receiver_batch_rank = receiver_rank - 2;
        let argument_batch_rank = argument_rank - 2;
        for axis in 0..argument_batch_rank {
            if self.shape[axis] != other.shape[axis] {
                return Err(ERR_MATMUL_BATCH_DIMENSION);
            }
        }

        let rows = self.shape[receiver_rank - 2];
        let inner = self.shape[receiver_rank - 1];
        let argument_inner = other.shape[argument_rank - 2];
        let columns = other.shape[argument_rank - 1];
        if inner != argument_inner {
            return Err(ERR_MATMUL_INNER_DIMENSION);
        }

        let mut result_shape = self.shape[..receiver_batch_rank].to_vec();
        result_shape.push(rows);
        result_shape.push(columns);
        let result_count = checked_allocation_count::<T>(&result_shape)?;
        let mut result = Vec::with_capacity(result_count);

        // WHY: explicit O(batch*M*K*N) contraction uses logical indices, so
        // materialized tensors and stride/offset views share one path. RHS
        // batch axes use the matching receiver prefix; any trailing receiver
        // batch axes broadcast the RHS matrix across them.
        if result_count == 0 {
            return Ok(Tensor::from_contiguous(result, result_shape));
        }
        let receiver_batch_shape = &self.shape[..receiver_batch_rank];
        let batch_count =
            checked_element_count_usize(receiver_batch_shape).ok_or(ERR_ELEMENT_COUNT_OVERFLOW)?;
        for batch_ordinal in 0..batch_count {
            let receiver_batch = unravel_index(batch_ordinal, receiver_batch_shape);
            let mut lhs_index = receiver_batch.clone();
            lhs_index.push(0);
            lhs_index.push(0);
            let mut rhs_index = receiver_batch[..argument_batch_rank].to_vec();
            rhs_index.push(0);
            rhs_index.push(0);
            for row in 0..rows {
                lhs_index[receiver_batch_rank] = row;
                for column in 0..columns {
                    rhs_index[argument_batch_rank] = 0;
                    rhs_index[argument_batch_rank + 1] = column;
                    let mut acc = T::default();
                    for k in 0..inner {
                        lhs_index[receiver_batch_rank + 1] = k;
                        rhs_index[argument_batch_rank] = k;
                        acc = acc
                            + self.value_at_logical(&lhs_index)?
                                * other.value_at_logical(&rhs_index)?;
                    }
                    result.push(acc);
                }
            }
        }
        Ok(Tensor::from_contiguous(result, result_shape))
    }
}

#[cfg(test)]
#[path = "tensor_test.rs"]
mod tests;
