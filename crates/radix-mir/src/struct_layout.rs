//! Genus kernel-parameter layout law (gpu-reset C4-6a, ruling (d)).
//!
//! TARGET: device kernels that receive a `genus` by value. The struct is laid
//! out as a C-style struct: fields in DECLARATION order (taken from
//! `MirValidationContext::struct_field_order`, never the unordered
//! `struct_fields` map and never interner `Symbol` order), each at the next
//! offset rounded up to its alignment. Scalars `i8`/`u8` are 1/1,
//! `i16`/`u16`/`f16`/`bf16` are 2/2, `f32`/`i32`/`u32` are 4/4, `int`
//! (`numerus`, i64) and `u64` are 8/8, `vector<_, size 2>` is 8/8, `vector<_, size 4>` is
//! 16/16. The struct alignment is the maximum field alignment and the struct
//! size is rounded up to it.
//!
//! `vector<_, size 3>` is a packed 12-byte, 4-aligned field; that arm is not
//! admitted yet and fails closed by name (`Vec3PackedNotYet`). The register
//! alignment that `layout.rs` reports for width 3 (16) is never read here.
//!
//! The device gate and the kernel ABI read it (C4-6b); no emitter or dispatcher
//! does yet.

use crate::{
    MirDeviceVectorLayout, MirDeviceViewElement, MirLayoutKind, MirScalarLayout, MirType,
    MirValidationContext,
};
use radix_lexer::Symbol;
use radix_types::{IndexExpr, IndexId, Primitive, Type, TypeId};
use serde::{Deserialize, Serialize};

/// Largest admitted struct, in bytes.
pub const MAX_STRUCT_LAYOUT_BYTES: u32 = 4096;

/// One field of a resolved struct layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirStructField {
    pub name: Symbol,
    pub offset: u32,
    pub size: u32,
    pub align: u32,
    pub element: MirDeviceViewElement,
}

/// A resolved genus parameter layout: fields in declaration order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirStructLayout {
    pub fields: Vec<MirStructField>,
    pub size: u32,
    pub align: u32,
}

/// Why a type has no struct layout. One variant per refusal row of ruling (d),
/// plus the structural refusals that keep the resolver total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructLayoutRefusal {
    /// The type is not a `genus` (`Type::Struct`).
    NotAGenus,
    /// The genus has no recorded field facts or a declared field has no type.
    UnknownFields,
    /// The genus is opaque (runtime-owned), not a plain data record.
    OpaqueGenus,
    /// A genus with no instance fields.
    EmptyGenus,
    Bool,
    F64,
    NestedGenus,
    /// A generic genus, a generic type parameter, or `Type::Applied`.
    Generic,
    Tuple,
    Record,
    List,
    Map,
    Set,
    Tensor,
    /// A `sponte` (optional) or nullable (`T ∪ none`) field.
    Nullable,
    Text,
    /// `vector<_, size 3>`: packed 12 bytes / align 4, not admitted yet.
    Vec3PackedNotYet,
    /// The struct exceeds [`MAX_STRUCT_LAYOUT_BYTES`].
    TooLarge,
    /// Any other field type with no admitted device layout.
    UnsupportedField,
}

impl StructLayoutRefusal {
    /// Stable machine-readable identifier for this refusal.
    #[must_use]
    pub fn identifier(self) -> &'static str {
        match self {
            Self::NotAGenus => "not_a_genus",
            Self::UnknownFields => "unknown_fields",
            Self::OpaqueGenus => "opaque_genus",
            Self::EmptyGenus => "empty_genus",
            Self::Bool => "bool",
            Self::F64 => "f64",
            Self::NestedGenus => "nested_genus",
            Self::Generic => "generic",
            Self::Tuple => "tuple",
            Self::Record => "record",
            Self::List => "list",
            Self::Map => "map",
            Self::Set => "set",
            Self::Tensor => "tensor",
            Self::Nullable => "nullable",
            Self::Text => "text",
            Self::Vec3PackedNotYet => "vec3_packed_not_yet",
            Self::TooLarge => "too_large",
            Self::UnsupportedField => "unsupported_field",
        }
    }

    /// Stable lowercase reason string.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::NotAGenus => "not a genus",
            Self::UnknownFields => "genus field facts are missing",
            Self::OpaqueGenus => "opaque genus",
            Self::EmptyGenus => "empty genus",
            Self::Bool => "bool field",
            Self::F64 => "f64 field",
            Self::NestedGenus => "nested genus field",
            Self::Generic => "generic field or genus",
            Self::Tuple => "tuple field",
            Self::Record => "record field",
            Self::List => "list field",
            Self::Map => "map field",
            Self::Set => "set field",
            Self::Tensor => "tensor field",
            Self::Nullable => "sponte or nullable field",
            Self::Text => "text field",
            Self::Vec3PackedNotYet => "vec3 field (packed layout not yet admitted)",
            Self::TooLarge => "struct larger than 4096 bytes",
            Self::UnsupportedField => "unsupported field type",
        }
    }
}

impl std::fmt::Display for StructLayoutRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason())
    }
}

impl std::error::Error for StructLayoutRefusal {}

/// Size, alignment and element of one admitted field type.
#[derive(Debug, Clone, Copy)]
struct FieldSpec {
    name: Symbol,
    size: u32,
    align: u32,
    element: MirDeviceViewElement,
}

/// Resolve the byte layout of the `genus` type `ty`.
///
/// # Errors
/// Returns the named refusal for the first field (in declaration order) or the
/// struct as a whole that the law does not admit.
pub fn resolve_struct_layout(
    ty: MirType,
    ctx: &MirValidationContext<'_>,
) -> Result<MirStructLayout, StructLayoutRefusal> {
    let types = ctx.types.as_table();
    let def_id = match types.get(ty.semantic_id()) {
        Type::Struct(def_id) => *def_id,
        Type::Applied(_, _) | Type::Param(_) => return Err(StructLayoutRefusal::Generic),
        _ => return Err(StructLayoutRefusal::NotAGenus),
    };
    if ctx.opaque_structs.contains(&def_id) {
        return Err(StructLayoutRefusal::OpaqueGenus);
    }
    if ctx
        .struct_type_params
        .get(&def_id)
        .is_some_and(|params| !params.is_empty())
    {
        return Err(StructLayoutRefusal::Generic);
    }
    let (Some(order), Some(fields)) = (
        ctx.struct_field_order.get(&def_id),
        ctx.struct_fields.get(&def_id),
    ) else {
        return Err(StructLayoutRefusal::UnknownFields);
    };
    if order.is_empty() {
        return Err(StructLayoutRefusal::EmptyGenus);
    }
    let mut specs = Vec::with_capacity(order.len());
    for name in order {
        let Some(field_ty) = fields.get(name) else {
            return Err(StructLayoutRefusal::UnknownFields);
        };
        let nullable = ctx
            .optional_struct_fields
            .get(&def_id)
            .is_some_and(|set| set.contains(name))
            || ctx
                .nullable_struct_fields
                .get(&def_id)
                .is_some_and(|set| set.contains(name));
        if nullable {
            return Err(StructLayoutRefusal::Nullable);
        }
        specs.push(field_spec(ctx, *name, field_ty.semantic_id())?);
    }
    pack_fields(&specs)
}

fn field_spec(
    ctx: &MirValidationContext<'_>,
    name: Symbol,
    ty: TypeId,
) -> Result<FieldSpec, StructLayoutRefusal> {
    let types = ctx.types.as_table();
    let mut ty = ty;
    while let Type::Alias(_, target) = types.get(ty) {
        ty = *target;
    }
    match types.get(ty) {
        Type::Struct(_) => return Err(StructLayoutRefusal::NestedGenus),
        Type::Applied(_, _) | Type::Param(_) => return Err(StructLayoutRefusal::Generic),
        Type::Tuple(_) => return Err(StructLayoutRefusal::Tuple),
        Type::Record(_) => return Err(StructLayoutRefusal::Record),
        Type::Array(_) | Type::RangedArray { .. } => return Err(StructLayoutRefusal::List),
        Type::BoundedArray { element, capacity } => {
            return bounded_array_spec(ctx, name, *element, *capacity);
        }
        Type::Map(_, _) | Type::Ledger(_, _) => return Err(StructLayoutRefusal::Map),
        Type::Set(_) => return Err(StructLayoutRefusal::Set),
        Type::Tensor(_, _) | Type::Sparsa(_, _) => return Err(StructLayoutRefusal::Tensor),
        Type::Option(_) => return Err(StructLayoutRefusal::Nullable),
        Type::BoundedTextus { .. }
        | Type::BoundedAscii { .. }
        | Type::Primitive(Primitive::Textus | Primitive::Littera | Primitive::Ascii) => {
            return Err(StructLayoutRefusal::Text);
        }
        _ => {}
    }
    let Ok(layout) = ctx.layouts.layout_for_type(MirType::semantic(ty)) else {
        return Err(StructLayoutRefusal::UnsupportedField);
    };
    match layout.kind {
        MirLayoutKind::Scalar(scalar) => {
            let (size, align) = scalar_size_align(scalar)?;
            Ok(FieldSpec {
                name,
                size,
                align,
                element: MirDeviceViewElement::Scalar(scalar),
            })
        }
        MirLayoutKind::DeviceVector(vector) => vector_spec(name, vector),
        _ => Err(StructLayoutRefusal::UnsupportedField),
    }
}

fn bounded_array_spec(
    ctx: &MirValidationContext<'_>,
    name: Symbol,
    element: TypeId,
    capacity: IndexId,
) -> Result<FieldSpec, StructLayoutRefusal> {
    let types = ctx.types.as_table();
    let IndexExpr::Literal(n) = types.get_index(capacity) else {
        return Err(StructLayoutRefusal::UnsupportedField);
    };
    let mut element = element;
    while let Type::Alias(_, target) = types.get(element) {
        element = *target;
    }
    let Ok(layout) = ctx.layouts.layout_for_type(MirType::semantic(element)) else {
        return Err(StructLayoutRefusal::UnsupportedField);
    };
    let MirLayoutKind::Scalar(scalar) = layout.kind else {
        return Err(StructLayoutRefusal::UnsupportedField);
    };
    let (size, align) = scalar_size_align(scalar)?;
    let Some(total) = u32::try_from(*n).ok().and_then(|n| size.checked_mul(n)) else {
        return Err(StructLayoutRefusal::UnsupportedField);
    };
    if total > MAX_STRUCT_LAYOUT_BYTES {
        return Err(StructLayoutRefusal::UnsupportedField);
    }
    Ok(FieldSpec {
        name,
        size: total,
        align,
        element: MirDeviceViewElement::Scalar(scalar),
    })
}

fn scalar_size_align(scalar: MirScalarLayout) -> Result<(u32, u32), StructLayoutRefusal> {
    match scalar {
        MirScalarLayout::F32 | MirScalarLayout::I32 | MirScalarLayout::U32 => Ok((4, 4)),
        MirScalarLayout::I64 | MirScalarLayout::U64 => Ok((8, 8)),
        MirScalarLayout::Bool => Err(StructLayoutRefusal::Bool),
        MirScalarLayout::I8 | MirScalarLayout::U8 => Ok((1, 1)),
        MirScalarLayout::I16
        | MirScalarLayout::U16
        | MirScalarLayout::F16
        | MirScalarLayout::Bf16 => Ok((2, 2)),
        MirScalarLayout::F64 => Err(StructLayoutRefusal::F64),
        MirScalarLayout::I128 | MirScalarLayout::Magnus | MirScalarLayout::D64 => {
            Err(StructLayoutRefusal::UnsupportedField)
        }
    }
}

/// Vector fields: width 2 is 8/8 and width 4 is 16/16 (4-byte elements only);
/// width 3 is refused by name WITHOUT reading `MirDeviceVectorLayout::align`,
/// which is the 16-byte register alignment for width 3.
fn vector_spec(
    name: Symbol,
    vector: MirDeviceVectorLayout,
) -> Result<FieldSpec, StructLayoutRefusal> {
    if vector.width == 3 {
        return Err(StructLayoutRefusal::Vec3PackedNotYet);
    }
    let (element_size, _) = scalar_size_align(vector.element)?;
    if element_size != 4 || !matches!(vector.width, 2 | 4) {
        return Err(StructLayoutRefusal::UnsupportedField);
    }
    let size = element_size * u32::from(vector.width);
    Ok(FieldSpec {
        name,
        size,
        align: size,
        element: MirDeviceViewElement::Vector(vector),
    })
}

/// The shared C-style packer. Fields are placed in the given order.
fn pack_fields(specs: &[FieldSpec]) -> Result<MirStructLayout, StructLayoutRefusal> {
    let mut fields = Vec::with_capacity(specs.len());
    let mut offset = 0u32;
    let mut struct_align = 1u32;
    for spec in specs {
        let at = offset.next_multiple_of(spec.align);
        fields.push(MirStructField {
            name: spec.name,
            offset: at,
            size: spec.size,
            align: spec.align,
            element: spec.element,
        });
        offset = at + spec.size;
        struct_align = struct_align.max(spec.align);
    }
    let size = offset.next_multiple_of(struct_align);
    if size > MAX_STRUCT_LAYOUT_BYTES {
        return Err(StructLayoutRefusal::TooLarge);
    }
    Ok(MirStructLayout {
        fields,
        size,
        align: struct_align,
    })
}

/// Table-only packer for the packed-vec3 arithmetic rows: `vector<_, size 3>` is
/// 12 bytes, align 4. Not product behaviour; the resolver refuses width 3.
#[cfg(test)]
pub(crate) fn pack_table_with_packed_vec3(
    rows: &[(Symbol, MirDeviceViewElement)],
) -> Result<MirStructLayout, StructLayoutRefusal> {
    let mut specs = Vec::with_capacity(rows.len());
    for (name, element) in rows {
        let spec = match element {
            MirDeviceViewElement::Vector(vector) if vector.width == 3 => FieldSpec {
                name: *name,
                size: 12,
                align: 4,
                element: *element,
            },
            MirDeviceViewElement::Vector(vector) => vector_spec(*name, *vector)?,
            MirDeviceViewElement::Scalar(scalar) => {
                let (size, align) = scalar_size_align(*scalar)?;
                FieldSpec {
                    name: *name,
                    size,
                    align,
                    element: *element,
                }
            }
            // A record has no table-only packed-vec3 spec; the row packer is
            // test-only and admits only scalars and vectors.
            MirDeviceViewElement::Record(_) => return Err(StructLayoutRefusal::UnsupportedField),
        };
        specs.push(spec);
    }
    pack_fields(&specs)
}
