//! Tests for MIR type helper functions.
//!
//! These tests prove that option payload extraction, option chain base
//! type resolution, constant type mapping, index projection result
//! types, and place base type resolution produce correct MirType values
//! or appropriate errors. No dependency on lowering or emission.

use crate::mir::ty;
use crate::mir::*;
use crate::semantic::{NumericWidth, Primitive, Type, TypeTable};

#[test]
fn option_payload_ty_unwraps_option() {
    let mut types = TypeTable::new();
    let inner = types.primitive(Primitive::Numerus);
    let option_id = types.intern(Type::Option(inner));
    let payload = ty::option_payload_ty(MirType::semantic(option_id), &types).expect("option unwraps");
    assert_eq!(payload.semantic_id(), inner);
}

#[test]
fn option_payload_ty_resolves_nullable_union() {
    let mut types = TypeTable::new();
    let nihil = types.primitive(Primitive::Nihil);
    let numerus = types.primitive(Primitive::Numerus);
    let union_id = types.intern(Type::Union(vec![numerus, nihil]));
    let payload = ty::option_payload_ty(MirType::semantic(union_id), &types).expect("union unwraps");
    assert_eq!(payload.semantic_id(), numerus);
}

#[test]
fn option_payload_ty_passes_through_primitive() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let payload = ty::option_payload_ty(MirType::semantic(numerus), &types).expect("primitive passes through");
    assert_eq!(payload.semantic_id(), numerus);
}

#[test]
fn option_chain_base_ty_strips_option() {
    let mut types = TypeTable::new();
    let inner = types.primitive(Primitive::Textus);
    let option_id = types.intern(Type::Option(inner));
    let base = ty::option_chain_base_ty(MirType::semantic(option_id), &types);
    assert_eq!(base.semantic_id(), inner);
}

#[test]
fn option_chain_base_ty_passes_through_non_option() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let base = ty::option_chain_base_ty(MirType::semantic(numerus), &types);
    assert_eq!(base.semantic_id(), numerus);
}

#[test]
fn constant_ty_maps_primitives() {
    let types = TypeTable::new();
    let program = MirProgram::default();
    let int_ty = ty::constant_ty(&MirConstant::Int(42), &types, &program).expect("int constant");
    assert_eq!(int_ty.semantic_id(), types.primitive(Primitive::Numerus));

    let float_ty = ty::constant_ty(&MirConstant::Float(1.5), &types, &program).expect("float constant");
    assert_eq!(float_ty.semantic_id(), types.primitive(Primitive::Fractus));

    let nil_ty = ty::constant_ty(&MirConstant::Nil, &types, &program).expect("nil constant");
    assert_eq!(nil_ty.semantic_id(), types.primitive(Primitive::Nihil));
}

#[test]
fn constant_ty_rejects_unknown_function() {
    let types = TypeTable::new();
    let program = MirProgram::default();
    let err = ty::constant_ty(&MirConstant::Function(MirFunctionId(99)), &types, &program)
        .expect_err("unknown function rejected");
    assert_eq!(err.issue, "mir_type_unknown_function");
}

#[derive(Default)]
struct Lookup {
    locals: std::collections::HashMap<MirLocalId, MirType>,
    temps: std::collections::HashMap<MirTempId, MirType>,
}

impl ty::MirTypeLookup for Lookup {
    fn local_ty(&self, id: MirLocalId) -> Option<MirType> {
        self.locals.get(&id).copied()
    }
    fn temp_ty(&self, id: MirTempId) -> Option<MirType> {
        self.temps.get(&id).copied()
    }
}

#[test]
fn place_base_ty_resolves_local() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let lookup = Lookup {
        locals: [(MirLocalId(1), MirType::semantic(numerus))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let place = MirPlace::local(MirLocalId(1));
    let resolved = ty::place_base_ty(&place, &lookup).expect("local resolves");
    assert_eq!(resolved.semantic_id(), numerus);
}

#[test]
fn index_projection_result_ty_supports_octeti_and_lista_u8() {
    let mut types = TypeTable::new();
    let u8 = types.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8));
    let octeti = types.primitive(Primitive::Octeti);
    let lista_u8 = types.intern(Type::Array(u8));

    let octeti_index = ty::index_projection_result_ty(MirType::semantic(octeti), &types).expect("octeti indexes to u8");
    assert_eq!(octeti_index.semantic_id(), u8);

    let lista_index = ty::index_projection_result_ty(MirType::semantic(lista_u8), &types).expect("lista indexes to u8");
    assert_eq!(lista_index.semantic_id(), u8);
}

#[test]
fn index_projection_result_ty_rejects_tuple() {
    let mut types = TypeTable::new();
    let i64 = types.primitive(Primitive::Numerus);
    let tuple_id = types.tuple(vec![i64]);
    let tuple_ty = MirType::semantic(tuple_id);
    let err = ty::index_projection_result_ty(tuple_ty, &types).expect_err("tuple index projection returns error");
    assert!(err.to_string().contains("tuple"), "Error should mention tuple, got: {err}");
}

#[test]
fn place_base_ty_rejects_undefined_local() {
    let lookup = Lookup::default();
    let place = MirPlace::local(MirLocalId(99));
    let err = ty::place_base_ty(&place, &lookup).expect_err("undefined local rejected");
    assert_eq!(err.issue, "mir_type_undefined_local");
}
