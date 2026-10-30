//! Shared physical layout facts for MIR-backed systems targets.
//!
//! TARGET: low-level MIR consumers that need representation facts before
//! emitting target text. The table is intentionally target-neutral: it says
//! what the compiler knows about a value's physical carrier, not how any one
//! backend spells that carrier.

use crate::{MirLayoutId, MirType};
use handroll::{FxHashMap, FxHashSet};
use radix_types::{
    BitsWidth, IndexExpr, IndexId, NumericWidth, Primitive, Type, TypeId, TypeTable,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirScalarLayout {
    Bool,
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    F16,
    /// IEEE-754 bfloat16 (top 16 bits of an f32). Native on Metal (`bfloat`);
    /// fail-closed on targets without a carrier.
    Bf16,
    F32,
    F64,
    /// The unbounded `inf` integer (inf-delivery rulings 32, 46): an opaque,
    /// heap-resident bignum with no fixed size or device layout. Appended so
    /// the serde index of every earlier variant is unchanged. Every native
    /// emitter fails closed on it (ruling 46) and every device gate rejects
    /// it (ruling 39); only the MIR runner carries a value of this layout.
    Magnus,
    /// `d64`: an `i64` mantissa at scale 10^-8 (decimal-widths; kernel-wide
    /// types ruling 2026-10-28). Eight bytes, eight-byte aligned, like `i64`,
    /// but never an integer: arithmetic on it is decimal arithmetic. Appended
    /// so the serde index of every earlier variant is unchanged. A consumer
    /// that does not carry decimals must refuse it by name.
    D64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirLayoutKind {
    Scalar(MirScalarLayout),
    Aggregate(MirAggregateLayout),
    DeviceView(MirDeviceViewLayout),
    DeviceVector(MirDeviceVectorLayout),
    DeviceMatrix(MirDeviceMatrixLayout),
    Atomic(MirAtomicLayout),
    PackedNumeric(MirPackedNumericLayout),
    OpaqueHandle,
    Void,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirAggregateFamily {
    Array,
    Map,
    Record,
    Set,
    TensorRuntime,
    Option,
    Struct,
    Enum,
    Union,
    Tuple,
    BoundedArray,
    BoundedAscii,
    BoundedTextus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirAggregateCarrier {
    OpaqueRuntimeHandle,
    PhysicalValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirAggregateLayout {
    pub family: MirAggregateFamily,
    pub carrier: MirAggregateCarrier,
    /// Scalar element fact for the bounded families: `Some(U8)` for the
    /// `ascii<N>`/`textus<N>` byte storage; `Some(s)` when the `T` of
    /// `BoundedArray<T, N>` is itself scalar. `None` for every other family.
    pub element: Option<MirScalarLayout>,
    /// Compile-time capacity for the bounded families — `element × capacity`
    /// storage cells, plus the runtime length member on the byte families.
    /// `None` for every other family.
    pub capacity: Option<u64>,
}

/// Element of a device view: a scalar cell, one packed vector per cell
/// (`tensor<vector<f32, size 4>, [H, W]>`), or a genus record per cell. The vector
/// arm admits only widths whose size equals their alignment (2 and 4); width 3
/// fails closed at layout time because MSL `float3` has a 16-byte stride.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirDeviceViewElement {
    Scalar(MirScalarLayout),
    Vector(MirDeviceVectorLayout),
    /// The element is a genus record; its byte size, alignment and field
    /// offsets come only from `resolve_struct_layout`. Device byte order is
    /// little-endian. The layout table is built from the `TypeTable` alone and
    /// cannot see struct fields, so the variant carries the element type id and
    /// consumers resolve it with the validation context.
    Record(TypeId),
}

impl MirDeviceViewElement {
    /// The scalar element, or `None` for a vector or record element.
    #[must_use]
    pub fn scalar(self) -> Option<MirScalarLayout> {
        match self {
            Self::Scalar(scalar) => Some(scalar),
            Self::Vector(_) | Self::Record(_) => None,
        }
    }
}

impl From<MirScalarLayout> for MirDeviceViewElement {
    fn from(scalar: MirScalarLayout) -> Self {
        Self::Scalar(scalar)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirDeviceViewLayout {
    pub element: MirDeviceViewElement,
    pub rank: u8,
    pub storage: MirTensorStorageLayout,
    pub locus: MirTensorLocusLayout,
    pub view: MirTensorViewLayout,
    pub offset_elements: u64,
    pub contiguous: bool,
    pub row_major: bool,
    pub stride: MirTensorStrideLayout,
    /// Static element count when the view's extent is a compile-time bound.
    /// `None` for tensor views (runtime extent) — `octeti<N>` sets it to `N`.
    pub length: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirTensorStorageLayout {
    HostOwned,
    DeviceHandle,
    /// MLC-4: sparse backing store (`sparsa` values) — layout facts only;
    /// every kernel admission surface rejects it by name, fail-closed.
    Sparse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirTensorLocusLayout {
    Host,
    Device,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirTensorViewLayout {
    Materialized,
    Axis0View,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirTensorStrideLayout {
    RowMajor,
    Axis0 { stride_elements: u64 },
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirDeviceVectorLayout {
    pub element: MirScalarLayout,
    pub width: u8,
    pub align: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirDeviceMatrixLayout {
    pub element: MirScalarLayout,
    pub rows: u8,
    pub columns: u8,
    pub align: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirAtomicLayout {
    pub element: MirScalarLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirPackedNumericFormat {
    ToyU4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirPackedBitOrder {
    LowNibbleFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirPackedNumericLayout {
    pub format: MirPackedNumericFormat,
    pub packed_element_width_bits: u8,
    pub values_per_byte: u8,
    pub block_values: u16,
    pub bit_order: MirPackedBitOrder,
    pub widened_element: MirScalarLayout,
    pub has_scale: bool,
    pub has_zero_point: bool,
}

impl MirPackedNumericLayout {
    #[must_use]
    pub const fn toy_u4() -> Self {
        Self {
            format: MirPackedNumericFormat::ToyU4,
            packed_element_width_bits: 4,
            values_per_byte: 2,
            block_values: 8,
            bit_order: MirPackedBitOrder::LowNibbleFirst,
            widened_element: MirScalarLayout::F32,
            has_scale: true,
            has_zero_point: true,
        }
    }

    #[must_use]
    pub const fn unsupported_execution_issue(self) -> &'static str {
        match self.format {
            MirPackedNumericFormat::ToyU4 => "mir_packed_numeric_toy_u4_execution_unsupported",
        }
    }
}

impl MirDeviceVectorLayout {
    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`String`].
    pub fn wgsl_ty(self) -> Result<String, String> {
        let elem = wgsl_element_ty(self.element)?;
        Ok(format!("vec{}<{elem}>", self.width))
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`String`].
    pub fn metal_ty(self) -> Result<String, String> {
        let elem = metal_vector_element_ty(self.element)?;
        Ok(format!("{elem}{}", self.width))
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`String`].
    pub fn llvm_ty(self) -> Result<String, String> {
        let elem = llvm_element_ty(self.element)?;
        Ok(format!("<{} x {elem}>", self.width))
    }
}

impl MirDeviceMatrixLayout {
    /// WGSL matrix types are `mat<columns>x<rows><element>` (column-major).
    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`String`].
    pub fn wgsl_ty(self) -> Result<String, String> {
        let elem = wgsl_matrix_element_ty(self.element)?;
        Ok(format!("mat{}x{}<{elem}>", self.columns, self.rows))
    }
}

fn wgsl_element_ty(element: MirScalarLayout) -> Result<&'static str, String> {
    match element {
        MirScalarLayout::Bool => Ok("bool"),
        MirScalarLayout::I8 | MirScalarLayout::I16 | MirScalarLayout::I32 => Ok("i32"),
        MirScalarLayout::U8 | MirScalarLayout::U16 | MirScalarLayout::U32 => Ok("u32"),
        MirScalarLayout::F32 => Ok("f32"),
        other => Err(format!("kernel vector element {}", other.spelling())),
    }
}

fn wgsl_matrix_element_ty(element: MirScalarLayout) -> Result<&'static str, String> {
    match element {
        MirScalarLayout::F32 => Ok("f32"),
        other => Err(format!("kernel matrix element {}", other.spelling())),
    }
}

fn metal_vector_element_ty(element: MirScalarLayout) -> Result<&'static str, String> {
    // MSL vector values use their actual lane storage width. `metal_element_ty`
    // intentionally widens narrow scalar buffer elements to int/uint, which
    // is not valid for a packed by-value vector ABI.
    match element {
        MirScalarLayout::Bool => Ok("bool"),
        MirScalarLayout::I8 => Ok("char"),
        MirScalarLayout::U8 => Ok("uchar"),
        MirScalarLayout::I16 => Ok("short"),
        MirScalarLayout::U16 => Ok("ushort"),
        MirScalarLayout::I32 => Ok("int"),
        MirScalarLayout::U32 => Ok("uint"),
        MirScalarLayout::F32 => Ok("float"),
        other => Err(format!("kernel vector element {}", other.spelling())),
    }
}

fn llvm_element_ty(element: MirScalarLayout) -> Result<&'static str, String> {
    match element {
        MirScalarLayout::Bool => Ok("i1"),
        MirScalarLayout::I8 | MirScalarLayout::U8 => Ok("i8"),
        MirScalarLayout::I16 | MirScalarLayout::U16 => Ok("i16"),
        MirScalarLayout::I32 | MirScalarLayout::U32 => Ok("i32"),
        MirScalarLayout::I64 | MirScalarLayout::U64 => Ok("i64"),
        MirScalarLayout::F32 => Ok("float"),
        MirScalarLayout::F64 => Ok("double"),
        other => Err(format!("kernel vector element {}", other.spelling())),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirLayout {
    pub id: MirLayoutId,
    pub kind: MirLayoutKind,
    pub size_bits: Option<u16>,
    pub align_bits: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirLayoutError {
    pub message: String,
    pub issue: &'static str,
    pub id: Option<MirLayoutId>,
    pub ty: Option<TypeId>,
    /// Named facts behind the message (`reason`, `width`, ...); consumers
    /// identify the failure by `issue` and these, never by the prose.
    pub args: LayoutFacts,
}

/// Named facts behind a layout refusal (`reason`, `width`, ...).
pub type LayoutFacts = Vec<(&'static str, String)>;

/// Resolution of one `Type` variant under the layout resolver.
///
/// Distinct from `Option<MirLayoutKind>` so an unmapped sized numeric is a
/// *loud* missing fact (the fallthrough law) instead of a silent no-row.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LayoutResolution {
    Resolved(MirLayoutKind),
    /// Compiler-internal type — no layout row and no error (`Param`, `Infer`,
    /// holes, `Error`).
    NoLayout,
    /// A type that fails the layout resolver loudly; the message names the
    /// offending family/width.
    MissingFact(String, LayoutFacts),
}

impl LayoutResolution {
    /// Continue with a resolved kind; `NoLayout` / `MissingFact` propagate.
    fn then(self, f: impl FnOnce(MirLayoutKind) -> LayoutResolution) -> LayoutResolution {
        match self {
            LayoutResolution::Resolved(kind) => f(kind),
            other => other,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirLayoutTable {
    layouts: Vec<MirLayout>,
    by_type: FxHashMap<TypeId, MirLayoutId>,
    /// Named missing-fact rows (fallthrough law): an unmapped sized numeric
    /// records its width-named message here instead of a silent no-row, so
    /// resolving it fails loudly with `mir_layout_missing_type_fact`.
    missing_facts: FxHashMap<TypeId, (String, LayoutFacts)>,
}

impl MirLayoutTable {
    #[must_use]
    pub fn from_types(types: &TypeTable) -> Self {
        let mut table = Self::default();
        let mut memo = FxHashMap::default();
        let mut visiting = FxHashSet::default();
        for index in 0..types.type_count() {
            // TypeId is a u32 arena index by design (radix-types::types).
            #[expect(
                clippy::cast_possible_truncation,
                reason = "TypeId is a u32 arena index by design"
            )]
            let ty = TypeId(index as u32);
            match layout_kind_for_type(types, ty, &mut memo, &mut visiting) {
                LayoutResolution::Resolved(kind) => {
                    table.set_type_layout(ty, kind);
                }
                LayoutResolution::MissingFact(message, args) => {
                    table.missing_facts.insert(ty, (message, args));
                }
                LayoutResolution::NoLayout => {}
            }
        }
        table
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirLayoutError`].
    pub fn layout_for_type(&self, ty: MirType) -> Result<&MirLayout, MirLayoutError> {
        let semantic = ty.semantic_id();
        let id = ty
            .layout_id()
            .or_else(|| self.by_type.get(&semantic).copied())
            .ok_or_else(|| match self.missing_facts.get(&semantic) {
                // Fallthrough law: a named missing fact (an unmapped sized
                // numeric) is loud, with the family/width named in the message.
                Some((message, args)) => MirLayoutError {
                    message: message.clone(),
                    issue: "mir_layout_missing_type_fact",
                    id: None,
                    ty: Some(semantic),
                    args: args.clone(),
                },
                None => MirLayoutError {
                    message: format!("missing layout fact for ty#{}", semantic.0),
                    issue: "mir_layout_missing_type_fact",
                    id: None,
                    ty: Some(semantic),
                    args: vec![("reason", "no_layout_fact".to_owned())],
                },
            })?;
        self.layout(id).ok_or_else(|| MirLayoutError {
            message: format!("missing layout row #{}", id.0),
            issue: "mir_layout_missing_row",
            id: Some(id),
            ty: None,
            args: vec![("reason", "no_layout_row".to_owned())],
        })
    }

    #[must_use]
    pub fn layout_id_for_semantic_type(&self, ty: TypeId) -> Option<MirLayoutId> {
        self.by_type.get(&ty).copied()
    }

    #[must_use]
    pub fn layout(&self, id: MirLayoutId) -> Option<&MirLayout> {
        self.layouts.get(id.0 as usize)
    }

    pub fn insert_layout(&mut self, kind: MirLayoutKind) -> MirLayoutId {
        self.insert_layout_row(kind)
    }

    pub fn set_type_layout(&mut self, ty: TypeId, kind: MirLayoutKind) -> MirLayoutId {
        let id = self.insert_layout_row(kind);
        self.by_type.insert(ty, id);
        id
    }

    fn insert_layout_row(&mut self, kind: MirLayoutKind) -> MirLayoutId {
        // MirLayoutId is a u32 backend handle by design; rows derive from the u32-bounded type arena.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirLayoutId is a u32 handle by design; row count derives from the u32-bounded type arena"
        )]
        let id = MirLayoutId(self.layouts.len() as u32);
        self.layouts.push(MirLayout {
            id,
            kind,
            size_bits: kind.size_bits(),
            align_bits: kind.align_bits(),
        });
        id
    }
}

impl MirLayoutKind {
    /// The scalar carrier of a fixed-size kind, or `None` for the unsized
    /// kinds: `Magnus` is a heap bignum, aggregates/views/vectors/matrices and
    /// opaque handles have no fixed size, packed numerics are not a scalar,
    /// and void/never carry no value.
    fn fixed_scalar(self) -> Option<MirScalarLayout> {
        match self {
            Self::Scalar(MirScalarLayout::Magnus)
            | Self::Aggregate(_)
            | Self::DeviceView(_)
            | Self::DeviceVector(_)
            | Self::DeviceMatrix(_)
            | Self::OpaqueHandle
            | Self::PackedNumeric(_)
            | Self::Void
            | Self::Never => None,
            Self::Scalar(scalar) => Some(scalar),
            Self::Atomic(atomic) => Some(atomic.element),
        }
    }

    fn size_bits(self) -> Option<u16> {
        self.fixed_scalar().map(MirScalarLayout::size_bits)
    }

    fn align_bits(self) -> Option<u16> {
        self.fixed_scalar().map(MirScalarLayout::align_bits)
    }
}

impl MirScalarLayout {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::I128 => "i128",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::F16 => "f16",
            Self::Bf16 => "bf16",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Magnus => "magnus",
            Self::D64 => "d64",
        }
    }

    #[must_use]
    pub fn size_bits(self) -> u16 {
        match self {
            Self::Bool => 1,
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 | Self::F16 | Self::Bf16 => 16,
            Self::I32 | Self::U32 | Self::F32 => 32,
            Self::I64 | Self::U64 | Self::F64 | Self::D64 => 64,
            Self::I128 => 128,
            // Unsized heap carrier: no fixed width. Reported as 0 so a device
            // consumer can never mistake it for a sized element; every
            // device and native gate rejects `Magnus` before reading this
            // (ruling 39), and `MirLayoutKind::size_bits` is `None`.
            Self::Magnus => 0,
        }
    }

    #[must_use]
    pub fn align_bits(self) -> u16 {
        self.size_bits()
    }

    #[must_use]
    pub fn is_integer(self) -> bool {
        self.integer_bits().is_some()
    }

    #[must_use]
    pub fn is_signed_integer(self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::I128
        )
    }

    #[must_use]
    pub fn is_unsigned_integer(self) -> bool {
        matches!(self, Self::U8 | Self::U16 | Self::U32 | Self::U64)
    }

    #[must_use]
    pub fn is_float(self) -> bool {
        matches!(self, Self::F16 | Self::Bf16 | Self::F32 | Self::F64)
    }

    #[must_use]
    pub fn integer_bits(self) -> Option<u16> {
        match self {
            Self::I8 | Self::U8 => Some(8),
            Self::I16 | Self::U16 => Some(16),
            Self::I32 | Self::U32 => Some(32),
            Self::I64 | Self::U64 => Some(64),
            Self::I128 => Some(128),
            // `Magnus` is an integer of no fixed width: not a sized integer.
            Self::Bool
            | Self::F16
            | Self::Bf16
            | Self::F32
            | Self::F64
            | Self::Magnus
            | Self::D64 => None,
        }
    }

    #[must_use]
    pub fn represents_integer(self, source: Self) -> bool {
        let Some(target_bits) = self.integer_bits() else {
            return false;
        };
        let Some(source_bits) = source.integer_bits() else {
            return false;
        };
        target_bits >= source_bits
            && (self.is_signed_integer() == source.is_signed_integer()
                || (self.is_signed_integer() && target_bits > source_bits))
    }
}

fn layout_kind_for_type(
    types: &TypeTable,
    ty: TypeId,
    memo: &mut FxHashMap<TypeId, LayoutResolution>,
    visiting: &mut FxHashSet<TypeId>,
) -> LayoutResolution {
    if let Some(cached) = memo.get(&ty) {
        return cached.clone();
    }
    // Alias-cycle guard: path-local membership; remove after the recursive walk.
    if !visiting.insert(ty) {
        memo.insert(ty, LayoutResolution::NoLayout);
        return LayoutResolution::NoLayout;
    }
    let kind = match types.get(ty) {
        Type::Primitive(Primitive::Bivalens) => {
            LayoutResolution::Resolved(MirLayoutKind::Scalar(MirScalarLayout::Bool))
        }
        Type::Primitive(Primitive::Numerus) => {
            LayoutResolution::Resolved(MirLayoutKind::Scalar(MirScalarLayout::I64))
        }
        Type::Primitive(Primitive::Fractus) => {
            LayoutResolution::Resolved(MirLayoutKind::Scalar(MirScalarLayout::F64))
        }
        // Fallthrough law: every sized-numeric combination resolves through a
        // named arm or errors loudly; nothing reaches `OpaqueHandle` silently.
        Type::SizedNumeric(prim, width) => sized_numeric_layout(*prim, *width),
        Type::ModularWord(width) | Type::SaturatingWord(width) => int_layout(*width),
        // A ranged integer is stored in its width; the range is a store rule.
        Type::RangedInt { width, .. } => int_layout(*width),
        // A bN is an unsigned N-bit scalar with the size and alignment of uN.
        Type::Bits(width) => bits_layout(*width),
        // N7E-3 (D2): a policy-float cell has the layout of the bare float of
        // its width; the policy only matters where a value is stored.
        Type::PolicyFloat(_, width) => sized_numeric_layout(Primitive::Fractus, *width),
        Type::TensorEdgePolicy(_) => LayoutResolution::MissingFact(
            "tensor edge-policy marker has no runtime layout".to_owned(),
            vec![("reason", "edge_policy_marker_no_layout".to_owned())],
        ),
        Type::Primitive(Primitive::Vacuum) => LayoutResolution::Resolved(MirLayoutKind::Void),
        Type::Primitive(Primitive::Numquam) => LayoutResolution::Resolved(MirLayoutKind::Never),
        Type::Alias(_, target) => layout_kind_for_type(types, *target, memo, visiting),
        Type::Param(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::MarkerHole { .. }
        | Type::Error
        | Type::Intersection(_) => LayoutResolution::NoLayout,
        Type::Array(_) => LayoutResolution::Resolved(opaque_aggregate(MirAggregateFamily::Array)),
        // A length range is a store rule over a heap list: the carrier is the
        // plain array, and the range is checked where the list is stored.
        Type::RangedArray { .. } => {
            LayoutResolution::Resolved(opaque_aggregate(MirAggregateFamily::Array))
        }
        // A ledger is a map underneath; its own carrier lands with the ledger
        // operations (need, L2+).
        Type::Map(_, _) | Type::Ledger(_, _) => {
            LayoutResolution::Resolved(opaque_aggregate(MirAggregateFamily::Map))
        }
        Type::Record(_) => {
            LayoutResolution::Resolved(physical_aggregate(MirAggregateFamily::Record))
        }
        Type::Set(_) => LayoutResolution::Resolved(opaque_aggregate(MirAggregateFamily::Set)),
        Type::Tensor(element, shape) => tensor_layout_kind(types, *element, *shape, memo, visiting),
        Type::Sparsa(element, figura) => {
            sparsa_layout_kind(types, *element, *figura, memo, visiting)
        }
        Type::Vector(element, width) => vector_layout_kind(types, *element, *width, memo, visiting),
        Type::Matrix(element, shape) => matrix_layout_kind(types, *element, *shape, memo, visiting),
        Type::Atomic(element) => atomic_layout_kind(types, *element, memo, visiting),
        Type::BoundedOcteti { capacity } => bounded_octeti_layout_kind(types, *capacity),
        Type::BoundedArray { element, capacity } => {
            bounded_array_layout_kind(types, *element, *capacity, memo, visiting)
        }
        Type::BoundedTextus { capacity } => {
            bounded_text_layout_kind(MirAggregateFamily::BoundedTextus, types, *capacity)
        }
        Type::BoundedAscii { capacity } => {
            bounded_text_layout_kind(MirAggregateFamily::BoundedAscii, types, *capacity)
        }
        Type::Option(_) => LayoutResolution::Resolved(opaque_aggregate(MirAggregateFamily::Option)),
        Type::Struct(_) => {
            LayoutResolution::Resolved(physical_aggregate(MirAggregateFamily::Struct))
        }
        Type::Enum(_) => LayoutResolution::Resolved(opaque_aggregate(MirAggregateFamily::Enum)),
        Type::Union(_) => LayoutResolution::Resolved(opaque_aggregate(MirAggregateFamily::Union)),
        Type::Tuple(_) => LayoutResolution::Resolved(physical_aggregate(MirAggregateFamily::Tuple)),
        Type::Primitive(
            Primitive::Ascii
            | Primitive::Littera
            | Primitive::Textus
            | Primitive::Nihil
            | Primitive::Ignotum
            | Primitive::Octeti
            | Primitive::Regex
            | Primitive::Match
            | Primitive::Json
            | Primitive::Valor
            | Primitive::Instans,
        )
        | Type::SizedInstans(_)
        | Type::Promissum(_)
        | Type::PromissumFailable(_, _)
        | Type::AsyncCursor(_, _)
        | Type::Cursor(_)
        | Type::Queue(_)
        | Type::Stack(_)
        | Type::Intervallum(..)
        | Type::Ref(_, _)
        | Type::Interface(_)
        | Type::Func(_)
        | Type::Applied(_, _)
        | Type::BoundedQueue { .. }
        | Type::BoundedStack { .. } => LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle),
    };
    visiting.remove(&ty);
    memo.insert(ty, kind.clone());
    kind
}

fn atomic_layout_kind(
    types: &TypeTable,
    element_ty: TypeId,
    memo: &mut FxHashMap<TypeId, LayoutResolution>,
    visiting: &mut FxHashSet<TypeId>,
) -> LayoutResolution {
    if !radix_types::is_atomic_element_type(types, element_ty) {
        return LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle);
    }
    layout_kind_for_type(types, element_ty, memo, visiting).then(|element_layout| {
        let MirLayoutKind::Scalar(scalar_element) = element_layout else {
            return LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle);
        };
        LayoutResolution::Resolved(MirLayoutKind::Atomic(MirAtomicLayout {
            element: scalar_element,
        }))
    })
}

fn opaque_aggregate(family: MirAggregateFamily) -> MirLayoutKind {
    MirLayoutKind::Aggregate(MirAggregateLayout {
        family,
        carrier: MirAggregateCarrier::OpaqueRuntimeHandle,
        element: None,
        capacity: None,
    })
}

fn physical_aggregate(family: MirAggregateFamily) -> MirLayoutKind {
    MirLayoutKind::Aggregate(MirAggregateLayout {
        family,
        carrier: MirAggregateCarrier::PhysicalValue,
        element: None,
        capacity: None,
    })
}

/// MLC-3: `BoundedArray<T, N>` — a fixed-capacity row of `T` is a physical
/// aggregate. The element fact records `T` when it is itself scalar
/// (`Some(s)`); a non-scalar member keeps the physical row with no scalar
/// element fact. A capacity that is not a compile-time literal has no
/// provable bounds and stays opaque (same law as `octeti<N>`).
fn bounded_array_layout_kind(
    types: &TypeTable,
    element_ty: TypeId,
    capacity: radix_types::IndexId,
    memo: &mut FxHashMap<TypeId, LayoutResolution>,
    visiting: &mut FxHashSet<TypeId>,
) -> LayoutResolution {
    let Some(length) = literal_capacity(types, capacity) else {
        return LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle);
    };
    layout_kind_for_type(types, element_ty, memo, visiting).then(|element_layout| {
        let element = match element_layout {
            MirLayoutKind::Scalar(scalar) => Some(scalar),
            _ => None,
        };
        LayoutResolution::Resolved(MirLayoutKind::Aggregate(MirAggregateLayout {
            family: MirAggregateFamily::BoundedArray,
            carrier: MirAggregateCarrier::PhysicalValue,
            element,
            capacity: Some(length),
        }))
    })
}

/// MLC-3: `ascii<N>` / `textus<N>` — a physical byte aggregate: `u8 × N`
/// storage plus a runtime length member. Non-literal capacities stay opaque.
fn bounded_text_layout_kind(
    family: MirAggregateFamily,
    types: &TypeTable,
    capacity: radix_types::IndexId,
) -> LayoutResolution {
    let Some(length) = literal_capacity(types, capacity) else {
        return LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle);
    };
    LayoutResolution::Resolved(MirLayoutKind::Aggregate(MirAggregateLayout {
        family,
        carrier: MirAggregateCarrier::PhysicalValue,
        element: Some(MirScalarLayout::U8),
        capacity: Some(length),
    }))
}

fn literal_capacity(types: &TypeTable, capacity: radix_types::IndexId) -> Option<u64> {
    match types.get_index(capacity) {
        IndexExpr::Literal(length) => Some(*length),
        _ => None,
    }
}

fn vector_layout_kind(
    types: &TypeTable,
    element_ty: TypeId,
    width: IndexId,
    memo: &mut FxHashMap<TypeId, LayoutResolution>,
    visiting: &mut FxHashSet<TypeId>,
) -> LayoutResolution {
    layout_kind_for_type(types, element_ty, memo, visiting).then(|element_layout| {
        let MirLayoutKind::Scalar(scalar_element) = element_layout else {
            return LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle);
        };
        let Some(width) = crate::vector_width::literal_vector_width(width, types) else {
            return LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle);
        };
        LayoutResolution::Resolved(MirLayoutKind::DeviceVector(MirDeviceVectorLayout {
            element: scalar_element,
            width,
            align: device_vector_align(scalar_element, width),
        }))
    })
}

fn matrix_layout_kind(
    types: &TypeTable,
    element_ty: TypeId,
    shape: IndexId,
    memo: &mut FxHashMap<TypeId, LayoutResolution>,
    visiting: &mut FxHashSet<TypeId>,
) -> LayoutResolution {
    layout_kind_for_type(types, element_ty, memo, visiting).then(|element_layout| {
        let MirLayoutKind::Scalar(scalar_element) = element_layout else {
            return LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle);
        };
        let Some((rows, columns)) = crate::matrix_shape::literal_matrix_shape(shape, types) else {
            return LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle);
        };
        LayoutResolution::Resolved(MirLayoutKind::DeviceMatrix(MirDeviceMatrixLayout {
            element: scalar_element,
            rows,
            columns,
            align: device_matrix_align(scalar_element, rows),
        }))
    })
}

fn device_vector_align(element: MirScalarLayout, width: u8) -> u8 {
    // WHY: vec3 uses 16-byte register alignment on GPU targets even though only 12 bytes are occupied.
    if width == 3 {
        16
    } else {
        // Scalar sizes are {1, 8, 16, 32, 64, 128} bits (exhaustive match above), so /8 <= 16 < 2^8.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirScalarLayout::size_bits is one of {1,8,16,32,64,128}, so /8 <= 16 < u8::MAX"
        )]
        let elem_bytes = if element == MirScalarLayout::Bool {
            1
        } else {
            (element.size_bits() / 8) as u8
        };
        elem_bytes.saturating_mul(width)
    }
}

fn device_matrix_align(element: MirScalarLayout, rows: u8) -> u8 {
    device_vector_align(element, rows)
}

/// MLC-4: `Sparsa<T, Figura>` — a scalar `T` with a resolvable figura rank
/// resolves to a host-locus `DeviceView` row carrying the sparse storage
/// marker. A non-scalar element or an unresolvable figura has no provable
/// physical facts and stays opaque (same law as `Tensor`).
fn sparsa_layout_kind(
    types: &TypeTable,
    element: TypeId,
    figura: radix_types::IndexId,
    memo: &mut FxHashMap<TypeId, LayoutResolution>,
    visiting: &mut FxHashSet<TypeId>,
) -> LayoutResolution {
    layout_kind_for_type(types, element, memo, visiting).then(|element| {
        let MirLayoutKind::Scalar(element) = element else {
            return LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle);
        };
        let Some(rank) = tensor_rank(types, figura) else {
            return LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle);
        };
        LayoutResolution::Resolved(MirLayoutKind::DeviceView(
            MirDeviceViewLayout::sparse_row_major(element, rank),
        ))
    })
}

fn tensor_layout_kind(
    types: &TypeTable,
    element: TypeId,
    shape: radix_types::IndexId,
    memo: &mut FxHashMap<TypeId, LayoutResolution>,
    visiting: &mut FxHashSet<TypeId>,
) -> LayoutResolution {
    let element_ty = element;
    layout_kind_for_type(types, element, memo, visiting).then(|element| {
        let element = match element {
            MirLayoutKind::Scalar(scalar) => MirDeviceViewElement::Scalar(scalar),
            MirLayoutKind::DeviceVector(vector) => {
                // WHY: width 3 has size 12 but stride/alignment 16 (MSL `float3`);
                // size/stride conflation is a known trap, so only widths whose
                // size equals their alignment are admitted.
                if !matches!(vector.width, 2 | 4) {
                    return LayoutResolution::MissingFact(
                        format!(
                            "tensor of vector<_, {}> has no device-view layout: only widths 2 and 4 \
                             (size == alignment) are admitted; vec3 stride/packing is undecided",
                            vector.width
                        ),
                        vec![
                            ("reason", "vector_width_unsupported".to_owned()),
                            ("width", vector.width.to_string()),
                        ],
                    );
                }
                MirDeviceViewElement::Vector(vector)
            }
            // The layout table cannot resolve a genus record's fields, so the
            // element carries its type id; consumers resolve the byte size,
            // alignment and field offsets with the validation context.
            MirLayoutKind::Aggregate(aggregate)
                if aggregate.family == MirAggregateFamily::Struct
                    && aggregate.carrier == MirAggregateCarrier::PhysicalValue =>
            {
                MirDeviceViewElement::Record(element_ty)
            }
            _ => {
                return LayoutResolution::Resolved(opaque_aggregate(
                    MirAggregateFamily::TensorRuntime,
                ));
            }
        };
        let Some(rank) = tensor_rank(types, shape) else {
            return LayoutResolution::Resolved(opaque_aggregate(MirAggregateFamily::TensorRuntime));
        };
        let mut view = MirDeviceViewLayout::materialized_row_major(MirScalarLayout::F32, rank);
        view.element = element;
        LayoutResolution::Resolved(MirLayoutKind::DeviceView(view))
    })
}

/// `octeti<N>` — a statically-bounded byte buffer is a rank-1 `u8` device
/// view with its static length. A capacity that is not a compile-time
/// literal has no provable bounds and stays opaque.
fn bounded_octeti_layout_kind(
    types: &TypeTable,
    capacity: radix_types::IndexId,
) -> LayoutResolution {
    match types.get_index(capacity) {
        IndexExpr::Literal(length) => LayoutResolution::Resolved(MirLayoutKind::DeviceView(
            MirDeviceViewLayout::static_byte_buffer(*length),
        )),
        _ => LayoutResolution::Resolved(MirLayoutKind::OpaqueHandle),
    }
}

impl MirDeviceViewLayout {
    #[must_use]
    pub fn materialized_row_major(element: MirScalarLayout, rank: u8) -> Self {
        Self {
            element: MirDeviceViewElement::Scalar(element),
            rank,
            storage: MirTensorStorageLayout::HostOwned,
            locus: MirTensorLocusLayout::Host,
            view: MirTensorViewLayout::Materialized,
            offset_elements: 0,
            contiguous: true,
            row_major: true,
            stride: MirTensorStrideLayout::RowMajor,
            length: None,
        }
    }

    /// `octeti<N>` byte view: host-staged per the existing tensor facts,
    /// with the static capacity carried as the view length.
    #[must_use]
    pub fn static_byte_buffer(length: u64) -> Self {
        Self {
            length: Some(length),
            ..Self::materialized_row_major(MirScalarLayout::U8, 1)
        }
    }

    /// MLC-4: `sparsa` row — host-locus materialized view with the sparse
    /// storage marker. Layout fact only; kernel admission rejects it by name.
    #[must_use]
    pub fn sparse_row_major(element: MirScalarLayout, rank: u8) -> Self {
        Self {
            storage: MirTensorStorageLayout::Sparse,
            ..Self::materialized_row_major(element, rank)
        }
    }

    #[must_use]
    pub fn device_handle_row_major(element: MirScalarLayout, rank: u8) -> Self {
        Self {
            element: MirDeviceViewElement::Scalar(element),
            rank,
            storage: MirTensorStorageLayout::DeviceHandle,
            locus: MirTensorLocusLayout::Device,
            view: MirTensorViewLayout::Materialized,
            offset_elements: 0,
            contiguous: true,
            row_major: true,
            stride: MirTensorStrideLayout::RowMajor,
            length: None,
        }
    }

    #[must_use]
    pub fn axis0_view(
        element: MirScalarLayout,
        rank: u8,
        offset_elements: u64,
        stride_elements: u64,
    ) -> Self {
        Self {
            element: MirDeviceViewElement::Scalar(element),
            rank,
            storage: MirTensorStorageLayout::HostOwned,
            locus: MirTensorLocusLayout::Host,
            view: MirTensorViewLayout::Axis0View,
            offset_elements,
            contiguous: false,
            row_major: true,
            stride: MirTensorStrideLayout::Axis0 { stride_elements },
            length: None,
        }
    }
}

fn tensor_rank(types: &TypeTable, shape: radix_types::IndexId) -> Option<u8> {
    let rank = match types.get_index(shape) {
        IndexExpr::Literal(_)
        | IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Affine { .. }
        | IndexExpr::Divide { .. } => 1,
        IndexExpr::Tuple(dims) => dims.len(),
        IndexExpr::Infer(_) | IndexExpr::Unspecified => return None,
    };
    u8::try_from(rank).ok()
}

/// Named arms for the sized-numeric family — the fallthrough law.
///
/// Every `(family, width)` combination resolves through a named arm or fails
/// loudly with the width named; nothing reaches `OpaqueHandle` by fallthrough.
/// `fractus<bf16>` is the loud pending-error row until bf16 owns
/// `MirScalarLayout::BF16` (bf16-width-admission U2); a new width lands a row
/// here or the pin-table test fails first.
fn sized_numeric_layout(prim: Primitive, width: NumericWidth) -> LayoutResolution {
    let scalar = match (prim, width) {
        (Primitive::Numerus, NumericWidth::I8) => MirScalarLayout::I8,
        (Primitive::Numerus, NumericWidth::I16) => MirScalarLayout::I16,
        (Primitive::Numerus, NumericWidth::I32) => MirScalarLayout::I32,
        (Primitive::Numerus, NumericWidth::I64) => MirScalarLayout::I64,
        (Primitive::Numerus, NumericWidth::U8) => MirScalarLayout::U8,
        (Primitive::Numerus, NumericWidth::U16) => MirScalarLayout::U16,
        (Primitive::Numerus, NumericWidth::U32) => MirScalarLayout::U32,
        (Primitive::Numerus, NumericWidth::U64) => MirScalarLayout::U64,
        (Primitive::Fractus, NumericWidth::F16) => MirScalarLayout::F16,
        (Primitive::Fractus, NumericWidth::BF16) => MirScalarLayout::Bf16,
        (Primitive::Fractus, NumericWidth::F32) => MirScalarLayout::F32,
        (Primitive::Fractus, NumericWidth::F64) => MirScalarLayout::F64,
        // Ruling 46: `inf` is a heap-resident bignum (`Magnus`), an opaque
        // scalar layout no native or device emitter may read as an integer.
        (Primitive::Numerus, NumericWidth::Inf) => MirScalarLayout::Magnus,
        // Kernel-wide types ruling 2026-10-28: a `d64` cell is an `i64`
        // mantissa at scale 10^-8.
        (Primitive::Numerus, NumericWidth::D64) => MirScalarLayout::D64,
        (prim, width) => {
            let family = match prim {
                Primitive::Numerus => "numerus",
                Primitive::Fractus => "fractus",
                _ => "sized-numeric",
            };
            return LayoutResolution::MissingFact(
                format!("{family}<{}> has no named layout arm", width.faber_marker()),
                vec![
                    ("reason", "no_named_layout_arm".to_owned()),
                    ("family", family.to_owned()),
                    ("width", width.faber_marker().to_owned()),
                ],
            );
        }
    };
    LayoutResolution::Resolved(MirLayoutKind::Scalar(scalar))
}

fn bits_layout(width: BitsWidth) -> LayoutResolution {
    LayoutResolution::Resolved(MirLayoutKind::Scalar(match width {
        BitsWidth::B8 => MirScalarLayout::U8,
        BitsWidth::B16 => MirScalarLayout::U16,
        BitsWidth::B32 => MirScalarLayout::U32,
        BitsWidth::B64 => MirScalarLayout::U64,
    }))
}

fn int_layout(width: NumericWidth) -> LayoutResolution {
    let scalar = match width {
        NumericWidth::I8 => MirScalarLayout::I8,
        NumericWidth::I16 => MirScalarLayout::I16,
        NumericWidth::I32 => MirScalarLayout::I32,
        NumericWidth::I64 => MirScalarLayout::I64,
        NumericWidth::U8 => MirScalarLayout::U8,
        NumericWidth::U16 => MirScalarLayout::U16,
        NumericWidth::U32 => MirScalarLayout::U32,
        NumericWidth::U64 => MirScalarLayout::U64,
        NumericWidth::F16
        | NumericWidth::BF16
        | NumericWidth::F32
        | NumericWidth::F64
        | NumericWidth::D64 => {
            return LayoutResolution::MissingFact(
                format!("modulus<{}> has no named layout arm", width.faber_marker()),
                vec![
                    ("reason", "no_named_layout_arm".to_owned()),
                    ("family", "modulus".to_owned()),
                    ("width", width.faber_marker().to_owned()),
                ],
            );
        }
        // A word over `inf` is the bare `inf` type (ruling 32): the same
        // `Magnus` layout, never a fixed-width word.
        NumericWidth::Inf => MirScalarLayout::Magnus,
    };
    LayoutResolution::Resolved(MirLayoutKind::Scalar(scalar))
}
