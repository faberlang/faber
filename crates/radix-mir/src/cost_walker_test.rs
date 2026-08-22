//! Site-walker pins for [`super`].
//!
//! Covers one site per cost-basis family, amplified vs unamplified list/tensor
//! conversions, loop-member vs cold blocks, F-bucket constant/literal-fed
//! traces, and U-bucket unmatched converts plus package extern calls.

use super::*;
use crate::{
    MirBlock, MirBlockId, MirCallee, MirConstant, MirConversion, MirConversionFlavor,
    MirExternalFunctionIdentity, MirFunction, MirFunctionId, MirFunctionSignature, MirIntrinsic,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirProgram, MirProvider,
    MirProviderKind, MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator,
    MirTerminatorKind, MirType, MirValidationContext, MirValue, MirValueId, MirValueKind,
    ValidatedMir,
};
use radix_hir::DefId;
use radix_lexer::{Interner, Span, Symbol};
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};

fn site_span(start: u32) -> Span {
    Span::new(start, start + 10)
}

fn prim(types: &TypeTable, primitive: Primitive) -> MirType {
    MirType::semantic(types.primitive(primitive))
}

fn vacuum(types: &TypeTable) -> MirType {
    prim(types, Primitive::Vacuum)
}

fn local(id: u32, ty: MirType) -> MirLocal {
    MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: false,
        span: Span::default(),
    }
}

fn param(id: u32, ty: MirType) -> MirParam {
    MirParam {
        local: MirLocalId(id),
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span: Span::default(),
    }
}

fn conversion(flavor: MirConversionFlavor, source: MirType, target: MirType) -> MirConversion {
    MirConversion {
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

fn place(id: u32) -> MirOperand {
    MirOperand::Place(MirPlace::local(MirLocalId(id)))
}

fn convert_stmt(span: Span, dest: u32, conv: MirConversion, arg: MirOperand) -> MirStatement {
    let return_ty = conv.target_ty;
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::local(MirLocalId(dest))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Convert(conv),
                args: vec![arg],
                return_ty,
            },
        },
        span,
    }
}

fn ret() -> MirTerminator {
    MirTerminator {
        kind: MirTerminatorKind::Return(None),
        span: Span::default(),
    }
}

fn block(id: u32, statements: Vec<MirStatement>, terminator: MirTerminator) -> MirBlock {
    MirBlock {
        id: MirBlockId(id),
        statements,
        terminator,
        span: Span::default(),
    }
}

fn function(
    types: &TypeTable,
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    blocks: Vec<MirBlock>,
) -> MirFunction {
    function_with(types, None, None, params, locals, blocks)
}

fn function_with(
    types: &TypeTable,
    source: Option<DefId>,
    name: Option<Symbol>,
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    blocks: Vec<MirBlock>,
) -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source,
        name,
        params,
        locals,
        temps: Vec::new(),
        blocks,
        return_ty: vacuum(types),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn program(function: MirFunction) -> MirProgram {
    MirProgram {
        functions: vec![function],
    }
}

fn sites_of(program: MirProgram, types: &TypeTable) -> Vec<CostSite> {
    sites_with(program, MirValidationContext::new(types))
}

fn sites_with(program: MirProgram, context: MirValidationContext<'_>) -> Vec<CostSite> {
    let validated = ValidatedMir::new(program, context).unwrap_or_else(|errors| {
        panic!(
            "fixture must validate: {:?}",
            errors
                .iter()
                .map(|error| &error.message)
                .collect::<Vec<_>>()
        )
    });
    cost_sites(&validated)
}

fn tensor(types: &mut TypeTable, dims: Option<[u64; 2]>) -> MirType {
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = match dims {
        Some([rows, cols]) => {
            let row = types.intern_index(IndexExpr::Literal(rows));
            let col = types.intern_index(IndexExpr::Literal(cols));
            types.intern_index(IndexExpr::Tuple(vec![row, col]))
        }
        None => types.intern_index(IndexExpr::Unspecified),
    };
    MirType::semantic(types.tensor_with_shape(element, shape))
}

fn list_f32(types: &mut TypeTable) -> MirType {
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    MirType::semantic(types.array(element))
}

fn genus(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.intern(Type::Struct(DefId(1))))
}

fn i32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I32))
}

fn by_span(sites: &[CostSite], start: u32) -> &CostSite {
    let want = site_span(start);
    sites
        .iter()
        .find(|site| site.span == want)
        .unwrap_or_else(|| panic!("missing site at span {want:?} in {sites:?}"))
}

fn family_fixture(types: &mut TypeTable, interner: &mut Interner) -> MirProgram {
    let numerus = prim(types, Primitive::Numerus);
    let fractus = prim(types, Primitive::Fractus);
    let octeti = prim(types, Primitive::Octeti);
    let textus = prim(types, Primitive::Textus);
    let valor = prim(types, Primitive::Valor);
    let json = prim(types, Primitive::Json);
    let regex = prim(types, Primitive::Regex);
    let i32 = i32_ty(types);
    let genus = genus(types);
    let dynamic_tensor = tensor(types, None);
    let list = list_f32(types);
    let literal = MirOperand::Constant(MirConstant::String(interner.intern("42")));
    program(function(
        types,
        vec![
            param(0, numerus),
            param(1, octeti),
            param(2, textus),
            param(3, valor),
            param(4, dynamic_tensor),
            param(5, regex),
        ],
        vec![
            local(10, fractus),
            local(11, i32),
            local(12, numerus),
            local(13, textus),
            local(14, genus),
            local(15, list),
            local(16, regex),
            local(17, json),
            local(18, numerus),
            local(19, textus),
        ],
        vec![block(
            0,
            vec![
                convert_stmt(
                    site_span(100),
                    10,
                    conversion(MirConversionFlavor::Cast, numerus, fractus),
                    place(0),
                ),
                convert_stmt(site_span(200), 11, runtime(octeti, i32), place(1)),
                convert_stmt(site_span(300), 12, runtime(textus, numerus), place(2)),
                convert_stmt(site_span(400), 13, runtime(numerus, textus), place(0)),
                convert_stmt(site_span(500), 14, runtime(valor, genus), place(3)),
                convert_stmt(site_span(600), 15, runtime(dynamic_tensor, list), place(4)),
                convert_stmt(site_span(700), 16, runtime(textus, regex), place(2)),
                convert_stmt(site_span(800), 17, runtime(valor, json), place(3)),
                convert_stmt(site_span(900), 18, runtime(textus, numerus), literal),
                convert_stmt(site_span(1000), 19, runtime(regex, textus), place(5)),
            ],
            ret(),
        )],
    ))
}

#[test]
fn walker_emits_one_site_per_basis_family() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let sites = sites_of(family_fixture(&mut types, &mut interner), &types);
    assert_eq!(sites.len(), 10);

    let cast = by_span(&sites, 100);
    assert_eq!(cast.family, CostFamily::Cast);
    assert_eq!(cast.tag, CostTag::Class(CostClass::C0));
    assert!(!cast.hot);

    let octeti = by_span(&sites, 200);
    assert_eq!(octeti.family, CostFamily::OctetiWindowUnpack);
    assert_eq!(octeti.tag, CostTag::Class(CostClass::C0));

    let textus_numerus = by_span(&sites, 300);
    assert_eq!(textus_numerus.family, CostFamily::TextusToNumerus);
    assert_eq!(textus_numerus.tag, CostTag::Class(CostClass::C1));

    let numerus_textus = by_span(&sites, 400);
    assert_eq!(numerus_textus.family, CostFamily::NumerusToTextus);
    assert_eq!(numerus_textus.tag, CostTag::Class(CostClass::C1));

    let genus = by_span(&sites, 500);
    assert_eq!(genus.family, CostFamily::GenusRecordExtraction);
    assert_eq!(genus.tag, CostTag::Class(CostClass::C1));

    let list_tensor = by_span(&sites, 600);
    assert_eq!(list_tensor.family, CostFamily::ListTensor);
    assert_eq!(list_tensor.tag, CostTag::Class(CostClass::C2));
    assert_eq!(list_tensor.element_count, None);

    let regex = by_span(&sites, 700);
    assert_eq!(regex.family, CostFamily::TextusToRegex);
    assert_eq!(regex.tag, CostTag::Class(CostClass::C2));

    let valor_json = by_span(&sites, 800);
    assert_eq!(valor_json.family, CostFamily::ValorJson);
    assert_eq!(valor_json.tag, CostTag::Class(CostClass::C3));

    let foldable = by_span(&sites, 900);
    assert_eq!(foldable.family, CostFamily::TextusToNumerus);
    assert_eq!(foldable.tag, CostTag::F);
    assert_eq!(foldable.kind, CostSiteKind::Convert);

    let unknown = by_span(&sites, 1000);
    assert_eq!(unknown.family, CostFamily::UnknownExtern);
    assert_eq!(unknown.tag, CostTag::U);
}

#[test]
fn walker_amplifies_static_tensor_and_leaves_dynamic_unamplified() {
    let mut types = TypeTable::new();
    let static_tensor = tensor(&mut types, Some([10, 10]));
    let dynamic_tensor = tensor(&mut types, None);
    let list = list_f32(&mut types);
    let sites = sites_of(
        program(function(
            &types,
            vec![param(0, static_tensor), param(1, dynamic_tensor)],
            vec![local(2, list), local(3, list)],
            vec![block(
                0,
                vec![
                    convert_stmt(site_span(10), 2, runtime(static_tensor, list), place(0)),
                    convert_stmt(site_span(20), 3, runtime(dynamic_tensor, list), place(1)),
                ],
                ret(),
            )],
        )),
        &types,
    );
    let amplified = by_span(&sites, 10);
    assert_eq!(amplified.family, CostFamily::ListTensor);
    assert_eq!(amplified.element_count, Some(100));
    assert_eq!(amplified.tag, CostTag::Class(CostClass::from_log10(4)));
    let unamplified = by_span(&sites, 20);
    assert_eq!(unamplified.element_count, None);
    assert_eq!(unamplified.tag, CostTag::Class(CostClass::C2));
}

#[test]
fn walker_tags_loop_members_hot_and_straightline_cold() {
    let types = TypeTable::new();
    let numerus = prim(&types, Primitive::Numerus);
    let textus = prim(&types, Primitive::Textus);
    let flag = prim(&types, Primitive::Bivalens);
    let sites = sites_of(
        program(function(
            &types,
            vec![param(0, flag), param(1, numerus)],
            vec![local(2, textus), local(3, textus)],
            vec![
                block(
                    0,
                    Vec::new(),
                    MirTerminator {
                        kind: MirTerminatorKind::Branch {
                            condition: place(0),
                            then_block: MirBlockId(1),
                            else_block: MirBlockId(2),
                        },
                        span: Span::default(),
                    },
                ),
                block(
                    1,
                    vec![convert_stmt(
                        site_span(10),
                        2,
                        runtime(numerus, textus),
                        place(1),
                    )],
                    MirTerminator {
                        kind: MirTerminatorKind::Goto(MirBlockId(0)),
                        span: Span::default(),
                    },
                ),
                block(
                    2,
                    vec![convert_stmt(
                        site_span(20),
                        3,
                        runtime(numerus, textus),
                        place(1),
                    )],
                    ret(),
                ),
            ],
        )),
        &types,
    );
    assert!(by_span(&sites, 10).hot);
    assert!(!by_span(&sites, 20).hot);
    assert_eq!(by_span(&sites, 10).block, MirBlockId(1));
    assert_eq!(by_span(&sites, 20).block, MirBlockId(2));
}

#[test]
fn walker_folds_constant_operand_and_literal_fed_local() {
    let types = TypeTable::new();
    let mut interner = Interner::new();
    let textus = prim(&types, Primitive::Textus);
    let numerus = prim(&types, Primitive::Numerus);
    let literal = MirConstant::String(interner.intern("7"));
    let sites = sites_of(
        program(function(
            &types,
            Vec::new(),
            vec![local(0, textus), local(1, numerus), local(2, numerus)],
            vec![block(
                0,
                vec![
                    MirStatement {
                        kind: MirStatementKind::Assign {
                            place: MirPlace::local(MirLocalId(0)),
                            value: MirValue {
                                id: MirValueId(0),
                                kind: MirValueKind::Operand(MirOperand::Constant(literal.clone())),
                                ty: textus,
                                span: site_span(1),
                            },
                        },
                        span: site_span(1),
                    },
                    convert_stmt(site_span(10), 1, runtime(textus, numerus), place(0)),
                    convert_stmt(
                        site_span(20),
                        2,
                        runtime(textus, numerus),
                        MirOperand::Constant(literal),
                    ),
                ],
                ret(),
            )],
        )),
        &types,
    );
    assert_eq!(by_span(&sites, 10).tag, CostTag::F);
    assert_eq!(by_span(&sites, 20).tag, CostTag::F);
    assert_eq!(by_span(&sites, 10).family, CostFamily::TextusToNumerus);
}

#[test]
fn walker_buckets_package_call_and_provider_as_u() {
    let types = TypeTable::new();
    let vacuum = vacuum(&types);
    let def = DefId(99);
    let program = program(function(
        &types,
        Vec::new(),
        Vec::new(),
        vec![block(
            0,
            vec![
                MirStatement {
                    kind: MirStatementKind::Call {
                        destination: None,
                        callee: MirCallee::Definition {
                            source: def,
                            type_args: Vec::new(),
                        },
                        args: Vec::new(),
                    },
                    span: site_span(10),
                },
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: None,
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Provider(MirProvider {
                                kind: MirProviderKind::Package,
                                module: Vec::new(),
                                name: Symbol(0),
                            }),
                            args: Vec::new(),
                            return_ty: vacuum,
                        },
                    },
                    span: site_span(20),
                },
            ],
            ret(),
        )],
    ));
    let mut context = MirValidationContext::new(&types);
    context.functions.insert(
        def,
        MirFunctionSignature {
            params: Vec::new(),
            return_ty: vacuum,
            error_ty: None,
        },
    );
    context.external_function_identities.insert(
        def,
        MirExternalFunctionIdentity::new("acme", vec!["ext".to_owned()], "dispatch"),
    );
    let sites = sites_with(program, context);
    let call = by_span(&sites, 10);
    assert_eq!(call.kind, CostSiteKind::ExternCall);
    assert_eq!(call.family, CostFamily::UnknownExtern);
    assert_eq!(call.tag, CostTag::U);
    let provider = by_span(&sites, 20);
    assert_eq!(provider.kind, CostSiteKind::ExternCall);
    assert_eq!(provider.tag, CostTag::U);
}

#[test]
fn walker_records_function_source_and_statement_span() {
    let types = TypeTable::new();
    let numerus = prim(&types, Primitive::Numerus);
    let fractus = prim(&types, Primitive::Fractus);
    let source = DefId(7);
    let sites = sites_of(
        program(function_with(
            &types,
            Some(source),
            None,
            vec![param(0, numerus)],
            vec![local(1, fractus)],
            vec![block(
                0,
                vec![convert_stmt(
                    Span::new(40, 55),
                    1,
                    conversion(MirConversionFlavor::Cast, numerus, fractus),
                    place(0),
                )],
                ret(),
            )],
        )),
        &types,
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].function, MirFunctionId(0));
    assert_eq!(sites[0].source, Some(source));
    assert_eq!(sites[0].span, Span::new(40, 55));
    assert_eq!(sites[0].block, MirBlockId(0));
}

#[test]
fn rollup_reports_dominant_class_and_histogram() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let sites = sites_of(family_fixture(&mut types, &mut interner), &types);
    let rollups = cost_rollups(&sites);
    assert_eq!(rollups.len(), 1);
    let rollup = &rollups[0];
    assert_eq!(rollup.function, MirFunctionId(0));
    assert_eq!(rollup.dominant, Some(CostClass::C3));
    assert_eq!(
        rollup.class_histogram,
        vec![
            (CostClass::C0, 2),
            (CostClass::C1, 3),
            (CostClass::C2, 2),
            (CostClass::C3, 1),
        ]
    );
    assert_eq!(rollup.foldable, 1);
    assert_eq!(rollup.uncosted, 1);
    assert_eq!(rollup.hot, 0);
}
