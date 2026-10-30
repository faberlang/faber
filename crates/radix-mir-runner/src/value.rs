//! Runner runtime value model and scalar/tensor operations.
//!
//! `Value` is the interpreter-side mirror of MIR operands during stepping.

#![allow(
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::missing_errors_doc,
    clippy::needless_lifetimes,
    clippy::needless_pass_by_value,
    clippy::map_unwrap_or
)]
// Semantic numeric conversions between host value kinds: map-key
// canonicalization, modular-word carriers, width-check i128 intermediates,
// masked shift amounts, and mixed-carrier numeric math all convert between
// i64/u64/f64 by design.
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "semantic numeric conversions between host value kinds in the runner value model"
)]

use super::RunnerError;
use crate::dense::{DenseCell, DenseStorage};
use crate::kernel_dispatch::{DeviceToken, ResidencyHook, ResidencySlot};
use crate::regex_program::RegexProgram;
use crate::tensor_edge::{
    RunnerTensorEdgeDescriptor, TensorEdgeResolution, resolve_builtin, resolve_optional,
};
use handroll::Magnus;
use handroll::{FxHashMap, FxHashSet};
use radix_hir::DefId;
use radix_lexer::{Interner, Span, Symbol};
use radix_mir::MirClosureEnvironmentId;
use radix_mir::{MirBinOp, MirFunctionId};
use radix_runtime_contract::display::{display_bivalens, display_fractus, display_text_payload};
use radix_runtime_contract::display_tokens::DisplayTokens;
use radix_runtime_contract::instans::Instans;
use radix_runtime_contract::trap::text as trap_text;
use radix_types::{FloatPolicy, NumericWidth, Type, TypeId, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Declaration order of each struct's instance fields, from MIR validation
/// metadata; display renders struct fields in this order.
pub type StructFieldOrder = FxHashMap<DefId, Vec<Symbol>>;

/// Declared float width of a struct field, keyed by `(struct, field)`; only
/// non-f64 float fields (or collections of them) are listed.
pub type FieldFloatWidths = FxHashMap<(DefId, Symbol), NumericWidth>;

/// Construction-literal display name of each `genus` and `discretio` variant
/// (`Pulsus`, `Nuntius.Pulsus`), from MIR validation metadata (D2.8).
pub type DisplayNames = FxHashMap<DefId, String>;

/// Struct and variant fields whose declared type is (or holds, through
/// collections and options) a `littera`, keyed by `(def, field)`. Display
/// quotes such a value as a char (`'q'`); every other text, including every
/// `ascii` / `ascii<N>`, quotes as text (`"q"`). The static type decides, never
/// the value: a `'q'` literal reaches the runner as [`Value::Ascii`] whether
/// its slot is a `littera`, an `ascii<1>` or an unbounded `ascii` (pr4).
pub type FieldCharSet = FxHashSet<(DefId, Symbol)>;

/// Declared display shape of a tuple: one entry per element, in order. Only
/// the static type tells a tuple from an interval (both are `Value::Tuple`),
/// and only it tells a `d64` element from an integer one, so a value is shown
/// as `iuncta [a, b]` exactly when this shape is attached to its display.
#[derive(Debug, Clone, PartialEq)]
pub struct TupleParts(pub Vec<TuplePart>);

/// One element of a [`TupleParts`]: the width its value renders at, and the
/// shape of the element itself when it is a tuple (or holds one).
#[derive(Debug, Clone, PartialEq)]
pub struct TuplePart {
    pub width: Option<NumericWidth>,
    /// The element is a raw-bits `bN` (or holds one): it prints as hex.
    pub bits: Option<radix_types::BitsWidth>,
    pub tuple: Option<TupleParts>,
}

/// Struct and variant fields whose declared type holds a tuple.
pub type FieldTupleParts = FxHashMap<(DefId, Symbol), TupleParts>;

/// Struct and variant fields whose declared type is (or holds, through
/// collections and options) a raw-bits `bN`, keyed by `(def, field)`. A `bN`
/// prints as hexadecimal at its full width (need, B12).
pub type FieldBitsWidths = FxHashMap<(DefId, Symbol), radix_types::BitsWidth>;

static NO_BITS_FIELDS: std::sync::LazyLock<FieldBitsWidths> =
    std::sync::LazyLock::new(FxHashMap::default);

/// Hexadecimal display of a raw-bits value: `0x` then lowercase digits at the
/// type's full width (`0x7f` for a `b8`, `0x007f` for a `b16`).
fn display_bits(pattern: u64, width: radix_types::BitsWidth) -> String {
    let digits = usize::try_from(width.bits() / 4).unwrap_or(0);
    format!("0x{pattern:0digits$x}")
}

static NO_TUPLE_FIELDS: std::sync::LazyLock<FieldTupleParts> =
    std::sync::LazyLock::new(FxHashMap::default);

static NO_FLOAT_WIDTHS: std::sync::LazyLock<FieldFloatWidths> =
    std::sync::LazyLock::new(FxHashMap::default);

static NO_CHAR_FIELDS: std::sync::LazyLock<FieldCharSet> =
    std::sync::LazyLock::new(FxHashSet::default);

static LATIN_DISPLAY_TOKENS: std::sync::LazyLock<DisplayTokens> =
    std::sync::LazyLock::new(DisplayTokens::latin);

static NO_DISPLAY_NAMES: std::sync::LazyLock<DisplayNames> =
    std::sync::LazyLock::new(FxHashMap::default);

/// Display metadata threaded through value rendering: declaration order of
/// struct fields and of enum-variant payload fields, declared float widths
/// of struct fields, and the construction-literal names of genus/variants.
#[derive(Clone, Copy)]
pub struct FieldOrder<'a> {
    pub structs: &'a StructFieldOrder,
    pub variants: &'a StructFieldOrder,
    pub float_widths: &'a FieldFloatWidths,
    /// Fields whose declared type holds a `littera` (see [`FieldCharSet`]).
    pub char_fields: &'a FieldCharSet,
    /// The value being shown holds a `littera` per its static type, so an
    /// ascii-carried element prints as a char literal. Reset per field inside a
    /// construction literal, where the field's declared type decides.
    pub char_text: bool,
    pub names: &'a DisplayNames,
    /// The words printed for `bivalens`, the null value and a tuple head: the
    /// tokens of the file the printing function was written in.
    pub tokens: &'a DisplayTokens,
    /// The value is shown through a `valor`/`json` static type: a dynamic
    /// carrier displays as a JSON-shaped object, not a construction literal.
    pub dynamic: bool,
    /// Declared tuple fields (see [`FieldTupleParts`]).
    pub tuple_fields: &'a FieldTupleParts,
    /// The value being shown is (or holds) a tuple per its static type: its
    /// shape. Reset per field and per element, where the declared type decides.
    pub tuple: Option<&'a TupleParts>,
    /// Declared raw-bits fields (see [`FieldBitsWidths`]).
    pub bits_fields: &'a FieldBitsWidths,
    /// The value being shown is (or holds, through collections) a raw-bits
    /// `bN` per its static type: it prints as hexadecimal at the full width.
    /// Reset per field and per element, where the declared type decides.
    pub bits: Option<radix_types::BitsWidth>,
    /// The value being shown is (or holds, through collections) an
    /// `intervallum` per its static type: its tuple-shaped carrier renders as
    /// the interval construction literal (`2‥9`) rather than debug text.
    pub interval: bool,
}

impl<'a> FieldOrder<'a> {
    /// Orders only, with no declared float widths (callers without a type
    /// table, such as compile-time evaluation and tests).
    #[must_use]
    pub fn new(structs: &'a StructFieldOrder, variants: &'a StructFieldOrder) -> Self {
        Self {
            structs,
            variants,
            float_widths: &NO_FLOAT_WIDTHS,
            char_fields: &NO_CHAR_FIELDS,
            char_text: false,
            names: &NO_DISPLAY_NAMES,
            tokens: &LATIN_DISPLAY_TOKENS,
            dynamic: false,
            tuple_fields: &NO_TUPLE_FIELDS,
            tuple: None,
            bits_fields: &NO_BITS_FIELDS,
            bits: None,
            interval: false,
        }
    }

    /// The same metadata for a value whose static type is (or holds) an
    /// `intervallum`.
    #[must_use]
    pub fn with_interval(self, interval: bool) -> Self {
        Self { interval, ..self }
    }

    /// The same metadata for a value whose static type is (or holds) a `bN`.
    #[must_use]
    pub fn with_bits(self, bits: Option<radix_types::BitsWidth>) -> Self {
        Self { bits, ..self }
    }

    /// The same metadata with the declared raw-bits fields attached.
    #[must_use]
    pub fn with_bits_fields(self, bits_fields: &'a FieldBitsWidths) -> Self {
        Self {
            bits_fields,
            ..self
        }
    }

    /// The same metadata for a value whose static type is (or holds) a tuple.
    #[must_use]
    pub fn with_tuple(self, tuple: Option<&'a TupleParts>) -> Self {
        Self { tuple, ..self }
    }

    /// The same metadata with construction-literal names attached.
    #[must_use]
    pub fn with_names(self, names: &'a DisplayNames) -> Self {
        Self { names, ..self }
    }

    /// The same metadata with the output tokens of the printing function.
    #[must_use]
    pub fn with_tokens(self, tokens: &'a DisplayTokens) -> Self {
        Self { tokens, ..self }
    }

    /// The Latin output tokens, the fallback for a function with no entry.
    #[must_use]
    pub fn latin_tokens() -> &'static DisplayTokens {
        &LATIN_DISPLAY_TOKENS
    }

    /// The same metadata for a value whose static type is a dynamic carrier.
    #[must_use]
    pub fn with_dynamic(self, dynamic: bool) -> Self {
        Self { dynamic, ..self }
    }

    /// The same metadata with the declared `littera` fields attached.
    #[must_use]
    pub fn with_char_fields(self, char_fields: &'a FieldCharSet) -> Self {
        Self {
            char_fields,
            ..self
        }
    }

    /// The same metadata for a value whose static type holds a `littera`.
    #[must_use]
    pub fn with_char_text(self, char_text: bool) -> Self {
        Self { char_text, ..self }
    }
}

/// Runtime value carried by the MIR runner.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    /// Unsigned integer magnitude (`numerus<u64>`); never sign-encoded.
    UInt(u64),
    /// Unbounded `inf` integer (inf-delivery rulings 44, 46): the oracle
    /// carrier. **Canonical form**: a `Magnus` holds only a value outside
    /// `[i64::MIN, u64::MAX]`; every value the 64-bit carriers can hold stays
    /// `Int`/`UInt` (see `crate::inf::from_magnus`), so a map key, an equality
    /// or a display never has two spellings of one integer.
    Magnus(Rc<Magnus>),
    /// Exact decimal intermediate (D11.7): `carrier × 10^-scale`. A stored
    /// `d64` is `Int` at scale 8; only a value that is not one (a product of
    /// scale 16, an out-of-range sum) is `Dec`, until a slot stores it.
    Dec(i128, u32),
    Float(f64),
    Bool(bool),
    Str(String),
    /// One-byte `ascii<1>` (`'x'`, `s[i]`). Not a heap `String`.
    Ascii(u8),
    /// One Unicode scalar, `littera` (D10.2): text iteration and cursors. An
    /// ASCII scalar may also arrive as [`Value::Ascii`] (a `'x'` literal or
    /// `ascii[i]`); equality, order, display, and keys join the two by
    /// scalar value.
    Char(char),
    Nil,
    Unit,
    Octeti(Vec<u8>),
    Instans(Instans),
    Regex(Rc<RegexProgram>),
    /// One successful regex match (RD-3: internal only, no source spelling).
    Match(Rc<MatchValue>),
    Sermo(Rc<RefCell<SermoValue>>),
    Function(MirFunctionId),
    Closure(MirFunctionId, MirClosureEnvironmentId, Box<Value>),
    ClosureEnvironment(
        MirClosureEnvironmentId,
        Rc<RefCell<FxHashMap<Symbol, Value>>>,
    ),
    /// Storage of a binding shared by reference with closures
    /// (`MirAggregateKind::SharedCell`); read and written through
    /// `MirProjection::SharedCell`, never observed as a value.
    SharedCell(Rc<RefCell<Value>>),
    Array(Rc<RefCell<Vec<Value>>>),
    Tensor(Rc<RefCell<TensorValue>>),
    /// Register-class vector: lane-ordered cells matching `VectorLane(i)`.
    Vector(Rc<RefCell<Vec<Value>>>),
    /// Register-class matrix: row-major flat cells matching `MatrixCell { row, column }`.
    Matrix(Rc<RefCell<MatrixValue>>),
    Map(Rc<RefCell<FxHashMap<MapKey, Value>>>),
    /// Insertion-ordered `tabulae<K, V>` (L2d.1); see [`LedgerValue`].
    Ledger(Rc<RefCell<LedgerValue>>),
    Set(Rc<RefCell<FxHashSet<MapKey>>>),
    /// Anonymous record: unordered fields plus the order they were written
    /// in the record construct (display order).
    Record(Rc<RefCell<FxHashMap<Symbol, Value>>>, Rc<[Symbol]>),
    Struct(DefId, Rc<RefCell<FxHashMap<Symbol, Value>>>),
    EnumVariant(DefId, Rc<RefCell<FxHashMap<Symbol, Value>>>),
    Tuple(Vec<Value>),
    /// Opaque gradient-handle id for in-process gradient accumulation
    /// (`MirIntrinsic::Gradient`). Carries the handle-key into the gradient
    /// storage map on the runner; never a numeric value.
    GradientHandle(u64),
    /// Suspended `fiunt`/`fient` generator read by a stepped `itera ex` walk
    /// (D6.6); never observed outside that walk's `length`/index reads.
    Cursor(crate::cursor::GeneratorCursor),
}

/// A regex match: the matched text and its half-open span in code points of
/// the haystack (offsets are code points, never bytes), plus the text of each
/// capture group (`0` the whole match; `None` for a group that did not
/// participate) and the `(?P<name>…)` group names with their indices.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchValue {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub groups: Vec<Option<String>>,
    pub names: Vec<(String, usize)>,
}

impl MatchValue {
    /// Text of group `index`; `None` when out of range or not participating.
    #[must_use]
    pub fn group(&self, index: usize) -> Option<&str> {
        self.groups.get(index)?.as_deref()
    }

    /// Text of the group named `name`; `None` when unknown or not participating.
    #[must_use]
    pub fn named(&self, name: &str) -> Option<&str> {
        let (_, index) = self.names.iter().find(|(known, _)| known == name)?;
        self.group(*index)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TensorValue {
    /// Shared flat allocation. Tensor views keep this identity and carry their
    /// own logical shape, strides, and origin.
    pub data: Arc<DenseCell>,
    pub shape: Vec<i64>,
    pub strides: Vec<isize>,
    pub offset: isize,
    is_view: bool,
    pub(crate) edge_base: Option<TensorEdgeBase>,
    pub(crate) edge_ops: Vec<TensorEdgeOp>,
    /// Device residency of the shared backing; absent unless a dispatcher set it.
    residency: ResidencySlot,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TensorEdgeBase {
    pub(crate) shape: Vec<i64>,
    pub(crate) strides: Vec<isize>,
    pub(crate) offset: isize,
}

/// A sequence of logical operations layered over the affine backing layout.
/// Reads traverse this list backward, so views created after a shift remain
/// lazy and preserve the coordinate space in which each shift was declared.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TensorEdgeOp {
    Shift {
        input_shape: Vec<i64>,
        descriptor: RunnerTensorEdgeDescriptor,
    },
    View {
        input_shape: Vec<i64>,
        output_shape: Vec<i64>,
        inverse: TensorInverseMap,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TensorInverseMap {
    SliceAxis0 {
        start: usize,
        step: usize,
    },
    TransposeLastTwo,
    Expand {
        target_to_source: Vec<Option<usize>>,
    },
    Reshape,
}

impl TensorValue {
    /// The rank-0, zero-element tensor: it has no extents whose strides could
    /// fail to be representable.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            data: Arc::new(DenseCell::new(DenseStorage::Values(Vec::new()))),
            shape: Vec::new(),
            strides: Vec::new(),
            offset: 0,
            is_view: false,
            edge_base: None,
            edge_ops: Vec::new(),
            residency: ResidencySlot::default(),
        }
    }

    /// Build a contiguous tensor only when its stride products fit the
    /// runner's signed physical-offset representation.
    #[must_use]
    pub fn try_contiguous(data: Vec<Value>, shape: Vec<i64>) -> Option<Self> {
        Self::try_contiguous_typed(DenseStorage::Values(data), shape)
    }

    /// Build a contiguous tensor over typed dense storage.
    #[must_use]
    pub fn try_contiguous_typed(storage: DenseStorage, shape: Vec<i64>) -> Option<Self> {
        let strides = Self::try_contiguous_strides(&shape)?;
        Some(Self {
            data: Arc::new(DenseCell::new(storage)),
            shape,
            strides,
            offset: 0,
            is_view: false,
            edge_base: None,
            edge_ops: Vec::new(),
            residency: ResidencySlot::default(),
        })
    }

    #[must_use]
    pub fn try_contiguous_strides(shape: &[i64]) -> Option<Vec<isize>> {
        if shape.iter().any(|dimension| *dimension < 0) {
            return None;
        }
        let mut strides = vec![0; shape.len()];
        let mut stride = 1_isize;
        for axis in (0..shape.len()).rev() {
            strides[axis] = stride;
            let dimension = isize::try_from(shape[axis]).ok()?;
            stride = stride.checked_mul(dimension)?;
        }
        Some(strides)
    }

    #[must_use]
    pub fn view(&self, shape: Vec<i64>, strides: Vec<isize>, offset: isize) -> Option<Self> {
        if shape.len() != strides.len()
            || !Self::layout_fits(self.data.len(), &shape, &strides, offset)
        {
            return None;
        }
        Some(Self {
            data: Arc::clone(&self.data),
            shape,
            strides,
            offset,
            is_view: true,
            edge_base: self.edge_base.clone(),
            edge_ops: self.edge_ops.clone(),
            residency: self.residency.clone(),
        })
    }

    #[must_use]
    pub fn view_allowing_out_of_bounds(
        &self,
        shape: Vec<i64>,
        strides: Vec<isize>,
        offset: isize,
    ) -> Option<Self> {
        if shape.len() != strides.len() || shape.iter().any(|dimension| *dimension < 0) {
            return None;
        }
        Some(Self {
            data: Arc::clone(&self.data),
            shape,
            strides,
            offset,
            is_view: true,
            edge_base: self.edge_base.clone(),
            edge_ops: self.edge_ops.clone(),
            residency: self.residency.clone(),
        })
    }

    pub(crate) fn logical_view(
        &self,
        shape: Vec<i64>,
        strides: Vec<isize>,
        offset: isize,
    ) -> Option<Self> {
        if self.edge_base.is_some() {
            self.view_allowing_out_of_bounds(shape, strides, offset)
        } else {
            self.view(shape, strides, offset)
        }
    }

    #[must_use]
    pub fn logical_len(&self) -> Option<usize> {
        if self.shape.is_empty() {
            // `[]` is the rank-zero shape (one scalar) when storage exists.
            // The runner also uses an empty `([], [])` tensor as an unshaped
            // seed before a shape-bearing tensor operation.
            return Some(usize::from(!self.data.is_empty()));
        }
        let count = self.shape.iter().try_fold(1_usize, |count, dimension| {
            usize::try_from(*dimension)
                .ok()
                .and_then(|dimension| count.checked_mul(dimension))
        })?;
        // An empty backing is the runner's established empty-runtime tensor
        // representation, even when its shape is retained as metadata.
        Some(if self.data.is_empty() { 0 } else { count })
    }

    /// Detect a malformed direct contiguous tensor while allowing affine views
    /// to share a larger or non-contiguous backing allocation.
    #[must_use]
    pub(crate) fn has_contiguous_backing_mismatch(&self) -> bool {
        if !self.edge_ops.is_empty() {
            return false;
        }
        if self.is_view {
            return !Self::layout_fits(self.data.len(), &self.shape, &self.strides, self.offset);
        }
        if self.offset != 0 {
            return false;
        }
        let Some(strides) = Self::try_contiguous_strides(&self.shape) else {
            return false;
        };
        if self.strides != strides {
            return false;
        }
        let Some(expected) = self.shape.iter().try_fold(1_usize, |count, dimension| {
            usize::try_from(*dimension)
                .ok()
                .and_then(|dimension| count.checked_mul(dimension))
        }) else {
            return false;
        };
        let data_len = self.data.len();
        data_len != 0 && data_len != expected
    }

    #[must_use]
    pub fn physical_offset(&self, logical_index: usize) -> Option<usize> {
        let count = self.logical_len()?;
        if logical_index >= count || self.strides.len() != self.shape.len() {
            return None;
        }
        let coordinates = decompose_row_major(logical_index, &self.shape)?;
        self.physical_offset_for_coordinates(&coordinates)
    }

    /// Map a logical coordinate to the shared flat allocation. This is the
    /// common seam for ordinary views and lazy shifted-edge reads.
    #[must_use]
    pub fn physical_offset_for_coordinates(&self, coordinates: &[usize]) -> Option<usize> {
        if coordinates.len() != self.shape.len() || self.strides.len() != self.shape.len() {
            return None;
        }
        let mut physical = self.offset;
        for ((coordinate, dimension), stride) in
            coordinates.iter().zip(&self.shape).zip(&self.strides)
        {
            if *coordinate >= usize::try_from(*dimension).ok()? {
                return None;
            }
            physical =
                physical.checked_add(isize::try_from(*coordinate).ok()?.checked_mul(*stride)?)?;
        }
        usize::try_from(physical).ok()
    }

    /// Convert a flat logical index into its row-major logical coordinate.
    #[must_use]
    pub fn logical_coordinates(&self, logical_index: usize) -> Option<Vec<usize>> {
        let count = self.logical_len()?;
        if logical_index >= count {
            return None;
        }
        decompose_row_major(logical_index, &self.shape)
    }

    pub(crate) fn push_edge_shift(&mut self, descriptor: RunnerTensorEdgeDescriptor) {
        if self.edge_base.is_none() {
            self.edge_base = Some(TensorEdgeBase {
                shape: self.shape.clone(),
                strides: self.strides.clone(),
                offset: self.offset,
            });
        }
        self.edge_ops.push(TensorEdgeOp::Shift {
            input_shape: self.shape.clone(),
            descriptor,
        });
    }

    pub(crate) fn resolve_edge_shift(
        &mut self,
        resolution: TensorEdgeResolution<crate::tensor_edge::CustomEdgePolicyId>,
    ) -> bool {
        let Some(TensorEdgeOp::Shift { descriptor, .. }) =
            self.edge_ops.iter_mut().rev().find(|operation| {
                matches!(
                    operation,
                    TensorEdgeOp::Shift { descriptor, .. }
                        if matches!(descriptor.resolution, TensorEdgeResolution::Optional)
                )
            })
        else {
            return false;
        };
        let Some(resolved) = descriptor.clone().limes(resolution) else {
            return false;
        };
        *descriptor = resolved;
        true
    }

    pub(crate) fn has_unresolved_edge_shift(&self) -> bool {
        self.edge_ops.iter().any(|operation| {
            matches!(
                operation,
                TensorEdgeOp::Shift { descriptor, .. }
                    if matches!(descriptor.resolution, TensorEdgeResolution::Optional)
            )
        })
    }

    pub(crate) fn push_edge_view(&mut self, input_shape: Vec<i64>, inverse: TensorInverseMap) {
        if self.edge_base.is_some() {
            self.edge_ops.push(TensorEdgeOp::View {
                input_shape,
                output_shape: self.shape.clone(),
                inverse,
            });
        }
    }

    /// Add broadcast axes or stretch singleton axes without copying storage.
    /// Exact extents are preferred; ties select the rightmost target axes.
    #[must_use]
    pub fn expanded_view(&self, target_shape: Vec<i64>) -> Option<Self> {
        if target_shape.len() < self.shape.len()
            || target_shape.iter().any(|dimension| *dimension < 0)
            || self.shape.iter().any(|dimension| *dimension < 0)
        {
            return None;
        }
        let source_rank = self.shape.len();
        let target_rank = target_shape.len();
        let impossible = usize::MAX;
        let mut costs = vec![vec![impossible; target_rank + 1]; source_rank + 1];
        costs[source_rank].fill(0);
        for source_axis in (0..source_rank).rev() {
            for first_target_axis in (0..target_rank).rev() {
                let mut best = impossible;
                for target_axis in first_target_axis..target_rank {
                    let source_extent = self.shape[source_axis];
                    let target_extent = target_shape[target_axis];
                    let stretch = if source_extent == target_extent {
                        0
                    } else if source_extent == 1 {
                        1
                    } else {
                        continue;
                    };
                    let suffix = costs[source_axis + 1][target_axis + 1];
                    if suffix != impossible {
                        best = best.min(suffix + stretch);
                    }
                }
                costs[source_axis][first_target_axis] = best;
            }
        }
        let &minimum_stretches = costs.first().and_then(|row| row.first())?;
        if minimum_stretches == impossible {
            return None;
        }

        let mut target_strides = vec![0_isize; target_rank];
        let mut target_axis_for_source = vec![0; source_rank];
        let mut first_target_axis = 0;
        let mut remaining_stretches = minimum_stretches;
        for source_axis in 0..source_rank {
            let mut selected = None;
            for target_axis in (first_target_axis..target_rank).rev() {
                let source_extent = self.shape[source_axis];
                let target_extent = target_shape[target_axis];
                let stretch = if source_extent == target_extent {
                    0
                } else if source_extent == 1 {
                    1
                } else {
                    continue;
                };
                let suffix = costs[source_axis + 1][target_axis + 1];
                if suffix != impossible && suffix + stretch == remaining_stretches {
                    selected = Some((target_axis, stretch));
                    break;
                }
            }
            let (target_axis, stretch) = selected?;
            target_axis_for_source[source_axis] = target_axis;
            target_strides[target_axis] = if stretch == 0 {
                self.strides[source_axis]
            } else {
                0
            };
            remaining_stretches -= stretch;
            first_target_axis = target_axis + 1;
        }
        let mut target_to_source = vec![None; target_rank];
        for (source_axis, target_axis) in target_axis_for_source.iter().copied().enumerate() {
            target_to_source[target_axis] = Some(source_axis);
        }
        let mut view = self.logical_view(target_shape, target_strides, self.offset)?;
        view.push_edge_view(
            self.shape.clone(),
            TensorInverseMap::Expand { target_to_source },
        );
        Some(view)
    }

    /// Dense read gate: true when logical order is exactly the backing order
    /// — no lazy edge ops, zero origin, and strides equal to the row-major
    /// strides for the shape, so a logical ordinal is its physical offset.
    /// `edge_ops.is_empty()` alone does NOT imply this: sectio and transpose
    /// views permute strides and expanded views carry zero strides, so the
    /// strides must be compared against the shape's row-major strides.
    #[must_use]
    pub(crate) fn is_dense_row_major(&self) -> bool {
        if !self.edge_ops.is_empty() || self.offset != 0 {
            return false;
        }
        Self::try_contiguous_strides(&self.shape).is_some_and(|strides| strides == self.strides)
    }

    /// Independent copy of a fully dense typed backing; `None` for `Values`
    /// storage or any view layout, which copy element by element.
    fn typed_deep_copy(&self) -> Option<Self> {
        if self.data.kind().is_none()
            || !self.is_dense_row_major()
            || self.logical_len()? != self.data.len()
            || !self.residency.ensure_host(0..self.data.len())
        {
            return None;
        }
        Self::try_contiguous_typed(self.data.clone_storage(), self.shape.clone())
    }

    /// [`Self::value_at`] for an index the checker proved inside the tensor
    ///: a dense row-major tensor reads its slot without the
    /// logical-extent comparison. `None` only when the host copy is
    /// unavailable (or a wrong proof named a slot outside the storage).
    #[must_use]
    pub fn value_at_proven(&self, logical_index: usize) -> Option<Value> {
        if self.is_dense_row_major() {
            return self.read_physical(logical_index);
        }
        self.value_at(logical_index)
    }

    #[must_use]
    pub fn value_at(&self, logical_index: usize) -> Option<Value> {
        if self.is_dense_row_major() {
            let count = self.logical_len()?;
            if logical_index >= count {
                return None;
            }
            return self.read_physical(logical_index);
        }
        let coordinates = self.logical_coordinates(logical_index)?;
        self.value_at_coordinates(&coordinates)
    }

    #[must_use]
    pub fn value_at_coordinates(&self, coordinates: &[usize]) -> Option<Value> {
        if coordinates.len() != self.shape.len()
            || coordinates
                .iter()
                .zip(&self.shape)
                .any(|(coordinate, extent)| {
                    !usize::try_from(*extent).is_ok_and(|extent| *coordinate < extent)
                })
        {
            return None;
        }
        if self.edge_ops.is_empty() {
            let physical = self.physical_offset_for_coordinates(coordinates)?;
            return self.read_physical(physical);
        }
        let base = self.edge_base.as_ref()?;
        let mut mapped = coordinates.to_vec();
        for operation in self.edge_ops.iter().rev() {
            match operation {
                TensorEdgeOp::View {
                    input_shape,
                    output_shape,
                    inverse,
                } => {
                    if !coordinate_fits(&mapped, output_shape) {
                        return None;
                    }
                    mapped = inverse_view_coordinate(&mapped, input_shape, output_shape, inverse)?;
                }
                TensorEdgeOp::Shift {
                    input_shape,
                    descriptor,
                } => {
                    if !coordinate_fits(&mapped, input_shape) {
                        return None;
                    }
                    let extents = input_shape
                        .iter()
                        .map(|dimension| usize::try_from(*dimension).ok())
                        .collect::<Option<Vec<_>>>()?;
                    let coordinate = mapped
                        .iter()
                        .map(|coordinate| i128::try_from(*coordinate).ok())
                        .collect::<Option<Vec<_>>>()?;
                    mapped = match &descriptor.resolution {
                        TensorEdgeResolution::Optional => {
                            let Some(mapped) =
                                resolve_optional(&coordinate, &descriptor.shift, &extents)
                            else {
                                return Some(Value::Nil);
                            };
                            mapped
                        }
                        TensorEdgeResolution::Builtin(policy) => {
                            resolve_builtin(*policy, &coordinate, &descriptor.shift, &extents)?
                        }
                        TensorEdgeResolution::Custom(_) => return None,
                    };
                }
            }
        }
        let physical =
            Self::physical_offset_in_layout(&mapped, &base.shape, &base.strides, base.offset)?;
        self.read_physical(physical)
    }

    /// Host read of one backing element: the residency sync point.
    pub(crate) fn read_physical(&self, physical: usize) -> Option<Value> {
        if !self
            .residency
            .ensure_host(physical..physical.saturating_add(1))
        {
            return None;
        }
        self.data.get(physical)
    }

    /// Make the host copy of `range` current (a download when the device copy
    /// is the current one). `false` when the download failed; the error is
    /// kept for [`Self::host_read_error`].
    pub(crate) fn make_host_current(&self, range: std::ops::Range<usize>) -> bool {
        self.residency.ensure_host(range)
    }

    /// Mark this tensor's storage (and every view of it) device-resident
    /// under `token`. `host_current` says the host copy is also current (an
    /// upload) rather than stale (a kernel result).
    pub fn mark_device_resident(
        &self,
        token: DeviceToken,
        hook: ResidencyHook,
        host_current: bool,
    ) {
        self.residency.mark(token, hook, host_current);
    }

    /// The device token and whether the device copy is current, if resident.
    #[must_use]
    pub fn device_residency(&self) -> Option<(DeviceToken, bool)> {
        self.residency.state()
    }

    /// The error of the last failed residency download, if any.
    #[must_use]
    pub fn take_residency_error(&self) -> Option<RunnerError> {
        self.residency.take_error()
    }

    /// The error for a host read of this tensor that yielded nothing: a
    /// device fault stored by a failed residency download surfaces as itself
    /// (the dispatcher's error and issue code); otherwise `fallback`.
    pub(crate) fn host_read_error(&self, fallback: impl FnOnce() -> RunnerError) -> RunnerError {
        self.take_residency_error().unwrap_or_else(fallback)
    }

    #[allow(clippy::must_use_candidate)] // mutates through interior cells; the result is optional to read
    pub fn set_value_at(&self, logical_index: usize, value: Value) -> Option<()> {
        if !self.edge_ops.is_empty() {
            return None;
        }
        let physical = self.physical_offset(logical_index)?;
        let len = self.data.len();
        if !self.residency.before_write(len) {
            return None;
        }
        self.data.set(physical, &value).then_some(())
    }

    #[must_use]
    pub fn logical_values(&self) -> Option<Vec<Value>> {
        if self.is_dense_row_major() {
            let count = self.logical_len()?;
            if !self.residency.ensure_host(0..count) {
                return None;
            }
            // Ordinal == physical offset here, so the storage prefix is the
            // logical read; a short backing fails exactly where the
            // per-element read would.
            return if self.data.len() >= count {
                Some(self.data.values_prefix(count))
            } else {
                None
            };
        }
        (0..self.logical_len()?)
            .map(|index| self.value_at(index))
            .collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.logical_len().is_none_or(|count| count == 0)
    }

    #[must_use]
    pub fn is_contiguous(&self) -> bool {
        let Some(count) = self.logical_len() else {
            return false;
        };
        if count == 0 {
            return true;
        }
        let mut expected = 1_isize;
        for axis in (0..self.shape.len()).rev() {
            let Ok(dimension) = isize::try_from(self.shape[axis]) else {
                return false;
            };
            if dimension > 1 && self.strides.get(axis) != Some(&expected) {
                return false;
            }
            let Some(next) = expected.checked_mul(dimension) else {
                return false;
            };
            expected = next;
        }
        true
    }

    #[must_use]
    pub fn reshaped_view(&self, shape: Vec<i64>) -> Option<Self> {
        let input_shape = self.shape.clone();
        let current_count = self.logical_len()?;
        let requested_count = shape.iter().try_fold(1_usize, |count, dimension| {
            usize::try_from(*dimension)
                .ok()
                .and_then(|dimension| count.checked_mul(dimension))
        })?;
        if current_count != requested_count {
            return None;
        }
        let strides = if shape == self.shape {
            self.strides.clone()
        } else if self.edge_base.is_some() || self.is_contiguous() {
            Self::try_contiguous_strides(&shape)?
        } else {
            return None;
        };
        let mut view = self.logical_view(shape, strides, self.offset)?;
        view.push_edge_view(input_shape, TensorInverseMap::Reshape);
        Some(view)
    }

    pub(crate) fn physical_offset_in_layout(
        coordinates: &[usize],
        shape: &[i64],
        strides: &[isize],
        offset: isize,
    ) -> Option<usize> {
        if coordinates.len() != shape.len() || shape.len() != strides.len() {
            return None;
        }
        let mut physical = offset;
        for ((coordinate, dimension), stride) in coordinates.iter().zip(shape).zip(strides) {
            if *coordinate >= usize::try_from(*dimension).ok()? {
                return None;
            }
            physical =
                physical.checked_add(isize::try_from(*coordinate).ok()?.checked_mul(*stride)?)?;
        }
        usize::try_from(physical).ok()
    }

    #[must_use]
    fn layout_fits(data_len: usize, shape: &[i64], strides: &[isize], offset: isize) -> bool {
        if shape.len() != strides.len() {
            return false;
        }
        if shape.is_empty() {
            return if data_len == 0 {
                offset == 0
            } else {
                usize::try_from(offset).is_ok_and(|offset| offset < data_len)
            };
        }
        let Some(element_count) = shape.iter().try_fold(1_usize, |count, dimension| {
            usize::try_from(*dimension)
                .ok()
                .and_then(|dimension| count.checked_mul(dimension))
        }) else {
            return false;
        };
        if element_count == 0 {
            return usize::try_from(offset).is_ok_and(|offset| offset <= data_len);
        }
        let Some((lowest, highest)) = shape.iter().zip(strides).try_fold(
            (offset, offset),
            |(lowest, highest), (dimension, stride)| {
                let extent = isize::try_from(*dimension)
                    .ok()?
                    .checked_sub(1)?
                    .checked_mul(*stride)?;
                if extent < 0 {
                    Some((lowest.checked_add(extent)?, highest))
                } else {
                    Some((lowest, highest.checked_add(extent)?))
                }
            },
        ) else {
            return false;
        };
        usize::try_from(lowest).is_ok()
            && usize::try_from(highest).is_ok_and(|highest| highest < data_len)
    }
}

pub(crate) fn coordinate_fits(coordinate: &[usize], shape: &[i64]) -> bool {
    coordinate.len() == shape.len()
        && coordinate.iter().zip(shape).all(|(coordinate, extent)| {
            usize::try_from(*extent).is_ok_and(|extent| *coordinate < extent)
        })
}

pub(crate) fn inverse_view_coordinate(
    output_coordinate: &[usize],
    input_shape: &[i64],
    output_shape: &[i64],
    inverse: &TensorInverseMap,
) -> Option<Vec<usize>> {
    if !coordinate_fits(output_coordinate, output_shape) {
        return None;
    }
    match inverse {
        TensorInverseMap::SliceAxis0 { start, step } => {
            if input_shape.len() != output_shape.len() || output_shape.is_empty() || *step == 0 {
                return None;
            }
            let mut coordinate = output_coordinate.to_vec();
            coordinate[0] = start.checked_add(coordinate[0].checked_mul(*step)?)?;
            coordinate_fits(&coordinate, input_shape).then_some(coordinate)
        }
        TensorInverseMap::TransposeLastTwo => {
            let rank = output_shape.len();
            if rank < 2
                || input_shape.len() != rank
                || input_shape[..rank - 2] != output_shape[..rank - 2]
                || input_shape[rank - 2] != output_shape[rank - 1]
                || input_shape[rank - 1] != output_shape[rank - 2]
            {
                return None;
            }
            let mut coordinate = output_coordinate.to_vec();
            coordinate.swap(rank - 2, rank - 1);
            coordinate_fits(&coordinate, input_shape).then_some(coordinate)
        }
        TensorInverseMap::Expand { target_to_source } => {
            if target_to_source.len() != output_shape.len()
                || input_shape.len() != target_to_source.iter().flatten().count()
            {
                return None;
            }
            let mut coordinate = vec![0; input_shape.len()];
            for (target_axis, source_axis) in target_to_source.iter().enumerate() {
                if let Some(source_axis) = source_axis {
                    coordinate[*source_axis] = output_coordinate[target_axis];
                }
            }
            coordinate_fits(&coordinate, input_shape).then_some(coordinate)
        }
        TensorInverseMap::Reshape => {
            let flat = flatten_coordinate(output_coordinate, output_shape)?;
            unflatten_coordinate(flat, input_shape)
        }
    }
}

fn flatten_coordinate(coordinate: &[usize], shape: &[i64]) -> Option<usize> {
    if !coordinate_fits(coordinate, shape) {
        return None;
    }
    coordinate
        .iter()
        .zip(shape)
        .try_fold(0_usize, |flat, (coordinate, extent)| {
            flat.checked_mul(usize::try_from(*extent).ok()?)?
                .checked_add(*coordinate)
        })
}

fn unflatten_coordinate(flat: usize, shape: &[i64]) -> Option<Vec<usize>> {
    let count = shape.iter().try_fold(1_usize, |count, extent| {
        count.checked_mul(usize::try_from(*extent).ok()?)
    })?;
    if flat >= count {
        return None;
    }
    decompose_row_major(flat, shape)
}

/// Row-major coordinates of a flat ordinal: the last axis varies fastest.
/// Fails when an extent does not fit `usize` or is zero; every caller checks
/// the ordinal against the shape's element count first, so the zero-extent
/// arm only guards direct misuse.
fn decompose_row_major(flat: usize, shape: &[i64]) -> Option<Vec<usize>> {
    let mut remainder = flat;
    let mut coordinates = vec![0; shape.len()];
    for axis in (0..shape.len()).rev() {
        let extent = usize::try_from(shape[axis]).ok()?;
        if extent == 0 {
            return None;
        }
        coordinates[axis] = remainder % extent;
        remainder /= extent;
    }
    Some(coordinates)
}

/// Opaque gradient-handle storage for the in-process runner.
///
/// Clean-break internal contract (mirror of the host LLVM gradient ABI
/// `__faber_rt_v1_gradient_*`): one handle per trainable parameter, flat
/// element-ordered data, explicit shape, element-wise accumulate, zero fill.
/// Values are `Value` carriers instead of raw `f32` buffers so the runner
/// never touches the device ABI ownership surface.
#[derive(Debug, Clone, PartialEq)]
pub struct GradientStorage {
    pub data: Vec<Value>,
    pub shape: Vec<i64>,
}

/// Small register matrix carried by the CPU runner (Stage matrix subset).
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixValue {
    pub rows: u8,
    pub columns: u8,
    /// Row-major length `rows * columns`.
    pub data: Vec<Value>,
}

impl MatrixValue {
    #[must_use]
    pub fn cell(&self, row: u8, column: u8) -> Option<&Value> {
        let index = usize::from(row) * usize::from(self.columns) + usize::from(column);
        self.data.get(index)
    }

    #[must_use]
    pub fn cell_mut(&mut self, row: u8, column: u8) -> Option<&mut Value> {
        let index = usize::from(row) * usize::from(self.columns) + usize::from(column);
        self.data.get_mut(index)
    }
}

pub use crate::sermo::SermoValue;

/// Hashable key subset for `tabula` / `copia`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapKey {
    /// Negative integers only; non-negative integers canonicalize to `UInt`.
    Int(i64),
    /// Non-negative integers of either carrier, plus all unsigned magnitudes:
    /// same-value keys collide, upper-half magnitudes never sign-encode.
    UInt(u64),
    /// An `inf` key beyond the 64-bit carriers (canonical: never a value
    /// `Int`/`UInt` can hold, see [`Value::Magnus`]).
    Magnus(Magnus),
    Bool(bool),
    Str(String),
    /// The `none` element of a `set<T ∪ none>` or key of a
    /// `map<K ∪ none, V>` (reads back as [`Value::Nil`]); a `unit` value keys
    /// to the same slot, since no collection type holds both.
    Unit,
    /// Composite `iuncta` key (D3.2): hashable when every part is.
    Tuple(Vec<MapKey>),
    /// A `discretio` variant key (GAPS #32): the variant's `DefId` plus its
    /// payload fields as `(name, key)` pairs, sorted by symbol so hashing is
    /// order-stable.
    Enum(DefId, Vec<(Symbol, MapKey)>),
}

impl MapKey {
    /// The runner value a key reads back as (map keys, set elements).
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            MapKey::Int(v) => Value::Int(*v),
            MapKey::UInt(v) => Value::UInt(*v),
            MapKey::Magnus(v) => Value::Magnus(Rc::new(v.clone())),
            MapKey::Bool(v) => Value::Bool(*v),
            MapKey::Str(v) => Value::Str(v.clone()),
            MapKey::Unit => Value::Nil,
            MapKey::Tuple(parts) => Value::Tuple(parts.iter().map(MapKey::to_value).collect()),
            MapKey::Enum(def, parts) => Value::EnumVariant(
                *def,
                Rc::new(RefCell::new(
                    parts
                        .iter()
                        .map(|(name, key)| (*name, key.to_value()))
                        .collect(),
                )),
            ),
        }
    }

    /// Plain-text key spelling for document encoders (JSON/TOML object
    /// keys); a composite key spells its parts comma-separated.
    #[must_use]
    pub fn document_key(&self) -> String {
        match self {
            MapKey::Str(value) => value.clone(),
            MapKey::Int(value) => value.to_string(),
            MapKey::UInt(value) => value.to_string(),
            MapKey::Magnus(value) => value.to_string(),
            MapKey::Bool(value) => value.to_string(),
            MapKey::Unit => String::new(),
            MapKey::Tuple(parts) => parts
                .iter()
                .map(MapKey::document_key)
                .collect::<Vec<_>>()
                .join(","),
            MapKey::Enum(def, parts) => {
                let joined = parts
                    .iter()
                    .map(|(name, key)| format!("{}.{}", name.0, key.document_key()))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{}#{joined}", def.0)
            }
        }
    }
}

/// Insertion-ordered ledger (`tabulae<K, V>`): the native map from key to
/// slot number plus an array of entries in insertion order. Removal leaves a
/// hole in the array; holes are dropped, and slots renumbered, when the table
/// rebuilds. The slot number is never visible to a program, and order is part
/// of a ledger's equality.
#[derive(Debug, Clone, Default)]
pub struct LedgerValue {
    index: FxHashMap<MapKey, usize>,
    entries: Vec<Option<(MapKey, Value)>>,
    live: usize,
}

impl LedgerValue {
    #[must_use]
    pub fn new() -> Self {
        Self {
            index: FxHashMap::default(),
            entries: Vec::new(),
            live: 0,
        }
    }

    /// Insert or replace: an existing key keeps its slot, a new key is pushed
    /// at the end.
    pub fn insert(&mut self, key: MapKey, value: Value) {
        if let Some(&slot) = self.index.get(&key) {
            self.entries[slot] = Some((key, value));
            return;
        }
        self.index.insert(key.clone(), self.entries.len());
        self.entries.push(Some((key, value)));
        self.live += 1;
    }

    #[must_use]
    pub fn get(&self, key: &MapKey) -> Option<&Value> {
        let slot = *self.index.get(key)?;
        self.entries.get(slot)?.as_ref().map(|(_, value)| value)
    }

    #[must_use]
    pub fn contains_key(&self, key: &MapKey) -> bool {
        self.index.contains_key(key)
    }

    /// Remove a key: its slot becomes a hole and the key is dropped from the
    /// index. Holes over half the entry array trigger a compaction.
    pub fn remove(&mut self, key: &MapKey) -> Option<Value> {
        let slot = self.index.remove(key)?;
        let (_, value) = self.entries.get_mut(slot)?.take()?;
        self.live -= 1;
        let holes = self.entries.len() - self.live;
        if holes > self.entries.len() / 2 {
            self.compact();
        }
        Some(value)
    }

    /// Drop holes and renumber every remaining slot.
    fn compact(&mut self) {
        let entries = std::mem::take(&mut self.entries);
        self.entries = entries.into_iter().flatten().map(Some).collect();
        let mut index = FxHashMap::default();
        for (slot, entry) in self.entries.iter().enumerate() {
            if let Some((key, _)) = entry {
                index.insert(key.clone(), slot);
            }
        }
        self.index = index;
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.live
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Live entries in slot (insertion) order.
    pub fn iter(&self) -> impl Iterator<Item = (&MapKey, &Value)> {
        self.entries
            .iter()
            .filter_map(Option::as_ref)
            .map(|(key, value)| (key, value))
    }

    /// Live keys in slot (insertion) order.
    pub fn keys(&self) -> impl Iterator<Item = &MapKey> {
        self.entries
            .iter()
            .filter_map(Option::as_ref)
            .map(|(key, _)| key)
    }

    /// Live values in slot (insertion) order.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries
            .iter()
            .filter_map(Option::as_ref)
            .map(|(_, value)| value)
    }
}

impl PartialEq for LedgerValue {
    fn eq(&self, other: &Self) -> bool {
        self.live == other.live && self.iter().eq(other.iter())
    }
}

impl Value {
    /// A store of a register-class value owns its storage (operator ruling
    /// 7b25889f R1): vectors and matrices are silent values, so `b ← a`
    /// copies and a later lane write through one binding never reaches
    /// another. Tensors and every heap container keep their reference store.
    /// A vector's lanes are numeric scalars, so the deep duplicate is the
    /// whole copy.
    #[must_use]
    pub fn copied_register_class(self) -> Value {
        match self {
            Value::Vector(_) | Value::Matrix(_) => self.deep_copy(),
            other => other,
        }
    }

    /// Deep duplicate for the `copy` access marker (D-6): every container,
    /// genus, and record cell reachable from `self` is rebuilt, so no storage
    /// is shared with the source. Sharing *within* the copied graph (two
    /// fields naming one list, or a cycle) is preserved through a memo keyed
    /// on the source cell. Opaque handles (functions, closures, sermo
    /// conversations, gradients) are identities, not payloads, and stay shared.
    #[must_use]
    pub fn deep_copy(&self) -> Value {
        self.deep_copy_memo(&mut FxHashMap::default())
    }

    fn deep_copy_memo(&self, memo: &mut FxHashMap<*const (), Value>) -> Value {
        fn cell<T>(
            rc: &Rc<RefCell<T>>,
            memo: &mut FxHashMap<*const (), Value>,
            wrap: impl Fn(Rc<RefCell<T>>) -> Value,
            copy: impl FnOnce(&T, &mut FxHashMap<*const (), Value>) -> T,
            empty: impl FnOnce() -> T,
        ) -> Value {
            let key = Rc::as_ptr(rc).cast::<()>();
            if let Some(done) = memo.get(&key) {
                return done.clone();
            }
            // Register the fresh cell before descending so a cycle back to
            // this source cell resolves to the copy under construction.
            let fresh = Rc::new(RefCell::new(empty()));
            memo.insert(key, wrap(Rc::clone(&fresh)));
            let copied = copy(&rc.borrow(), memo);
            *fresh.borrow_mut() = copied;
            wrap(fresh)
        }
        fn fields(
            source: &FxHashMap<Symbol, Value>,
            memo: &mut FxHashMap<*const (), Value>,
        ) -> FxHashMap<Symbol, Value> {
            source
                .iter()
                .map(|(name, value)| (*name, value.deep_copy_memo(memo)))
                .collect()
        }
        fn elements(source: &[Value], memo: &mut FxHashMap<*const (), Value>) -> Vec<Value> {
            source
                .iter()
                .map(|value| value.deep_copy_memo(memo))
                .collect()
        }
        match self {
            Value::Array(rc) => cell(rc, memo, Value::Array, |v, m| elements(v, m), Vec::new),
            Value::Vector(rc) => cell(rc, memo, Value::Vector, |v, m| elements(v, m), Vec::new),
            Value::Tensor(rc) => cell(
                rc,
                memo,
                Value::Tensor,
                |t, m| {
                    // Typed dense storage has scalar elements, so the typed
                    // clone is the deep copy.
                    if let Some(copied) = t.typed_deep_copy() {
                        return copied;
                    }
                    // A source whose contiguous strides do not fit holds no
                    // elements, so sharing its (empty) layout copies nothing.
                    TensorValue::try_contiguous(
                        elements(&t.logical_values().unwrap_or_default(), m),
                        t.shape.clone(),
                    )
                    .unwrap_or_else(|| t.clone())
                },
                TensorValue::empty,
            ),
            Value::Matrix(rc) => {
                let source = rc.borrow();
                let mut copied = source.clone();
                copied.data = elements(&source.data, memo);
                Value::Matrix(Rc::new(RefCell::new(copied)))
            }
            Value::Map(rc) => cell(
                rc,
                memo,
                Value::Map,
                |map, m| {
                    map.iter()
                        .map(|(key, value)| (key.clone(), value.deep_copy_memo(m)))
                        .collect()
                },
                FxHashMap::default,
            ),
            Value::Ledger(rc) => cell(
                rc,
                memo,
                Value::Ledger,
                |ledger, m| {
                    let mut copied = LedgerValue::new();
                    for (key, value) in ledger.iter() {
                        copied.insert(key.clone(), value.deep_copy_memo(m));
                    }
                    copied
                },
                LedgerValue::new,
            ),
            Value::Set(rc) => Value::Set(Rc::new(RefCell::new(rc.borrow().clone()))),
            Value::Record(rc, written) => {
                let written = Rc::clone(written);
                cell(
                    rc,
                    memo,
                    move |r| Value::Record(r, Rc::clone(&written)),
                    fields,
                    FxHashMap::default,
                )
            }
            Value::Struct(def, rc) => {
                let def = *def;
                cell(
                    rc,
                    memo,
                    move |r| Value::Struct(def, r),
                    fields,
                    FxHashMap::default,
                )
            }
            Value::EnumVariant(def, rc) => {
                let def = *def;
                cell(
                    rc,
                    memo,
                    move |r| Value::EnumVariant(def, r),
                    fields,
                    FxHashMap::default,
                )
            }
            Value::Tuple(items) => Value::Tuple(elements(items, memo)),
            other => other.clone(),
        }
    }

    pub fn as_bool(&self, span: Span) -> Result<bool, RunnerError> {
        match self {
            Value::Bool(value) => Ok(*value),
            _ => Err(RunnerError::internal(span, "expected bivalens operand")),
        }
    }

    pub fn as_index(&self, span: Span) -> Result<usize, RunnerError> {
        match self {
            Value::Int(value) => usize::try_from(*value)
                .map_err(|_| RunnerError::internal(span, trap_text::NEGATIVE_INDEX)),
            Value::UInt(value) => usize::try_from(*value)
                .map_err(|_| RunnerError::internal(span, trap_text::NEGATIVE_INDEX)),
            _ => Err(RunnerError::internal(span, trap_text::NEGATIVE_INDEX)),
        }
    }

    pub fn display(
        &self,
        interner: &Interner,
        order: FieldOrder<'_>,
        span: Span,
    ) -> Result<String, RunnerError> {
        self.display_typed(interner, order, span, None)
    }

    /// Width-aware display: f32-declared floats render at f32 precision
    /// (9007e12c3 carried the declared width into arithmetic; the display
    /// path follows), everything else renders as before.
    pub fn display_typed(
        &self,
        interner: &Interner,
        order: FieldOrder<'_>,
        span: Span,
        float_width: Option<NumericWidth>,
    ) -> Result<String, RunnerError> {
        Ok(match self {
            Value::Int(v) => display_int_at_width(*v, float_width),
            Value::UInt(v) => order
                .bits
                .map_or_else(|| v.to_string(), |width| display_bits(*v, width)),
            Value::Magnus(v) => v.to_string(),
            Value::Dec(carrier, scale) => crate::decimal::format(*carrier, *scale),
            Value::Float(v) => display_float_at_width(*v, float_width),
            Value::Bool(v) => display_bivalens(*v, order.tokens).to_owned(),
            Value::Str(v) => display_text_payload(v).to_owned(),
            Value::Ascii(b) => display_char(char::from(*b)),
            Value::Char(ch) => display_char(*ch),
            Value::Nil => order.tokens.none.clone(),
            Value::Unit => String::new(),
            Value::Octeti(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            Value::Instans(value) => value.to_rfc3339(),
            Value::Regex(program) => program.pattern().to_owned(),
            Value::Match(found) => found.text.clone(),
            Value::Sermo(sermo) => {
                format!("sermo({})", sermo.borrow().conversation.borrow().route)
            }
            Value::Function(id) => format!("function f{}", id.0),
            Value::Closure(id, environment, _) => {
                format!("closure f{} env{}", id.0, environment.0)
            }
            Value::ClosureEnvironment(id, _) => format!("closure env{}", id.0),
            Value::SharedCell(cell) => {
                cell.borrow()
                    .display_typed(interner, order, span, float_width)?
            }
            Value::Array(items) => {
                display_list(&items.borrow(), interner, order, span, float_width)?
            }
            Value::Tensor(tensor) => {
                let tensor = tensor.borrow();
                let values = tensor.logical_values().ok_or_else(|| {
                    tensor.host_read_error(|| {
                        RunnerError::internal(span, "tensor view has invalid storage layout")
                    })
                })?;
                display_list(&values, interner, order, span, float_width)?
            }
            Value::Vector(lanes) => {
                display_list(&lanes.borrow(), interner, order, span, float_width)?
            }
            Value::Matrix(matrix) => {
                display_matrix(&matrix.borrow(), interner, order, span, float_width)?
            }
            Value::Map(entries) => {
                display_tabula_map(&entries.borrow(), interner, order, span, float_width)?
            }
            Value::Ledger(entries) => {
                display_ledger(&entries.borrow(), interner, order, span, float_width)?
            }
            Value::Struct(def, fields) => {
                display_tabula_struct(*def, fields, interner, order, span, float_width)?
            }
            Value::Record(fields, written) => {
                if order.dynamic {
                    display_json_object(&fields.borrow(), written, None, interner, order, span)?
                } else {
                    display_anonymous_construction(
                        &fields.borrow(),
                        written,
                        interner,
                        order,
                        span,
                    )?
                }
            }
            Value::EnumVariant(def, fields) => {
                let declared = order.variants.get(def).map_or(&[][..], Vec::as_slice);
                // A variant without a construction-literal name (defensive:
                // lowering registers one for every `discretio` and `ordo`
                // member) keeps the JSON-shaped display.
                if order.dynamic || !order.names.contains_key(def) {
                    display_json_object(&fields.borrow(), declared, None, interner, order, span)?
                } else {
                    display_construction(
                        *def,
                        &fields.borrow(),
                        declared,
                        Some((*def, None)),
                        true,
                        interner,
                        order,
                        span,
                    )?
                }
            }
            Value::Tuple(items) => {
                if order.interval
                    && let Some(rendered) = display_interval(items, interner, order, span)?
                {
                    rendered
                } else if let Some(parts) = order.tuple {
                    display_tuple(items, parts, interner, order, span)?
                } else {
                    // No interval or tuple shape: a value whose type the
                    // caller could not resolve.
                    format!("{self:?}")
                }
            }
            Value::Set(cell) => display_set(&cell.borrow(), interner, order, span, float_width)?,
            Value::GradientHandle(id) => format!("gradient-handle#{id}"),
            Value::Cursor(_) => "cursor".to_owned(),
        })
    }

    pub fn to_key(&self, span: Span) -> Result<MapKey, RunnerError> {
        match self {
            // Canonical numeric key: every non-negative integer maps to `UInt`
            // so the same value finds the same entry regardless of carrier.
            // Negative values keep the signed key; upper-half unsigned
            // magnitudes can never collide with signed keys.
            Value::Int(v) if *v >= 0 => Ok(MapKey::UInt(*v as u64)),
            Value::Int(v) => Ok(MapKey::Int(*v)),
            Value::UInt(v) => Ok(MapKey::UInt(*v)),
            // Canonical `Magnus` is always beyond the 64-bit keys above.
            Value::Magnus(v) => Ok(MapKey::Magnus((**v).clone())),
            Value::Bool(v) => Ok(MapKey::Bool(*v)),
            Value::Str(v) => Ok(MapKey::Str(v.clone())),
            Value::Ascii(b) => Ok(MapKey::Str(char::from(*b).to_string())),
            Value::Char(ch) => Ok(MapKey::Str(ch.to_string())),
            Value::Nil | Value::Unit => Ok(MapKey::Unit),
            Value::Tuple(items) => items
                .iter()
                .map(|item| item.to_key(span))
                .collect::<Result<Vec<_>, _>>()
                .map(MapKey::Tuple),
            Value::EnumVariant(def, fields) => {
                let mut parts: Vec<(Symbol, MapKey)> = fields
                    .borrow()
                    .iter()
                    .map(|(name, value)| Ok((*name, value.to_key(span)?)))
                    .collect::<Result<Vec<_>, _>>()?;
                parts.sort_by_key(|(name, _)| name.0);
                Ok(MapKey::Enum(*def, parts))
            }
            _ => Err(RunnerError::internal(
                span,
                "value is not a valid map/set key",
            )),
        }
    }

    #[must_use]
    pub fn is_empty_collection(&self) -> bool {
        match self {
            Value::Array(cell) => cell.borrow().is_empty(),
            Value::Tensor(cell) => cell.borrow().is_empty(),
            Value::Map(cell) => cell.borrow().is_empty(),
            Value::Ledger(cell) => cell.borrow().is_empty(),
            Value::Set(cell) => cell.borrow().is_empty(),
            Value::Str(value) => value.is_empty(),
            Value::Ascii(_) => false,
            Value::Octeti(bytes) => bytes.is_empty(),
            _ => false,
        }
    }
}

/// Render an interval carrier as its construction literal: `[start, end,
/// Bool(inclusive)]` prints `2‥9` (exclusive) or `2…9` (inclusive), and a
/// trailing step adds ` per 2`. Each part renders through the scalar display
/// arms, so it reads exactly as the same value printed bare. The declared
/// interval type supplies the shape; a carrier that does not match returns
/// `None`, and the caller falls back to the existing display.
fn display_interval(
    items: &[Value],
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
) -> Result<Option<String>, RunnerError> {
    let (start, end, inclusive, step) = match items {
        [start, end, Value::Bool(inclusive)] => (start, end, *inclusive, None),
        [start, end, Value::Bool(inclusive), step] => (start, end, *inclusive, Some(step)),
        _ => return Ok(None),
    };
    let glyph = if inclusive { '…' } else { '‥' };
    let order = order.with_interval(false);
    let start = start.display_typed(interner, order, span, None)?;
    let end = end.display_typed(interner, order, span, None)?;
    let mut text = format!("{start}{glyph}{end}");
    if let Some(step) = step {
        let step = step.display_typed(interner, order, span, None)?;
        text.push_str(" per ");
        text.push_str(&step);
    }
    Ok(Some(text))
}

/// Render a tuple as `iuncta [a, b]`: each element is a composite member shown
/// at its own declared width and shape.
fn display_tuple<'a>(
    items: &[Value],
    parts: &'a TupleParts,
    interner: &Interner,
    order: FieldOrder<'a>,
    span: Span,
) -> Result<String, RunnerError> {
    let rendered = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let part = parts.0.get(index);
            display_element(
                item,
                interner,
                order
                    .with_tuple(part.and_then(|part| part.tuple.as_ref()))
                    .with_bits(part.and_then(|part| part.bits)),
                span,
                part.and_then(|part| part.width),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("{} [{}]", order.tokens.tuple, rendered.join(", ")))
}

fn display_list(
    items: &[Value],
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let parts = items
        .iter()
        .map(|item| display_element(item, interner, order, span, float_width))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("[{}]", parts.join(", ")))
}

/// Render a register matrix as nested rows `[[...],[...]]`, matching the
/// compiled host route (row-major cells split on the column count).
fn display_matrix(
    matrix: &MatrixValue,
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let rows = usize::from(matrix.rows);
    let columns = usize::from(matrix.columns);
    let row_parts = (0..rows)
        .map(|row| {
            let start = row * columns;
            let end = start + columns;
            display_list(
                matrix.data.get(start..end).unwrap_or(&[]),
                interner,
                order,
                span,
                float_width,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("[{}]", row_parts.join(", ")))
}

/// A printed set is list-shaped (`[3, 7]`, `["x"]`): every element is rendered
/// exactly as a list element is, so the display tokens of the printing
/// function apply the same way. A set has no defined order (D19.2/D19.6), so
/// nothing sorts: the snapshot is shuffled under the test-only seed like a
/// printed `tabula`, and no golden can quietly pin the runner's hash order.
fn display_set(
    items: &FxHashSet<MapKey>,
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let mut keys: Vec<&MapKey> = items.iter().collect();
    crate::shuffle::shuffle_unordered(&mut keys);
    display_set_keys(&keys, interner, order, span, float_width)
}

/// Render the set elements `keys` in the given order, list-shaped.
pub(crate) fn display_set_keys(
    keys: &[&MapKey],
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let values: Vec<Value> = keys
        .iter()
        .map(|key| match key {
            // A list prints its none element through the none token; the
            // key's plain unit value would print as nothing.
            MapKey::Unit => Value::Nil,
            other => other.to_value(),
        })
        .collect();
    display_list(&values, interner, order, span, float_width)
}

/// A composite key shown in a printed map: its parts comma-separated, a bool
/// part through the printing function's tokens (not the document spelling).
fn display_tuple_key(parts: &[MapKey], tokens: &DisplayTokens) -> String {
    parts
        .iter()
        .map(|part| match part {
            MapKey::Bool(v) => display_bivalens(*v, tokens).to_owned(),
            MapKey::Tuple(inner) => display_tuple_key(inner, tokens),
            other => other.document_key(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn display_tabula_map(
    entries: &FxHashMap<MapKey, Value>,
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    // D19.2/D19.6: tabula order is unspecified; shuffle the snapshot under
    // the test-only seed so a `print tabula` golden cannot quietly pin the
    // runner's FxHashMap order.
    let mut rows: Vec<(&MapKey, &Value)> = entries.iter().collect();
    if order.dynamic {
        // A `valor`/`json` object is not an ordinary `tabula`: its display
        // order is fixed as sorted keys (byte order, the order a
        // `BTreeMap<String, _>` gives; stepper-valor-representation.md), so
        // no shuffle applies.
        rows.sort_by_key(|row| display_sort_key(row.0));
    } else {
        crate::shuffle::shuffle_unordered(&mut rows);
    }
    render_map_rows(&rows, interner, order, span, float_width)
}

/// A ledger (`tabulae<K, V>`) renders in slot (insertion) order: order is
/// part of its value, so no sort or shuffle applies.
fn display_ledger(
    entries: &LedgerValue,
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let rows: Vec<(&MapKey, &Value)> = entries.iter().collect();
    render_map_rows(&rows, interner, order, span, float_width)
}

/// Render the rows of a map-shaped value in the order given: each row as
/// `key: value`, the whole as `{row, row}`.
fn render_map_rows(
    rows: &[(&MapKey, &Value)],
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let mut parts = Vec::new();
    for (key, value) in rows {
        let key_str = match key {
            MapKey::Str(s) => format!("\"{s}\""),
            MapKey::Int(v) => v.to_string(),
            MapKey::UInt(v) => v.to_string(),
            MapKey::Magnus(v) => v.to_string(),
            MapKey::Bool(v) => display_bivalens(*v, order.tokens).to_owned(),
            MapKey::Unit => order.tokens.none.clone(),
            MapKey::Tuple(parts) => display_tuple_key(parts, order.tokens),
            MapKey::Enum(..) => key.document_key(),
        };
        let val_str = value.display_typed(interner, order, span, float_width)?;
        parts.push(format!("{key_str}: {val_str}"));
    }
    Ok(format!("{{{}}}", parts.join(", ")))
}

/// The key text a `valor`/`json` object display sorts by: a string key is its
/// own bytes; any other key (not reachable from a json literal) its document
/// form, so the order stays total and deterministic.
fn display_sort_key(key: &MapKey) -> String {
    match key {
        MapKey::Str(text) => text.clone(),
        other => other.document_key(),
    }
}

fn display_tabula_struct(
    def: DefId,
    fields: &RefCell<FxHashMap<Symbol, Value>>,
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    let declared = order.structs.get(&def).map_or(&[][..], Vec::as_slice);
    if order.dynamic {
        return display_json_object(
            &fields.borrow(),
            declared,
            Some((def, float_width)),
            interner,
            order,
            span,
        );
    }
    display_construction(
        def,
        &fields.borrow(),
        declared,
        Some((def, float_width)),
        false,
        interner,
        order,
        span,
    )
}

/// Named fields in `declared` order; any field the metadata does not list
/// follows in name order so output stays stable.
fn ordered_field_rows<'a>(
    fields: &'a FxHashMap<Symbol, Value>,
    declared: &[Symbol],
    interner: &Interner,
) -> Vec<(&'a Symbol, &'a Value)> {
    let mut rows: Vec<(&Symbol, &Value)> = declared
        .iter()
        .filter_map(|symbol| fields.get_key_value(symbol))
        .collect();
    let mut rest: Vec<(&Symbol, &Value)> = fields
        .iter()
        .filter(|(symbol, _)| !declared.contains(symbol))
        .collect();
    rest.sort_by(|a, b| interner.resolve(*a.0).cmp(interner.resolve(*b.0)));
    rows.extend(rest);
    rows
}

/// Render named fields as `{"name": value, ...}` in `declared` order: an
/// anonymous record (which has no construction-literal name, so D2.8 does not
/// apply) or any value shown through a dynamic `valor`/`json` carrier. `owner`
/// is the struct whose per-field declared float width applies.
fn display_json_object(
    fields: &FxHashMap<Symbol, Value>,
    declared: &[Symbol],
    owner: Option<(DefId, Option<NumericWidth>)>,
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
) -> Result<String, RunnerError> {
    let mut parts = Vec::new();
    for (symbol, value) in ordered_field_rows(fields, declared, interner) {
        let name = interner.resolve(*symbol);
        let width = owner.and_then(|(def, inherited)| {
            order
                .float_widths
                .get(&(def, *symbol))
                .copied()
                .or(inherited)
        });
        let val_str = value.display_typed(interner, order, span, width)?;
        parts.push(format!("\"{name}\": {val_str}"));
    }
    Ok(format!("{{{}}}", parts.join(", ")))
}

/// Render a `genus` value or `discretio` variant as its construction literal
/// (D2.8): `Name { field = value, ... }` in declaration order, text, char and
/// ascii values quoted. With no fields a genus prints `Name {}` and a variant
/// (`variant`) prints its bare qualified name. `owner` is the struct whose
/// per-field declared float width applies.
#[expect(clippy::too_many_arguments)]
fn display_construction(
    def: DefId,
    fields: &FxHashMap<Symbol, Value>,
    declared: &[Symbol],
    owner: Option<(DefId, Option<NumericWidth>)>,
    variant: bool,
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
) -> Result<String, RunnerError> {
    let name = order
        .names
        .get(&def)
        .cloned()
        .unwrap_or_else(|| format!("def{}", def.0));
    let mut parts = Vec::new();
    for (symbol, value) in ordered_field_rows(fields, declared, interner) {
        let field = interner.resolve(*symbol);
        let width = owner.and_then(|(def, inherited)| {
            order
                .float_widths
                .get(&(def, *symbol))
                .copied()
                .or(inherited)
        });
        // The field's declared type decides char vs text quoting; nothing is
        // inherited from the enclosing value.
        let field_order = order
            .with_char_text(order.char_fields.contains(&(def, *symbol)))
            .with_tuple(order.tuple_fields.get(&(def, *symbol)))
            .with_bits(order.bits_fields.get(&(def, *symbol)).copied());
        let val_str = display_element(value, interner, field_order, span, width)?;
        parts.push(format!("{field} = {val_str}"));
    }
    if parts.is_empty() {
        return Ok(if variant {
            name
        } else {
            format!("{name} {{}}")
        });
    }
    Ok(format!("{name} {{ {} }}", parts.join(", ")))
}

/// Render a `record` value as an anonymous construction literal: `{ field =
/// value, ... }` in written order, each field through [`display_element`] (the
/// value's default type, since a record has no declared field metadata). A
/// record with no fields prints `{}`. A value shown through a `valor`/`json`
/// carrier keeps the JSON-shaped display (see [`display_json_object`]).
fn display_anonymous_construction(
    fields: &FxHashMap<Symbol, Value>,
    written: &[Symbol],
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
) -> Result<String, RunnerError> {
    let mut parts = Vec::new();
    for (symbol, value) in ordered_field_rows(fields, written, interner) {
        let field = interner.resolve(*symbol);
        let val_str = display_element(value, interner, order, span, None)?;
        parts.push(format!("{field} = {val_str}"));
    }
    if parts.is_empty() {
        return Ok("{}".to_owned());
    }
    Ok(format!("{{ {} }}", parts.join(", ")))
}

/// A value inside a composite (list element or construction-literal field):
/// text and every `ascii` print double-quoted and a `littera` single-quoted,
/// as in source (the static type decides, see [`FieldCharSet`]); everything
/// else renders as it does at the top level. A value shown through a dynamic
/// `valor`/`json` carrier keeps the unquoted JSON-shaped form.
fn display_element(
    value: &Value,
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
    float_width: Option<NumericWidth>,
) -> Result<String, RunnerError> {
    if order.dynamic {
        return value.display_typed(interner, order, span, float_width);
    }
    match value {
        Value::Str(text) => Ok(format!("{text:?}")),
        // An ascii-carried value is a `littera` only when its static type says
        // so (a `'q'` literal reaches the runner as `Ascii` whichever slot it
        // fills); otherwise it is text, however short (D10.5).
        Value::Ascii(byte) if order.char_text => Ok(format!("{:?}", char::from(*byte))),
        Value::Ascii(byte) => Ok(format!("{:?}", char::from(*byte).to_string())),
        Value::Char(ch) => Ok(format!("{ch:?}")),
        Value::SharedCell(cell) => {
            display_element(&cell.borrow(), interner, order, span, float_width)
        }
        other => other.display_typed(interner, order, span, float_width),
    }
}

/// One Unicode scalar as its text payload (the top-level char display).
fn display_char(ch: char) -> String {
    let mut buf = [0u8; 4];
    display_text_payload(ch.encode_utf8(&mut buf)).to_owned()
}

/// Render an integer carrier at its declared width: a `d64` carrier prints as
/// its shortest decimal (D2.6: `4.2`, `12`, never the raw `420000000`);
/// every other integer prints as itself.
pub fn display_int_at_width(value: i64, width: Option<NumericWidth>) -> String {
    match width.and_then(NumericWidth::decimal_scale) {
        Some(scale) => crate::decimal::format(i128::from(value), scale),
        None => value.to_string(),
    }
}

/// Render a float at its declared width: non-f64 floats use the shortest
/// round-trip repr of that width (`66.666664`), keeping the fractus `N.0`
/// integral shape; f64 (or unknown width) uses the standard fractus display.
#[must_use]
pub fn display_float_at_width(value: f64, width: Option<NumericWidth>) -> String {
    match width {
        Some(width) if width.is_float() && width != NumericWidth::F64 => {
            #[expect(clippy::cast_possible_truncation)]
            let narrowed = format!("{}", value as f32);
            let integral = narrowed
                .chars()
                .all(|c| c.is_ascii_digit() || c == '-' || c == '+');
            if integral {
                format!("{narrowed}.0")
            } else {
                narrowed
            }
        }
        _ => display_fractus(value),
    }
}

/// Exact `& | ^ ⇐ ⇒` on integer carriers (rulings 9, 10, 19b): infinite
/// two's-complement bit operations and unmasked shifts. The result is checked
/// against the exact 64-bit range only; the store applies the slot's policy.
pub fn int_bitwise_binop(
    left: Value,
    right: Value,
    op: MirBinOp,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    match crate::exact_int::exact_int_binop(&left, &right, op, width, span) {
        Some(result) => result,
        None => Err(RunnerError::internal(
            span,
            "bitwise operand requires numerus",
        )),
    }
}

const NEGATIVE_BITS_SHIFT: &str = "negative shift count";

/// The stored pattern of a raw-bits value. A `bN` cell always holds its
/// pattern zero-extended inside N bits; a value outside that
/// range is a representation defect, never silently masked.
fn bits_pattern(
    value: &Value,
    width: radix_types::BitsWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    let pattern = match value {
        Value::UInt(pattern) => *pattern,
        Value::Int(pattern) => u64::try_from(*pattern)
            .map_err(|_| RunnerError::internal(span, "bits operand is not zero-extended"))?,
        _ => return Err(RunnerError::internal(span, "bits operand requires bN")),
    };
    if width.bits() < 64 && pattern >> width.bits() != 0 {
        return Err(RunnerError::internal(
            span,
            "bits operand exceeds its width",
        ));
    }
    Ok(pattern)
}

/// Mask of the low N bits of a `bN`.
fn bits_mask(width: radix_types::BitsWidth) -> u64 {
    if width.bits() == 64 {
        u64::MAX
    } else {
        (1u64 << width.bits()) - 1
    }
}

/// `¬x` on a `bN`: flips exactly N bits.
pub(crate) fn bits_not(
    value: &Value,
    width: radix_types::BitsWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let pattern = bits_pattern(value, width, span)?;
    Ok(Value::UInt(!pattern & bits_mask(width)))
}

/// `∧ ∨ ⊻ ⇐ ⇒` on a `bN`: every result stays inside N bits. A shift count of
/// N or more gives all zeros (the count is not wrapped); a negative count is
/// a run-time error; a right shift fills with zeros.
pub(crate) fn bits_binop(
    left: &Value,
    right: &Value,
    op: MirBinOp,
    width: radix_types::BitsWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let left = bits_pattern(left, width, span)?;
    let pattern = match op {
        MirBinOp::BitAnd | MirBinOp::BitOr | MirBinOp::BitXor => {
            let right = bits_pattern(right, width, span)?;
            match op {
                MirBinOp::BitAnd => left & right,
                MirBinOp::BitOr => left | right,
                _ => left ^ right,
            }
        }
        MirBinOp::Shl | MirBinOp::Shr => {
            let count = match right {
                Value::Int(count) => i128::from(*count),
                Value::UInt(count) => i128::from(*count),
                _ => return Err(RunnerError::internal(span, "shift count requires numerus")),
            };
            if count < 0 {
                return Err(RunnerError::internal(span, NEGATIVE_BITS_SHIFT));
            }
            match u32::try_from(count) {
                Ok(count) if count < width.bits() => {
                    if op == MirBinOp::Shl {
                        (left << count) & bits_mask(width)
                    } else {
                        left >> count
                    }
                }
                _ => 0,
            }
        }
        _ => {
            return Err(RunnerError::internal(span, "invalid bits bitwise operator"));
        }
    };
    Ok(Value::UInt(pattern))
}

/// Width authority for `wrapping<W>` execution (D11.8): the ring Z/2^W whose
/// representatives are `[lo, hi]` of the width, so unsigned words hold
/// `0..=2^W-1` and signed words hold `-2^(W-1)..=2^(W-1)-1`. Arithmetic runs
/// on exact `i128` representatives and every result reduces into the ring.
/// Results surface as `UInt` at `u64` and as `Int` for every other width.
fn modular_word_range(width: NumericWidth) -> (i128, i128) {
    // Total on admitted widths (words never name `inf`: `wrapping<inf>` is bare
    // `inf`); the fallback keeps the helper panic-free.
    width.integer_range().unwrap_or((0, i128::from(u64::MAX)))
}

/// Bit count of the word.
fn modular_word_bits(width: NumericWidth) -> u32 {
    let (lo, hi) = modular_word_range(width);
    (hi - lo + 1).trailing_zeros()
}

/// Reduce an exact integer into the representatives of `width`.
pub(crate) fn modular_word_reduce(value: i128, width: NumericWidth) -> i128 {
    let (lo, hi) = modular_word_range(width);
    let modulus = hi - lo + 1;
    // Reduce into `0..modulus` first, then shift to the signed window: the
    // representative never needs `value - lo`, which overflows near `i128::MIN`
    // and `i128::MAX` for a signed word.
    let residue = value.rem_euclid(modulus);
    if residue > hi {
        residue - modulus
    } else {
        residue
    }
}

/// Reduce an integer-valued float into the ring of `width` exactly. Every
/// finite double is an exact integer here, so `% 2^W` (an exact `fmod`) gives
/// the residue without first narrowing the magnitude through `i128`.
pub(crate) fn modular_word_float_result(value: f64, width: NumericWidth) -> Value {
    let modulus = 2f64.powi(i32::try_from(modular_word_bits(width)).unwrap_or(64));
    // `|residue| < 2^W <= 2^64`, so the cast is exact.
    modular_word_result((value % modulus) as i128, width)
}

/// Reduce `value` into the ring and surface it in the width's carrier.
pub(crate) fn modular_word_result(value: i128, width: NumericWidth) -> Value {
    word_carrier(modular_word_reduce(value, width), width)
}

/// The exact representative a wrapping word carries.
pub(crate) fn modular_operand(
    value: Value,
    span: Span,
    error: &'static str,
) -> Result<i128, RunnerError> {
    match value {
        Value::Int(value) => Ok(i128::from(value)),
        Value::UInt(value) => Ok(i128::from(value)),
        _ => Err(RunnerError::internal(span, error)),
    }
}

/// Floor quotient and remainder of representatives (`/` and `%` floor); `None`
/// on a zero divisor.
fn floor_div_rem(left: i128, right: i128) -> Option<(i128, i128)> {
    let quotient = left.checked_div(right)?;
    let remainder = left - quotient * right;
    if remainder != 0 && ((remainder < 0) != (right < 0)) {
        Some((quotient - 1, remainder + right))
    } else {
        Some((quotient, remainder))
    }
}

/// The single construction of a `wrapping<W>` division-or-remainder failure:
/// a word division by zero traps as a modular failure, never as the checked
/// `numerus` one.
pub(crate) fn modular_division_by_zero(span: Span) -> RunnerError {
    RunnerError::internal_with_issue(span, "modulus division failed", "modular_division_by_zero")
}

/// Zero-divisor guard for a modular-origin `/` or `%` whose *result* type is a
/// plain integer. Range-based typing can give arithmetic on `wrapping<W>`
/// operands an exact integer result, so the modular carrier no longer selects
/// [`modular_word_numeric_binop`] and the operation would otherwise reach the
/// checked `numerus` dispatch, reporting `runner_numerus_division_failed`. The
/// divisor's zero is still a modular failure.
///
/// A nonzero divisor returns `None`: the operation stays exact and reduces only
/// at the store boundary, never through per-operation wrapping.
pub(crate) fn modular_zero_divisor(
    op: MirBinOp,
    divisor: &Value,
    modular_origin: bool,
    span: Span,
) -> Option<RunnerError> {
    if !modular_origin || !matches!(op, MirBinOp::Div | MirBinOp::Mod) {
        return None;
    }
    let zero = match divisor {
        Value::Int(value) => *value == 0,
        Value::UInt(value) => *value == 0,
        _ => false,
    };
    if zero {
        Some(modular_division_by_zero(span))
    } else {
        None
    }
}

/// Execute one `wrapping<W>` arithmetic operation. Conversion at this boundary
/// keeps modular wraparound separate from checked `numerus` math.
pub fn modular_word_numeric_binop(
    left: Value,
    right: Value,
    op: MirBinOp,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let left = modular_operand(left, span, "modulus operand mismatch")?;
    let right = modular_operand(right, span, "modulus operand mismatch")?;
    let by_zero = || modular_division_by_zero(span);
    let value = match op {
        // Representatives fit 64 bits, so sums and products fit i128 exactly
        // and reduction is exact.
        MirBinOp::Add => left + right,
        MirBinOp::Sub => left - right,
        MirBinOp::Mul => left.wrapping_mul(right),
        MirBinOp::Div => floor_div_rem(left, right).ok_or_else(by_zero)?.0,
        MirBinOp::Mod => floor_div_rem(left, right).ok_or_else(by_zero)?.1,
        _ => {
            return Err(RunnerError::internal(
                span,
                "invalid modulus arithmetic operator",
            ));
        }
    };
    Ok(modular_word_result(value, width))
}

pub fn modular_word_neg(
    value: Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let value = modular_operand(value, span, "modulus negation operand mismatch")?;
    Ok(modular_word_result(-value, width))
}

pub fn modular_word_bitnot(
    value: Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let value = modular_operand(value, span, "modulus bitnot operand mismatch")?;
    Ok(modular_word_result(!value, width))
}

pub fn modular_word_bitwise_binop(
    left: Value,
    right: Value,
    op: MirBinOp,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let left = modular_operand(left, span, "modulus bitwise operand mismatch")?;
    let right = modular_operand(right, span, "modulus bitwise operand mismatch")?;
    // Shifts are pure math (ruling 10): the count is not masked. A left shift
    // by the word's width or more shifts every bit out; a right shift is
    // arithmetic on the representative (`⇒` floors), so it saturates at the
    // sign bit.
    let bits = modular_word_bits(width);
    let count = u32::try_from(right).ok();
    let value = match op {
        MirBinOp::BitAnd => left & right,
        MirBinOp::BitOr => left | right,
        MirBinOp::BitXor => left ^ right,
        MirBinOp::Shl => match count {
            Some(count) if count < bits => left << count,
            _ => 0,
        },
        MirBinOp::Shr => left >> count.map_or(bits, |count| count.min(bits)),
        _ => {
            return Err(RunnerError::internal(
                span,
                "invalid modulus bitwise operator",
            ));
        }
    };
    Ok(modular_word_result(value, width))
}

pub fn modular_word_compare_values(
    left: Value,
    right: Value,
    op: MirBinOp,
    span: Span,
) -> Result<Value, RunnerError> {
    let left = modular_operand(left, span, "modulus comparison operand mismatch")?;
    let right = modular_operand(right, span, "modulus comparison operand mismatch")?;
    let ordering = left.cmp(&right);
    let result = match op {
        MirBinOp::Lt => ordering.is_lt(),
        MirBinOp::Gt => ordering.is_gt(),
        MirBinOp::LtEq => ordering.is_le(),
        MirBinOp::GtEq => ordering.is_ge(),
        _ => {
            return Err(RunnerError::internal(
                span,
                "invalid modulus comparison operator",
            ));
        }
    };
    Ok(Value::Bool(result))
}

// INVARIANT: the runner is Faber's target-neutral numeric authority. Integer
// overflow is therefore decided by the declared `numerus<W>` width, not by host
// i64 bounds. These helpers compute in i128 and range-check against `W` so a
// `numerus<u8>` add of 200 + 100 fails here exactly as `u8::checked_add` would
// fail in Rust codegen — independent of any target backend. Faber requires
// same-width operands, so the result width governs both sides.
//
// Scope: integer widths only. Float width fidelity (f32 vs f64 rounding) is a
// separate gap. The signed `(Int, Int)` carrier cannot encode `numerus<u64>`
// magnitudes above i64::MAX — those results fail closed here (`check_int_width`
// rejects them) instead of wrapping; the unsigned carrier path
// (`check_uint_width`, used for `Value::UInt` operands) keeps upper-half
// magnitudes exact.

/// Clamp an exact i128 result into `W`. The carrier follows the width like
/// `modular_word_result`: `UInt` at `u64`, `Int` below it.
pub(crate) fn saturating_word_result(value: i128, width: NumericWidth) -> Value {
    let (lo, hi) = int_width_range(width);
    word_carrier(value.clamp(lo, hi), width)
}

/// Surface a representative already inside `width`'s range in the width's
/// carrier: `UInt` at `u64`, `Int` at every other width. The `unwrap_or(0)`
/// keeps the helper total; callers reduce or clamp before calling.
fn word_carrier(value: i128, width: NumericWidth) -> Value {
    if width == NumericWidth::U64 {
        Value::UInt(u64::try_from(value).unwrap_or(0))
    } else {
        Value::Int(i64::try_from(value).unwrap_or(0))
    }
}

/// Signed `[min, max]` of an integer width as i128, so every Faber integer width
/// — including `u64` — range-checks without host overflow.
#[must_use]
pub fn int_width_range(width: NumericWidth) -> (i128, i128) {
    match width {
        NumericWidth::I8 => (-128, 127),
        NumericWidth::I16 => (-32_768, 32_767),
        NumericWidth::I32 => (i32::MIN as i128, i32::MAX as i128),
        NumericWidth::I64 => (i64::MIN as i128, i64::MAX as i128),
        NumericWidth::U8 => (0, u8::MAX as i128),
        NumericWidth::U16 => (0, u16::MAX as i128),
        NumericWidth::U32 => (0, u32::MAX as i128),
        NumericWidth::U64 => (0, u64::MAX as i128),
        // Floats are not range-checked here; their width is a separate fidelity gap.
        NumericWidth::F16 | NumericWidth::BF16 | NumericWidth::F32 | NumericWidth::F64 => {
            (i64::MIN as i128, i64::MAX as i128)
        }
        // decimal-widths D1: scaled-integer carriers fit their integer domain
        // (d64 = i64×10⁻⁸); D4 owns runner arithmetic.
        NumericWidth::D64 => (i64::MIN as i128, i64::MAX as i128),
        // `inf` has no bound: an `inf` operation never range-checks through
        // this i128 window (it computes on `Magnus`, `crate::inf`); the row
        // only keeps this helper total for a caller that reaches it with an
        // `inf`-typed 64-bit-carrier value.
        NumericWidth::Inf => (i128::MIN, i128::MAX),
    }
}

/// True when `value` is finite and its truncation toward zero lies in the
/// integer range of `width` (`↦` float-to-integer is a check, D1.11).
#[must_use]
pub fn float_truncation_fits_width(value: f64, width: NumericWidth) -> bool {
    if !value.is_finite() {
        return false;
    }
    let (lo, hi) = int_width_range(width);
    // f64 -> i128 `as` is exact for |v| < 2^127 and saturates beyond it, which
    // is far outside every integer width's range.
    let truncated = value.trunc() as i128;
    (lo..=hi).contains(&truncated)
}

/// Truncate a float carrier into the declared `numerus<W>` width, with Rust
/// `as` semantics (clamping). Callers own the range check: `fractus ↦ numerus`
/// only reaches this after `float_truncation_fits_width` (D1.11, D11.8); no
/// `∷` path reaches it, since a float-to-integer ascription is a compile error.
#[must_use]
pub fn float_to_numerus_width(value: f64, width: NumericWidth) -> Value {
    match width {
        NumericWidth::I8 => Value::Int(i64::from(value as i8)),
        NumericWidth::I16 => Value::Int(i64::from(value as i16)),
        NumericWidth::I32 => Value::Int(i64::from(value as i32)),
        NumericWidth::I64 => Value::Int(value as i64),
        NumericWidth::U8 => Value::UInt(u64::from(value as u8)),
        NumericWidth::U16 => Value::UInt(u64::from(value as u16)),
        NumericWidth::U32 => Value::UInt(u64::from(value as u32)),
        NumericWidth::U64 => Value::UInt(value as u64),
        // Float and decimal widths keep the width-agnostic saturating carrier;
        // decimals convert through the decimal carrier path.
        NumericWidth::F16
        | NumericWidth::BF16
        | NumericWidth::F32
        | NumericWidth::F64
        | NumericWidth::D64
        // `float ↦ inf` never reaches this helper: `convert_to_inf` truncates
        // through `Magnus` (NaN and ±∞ fail). Kept on the width-agnostic carrier.
        | NumericWidth::Inf => Value::Int(value as i64),
    }
}

// S07-12: the runner carries no local decimal carrier math. The scale factor
// and the pure round-half-even quotient are `radix_codegen_shared::decimal`'s
// — the one copy every tier (literal folding, emitted runtime code, this
// runner) computes through. Re-exported here so `conversio` and the decimal
// tests keep their `crate::value` import paths.
pub(crate) use radix_codegen_shared::decimal::decimal_scale_factor;
pub(crate) use radix_codegen_shared::decimal::round_half_even_quotient;

/// Execute one decimal arithmetic operation over exact decimal values.
pub(crate) fn decimal_numeric_binop(
    left: Value,
    right: Value,
    op: MirBinOp,
    _width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    crate::decimal::binop(&left, &right, op, span)
}

fn check_int_width(
    result: i128,
    width: NumericWidth,
    span: Span,
    error: &'static str,
) -> Result<i64, RunnerError> {
    let (lo, hi) = int_width_range(width);
    if !(lo..=hi).contains(&result) {
        return Err(RunnerError::internal(span, error));
    }
    // The i64 carrier cannot sign-encode `numerus<u64>` magnitudes above
    // i64::MAX — a raw `as i64` cast wraps them negative. Fail closed instead,
    // matching the out-of-range discipline of every other width; genuine u64
    // arithmetic runs through the unsigned carrier path (`check_uint_width`),
    // which keeps upper-half magnitudes exact as `UInt`.
    let Ok(encoded) = i64::try_from(result) else {
        return Err(RunnerError::internal(span, error));
    };
    Ok(encoded)
}

pub fn width_checked_neg(v: i64, width: NumericWidth, span: Span) -> Result<i64, RunnerError> {
    check_int_width(-(v as i128), width, span, trap_text::NUMERUS_OVERFLOW)
}

pub fn width_checked_add(
    a: i64,
    b: i64,
    width: NumericWidth,
    span: Span,
) -> Result<i64, RunnerError> {
    check_int_width(
        a as i128 + b as i128,
        width,
        span,
        trap_text::NUMERUS_OVERFLOW,
    )
}

pub fn width_checked_sub(
    a: i64,
    b: i64,
    width: NumericWidth,
    span: Span,
) -> Result<i64, RunnerError> {
    check_int_width(
        a as i128 - b as i128,
        width,
        span,
        trap_text::NUMERUS_OVERFLOW,
    )
}

pub fn width_checked_mul(
    a: i64,
    b: i64,
    width: NumericWidth,
    span: Span,
) -> Result<i64, RunnerError> {
    check_int_width(
        a as i128 * b as i128,
        width,
        span,
        trap_text::NUMERUS_OVERFLOW,
    )
}

/// Floor division (ruling 14). A zero divisor reports `numerus division
/// failed`; `MIN / -1` is the exact quotient and is range-checked like any
/// other result.
pub fn width_checked_div(
    a: i64,
    b: i64,
    width: NumericWidth,
    span: Span,
) -> Result<i64, RunnerError> {
    if b == 0 {
        return Err(RunnerError::internal(span, "numerus division failed"));
    }
    check_int_width(
        crate::exact_int::floor_div(a as i128, b as i128),
        width,
        span,
        "numerus division failed",
    )
}

pub fn width_checked_rem(
    a: i64,
    b: i64,
    width: NumericWidth,
    span: Span,
) -> Result<i64, RunnerError> {
    if b == 0 {
        return Err(RunnerError::internal(span, "numerus division failed"));
    }
    check_int_width(
        crate::exact_int::floor_rem(a as i128, b as i128),
        width,
        span,
        "numerus division failed",
    )
}

/// Unsigned counterparts of the width-checked ops above. They compute in u128
/// and range-check against the declared width so `numerus<u64>` upper-half
/// magnitudes stay exact — never sign-encoded into i64.
fn check_uint_width(
    result: u128,
    width: NumericWidth,
    span: Span,
    error: &'static str,
) -> Result<u64, RunnerError> {
    let (_, hi) = int_width_range(width);
    if result <= hi as u128 {
        Ok(result as u64)
    } else {
        Err(RunnerError::internal(span, error))
    }
}

/// Unsigned negation: only zero is representable; anything else overflows.
pub fn uint_width_checked_neg(
    v: u64,
    _width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    if v == 0 {
        Ok(0)
    } else {
        Err(RunnerError::internal(span, trap_text::NUMERUS_OVERFLOW))
    }
}

pub fn uint_width_checked_add(
    a: u64,
    b: u64,
    width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    check_uint_width(
        a as u128 + b as u128,
        width,
        span,
        trap_text::NUMERUS_OVERFLOW,
    )
}

pub fn uint_width_checked_sub(
    a: u64,
    b: u64,
    width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    let result = a
        .checked_sub(b)
        .ok_or_else(|| RunnerError::internal(span, trap_text::NUMERUS_OVERFLOW))?;
    check_uint_width(result as u128, width, span, trap_text::NUMERUS_OVERFLOW)
}

pub fn uint_width_checked_mul(
    a: u64,
    b: u64,
    width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    check_uint_width(
        a as u128 * b as u128,
        width,
        span,
        trap_text::NUMERUS_OVERFLOW,
    )
}

pub fn uint_width_checked_div(
    a: u64,
    b: u64,
    width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    if b == 0 {
        return Err(RunnerError::internal(span, "numerus division failed"));
    }
    check_uint_width((a / b) as u128, width, span, "numerus division failed")
}

pub fn uint_width_checked_rem(
    a: u64,
    b: u64,
    width: NumericWidth,
    span: Span,
) -> Result<u64, RunnerError> {
    if b == 0 {
        return Err(RunnerError::internal(span, "numerus division failed"));
    }
    check_uint_width((a % b) as u128, width, span, "numerus division failed")
}

/// Declared numeric width of `ty`, seeing a policy-float cell as the bare
/// float of its width (D2: the cell adds no behavior of its own to evaluation,
/// display or arithmetic; only a `trapping` store checks it, `store.rs`).
///
/// This wraps [`radix_types::effective_width`] rather than extending it: that
/// function also answers the type checker's widening predicates, where a bare
/// float must not read as widening into a cell (D4).
pub(crate) fn effective_width(types: &TypeTable, ty: TypeId) -> Option<NumericWidth> {
    match types.get(ty) {
        Type::PolicyFloat(_, width) => Some(*width),
        Type::Alias(_, inner) => effective_width(types, *inner),
        _ => radix_types::effective_width(types, ty),
    }
}

/// The overflow policy and width of a policy-float cell, alias-peeled.
pub(crate) fn policy_float_cell(
    types: &TypeTable,
    ty: TypeId,
) -> Option<(FloatPolicy, NumericWidth)> {
    match types.get(ty) {
        Type::PolicyFloat(policy, width) => Some((*policy, *width)),
        Type::Alias(_, inner) => policy_float_cell(types, *inner),
        _ => None,
    }
}

/// D1: whether storing `value` into a `trapping<width>` cell traps. NaN and
/// both infinities trap. Bare `f16` and `bf16` slots round at the store via
/// [`round_float_at_slot`], so a trapping store traps exactly when the
/// slot-rounded value is infinite. Nothing clamps.
#[must_use]
pub(crate) fn trapping_float_store_fails(value: f64, width: NumericWidth) -> bool {
    if value.is_nan() {
        return true;
    }
    let rounds_to_infinity_at = match width {
        // 65504 is the largest finite binary16; 65520 is the tie that rounds up.
        NumericWidth::F16 => 65_520.0,
        // The largest finite bfloat16 is (2 - 2^-7) * 2^127; the tie that
        // rounds up to infinity is (2 - 2^-8) * 2^127.
        NumericWidth::BF16 => (2.0 - 2f64.powi(-8)) * 2f64.powi(127),
        NumericWidth::F32 => return !f64::from(value as f32).is_finite(),
        _ => return !value.is_finite(),
    };
    value.abs() >= rounds_to_infinity_at
}

/// Round a scalar float to the runner's declared arithmetic width.
///
/// WHY: `Value::Float` is a single f64 carrier, but Faber scalar arithmetic
/// still executes at the declared width. The fuzzy-value path uses this same
/// f32-round-trip policy for every non-f64 width.
pub(crate) fn round_float(value: f64, width: NumericWidth) -> f64 {
    if matches!(width, NumericWidth::F64) {
        value
    } else {
        #[expect(clippy::cast_possible_truncation)]
        {
            value as f32 as f64
        }
    }
}

/// Round a float at a store/conversion slot of `width` (ruling 26(d)): f16
/// and bf16 round to their own formats, nearest-even, +/-inf past the finite
/// range; f32 through binary32; f64 and non-float widths unchanged.
pub(crate) fn round_float_at_slot(value: f64, width: NumericWidth) -> f64 {
    match width {
        NumericWidth::F64 => value,
        NumericWidth::F32 => f64::from(value as f32),
        NumericWidth::F16 => crate::f16_round_f64(value),
        NumericWidth::BF16 => crate::bf16_round_f64(value),
        _ => value,
    }
}

fn float_binop(
    left: f64,
    right: f64,
    width: NumericWidth,
    float_op: impl FnOnce(f64, f64) -> f64,
) -> f64 {
    let left = round_float(left, width);
    let right = round_float(right, width);
    round_float(float_op(left, right), width)
}

/// Numeric binary dispatch. Integer operands compute exactly (d11-6 ruling
/// 17a): the result must lie in the single 64-bit range, and the store into a
/// slot applies the slot's policy. Scalar float operands and results are
/// rounded at the declared width before and after the operation.
pub fn numeric_binop(
    left: Value,
    right: Value,
    op: MirBinOp,
    float_op: impl FnOnce(f64, f64) -> f64,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    if let Some(result) = crate::exact_int::exact_int_binop(&left, &right, op, width, span) {
        return result;
    }
    match (left, right) {
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_binop(a, b, width, float_op))),
        (Value::Int(a), Value::Float(b)) => {
            Ok(Value::Float(float_binop(a as f64, b, width, float_op)))
        }
        (Value::UInt(a), Value::Float(b)) => {
            Ok(Value::Float(float_binop(a as f64, b, width, float_op)))
        }
        (Value::Float(a), Value::Int(b)) => {
            Ok(Value::Float(float_binop(a, b as f64, width, float_op)))
        }
        (Value::Float(a), Value::UInt(b)) => {
            Ok(Value::Float(float_binop(a, b as f64, width, float_op)))
        }
        _ => Err(RunnerError::internal(span, "numeric operand mismatch")),
    }
}

pub fn add_values(
    left: Value,
    right: Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    if width.is_decimal() {
        return decimal_numeric_binop(left, right, MirBinOp::Add, width, span);
    }
    match (left, right) {
        (Value::Str(left), Value::Str(right)) => Ok(Value::Str(format!("{left}{right}"))),
        (left, right) => numeric_binop(left, right, MirBinOp::Add, |a, b| a + b, width, span),
    }
}

/// Sort comparator for `lista` `.sort()`/`.sorted()`: a total order over
/// runner scalars (floats by IEEE 754 totalOrder).
pub(crate) fn compare_sort_values(left: &Value, right: &Value) -> std::cmp::Ordering {
    match (left, right) {
        // `inf` (ruling 37): exact order against any integer carrier.
        (Value::Magnus(_), Value::Int(_) | Value::UInt(_) | Value::Magnus(_))
        | (Value::Int(_) | Value::UInt(_), Value::Magnus(_)) => {
            crate::exact_int::cmp_numeric(left, right).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::UInt(a), Value::UInt(b)) => a.cmp(b),
        // Mixed carriers order by numeric value: a negative signed value
        // sorts below every unsigned value, a non-negative signed value
        // compares by magnitude (same rule as `compare_values`).
        (Value::Int(a), Value::UInt(b)) => {
            if *a < 0 {
                std::cmp::Ordering::Less
            } else {
                (*a as u64).cmp(b)
            }
        }
        (Value::UInt(a), Value::Int(b)) => {
            if *b < 0 {
                std::cmp::Ordering::Greater
            } else {
                a.cmp(&(*b as u64))
            }
        }
        // IEEE 754 totalOrder (operator ruling 2026-09-24): -NaN < -∞ < … <
        // -0.0 < +0.0 < … < +∞ < +NaN, so NaN never scrambles a float sort.
        (Value::Float(a), Value::Float(b)) => a.total_cmp(b),
        (Value::Str(a), Value::Str(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        _ => format!("{left:?}").cmp(&format!("{right:?}")),
    }
}

/// Three-way compare through the ordering contract (D1.4).
///
/// Integers by value, `fractus` by IEEE 754 totalOrder (operator default
/// F2), text by Unicode code point (D1.6), `instans` by point in time, and
/// tuples lexicographically. `None` for any other operand pair: a genus
/// witness calls its own `compare` and never reaches this helper.
pub(crate) fn contract_compare_values(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (
            Value::Int(_) | Value::UInt(_) | Value::Magnus(_),
            Value::Int(_) | Value::UInt(_) | Value::Magnus(_),
        )
        | (Value::Float(_), Value::Float(_))
        | (Value::Str(_), Value::Str(_)) => Some(compare_sort_values(left, right)),
        (Value::Instans(a), Value::Instans(b)) => a.partial_cmp_at_coarser(*b),
        (Value::Tuple(a), Value::Tuple(b)) if a.len() == b.len() => {
            for (x, y) in a.iter().zip(b) {
                let ordering = contract_compare_values(x, y)?;
                if ordering.is_ne() {
                    return Some(ordering);
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        _ => None,
    }
}

/// IEEE 754-2019 `minimum` (operator ruling 2026-09-24): NaN in either
/// operand propagates, and -0.0 orders below +0.0.
#[allow(clippy::float_cmp)] // IEEE ordering needs exact equality for the signed-zero tie
pub(crate) fn ieee_minimum(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        a + b
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}

/// IEEE 754-2019 `maximum` (operator ruling 2026-09-24): NaN in either
/// operand propagates, and +0.0 orders above -0.0.
#[allow(clippy::float_cmp)] // IEEE ordering needs exact equality for the signed-zero tie
pub(crate) fn ieee_maximum(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        a + b
    } else if a > b || (a == b && a.is_sign_positive()) {
        a
    } else {
        b
    }
}

pub fn compare_values(
    left: Value,
    right: Value,
    op: MirBinOp,
    span: Span,
) -> Result<Value, RunnerError> {
    // IEEE 754 (operator ruling 2026-09-24): float pairs yield `None` when
    // either side is NaN, and every ordered predicate is then false — NaN
    // flows through comparisons, it never traps.
    let ordering = match (&left, &right) {
        // Integer and float operands compare their exact mathematical values
        // (ruling 22): an integer above 2^53 is never rounded to a float.
        (
            Value::Int(_) | Value::UInt(_) | Value::Magnus(_) | Value::Float(_),
            Value::Int(_) | Value::UInt(_) | Value::Magnus(_) | Value::Float(_),
        ) => crate::exact_int::cmp_numeric(&left, &right),
        (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
        (Value::Ascii(a), Value::Ascii(b)) => Some(a.cmp(b)),
        (Value::Char(a), Value::Char(b)) => Some(a.cmp(b)),
        (Value::Char(a), Value::Ascii(b)) => Some(a.cmp(&char::from(*b))),
        (Value::Ascii(a), Value::Char(b)) => Some(char::from(*a).cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        (Value::Instans(a), Value::Instans(b)) => Some(
            a.partial_cmp_at_coarser(*b)
                .ok_or_else(|| RunnerError::internal(span, "instans compare"))?,
        ),
        _ => return Err(RunnerError::internal(span, "comparison type mismatch")),
    };
    let result = match op {
        MirBinOp::Lt => ordering.is_some_and(std::cmp::Ordering::is_lt),
        MirBinOp::Gt => ordering.is_some_and(std::cmp::Ordering::is_gt),
        MirBinOp::LtEq => ordering.is_some_and(std::cmp::Ordering::is_le),
        MirBinOp::GtEq => ordering.is_some_and(std::cmp::Ordering::is_ge),
        _ => return Err(RunnerError::internal(span, "invalid comparison operator")),
    };
    Ok(Value::Bool(result))
}

#[must_use]
pub fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Instans(a), Value::Instans(b)) => a
            .partial_cmp_at_coarser(*b)
            .is_some_and(|ord| ord == std::cmp::Ordering::Equal),
        // Mixed carriers compare by numeric value: a non-negative `Int` and a
        // `UInt` with the same magnitude are the same Faber integer.
        // Integer and float operands compare their exact values (ruling 22);
        // NaN equals nothing. Fuzzy `≈` is a collection Approximate call and
        // does not use this exact-value helper.
        (
            Value::Int(_) | Value::UInt(_) | Value::Magnus(_),
            Value::Int(_) | Value::UInt(_) | Value::Magnus(_) | Value::Float(_),
        )
        | (Value::Float(_), Value::Int(_) | Value::UInt(_) | Value::Magnus(_)) => {
            crate::exact_int::cmp_numeric(left, right).is_some_and(std::cmp::Ordering::is_eq)
        }
        // Scalar `s[i]`/`accipe` yields the one-byte `ascii<1>` carrier while
        // literals arrive as heap textus; the declared intrinsic return is
        // Textus, so equality joins the carriers by character value (a single
        // ASCII byte). Multi-byte textus never equals an `ascii<1>`.
        (Value::Ascii(a), Value::Str(b)) => b.as_bytes() == [*a],
        (Value::Str(a), Value::Ascii(b)) => a.as_bytes() == [*b],
        // `littera` joins the ascii scalar and a one-letter textus by value.
        (Value::Char(a), Value::Ascii(b)) | (Value::Ascii(b), Value::Char(a)) => {
            *a == char::from(*b)
        }
        (Value::Char(a), Value::Str(b)) | (Value::Str(b), Value::Char(a)) => {
            let mut chars = b.chars();
            chars.next() == Some(*a) && chars.next().is_none()
        }
        // A ledger's insertion order is part of its value (L2e.1): entries
        // compare slot-by-slot through [`LedgerValue::eq`]. `Rc::ptr_eq`
        // short-circuits the same cell before either side is borrowed, so a
        // shared ledger is never borrowed twice here.
        (Value::Ledger(a), Value::Ledger(b)) => Rc::ptr_eq(a, b) || *a.borrow() == *b.borrow(),
        // Collections compare elementwise through this same canonical helper:
        // a converted bits list holds `UInt` cells while an equal-valued
        // literal holds `Int` cells, so derived `PartialEq` would split them.
        (Value::Array(a), Value::Array(b)) | (Value::Vector(a), Value::Vector(b)) => {
            let a = a.borrow();
            let b = b.borrow();
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y))
        }
        (Value::Tuple(a), Value::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y))
        }
        _ => left == right,
    }
}

/// One-byte `ascii<1>` from a Unicode scalar. Non-ASCII fails closed.
pub fn ascii1_from_scalar(ch: char, span: Span) -> Result<Value, RunnerError> {
    if u32::from(ch) > 0x7f {
        return Err(RunnerError::internal(span, "textus index trap"));
    }
    Ok(Value::Ascii(ch as u8))
}

/// Scalar `[i]` on a textus/ascii payload: the i-th Unicode character. An
/// ASCII character keeps the compact one-byte `ascii<1>` carrier; any other
/// scalar is a one-character heap textus (the one-byte carrier cannot hold it).
/// Fails only when `index` is past the last character.
pub fn scalar_text_index(text: &str, index: usize, span: Span) -> Result<Value, RunnerError> {
    let ch = text
        .chars()
        .nth(index)
        .ok_or_else(|| RunnerError::internal(span, trap_text::TEXT_INDEX_OUT_OF_BOUNDS))?;
    if ch.is_ascii() {
        ascii1_from_scalar(ch, span)
    } else {
        Ok(Value::Str(ch.to_string()))
    }
}

/// The position a proven index names. The checker showed `0 <= index`, so the
/// sign is not tested; `None` only for a value that is not an integer, which
/// keeps the checked read.
#[must_use]
pub(crate) fn proven_position(index: &Value) -> Option<usize> {
    match index {
        Value::Int(value) => Some(*value as usize),
        Value::UInt(value) => usize::try_from(*value).ok(),
        _ => None,
    }
}

/// Text of a value a `textus` consumer accepts. A scalar bracket projection
/// (`s[i]`) carries the compact one-byte `Value::Ascii` because MIR places have
/// no static type for the runner to widen by (F-44); every `Str`-only consumer
/// reads its text through here so both carriers mean the same text.
#[must_use]
pub fn as_text_str(value: Value) -> Option<String> {
    match value {
        Value::Str(text) => Some(text),
        Value::Ascii(byte) => Some(char::from(byte).to_string()),
        _ => None,
    }
}

pub fn format_template(
    template: &str,
    args: &[Value],
    widths: &[Option<NumericWidth>],
    char_text: &[bool],
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
) -> Result<String, RunnerError> {
    format_template_shaped(
        template,
        args,
        widths,
        char_text,
        &[],
        &[],
        &[],
        interner,
        order,
        span,
    )
}

/// [`format_template`] with the static tuple shape of each argument.
#[expect(clippy::too_many_arguments)]
pub fn format_template_shaped(
    template: &str,
    args: &[Value],
    widths: &[Option<NumericWidth>],
    char_text: &[bool],
    tuples: &[Option<TupleParts>],
    bits: &[Option<radix_types::BitsWidth>],
    intervals: &[bool],
    interner: &Interner,
    order: FieldOrder<'_>,
    span: Span,
) -> Result<String, RunnerError> {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    let mut next_arg = 0usize;
    while let Some(ch) = chars.next() {
        if ch != '§' {
            out.push(ch);
            continue;
        }
        let mut index = String::new();
        while chars.peek().is_some_and(char::is_ascii_digit) {
            let Some(digit) = chars.next() else {
                break;
            };
            index.push(digit);
        }
        let arg_index = if index.is_empty() {
            let current = next_arg;
            next_arg += 1;
            current
        } else {
            index.parse::<usize>().unwrap_or(usize::MAX)
        };
        let rendered = args
            .get(arg_index)
            .map(|arg| {
                let width = widths.get(arg_index).copied().flatten();
                let chars = char_text.get(arg_index).copied().unwrap_or(false);
                let tuple = tuples.get(arg_index).and_then(Option::as_ref);
                let order = order
                    .with_char_text(chars)
                    .with_tuple(tuple)
                    .with_bits(bits.get(arg_index).copied().flatten())
                    .with_interval(intervals.get(arg_index).copied().unwrap_or(false));
                arg.display_typed(interner, order, span, width)
            })
            .transpose()?
            .unwrap_or_else(|| format!("§{index}"));
        out.push_str(&rendered);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "f16_bf16_slot_round_test.rs"]
mod f16_bf16_slot_round_test;

/// The `(start, end, inclusive)` window of an interval index, or `None` for a
/// plain element index. Tests the carrier shape directly: the fallible
/// `interval_parts` builds (and the caller then drops) a `RunnerError` for
/// every non-interval value, which is every ordinary `xs[i]`.
fn window_interval(value: &Value) -> Option<(Value, Value, bool)> {
    let Value::Tuple(parts) = value else {
        return None;
    };
    // A stepped window carries its step as a fourth part (`window_step`).
    let ([start, end, Value::Bool(inclusive)] | [start, end, Value::Bool(inclusive), _]) =
        parts.as_slice()
    else {
        return None;
    };
    Some((start.clone(), end.clone(), *inclusive))
}

/// The half-open bounds of a range index (an inclusive end advanced by one),
/// `None` when the index is not a range. A negative bound is the negative
/// index failure, as for a scalar index.
pub(crate) fn window_bounds(
    index: &Value,
    span: Span,
) -> Result<Option<(i128, i128)>, RunnerError> {
    let Some((start, end, inclusive)) = window_interval(index) else {
        return Ok(None);
    };
    let signed = |bound: &Value| match bound {
        Value::Int(value) if *value >= 0 => Ok(i128::from(*value)),
        Value::UInt(value) => Ok(i128::from(*value)),
        _ => Err(RunnerError::internal(span, trap_text::NEGATIVE_INDEX)),
    };
    let start = signed(&start)?;
    let end = signed(&end)? + i128::from(inclusive);
    Ok(Some((start, end)))
}
