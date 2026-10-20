//! Cost-basis and classifier pins for [`super`].
//!
//! Each v1 basis-table row has a classification pin (including `F` and `U`).
//! Amplification is a walker hook: the classifier returns the intrinsic class
//! and [`super::amplify_class`] adds `round(log10(n))`.

use super::*;
use crate::{MirConversion, MirConversionFlavor, MirStructFieldDefaults, MirType};
use radix_types::{DefId, IndexExpr, NumericWidth, Primitive, Type, TypeTable};

fn prim(types: &TypeTable, prim: Primitive) -> MirType {
    MirType::semantic(types.primitive(prim))
}

fn conversion(flavor: MirConversionFlavor, source: MirType, target: MirType) -> MirConversion {
    MirConversion {
        store: None,
        flavor,
        source_ty: source,
        target_ty: target,
        params: Vec::new(),
        recovery: None,
        struct_defaults: Vec::new(),
    }
}

fn runtime(source: MirType, target: MirType) -> MirConversion {
    conversion(MirConversionFlavor::Runtime, source, target)
}

fn classify(conversion: &MirConversion, types: &TypeTable) -> CostTag {
    classify_conversion(conversion, types)
}

fn genus_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.intern(Type::Struct(DefId(1))))
}

fn list_f32(types: &mut TypeTable) -> MirType {
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    MirType::semantic(types.array(f32_ty))
}

fn tensor_f32(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32_ty, shape)))
}

#[test]
fn basis_table_is_the_thirteen_v1_rows() {
    assert_eq!(COST_BASIS.len(), CostFamily::ALL.len());
    for (row, family) in COST_BASIS.iter().zip(CostFamily::ALL) {
        assert_eq!(row.family, family);
        assert_eq!(row.tag, basis_tag(family));
    }
}

#[test]
fn class_units_are_logarithmic() {
    assert_eq!(CostClass::C0.units(), 1);
    assert_eq!(CostClass::C1.units(), 10);
    assert_eq!(CostClass::C2.units(), 100);
    assert_eq!(CostClass::C3.units(), 1000);
}

#[test]
fn cast_flavor_is_c0() {
    let types = TypeTable::new();
    let conv = conversion(
        MirConversionFlavor::Cast,
        prim(&types, Primitive::Numerus),
        prim(&types, Primitive::Fractus),
    );
    assert_eq!(conversion_family(&conv, &types), CostFamily::Cast);
    assert_eq!(classify(&conv, &types), CostTag::Class(CostClass::C0));
}

#[test]
fn runtime_numeric_scalar_cast_is_c0() {
    let mut types = TypeTable::new();
    let i32 = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I32));
    let conv = runtime(prim(&types, Primitive::Numerus), i32);
    assert_eq!(conversion_family(&conv, &types), CostFamily::Cast);
    assert_eq!(classify(&conv, &types), CostTag::Class(CostClass::C0));
}

#[test]
fn octeti_window_unpack_is_c0() {
    let mut types = TypeTable::new();
    let i32 = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I32));
    let conv = runtime(prim(&types, Primitive::Octeti), i32);
    assert_eq!(
        conversion_family(&conv, &types),
        CostFamily::OctetiWindowUnpack
    );
    assert_eq!(classify(&conv, &types), CostTag::Class(CostClass::C0));
}

#[test]
fn textus_to_numerus_is_c1() {
    let types = TypeTable::new();
    let conv = runtime(
        prim(&types, Primitive::Textus),
        prim(&types, Primitive::Numerus),
    );
    assert_eq!(
        conversion_family(&conv, &types),
        CostFamily::TextusToNumerus
    );
    assert_eq!(classify(&conv, &types), CostTag::Class(CostClass::C1));
}

#[test]
fn numerus_to_textus_is_c1() {
    let types = TypeTable::new();
    let conv = runtime(
        prim(&types, Primitive::Numerus),
        prim(&types, Primitive::Textus),
    );
    assert_eq!(
        conversion_family(&conv, &types),
        CostFamily::NumerusToTextus
    );
    assert_eq!(classify(&conv, &types), CostTag::Class(CostClass::C1));
}

#[test]
fn genus_record_extraction_is_c1() {
    let mut types = TypeTable::new();
    let genus = genus_ty(&mut types);
    let conv = MirConversion {
        store: None,
        flavor: MirConversionFlavor::Runtime,
        source_ty: prim(&types, Primitive::Valor),
        target_ty: genus,
        params: Vec::new(),
        recovery: None,
        struct_defaults: vec![MirStructFieldDefaults {
            def_id: DefId(1),
            fields: Vec::new(),
        }],
    };
    assert_eq!(
        conversion_family(&conv, &types),
        CostFamily::GenusRecordExtraction
    );
    assert_eq!(classify(&conv, &types), CostTag::Class(CostClass::C1));
}

#[test]
fn list_tensor_is_c2_intrinsic() {
    let mut types = TypeTable::new();
    let list = list_f32(&mut types);
    let tensor = tensor_f32(&mut types, [4, 4]);
    let to_tensor = runtime(list, tensor);
    let to_list = runtime(tensor, list);
    assert_eq!(
        conversion_family(&to_tensor, &types),
        CostFamily::ListTensor
    );
    assert_eq!(conversion_family(&to_list, &types), CostFamily::ListTensor);
    assert_eq!(classify(&to_tensor, &types), CostTag::Class(CostClass::C2));
    assert_eq!(classify(&to_list, &types), CostTag::Class(CostClass::C2));
}

fn vector_u32x4(types: &mut TypeTable) -> MirType {
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let width = types.intern_index(IndexExpr::Literal(4));
    MirType::semantic(types.vector_with_width(u32_ty, width))
}

#[test]
fn register_window_rows_are_c0() {
    let mut types = TypeTable::new();
    let input_ty = vector_u32x4(&mut types);
    let cap16 = types.intern_index(IndexExpr::Literal(16));
    let octeti16 = MirType::semantic(types.bounded_octeti(cap16));
    let window_in = runtime(octeti16, input_ty);
    let window_out = runtime(input_ty, octeti16);
    assert_eq!(
        conversion_family(&window_in, &types),
        CostFamily::RegisterWindow
    );
    assert_eq!(
        conversion_family(&window_out, &types),
        CostFamily::RegisterWindow
    );
    assert_eq!(classify(&window_in, &types), CostTag::Class(CostClass::C0));
    assert_eq!(classify(&window_out, &types), CostTag::Class(CostClass::C0));
}

#[test]
fn register_bridge_is_c0() {
    let mut types = TypeTable::new();
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width4 = types.intern_index(IndexExpr::Literal(4));
    let input_ty = MirType::semantic(types.vector_with_width(u32_ty, width4));
    let output_ty = MirType::semantic(types.vector_with_width(f32_ty, width4));
    let conv = runtime(input_ty, output_ty);
    assert_eq!(conversion_family(&conv, &types), CostFamily::RegisterBridge);
    assert_eq!(classify(&conv, &types), CostTag::Class(CostClass::C0));
}

#[test]
fn register_lista_rows_are_c2() {
    let mut types = TypeTable::new();
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let width4 = types.intern_index(IndexExpr::Literal(4));
    let input_ty = MirType::semantic(types.vector_with_width(u32_ty, width4));
    let list_u32 = MirType::semantic(types.array(u32_ty));
    let to_list = runtime(input_ty, list_u32);
    let to_vector = runtime(list_u32, input_ty);
    assert_eq!(
        conversion_family(&to_list, &types),
        CostFamily::RegisterLista
    );
    assert_eq!(
        conversion_family(&to_vector, &types),
        CostFamily::RegisterLista
    );
    assert_eq!(classify(&to_list, &types), CostTag::Class(CostClass::C2));
    assert_eq!(classify(&to_vector, &types), CostTag::Class(CostClass::C2));
}

#[test]
fn textus_to_regex_is_c2() {
    let types = TypeTable::new();
    let conv = runtime(
        prim(&types, Primitive::Textus),
        prim(&types, Primitive::Regex),
    );
    assert_eq!(conversion_family(&conv, &types), CostFamily::TextusToRegex);
    assert_eq!(classify(&conv, &types), CostTag::Class(CostClass::C2));
}

#[test]
fn valor_json_and_json_genus_are_c3() {
    let mut types = TypeTable::new();
    let genus = genus_ty(&mut types);
    let valor_json = runtime(
        prim(&types, Primitive::Valor),
        prim(&types, Primitive::Json),
    );
    let json_genus = runtime(prim(&types, Primitive::Json), genus);
    assert_eq!(
        conversion_family(&valor_json, &types),
        CostFamily::ValorJson
    );
    assert_eq!(
        conversion_family(&json_genus, &types),
        CostFamily::ValorJson
    );
    assert_eq!(classify(&valor_json, &types), CostTag::Class(CostClass::C3));
    assert_eq!(classify(&json_genus, &types), CostTag::Class(CostClass::C3));
}

#[test]
fn constant_operand_row_is_f() {
    assert_eq!(basis_tag(CostFamily::ConstantOperand), CostTag::F);
    assert_eq!(foldable_tag(), CostTag::F);
}

#[test]
fn classifier_does_not_assign_f_from_type_pair() {
    let types = TypeTable::new();
    let conv = runtime(
        prim(&types, Primitive::Textus),
        prim(&types, Primitive::Numerus),
    );
    assert_ne!(classify(&conv, &types), CostTag::F);
    assert_ne!(
        conversion_family(&conv, &types),
        CostFamily::ConstantOperand
    );
}

#[test]
fn unknown_extern_row_and_unmatched_conversion_are_u() {
    let types = TypeTable::new();
    let conv = runtime(
        prim(&types, Primitive::Regex),
        prim(&types, Primitive::Textus),
    );
    assert_eq!(basis_tag(CostFamily::UnknownExtern), CostTag::U);
    assert_eq!(uncosted_tag(), CostTag::U);
    assert_eq!(conversion_family(&conv, &types), CostFamily::UnknownExtern);
    assert_eq!(classify(&conv, &types), CostTag::U);
}

#[test]
fn amplification_is_class_addition_never_multiplication() {
    assert_eq!(amplify_class(CostClass::C0, 0), CostClass::C0);
    assert_eq!(amplify_class(CostClass::C0, 1), CostClass::C0);
    assert_eq!(amplify_class(CostClass::C2, 1), CostClass::C2);
    assert_eq!(amplify_class(CostClass::C2, 10), CostClass::C3);
    assert_eq!(amplify_class(CostClass::C2, 100), CostClass::from_log10(4));
    assert_eq!(amplify_class(CostClass::C3, 1000), CostClass::from_log10(6));
    // Class addition, not multiplying the class number by N or by log10(N).
    assert_ne!(
        amplify_class(CostClass::C2, 1000),
        CostClass::from_log10(2 * 3)
    );
    assert_ne!(
        amplify_class(CostClass::C2, 50).units(),
        CostClass::C2.units().saturating_mul(50)
    );
}

#[test]
fn amplification_rounds_log10() {
    assert_eq!(amplify_class(CostClass::C0, 3), CostClass::C0);
    assert_eq!(amplify_class(CostClass::C0, 4), CostClass::C1);
    assert_eq!(amplify_class(CostClass::C0, 31), CostClass::C1);
    assert_eq!(amplify_class(CostClass::C0, 32), CostClass::C2);
    assert_eq!(amplify_class(CostClass::C0, 316), CostClass::C2);
    assert_eq!(amplify_class(CostClass::C0, 317), CostClass::C3);
}

#[test]
fn classifier_returns_intrinsic_class_walker_applies_amplification() {
    let mut types = TypeTable::new();
    let list = list_f32(&mut types);
    let tensor = tensor_f32(&mut types, [10, 10]);
    let conv = runtime(list, tensor);
    let tag = classify(&conv, &types);
    assert_eq!(tag, CostTag::Class(CostClass::C2));
    assert_eq!(tag.amplify(100), CostTag::Class(CostClass::from_log10(4)));
}

#[test]
fn foldable_and_uncosted_tags_ignore_amplification() {
    assert_eq!(foldable_tag().amplify(1_000), CostTag::F);
    assert_eq!(uncosted_tag().amplify(1_000), CostTag::U);
}

#[test]
fn proven_flavor_is_c0() {
    let mut types = TypeTable::new();
    let i16 = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I16));
    let conv = conversion(
        MirConversionFlavor::Proven,
        prim(&types, Primitive::Numerus),
        i16,
    );
    assert_eq!(conversion_family(&conv, &types), CostFamily::Cast);
    assert_eq!(classify(&conv, &types), CostTag::Class(CostClass::C0));
}
