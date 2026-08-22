// Tests for MIR naming conventions and identity construction.
//
// These tests lock the single-letter prefix scheme for MIR ids,
// literal identities (text, ASCII, octeti, regex), external function
// import names, and entry-function detection. Names are a stable
// target-facing contract shared by text backends.

use super::*;
use crate::lexer::{Interner, Span, Symbol};
use crate::semantic::{Primitive, TypeTable};
use radix_mir::names::*;

fn ty(types: &TypeTable, primitive: Primitive) -> MirType {
    MirType::semantic(types.primitive(primitive))
}

#[test]
fn probe_names_use_single_letter_prefixes() {
    assert_eq!(MirNames::local(MirLocalId(3)), "l3");
    assert_eq!(MirNames::temp(MirTempId(7)), "t7");
    assert_eq!(MirNames::value(MirValueId(2)), "v2");
    assert_eq!(MirNames::block(MirBlockId(1)), "b1");
    assert_eq!(MirNames::function_fallback(MirFunctionId(4)), "f4");
}

#[test]
fn regex_literal_flags_suffix_preserves_no_flags_name() {
    assert_eq!(regex_literal_flags_suffix(None), "");
    assert_eq!(regex_literal_flags_suffix(Some(Symbol(8))), "_flags_8");
}

#[test]
fn octeti_literal_identity_includes_length_and_bytes() {
    assert_eq!(octeti_literal_identity(&[]), "octeti_0_empty");
    assert_eq!(octeti_literal_identity(&[0xde, 0xad, 0xbe]), "octeti_3_de_ad_be");
    assert_eq!(octeti_literal_identity(&[0xca, 0xfe, 0xba]), "octeti_3_ca_fe_ba");
}

#[test]
fn static_literal_identities_include_family_length_and_bytes() {
    assert_eq!(text_literal_identity("same"), "text_4_73_61_6d_65");
    assert_eq!(ascii_literal_identity("same"), "ascii_4_73_61_6d_65");
    assert_eq!(regex_pattern_literal_identity("a+"), "regex_pattern_2_61_2b");
    assert_eq!(regex_flags_literal_identity("im"), "regex_flags_2_69_6d");
    assert_eq!(text_literal_identity(""), "text_0_empty");
}

#[test]
fn external_function_import_name_uses_product_module_and_callable() {
    let identity = MirExternalFunctionIdentity::new("norma", vec!["json".to_owned()], "solve!");
    assert_eq!(
        external_function_import_name(&identity),
        "external_product_norma_module_json_func_solve_"
    );
}

#[test]
fn function_name_prefers_source_and_entry() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let vacuum = ty(&types, Primitive::Vacuum);
    let numerus = ty(&types, Primitive::Numerus);
    let factorial = interner.intern("factorial");
    let program = MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(0),
                source: Some(crate::hir::DefId(1)),
                name: Some(factorial),
                params: vec![MirParam {
                    local: MirLocalId(0),
                    name: None,
                    ty: numerus,
                    mode: MirParamMode::Owned,
                    span: Span::new(0, 0),
                }],
                locals: vec![],
                temps: vec![],
                blocks: vec![],
                return_ty: numerus,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: Span::new(0, 0),
            },
            MirFunction {
                id: MirFunctionId(1),
                source: None,
                name: None,
                params: vec![],
                locals: vec![],
                temps: vec![],
                blocks: vec![],
                return_ty: vacuum,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: Span::new(0, 0),
            },
        ],
    };
    let names = MirNames::new(&program, &types, &interner);
    assert_eq!(names.function(&program.functions[0]), "factorial");
    assert_eq!(names.function(&program.functions[1]), "incipit");
    assert!(names.is_entry_function(&program.functions[1]));
    assert_eq!(names.entry_function().map(|f| f.id), Some(MirFunctionId(1)));
}
