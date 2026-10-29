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

// ── Package-link id plumbing over option chains, switch cases, closures and
//    tensor-edge identities: pure rewrites with observable id effects ──

fn fn_const(id: u32) -> MirOperand {
    MirOperand::Constant(MirConstant::Function(MirFunctionId(id)))
}

fn variant_place(variant: u32, symbol: Symbol) -> MirOperand {
    let mut place = MirPlace::local(radix_mir::MirLocalId(0));
    place.projections.push(MirProjection::VariantField {
        variant: DefId(variant),
        field: symbol,
    });
    MirOperand::Place(place)
}

fn closure_callee(function: u32, environment: u32, captured: u32) -> MirClosureCallee {
    MirClosureCallee {
        function: MirFunctionId(function),
        environment: fn_const(captured),
        environment_id: MirClosureEnvironmentId(environment),
    }
}

#[test]
fn tensor_edge_identity_rewrite_follows_nominals_through_every_wrapper() {
    use radix_mir_lower::lower::{
        MirTensorEdgeAppliedArg as Arg, MirTensorEdgeShapeIdentity as Shape,
        MirTensorEdgeTypeIdentity as Identity,
    };
    let rewrites: HashMap<DefId, DefId> = [(DefId(1), DefId(101)), (DefId(2), DefId(102))].into();
    let leaf = |def| Box::new(Identity::Struct(DefId(def)));
    let mut identity = Identity::Tuple(vec![
        Identity::Applied(
            leaf(1),
            vec![
                Arg::Type(Identity::Enum(DefId(2))),
                Arg::Size(Shape::Literal(4)),
            ],
        ),
        Identity::Union(vec![Identity::Interface(DefId(1))]),
        Identity::Intersection(vec![Identity::Option(leaf(2))]),
        Identity::Record(vec![("f".to_owned(), Identity::Array(leaf(1)))]),
        Identity::Tensor(leaf(2), Shape::Literal(3)),
        Identity::Ref(radix_types::SemanticMutability::Mutable, leaf(1)),
        // Not in the rewrite map: the identity is kept as is.
        Identity::Struct(DefId(9)),
        Identity::Primitive(Primitive::Numerus),
    ]);
    rewrite_tensor_edge_type_identity(&mut identity, &rewrites);
    assert_eq!(
        identity,
        Identity::Tuple(vec![
            Identity::Applied(
                leaf(101),
                vec![
                    Arg::Type(Identity::Enum(DefId(102))),
                    Arg::Size(Shape::Literal(4)),
                ],
            ),
            Identity::Union(vec![Identity::Interface(DefId(101))]),
            Identity::Intersection(vec![Identity::Option(leaf(102))]),
            Identity::Record(vec![("f".to_owned(), Identity::Array(leaf(101)))]),
            Identity::Tensor(leaf(102), Shape::Literal(3)),
            Identity::Ref(radix_types::SemanticMutability::Mutable, leaf(101)),
            Identity::Struct(DefId(9)),
            Identity::Primitive(Primitive::Numerus),
        ])
    );
}

#[test]
fn option_chain_variant_rewrite_reaches_links_and_operands() {
    let mut interner = Interner::new();
    let field = interner.intern("field");
    let remap: HashMap<DefId, DefId> = [(DefId(1), DefId(11)), (DefId(2), DefId(12))].into();

    let mut link = MirOptionChainLink::VariantField {
        variant: DefId(1),
        field,
    };
    rewrite_option_chain_variant_defs(&mut link, &remap);
    assert!(matches!(
        link,
        MirOptionChainLink::VariantField {
            variant: DefId(11),
            ..
        }
    ));
    // An unmapped variant is left alone.
    let mut link = MirOptionChainLink::VariantField {
        variant: DefId(7),
        field,
    };
    rewrite_option_chain_variant_defs(&mut link, &remap);
    assert!(matches!(
        link,
        MirOptionChainLink::VariantField {
            variant: DefId(7),
            ..
        }
    ));

    let mut link = MirOptionChainLink::Index(variant_place(2, field));
    rewrite_option_chain_variant_defs(&mut link, &remap);
    assert_eq!(link, MirOptionChainLink::Index(variant_place(12, field)));

    let mut link = MirOptionChainLink::Call {
        callee: MirCallee::Function(MirFunctionId(0)),
        args: vec![variant_place(1, field), variant_place(2, field)],
    };
    rewrite_option_chain_variant_defs(&mut link, &remap);
    assert_eq!(
        link,
        MirOptionChainLink::Call {
            callee: MirCallee::Function(MirFunctionId(0)),
            args: vec![variant_place(11, field), variant_place(12, field)],
        }
    );

    let mut link = MirOptionChainLink::Field(field);
    rewrite_option_chain_variant_defs(&mut link, &remap);
    assert_eq!(link, MirOptionChainLink::Field(field));

    // Through the option op: base operand and link are both rewritten.
    let mut op = MirOptionOp::Chain {
        base: variant_place(1, field),
        link: MirOptionChainLink::VariantField {
            variant: DefId(2),
            field,
        },
    };
    rewrite_option_variant_defs(&mut op, &remap);
    match op {
        MirOptionOp::Chain { base, link } => {
            assert_eq!(base, variant_place(11, field));
            assert!(matches!(
                link,
                MirOptionChainLink::VariantField {
                    variant: DefId(12),
                    ..
                }
            ));
        }
        other => panic!("expected a chain, got {other:?}"),
    }
}

#[test]
fn function_id_shift_moves_option_chain_calls_switch_cases_and_closures() {
    let mut case = MirSwitchCase {
        value: MirConstant::Function(MirFunctionId(3)),
        target: MirBlockId(1),
    };
    shift_switch_case_ids(&mut case, 10);
    assert_eq!(case.value, MirConstant::Function(MirFunctionId(13)));
    assert_eq!(case.target, MirBlockId(1));

    let mut link = MirOptionChainLink::Call {
        callee: MirCallee::Closure(closure_callee(1, 2, 3)),
        args: vec![fn_const(4)],
    };
    shift_option_chain_link_ids(&mut link, 10);
    assert_eq!(
        link,
        MirOptionChainLink::Call {
            callee: MirCallee::Closure(closure_callee(11, 12, 13)),
            args: vec![fn_const(14)],
        }
    );
    let mut link = MirOptionChainLink::Index(fn_const(5));
    shift_option_chain_link_ids(&mut link, 10);
    assert_eq!(link, MirOptionChainLink::Index(fn_const(15)));

    let mut op = MirOptionOp::Chain {
        base: fn_const(6),
        link: MirOptionChainLink::Index(fn_const(7)),
    };
    shift_option_ids(&mut op, 100);
    assert_eq!(
        op,
        MirOptionOp::Chain {
            base: fn_const(106),
            link: MirOptionChainLink::Index(fn_const(107))
        }
    );

    let mut closure = closure_callee(1, 2, 3);
    shift_closure_callee_ids(&mut closure, 5);
    assert_eq!(closure, closure_callee(6, 7, 8));
}

#[test]
fn function_id_remap_moves_option_chain_calls_and_switch_cases() {
    let remap: HashMap<MirFunctionId, MirFunctionId> = [
        (MirFunctionId(1), MirFunctionId(21)),
        (MirFunctionId(2), MirFunctionId(22)),
    ]
    .into();

    let mut case = MirSwitchCase {
        value: MirConstant::Function(MirFunctionId(2)),
        target: MirBlockId(0),
    };
    remap_switch_case_function_ids(&mut case, &remap);
    assert_eq!(case.value, MirConstant::Function(MirFunctionId(22)));
    // An id outside the remap is kept.
    let mut case = MirSwitchCase {
        value: MirConstant::Function(MirFunctionId(9)),
        target: MirBlockId(0),
    };
    remap_switch_case_function_ids(&mut case, &remap);
    assert_eq!(case.value, MirConstant::Function(MirFunctionId(9)));

    let mut op = MirOptionOp::Chain {
        base: fn_const(1),
        link: MirOptionChainLink::Call {
            callee: MirCallee::Closure(closure_callee(2, 0, 9)),
            args: vec![fn_const(1)],
        },
    };
    remap_option_function_ids(&mut op, &remap);
    match &op {
        MirOptionOp::Chain {
            base,
            link: MirOptionChainLink::Call { callee, args },
        } => {
            assert_eq!(*base, fn_const(21));
            assert_eq!(*args, vec![fn_const(21)]);
            // The closure function follows the remap; its captured operand is a
            // separate value and is not part of this remap.
            assert!(matches!(callee, MirCallee::Closure(c) if c.function == MirFunctionId(22)));
        }
        other => panic!("expected a call chain, got {other:?}"),
    }

    let mut link = MirOptionChainLink::Index(fn_const(2));
    remap_option_chain_link_function_ids(&mut link, &remap);
    assert_eq!(link, MirOptionChainLink::Index(fn_const(22)));

    let mut op = MirOptionOp::Coalesce {
        value: fn_const(1),
        fallback: fn_const(2),
    };
    remap_option_function_ids(&mut op, &remap);
    assert_eq!(
        op,
        MirOptionOp::Coalesce {
            value: fn_const(21),
            fallback: fn_const(22)
        }
    );
}

#[test]
fn reference_scan_collects_option_chain_call_targets() {
    let mut scan = MirReferenceScan::default();
    let op = MirOptionOp::Chain {
        base: fn_const(1),
        link: MirOptionChainLink::Call {
            callee: MirCallee::Closure(closure_callee(2, 5, 3)),
            args: vec![fn_const(4)],
        },
    };
    scan_option_references(&op, &mut scan);
    scan.functions.sort();
    assert_eq!(
        scan.functions,
        vec![
            MirFunctionId(1),
            MirFunctionId(2),
            MirFunctionId(3),
            MirFunctionId(4)
        ]
    );
    assert_eq!(scan.environments, vec![MirClosureEnvironmentId(5)]);

    let mut scan = MirReferenceScan::default();
    let link = MirOptionChainLink::Call {
        callee: MirCallee::Definition {
            source: DefId(77),
            type_args: Vec::new(),
        },
        args: Vec::new(),
    };
    scan_option_chain_references(&link, &mut scan);
    assert_eq!(scan.definitions, vec![DefId(77)]);
    assert!(scan.functions.is_empty());
}

// ── Const data member initializer transplant across analyses ──

fn hir_expr(id: u32, kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind,
        ty: None,
        span: radix_module::lexer::Span::default(),
    }
}

fn transplant(
    expr: &HirExpression,
    source_interner: &Interner,
    entry_interner: &mut Interner,
    rewrites: &HashMap<DefId, DefId>,
) -> Result<HirExpression, String> {
    let source_types = TypeTable::new();
    let mut entry_types = TypeTable::new();
    transplant_expr(
        expr,
        source_interner,
        &source_types,
        entry_interner,
        &mut entry_types,
        &mut HashMap::new(),
        None,
        rewrites,
    )
}

#[test]
fn transplant_reinterns_text_and_keeps_nested_shapes() {
    use radix_module::hir::HirArrayElement;
    let mut source = Interner::new();
    // Fill the source interner so its symbols differ from the entry's.
    let _pad = source.intern("padding");
    let text = source.intern("hello");
    let template = source.intern("tpl");
    let mut entry = Interner::new();
    let none = HashMap::new();

    let string = hir_expr(1, HirExpressionKind::Literal(HirLiteral::String(text)));
    let array = hir_expr(
        2,
        HirExpressionKind::Array(vec![
            HirArrayElement::Expr(string.clone()),
            HirArrayElement::Spread(string.clone()),
            HirArrayElement::Infer(radix_module::lexer::Span::default()),
        ]),
    );
    let out = transplant(&array, &source, &mut entry, &none).expect("array transplants");
    let HirExpressionKind::Array(elements) = &out.kind else {
        panic!("array expected")
    };
    assert_eq!(elements.len(), 3);
    let HirArrayElement::Expr(first) = &elements[0] else {
        panic!("expr element")
    };
    let HirExpressionKind::Literal(HirLiteral::String(symbol)) = &first.kind else {
        panic!("string literal")
    };
    assert_eq!(entry.resolve(*symbol), "hello");
    assert!(matches!(elements[1], HirArrayElement::Spread(_)));
    assert!(matches!(elements[2], HirArrayElement::Infer(_)));

    let tuple = hir_expr(
        3,
        HirExpressionKind::Tuple(vec![string.clone()], Some(Vec::new())),
    );
    let out = transplant(&tuple, &source, &mut entry, &none).expect("tuple transplants");
    assert!(
        matches!(out.kind, HirExpressionKind::Tuple(ref items, Some(ref args))
        if items.len() == 1 && args.is_empty())
    );

    let scriptum = hir_expr(
        4,
        HirExpressionKind::Scriptum(template, vec![string.clone()]),
    );
    let out = transplant(&scriptum, &source, &mut entry, &none).expect("scriptum transplants");
    let HirExpressionKind::Scriptum(symbol, items) = &out.kind else {
        panic!("scriptum")
    };
    assert_eq!(entry.resolve(*symbol), "tpl");
    assert_eq!(items.len(), 1);

    let scribe = hir_expr(
        5,
        HirExpressionKind::Scribe(radix_module::hir::HirScribeKind::Nota, vec![string]),
    );
    let out = transplant(&scribe, &source, &mut entry, &none).expect("scribe transplants");
    assert!(
        matches!(out.kind, HirExpressionKind::Scribe(radix_module::hir::HirScribeKind::Nota, ref items)
        if items.len() == 1)
    );
}

#[test]
fn transplant_carries_static_ascription_with_object_entries() {
    use radix_module::hir::HirObjectKey;
    let mut source = Interner::new();
    let _pad = source.intern("padding");
    let key = source.intern("k");
    let skey = source.intern("sk");
    let source_types = TypeTable::new();
    let target = source_types.primitive(Primitive::Numerus);
    let mut entry = Interner::new();

    let one = hir_expr(1, HirExpressionKind::Literal(HirLiteral::Int(1)));
    let verte = hir_expr(
        9,
        HirExpressionKind::Verte {
            source: Box::new(one.clone()),
            target,
            entries: Some(vec![
                HirObjectField {
                    key: HirObjectKey::Ident(key),
                    value: Some(one.clone()),
                },
                HirObjectField {
                    key: HirObjectKey::String(skey),
                    value: None,
                },
                HirObjectField {
                    key: HirObjectKey::Computed(one.clone()),
                    value: Some(one.clone()),
                },
                HirObjectField {
                    key: HirObjectKey::Spread(one.clone()),
                    value: None,
                },
            ]),
        },
    );
    let out = transplant(&verte, &source, &mut entry, &HashMap::new()).expect("verte transplants");
    let HirExpressionKind::Verte {
        entries: Some(fields),
        ..
    } = &out.kind
    else {
        panic!("verte with entries")
    };
    assert_eq!(fields.len(), 4);
    assert!(matches!(&fields[0].key, HirObjectKey::Ident(s) if entry.resolve(*s) == "k"));
    assert!(matches!(&fields[1].key, HirObjectKey::String(s) if entry.resolve(*s) == "sk"));
    assert!(fields[1].value.is_none());
    assert!(matches!(fields[2].key, HirObjectKey::Computed(_)));
    assert!(matches!(fields[3].key, HirObjectKey::Spread(_)));

    // Without entries the ascription stays a plain typed conversion.
    let bare = hir_expr(
        10,
        HirExpressionKind::Verte {
            source: Box::new(one),
            target,
            entries: None,
        },
    );
    let out = transplant(&bare, &source, &mut entry, &HashMap::new()).expect("bare verte");
    assert!(matches!(
        out.kind,
        HirExpressionKind::Verte { entries: None, .. }
    ));
}

#[test]
fn transplant_carries_runtime_conversion_and_refuses_routed_ones() {
    use radix_module::hir::HirConversioTarget;
    let mut source = Interner::new();
    let _pad = source.intern("padding");
    let hint = source.intern("Hex");
    let slot = source.intern("slot");
    let source_types = TypeTable::new();
    let target = source_types.primitive(Primitive::Numerus);
    let mut entry = Interner::new();
    let one = hir_expr(1, HirExpressionKind::Literal(HirLiteral::Int(1)));
    let site = radix_module::hir::HirStoreSite {
        kind: radix_module::hir::HirStoreKind::Declaration,
        slot: Some(slot),
        inferred_from: None,
        inferred_text: None,
    };
    let conversio = |target: HirConversioTarget, recovery: Option<Box<HirExpression>>| {
        hir_expr(
            2,
            HirExpressionKind::Conversio {
                route: None,
                source: Box::new(one.clone()),
                target,
                params: vec![hint],
                recovery,
                implicit: Some(site.clone()),
            },
        )
    };

    let typed = conversio(
        HirConversioTarget::Type(target),
        Some(Box::new(one.clone())),
    );
    let out = transplant(&typed, &source, &mut entry, &HashMap::new()).expect("typed conversio");
    let HirExpressionKind::Conversio {
        route,
        params,
        recovery,
        implicit,
        ..
    } = &out.kind
    else {
        panic!("conversio")
    };
    assert!(route.is_none());
    assert_eq!(entry.resolve(params[0]), "Hex");
    assert!(recovery.is_some());
    assert_eq!(
        entry.resolve(implicit.as_ref().and_then(|s| s.slot).expect("slot")),
        "slot"
    );

    let clamped = conversio(HirConversioTarget::Intervallum(Box::new(one.clone())), None);
    let out = transplant(&clamped, &source, &mut entry, &HashMap::new()).expect("clamp conversio");
    assert!(matches!(
        out.kind,
        HirExpressionKind::Conversio {
            target: HirConversioTarget::Intervallum(_),
            recovery: None,
            ..
        }
    ));

    // A routed conversion names definitions of the source analysis.
    let mut routed = typed;
    if let HirExpressionKind::Conversio { route, .. } = &mut routed.kind {
        *route = Some(radix_module::hir::HirConversioRoute::Function {
            def_id: DefId(3),
            failable: false,
        });
    }
    assert!(transplant(&routed, &source, &mut entry, &HashMap::new()).is_err());
}

#[test]
fn transplant_admits_only_linked_const_paths_and_closed_shapes() {
    let source = Interner::new();
    let mut entry = Interner::new();
    let rewrites: HashMap<DefId, DefId> = [(DefId(5), DefId(500))].into();

    let linked = hir_expr(1, HirExpressionKind::Path(DefId(5)));
    let out = transplant(&linked, &source, &mut entry, &rewrites).expect("linked path");
    assert!(matches!(out.kind, HirExpressionKind::Path(DefId(500))));
    assert_eq!(out.id, HirId(1));

    let unlinked = hir_expr(2, HirExpressionKind::Path(DefId(6)));
    assert!(transplant(&unlinked, &source, &mut entry, &rewrites).is_err());

    // A path nested in an operator fails the whole initializer.
    let one = hir_expr(3, HirExpressionKind::Literal(HirLiteral::Int(1)));
    let nested = hir_expr(
        4,
        HirExpressionKind::Unary(radix_module::hir::HirUnOp::Neg, Box::new(unlinked)),
    );
    assert!(transplant(&nested, &source, &mut entry, &rewrites).is_err());

    // Reads of runtime input are not const-value shapes.
    let read = hir_expr(5, HirExpressionKind::ReadLine(true));
    assert!(transplant(&read, &source, &mut entry, &rewrites).is_err());

    let vacua = hir_expr(6, HirExpressionKind::Vacua);
    assert!(transplant(&vacua, &source, &mut entry, &rewrites).is_ok());
    let binary = hir_expr(
        7,
        HirExpressionKind::Binary(
            radix_module::hir::HirBinOp::Add,
            Box::new(one.clone()),
            Box::new(linked),
        ),
    );
    assert!(transplant(&binary, &source, &mut entry, &rewrites).is_ok());
}

#[test]
fn transplant_literal_remaps_every_text_carrying_literal() {
    let mut source = Interner::new();
    let _pad = source.intern("padding");
    let text = source.intern("txt");
    let flags = source.intern("gi");
    let mut entry = Interner::new();
    let name = |literal: HirLiteral, entry: &mut Interner| match transplant_literal(
        &literal, &source, entry,
    ) {
        HirLiteral::Ascii(s) | HirLiteral::Octeti(s) => entry.resolve(s).to_owned(),
        HirLiteral::FloatText { text, value } => format!("{}:{value}", entry.resolve(text)),
        HirLiteral::Regex(p, f) => format!(
            "{}/{}",
            entry.resolve(p),
            f.map(|f| entry.resolve(f).to_owned()).unwrap_or_default()
        ),
        other => format!("{other:?}"),
    };
    assert_eq!(name(HirLiteral::Ascii(text), &mut entry), "txt");
    assert_eq!(name(HirLiteral::Octeti(text), &mut entry), "txt");
    assert_eq!(
        name(HirLiteral::Regex(text, Some(flags)), &mut entry),
        "txt/gi"
    );
    assert_eq!(name(HirLiteral::Regex(text, None), &mut entry), "txt/");
    assert_eq!(
        name(HirLiteral::FloatText { value: 1.5, text }, &mut entry),
        "txt:1.5"
    );
    assert_eq!(
        name(
            HirLiteral::BigInt("123456789012345678901234567890".into()),
            &mut entry
        ),
        format!(
            "{:?}",
            HirLiteral::BigInt("123456789012345678901234567890".into())
        )
    );
    assert!(matches!(
        transplant_literal(&HirLiteral::Bool(true), &source, &mut entry),
        HirLiteral::Bool(true)
    ));
    assert!(matches!(
        transplant_literal(&HirLiteral::Nil, &source, &mut entry),
        HirLiteral::Nil
    ));
}
