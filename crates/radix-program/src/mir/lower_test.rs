use super::*;

#[test]
fn remapped_validation_keeps_colliding_struct_and_variant_consumers() {
    // This is the source shape from parameter.fab: the same source DefId
    // reaches both metadata domains because variant lowering falls back to
    // a bare name lookup when its parent-qualified entry is unavailable.
    let source_def = DefId(4109);
    let source_enum = DefId(4110);
    let struct_dest = DefId(2_000_000_301);
    let enum_dest = DefId(2_000_000_302);
    let variant_dest = DefId(2_000_000_235);

    let source_types = TypeTable::new();
    let number = source_types.primitive(Primitive::Numerus);
    let mut source = radix_mir::MirValidationContext::new(&source_types);
    let mut source_interner = Interner::new();
    let struct_field = source_interner.intern("struct_field");
    let variant_field = source_interner.intern("variant_field");
    source
        .struct_fields
        .entry(source_def)
        .or_default()
        .insert(struct_field, MirType::semantic(number));
    source.variant_parents.insert(source_def, source_enum);
    source.enum_variants.insert(source_enum, vec![source_def]);
    source
        .variant_fields
        .entry(source_def)
        .or_default()
        .insert(variant_field, MirType::semantic(number));

    let mut struct_remap = HashMap::new();
    struct_remap.insert(source_def, struct_dest);
    struct_remap.insert(source_enum, enum_dest);
    let mut variant_remap = HashMap::new();
    variant_remap.insert(source_def, variant_dest);
    let mut entry_interner = Interner::new();
    let source_rewrites = SourceRewrites::new();
    let remapped = collect_remapped_validation(
        &source,
        &source_interner,
        &mut entry_interner,
        &[],
        &struct_remap,
        &variant_remap,
        &HashMap::new(),
        &source_types,
        &source_rewrites,
        Path::new("parameter.fab"),
    );

    // Struct consumers must keep the struct slot, while variant consumers
    // must keep the package-synthetic variant slot. A precedence-based
    // lookup cannot satisfy both assertions for one source DefId.
    assert!(remapped.struct_fields.contains_key(&struct_dest));
    assert!(!remapped.struct_fields.contains_key(&variant_dest));
    assert_eq!(
        remapped.variant_parents.get(&variant_dest),
        Some(&enum_dest)
    );
    assert_eq!(
        remapped.enum_variants.get(&enum_dest),
        Some(&vec![variant_dest])
    );
    assert!(remapped.variant_fields.contains_key(&variant_dest));
    assert!(!remapped.variant_fields.contains_key(&struct_dest));
}

#[test]
fn rewrite_validation_def_ids_moves_source_local_fields_off_remapped_dests() {
    // dtype.fab after remap: DType.I32 dest is 4116 (empty payload), while
    // source-local DTypeError.UnknownName still occupies DefId 4116 with
    // commune `message`. Applying remapped metadata without moving keys
    // contaminates the unit-variant slot.
    let i32_source = DefId(4100);
    let unknown_name_source = DefId(4116);
    let i32_dest = DefId(4116);
    let unknown_name_dest = DefId(4119);
    let dtype = DefId(4096);
    let dtype_error = DefId(4115);
    let dtype_dest = DefId(4112);
    let dtype_error_dest = DefId(4118);

    let types = TypeTable::new();
    let text = types.primitive(Primitive::Textus);
    let mut context = radix_mir::MirValidationContext::new(&types);
    let mut interner = Interner::new();
    let message = interner.intern("message");
    context.variant_fields.entry(i32_source).or_default();
    context
        .variant_fields
        .entry(unknown_name_source)
        .or_default()
        .insert(message, MirType::semantic(text));
    context.variant_parents.insert(i32_source, dtype);
    context
        .variant_parents
        .insert(unknown_name_source, dtype_error);
    context.enum_variants.insert(dtype, vec![i32_source]);
    context
        .enum_variants
        .insert(dtype_error, vec![unknown_name_source]);

    let mut variant_remap = HashMap::new();
    variant_remap.insert(i32_source, i32_dest);
    variant_remap.insert(unknown_name_source, unknown_name_dest);
    let mut struct_remap = HashMap::new();
    struct_remap.insert(dtype, dtype_dest);
    struct_remap.insert(dtype_error, dtype_error_dest);

    rewrite_validation_def_ids(&mut context, &variant_remap, &struct_remap);

    let i32_fields = context
        .variant_fields
        .get(&i32_dest)
        .expect("remapped DType.I32 keeps a field slot");
    assert!(
        !i32_fields.contains_key(&message),
        "commune message must not remain on the remapped unit-variant dest"
    );
    assert_eq!(
        context
            .variant_fields
            .get(&unknown_name_dest)
            .map(|fields| fields.contains_key(&message)),
        Some(true),
        "DTypeError.UnknownName must keep message at its remapped dest"
    );
    assert_eq!(context.variant_parents.get(&i32_dest), Some(&dtype_dest));
    assert_eq!(
        context.variant_parents.get(&unknown_name_dest),
        Some(&dtype_error_dest)
    );
}

#[test]
fn rewrite_lowered_type_ids_applies_remapped_validation_off_rewritten_keys() {
    // Full production path for dtype.fab: the sibling unit's context goes
    // through rewrite_lowered_type_ids, which must move source-local keys
    // BEFORE remapped.apply. Source-local DTypeError.UnknownName still
    // occupies DefId 4116 with commune `message`, and the consumer's
    // DType.I32 remaps onto 4116; apply runs or_insert, so unrewritten
    // keys leave message on the unit-variant slot.
    let i32_source = DefId(4100);
    let unknown_name_source = DefId(4116);
    let i32_dest = DefId(4116);
    let unknown_name_dest = DefId(4119);
    let dtype = DefId(4096);
    let dtype_error = DefId(4115);
    let dtype_dest = DefId(4112);
    let dtype_error_dest = DefId(4118);

    let types = TypeTable::new();
    let text = types.primitive(Primitive::Textus);
    let mut source = radix_mir::MirValidationContext::new(&types);
    let mut interner = Interner::new();
    let message = interner.intern("message");
    source.variant_fields.entry(i32_source).or_default();
    source
        .variant_fields
        .entry(unknown_name_source)
        .or_default()
        .insert(message, MirType::semantic(text));
    source.variant_parents.insert(i32_source, dtype);
    source
        .variant_parents
        .insert(unknown_name_source, dtype_error);
    source.enum_variants.insert(dtype, vec![i32_source]);
    source
        .enum_variants
        .insert(dtype_error, vec![unknown_name_source]);

    let validated = radix_mir::ValidatedMir::new(MirProgram::new(), source.clone())
        .expect("sibling dtype unit validates before the rewrite");
    let lowered = LoweredMirUnit {
        interner: Interner::new(),
        monomorphizations: Vec::new(),
        closure_environments: Vec::new(),
        device_function_metadata: MirDeviceFunctionMetadataMap::default(),
        generic_templates: Vec::new(),
        generic_params: Vec::new(),
        tensor_edge_policy_metadata: MirTensorEdgePolicyMetadata::default(),
        validated,
        companions: Default::default(),
        imported_device_routes: Default::default(),
        imported_device_registrations: Vec::new(),
        display_tokens: radix_module::runtime_contract::display_tokens::DisplayTokens::latin(),
    };

    let mut variant_remap = HashMap::new();
    variant_remap.insert(i32_source, i32_dest);
    variant_remap.insert(unknown_name_source, unknown_name_dest);
    let mut struct_remap = HashMap::new();
    struct_remap.insert(dtype, dtype_dest);
    struct_remap.insert(dtype_error, dtype_error_dest);

    let mut entry_interner = Interner::new();
    let remapped = collect_remapped_validation(
        &source,
        &interner,
        &mut entry_interner,
        &[],
        &struct_remap,
        &variant_remap,
        &HashMap::new(),
        &types,
        &SourceRewrites::new(),
        Path::new("dtype.fab"),
    );

    let rewritten = rewrite_lowered_type_ids(
        lowered,
        &[],
        &types,
        &variant_remap,
        &struct_remap,
        &remapped,
        Path::new("dtype.fab"),
    )
    .expect("rewrite + remapped.apply must revalidate the sibling unit");

    let validation = rewritten.validated.validation();
    let i32_fields = validation
        .variant_fields
        .get(&i32_dest)
        .expect("remapped DType.I32 keeps a field slot");
    assert!(
        !i32_fields.contains_key(&message),
        "commune message must not land on the remapped unit-variant dest through rewrite + apply"
    );
    assert_eq!(
        validation
            .variant_fields
            .get(&unknown_name_dest)
            .map(|fields| fields.contains_key(&message)),
        Some(true),
        "DTypeError.UnknownName must keep message at its remapped dest"
    );
    assert_eq!(validation.variant_parents.get(&i32_dest), Some(&dtype_dest));
    assert_eq!(
        validation.variant_parents.get(&unknown_name_dest),
        Some(&dtype_error_dest)
    );
}

fn reachability_function(id: u32, source: u32, statements: Vec<MirStatement>) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: Some(DefId(source)),
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: radix_module::lexer::Span::default(),
            },
            span: radix_module::lexer::Span::default(),
        }],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: radix_module::lexer::Span::default(),
    }
}

fn reachability_call(callee: MirCallee) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Call {
            destination: None,
            callee,
            args: Vec::new(),
        },
        span: radix_module::lexer::Span::default(),
    }
}

fn reachability_library(
    functions: Vec<MirFunction>,
    templates: Vec<MirFunction>,
) -> LibraryMirParts {
    LibraryMirParts {
        program: MirProgram { functions },
        environments: Vec::new(),
        templates,
        generic_params: Vec::new(),
        tensor_edge_policy_metadata: MirTensorEdgePolicyMetadata::default(),
        device_function_metadata: MirDeviceFunctionMetadataMap::default(),
        display_tokens: radix_mir::MirDisplayTokens::default(),
    }
}

#[test]
fn library_pruning_follows_calls_templates_and_compacts_live_references() {
    use radix_mir::{MirClosureValue, MirConstant, MirLocalId, MirPlaceBase, MirValueId};
    use radix_module::lexer::Span;

    let span = Span::default();
    let closure = MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace {
                base: MirPlaceBase::Local(MirLocalId(0)),
                projections: Vec::new(),
            },
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Closure(MirClosureValue {
                    function: MirFunctionId(3),
                    environment: MirOperand::Value(MirValueId(1)),
                    environment_id: MirClosureEnvironmentId(7),
                }),
                ty: MirType::semantic(TypeId(0)),
                span,
            },
        },
        span,
    };
    let function_value = MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace {
                base: MirPlaceBase::Local(MirLocalId(0)),
                projections: Vec::new(),
            },
            value: MirValue {
                id: MirValueId(2),
                kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Function(
                    MirFunctionId(4),
                ))),
                ty: MirType::semantic(TypeId(0)),
                span,
            },
        },
        span,
    };
    let entry = reachability_function(
        0,
        100,
        vec![
            reachability_call(MirCallee::Function(MirFunctionId(2))),
            reachability_call(MirCallee::Definition {
                source: DefId(200),
                type_args: Vec::new(),
            }),
            closure,
            function_value,
        ],
    );
    let template = reachability_function(
        20,
        100,
        vec![reachability_call(MirCallee::Definition {
            source: DefId(204),
            type_args: Vec::new(),
        })],
    );

    let mut first = reachability_library(
        vec![
            entry,
            reachability_function(1, 101, Vec::new()),
            reachability_function(2, 102, Vec::new()),
            reachability_function(3, 103, Vec::new()),
            reachability_function(4, 104, Vec::new()),
        ],
        vec![template],
    );
    first.environments.push(MirClosureEnvironment {
        id: MirClosureEnvironmentId(7),
        function: MirFunctionId(3),
        value_ty: MirType::semantic(TypeId(0)),
        environment_local: MirLocalId(0),
        captures: Vec::new(),
    });
    let second = reachability_library(
        vec![
            reachability_function(
                0,
                200,
                vec![reachability_call(MirCallee::Definition {
                    source: DefId(202),
                    type_args: Vec::new(),
                })],
            ),
            reachability_function(
                1,
                202,
                vec![reachability_call(MirCallee::Definition {
                    source: DefId(203),
                    type_args: Vec::new(),
                })],
            ),
        ],
        Vec::new(),
    );
    let third = reachability_library(vec![reachability_function(0, 203, Vec::new())], Vec::new());
    let fourth = reachability_library(vec![reachability_function(0, 204, Vec::new())], Vec::new());
    let mut libraries = vec![first, second, third, fourth];
    let mut seeds = HashSet::from([DefId(100)]);

    prune_unreachable_library_functions(&mut libraries, &mut seeds);

    let kept_sources: Vec<Vec<DefId>> = libraries
        .iter()
        .map(|library| {
            library
                .program
                .functions
                .iter()
                .map(|function| function.source.unwrap())
                .collect()
        })
        .collect();
    assert_eq!(
        kept_sources,
        vec![
            vec![DefId(100), DefId(102), DefId(103), DefId(104)],
            vec![DefId(200), DefId(202)],
            vec![DefId(203)],
            vec![DefId(204)],
        ]
    );
    let kept_entry = &libraries[0].program.functions[0];
    assert!(matches!(
        kept_entry.blocks[0].statements[0].kind,
        MirStatementKind::Call {
            callee: MirCallee::Function(MirFunctionId(1)),
            ..
        }
    ));
    assert!(matches!(
        kept_entry.blocks[0].statements[2].kind,
        MirStatementKind::Assign {
            value: MirValue {
                kind: MirValueKind::Closure(MirClosureValue {
                    function: MirFunctionId(2),
                    ..
                }),
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        kept_entry.blocks[0].statements[3].kind,
        MirStatementKind::Assign {
            value: MirValue {
                kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Function(
                    MirFunctionId(3)
                ))),
                ..
            },
            ..
        }
    ));
    assert_eq!(libraries[0].environments[0].function, MirFunctionId(2));
    assert!(seeds.contains(&DefId(203)) && seeds.contains(&DefId(204)));
}
