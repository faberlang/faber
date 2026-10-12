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
//!   `bool` per byte as `0`/`1`.

use crate::value::Value;
use radix_types::{NumericWidth, Primitive, Type, TypeId, TypeTable};
use std::borrow::Cow;
use std::cell::{Cell, UnsafeCell};
use std::ops::Range;

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

/// Backing array of one dense tensor. `Values` keeps today's behaviour for
/// vector, matrix, record and unknown element kinds.
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
    Values(Vec<Value>),
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

/// Apply `$body` to the typed vector of every non-`Values` variant, bound as
/// `$items`; `$values` handles the `Values` variant.
macro_rules! with_typed {
    ($storage:expr, $items:ident => $body:expr, $values:ident => $values_body:expr) => {
        match $storage {
            DenseStorage::F32($items) => $body,
            DenseStorage::F64($items) => $body,
            DenseStorage::I32($items) => $body,
            DenseStorage::I64($items) => $body,
            DenseStorage::U8($items) => $body,
            DenseStorage::U32($items) => $body,
            DenseStorage::U64($items) => $body,
            DenseStorage::Bool($items) => $body,
            DenseStorage::Values($values) => $values_body,
        }
    };
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
        (DenseKind::F64, Value::Float(_)) | (DenseKind::I64, Value::Int(_)) => true,
        (DenseKind::I32, Value::Int(v)) => i32::try_from(*v).is_ok(),
        (DenseKind::U8, Value::Int(v)) => u8::try_from(*v).is_ok(),
        (DenseKind::U32, Value::Int(v)) => u32::try_from(*v).is_ok(),
        (DenseKind::U64, Value::UInt(_)) | (DenseKind::Bool, Value::Bool(_)) => true,
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
        Type::PolicyFloat(_, width) => float_kind(*width),
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
        Type::SizedNumeric(Primitive::Fractus, width) => float_kind(*width),
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

    /// Element type, or `None` for `Values`.
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
            Self::Values(_) => None,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        with_typed!(self, items => items.len(), values => values.len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The element as the runner reads it today; `None` out of bounds.
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
            Self::Values(items) => items.get(index)?.clone(),
        })
    }

    /// Store `value` at `index`, rounding or narrowing per kind. `false`
    /// (storage unchanged) for an out-of-bounds index, a value of the wrong
    /// carrier, or an integer outside the kind's range.
    #[expect(clippy::cast_possible_truncation)]
    pub fn set(&mut self, index: usize, value: &Value) -> bool {
        match (self, value) {
            (Self::F32(items), Value::Float(v)) => items.get_mut(index).map(|s| *s = *v as f32),
            (Self::F64(items), Value::Float(v)) => items.get_mut(index).map(|s| *s = *v),
            (Self::I32(items), Value::Int(v)) => store(items, index, i32::try_from(*v).ok()),
            (Self::I64(items), Value::Int(v)) => items.get_mut(index).map(|s| *s = *v),
            (Self::U8(items), v) => store(items, index, narrow_int(v)),
            (Self::U32(items), v) => store(items, index, narrow_int(v)),
            (Self::U64(items), v) => store(items, index, narrow_int(v)),
            (Self::Bool(items), Value::Bool(v)) => items.get_mut(index).map(|s| *s = *v),
            (Self::Values(items), v) => items.get_mut(index).map(|s| *s = v.clone()),
            _ => None,
        }
        .is_some()
    }

    /// Every element boxed, as today's `Vec<Value>` storage holds them.
    #[must_use]
    pub fn to_values(&self) -> Vec<Value> {
        match self {
            Self::Values(items) => items.clone(),
            _ => (0..self.len()).filter_map(|i| self.get(i)).collect(),
        }
    }

    /// The first `count` elements boxed; `count` must not exceed `len()`.
    #[must_use]
    pub fn values_prefix(&self, count: usize) -> Vec<Value> {
        match self {
            Self::Values(items) => items[..count].to_vec(),
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
    /// `Values` or a range outside the array.
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
            Self::Values(_) => None,
        }
    }

    /// Overwrite the element `range` from little-endian `bytes`. `false`
    /// (storage unchanged) for `Values`, a range outside the array, a byte
    /// length other than `range.len() * width`, or a non-`0`/`1` bool byte.
    pub fn write_le_bytes(&mut self, range: Range<usize>, bytes: &[u8]) -> bool {
        match self {
            Self::F32(items) => write_le(items, range, bytes),
            Self::F64(items) => write_le(items, range, bytes),
            Self::I32(items) => write_le(items, range, bytes),
            Self::I64(items) => write_le(items, range, bytes),
            Self::U8(items) => write_le(items, range, bytes),
            Self::U32(items) => write_le(items, range, bytes),
            Self::U64(items) => write_le(items, range, bytes),
            Self::Bool(items) => write_le(items, range, bytes),
            Self::Values(_) => false,
        }
    }
}

fn store<T>(items: &mut [T], index: usize, value: Option<T>) -> Option<()> {
    let value = value?;
    *items.get_mut(index)? = value;
    Some(())
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
        f(unsafe { &mut *self.inner.get() })
    }

    pub fn len(&self) -> usize {
        self.read(DenseStorage::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Element type, or `None` for `Values`.
    pub fn kind(&self) -> Option<DenseKind> {
        self.read(DenseStorage::kind)
    }

    pub fn get(&self, index: usize) -> Option<Value> {
        self.read(|storage| storage.get(index))
    }

    /// See [`DenseStorage::set`].
    pub fn set(&self, index: usize, value: &Value) -> bool {
        self.write(|storage| storage.set(index, value))
    }

    pub fn to_values(&self) -> Vec<Value> {
        self.read(DenseStorage::to_values)
    }

    /// Run `f` over the boxed elements when the storage is `Values`; `None`
    /// for a typed storage (use [`DenseCell::to_values`]).
    pub fn with_values<R>(&self, f: impl FnOnce(&[Value]) -> R) -> Option<R> {
        self.read(|storage| match storage {
            DenseStorage::Values(items) => Some(f(items)),
            _ => None,
        })
    }

    /// Typed copy of the storage.
    pub fn clone_storage(&self) -> DenseStorage {
        self.read(DenseStorage::clone)
    }

    /// The first `count` elements boxed; `count` must not exceed `len()`.
    pub fn values_prefix(&self, count: usize) -> Vec<Value> {
        self.read(|storage| storage.values_prefix(count))
    }

    /// Typed shared slice for the duration of `f`; `None` on a kind mismatch.
    pub fn with_slice<T: DenseElement, R>(&self, f: impl FnOnce(&[T]) -> R) -> Option<R> {
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
    /// Equality by storage content.
    fn eq(&self, other: &Self) -> bool {
        self.read(|a| other.read(|b| a == b))
    }
}

impl Clone for DenseCell {
    /// Deep copy of the storage (the typed clone `deep_copy` needs).
    fn clone(&self) -> Self {
        Self::new(self.read(DenseStorage::clone))
    }
}
