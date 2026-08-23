//! Shared physical layout facts for MIR-backed systems targets.
//!
//! TARGET: low-level MIR consumers that need representation facts before
//! emitting target text. The table is intentionally target-neutral: it says
//! what the compiler knows about a value's physical carrier, not how any one
//! backend spells that carrier.

use crate::{MirLayoutId, MirType};
use radix_types::{IndexExpr, IndexId, NumericWidth, Primitive, Type, TypeId, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    F32,
    F64,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirDeviceViewLayout {
    pub element: MirScalarLayout,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        let elem = metal_element_ty(self.element)?;
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
        MirScalarLayout::I8 => Ok("i32"),
        MirScalarLayout::I16 => Ok("i32"),
        MirScalarLayout::I32 => Ok("i32"),
        MirScalarLayout::U8 => Ok("u32"),
        MirScalarLayout::U16 => Ok("u32"),
        MirScalarLayout::U32 => Ok("u32"),
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

fn metal_element_ty(element: MirScalarLayout) -> Result<&'static str, String> {
    match element {
        MirScalarLayout::Bool => Ok("bool"),
        MirScalarLayout::I8 | MirScalarLayout::I16 | MirScalarLayout::I32 => Ok("int"),
        MirScalarLayout::U8 | MirScalarLayout::U16 | MirScalarLayout::U32 => Ok("uint"),
        MirScalarLayout::F32 => Ok("float"),
        other => Err(format!("kernel vector element {}", other.spelling())),
    }
}

fn llvm_element_ty(element: MirScalarLayout) -> Result<&'static str, String> {
    match element {
        MirScalarLayout::Bool => Ok("i1"),
        MirScalarLayout::I8 => Ok("i8"),
        MirScalarLayout::I16 => Ok("i16"),
        MirScalarLayout::I32 => Ok("i32"),
        MirScalarLayout::I64 => Ok("i64"),
        MirScalarLayout::U8 => Ok("i8"),
        MirScalarLayout::U16 => Ok("i16"),
        MirScalarLayout::U32 => Ok("i32"),
        MirScalarLayout::U64 => Ok("i64"),
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirLayoutTable {
    layouts: Vec<MirLayout>,
    by_type: FxHashMap<TypeId, MirLayoutId>,
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
            if let Some(kind) = layout_kind_for_type(types, ty, &mut memo, &mut visiting) {
                table.insert_type_layout(ty, kind);
            }
        }
        table
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirLayoutError`].
    pub fn layout_for_type(&self, ty: MirType) -> Result<&MirLayout, MirLayoutError> {
        let id = ty
            .layout_id()
            .or_else(|| self.by_type.get(&ty.semantic_id()).copied())
            .ok_or_else(|| MirLayoutError {
                message: format!("missing layout fact for ty#{}", ty.semantic_id().0),
                issue: "mir_layout_missing_type_fact",
                id: None,
                ty: Some(ty.semantic_id()),
            })?;
        self.layout(id).ok_or_else(|| MirLayoutError {
            message: format!("missing layout row #{}", id.0),
            issue: "mir_layout_missing_row",
            id: Some(id),
            ty: None,
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

    fn insert_type_layout(&mut self, ty: TypeId, kind: MirLayoutKind) -> MirLayoutId {
        self.set_type_layout(ty, kind)
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
    fn size_bits(self) -> Option<u16> {
        match self {
            Self::Scalar(scalar) => Some(scalar.size_bits()),
            Self::Atomic(atomic) => Some(atomic.element.size_bits()),
            Self::Aggregate(_)
            | Self::DeviceView(_)
            | Self::DeviceVector(_)
            | Self::DeviceMatrix(_)
            | Self::OpaqueHandle
            | Self::PackedNumeric(_)
            | Self::Void
            | Self::Never => None,
        }
    }

    fn align_bits(self) -> Option<u16> {
        match self {
            Self::Scalar(scalar) => Some(scalar.align_bits()),
            Self::Atomic(atomic) => Some(atomic.element.align_bits()),
            Self::Aggregate(_)
            | Self::DeviceView(_)
            | Self::DeviceVector(_)
            | Self::DeviceMatrix(_)
            | Self::OpaqueHandle
            | Self::PackedNumeric(_)
            | Self::Void
            | Self::Never => None,
        }
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
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }

    #[must_use]
    pub fn size_bits(self) -> u16 {
        match self {
            Self::Bool => 1,
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 | Self::F16 => 16,
            Self::I32 | Self::U32 | Self::F32 => 32,
            Self::I64 | Self::U64 | Self::F64 => 64,
            Self::I128 => 128,
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
        matches!(self, Self::F16 | Self::F32 | Self::F64)
    }

    #[must_use]
    pub fn integer_bits(self) -> Option<u16> {
        match self {
            Self::I8 | Self::U8 => Some(8),
            Self::I16 | Self::U16 => Some(16),
            Self::I32 | Self::U32 => Some(32),
            Self::I64 | Self::U64 => Some(64),
            Self::I128 => Some(128),
            Self::Bool | Self::F16 | Self::F32 | Self::F64 => None,
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
    memo: &mut FxHashMap<TypeId, Option<MirLayoutKind>>,
    visiting: &mut FxHashSet<TypeId>,
) -> Option<MirLayoutKind> {
    if let Some(cached) = memo.get(&ty) {
        return *cached;
    }
    // Alias-cycle guard: path-local membership; remove after the recursive walk.
    if !visiting.insert(ty) {
        memo.insert(ty, None);
        return None;
    }
    let kind = match types.get(ty) {
        Type::Primitive(Primitive::Bivalens) => Some(MirLayoutKind::Scalar(MirScalarLayout::Bool)),
        Type::Primitive(Primitive::Numerus) => Some(MirLayoutKind::Scalar(MirScalarLayout::I64)),
        Type::Primitive(Primitive::Fractus) => Some(MirLayoutKind::Scalar(MirScalarLayout::F64)),
        Type::SizedNumeric(Primitive::Numerus, width) => int_layout(*width),
        Type::ModularWord(width) => int_layout(*width),
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F16) => {
            Some(MirLayoutKind::Scalar(MirScalarLayout::F16))
        }
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32) => {
            Some(MirLayoutKind::Scalar(MirScalarLayout::F32))
        }
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64) => {
            Some(MirLayoutKind::Scalar(MirScalarLayout::F64))
        }
        Type::Primitive(Primitive::Vacuum) => Some(MirLayoutKind::Void),
        Type::Primitive(Primitive::Numquam) => Some(MirLayoutKind::Never),
        Type::Alias(_, target) => layout_kind_for_type(types, *target, memo, visiting),
        Type::Param(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::MarkerHole { .. }
        | Type::Error => None,
        Type::SizedNumeric(_, _) => Some(MirLayoutKind::OpaqueHandle),
        Type::Array(_) => Some(opaque_aggregate(MirAggregateFamily::Array)),
        Type::Map(_, _) => Some(opaque_aggregate(MirAggregateFamily::Map)),
        Type::Record(_) => Some(physical_aggregate(MirAggregateFamily::Record)),
        Type::Set(_) => Some(opaque_aggregate(MirAggregateFamily::Set)),
        Type::Tensor(element, shape) => tensor_layout_kind(types, *element, *shape, memo, visiting),
        Type::Vector(element, width) => vector_layout_kind(types, *element, *width, memo, visiting),
        Type::Matrix(element, shape) => matrix_layout_kind(types, *element, *shape, memo, visiting),
        Type::Atomic(element) => atomic_layout_kind(types, *element, memo, visiting),
        Type::BoundedOcteti { capacity } => bounded_octeti_layout_kind(types, *capacity),
        Type::Option(_) => Some(opaque_aggregate(MirAggregateFamily::Option)),
        Type::Struct(_) => Some(physical_aggregate(MirAggregateFamily::Struct)),
        Type::Enum(_) => Some(opaque_aggregate(MirAggregateFamily::Enum)),
        Type::Union(_) => Some(opaque_aggregate(MirAggregateFamily::Union)),
        Type::Tuple(_) => Some(physical_aggregate(MirAggregateFamily::Tuple)),
        Type::Primitive(
            Primitive::Ascii
            | Primitive::Textus
            | Primitive::Nihil
            | Primitive::Ignotum
            | Primitive::Octeti
            | Primitive::Regex
            | Primitive::Json
            | Primitive::Valor
            | Primitive::Instans,
        )
        | Type::SizedInstans(_)
        | Type::Promissum(_)
        | Type::PromissumFailable(_, _)
        | Type::AsyncCursor(_, _)
        | Type::Cursor(_)
        | Type::Sparsa(_, _)
        | Type::Intervallum(_)
        | Type::Ref(_, _)
        | Type::Interface(_)
        | Type::Func(_)
        | Type::Applied(_, _)
        | Type::BoundedArray { .. }
        | Type::BoundedTextus { .. }
        | Type::BoundedAscii { .. } => Some(MirLayoutKind::OpaqueHandle),
    };
    visiting.remove(&ty);
    memo.insert(ty, kind);
    kind
}

fn atomic_layout_kind(
    types: &TypeTable,
    element_ty: TypeId,
    memo: &mut FxHashMap<TypeId, Option<MirLayoutKind>>,
    visiting: &mut FxHashSet<TypeId>,
) -> Option<MirLayoutKind> {
    if !radix_types::is_atomic_element_type(types, element_ty) {
        return Some(MirLayoutKind::OpaqueHandle);
    }
    let element_layout = layout_kind_for_type(types, element_ty, memo, visiting)?;
    let MirLayoutKind::Scalar(scalar_element) = element_layout else {
        return Some(MirLayoutKind::OpaqueHandle);
    };
    Some(MirLayoutKind::Atomic(MirAtomicLayout {
        element: scalar_element,
    }))
}

fn opaque_aggregate(family: MirAggregateFamily) -> MirLayoutKind {
    MirLayoutKind::Aggregate(MirAggregateLayout {
        family,
        carrier: MirAggregateCarrier::OpaqueRuntimeHandle,
    })
}

fn physical_aggregate(family: MirAggregateFamily) -> MirLayoutKind {
    MirLayoutKind::Aggregate(MirAggregateLayout {
        family,
        carrier: MirAggregateCarrier::PhysicalValue,
    })
}

fn vector_layout_kind(
    types: &TypeTable,
    element_ty: TypeId,
    width: IndexId,
    memo: &mut FxHashMap<TypeId, Option<MirLayoutKind>>,
    visiting: &mut FxHashSet<TypeId>,
) -> Option<MirLayoutKind> {
    let element_layout = layout_kind_for_type(types, element_ty, memo, visiting)?;
    let MirLayoutKind::Scalar(scalar_element) = element_layout else {
        return Some(MirLayoutKind::OpaqueHandle);
    };
    let Some(width) = crate::vector_width::literal_vector_width(width, types) else {
        return Some(MirLayoutKind::OpaqueHandle);
    };
    Some(MirLayoutKind::DeviceVector(MirDeviceVectorLayout {
        element: scalar_element,
        width,
        align: device_vector_align(scalar_element, width),
    }))
}

fn matrix_layout_kind(
    types: &TypeTable,
    element_ty: TypeId,
    shape: IndexId,
    memo: &mut FxHashMap<TypeId, Option<MirLayoutKind>>,
    visiting: &mut FxHashSet<TypeId>,
) -> Option<MirLayoutKind> {
    let element_layout = layout_kind_for_type(types, element_ty, memo, visiting)?;
    let MirLayoutKind::Scalar(scalar_element) = element_layout else {
        return Some(MirLayoutKind::OpaqueHandle);
    };
    let Some((rows, columns)) = crate::matrix_shape::literal_matrix_shape(shape, types) else {
        return Some(MirLayoutKind::OpaqueHandle);
    };
    Some(MirLayoutKind::DeviceMatrix(MirDeviceMatrixLayout {
        element: scalar_element,
        rows,
        columns,
        align: device_matrix_align(scalar_element, rows),
    }))
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
        let elem_bytes = (element.size_bits() / 8) as u8;
        elem_bytes.saturating_mul(width)
    }
}

fn device_matrix_align(element: MirScalarLayout, rows: u8) -> u8 {
    device_vector_align(element, rows)
}

fn tensor_layout_kind(
    types: &TypeTable,
    element: TypeId,
    shape: radix_types::IndexId,
    memo: &mut FxHashMap<TypeId, Option<MirLayoutKind>>,
    visiting: &mut FxHashSet<TypeId>,
) -> Option<MirLayoutKind> {
    let element_layout = layout_kind_for_type(types, element, memo, visiting)?;
    let MirLayoutKind::Scalar(element) = element_layout else {
        return Some(opaque_aggregate(MirAggregateFamily::TensorRuntime));
    };
    let Some(rank) = tensor_rank(types, shape) else {
        return Some(opaque_aggregate(MirAggregateFamily::TensorRuntime));
    };
    Some(MirLayoutKind::DeviceView(
        MirDeviceViewLayout::materialized_row_major(element, rank),
    ))
}

/// `octeti<N>` — a statically-bounded byte buffer is a rank-1 `u8` device
/// view with its static length. A capacity that is not a compile-time
/// literal has no provable bounds and stays opaque.
fn bounded_octeti_layout_kind(types: &TypeTable, capacity: radix_types::IndexId) -> Option<MirLayoutKind> {
    match types.get_index(capacity) {
        IndexExpr::Literal(length) => Some(MirLayoutKind::DeviceView(
            MirDeviceViewLayout::static_byte_buffer(*length),
        )),
        _ => Some(MirLayoutKind::OpaqueHandle),
    }
}

impl MirDeviceViewLayout {
    #[must_use]
    pub fn materialized_row_major(element: MirScalarLayout, rank: u8) -> Self {
        Self {
            element,
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

    #[must_use]
    pub fn device_handle_row_major(element: MirScalarLayout, rank: u8) -> Self {
        Self {
            element,
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
            element,
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
        IndexExpr::Literal(_) | IndexExpr::Param(_) => 1,
        IndexExpr::Tuple(dims) => dims.len(),
        IndexExpr::Infer(_) | IndexExpr::Unspecified => return None,
    };
    u8::try_from(rank).ok()
}

fn int_layout(width: NumericWidth) -> Option<MirLayoutKind> {
    let scalar = match width {
        NumericWidth::I8 => MirScalarLayout::I8,
        NumericWidth::I16 => MirScalarLayout::I16,
        NumericWidth::I32 => MirScalarLayout::I32,
        NumericWidth::I64 => MirScalarLayout::I64,
        NumericWidth::U8 => MirScalarLayout::U8,
        NumericWidth::U16 => MirScalarLayout::U16,
        NumericWidth::U32 => MirScalarLayout::U32,
        NumericWidth::U64 => MirScalarLayout::U64,
        NumericWidth::F16 | NumericWidth::F32 | NumericWidth::F64 => return None,
    };
    Some(MirLayoutKind::Scalar(scalar))
}
