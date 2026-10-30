//! Typed dense element storage for runner tensors (design:
//! `docs/factory/gpu-reset/typed-dense-storage-design.md`, unit U0).
//!
//! `TensorValue::data` holds a `DenseCell`; reads and writes reproduce what the
//! runner does with `Vec<Value>` elements and what the device mirror does with
//! `ScalarKind::{encode, decode}`:
//!
//! - an `F32` cell stores `value as f32` (round-to-nearest, overflow to
//!   infinity) and reads back as `Value::Float(f64::from(x))`;
//! - integer kinds read as `Value::Int` (`U64` as `Value::UInt`, the same
//!   carrier `ScalarKind::decode` uses) and store with a range check; an
//!   out-of-range or wrong-carrier value is refused (`set` returns `false`);
//! - the byte view is the little-endian layout of the element array, one
//!   `bool` per byte as `0`/`1`;
//! - a `Record` cell holds a fixed-layout genus tensor as its packed
//!   little-endian row bytes (the layout law of `resolve_struct_layout`), one
//!   `layout.size()` row per element, and decodes a row to a `Value::Struct`
//!   only when an element is read. Its byte view is the row bytes verbatim.

use crate::value::Value;
use handroll::FxHashMap;
use radix_hir::DefId;
use radix_lexer::Symbol;
use radix_mir::{
    MirDeviceViewElement, MirScalarLayout, MirType, MirValidationContext, StructLayoutRefusal,
    resolve_struct_layout,
};
use radix_types::{NumericWidth, Primitive, Type, TypeId, TypeTable};
use std::borrow::Cow;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Element type of a typed dense array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DenseKind {
    F32,
    F64,
    I32,
    I64,
    U8,
    U32,
    U64,
    Bool,
}

impl DenseKind {
    /// Bytes per element in the little-endian view.
    #[must_use]
    pub const fn width(self) -> usize {
        match self {
            Self::U8 | Self::Bool => 1,
            Self::F32 | Self::I32 | Self::U32 => 4,
            Self::F64 | Self::I64 | Self::U64 => 8,
        }
    }
}

/// Backing array of one dense tensor. `Values` is the general boxed path;
/// [`DenseStorage::VectorValues`] marks materialized fresh vector outputs and
/// accounts for their outer element array and per-element lane arrays.
#[derive(Clone, Debug, PartialEq)]
pub enum DenseStorage {
    F32(Vec<f32>),
    F64(Vec<f64>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    U8(Vec<u8>),
    U32(Vec<u32>),
    U64(Vec<u64>),
    Bool(Vec<bool>),
    /// A tensor of fixed-layout genus rows: `bytes.len() == len * layout.size()`.
    Record {
        layout: Arc<RecordLayout>,
        bytes: Vec<u8>,
    },
    Values(Vec<Value>),
    /// Host twin of a fresh device-kernel output that no host access has
    /// touched yet: it reads as `len` zero elements of `kind` but holds no
    /// element array. The first write (a download landing a read range, a
    /// host store) materializes the whole zeros buffer; reads synthesize the
    /// zeros. WHY: a device-resident output's twin is usually read in small
    /// ranges or never, so allocating it at launch spends host memory the
    /// run never touches (device dispatcher, fresh kernel outputs).
    FreshZeros {
        kind: DenseKind,
        len: usize,
    },
    /// [`DenseStorage::FreshZeros`] for a record element: `len` zero rows of
    /// `layout`, deferred the same way.
    FreshRecordZeros {
        layout: Arc<RecordLayout>,
        len: usize,
    },
    /// Deferred zeros for a vector element: each element is an independent
    /// vector of `lanes` zero scalar values. The outer element array and each
    /// vector's lane array are allocated only on the first write.
    FreshVectorZeros {
        lane_kind: DenseKind,
        lanes: u8,
        len: usize,
    },
    /// Materialized vector elements originating as [`DenseStorage::FreshVectorZeros`].
    /// `lanes` lets backing-byte accounting include both the outer array and
    /// each element's lane array without inspecting mutable `Value`s.
    VectorValues {
        items: Vec<Value>,
        lanes: u8,
    },
}

/// A Rust element type that one `DenseStorage` variant holds.
pub trait DenseElement: Copy + 'static {
    const KIND: DenseKind;
    fn view(storage: &DenseStorage) -> Option<&[Self]>;
    fn view_mut(storage: &mut DenseStorage) -> Option<&mut [Self]>;
    /// Little-endian encoding appended to `out`.
    fn write_le(self, out: &mut Vec<u8>);
    /// Decode one `KIND.width()`-byte chunk; `None` for an invalid pattern.
    fn read_le(chunk: &[u8]) -> Option<Self>;
}

macro_rules! numeric_element {
    ($ty:ty, $variant:ident) => {
        impl DenseElement for $ty {
            const KIND: DenseKind = DenseKind::$variant;
            fn view(storage: &DenseStorage) -> Option<&[Self]> {
                match storage {
                    DenseStorage::$variant(items) => Some(items),
                    _ => None,
                }
            }
            fn view_mut(storage: &mut DenseStorage) -> Option<&mut [Self]> {
                match storage {
                    DenseStorage::$variant(items) => Some(items),
                    _ => None,
                }
            }
            fn write_le(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
            fn read_le(chunk: &[u8]) -> Option<Self> {
                chunk.try_into().ok().map(<$ty>::from_le_bytes)
            }
        }
    };
}

numeric_element!(f32, F32);
numeric_element!(f64, F64);
numeric_element!(i32, I32);
numeric_element!(i64, I64);
numeric_element!(u8, U8);
numeric_element!(u32, U32);
numeric_element!(u64, U64);

impl DenseElement for bool {
    const KIND: DenseKind = DenseKind::Bool;
    fn view(storage: &DenseStorage) -> Option<&[Self]> {
        match storage {
            DenseStorage::Bool(items) => Some(items),
            _ => None,
        }
    }
    fn view_mut(storage: &mut DenseStorage) -> Option<&mut [Self]> {
        match storage {
            DenseStorage::Bool(items) => Some(items),
            _ => None,
        }
    }
    fn write_le(self, out: &mut Vec<u8>) {
        out.push(u8::from(self));
    }
    fn read_le(chunk: &[u8]) -> Option<Self> {
        match chunk {
            [0] => Some(false),
            [1] => Some(true),
            _ => None,
        }
    }
}

fn narrow_int<T: TryFrom<i64> + TryFrom<u64>>(value: &Value) -> Option<T> {
    match value {
        Value::Int(v) => T::try_from(*v).ok(),
        Value::UInt(v) => T::try_from(*v).ok(),
        _ => None,
    }
}

/// Whether storing `value` in a `kind` array and reading it back returns the
/// same value (same carrier, bit-identical for floats up to NaN payload).
fn reads_back_exactly(kind: DenseKind, value: &Value) -> bool {
    match (kind, value) {
        (DenseKind::F32, Value::Float(v)) => {
            let rounded = f64::from(*v as f32);
            v.to_bits() == rounded.to_bits() || (v.is_nan() && rounded.is_nan())
        }
        (DenseKind::F64, Value::Float(_))
        | (DenseKind::I64, Value::Int(_))
        | (DenseKind::U64, Value::UInt(_))
        | (DenseKind::Bool, Value::Bool(_)) => true,
        (DenseKind::I32, Value::Int(v)) => i32::try_from(*v).is_ok(),
        (DenseKind::U8, Value::Int(v)) => u8::try_from(*v).is_ok(),
        (DenseKind::U32, Value::Int(v)) => u32::try_from(*v).is_ok(),
        _ => false,
    }
}

/// The typed storage kind of a tensor element type, or `None` when the
/// element stays boxed (vector, matrix, record, text, policy-integer and
/// widths without a dense kind). Policy floats read as the bare float.
pub fn dense_kind_of_type(types: &TypeTable, ty: TypeId) -> Option<DenseKind> {
    match types.get(ty) {
        Type::Alias(_, inner) => dense_kind_of_type(types, *inner),
        Type::Primitive(Primitive::Bivalens) => Some(DenseKind::Bool),
        Type::PolicyFloat(_, width) | Type::SizedNumeric(Primitive::Fractus, width) => {
            float_kind(*width)
        }
        Type::Primitive(Primitive::Numerus) => Some(DenseKind::I64),
        Type::Primitive(Primitive::Fractus) => Some(DenseKind::F64),
        Type::SizedNumeric(Primitive::Numerus, width) => match width {
            NumericWidth::I32 => Some(DenseKind::I32),
            NumericWidth::I64 => Some(DenseKind::I64),
            NumericWidth::U8 => Some(DenseKind::U8),
            NumericWidth::U32 => Some(DenseKind::U32),
            NumericWidth::U64 => Some(DenseKind::U64),
            _ => None,
        },
        _ => None,
    }
}

fn float_kind(width: NumericWidth) -> Option<DenseKind> {
    match width {
        NumericWidth::F32 => Some(DenseKind::F32),
        NumericWidth::F64 => Some(DenseKind::F64),
        _ => None,
    }
}

impl DenseStorage {
    /// `len` zero elements of `kind`.
    #[must_use]
    pub fn zeros(kind: DenseKind, len: usize) -> Self {
        match kind {
            DenseKind::F32 => Self::F32(vec![0.0; len]),
            DenseKind::F64 => Self::F64(vec![0.0; len]),
            DenseKind::I32 => Self::I32(vec![0; len]),
            DenseKind::I64 => Self::I64(vec![0; len]),
            DenseKind::U8 => Self::U8(vec![0; len]),
            DenseKind::U32 => Self::U32(vec![0; len]),
            DenseKind::U64 => Self::U64(vec![0; len]),
            DenseKind::Bool => Self::Bool(vec![false; len]),
        }
    }

    /// Build storage from boxed values.
    ///
    /// With `kind = None` the kind is inferred: all `Float` is `F64`, all
    /// `Int` is `I64`, anything else (including empty) is `Values`. With a
    /// kind every value is narrowed as `set` would; if any value does not fit,
    /// nothing is lost and the result is `Values` (today's behaviour).
    #[must_use]
    pub fn from_values(values: Vec<Value>, kind: Option<DenseKind>) -> Self {
        let kind = kind.or_else(|| match values.first()? {
            Value::Float(_) if values.iter().all(|v| matches!(v, Value::Float(_))) => {
                Some(DenseKind::F64)
            }
            Value::Int(_) if values.iter().all(|v| matches!(v, Value::Int(_))) => {
                Some(DenseKind::I64)
            }
            _ => None,
        });
        let Some(kind) = kind else {
            return Self::Values(values);
        };
        let mut typed = Self::zeros(kind, values.len());
        if values
            .iter()
            .enumerate()
            .all(|(index, value)| typed.set(index, value))
        {
            typed
        } else {
            Self::Values(values)
        }
    }

    /// Build storage from boxed values, typed only when the typed array reads
    /// back exactly the boxed values (same carrier, same bits): a value that
    /// `set` would narrow, round or re-carrier keeps the whole array `Values`.
    /// This is the birth path of runner tensors: it never changes a read.
    #[must_use]
    pub fn from_values_exact(values: Vec<Value>, kind: Option<DenseKind>) -> Self {
        match kind {
            Some(kind) if values.iter().all(|value| reads_back_exactly(kind, value)) => {
                Self::from_values(values, Some(kind))
            }
            _ => Self::Values(values),
        }
    }

    /// Element type, or `None` for boxed, vector and record storage (a record
    /// row is not one scalar kind; see [`DenseStorage::record_layout`]).
    #[must_use]
    pub fn kind(&self) -> Option<DenseKind> {
        match self {
            Self::F32(_) => Some(DenseKind::F32),
            Self::F64(_) => Some(DenseKind::F64),
            Self::I32(_) => Some(DenseKind::I32),
            Self::I64(_) => Some(DenseKind::I64),
            Self::U8(_) => Some(DenseKind::U8),
            Self::U32(_) => Some(DenseKind::U32),
            Self::U64(_) => Some(DenseKind::U64),
            Self::Bool(_) => Some(DenseKind::Bool),
            Self::Record { .. } | Self::Values(_) | Self::VectorValues { .. } => None,
            // A deferred twin answers before any array exists: its kind is
            // part of the deferral (the typed download path checks it).
            Self::FreshZeros { kind, .. } => Some(*kind),
            Self::FreshRecordZeros { .. } | Self::FreshVectorZeros { .. } => None,
        }
    }

    /// `len` zero rows of the record `layout`.
    #[must_use]
    pub fn zeros_record(layout: Arc<RecordLayout>, len: usize) -> Self {
        let bytes = vec![0; len * layout.size()];
        Self::Record { layout, bytes }
    }

    /// Deferred zeros: storage that reads as `len` zero elements of `kind`
    /// and materializes its array at the first write. See [`Self::FreshZeros`].
    #[must_use]
    pub fn fresh_zeros(kind: DenseKind, len: usize) -> Self {
        Self::FreshZeros { kind, len }
    }

    /// [`Self::fresh_zeros`] for a record element: `len` zero rows of `layout`,
    /// deferred until the first write.
    #[must_use]
    pub fn fresh_zeros_record(layout: Arc<RecordLayout>, len: usize) -> Self {
        Self::FreshRecordZeros { layout, len }
    }

    /// Deferred zero vector elements, with independent lane storage per
    /// element when read or materialized.
    #[must_use]
    pub fn fresh_zeros_vector(lane_kind: DenseKind, lanes: u8, len: usize) -> Self {
        Self::FreshVectorZeros {
            lane_kind,
            lanes,
            len,
        }
    }

    /// Whether this storage reads as zeros it has not allocated yet.
    #[must_use]
    pub fn is_deferred_zeros(&self) -> bool {
        matches!(
            self,
            Self::FreshZeros { .. } | Self::FreshRecordZeros { .. } | Self::FreshVectorZeros { .. }
        )
    }

    /// Replace deferred zeros with their materialized array; every other
    /// storage is unchanged. The one conversion every write path and typed
    /// slice view runs through.
    pub fn materialized_zeros(&mut self) {
        match self {
            Self::FreshZeros { kind, len } => {
                let (kind, len) = (*kind, *len);
                *self = Self::zeros(kind, len);
            }
            Self::FreshRecordZeros { layout, len } => {
                let (layout, len) = (Arc::clone(layout), *len);
                *self = Self::zeros_record(layout, len);
            }
            Self::FreshVectorZeros {
                lane_kind,
                lanes,
                len,
            } => {
                let (lane_kind, lanes, len) = (*lane_kind, *lanes, *len);
                *self = Self::VectorValues {
                    items: (0..len)
                        .map(|_| zero_vector_value(lane_kind, lanes))
                        .collect(),
                    lanes,
                };
            }
            _ => {}
        }
    }

    /// Host bytes the element array occupies now. Deferred zeros occupy
    /// nothing until a write materializes them.
    #[must_use]
    pub fn backing_bytes(&self) -> usize {
        match self {
            Self::F32(items) => items.len() * size_of::<f32>(),
            Self::F64(items) => items.len() * size_of::<f64>(),
            Self::I32(items) => items.len() * size_of::<i32>(),
            Self::I64(items) => items.len() * size_of::<i64>(),
            Self::U8(items) => items.len() * size_of::<u8>(),
            Self::U32(items) => items.len() * size_of::<u32>(),
            Self::U64(items) => items.len() * size_of::<u64>(),
            Self::Bool(items) => items.len() * size_of::<bool>(),
            Self::Record { bytes, .. } => bytes.len(),
            Self::Values(items) => items.len() * size_of::<Value>(),
            Self::VectorValues { items, lanes } => {
                items.len() * size_of::<Value>()
                    + items.len() * usize::from(*lanes) * size_of::<Value>()
            }
            Self::FreshZeros { .. }
            | Self::FreshRecordZeros { .. }
            | Self::FreshVectorZeros { .. } => 0,
        }
    }

    /// Record storage over `bytes`, taken as is (no copy). `None` unless the
    /// length is a whole number of `layout.size()` rows.
    #[must_use]
    pub fn from_record_bytes(layout: Arc<RecordLayout>, bytes: Vec<u8>) -> Option<Self> {
        bytes
            .len()
            .is_multiple_of(layout.size())
            .then_some(Self::Record { layout, bytes })
    }

    /// The row layout of a `Record` storage, `None` for every other storage.
    #[must_use]
    pub fn record_layout(&self) -> Option<&Arc<RecordLayout>> {
        match self {
            Self::Record { layout, .. } | Self::FreshRecordZeros { layout, .. } => Some(layout),
            _ => None,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::F32(items) => items.len(),
            Self::F64(items) => items.len(),
            Self::I32(items) => items.len(),
            Self::I64(items) => items.len(),
            Self::U8(items) => items.len(),
            Self::U32(items) => items.len(),
            Self::U64(items) => items.len(),
            Self::Bool(items) => items.len(),
            Self::Record { layout, bytes } => bytes.len() / layout.size(),
            Self::Values(items) | Self::VectorValues { items, .. } => items.len(),
            Self::FreshZeros { len, .. }
            | Self::FreshRecordZeros { len, .. }
            | Self::FreshVectorZeros { len, .. } => *len,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The element as the runner reads it today; `None` out of bounds.
    /// Deferred zeros read as zeros without materializing.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<Value> {
        Some(match self {
            Self::F32(items) => Value::Float(f64::from(*items.get(index)?)),
            Self::F64(items) => Value::Float(*items.get(index)?),
            Self::I32(items) => Value::Int(i64::from(*items.get(index)?)),
            Self::I64(items) => Value::Int(*items.get(index)?),
            Self::U8(items) => Value::Int(i64::from(*items.get(index)?)),
            Self::U32(items) => Value::Int(i64::from(*items.get(index)?)),
            Self::U64(items) => Value::UInt(*items.get(index)?),
            Self::Bool(items) => Value::Bool(*items.get(index)?),
            Self::Record { layout, bytes } => layout.decode(layout.row(bytes, index)?)?,
            Self::Values(items) | Self::VectorValues { items, .. } => items.get(index)?.clone(),
            Self::FreshZeros { kind, len } => {
                if index >= *len {
                    return None;
                }
                zero_value(*kind)
            }
            Self::FreshRecordZeros { layout, len } => {
                if index >= *len {
                    return None;
                }
                layout.decode(&vec![0; layout.size()])?
            }
            Self::FreshVectorZeros {
                lane_kind,
                lanes,
                len,
            } => {
                if index >= *len {
                    return None;
                }
                zero_vector_value(*lane_kind, *lanes)
            }
        })
    }

    /// Store `value` at `index`, rounding or narrowing per kind. `false`
    /// (storage unchanged) for an out-of-bounds index, a value of the wrong
    /// carrier, or an integer outside the kind's range. A store materializes
    /// deferred zeros first.
    #[expect(clippy::cast_possible_truncation)]
    pub fn set(&mut self, index: usize, value: &Value) -> bool {
        self.materialized_zeros();
        match (self, value) {
            (Self::F32(items), Value::Float(v)) => items.get_mut(index).map(|s| *s = *v as f32),
            (Self::F64(items), Value::Float(v)) => items.get_mut(index).map(|s| *s = *v),
            (Self::I32(items), Value::Int(v)) => store(items, index, i32::try_from(*v).ok()),
            (Self::I64(items), Value::Int(v)) => items.get_mut(index).map(|s| *s = *v),
            (Self::U8(items), v) => store(items, index, narrow_int(v)),
            (Self::U32(items), v) => store(items, index, narrow_int(v)),
            (Self::U64(items), v) => store(items, index, narrow_int(v)),
            (Self::Bool(items), Value::Bool(v)) => items.get_mut(index).map(|s| *s = *v),
            (Self::Record { layout, bytes }, v) => {
                let layout = &**layout;
                layout.row_mut(bytes, index).and_then(|row| {
                    let mut encoded = vec![0_u8; row.len()];
                    layout
                        .encode(v, &mut encoded)
                        .then(|| row.copy_from_slice(&encoded))
                })
            }
            (Self::Values(items), v) | (Self::VectorValues { items, .. }, v) => {
                items.get_mut(index).map(|s| *s = v.clone())
            }
            _ => None,
        }
        .is_some()
    }

    /// Every element boxed, as today's `Vec<Value>` storage holds them.
    #[must_use]
    pub fn to_values(&self) -> Vec<Value> {
        match self {
            Self::Values(items) | Self::VectorValues { items, .. } => items.clone(),
            _ => (0..self.len()).filter_map(|i| self.get(i)).collect(),
        }
    }

    /// The first `count` elements boxed; `count` must not exceed `len()`.
    #[must_use]
    pub fn values_prefix(&self, count: usize) -> Vec<Value> {
        match self {
            Self::Values(items) | Self::VectorValues { items, .. } => items[..count].to_vec(),
            _ => (0..count).filter_map(|i| self.get(i)).collect(),
        }
    }

    /// Typed slice; `None` when `T` is not this storage's element type.
    #[must_use]
    pub fn slice<T: DenseElement>(&self) -> Option<&[T]> {
        T::view(self)
    }

    pub fn slice_mut<T: DenseElement>(&mut self) -> Option<&mut [T]> {
        T::view_mut(self)
    }

    /// Run `f` over the little-endian bytes of the element `range`: borrowed
    /// (zero-copy) on little-endian hosts, an owned copy elsewhere. `None` for
    /// boxed storage, an invalid range, or a byte length that overflows
    /// `usize`. Deferred zeros hand back zero bytes without materializing.
    pub fn with_le_bytes<R>(
        &self,
        range: Range<usize>,
        f: impl FnOnce(Cow<'_, [u8]>) -> R,
    ) -> Option<R> {
        match self {
            Self::F32(items) => le_bytes(items, range, f),
            Self::F64(items) => le_bytes(items, range, f),
            Self::I32(items) => le_bytes(items, range, f),
            Self::I64(items) => le_bytes(items, range, f),
            Self::U8(items) => le_bytes(items, range, f),
            Self::U32(items) => le_bytes(items, range, f),
            Self::U64(items) => le_bytes(items, range, f),
            Self::Bool(items) => le_bytes(items, range, f),
            Self::Record { layout, bytes } => {
                let size = layout.size();
                let part =
                    bytes.get(range.start.checked_mul(size)?..range.end.checked_mul(size)?)?;
                Some(f(Cow::Borrowed(part)))
            }
            Self::Values(_) | Self::VectorValues { .. } => None,
            Self::FreshZeros { kind, len } => zero_le_bytes(*len, kind.width(), range, f),
            Self::FreshRecordZeros { layout, len } => zero_le_bytes(*len, layout.size(), range, f),
            Self::FreshVectorZeros {
                lane_kind,
                lanes,
                len,
            } => zero_le_bytes(*len, lane_kind.width() * usize::from(*lanes), range, f),
        }
    }

    /// See [`Self::with_le_bytes`], passing a byte slice rather than a `Cow`.
    pub fn with_byte_slice<R>(&self, range: Range<usize>, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
        self.with_le_bytes(range, |bytes| f(&bytes))
    }

    /// Overwrite the element `range` from little-endian `bytes`. `false`
    /// (storage unchanged) for `Values`, a range outside the array, a byte
    /// length other than `range.len() * width`, or a non-`0`/`1` bool byte.
    /// A write materializes deferred zeros first.
    pub fn write_le_bytes(&mut self, range: Range<usize>, bytes: &[u8]) -> bool {
        self.materialized_zeros();
        match self {
            Self::F32(items) => write_le(items, range, bytes),
            Self::F64(items) => write_le(items, range, bytes),
            Self::I32(items) => write_le(items, range, bytes),
            Self::I64(items) => write_le(items, range, bytes),
            Self::U8(items) => write_le(items, range, bytes),
            Self::U32(items) => write_le(items, range, bytes),
            Self::U64(items) => write_le(items, range, bytes),
            Self::Bool(items) => write_le(items, range, bytes),
            Self::Record {
                layout,
                bytes: rows,
            } => {
                let size = layout.size();
                let (Some(start), Some(end)) =
                    (range.start.checked_mul(size), range.end.checked_mul(size))
                else {
                    return false;
                };
                match rows.get_mut(start..end) {
                    Some(part) if part.len() == bytes.len() => {
                        part.copy_from_slice(bytes);
                        true
                    }
                    _ => false,
                }
            }
            // Deferred variants have already materialized above.
            Self::Values(_)
            | Self::VectorValues { .. }
            | Self::FreshZeros { .. }
            | Self::FreshRecordZeros { .. }
            | Self::FreshVectorZeros { .. } => false,
        }
    }
}

fn store<T>(items: &mut [T], index: usize, value: Option<T>) -> Option<()> {
    let value = value?;
    *items.get_mut(index)? = value;
    Some(())
}

fn zero_vector_value(kind: DenseKind, lanes: u8) -> Value {
    Value::Vector(Rc::new(RefCell::new(
        (0..lanes).map(|_| zero_value(kind)).collect(),
    )))
}

/// The zero element of a dense kind, as [`DenseStorage::get`] reads it: the
/// carrier and value a fresh zeros cell synthesizes.
fn zero_value(kind: DenseKind) -> Value {
    match kind {
        DenseKind::F32 | DenseKind::F64 => Value::Float(0.0),
        DenseKind::I32 | DenseKind::I64 | DenseKind::U8 | DenseKind::U32 => Value::Int(0),
        DenseKind::U64 => Value::UInt(0),
        DenseKind::Bool => Value::Bool(false),
    }
}

fn zero_le_bytes<R>(
    len: usize,
    width: usize,
    range: Range<usize>,
    f: impl FnOnce(Cow<'_, [u8]>) -> R,
) -> Option<R> {
    if range.end > len {
        return None;
    }
    let count = range.end.checked_sub(range.start)?;
    let byte_len = count.checked_mul(width)?;
    Some(f(Cow::Owned(vec![0; byte_len])))
}

fn le_bytes<T: DenseElement, R>(
    items: &[T],
    range: Range<usize>,
    f: impl FnOnce(Cow<'_, [u8]>) -> R,
) -> Option<R> {
    let part = items.get(range)?;
    #[cfg(target_endian = "little")]
    {
        // SAFETY: `T` is one of f32/f64/i32/i64/u8/u32/u64/bool, all
        // padding-free with every byte initialised, so the `size_of_val` bytes
        // starting at `part.as_ptr()` are valid `u8`s (alignment 1). The
        // returned borrow is tied to `part`, which outlives `f`.
        let bytes = unsafe {
            std::slice::from_raw_parts(part.as_ptr().cast::<u8>(), std::mem::size_of_val(part))
        };
        Some(f(Cow::Borrowed(bytes)))
    }
    #[cfg(not(target_endian = "little"))]
    {
        let mut out = Vec::with_capacity(std::mem::size_of_val(part));
        for item in part {
            item.write_le(&mut out);
        }
        Some(f(Cow::Owned(out)))
    }
}

fn write_le<T: DenseElement>(items: &mut [T], range: Range<usize>, bytes: &[u8]) -> bool {
    let width = T::KIND.width();
    let Some(part) = items.get_mut(range) else {
        return false;
    };
    if bytes.len() != part.len() * width {
        return false;
    }
    if !bytes.chunks_exact(width).all(|c| T::read_le(c).is_some()) {
        return false;
    }
    for (slot, chunk) in part.iter_mut().zip(bytes.chunks_exact(width)) {
        if let Some(value) = T::read_le(chunk) {
            *slot = value;
        }
    }
    true
}

/// One field of a [`RecordLayout`]: where it sits in a row and how its bytes
/// read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordField {
    pub name: Symbol,
    pub offset: usize,
    pub scalar: MirScalarLayout,
    /// `Some(n)`: the field is a fixed array of `n` scalars (`list<T, size n>`).
    pub array_len: Option<usize>,
    /// `Some(w)`: the field is a `vector<scalar, w>` register.
    pub lanes: Option<u8>,
}

/// The packed little-endian row layout of one fixed-layout genus, taken from
/// `resolve_struct_layout` (the layout law of record device elements). It
/// carries what a row needs to read back as a `Value::Struct`: the genus
/// definition and each field's name, offset and scalar. It names no format;
/// any genus the layout law admits has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordLayout {
    def: DefId,
    size: usize,
    fields: Vec<RecordField>,
}

impl RecordLayout {
    /// Resolve the row layout of the genus type `ty`.
    ///
    /// # Errors
    /// The named refusal of the layout law when `ty` is not a fixed-layout
    /// genus (generic, nullable, text, nested genus, too large, ...).
    pub fn resolve(
        ty: TypeId,
        ctx: &MirValidationContext<'_>,
    ) -> Result<Self, StructLayoutRefusal> {
        let types = ctx.types.as_table();
        let mut ty = ty;
        while let Type::Alias(_, inner) = types.get(ty) {
            ty = *inner;
        }
        let layout = resolve_struct_layout(MirType::semantic(ty), ctx)?;
        let Type::Struct(def) = types.get(ty) else {
            return Err(StructLayoutRefusal::NotAGenus);
        };
        let declared = ctx.struct_fields.get(def);
        let fields = layout
            .fields
            .iter()
            .map(|field| {
                let is_array = declared
                    .and_then(|fields| fields.get(&field.name))
                    .is_some_and(|ty| {
                        let mut ty = ty.semantic_id();
                        while let Type::Alias(_, inner) = types.get(ty) {
                            ty = *inner;
                        }
                        matches!(types.get(ty), Type::BoundedArray { .. })
                    });
                let (scalar, lanes) = match field.element {
                    MirDeviceViewElement::Scalar(scalar) => (scalar, None),
                    MirDeviceViewElement::Vector(vector) => (vector.element, Some(vector.width)),
                    MirDeviceViewElement::Record(_) => {
                        return Err(StructLayoutRefusal::NestedGenus);
                    }
                };
                let width = scalar_width(scalar).ok_or(StructLayoutRefusal::UnsupportedField)?;
                Ok(RecordField {
                    name: field.name,
                    offset: field.offset as usize,
                    scalar,
                    array_len: is_array.then(|| field.size as usize / width),
                    lanes,
                })
            })
            .collect::<Result<Vec<_>, StructLayoutRefusal>>()?;
        Ok(Self {
            def: *def,
            size: layout.size as usize,
            fields,
        })
    }

    /// Bytes of one row (the layout's struct size, padding included).
    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    /// The genus definition a decoded row carries.
    #[must_use]
    pub fn def(&self) -> DefId {
        self.def
    }

    fn row<'a>(&self, bytes: &'a [u8], index: usize) -> Option<&'a [u8]> {
        let start = index.checked_mul(self.size)?;
        bytes.get(start..start.checked_add(self.size)?)
    }

    fn row_mut<'a>(&self, bytes: &'a mut [u8], index: usize) -> Option<&'a mut [u8]> {
        let start = index.checked_mul(self.size)?;
        bytes.get_mut(start..start.checked_add(self.size)?)
    }

    /// One row as the struct value the runner builds for a plain binding.
    fn decode(&self, row: &[u8]) -> Option<Value> {
        let mut fields = FxHashMap::default();
        for field in &self.fields {
            let width = scalar_width(field.scalar)?;
            let count = field
                .array_len
                .or(field.lanes.map(usize::from))
                .unwrap_or(1);
            let mut items = Vec::with_capacity(count);
            for slot in 0..count {
                let at = field.offset + slot * width;
                items.push(decode_scalar(field.scalar, row.get(at..at + width)?)?);
            }
            let value = if field.array_len.is_some() {
                Value::Array(Rc::new(RefCell::new(items)))
            } else if field.lanes.is_some() {
                Value::Vector(Rc::new(RefCell::new(items)))
            } else {
                items.pop()?
            };
            fields.insert(field.name, value);
        }
        Some(Value::Struct(self.def, Rc::new(RefCell::new(fields))))
    }

    /// Encode `value` into `row` (`size()` zeroed bytes). `false` for a value
    /// that is not this genus, a missing field, or a field out of its range.
    fn encode(&self, value: &Value, row: &mut [u8]) -> bool {
        let Value::Struct(def, fields) = value else {
            return false;
        };
        if *def != self.def {
            return false;
        }
        let fields = fields.borrow();
        self.fields.iter().all(|field| {
            let Some(width) = scalar_width(field.scalar) else {
                return false;
            };
            let Some(value) = fields.get(&field.name) else {
                return false;
            };
            let scalars: Vec<Value> = match (value, field.array_len, field.lanes) {
                (Value::Array(items), Some(count), None) if items.borrow().len() == count => {
                    items.borrow().clone()
                }
                (Value::Vector(items), None, Some(lanes))
                    if items.borrow().len() == usize::from(lanes) =>
                {
                    items.borrow().clone()
                }
                (value, None, None) => vec![value.clone()],
                _ => return false,
            };
            scalars.iter().enumerate().all(|(slot, scalar)| {
                let at = field.offset + slot * width;
                row.get_mut(at..at + width)
                    .is_some_and(|out| encode_scalar(field.scalar, scalar, out))
            })
        })
    }
}

const fn scalar_width(scalar: MirScalarLayout) -> Option<usize> {
    Some(match scalar {
        MirScalarLayout::I8 | MirScalarLayout::U8 => 1,
        MirScalarLayout::I16
        | MirScalarLayout::U16
        | MirScalarLayout::F16
        | MirScalarLayout::Bf16 => 2,
        MirScalarLayout::I32 | MirScalarLayout::U32 | MirScalarLayout::F32 => 4,
        MirScalarLayout::I64 | MirScalarLayout::U64 => 8,
        _ => return None,
    })
}

fn decode_scalar(scalar: MirScalarLayout, bytes: &[u8]) -> Option<Value> {
    Some(match scalar {
        MirScalarLayout::I8 => Value::Int(i64::from(i8::from_le_bytes(bytes.try_into().ok()?))),
        MirScalarLayout::U8 => Value::Int(i64::from(u8::from_le_bytes(bytes.try_into().ok()?))),
        MirScalarLayout::I16 => Value::Int(i64::from(i16::from_le_bytes(bytes.try_into().ok()?))),
        MirScalarLayout::U16 => Value::Int(i64::from(u16::from_le_bytes(bytes.try_into().ok()?))),
        MirScalarLayout::I32 => Value::Int(i64::from(i32::from_le_bytes(bytes.try_into().ok()?))),
        MirScalarLayout::U32 => Value::Int(i64::from(u32::from_le_bytes(bytes.try_into().ok()?))),
        MirScalarLayout::I64 => Value::Int(i64::from_le_bytes(bytes.try_into().ok()?)),
        MirScalarLayout::U64 => Value::UInt(u64::from_le_bytes(bytes.try_into().ok()?)),
        MirScalarLayout::F32 => Value::Float(f64::from(f32::from_le_bytes(bytes.try_into().ok()?))),
        MirScalarLayout::F16 => Value::Float(f64::from(crate::half_to_f32(u16::from_le_bytes(
            bytes.try_into().ok()?,
        )))),
        MirScalarLayout::Bf16 => Value::Float(f64::from(f32::from_bits(
            u32::from(u16::from_le_bytes(bytes.try_into().ok()?)) << 16,
        ))),
        _ => return None,
    })
}

#[expect(clippy::cast_possible_truncation)]
fn encode_scalar(scalar: MirScalarLayout, value: &Value, out: &mut [u8]) -> bool {
    let bytes: Option<Vec<u8>> = match (scalar, value) {
        (MirScalarLayout::I8, v) => narrow_int::<i8>(v).map(|n| n.to_le_bytes().to_vec()),
        (MirScalarLayout::U8, v) => narrow_int::<u8>(v).map(|n| n.to_le_bytes().to_vec()),
        (MirScalarLayout::I16, v) => narrow_int::<i16>(v).map(|n| n.to_le_bytes().to_vec()),
        (MirScalarLayout::U16, v) => narrow_int::<u16>(v).map(|n| n.to_le_bytes().to_vec()),
        (MirScalarLayout::I32, v) => narrow_int::<i32>(v).map(|n| n.to_le_bytes().to_vec()),
        (MirScalarLayout::U32, v) => narrow_int::<u32>(v).map(|n| n.to_le_bytes().to_vec()),
        (MirScalarLayout::I64, v) => narrow_int::<i64>(v).map(|n| n.to_le_bytes().to_vec()),
        (MirScalarLayout::U64, v) => narrow_int::<u64>(v).map(|n| n.to_le_bytes().to_vec()),
        (MirScalarLayout::F32, Value::Float(v)) => Some((*v as f32).to_le_bytes().to_vec()),
        (MirScalarLayout::F16, Value::Float(v)) => {
            Some(crate::f32_to_f16_rn(*v as f32).to_le_bytes().to_vec())
        }
        (MirScalarLayout::Bf16, Value::Float(v)) => {
            let rounded = crate::bf16_round_f64(*v) as f32;
            Some(((rounded.to_bits() >> 16) as u16).to_le_bytes().to_vec())
        }
        _ => None,
    };
    bytes.is_some_and(|bytes| {
        let fits = bytes.len() == out.len();
        if fits {
            out.copy_from_slice(&bytes);
        }
        fits
    })
}

/// Shared-ownership cell around a [`DenseStorage`].
///
/// Single-thread discipline: the API mirrors `RefCell`. Access is
/// closure-scoped, so a borrow can never outlive its call, and a debug build
/// tracks the borrow count and panics on an overlapping mutable and
/// immutable (or two mutable) borrows, as `RefCell` does in every build.
/// Release builds skip the check.
///
/// `Send + Sync`: `unsafe impl` below. SAFETY argument: the cell holds no
/// thread-affine state of its own, and every access goes through the
/// closure-scoped methods above, which hand out `&`/`&mut` to the storage
/// only for the closure's duration. Until the CPU-device lane API (U5) no
/// `DenseCell` crosses a thread, so there is no concurrent access. U5 is the
/// only cross-thread entry; it must (a) reach the storage only through
/// atomic element access over disjoint lanes and (b) refuse `Values`
/// storage, whose `Value` elements hold `Rc` and are not thread-safe. The
/// debug borrow flag is a plain `Cell` and is NOT meaningful across threads.
pub struct DenseCell {
    inner: UnsafeCell<DenseStorage>,
    /// `> 0`: that many shared borrows; `-1`: one exclusive borrow. Only
    /// maintained when `debug_assertions` is on.
    borrow: Cell<isize>,
    /// Number of typed elements converted into boxed `Value`s.
    boxed: AtomicU64,
    /// Number of calls to `set`.
    stored: AtomicU64,
}

// SAFETY: see the type documentation.
unsafe impl Send for DenseCell {}
// SAFETY: see the type documentation.
unsafe impl Sync for DenseCell {}

/// Releases a borrow-flag claim when the closure returns or unwinds.
struct BorrowGuard<'a> {
    flag: &'a Cell<isize>,
    exclusive: bool,
}

impl Drop for BorrowGuard<'_> {
    fn drop(&mut self) {
        if cfg!(debug_assertions) {
            self.flag.set(if self.exclusive {
                0
            } else {
                self.flag.get() - 1
            });
        }
    }
}

impl DenseCell {
    #[must_use]
    pub fn new(storage: DenseStorage) -> Self {
        Self {
            inner: UnsafeCell::new(storage),
            borrow: Cell::new(0),
            boxed: AtomicU64::new(0),
            stored: AtomicU64::new(0),
        }
    }

    #[must_use]
    pub fn zeros(kind: DenseKind, len: usize) -> Self {
        Self::new(DenseStorage::zeros(kind, len))
    }

    #[must_use]
    pub fn from_values(values: Vec<Value>, kind: Option<DenseKind>) -> Self {
        Self::new(DenseStorage::from_values(values, kind))
    }

    pub fn into_inner(self) -> DenseStorage {
        self.inner.into_inner()
    }

    fn read<R>(&self, f: impl FnOnce(&DenseStorage) -> R) -> R {
        if cfg!(debug_assertions) {
            assert!(
                self.borrow.get() >= 0,
                "DenseCell: shared borrow while mutably borrowed"
            );
            self.borrow.set(self.borrow.get() + 1);
        }
        let _guard = BorrowGuard {
            flag: &self.borrow,
            exclusive: false,
        };
        // SAFETY: the single-thread discipline in the type documentation;
        // in debug builds the flag above proves no `&mut` is live.
        f(unsafe { &*self.inner.get() })
    }

    fn write<R>(&self, f: impl FnOnce(&mut DenseStorage) -> R) -> R {
        if cfg!(debug_assertions) {
            assert!(
                self.borrow.get() == 0,
                "DenseCell: mutable borrow while already borrowed"
            );
            self.borrow.set(-1);
        }
        let _guard = BorrowGuard {
            flag: &self.borrow,
            exclusive: true,
        };
        // SAFETY: the single-thread discipline in the type documentation;
        // in debug builds the flag above proves no other borrow is live.
        let storage = unsafe { &mut *self.inner.get() };
        // A write needs the real array: materialize deferred zeros first.
        // One discriminant compare on every write; reads stay deferred.
        storage.materialized_zeros();
        f(storage)
    }

    /// Materialize deferred zeros in place: the typed shared slice view below
    /// hands out bytes, so it runs through here first. Reads elsewhere stay
    /// deferred.
    fn materialize(&self) {
        if !self.read(DenseStorage::is_deferred_zeros) {
            return;
        }
        self.write(|_| {});
    }

    pub fn len(&self) -> usize {
        self.read(DenseStorage::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Element type, or `None` for boxed, vector and record storage.
    pub fn kind(&self) -> Option<DenseKind> {
        self.read(DenseStorage::kind)
    }

    pub fn get(&self, index: usize) -> Option<Value> {
        self.read(|storage| storage.get(index))
    }

    /// Byte size of one row when the storage is `Record`, else `None`.
    pub fn record_size(&self) -> Option<usize> {
        self.read(|storage| storage.record_layout().map(|layout| layout.size()))
    }

    /// See [`DenseStorage::set`]; increments the element-store counter once per call.
    pub fn set(&self, index: usize, value: &Value) -> bool {
        self.write(|storage| {
            let stored = storage.set(index, value);
            self.stored.fetch_add(1, Ordering::Relaxed);
            stored
        })
    }

    /// Every element boxed, as today's `Vec<Value>` storage holds them.
    pub fn to_values(&self) -> Vec<Value> {
        self.read(|storage| {
            let values = storage.to_values();
            if !matches!(
                storage,
                DenseStorage::Values(_) | DenseStorage::VectorValues { .. }
            ) {
                self.boxed.fetch_add(
                    u64::try_from(storage.len()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
            }
            values
        })
    }

    /// Number of typed elements boxed by calls to [`DenseCell::to_values`] and
    /// [`DenseCell::values_prefix`]. `Values` storage is not counted.
    #[must_use]
    pub fn boxed_elements(&self) -> u64 {
        self.boxed.load(Ordering::Relaxed)
    }

    /// Number of calls to [`DenseCell::set`].
    #[must_use]
    pub fn element_stores(&self) -> u64 {
        self.stored.load(Ordering::Relaxed)
    }

    /// Run `f` over boxed elements in either boxed storage variant; `None`
    /// for a typed storage (use [`DenseCell::to_values`]).
    pub fn with_values<R>(&self, f: impl FnOnce(&[Value]) -> R) -> Option<R> {
        self.read(|storage| match storage {
            DenseStorage::Values(items) | DenseStorage::VectorValues { items, .. } => {
                Some(f(items))
            }
            _ => None,
        })
    }

    /// Typed copy of the storage.
    pub fn clone_storage(&self) -> DenseStorage {
        self.read(DenseStorage::clone)
    }

    /// Host bytes the element backing occupies now; deferred zeros occupy
    /// nothing until a write materializes them. For fresh vector outputs this
    /// includes both the outer element array and the per-element lane arrays.
    /// The dispatcher's host storage accounting reads this around downloads.
    #[must_use]
    pub fn backing_bytes(&self) -> usize {
        self.read(DenseStorage::backing_bytes)
    }

    /// The first `count` elements boxed; `count` must not exceed `len()`.
    pub fn values_prefix(&self, count: usize) -> Vec<Value> {
        self.read(|storage| {
            let values = storage.values_prefix(count);
            if !matches!(
                storage,
                DenseStorage::Values(_) | DenseStorage::VectorValues { .. }
            ) {
                self.boxed
                    .fetch_add(u64::try_from(count).unwrap_or(u64::MAX), Ordering::Relaxed);
            }
            values
        })
    }

    /// Typed shared slice for the duration of `f`; `None` on a kind mismatch.
    /// A slice view reads real bytes, so deferred zeros materialize first.
    pub fn with_slice<T: DenseElement, R>(&self, f: impl FnOnce(&[T]) -> R) -> Option<R> {
        self.materialize();
        self.read(|storage| storage.slice::<T>().map(f))
    }

    /// Typed exclusive slice for the duration of `f`; `None` on a kind mismatch.
    pub fn with_slice_mut<T: DenseElement, R>(&self, f: impl FnOnce(&mut [T]) -> R) -> Option<R> {
        self.write(|storage| storage.slice_mut::<T>().map(f))
    }

    /// See [`DenseStorage::with_le_bytes`].
    pub fn with_le_bytes<R>(
        &self,
        range: Range<usize>,
        f: impl FnOnce(Cow<'_, [u8]>) -> R,
    ) -> Option<R> {
        self.read(|storage| storage.with_le_bytes(range, f))
    }

    /// See [`DenseStorage::with_byte_slice`].
    pub fn with_byte_slice<R>(&self, range: Range<usize>, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
        self.read(|storage| storage.with_byte_slice(range, f))
    }

    /// Owned copy of the little-endian bytes of the element `range`.
    pub fn as_le_bytes(&self, range: Range<usize>) -> Option<Vec<u8>> {
        self.with_le_bytes(range, |bytes| bytes.into_owned())
    }

    /// See [`DenseStorage::write_le_bytes`].
    pub fn write_le_bytes(&self, range: Range<usize>, bytes: &[u8]) -> bool {
        self.write(|storage| storage.write_le_bytes(range, bytes))
    }
}

impl std::fmt::Debug for DenseCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.read(|storage| storage.fmt(f))
    }
}

impl PartialEq for DenseCell {
    /// Equality by storage content. A deferred zeros cell is its zeros: both
    /// sides materialize first, so a fresh twin compares equal to the zeros
    /// buffer it stands for (and to another twin of the same shape). Deferred
    /// vector values also compare by their elements with ordinary `Values`.
    fn eq(&self, other: &Self) -> bool {
        self.materialize();
        other.materialize();
        self.read(|a| {
            other.read(|b| match (a, b) {
                (DenseStorage::VectorValues { items, .. }, DenseStorage::Values(other))
                | (DenseStorage::Values(other), DenseStorage::VectorValues { items, .. }) => {
                    items == other
                }
                _ => a == b,
            })
        })
    }
}

impl Clone for DenseCell {
    /// Deep copy of the storage (the typed clone `deep_copy` needs).
    fn clone(&self) -> Self {
        Self::new(self.read(DenseStorage::clone))
    }
}
