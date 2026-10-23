//! gpu-reset C4-6a: pin tests for the genus parameter layout law (ruling (d)).
//!
//! Field symbols descend with declaration order (`Symbol(100_000 - i)`), so any
//! implementation that ordered by interner Symbol would reverse the layout and
//! fail every row.
// Test fixture text builders; allocation cost is irrelevant.
#![allow(clippy::format_push_string)]

use crate::struct_layout::pack_table_with_packed_vec3;
use crate::{
    MAX_STRUCT_LAYOUT_BYTES, MirDeviceVectorLayout, MirDeviceViewElement, MirScalarLayout,
    MirStructLayout, MirType, MirValidationContext, StructLayoutRefusal, resolve_struct_layout,
};
use radix_hir::DefId;
use radix_lexer::Symbol;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable};
use rustc_hash::FxHashMap;

const DEF: DefId = DefId(7);

struct Types {
    table: TypeTable,
    genus: TypeId,
}

impl Types {
    fn new() -> Self {
        let mut table = TypeTable::new();
        let genus = table.intern(Type::Struct(DEF));
        Self { table, genus }
    }

    fn f32(&mut self) -> TypeId {
        self.table
            .sized_numeric(Primitive::Fractus, NumericWidth::F32)
    }

    fn sized(&mut self, prim: Primitive, width: NumericWidth) -> TypeId {
        self.table.sized_numeric(prim, width)
    }

    fn i32(&mut self) -> TypeId {
        self.sized(Primitive::Numerus, NumericWidth::I32)
    }

    fn int(&self) -> TypeId {
        self.table.primitive(Primitive::Numerus)
    }

    fn vec(&mut self, element: TypeId, width: u64) -> TypeId {
        let width = self.table.intern_index(IndexExpr::Literal(width));
        self.table.intern(Type::Vector(element, width))
    }

    fn bounded_array(&mut self, element: TypeId, capacity: IndexExpr) -> TypeId {
        let capacity = self.table.intern_index(capacity);
        self.table.bounded_array(element, capacity)
    }
}

fn sym(index: usize) -> Symbol {
    Symbol(100_000 - u32::try_from(index).expect("small index"))
}

fn names(count: usize) -> Vec<Symbol> {
    (0..count).map(sym).collect()
}

/// Resolve `genus` with `fields` in declaration order; `tweak` edits the context.
fn resolve_with(
    types: &Types,
    fields: &[TypeId],
    tweak: impl FnOnce(&mut MirValidationContext<'_>),
) -> Result<MirStructLayout, StructLayoutRefusal> {
    let mut ctx = MirValidationContext::new(&types.table);
    let order = names(fields.len());
    let map: FxHashMap<Symbol, MirType> = order
        .iter()
        .zip(fields)
        .map(|(name, ty)| (*name, MirType::semantic(*ty)))
        .collect();
    ctx.struct_fields.insert(DEF, map);
    ctx.struct_field_order.insert(DEF, order);
    tweak(&mut ctx);
    resolve_struct_layout(MirType::semantic(types.genus), &ctx)
}

fn resolve(types: &Types, fields: &[TypeId]) -> Result<MirStructLayout, StructLayoutRefusal> {
    resolve_with(types, fields, |_| {})
}

fn rows(layout: &MirStructLayout) -> Vec<(u32, u32, u32)> {
    layout
        .fields
        .iter()
        .map(|field| (field.offset, field.size, field.align))
        .collect()
}

fn refusal(types: &Types, fields: &[TypeId]) -> StructLayoutRefusal {
    resolve(types, fields).expect_err("must be refused")
}

#[test]
fn mixed_scalar_and_vector_struct() {
    // genus Ex { f32 a; i32 b; vec2 c; vec4 d; f32 f }
    let mut t = Types::new();
    let (f, i) = (t.f32(), t.i32());
    let (v2, v4) = (t.vec(f, 2), t.vec(f, 4));
    let layout = resolve(&t, &[f, i, v2, v4, f]).expect("layout");
    assert_eq!(
        rows(&layout),
        vec![(0, 4, 4), (4, 4, 4), (8, 8, 8), (16, 16, 16), (32, 4, 4)]
    );
    assert_eq!((layout.size, layout.align), (48, 16));
    assert_eq!(layout.fields[0].name, sym(0));
    assert_eq!(layout.fields[4].name, sym(4));
}

#[test]
fn bounded_array_fields_pack_as_rows() {
    let mut t = Types::new();
    let f16 = t.sized(Primitive::Fractus, NumericWidth::F16);
    let i8 = t.sized(Primitive::Numerus, NumericWidth::I8);
    let first = t.bounded_array(i8, IndexExpr::Literal(32));
    let layout = resolve(&t, &[f16, first]).expect("Q8_0 row layout");
    assert_eq!(rows(&layout), vec![(0, 2, 2), (2, 32, 1)]);
    assert_eq!((layout.size, layout.align), (34, 2));

    let u8 = t.sized(Primitive::Numerus, NumericWidth::U8);
    let first = t.bounded_array(u8, IndexExpr::Literal(12));
    let second = t.bounded_array(u8, IndexExpr::Literal(128));
    let layout = resolve(&t, &[first, second]).expect("packed quantization rows");
    assert_eq!(rows(&layout), vec![(0, 12, 1), (12, 128, 1)]);
    assert_eq!((layout.size, layout.align), (140, 1));

    let f32 = t.f32();
    let i16 = t.sized(Primitive::Numerus, NumericWidth::I16);
    let third = t.bounded_array(i16, IndexExpr::Literal(3));
    let layout = resolve(&t, &[f32, third]).expect("aligned scalar tail");
    assert_eq!(rows(&layout), vec![(0, 4, 4), (4, 6, 2)]);
    assert_eq!((layout.size, layout.align), (12, 4));
}

#[test]
fn bounded_array_requires_literal_scalar_rows() {
    let mut t = Types::new();
    let f32 = t.f32();
    let hole = t.table.fresh_index_infer_var();
    let capacity_hole = t.bounded_array(f32, IndexExpr::Infer(hole));
    assert_eq!(
        refusal(&t, &[capacity_hole]),
        StructLayoutRefusal::UnsupportedField
    );

    let vector = t.vec(f32, 4);
    let vector_row = t.bounded_array(vector, IndexExpr::Literal(2));
    assert_eq!(
        refusal(&t, &[vector_row]),
        StructLayoutRefusal::UnsupportedField
    );

    let oversized_row = t.bounded_array(f32, IndexExpr::Literal(1025));
    assert_eq!(
        refusal(&t, &[oversized_row]),
        StructLayoutRefusal::UnsupportedField
    );

    let unbounded = t.table.intern(Type::Array(f32));
    assert_eq!(refusal(&t, &[unbounded]), StructLayoutRefusal::List);
}

#[test]
fn scalar_then_vec4_pads_to_16() {
    let mut t = Types::new();
    let (f, i) = (t.f32(), t.i32());
    let v4 = t.vec(f, 4);
    let layout = resolve(&t, &[i, v4]).expect("layout");
    assert_eq!(rows(&layout), vec![(0, 4, 4), (16, 16, 16)]);
    assert_eq!((layout.size, layout.align), (32, 16));
}

#[test]
fn int_is_eight_byte_aligned_and_u32_is_four() {
    let mut t = Types::new();
    let (f, u) = (t.f32(), t.sized(Primitive::Numerus, NumericWidth::U32));
    let layout = resolve(&t, &[f, t.int(), u]).expect("layout");
    assert_eq!(rows(&layout), vec![(0, 4, 4), (8, 8, 8), (16, 4, 4)]);
    assert_eq!((layout.size, layout.align), (24, 8));
}

fn vec3_element(align: u8) -> MirDeviceViewElement {
    MirDeviceViewElement::Vector(MirDeviceVectorLayout {
        element: MirScalarLayout::F32,
        width: 3,
        align,
    })
}

fn vec_element(width: u8) -> MirDeviceViewElement {
    MirDeviceViewElement::Vector(MirDeviceVectorLayout {
        element: MirScalarLayout::F32,
        width,
        align: width * 4,
    })
}

fn scalar_element(scalar: MirScalarLayout) -> MirDeviceViewElement {
    MirDeviceViewElement::Scalar(scalar)
}

#[test]
fn float3_trap_table_only() {
    // { f32 a; i32 b; vec2 c; vec4 d; vec3 e; f32 f }: packed vec3 is 12/4, so
    // e sits at 32 and f at 44; an unpacked float3 would put f at 48, size 64.
    // The vec3 row carries the 16-byte register align from layout.rs on
    // purpose: the packer must not read it.
    let layout = pack_table_with_packed_vec3(&[
        (sym(0), scalar_element(MirScalarLayout::F32)),
        (sym(1), scalar_element(MirScalarLayout::I32)),
        (sym(2), vec_element(2)),
        (sym(3), vec_element(4)),
        (sym(4), vec3_element(16)),
        (sym(5), scalar_element(MirScalarLayout::F32)),
    ])
    .expect("table");
    assert_eq!(
        rows(&layout),
        vec![
            (0, 4, 4),
            (4, 4, 4),
            (8, 8, 8),
            (16, 16, 16),
            (32, 12, 4),
            (44, 4, 4)
        ]
    );
    assert_eq!((layout.size, layout.align), (48, 16));
}

#[test]
fn vec3_then_scalar_table_only() {
    let layout = pack_table_with_packed_vec3(&[
        (sym(0), vec3_element(16)),
        (sym(1), scalar_element(MirScalarLayout::F32)),
    ])
    .expect("table");
    assert_eq!(rows(&layout), vec![(0, 12, 4), (12, 4, 4)]);
    assert_eq!((layout.size, layout.align), (16, 4));
}

/// The real `class Params` of acceptance/a2-flood/src/params.fab, in source
/// order. radix-mir cannot build a type table from source, so the field list
/// is mirrored: width, height (i32), sixteen f32 (dx .. `axis_z`), then
/// `geo_step`, `geo_w`, `geo_h` (i32). Every field is four bytes, so the genus
/// is 21 consecutive 4-byte fields: size 84, align 4.
const PARAMS_FIELDS: [(&str, bool); 21] = [
    ("width", true),
    ("height", true),
    ("dx", false),
    ("g", false),
    ("cfl", false),
    ("dt_max", false),
    ("h_dry", false),
    ("manning", false),
    ("ocean_floor", false),
    ("flip_start", false),
    ("flip_duration", false),
    ("flip_power", false),
    ("flip_k", false),
    ("radius", false),
    ("omega0", false),
    ("axis_x", false),
    ("axis_y", false),
    ("axis_z", false),
    ("geo_step", true),
    ("geo_w", true),
    ("geo_h", true),
];

#[test]
fn a2_params_layout() {
    let mut t = Types::new();
    let (f, i32) = (t.f32(), t.i32());
    let fields: Vec<TypeId> = PARAMS_FIELDS
        .iter()
        .map(|(_, is_int)| if *is_int { i32 } else { f })
        .collect();
    let layout = resolve(&t, &fields).expect("Params layout");
    let mut table = String::new();
    for (field, (name, _)) in layout.fields.iter().zip(PARAMS_FIELDS) {
        table.push_str(&format!(
            "{name:<14} offset {:>3} size {} align {}\n",
            field.offset, field.size, field.align
        ));
    }
    table.push_str(&format!("size {} align {}\n", layout.size, layout.align));
    println!("{table}");
    let offset = |name: &str| {
        let index = PARAMS_FIELDS.iter().position(|(n, _)| *n == name).unwrap();
        layout.fields[index].offset
    };
    assert_eq!(offset("width"), 0);
    assert_eq!(offset("height"), 4);
    assert_eq!(offset("dx"), 8);
    assert_eq!(offset("axis_z"), 68);
    assert_eq!(offset("geo_step"), 72);
    assert_eq!(offset("geo_w"), 76);
    assert_eq!(offset("geo_h"), 80);
    assert_eq!((layout.size, layout.align), (84, 4));
}

#[test]
fn declaration_order_not_symbol_order() {
    // Symbols descend with declaration order; the layout must still follow
    // declaration order.
    let mut t = Types::new();
    let (f, i) = (t.f32(), t.i32());
    let layout = resolve(&t, &[i, f]).expect("layout");
    assert_eq!(layout.fields[0].name, sym(0));
    assert_eq!(
        layout.fields[0].element,
        scalar_element(MirScalarLayout::I32)
    );
    assert_eq!(
        layout.fields[1].element,
        scalar_element(MirScalarLayout::F32)
    );
}

// --- Refusal rows: one named refusal each ------------------------------------

#[test]
fn refuses_bool() {
    let t = Types::new();
    let b = t.table.primitive(Primitive::Bivalens);
    assert_eq!(refusal(&t, &[b]), StructLayoutRefusal::Bool);
}

#[test]
fn narrow_scalars_pack_by_width() {
    let mut t = Types::new();
    let i8 = t.sized(Primitive::Numerus, NumericWidth::I8);
    let u8 = t.sized(Primitive::Numerus, NumericWidth::U8);
    let i16 = t.sized(Primitive::Numerus, NumericWidth::I16);
    let u16 = t.sized(Primitive::Numerus, NumericWidth::U16);
    let f16 = t.sized(Primitive::Fractus, NumericWidth::F16);
    let bf16 = t.sized(Primitive::Fractus, NumericWidth::BF16);
    let f32 = t.f32();
    let layout = resolve(&t, &[i8, u8, i16, u16, f16, bf16, f32]).expect("layout");
    assert_eq!(
        rows(&layout),
        vec![
            (0, 1, 1),
            (1, 1, 1),
            (2, 2, 2),
            (4, 2, 2),
            (6, 2, 2),
            (8, 2, 2),
            (12, 4, 4),
        ]
    );
    assert_eq!((layout.size, layout.align), (16, 4));
}

#[test]
fn refuses_f64() {
    let mut t = Types::new();
    let sized = t.sized(Primitive::Fractus, NumericWidth::F64);
    let bare = t.table.primitive(Primitive::Fractus);
    assert_eq!(refusal(&t, &[sized]), StructLayoutRefusal::F64);
    assert_eq!(refusal(&t, &[bare]), StructLayoutRefusal::F64);
}

#[test]
fn refuses_nested_genus() {
    let mut t = Types::new();
    let inner = t.table.intern(Type::Struct(DefId(8)));
    assert_eq!(refusal(&t, &[inner]), StructLayoutRefusal::NestedGenus);
}

#[test]
fn refuses_generics() {
    let mut t = Types::new();
    let f = t.f32();
    let applied = t.table.intern(Type::Applied(f, Vec::new()));
    assert_eq!(refusal(&t, &[applied]), StructLayoutRefusal::Generic);
    let result = resolve_with(&t, &[f], |ctx| {
        ctx.struct_type_params.insert(DEF, vec![Symbol(5)]);
    });
    assert_eq!(result, Err(StructLayoutRefusal::Generic));
}

#[test]
fn refuses_tuple_record_list_map_set_tensor() {
    let mut t = Types::new();
    let f = t.f32();
    let tuple = t.table.intern(Type::Tuple(vec![f, f]));
    let record = t.table.intern(Type::Record(FxHashMap::default()));
    let list = t.table.intern(Type::Array(f));
    let map = t.table.intern(Type::Map(f, f));
    let set = t.table.intern(Type::Set(f));
    let dim = t.table.intern_index(IndexExpr::Literal(8));
    let shape = t.table.intern_index(IndexExpr::Tuple(vec![dim]));
    let tensor = t.table.tensor_with_shape(f, shape);
    assert_eq!(refusal(&t, &[tuple]), StructLayoutRefusal::Tuple);
    assert_eq!(refusal(&t, &[record]), StructLayoutRefusal::Record);
    assert_eq!(refusal(&t, &[list]), StructLayoutRefusal::List);
    assert_eq!(refusal(&t, &[map]), StructLayoutRefusal::Map);
    assert_eq!(refusal(&t, &[set]), StructLayoutRefusal::Set);
    assert_eq!(refusal(&t, &[tensor]), StructLayoutRefusal::Tensor);
}

#[test]
fn refuses_sponte_and_nullable_fields() {
    let mut t = Types::new();
    let f = t.f32();
    let option = t.table.intern(Type::Option(f));
    assert_eq!(refusal(&t, &[option]), StructLayoutRefusal::Nullable);
    let optional = resolve_with(&t, &[f], |ctx| {
        ctx.optional_struct_fields
            .insert(DEF, std::iter::once(sym(0)).collect());
    });
    assert_eq!(optional, Err(StructLayoutRefusal::Nullable));
    let nullable = resolve_with(&t, &[f], |ctx| {
        ctx.nullable_struct_fields
            .insert(DEF, std::iter::once(sym(0)).collect());
    });
    assert_eq!(nullable, Err(StructLayoutRefusal::Nullable));
}

#[test]
fn refuses_text() {
    let t = Types::new();
    let text = t.table.primitive(Primitive::Textus);
    let littera = t.table.primitive(Primitive::Littera);
    assert_eq!(refusal(&t, &[text]), StructLayoutRefusal::Text);
    assert_eq!(refusal(&t, &[littera]), StructLayoutRefusal::Text);
}

#[test]
fn refuses_empty_genus() {
    let t = Types::new();
    assert_eq!(refusal(&t, &[]), StructLayoutRefusal::EmptyGenus);
}

#[test]
fn refuses_vec3_until_packed() {
    let mut t = Types::new();
    let f = t.f32();
    let v3 = t.vec(f, 3);
    assert_eq!(refusal(&t, &[f, v3]), StructLayoutRefusal::Vec3PackedNotYet);
}

#[test]
fn refuses_struct_above_4096_bytes() {
    let mut t = Types::new();
    let f = t.f32();
    let exactly = vec![f; (MAX_STRUCT_LAYOUT_BYTES / 4) as usize];
    let layout = resolve(&t, &exactly).expect("4096 bytes is admitted");
    assert_eq!(layout.size, MAX_STRUCT_LAYOUT_BYTES);
    let over = vec![f; (MAX_STRUCT_LAYOUT_BYTES / 4) as usize + 1];
    assert_eq!(refusal(&t, &over), StructLayoutRefusal::TooLarge);
}

#[test]
fn refuses_non_genus_and_missing_facts() {
    let mut t = Types::new();
    let f = t.f32();
    let ctx = MirValidationContext::new(&t.table);
    assert_eq!(
        resolve_struct_layout(MirType::semantic(f), &ctx),
        Err(StructLayoutRefusal::NotAGenus)
    );
    assert_eq!(
        resolve_struct_layout(MirType::semantic(t.genus), &ctx),
        Err(StructLayoutRefusal::UnknownFields)
    );
}

#[test]
fn refusal_reasons_are_lowercase_and_distinct() {
    use StructLayoutRefusal as R;
    let all = [
        R::NotAGenus,
        R::UnknownFields,
        R::OpaqueGenus,
        R::EmptyGenus,
        R::Bool,
        R::F64,
        R::NestedGenus,
        R::Generic,
        R::Tuple,
        R::Record,
        R::List,
        R::Map,
        R::Set,
        R::Tensor,
        R::Nullable,
        R::Text,
        R::Vec3PackedNotYet,
        R::TooLarge,
        R::UnsupportedField,
    ];
    let mut seen = std::collections::HashSet::new();
    for refusal in all {
        let text = refusal.to_string();
        assert_eq!(text, text.to_lowercase());
        assert!(seen.insert(text));
    }
}

/// C4-6b: the layout table travels beside the artifact, so it round-trips.
#[test]
fn layout_table_serialization_round_trips() {
    let mut t = Types::new();
    let (f, i) = (t.f32(), t.i32());
    let (v2, v4) = (t.vec(f, 2), t.vec(f, 4));
    let layout = resolve(&t, &[f, i, v2, v4, f]).expect("layout");
    let json = serde_json::to_string(&layout).expect("serializes");
    let back: MirStructLayout = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(back, layout);
    assert_eq!(back.fields[2].element, layout.fields[2].element);
}
